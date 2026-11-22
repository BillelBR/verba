//! The local engine: finishes the word being typed from a frequency list, with no internet.
//!
//! A list is a text file with one `word count` per line (the format of the open FrequencyWords
//! lists). The files are not part of the repository: `scripts/fetch-data.ps1` downloads them.
//!
//! What it does beyond "words that start the same way, most frequent first":
//! - cleans the subtitle noise out of the lists (see `clean.rs`);
//! - matches leniently (see `fold.rs`): `tres` finds `très`, Arabic without hamza finds the word with it;
//! - Arabic words with the attached article or particle (`ال`, `و`, `ب`, `ل`, `ك`, `ف`) are completed
//!   through their stem when the list does not hold that exact form;
//! - when English and French are both on, the words before the caret lean the choice towards the right one.
//! - Algerian Darija in Latin letters (`Lang::Dz`, see `dz.rs`) has its own embedded seed list, is off until switched on, and is never
//!   loaded by `load_dir` (use `add_dz_seed` or `load_dz`).

mod clean;
pub mod colloquial;
mod dz;
mod fold;
mod nextword;
mod personal;
pub mod sentence;
pub mod slang;
mod spell;
mod arabizi;
mod translate;
mod usertr;
pub mod wrongkeys;

pub use dz::{DZ_AR_SEED, DZ_SEED, DzList};
#[doc(hidden)]
pub use dz::{dz_ar_key, dz_sound_key};
#[doc(hidden)]
pub use nextword::{BuildParams, NextBuilder, NextParams, word_runs};
pub use nextword::NextTable;
pub use personal::PersonalDict;
pub use translate::{Dictionary, LOOKUP_LIMIT, Translation};
pub use arabizi::{ArabiziIndex, arabic_skeleton, looks_like_arabizi};
pub use usertr::{MAX_ENTRIES as MAX_USER_TRANSLATIONS, UserEntry, UserTranslations};
#[doc(hidden)]
pub use personal::UsageParams;
#[doc(hidden)]
pub use spell::SpellParams;

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use verba_core::{Engine, Lang, Suggestion, current_word, is_arabic_word, match_case, strip_arabic_marks};

use clean::{Special, clean_token, special};
use fold::{fold, starts_leniently};

/// Fewest letters typed before suggestions show.
const MIN_PREFIX: usize = 2;
/// A word found only through the lenient match (other accent, other hamza) counts for this much of an exact match,
/// so exact spellings are kept first unless the lenient word is much more frequent.
const LENIENT_PENALTY: f64 = 0.4;
/// Another spelling of the word just typed (`tres` for `très`, `المدرسه` for `المدرسة`) is offered only when it is at least
/// this many times more frequent than what was typed. Below that, what was typed is probably meant (`علي`, a name,
/// next to `على`, 10 times more frequent) and the spelling is left to the correction feature.
const SAME_WORD_RATIO: f64 = 12.0;
/// A word rebuilt from an Arabic stem and an attached particle counts for this much of a direct match.
const STEM_PENALTY: f64 = 0.5;
/// Longest run of previous words that decides the language of the current one.
const CONTEXT_WORDS: usize = 3;

struct Entry {
    /// The comparison key (see `fold.rs`); the list is sorted by it.
    key: String,
    /// The most frequent spelling among the words that share this key: the right one. The others (`انا` next to `أنا`)
    /// are only ever offered when what was typed matches them exactly.
    canonical: bool,
    /// The word as it is suggested: lower case, no short vowels.
    word: String,
    count: u64,
}

/// How many words of a language's list the suggestions use (the most frequent ones, after the cleaning). English and Arabic have more than
/// French: measured on real text (`examples/realtext_accuracy.rs`, docs/ACCURACY.md), a longer list finds more rare words (Arabic most;
/// English 100 000 and Arabic 150 000 were the sizes of 0.10.5, 150 000 and 300 000 are those of 0.10.7), and the spelling correction does not change, because
/// it keeps its dictionary of 50 000 lines (`WordList::spell_min_count`).
pub const fn list_cap(lang: Lang) -> usize {
    match lang {
        Lang::En => 150_000,
        Lang::Ar => 300_000,
        _ => 50_000,
    }
}

/// The one-word headwords of the common Modern Standard Arabic words written for Verba (`seed/ar-common.txt`; the same file feeds the dictionary).
fn arabic_common_words() -> impl Iterator<Item = &'static str> {
    include_str!("../seed/ar-common.txt")
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split('\t').next())
        .map(str::trim)
        .filter(|w| !w.is_empty() && !w.contains(' '))
}

/// The line of a frequency list at which the words added from a seed get their frequency: those of the commonest 30 000 are not rare words.
const SEED_RANK: usize = 30_000;

/// `text` (a frequency list, most frequent first) with the words of `seed` that it lacks added at the end, each as frequent as the line
/// `SEED_RANK` of the list. A word the list has (with or without short vowels) is left as it is. The frequency is above where the spelling
/// stops (`WordList::spell_min_count`), so a word added here is a known word for the spelling.
fn with_seed(text: &str, seed: impl Iterator<Item = &'static str>) -> String {
    let mut have: HashSet<String> = HashSet::new();
    let (mut lines, mut at_rank, mut last) = (0usize, 0u64, 1u64);
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(word), Some(count)) = (parts.next(), parts.next()) else { continue };
        let Ok(count) = count.parse::<u64>() else { continue };
        lines += 1;
        if lines == SEED_RANK {
            at_rank = count;
        }
        last = count;
        have.insert(strip_arabic_marks(&word.to_lowercase()));
    }
    let count = if at_rank > 0 { at_rank } else { last.max(1) };
    let mut out = String::with_capacity(text.len() + 4096);
    out.push_str(text);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    for word in seed {
        if have.insert(strip_arabic_marks(word)) {
            out.push_str(&format!("{word} {count}\n"));
        }
    }
    out
}

