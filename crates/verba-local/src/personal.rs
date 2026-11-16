//! The personal dictionary: the words the user keeps, and (when the app allows it) the words and word pairs the engine
//! learned from what the user wrote.
//!
//! - **Words the user keeps** (`add_word`): never flagged by the spelling correction, and offered as completions.
//! - **Learning** (`learn_text`, off unless the app calls it): a word that is in no dictionary is counted; seen 3 times, it becomes
//!   a personal word. Pairs of words are counted too, to predict the next word. **Only single words and pair counts are
//!   stored: never a sentence, never the order of more than two words.** Numbers, addresses, e-mails, paths, identifiers, hex
//!   strings, acronyms and very short or very long words are not kept at all.
//! - **Files** are plain text a person can read and edit: `personal.txt` (one word per line) and `learned.txt` (counts).
//!   The engine knows no folder: the app passes the paths in. Writing is atomic (temp file, then rename); a missing or damaged
//!   file only gives a smaller dictionary.
//!
//! Words are kept per script (Latin letters or Arabic letters), like the word lists, whatever the language of the keyboard.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::Path;

use verba_core::{Lang, is_arabic, is_arabic_mark, is_arabic_word, match_case, strip_arabic_marks};

use crate::fold::{fold, is_core_arabic};
use crate::nextword::{Tok, scan, tail_words};
use crate::{AR_PARTICLES, LocalEngine, WordList};

/// A word becomes personal when it was seen this many times (and is in no dictionary).
pub const PROMOTE_AT: u32 = 3;
/// At most this many words are tracked (candidates and promoted ones); the least seen, then the oldest, are forgotten first.
pub const MAX_LEARNED_WORDS: usize = 5000;
/// At most this many word pairs are tracked.
pub const MAX_PAIRS: usize = 20_000;
/// A pair is used for prediction once it was seen this many times.
pub const MIN_PAIR_USE: u32 = 2;
/// Words added by hand: a sanity limit for a hand-edited file.
const MAX_ADDED: usize = 20_000;
/// Only this much of the end of a text is read by one `learn_text`.
const MAX_TEXT: usize = 20_000;
const MIN_WORD: usize = 3;
const MAX_WORD: usize = 24;

/// How much a personal word counts when completions are ranked. The unit is the share of the dictionary's total word count
/// (`the` is about 0.03, the 500th word about 0.0002, the 5000th about 0.00002): a word added by hand beats every dictionary word
/// beyond about the 400th, but not a very common one, so a short prefix still gets the common words and a longer prefix gets
/// the user's own word.
const ADDED_SCORE: f64 = 3.0e-4;
/// A learned word counts for a bit less than one added on purpose, and for more the more often it was seen (up to 1.6 times).
const LEARNED_SCORE: f64 = 1.5e-4;

#[derive(Clone, Debug)]
struct Learned {
    /// How the word is written when it is offered (`Snow`, `iPhone`, `positiv`).
    form: String,
    count: u32,
    /// A logical clock (the number of `learn_text` calls so far), not a date.
    last: u64,
    /// A word the spelling correction takes for a typo of another word: counted, but never promoted by learning.
    suspect: bool,
}

#[derive(Clone, Copy, Debug)]
struct Pair {
    count: u32,
    last: u64,
}

/// See the module comment.
#[derive(Default, Debug)]
pub struct PersonalDict {
    /// Words added by hand: comparison key -> form.
    added: BTreeMap<String, String>,
    /// Words that must never be learned (the user removed them).
    blocked: BTreeSet<String>,
    /// Words seen by `learn_text`: comparison key -> counts. Promoted when `count >= PROMOTE_AT`.
    learned: BTreeMap<String, Learned>,
    /// Pairs of words seen by `learn_text`: (first, second) -> counts. Lower case.
    pairs: BTreeMap<(String, String), Pair>,
    clock: u64,
    words_dirty: bool,
    learned_dirty: bool,
}

/// The comparison key of a word: lower case; for Arabic also without short vowels and with hamza and dots folded.
fn key_of(word: &str) -> String {
    if is_arabic_word(word) { fold(word) } else { word.to_lowercase() }
}

/// A word the user may keep: 2 to 40 letters of one script, with an apostrophe or hyphen inside. Returns (key, form).
fn hand_word(word: &str) -> Option<(String, String)> {
    let word = word.trim().replace('’', "'");
    let arabic = is_arabic_word(&word);
    let form: String = if arabic { strip_arabic_marks(&word) } else { word };
    let chars: Vec<char> = form.chars().collect();
    let n = chars.len();
    if !(2..=40).contains(&n) {
        return None;
    }
    let letter = |c: char| if arabic { is_arabic(c) && c.is_alphabetic() } else { c.is_alphabetic() && !is_arabic(c) };
    for (i, c) in chars.iter().enumerate() {
        let inner = i > 0 && i + 1 < n && matches!(c, '\'' | '-');
        if !(letter(*c) || inner) {
            return None;
        }
    }
    Some((key_of(&form), form))
}

/// A token as the learning sees it: lower case, no short vowels, 1 to 24 letters of one script, at most one inner apostrophe.
fn learn_token(raw: &str) -> Option<String> {
    let raw = raw.replace('’', "'");
    let chars: Vec<char> = raw.chars().filter(|c| !is_arabic_mark(*c)).collect();
    let n = chars.len();
    if !(1..=MAX_WORD).contains(&n) {
        return None;
    }
    if chars.iter().any(|c| is_arabic(*c)) {
        return chars.iter().all(|c| is_core_arabic(*c)).then(|| chars.into_iter().collect());
    }
    let apostrophes = chars.iter().filter(|c| **c == '\'').count();
    if apostrophes > 1 || chars[0] == '\'' || chars[n - 1] == '\'' {
        return None;
    }
    // Latin letters with their accents (up to Latin Extended-B); other scripts are not learned
    if !chars.iter().all(|c| *c == '\'' || (c.is_alphabetic() && (*c as u32) < 0x250)) {
        return None;
    }
    Some(chars.into_iter().flat_map(char::to_lowercase).collect())
}

/// Whether a word seen in the text may become a personal word (the shape only: dictionaries are asked separately).
fn plausible_word(key: &str, surface: &str) -> bool {
    let chars: Vec<char> = key.chars().collect();
    let n = chars.len();
    if !(MIN_WORD..=MAX_WORD).contains(&n) {
        return false;
    }
    // a stretched sound (`aaah`)
    if chars.windows(3).any(|w| w[0] == w[1] && w[1] == w[2]) {
        return false;
    }
    // a hex string (`deadbeef`, `cafebabe`)
    if n >= 6 && chars.iter().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let letters: Vec<char> = surface.chars().filter(|c| c.is_alphabetic()).collect();
    // an acronym or shouting (`NASA`, `OK`)
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return false;
    }
    // camelCase code (`getUserName`): two or more switches from lower to upper case
    let switches = letters.windows(2).filter(|w| w[0].is_lowercase() && w[1].is_uppercase()).count();
    switches < 2
}

/// A word with a capital after its first letter (`iPhone`, `McDonald`).
fn has_inner_capital(s: &str) -> bool {
    s.chars().skip(1).any(char::is_uppercase)
}

impl PersonalDict {
    pub fn new() -> Self {
        Self::default()
    }

    // ───────────── words the user keeps ─────────────

    /// Keeps a word: it is never flagged as a typo and is offered as a completion. A single word of 2 to 40 letters of one script
    /// (an apostrophe or hyphen inside is fine). Returns `true` when it was added now; `false` when it was already there or is not a word.
    /// The capitals are kept as given (`Snow`, `iPhone`).
    pub fn add_word(&mut self, word: &str) -> bool {
        let Some((key, form)) = hand_word(word) else { return false };
        if self.added.len() >= MAX_ADDED && !self.added.contains_key(&key) {
            return false;
        }
        if self.blocked.remove(&key) {
            self.words_dirty = true;
        }
        let was_there = self.contains(&key);
        if self.added.get(&key) != Some(&form) {
            self.added.insert(key, form);
            self.words_dirty = true;
        }
        !was_there
    }

    /// Forgets a word: the one added by hand and the one learned (which then is never learned again). Returns whether it was there.
    pub fn remove_word(&mut self, word: &str) -> bool {
        let key = key_of(word.trim());
        let mut removed = self.added.remove(&key).is_some();
        if self.learned.remove(&key).is_some_and(|l| l.count >= PROMOTE_AT) {
            removed = true;
        }
        if removed {
            self.blocked.insert(key);
            self.words_dirty = true;
            self.learned_dirty = true;
        }
        removed
    }