/// Words of one language, sorted by comparison key so that all the words starting the same way sit together.
pub struct WordList {
    entries: Vec<Entry>,
    /// Sum of all the counts, to compare lists of different sizes.
    total: f64,
    /// The lookup tables of the spelling correction, built the first time a correction is asked for (see `spell.rs`).
    spell: OnceLock<spell::Index>,
    /// The spelling correction knows the words that are at least this frequent: the frequency of the 50 000th line of the file the list
    /// was read from (0 for a list of 50 000 lines or less). A longer list serves the suggestions, not the spelling.
    spell_min_count: u64,
}

impl WordList {
    /// Reads `word count` lines and cleans them with the rules that suit any language.
    pub fn parse(text: &str, max_words: usize) -> Self {
        Self::build(None, text, max_words)
    }

    /// Like `parse`, plus the fixes of one language (for English, split contractions such as `don` + `'t` become `don't`).
    pub fn parse_for(lang: Lang, text: &str, max_words: usize) -> Self {
        Self::build(Some(lang), text, max_words)
    }

    fn build(lang: Option<Lang>, text: &str, max_words: usize) -> Self {
        let mut counts: HashMap<String, u64> = HashMap::new();
        let (mut not_total, mut stems_total) = (0u64, 0u64);
        let (mut lines_read, mut spell_min_count) = (0usize, 0u64);
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let (Some(raw), Some(count)) = (parts.next(), parts.next()) else { continue };
            let Ok(count) = count.parse::<u64>() else { continue };
            // the lists are sorted by frequency: the line where the spelling dictionary of old ends gives the frequency it stops at
            lines_read += 1;
            if lines_read == spell::SPELL_WORDS {
                spell_min_count = count;
            }
            let word = match lang.map_or(Special::None, |l| special(l, raw)) {
                Special::Drop => continue,
                Special::NotContraction => {
                    not_total += count;
                    continue;
                }
                Special::Contraction(w) => {
                    stems_total += count;
                    w
                }
                Special::None => raw.to_string(),
            };
            let Some(word) = clean_token(&word) else { continue };
            *counts.entry(word).or_insert(0) += count;
        }
        // The `'t` count covers every negative contraction. What the stems (don, didn, isn...) do not explain is
        // `can't` and `won't`, which have no stem of their own: share it out (an estimate, about 7 to 3).
        if lang == Some(Lang::En) {
            let rest = not_total.saturating_sub(stems_total);
            if rest > 0 {
                *counts.entry("can't".to_string()).or_insert(0) += rest * 7 / 10;
                *counts.entry("won't".to_string()).or_insert(0) += rest * 3 / 10;
            }
        }
        // the most frequent words first, cut to the size asked, then sorted for searching
        let mut all: Vec<(String, u64)> = counts.into_iter().collect();
        all.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        all.truncate(max_words);
        let mut entries: Vec<Entry> =
            all.into_iter().map(|(word, count)| Entry { key: fold(&word), canonical: false, word, count }).collect();
        entries.sort_by(|a, b| a.key.cmp(&b.key).then_with(|| a.word.cmp(&b.word)));
        let mut list = Self { entries, total: 1.0, spell: OnceLock::new(), spell_min_count };
        list.finish();
        list
    }

    /// Marks the canonical spelling of each group and sums the counts. Call it again after entries are removed.
    fn finish(&mut self) {
        // in each group of spellings with the same key, the most frequent one is the canonical one
        let entries = &mut self.entries;
        let mut i = 0;
        while i < entries.len() {
            let end = entries[i..].iter().position(|e| e.key != entries[i].key).map_or(entries.len(), |p| i + p);
            for e in &mut entries[i..end] {
                e.canonical = false;
            }
            if let Some(best) = (i..end).max_by_key(|&j| entries[j].count) {
                entries[best].canonical = true;
            }
            i = end;
        }
        self.total = entries.iter().map(|e| e.count as f64).sum::<f64>().max(1.0);
        // the entries may have changed: the correction tables are rebuilt when they are next needed
        self.spell = OnceLock::new();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every word of the list with how often it occurs (in no particular order). For tools that measure the engine.
    pub fn iter_words(&self) -> impl Iterator<Item = (&str, u64)> + '_ {
        self.entries.iter().map(|e| (e.word.as_str(), e.count))
    }

    /// How often `word` occurs in the list (counting the spellings that differ only by accent or hamza as the same
    /// word), as a share of the whole list. 0 when it is not there.
    fn share_of(&self, word: &str) -> f64 {
        let key = fold(word);
        let start = self.entries.partition_point(|e| e.key < key);
        self.entries[start..].iter().take_while(|e| e.key == key).map(|e| e.count as f64).sum::<f64>() / self.total
    }

    /// True when the list holds this word or a spelling with the same comparison key (another accent, another hamza). For tools that measure the engine.
    #[doc(hidden)]
    pub fn contains_word(&self, word: &str) -> bool {
        self.share_of(word) > 0.0
    }

    /// How often exactly this word occurs (not a variant spelling).
    fn count_of(&self, word: &str) -> u64 {
        let key = fold(word);
        let start = self.entries.partition_point(|e| e.key < key);
        self.entries[start..].iter().take_while(|e| e.key == key).find(|e| e.word == word).map_or(0, |e| e.count)
    }

    /// The best words that start like `typed` (lower case, no short vowels): the exact spelling first, the lenient
    /// matches (an accent or hamza left out) at a discount. Returns (word, score), best first. The word typed itself is
    /// not offered.
    fn complete(&self, typed: &str, limit: usize) -> Vec<(&str, f64)> {
        let key = fold(typed);
        let typed_count = self.count_of(typed);
        let start = self.entries.partition_point(|e| e.key < key);
        let mut best: Vec<(&Entry, f64)> = Vec::with_capacity(limit + 1);
        for e in &self.entries[start..] {
            if !e.key.starts_with(&key) {
                break;
            }
            if e.word == typed {
                continue;
            }
            // Another spelling of the very word that was typed (`علي` for `على`) is a correction, not a completion.
            // Offered only when what was typed is not a word of the list at all (`اكل` -> `أكل`), and then only the right spelling.
            if e.key == key && typed_count > 0 && (e.count as f64) < SAME_WORD_RATIO * typed_count as f64 {
                continue;
            }
            let exact = e.word.starts_with(typed);
            // A lenient match must be the right spelling of its word, and must not contradict an accent that was typed.
            if !exact && !(e.canonical && starts_leniently(&e.word, typed)) {
                continue;
            }
            // Two spellings of one word, one at least 10 times rarer (`الان` next to `الآن`), are one suggestion: the common one.
            // (Spellings that are both common are different words: `كتابة` writing, `كتابه` his book.)
            if let Some(i) = best.iter().position(|b| b.0.key == e.key) {
                let other = best[i].0.count;
                if e.count.saturating_mul(10) < other {
                    continue;
                }
                if other.saturating_mul(10) < e.count {
                    best.remove(i);
                }
            }
            let score = e.count as f64 / self.total * if exact { 1.0 } else { LENIENT_PENALTY };
            // keep `best` sorted from the best score, at most `limit` long
            let at = best.partition_point(|b| b.1 >= score);
            if at < limit {
                best.insert(at, (e, score));
                best.truncate(limit);
            }
        }
        let mut found: Vec<(&str, f64)> = best.into_iter().map(|(e, score)| (e.word.as_str(), score)).collect();
        // The tail of a long list (the words beyond the 50 000 lines the lists used to have) is rare words, names and slips of the corpus.
        // A word of the tail is offered when it is not far less frequent than the best word offered (`bobsledder` for `bobsl`), not to fill
        // a place with `eitaro` after `either`.
        if self.spell_min_count > 0 {
            if let Some(top) = found.first().map(|f| f.1) {
                found.retain(|(word, score)| *score >= top * TAIL_FLOOR || self.count_of(word) >= self.spell_min_count);
            }
        }
        found
    }
}