    /// Whether the word is a personal word (added, or learned and promoted).
    pub fn contains(&self, word: &str) -> bool {
        let key = key_of(word.trim());
        self.added.contains_key(&key) || self.learned.get(&key).is_some_and(|l| l.count >= PROMOTE_AT && !l.suspect)
    }

    /// Every personal word, as it is written, in alphabetical order: the ones added by hand and the ones learned.
    pub fn words(&self) -> Vec<String> {
        let mut all: BTreeMap<&str, &str> = BTreeMap::new();
        for (k, f) in &self.learned {
            if Self::promoted(f) {
                all.insert(k, &f.form);
            }
        }
        for (k, f) in &self.added {
            all.insert(k, f);
        }
        all.into_values().map(str::to_string).collect()
    }

    /// The words added by hand.
    pub fn added_words(&self) -> Vec<String> {
        self.added.values().cloned().collect()
    }

    /// The words learned from the user's text and promoted.
    pub fn learned_words(&self) -> Vec<String> {
        self.learned.iter().filter(|(k, l)| Self::promoted(l) && !self.added.contains_key(*k)).map(|(_, l)| l.form.clone()).collect()
    }

    /// Whether the word was learned (and not added by hand): the interface may show these apart.
    pub fn is_learned(&self, word: &str) -> bool {
        let key = key_of(word.trim());
        !self.added.contains_key(&key) && self.learned.get(&key).is_some_and(Self::promoted)
    }

    /// (words still counted and not yet personal, words learned, pairs of words) held now.
    pub fn learned_sizes(&self) -> (usize, usize, usize) {
        let promoted = self.learned.values().filter(|l| Self::promoted(l)).count();
        (self.learned.len() - promoted, promoted, self.pairs.len())
    }

    /// Forgets everything learned: the counts, the promoted words and the pairs. The words added by hand, and the words the user
    /// removed (so that they are not learned again), stay.
    pub fn clear_learned(&mut self) {
        if self.learned.is_empty() && self.pairs.is_empty() && self.clock == 0 {
            return;
        }
        self.learned.clear();
        self.pairs.clear();
        self.clock = 0;
        self.learned_dirty = true;
    }

    fn promoted(l: &Learned) -> bool {
        l.count >= PROMOTE_AT && !l.suspect
    }

    // ───────────── learning ─────────────

    /// Learns from finished text. `known(word, arabic)` says whether a dictionary that is on holds the word; `suspect(word)` whether the
    /// word looks like a typo (asked once, when a word is about to become personal). Each call counts every word once per occurrence:
    /// give it each piece of text once.
    pub(crate) fn learn(&mut self, text: &str, known: &dyn Fn(&str, bool) -> bool, suspect: &dyn Fn(&str) -> bool) {
        let mut from = text.len().saturating_sub(MAX_TEXT);
        while !text.is_char_boundary(from) {
            from += 1;
        }
        let text = &text[from..];
        self.clock += 1;
        let mut prev: Option<String> = None;
        let mut at_start = true;
        for span in scan(text, true, learn_token) {
            match span.tok {
                Tok::Word(key) => {
                    if let Some(p) = prev.take() {
                        let pair = self.pairs.entry((p, key.clone())).or_insert(Pair { count: 0, last: 0 });
                        pair.count = pair.count.saturating_add(1);
                        pair.last = self.clock;
                        self.learned_dirty = true;
                    }
                    self.see_word(&key, &text[span.start..span.end], at_start, known, suspect);
                    prev = Some(key);
                    at_start = false;
                }
                Tok::Barrier => {
                    prev = None;
                    at_start = false;
                }
                Tok::Break { sentence_end } => {
                    prev = None;
                    at_start = sentence_end;
                }
            }
        }
        self.enforce_caps();
    }