/// How much less frequent than the best word offered a word of the tail of a long list may be and still be offered (see `complete`).
const TAIL_FLOOR: f64 = 1.0 / 200.0;

/// Arabic words carry the article and one-letter particles attached: `و` and, `ف` so, `ب` with, `ل` for, `ك` like,
/// `ال` the, and their combinations (`وبال`, `لل` = `ل` + `ال`). Longest first.
const AR_PARTICLES: &[&str] = &[
    "وبال", "وكال", "ولل", "وال", "فال", "بال", "كال", "لل", "ول", "وب", "وك", "فل", "فب", "ال", "و", "ف", "ب", "ل", "ك",
];

/// Word completion for the languages whose lists were loaded.
pub struct LocalEngine {
    lists: HashMap<Lang, WordList>,
    disabled: HashSet<Lang>,
    /// Words the user dismissed (lower case): the correction leaves them alone for the rest of the session.
    ignored: HashSet<String>,
    /// The words the user keeps and what was learned from their text (see `personal.rs`).
    personal: PersonalDict,
    /// The bundled next-word tables (see `nextword.rs`).
    next_tables: HashMap<Lang, NextTable>,
    /// The bilingual dictionary for word translation (see `translate.rs`). It is read from `dict_path` the first time a word is translated:
    /// it takes 0.9 s and about 140 MB, which a session that never translates a word should not pay.
    dict: OnceLock<Dictionary>,
    dict_path: Option<PathBuf>,
    /// The translations the user wrote (see `usertr.rs`), and the same as a small dictionary that is asked before the big one.
    user_tr: UserTranslations,
    user_dict: Dictionary,
    /// Algerian Darija in Latin letters (see `dz.rs`). Off until switched on.
    dz: Option<DzList>,
    /// The same in Arabic letters. The one `Lang::Dz` switch covers both.
    dz_ar: Option<DzList>,
    /// The Arabic words by their consonants, for turning Arabizi into Arabic letters: built at the first use (see `arabizi.rs`).
    arabizi: OnceLock<ArabiziIndex>,
}

impl LocalEngine {
    pub fn new() -> Self {
        // Darija is the one language that is off until it is switched on: it is not a language every user types
        Self {
            lists: HashMap::new(),
            disabled: HashSet::from([Lang::Dz]),
            ignored: HashSet::new(),
            personal: PersonalDict::new(),
            next_tables: HashMap::new(),
            dict: OnceLock::new(),
            dict_path: None,
            user_tr: UserTranslations::new(),
            user_dict: Dictionary::default(),
            dz: None,
            dz_ar: None,
            arabizi: OnceLock::new(),
        }
    }

    pub fn add(&mut self, lang: Lang, list: WordList) {
        self.lists.insert(lang, list);
    }

    /// Loads `en.txt`, `fr.txt` and `ar.txt` from a folder; a missing file only means that language is off.
    /// Only the three word lists of a folder, each cut to its `max_words` most frequent words (after the cleaning), with the words that
    /// belong to the other language taken out: no next-word table, no dictionary. For measuring what the size of a list does
    /// (`examples/realtext_accuracy.rs`); the program itself uses `load_dir`.
    pub fn load_lists(dir: &Path, max_words: usize) -> io::Result<Self> {
        Self::load_lists_seeded(dir, max_words, false)
    }

    /// `load_lists`, and with `seeded` the Arabic list also gets the common words written for Verba that it lacks, as `load_dir` does.
    pub fn load_lists_seeded(dir: &Path, max_words: usize, seeded: bool) -> io::Result<Self> {
        let mut engine = Self::new();
        for lang in [Lang::En, Lang::Fr, Lang::Ar] {
            match std::fs::read_to_string(dir.join(format!("{}.txt", lang.code()))) {
                Ok(text) => {
                    let text = if seeded && lang == Lang::Ar { with_seed(&text, arabic_common_words()) } else { text };
                    engine.add(lang, WordList::parse_for(lang, &text, max_words))
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        engine.remove_bleed();
        Ok(engine)
    }

    pub fn load_dir(dir: &Path) -> io::Result<Self> {
        let mut engine = Self::new();
        for lang in [Lang::En, Lang::Fr, Lang::Ar] {
            let path = dir.join(format!("{}.txt", lang.code()));
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    // the Arabic list also gets the common words written for Verba that it lacks
                    let text = if lang == Lang::Ar { with_seed(&text, arabic_common_words()) } else { text };
                    engine.add(lang, WordList::parse_for(lang, &text, list_cap(lang)))
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        engine.load_next_words_dir(dir);
        // the bilingual dictionary, when it was downloaded: read at the first translation (a missing or unreadable file only means no translation)
        let dict_file = dir.join("dict.txt");
        if dict_file.is_file() {
            engine.dict_path = Some(dict_file);
        }
        engine.remove_bleed();
        Ok(engine)
    }

    /// Subtitle lists hold words of the other language (`the`, `really` in the French list, `tuer` in the English one).
    /// A word is dropped from a list when it is at least 15 times more frequent in the other one: it belongs there.
    /// Words that both languages use (`film`, `chance`) are about as frequent in both and stay in both.
    fn remove_bleed(&mut self) {
        const RATIO: f64 = 15.0;
        let (Some(en), Some(fr)) = (self.lists.get(&Lang::En), self.lists.get(&Lang::Fr)) else { return };
        let foreign = |mine: &WordList, other: &WordList| -> HashSet<String> {
            mine.entries
                .iter()
                .filter(|e| {
                    let theirs = other.count_of(&e.word) as f64 / other.total;
                    theirs > 0.0 && theirs >= RATIO * (e.count as f64 / mine.total)
                })
                .map(|e| e.word.clone())
                .collect()
        };
        let drop_en = foreign(en, fr);
        let drop_fr = foreign(fr, en);
        if let Some(l) = self.lists.get_mut(&Lang::En) {
            l.entries.retain(|e| !drop_en.contains(&e.word));
            l.finish();
        }
        if let Some(l) = self.lists.get_mut(&Lang::Fr) {
            l.entries.retain(|e| !drop_fr.contains(&e.word));
            l.finish();
        }
    }

    /// The languages that have a list (whether they are switched on or not).
    pub fn languages(&self) -> Vec<Lang> {
        let mut v: Vec<Lang> = self.lists.keys().copied().collect();
        if self.dz.is_some() || self.dz_ar.is_some() {
            v.push(Lang::Dz);
        }
        v.sort_by_key(|l| l.code());
        v
    }

    /// The list of a language, as the engine holds it after cleaning. For tools that measure the engine.
    #[doc(hidden)]
    pub fn list(&self, lang: Lang) -> Option<&WordList> {
        self.lists.get(&lang)
    }

    /// Whether the list of a language holds the word (an accent or a hamza left out still counts). For Arabic the Darija words written in
    /// Arabic letters count too. Used to tell text typed on the wrong keyboard layout from text that is fine (see `wrongkeys`).
    pub fn is_word_of(&self, lang: Lang, word: &str) -> bool {
        if self.lists.get(&lang).is_some_and(|l| l.contains_word(word)) {
            return true;
        }
        lang == Lang::Ar && self.dz_ar.as_ref().is_some_and(|d| d.contains(word))
    }

    /// How many words the list of a language holds after cleaning (0 when it is not loaded).
    pub fn word_count(&self, lang: Lang) -> usize {
        if lang == Lang::Dz {
            return self.dz.as_ref().map_or(0, DzList::len) + self.dz_ar.as_ref().map_or(0, DzList::len);
        }
        self.lists.get(&lang).map_or(0, WordList::len)
    }

    /// Switches a language on or off. Every language is on until it is switched off, except Darija (`Lang::Dz`), which is off until it is switched on.
    /// A language that is off never appears in `suggest`; the others fill the places it leaves.
    pub fn set_enabled(&mut self, lang: Lang, on: bool) {
        if on {
            self.disabled.remove(&lang);
        } else {
            self.disabled.insert(lang);
        }
    }

    /// Whether the language is switched on. (A language without a list is on but has nothing to suggest:
    /// see `languages` for what is loaded.)
    pub fn is_enabled(&self, lang: Lang) -> bool {
        !self.disabled.contains(&lang)
    }

    /// The languages that are loaded and switched on.
    pub fn enabled_languages(&self) -> Vec<Lang> {
        self.languages().into_iter().filter(|l| self.is_enabled(*l)).collect()
    }

    /// How much each Latin-script language should count, from the words before the one being typed.
    /// A word that is clearly in one list votes for it; the nearest words weigh most. `before` is the text up to the start of the
    /// current word. Only used when two or more Latin languages are on, so a single word typed alone is never biased.
    fn context_bias(&self, before: &str, langs: &[(Lang, &WordList)]) -> Vec<f64> {
        let mut votes = vec![0.0f64; langs.len()];
        let words: Vec<&str> = before.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty()).collect();
        for (i, w) in words.iter().rev().take(CONTEXT_WORDS).enumerate() {
            let weight = (CONTEXT_WORDS - i) as f64;
            let w = w.to_lowercase();
            // the lists hold no one-letter words, but `i` is as English as a word can be
            if w == "i" {
                for (v, (lang, _)) in votes.iter_mut().zip(langs) {
                    if *lang == Lang::En {
                        *v += weight;
                    }
                }
                continue;
            }
            // each word shares its vote between the languages in proportion to how often it occurs in each
            let shares: Vec<f64> = langs.iter().map(|(_, l)| l.share_of(&w)).collect();
            let sum: f64 = shares.iter().sum();
            if sum <= 0.0 {
                continue;
            }
            for (v, s) in votes.iter_mut().zip(&shares) {
                *v += weight * s / sum;
            }
        }
        // `l'`, `d'`, `j'`, `qu'` before the word is French
        if let Some(stem) = before.strip_suffix('\'') {
            let token: String = stem.chars().rev().take_while(|c| c.is_alphabetic()).collect::<Vec<_>>().into_iter().rev().collect();
            if ["l", "d", "j", "c", "n", "m", "t", "s", "qu", "lorsqu", "puisqu", "jusqu", "quelqu"].contains(&token.to_lowercase().as_str()) {
                for (v, (lang, _)) in votes.iter_mut().zip(langs) {
                    if *lang == Lang::Fr {
                        *v += 5.0;
                    }
                }
            }
        }
        let top = votes.iter().cloned().fold(0.0, f64::max);
        votes
            .iter()
            .map(|v| {
                // only the language ahead gets a push: up to 4 times when it is clearly the one
                let ahead = (v - votes.iter().cloned().filter(|o| o != v).fold(0.0, f64::max)).max(0.0);
                if *v == top { 1.0 + 0.6 * ahead.min(5.0) } else { 1.0 }
            })
            .collect()
    }

    /// Arabic: the stem of a word with an attached particle, completed, then dressed again with what was typed.
    fn arabic_by_stem(&self, list: &WordList, typed: &str, limit: usize) -> Vec<(String, f64)> {
        let folded = fold(typed);
        let mut out: Vec<(String, f64)> = Vec::new();
        let mut tried = 0;
        for particle in AR_PARTICLES {
            if tried == 3 {
                break;
            }
            let len = particle.chars().count();
            if !folded.starts_with(particle) || typed.chars().count() < len + MIN_PREFIX {
                continue;
            }
            tried += 1;
            let head: String = typed.chars().take(len).collect();
            let stem: String = typed.chars().skip(len).collect();
            let with_article = particle.ends_with("ال") || *particle == "لل";
            for (word, score) in list.complete(&stem, limit) {
                // `ال` + a word that already starts with `ال` is not a word
                if with_article && word.starts_with("ال") {
                    continue;
                }
                out.push((format!("{head}{word}"), score * STEM_PENALTY));
            }
        }
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        out
    }
}

impl Default for LocalEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for LocalEngine {
    fn suggest(&self, before_caret: &str, max: usize) -> Vec<Suggestion> {
        self.suggest_filtered(None, before_caret, max)
    }
}

impl LocalEngine {
    /// Suggestions in one language only, for when the keyboard layout says which language is being typed.
    /// A word written in the other script (Arabic letters while the keyboard is French, say) gets nothing.
    pub fn suggest_in(&self, lang: Lang, before_caret: &str, max: usize) -> Vec<Suggestion> {
        self.suggest_filtered(Some(lang), before_caret, max)
    }

    fn suggest_filtered(&self, only: Option<Lang>, before_caret: &str, max: usize) -> Vec<Suggestion> {
        // a word with digit-letters (`sa7`, `3la`) can only be Darija
        if let Some(found) = self.dz_digit_word(only, before_caret, max) {
            return found;
        }
        let typed = current_word(before_caret);
        let arabic = is_arabic_word(typed);
        // short vowels do not count as letters, and an Arabic word is matched without them
        let typed_key = if arabic { strip_arabic_marks(typed) } else { typed.to_lowercase() };
        let letters = if arabic { typed_key.chars().count() } else { typed.chars().filter(|c| c.is_alphabetic()).count() };
        if max == 0 || letters < MIN_PREFIX {
            return Vec::new();
        }

        // an Arabic word is looked up in Arabic; a Latin word in every Latin list that is on
        let mut langs: Vec<(Lang, &WordList)> = Vec::new();
        for lang in [Lang::En, Lang::Fr, Lang::Ar] {
            if lang.is_rtl() == arabic && self.is_enabled(lang) && only.is_none_or(|o| o == lang) {
                if let Some(list) = self.lists.get(&lang) {
                    langs.push((lang, list));
                }
            }
        }
        let bias = if !arabic && langs.len() > 1 {
            self.context_bias(&before_caret[..before_caret.len() - typed.len()], &langs)
        } else {
            vec![1.0; langs.len()]
        };

        let mut found: Vec<(String, f64, Lang)> = Vec::new();
        for ((lang, list), factor) in langs.iter().zip(&bias) {
            let direct = list.complete(&typed_key, max);
            let have = direct.len();
            found.extend(direct.into_iter().map(|(w, s)| (w.to_string(), s * factor, *lang)));
            if arabic && have < max {
                found.extend(self.arabic_by_stem(list, &typed_key, max).into_iter().map(|(w, s)| (w, s, *lang)));
            }
        }
        // the user's own words, already written the way they are to be offered
        let own = self.personal_completions(typed, arabic, max, &langs, &bias);
        let verbatim: HashSet<String> = own.iter().map(|o| o.0.clone()).collect();
        found.extend(own);
        // dictionary words the user writes more often than the dictionary says
        found.extend(self.usage_completions(&typed_key, max, &langs, &bias));
        if !arabic {
            self.dz_merge(only, before_caret, typed, &langs, max, &mut found);
        } else {
            let ar = langs.iter().find(|(l, _)| *l == Lang::Ar).map(|(_, l)| *l);
            self.dz_ar_merge(only, before_caret, typed, &typed_key, ar, max, &mut found);
        }
        found.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        // A word of the tail of a long list (see `WordList::complete`) has to be near the best word of ALL the lists: `genryu` of the English
        // tail is not offered after the French `genre`, though it is the best word of the English list for `ge`.
        if let Some(top) = found.first().map(|f| f.1) {
            found.retain(|(word, score, lang)| {
                let Some((_, list)) = langs.iter().find(|(l, _)| l == lang) else { return true };
                let count = list.count_of(word);
                let in_tail = list.spell_min_count > 0 && count > 0 && count < list.spell_min_count;
                !in_tail || *score >= top * TAIL_FLOOR
            });
        }

        let mut out: Vec<Suggestion> = Vec::new();
        for (word, _, lang) in found {
            let text = if arabic || verbatim.contains(&word) { word } else { match_case(typed, &word) };
            if !out.iter().any(|s| s.text == text) {
                out.push(Suggestion { text, lang });
            }
            if out.len() == max {
                break;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = "the 1000\nhello 500\nhelp 400\nheld 100\nhell 50\nworld 300\n, 9999\n42 800\n";
    const FR: &str = "bonjour 700\nbon 600\nbonsoir 200\nhéros 80\n";
    const AR: &str = "مرحبا 900\nمرحبتين 40\nكتاب 500\nكتابة 300\n";

    fn engine() -> LocalEngine {
        let mut e = LocalEngine::new();
        e.add(Lang::En, WordList::parse(EN, 100));
        e.add(Lang::Fr, WordList::parse(FR, 100));
        e.add(Lang::Ar, WordList::parse(AR, 100));
        e
    }

    fn texts(v: Vec<Suggestion>) -> Vec<String> {
        v.into_iter().map(|s| s.text).collect()
    }

    #[test]
    fn seed_words_that_the_list_lacks_are_added_at_the_end_with_one_frequency() {
        let text = "الكتاب 900
كتاب 500
بيت 300
";
        // one the list has with short vowels, one it lacks (twice in the seed), one it has
        let out = with_seed(text, ["كِتاب", "حاسوب", "حاسوب", "بيت"].into_iter());
        assert_eq!(out.lines().count(), 4, "{out}");
        assert!(out.ends_with("حاسوب 300
"), "as frequent as the last line when the list is short: {out}");
        let list = WordList::parse(&out, 100);
        assert!(list.contains_word("حاسوب") && list.contains_word("كتاب"));
        // a list that does not end with a line break
        assert_eq!(with_seed("بيت 3", ["حاسوب"].into_iter()), "بيت 3
حاسوب 3
");
        // the shipped seed has words to add
        assert!(arabic_common_words().count() > 400);
    }

    #[test]
    fn a_word_of_the_tail_of_one_list_is_not_offered_after_a_much_better_word_of_another() {
        // an English list of more than 50 000 lines ends with a rare word; the French list has a common one that starts the same way
        let english: String = (0..50_000usize).map(|i| format!("{} {}\n", tail_word(i), 1_000_000 - i)).collect::<String>() + "zyxwvu 3\n";
        let mut e = LocalEngine::new();
        e.add(Lang::En, WordList::parse(&english, 100_000));
        e.add(Lang::Fr, WordList::parse("zyxfrench 100000\n", 100_000));
        assert_eq!(texts(e.suggest("zyx", 3)), ["zyxfrench"], "the tail word of the English list is not near the best word");
        // alone it is offered: nothing better exists
        let mut alone = LocalEngine::new();
        alone.add(Lang::En, WordList::parse(&english, 100_000));
        assert_eq!(texts(alone.suggest("zyx", 3)), ["zyxwvu"]);
    }

    /// A different word of six letters for every number (a consonant and a vowel in turn), none of which starts with `zyx`.
    fn tail_word(mut i: usize) -> String {
        let (consonants, vowels) = (b"bcdfghjklmnpqrstvw", b"aeiou");
        let mut word = String::new();
        for position in 0..6 {
            if position % 2 == 0 {
                word.push(consonants[i % 18] as char);
                i /= 18;
            } else {
                word.push(vowels[i % 5] as char);
                i /= 5;
            }
        }
        word
    }

    #[test]
    fn follows_the_keyboard_language_when_asked() {
        let e = engine();
        // the same letters, two keyboards: English words on an English keyboard, French on a French one
        assert_eq!(texts(e.suggest_in(Lang::Fr, "bon", 3)), ["bonjour", "bonsoir"]);
        assert!(!texts(e.suggest_in(Lang::En, "hel", 3)).is_empty());
        assert!(e.suggest_in(Lang::Fr, "hel", 3).is_empty(), "no French word starts with hel in this list");
        // Arabic letters never come from a French keyboard, and the other way round
        assert!(e.suggest_in(Lang::Fr, "مرح", 3).is_empty());
        assert!(e.suggest_in(Lang::Ar, "hel", 3).is_empty());
        assert_eq!(texts(e.suggest_in(Lang::Ar, "مرح", 3)), ["مرحبا", "مرحبتين"]);
    }

    #[test]
    fn a_language_that_is_off_gives_nothing_even_when_the_keyboard_asks_for_it() {
        let mut e = engine();
        e.set_enabled(Lang::Fr, false);
        assert!(e.suggest_in(Lang::Fr, "bon", 3).is_empty());
    }

    #[test]
    fn skips_things_that_are_not_words() {
        assert_eq!(WordList::parse(EN, 100).len(), 6);
    }

    #[test]
    fn finishes_an_english_word_by_frequency() {
        assert_eq!(texts(engine().suggest("say hel", 3)), ["hello", "help", "held"]);
    }

    #[test]
    fn does_not_suggest_what_is_already_typed() {
        assert_eq!(texts(engine().suggest("hello", 5)), Vec::<String>::new());
    }

    #[test]
    fn waits_for_two_letters_and_a_word() {
        assert!(engine().suggest("h", 3).is_empty());
        assert!(engine().suggest("hello ", 3).is_empty());
    }

    #[test]
    fn a_latin_word_looks_in_english_and_french() {
        assert_eq!(texts(engine().suggest("bon", 3)), ["bonjour", "bonsoir"]);
    }

    #[test]
    fn keeps_the_capital_letter() {
        assert_eq!(texts(engine().suggest("Hel", 1)), ["Hello"]);
    }

    #[test]
    fn finishes_an_arabic_word() {
        let got = engine().suggest("قلت مرح", 3);
        assert_eq!(texts(got), ["مرحبا", "مرحبتين"]);
        assert_eq!(texts(engine().suggest("كتا", 2)), ["كتاب", "كتابة"]);
    }

    #[test]
    fn arabic_short_vowels_do_not_stop_a_match() {
        assert_eq!(texts(engine().suggest("مَرْحَ", 1)), ["مرحبا"]);
    }

    #[test]
    fn real_lists_give_sensible_answers() {
        // runs only when the lists were downloaded (scripts/fetch-data.ps1)
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
        let Ok(e) = LocalEngine::load_dir(&dir) else { return };
        if e.languages().len() < 3 {
            return;
        }
        assert!(texts(e.suggest("I would lik", 3)).contains(&"like".to_string()));
        assert!(!e.suggest("bonjo", 3).is_empty());
        assert!(!e.suggest("مرح", 3).is_empty());
    }

    // ───────────── new behaviour ─────────────

    #[test]
    fn a_language_can_be_switched_off() {
        let mut e = engine();
        assert!(e.is_enabled(Lang::Fr));
        e.set_enabled(Lang::Fr, false);
        assert!(!e.is_enabled(Lang::Fr));
        // French never appears; English keeps answering
        assert!(e.suggest("bon", 3).is_empty());
        assert_eq!(texts(e.suggest("hel", 3)), ["hello", "help", "held"]);
        assert_eq!(e.enabled_languages(), vec![Lang::Ar, Lang::En]);
        e.set_enabled(Lang::Fr, true);
        assert_eq!(texts(e.suggest("bon", 3)), ["bonjour", "bonsoir"]);
    }

    #[test]
    fn the_remaining_languages_fill_the_places() {
        let mut e = LocalEngine::new();
        // a common word each, so the shares of the two languages are comparable
        e.add(Lang::En, WordList::parse("the 100000\nbonus 100\nbonfire 50\nbond 40\n", 100));
        e.add(Lang::Fr, WordList::parse("les 100000\nbonjour 900\nbonsoir 800\nbonheur 700\n", 100));
        assert_eq!(texts(e.suggest("bon", 3)), ["bonjour", "bonsoir", "bonheur"]);
        e.set_enabled(Lang::Fr, false);
        assert_eq!(texts(e.suggest("bon", 3)), ["bonus", "bonfire", "bond"]);
        e.set_enabled(Lang::En, false);
        assert!(e.suggest("bon", 3).is_empty());
    }

    #[test]
    fn arabic_off_means_no_arabic() {
        let mut e = engine();
        e.set_enabled(Lang::Ar, false);
        assert!(e.suggest("مرح", 3).is_empty());
    }

    #[test]
    fn english_contractions_are_whole_words() {
        let en = "do 5000\ndon 900\nisn 300\ncan 800\nwon 200\n'll 100\n've 100\n't 2000\nll 50\ndone 700\n";
        let e = WordList::parse_for(Lang::En, en, 100);
        let mut eng = LocalEngine::new();
        eng.add(Lang::En, e);
        // the fragment `don` is gone; `don't` is offered, ranked by the count of `don`
        assert_eq!(texts(eng.suggest("don", 3)), ["don't", "done"]);
        assert_eq!(texts(eng.suggest("isn", 3)), ["isn't"]);
        // 't count (2000) minus the stems (900 + 300) is left for `can't` and `won't`
        assert!(texts(eng.suggest("can", 3)).contains(&"can't".to_string()));
        assert!(texts(eng.suggest("won", 3)).contains(&"won't".to_string()));
        assert!(eng.suggest("ll", 3).is_empty());
    }

    #[test]
    fn noise_is_cleaned_out() {
        let text = "hello 500\nyοu 400\nhelllo 300\nhel-lo 200\n1st 100\nhelmet 90\nhelp's 80\n'hel 70\nuh 60\nãªtre 50\n";
        let l = WordList::parse(text, 100);
        let mut eng = LocalEngine::new();
        eng.add(Lang::En, l);
        // a Greek omicron, a stretched letter, a hyphen, a digit, a leading apostrophe, a noise word and a broken accent are gone
        assert_eq!(texts(eng.suggest("hel", 5)), ["hello", "helmet", "help's"]);
        assert!(eng.suggest("yo", 3).is_empty());
        assert!(eng.suggest("ãª", 3).is_empty());
    }

    #[test]
    fn french_without_accents_finds_the_accented_word() {
        let fr = "très 900\nété 800\ntresse 5\nétait 700\netable 3\nécole 300\n";
        let mut e = LocalEngine::new();
        e.add(Lang::Fr, WordList::parse_for(Lang::Fr, fr, 100));
        // `tres` has an exact match (tresse) but the lenient `très` is far more frequent
        assert_eq!(texts(e.suggest("tres", 2))[0], "très");
        assert!(texts(e.suggest("ete", 3)).contains(&"été".to_string()));
        assert!(texts(e.suggest("eta", 3)).contains(&"était".to_string()) || texts(e.suggest("eta", 3)).contains(&"etable".to_string()));
        // exact accents are kept first when they are about as frequent
        assert_eq!(texts(e.suggest("éc", 2)), ["école"]);
        // the accent typed right: a less common variant of what was typed is not offered
        assert!(!texts(e.suggest("été", 3)).contains(&"ete".to_string()));
    }

    #[test]
    fn french_elision_completes_the_word_after_the_apostrophe() {
        let fr = "ami 900\namis 500\namie 300\nami 100\n";
        let mut e = LocalEngine::new();
        e.add(Lang::Fr, WordList::parse_for(Lang::Fr, fr, 100));
        // "ami" is listed twice (900 + 100): the counts add up
        assert_eq!(texts(e.suggest("c'est l'am", 3)), ["ami", "amis", "amie"]);
        assert_eq!(texts(e.suggest("qu'am", 2)), ["ami", "amis"]);
    }

    #[test]
    fn arabic_alef_variants_taa_marbuta_and_yaa_match() {
        let ar = "أنا 900\nانا 300\nأكل 500\nمدرسة 400\nعلى 700\nعلي 200\nكتابة 100\n";
        let mut e = LocalEngine::new();
        e.add(Lang::Ar, WordList::parse_for(Lang::Ar, ar, 100));
        // without hamza: the list's own spelling is suggested
        // typed without the hamza, the list's spelling is offered (it is the same word, more common that way)
        assert_eq!(texts(e.suggest("اكل", 3)), ["أكل"]);
        assert!(e.suggest("أكل", 3).is_empty());
        assert_eq!(texts(e.suggest("ان", 3))[0], "أنا");
        assert_eq!(texts(e.suggest("مدرس", 2)), ["مدرسة"]);
        assert_eq!(texts(e.suggest("مدرسه", 2)), ["مدرسة"]);
        assert_eq!(texts(e.suggest("عل", 3)), ["على", "علي"]);
        // `علي` is a word of the list (a name): another spelling of it is a correction, not a completion, so nothing is offered
        assert!(e.suggest("علي", 3).is_empty());
        // the right spelling typed: the less common wrong spellings are not offered back
        assert_eq!(texts(e.suggest("أن", 3)), ["أنا"]);
    }

    #[test]
    fn arabic_article_and_particles_reach_the_stem() {
        let ar = "كتاب 500\nكتابة 300\nالكتاب 400\nالكلام 350\nكلام 200\n";
        let mut e = LocalEngine::new();
        e.add(Lang::Ar, WordList::parse_for(Lang::Ar, ar, 100));
        // forms the list holds are found directly
        assert_eq!(texts(e.suggest("الك", 2)), ["الكتاب", "الكلام"]);
        // forms it does not hold are built from the stem and the typed particle
        assert!(texts(e.suggest("بكتا", 3)).contains(&"بكتاب".to_string()));
        assert!(texts(e.suggest("وبالكتا", 3)).is_empty() || texts(e.suggest("وبالكتا", 3))[0].starts_with("وبال"));
        assert!(texts(e.suggest("للكتا", 3)).contains(&"للكتاب".to_string()));
        assert!(texts(e.suggest("والكتا", 3)).contains(&"والكتاب".to_string()));
        // `ال` is not put twice
        assert!(!texts(e.suggest("بالكل", 3)).iter().any(|w| w.contains("الال")));
    }

    #[test]
    fn the_words_before_choose_between_english_and_french() {
        let mut e = LocalEngine::new();
        e.add(Lang::En, WordList::parse_for(Lang::En, "i 9000\nam 6000\nbone 60\nbones 50\nbonus 40\nthe 8000\nis 7000\n", 100));
        e.add(Lang::Fr, WordList::parse_for(Lang::Fr, "je 9000\nsuis 6000\nbonjour 150\nbonsoir 100\nbonheur 90\nle 8000\nest 7000\n", 100));
        // alone, the more frequent word wins
        assert_eq!(e.suggest("bon", 1)[0].lang, Lang::Fr);
        // after English words, English leads; after French words, French leads
        assert_eq!(e.suggest("i am bon", 1)[0].lang, Lang::En);
        assert_eq!(e.suggest("je suis bon", 1)[0].lang, Lang::Fr);
        // an elision is French
        assert_eq!(e.suggest("the l'bon", 1)[0].lang, Lang::Fr);
    }
}