    fn see_word(&mut self, lower: &str, surface: &str, at_start: bool, known: &dyn Fn(&str, bool) -> bool, suspect: &dyn Fn(&str) -> bool) {
        if !plausible_word(lower, surface) {
            return;
        }
        let arabic = is_arabic_word(lower);
        let key = key_of(lower);
        if self.blocked.contains(&key) || self.added.contains_key(&key) || known(lower, arabic) {
            return;
        }
        // how it is written: the capitals typed in the middle of a sentence are the user's; at the start of one they prove nothing
        let surface: String = if arabic { lower.to_string() } else { surface.replace('’', "'") };
        let form = if at_start && !has_inner_capital(&surface) { lower.to_string() } else { surface };
        let clock = self.clock;
        self.learned_dirty = true;
        match self.learned.get_mut(&key) {
            Some(l) => {
                l.last = clock;
                // a capital seen once is kept (a name written in lower case once is still a name)
                if !(form == lower && l.form != lower) {
                    l.form = form;
                }
                if l.suspect {
                    return;
                }
                if l.count + 1 == PROMOTE_AT && suspect(lower) {
                    l.suspect = true;
                    return;
                }
                l.count = l.count.saturating_add(1);
            }
            None => {
                self.learned.insert(key, Learned { form, count: 1, last: clock, suspect: false });
            }
        }
    }

    /// Keeps the tables within their limits: the words seen least, then the oldest, go first (a promoted word is never evicted
    /// before one that is only being counted).
    fn enforce_caps(&mut self) {
        if self.learned.len() > MAX_LEARNED_WORDS {
            let mut order: Vec<(bool, u32, u64, String)> =
                self.learned.iter().map(|(k, l)| (Self::promoted(l), l.count, l.last, k.clone())).collect();
            order.sort();
            let drop = self.learned.len() - MAX_LEARNED_WORDS * 9 / 10;
            for (_, _, _, k) in order.into_iter().take(drop) {
                self.learned.remove(&k);
            }
        }
        if self.pairs.len() > MAX_PAIRS {
            let mut order: Vec<(u32, u64, (String, String))> = self.pairs.iter().map(|(k, p)| (p.count, p.last, k.clone())).collect();
            order.sort();
            let drop = self.pairs.len() - MAX_PAIRS * 8 / 10;
            for (_, _, k) in order.into_iter().take(drop) {
                self.pairs.remove(&k);
            }
        }
    }

    // ───────────── what the engine asks ─────────────

    /// Personal words that start like `typed` (the part of the word typed, any case), with a score in the unit of the word lists' shares.
    /// Words are dressed like what was typed. The word typed itself is not offered.
    pub(crate) fn complete(&self, typed: &str, arabic: bool, limit: usize) -> Vec<(String, f64)> {
        let key = key_of(typed);
        if key.is_empty() || (self.added.is_empty() && self.learned.is_empty()) {
            return Vec::new();
        }
        let mut out: Vec<(String, f64)> = Vec::new();
        for (k, form) in self.added.range(key.clone()..).take_while(|(k, _)| k.starts_with(&key)) {
            if *k != key && is_arabic_word(k) == arabic {
                out.push((dress(typed, form, arabic), ADDED_SCORE));
            }
        }
        for (k, l) in self.learned.range(key.clone()..).take_while(|(k, _)| k.starts_with(&key)) {
            if *k != key && Self::promoted(l) && is_arabic_word(k) == arabic && !self.added.contains_key(k) {
                let boost = (1.0 + 0.2 * (l.count as f64 / PROMOTE_AT as f64).ln()).clamp(1.0, 1.6);
                out.push((dress(typed, &l.form, arabic), LEARNED_SCORE * boost));
            }
        }
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0)));
        out.truncate(limit);
        out
    }

    /// How a personal word is written (`Snow`), when it is one.
    pub(crate) fn form_of(&self, word: &str, _arabic: bool) -> Option<String> {
        let key = key_of(word);
        self.added.get(&key).cloned().or_else(|| self.learned.get(&key).filter(|l| Self::promoted(l)).map(|l| l.form.clone()))
    }

    /// The words that followed the last word of the text in the user's own writing, most often first. Empty unless the text ends
    /// with a word and a space.
    pub(crate) fn next_after(&self, before_caret: &str, arabic: bool, max: usize) -> Vec<String> {
        if self.pairs.is_empty() {
            return Vec::new();
        }
        let tail = tail_words(before_caret, true, learn_token);
        let Some(w2) = tail.last() else { return Vec::new() };
        if is_arabic_word(w2) != arabic {
            return Vec::new();
        }
        let mut found: Vec<(u32, u64, &str)> = self
            .pairs
            .range((w2.clone(), String::new())..)
            .take_while(|((a, _), _)| a == w2)
            .filter(|((_, b), p)| p.count >= MIN_PAIR_USE && !self.blocked.contains(&key_of(b)))
            .map(|((_, b), p)| (p.count, p.last, b.as_str()))
            .collect();
        found.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then_with(|| a.2.cmp(b.2)));
        found.into_iter().take(max).map(|f| f.2.to_string()).collect()
    }

    // ───────────── files ─────────────

    /// Reads the two files. A missing or damaged file gives what could be read (possibly nothing): never an error.
    pub fn load(words_path: &Path, learned_path: &Path) -> Self {
        let mut d = Self::new();
        if let Some(text) = read_lossy(words_path) {
            d.parse_words(&text);
        }
        if let Some(text) = read_lossy(learned_path) {
            d.parse_learned(&text);
        }
        d.enforce_caps();
        d.words_dirty = false;
        d.learned_dirty = false;
        d
    }

    fn parse_words(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim_start_matches('\u{feff}').trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix('-') {
                if let Some((key, _)) = hand_word(rest) {
                    self.blocked.insert(key);
                }
            } else if let Some((key, form)) = hand_word(line) {
                if self.added.len() < MAX_ADDED {
                    self.added.insert(key, form);
                }
            }
        }
    }

    fn parse_learned(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim_start_matches('\u{feff}');
            let f: Vec<&str> = line.split('\t').collect();
            match f.as_slice() {
                ["clock", n] => self.clock = n.trim().parse().unwrap_or(0),
                ["w", key, form, count, last, flag] => {
                    let (Some((k, _)), Ok(count), Ok(last)) = (hand_word(key), count.trim().parse::<u32>(), last.trim().parse::<u64>()) else { continue };
                    let Some((_, form)) = hand_word(form) else { continue };
                    if count > 0 && key_of(&form) == k && k.chars().count() <= MAX_WORD {
                        self.learned.insert(k, Learned { form, count: count.min(1_000_000), last, suspect: flag.trim() == "typo" });
                    }
                }
                ["b", a, b, count, last] => {
                    let (Some(a), Some(b), Ok(count), Ok(last)) = (learn_token(a), learn_token(b), count.trim().parse::<u32>(), last.trim().parse::<u64>()) else { continue };
                    if count > 0 {
                        self.pairs.insert((a, b), Pair { count: count.min(1_000_000), last });
                    }
                }
                _ => {}
            }
        }
        self.clock = self.clock.max(self.learned.values().map(|l| l.last).chain(self.pairs.values().map(|p| p.last)).max().unwrap_or(0));
    }

    /// Writes what changed since the last load or save: `personal.txt` and `learned.txt`, each through a temp file and a rename, so a
    /// crash never leaves half a file. Creates the folder when it is missing.
    pub fn save(&mut self, words_path: &Path, learned_path: &Path) -> io::Result<()> {
        if self.words_dirty {
            let mut out = String::from(
                "# Verba personal dictionary: one word per line. You can edit this file.\n# A line that starts with - is a word that must never be learned automatically.\n",
            );
            for form in self.added.values() {
                out.push_str(form);
                out.push('\n');
            }
            for key in &self.blocked {
                out.push_str("- ");
                out.push_str(key);
                out.push('\n');
            }
            write_atomic(words_path, &out)?;
            self.words_dirty = false;
        }
        if self.learned_dirty {
            let mut out = String::from(
                "# Verba learned words (word, how often, when) and word pairs. No sentences are stored.\n# Delete this file to forget everything that was learned.\n",
            );
            out.push_str(&format!("clock\t{}\n", self.clock));
            for (k, l) in &self.learned {
                out.push_str(&format!("w\t{k}\t{}\t{}\t{}\t{}\n", l.form, l.count, l.last, if l.suspect { "typo" } else { "-" }));
            }
            for ((a, b), p) in &self.pairs {
                out.push_str(&format!("b\t{a}\t{b}\t{}\t{}\n", p.count, p.last));
            }
            write_atomic(learned_path, &out)?;
            self.learned_dirty = false;
        }
        Ok(())
    }

    /// True when something changed since the last load or save.
    pub fn is_dirty(&self) -> bool {
        self.words_dirty || self.learned_dirty
    }
}

fn read_lossy(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Writes next to the file, then renames over it.
fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = std::path::PathBuf::from(tmp);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()?;
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

/// A personal word dressed like what was typed: a word with capitals of its own (`Snow`, `iPhone`) keeps them, unless the typing is all capitals.
fn dress(typed: &str, form: &str, arabic: bool) -> String {
    if arabic {
        return form.to_string();
    }
    let typed_letters: Vec<char> = typed.chars().filter(|c| c.is_alphabetic()).collect();
    let shouting = typed_letters.len() > 1 && typed_letters.iter().all(|c| c.is_uppercase());
    if shouting {
        form.to_uppercase()
    } else if form.chars().any(char::is_uppercase) {
        form.to_string()
    } else {
        match_case(typed, form)
    }
}

// ───────────── the engine ─────────────

impl LocalEngine {
    /// The personal dictionary (words kept, words learned). Read-only.
    pub fn personal(&self) -> &PersonalDict {
        &self.personal
    }

    /// The personal dictionary, to add and remove words, load and save it.
    pub fn personal_mut(&mut self) -> &mut PersonalDict {
        &mut self.personal
    }

    /// Replaces the personal dictionary (for example with `PersonalDict::load(..)`).
    pub fn set_personal(&mut self, dict: PersonalDict) {
        self.personal = dict;
    }

    /// Learns from finished text: counts its words and pairs of words (see the module comment for what is kept and what never is).
    /// The app decides when to call it: never for a password field or an app that is excluded; never with text that was not typed
    /// by the user. A word that is in no dictionary that is on and was seen 3 times becomes a personal word, unless the spelling
    /// correction takes it for a typo. Give each piece of text once.
    pub fn learn_text(&mut self, text: &str) {
        // the dictionary is taken out while the engine (which the checks below read) is borrowed
        let mut dict = std::mem::take(&mut self.personal);
        dict.learn(text, &|word, arabic| self.knows_word(word, arabic), &|word| self.correct(None, &format!("{word} "), 1).is_some());
        self.personal = dict;
    }

    /// Whether a dictionary that is on holds the word (lower case): exactly for Latin words; for Arabic also with a particle attached.
    fn knows_word(&self, word: &str, arabic: bool) -> bool {
        if self.ignored.contains(word) {
            return true;
        }
        self.lists.iter().filter(|(lang, _)| lang.is_rtl() == arabic && self.is_enabled(**lang)).any(|(_, list)| {
            if !arabic {
                return list.count_of(word) > 0;
            }
            let key = fold(word);
            let held = |k: &str| list.entries.binary_search_by(|e| e.key.as_str().cmp(k)).is_ok();
            held(&key)
                || AR_PARTICLES.iter().any(|p| {
                    key.strip_prefix(p).is_some_and(|stem| stem.chars().count() >= MIN_WORD && held(stem))
                })
        })
    }

    /// The user's own words that start like `typed`, ranked in the unit of the lists' shares, each with the language it is offered under
    /// (the one the words before lean to). Empty when no language of the script is on.
    pub(crate) fn personal_completions(
        &self,
        typed: &str,
        arabic: bool,
        limit: usize,
        langs: &[(Lang, &WordList)],
        bias: &[f64],
    ) -> Vec<(String, f64, Lang)> {
        let Some((lang, _)) = langs.iter().zip(bias).max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|(l, _)| *l) else {
            return Vec::new();
        };
        self.personal.complete(typed, arabic, limit).into_iter().map(|(w, s)| (w, s, lang)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_words_are_single_words_of_one_script() {
        assert!(hand_word("Positiv").is_some());
        assert!(hand_word("O'Brien").is_some());
        assert!(hand_word("anti-spam").is_some());
        assert!(hand_word("مرحبا").is_some());
        for bad in ["", "a", "two words", "abc123", "mixéd٣", "-lead", "trail-", "http://x.org", "ab\ncd"] {
            assert!(hand_word(bad).is_none(), "{bad:?}");
        }
        assert!(hand_word("abc\u{627}").is_none(), "two scripts in one word");
    }

    #[test]
    fn the_shape_of_a_learnable_word() {
        assert!(plausible_word("positiv", "Positiv"));
        assert!(plausible_word("iphone", "iPhone"));
        for (k, s) in [("ab", "ab"), ("aaah", "aaah"), ("deadbeef", "deadbeef"), ("nasa", "NASA"), ("getusername", "getUserName"), ("a".repeat(25).as_str(), "x")] {
            assert!(!plausible_word(k, s), "{s}");
        }
    }
}
