//! Algerian Darija written in Latin letters ("Arabizi"): `wach`, `kifach`, `bezzaf`, `sa7a`, `3lach`, `9ahwa`.
//!
//! There is no standard spelling and no big free frequency list, so this module works differently from `WordList`:
//! - the data is a SEED list written by the authors (`seed/dz-seed.txt`, embedded in the program), optionally completed by
//!   a downloaded `dz.txt` (see docs/DATA.md);
//! - a word matches what is typed in three tiers: the exact spelling (`bez` -> `bezzaf`), the same sounds spelled
//!   another way (`bezaf`, `kifech` for `kifach`), and the consonant skeleton (`bez` -> `bzaf`); see `soft_key` and `skeleton`;
//! - several spellings of one word are one suggestion, the one that looks like what is typed;
//! - Dz is OFF until `set_enabled(Lang::Dz, true)`. When it is on it is an extra pool next to English and French
//!   (`dz_merge`), the spelling correction never writes a Dz word and never touches one (`dz_knows`).
//!
//! The same list type also holds Darija written in ARABIC letters (`seed/dz-ar-seed.txt`, `DzList::seed_ar`): see `dz_ar_merge`.
//!
//! How Dz is merged with English and French in a Latin context (the keyboard is English or French there):
//! 1. a word with digit-letters (`sa7`, `3la`, `9ah`) can only be Dz: English and French are not asked;
//! 2. otherwise Dz candidates compete with the English and French ones by frequency share, times `DZ_PRIOR` (a small
//!    number: the seed list is small, so its words would otherwise outrank real English words);
//! 3. when the words before are clearly Darija (`wach rani`, `kifach nta`) Dz counts in full (`DZ_IN_CONTEXT`) and
//!    English and French are held back (`OTHER_IN_DZ_CONTEXT`), like the English / French lean already does.

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;

use verba_core::{Lang, Suggestion, current_word, current_word_dz, is_arabic_word, is_dz_digit, match_case, strip_arabic_marks};

use crate::fold::fold;
use crate::{AR_PARTICLES, LocalEngine, MIN_PREFIX, STEM_PENALTY, WordList};

/// The seed list, written by the Verba authors (PolyForm Noncommercial 1.0.0, like the code). One `word weight` pair after another.
pub const DZ_SEED: &str = include_str!("../seed/dz-seed.txt");

/// The seed list of Darija written in Arabic letters, written by the Verba authors (PolyForm Noncommercial 1.0.0).
pub const DZ_AR_SEED: &str = include_str!("../seed/dz-ar-seed.txt");

/// How much a Dz candidate counts next to an English or French one when nothing says the text is Darija.
/// (Measured: see docs/ACCURACY.md. Higher means more Dz suggestions in ordinary English and French.)
const DZ_PRIOR: f64 = 0.01;
/// ...and when the words just before are Darija.
const DZ_IN_CONTEXT: f64 = 1.5;
/// English and French are multiplied by this when the words just before are Darija.
const OTHER_IN_DZ_CONTEXT: f64 = 0.3;
/// A suggestion found through the sounds (another spelling) counts for this much of an exact match,
const TIER_SOUND: f64 = 0.5;
/// ...and one found through the consonants only (`bez` -> `bzaf`) for this much.
const TIER_SKELETON: f64 = 0.15;
/// The last words that decide whether the text is Darija.
const CONTEXT_WORDS: usize = 3;

// ---- Arabic script (see `ar_key`)
/// Like `DZ_PRIOR`, for Darija in Arabic letters next to the Arabic (Modern Standard Arabic) list.
const DZ_AR_PRIOR: f64 = 0.05;
/// When the words just before are Darija: Darija counts this much, and the Arabic list is multiplied by `AR_IN_DZ_CONTEXT`.
const DZ_AR_IN_CONTEXT: f64 = 1.5;
const AR_IN_DZ_CONTEXT: f64 = 0.3;
/// A Darija word the Arabic list also holds counts as Darija in a sentence only when the Arabic list has it this many times less often (share of the list).
const AR_CONTEXT_RATIO: f64 = 0.1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Script {
    Latin,
    Arabic,
}

/// The comparison key of a Darija word: sounds, not letters. `kifach` / `kifech` / `kifesh`, `bezzaf` / `bezaf`,
/// `7bibi` / `hbibi`, `9ahwa` / `qahwa`, `khouya` / `5ouya` / `khoya` have the same key.
/// (A vowel is only one of three kinds: a, i, u. `kh gh ch sh dj ck ph` are one sound each, `ou` before a vowel or at the start is `w`,
/// `2` is dropped, `7` is `h`, `6` is `t`, `q` is `9`, `c` is `k`, doubled letters are one.)
pub(crate) fn soft_key(word: &str) -> String {
    let w: Vec<char> = fold(word).chars().collect();
    // `ou` is a `w` at the start (`ouled`) and before a/e/o (`ouach`), and a vowel `u` anywhere else (`khouya`, `hadouk`, `khouia`)
    let is_wa = |c: Option<&char>| matches!(c, Some('a' | 'e' | 'o'));
    let mut out = String::with_capacity(w.len());
    let push = |c: char, out: &mut String| {
        if out.chars().next_back() != Some(c) {
            out.push(c);
        }
    };
    let mut i = 0;
    while i < w.len() {
        let (c, next) = (w[i], w.get(i + 1));
        let (sound, used) = match (c, next) {
            ('k', Some('h')) => ('5', 2),
            ('g', Some('h')) => ('8', 2),
            ('c', Some('h')) | ('s', Some('h')) => ('$', 2),
            ('d', Some('j')) => ('j', 2),
            ('c', Some('k')) => ('k', 2),
            ('p', Some('h')) => ('f', 2),
            ('o', Some('u')) if i == 0 || is_wa(w.get(i + 2)) => ('w', 2),
            ('o', Some('u')) => ('u', 2),
            ('2', _) => {
                i += 1;
                continue;
            }
            ('a' | 'e', _) => ('a', 1),
            ('i' | 'y', _) => ('i', 1),
            ('o' | 'u', _) => ('u', 1),
            ('7', _) => ('h', 1),
            ('6', _) => ('t', 1),
            ('q', _) => ('9', 1),
            ('c', _) => ('k', 1),
            (c, _) => (c, 1),
        };
        push(sound, &mut out);
        i += used;
    }
    out
}

/// The sound key of a word, for tools that measure the engine: two spellings with the same key are the same word.
#[doc(hidden)]
pub fn dz_sound_key(word: &str) -> String {
    soft_key(word)
}

/// The consonants of the sounds: no vowels and no `3` (the ain is often not typed at all: `3lach` / `lach`).
pub(crate) fn skeleton(soft: &str) -> String {
    soft.chars().filter(|c| !matches!(c, 'a' | 'i' | 'u' | '3')).collect()
}

/// The comparison key of a Darija word in Arabic letters: what a typist may spell differently is one letter.
/// Short vowels, hamza on alef, ta marbuta, alef maqsura (see `fold`), `ڨ گ` = `ق`, `ڤ` = `ف`, `پ` = `ب`, `چ` = `ش`, the interdental letters
/// said as stops (`ث` = `ت`, `ذ` = `د`, `ظ` = `ض`), a lone hamza dropped.
pub(crate) fn ar_key(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    for c in ar_strict_key(word).chars() {
        if c != 'ء' {
            out.push(c);
        }
    }
    out
}

/// The key used to decide that a word IS Darija (so that the spelling correction leaves it alone): the same letters as `ar_key`, but no hamza
/// dropped. Modern Standard Arabic words (`النساء`) must not be taken for Darija words (`نسا`) because the loose key makes them look alike.
pub(crate) fn ar_strict_key(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    for c in fold(&strip_arabic_marks(word)).chars() {
        let c = match c {
            'ڨ' | 'گ' | 'ڭ' => 'ق',
            'ڤ' => 'ف',
            'پ' => 'ب',
            'چ' => 'ش',
            'ک' => 'ك',
            'ی' => 'ي',
            'ث' => 'ت',
            'ذ' => 'د',
            'ظ' => 'ض',
            c => c,
        };
        out.push(c);
    }
    out
}

/// The consonants of an Arabic key: the long-vowel letters `ا و ي` dropped, except at the start (`واش` -> `وش`, `بزاف` -> `بزف`).
pub(crate) fn ar_skeleton(key: &str) -> String {
    key.chars().enumerate().filter(|(i, c)| *i == 0 || !matches!(c, 'ا' | 'و' | 'ي')).map(|(_, c)| c).collect()
}

/// The sound / spelling key of a Darija word in Arabic letters, for tools that measure the engine.
#[doc(hidden)]
pub fn dz_ar_key(word: &str) -> String {
    ar_key(word)
}

/// The key of a word in the script of the list.
fn key_of(script: Script, word: &str) -> String {
    match script {
        Script::Latin => soft_key(word),
        Script::Arabic => ar_key(word),
    }
}

fn skeleton_of(script: Script, key: &str) -> String {
    match script {
        Script::Latin => skeleton(key),
        Script::Arabic => ar_skeleton(key),
    }
}

struct DzEntry {
    word: String,
    count: u64,
    soft: String,
    skel: String,
    /// Index of the group of spellings that have the same `soft` key.
    group: u32,
}

/// The Darija word list: words with weights, grouped by sound.
pub struct DzList {
    script: Script,
    entries: Vec<DzEntry>,
    /// Sum of the weights of the group, by group.
    group_total: Vec<f64>,
    /// Exact word -> entry.
    words: HashMap<String, u32>,
    /// Sound key -> group.
    groups: HashMap<String, u32>,
    /// Group -> its entries.
    members: Vec<Vec<u32>>,
    /// Arabic lists: strict key (see `ar_strict_key`) -> its entries.
    strict: HashMap<String, Vec<u32>>,
    total: f64,
}

/// A Darija word: 2 to 24 characters, `a-z` and the digit-letters, at least one letter, and no two digits in a row.
fn valid_word(w: &str) -> bool {
    let n = w.chars().count();
    (2..=24).contains(&n)
        && w.chars().all(|c| c.is_ascii_lowercase() || is_dz_digit(c))
        && w.chars().any(|c| c.is_ascii_lowercase())
        && !w.as_bytes().windows(2).any(|p| p[0].is_ascii_digit() && p[1].is_ascii_digit())
        && !w.as_bytes().windows(3).any(|p| p[0] == p[1] && p[1] == p[2])
}

/// A Darija word in Arabic letters: 2 to 24 letters of the Arabic alphabet (and `ڨ ڤ پ چ گ`), no letter three times in a row.
fn valid_word_ar(w: &str) -> bool {
    let n = w.chars().count();
    (2..=24).contains(&n)
        && w.chars().all(|c| matches!(c as u32, 0x0621..=0x063A | 0x0641..=0x064A | 0x0671) || "ڨڤپچگ".contains(c))
        && !w.chars().collect::<Vec<_>>().windows(3).any(|p| p[0] == p[1] && p[1] == p[2])
}

impl DzList {
    /// The seed list embedded in the program.
    pub fn seed() -> Self {
        Self::parse(DZ_SEED)
    }

    /// The seed list of Darija written in Arabic letters, embedded in the program.
    pub fn seed_ar() -> Self {
        Self::parse_ar(DZ_AR_SEED)
    }

    /// Like `parse`, for Darija written in Arabic letters. Short vowels are dropped; a word with anything but Arabic letters is skipped.
    pub fn parse_ar(text: &str) -> Self {
        Self::parse_script(Script::Arabic, text)
    }

    /// Reads a list: `#` comments, `word weight` pairs (several to a line), `@N` sets the weight of the words that follow
    /// without one. A line of the usual `word count` format works too. A word seen twice keeps its larger weight.
    /// Anything that is not a Darija word (accents, apostrophes, digits other than 2 3 5 6 7 8 9) is skipped.
    pub fn parse(text: &str) -> Self {
        Self::parse_script(Script::Latin, text)
    }

    fn parse_script(script: Script, text: &str) -> Self {
        let mut counts: HashMap<String, u64> = HashMap::new();
        let mut default = 100u64;
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut tokens = line.split_whitespace().peekable();
            while let Some(t) = tokens.next() {
                if let Some(n) = t.strip_prefix('@') {
                    default = n.parse().unwrap_or(default);
                    continue;
                }
                if t.chars().all(|c| c.is_ascii_digit()) {
                    continue; // a weight with no word before it
                }
                let weight = match tokens.peek().and_then(|n| n.parse::<u64>().ok()) {
                    Some(n) => {
                        tokens.next();
                        n
                    }
                    None => default,
                };
                let word = if script == Script::Arabic { strip_arabic_marks(t) } else { t.to_lowercase() };
                if weight > 0 && if script == Script::Arabic { valid_word_ar(&word) } else { valid_word(&word) } {
                    let c = counts.entry(word).or_insert(0);
                    *c = (*c).max(weight);
                }
            }
        }
        Self::from_counts(script, counts)
    }

    fn from_counts(script: Script, counts: HashMap<String, u64>) -> Self {
        let mut all: Vec<(String, u64)> = counts.into_iter().collect();
        all.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut entries = Vec::with_capacity(all.len());
        let (mut words, mut groups) = (HashMap::new(), HashMap::new());
        let mut group_total: Vec<f64> = Vec::new();
        let mut members: Vec<Vec<u32>> = Vec::new();
        let mut strict: HashMap<String, Vec<u32>> = HashMap::new();
        for (word, count) in all {
            let soft = key_of(script, &word);
            let next = group_total.len() as u32;
            let group = *groups.entry(soft.clone()).or_insert(next);
            if group == next {
                group_total.push(0.0);
                members.push(Vec::new());
            }
            members[group as usize].push(entries.len() as u32);
            if script == Script::Arabic {
                strict.entry(ar_strict_key(&word)).or_default().push(entries.len() as u32);
            }
            group_total[group as usize] += count as f64;
            words.insert(word.clone(), entries.len() as u32);
            entries.push(DzEntry { skel: skeleton_of(script, &soft), soft, word, count, group });
        }
        let total = entries.iter().map(|e| e.count as f64).sum::<f64>().max(1.0);
        Self { script, entries, group_total, words, groups, members, strict, total }
    }

    /// Adds the words of a downloaded list (`dz.txt`) to this one. Its counts are corpus counts, not the seed's estimates, so they
    /// are scaled to carry half the weight of the seed overall; a word in both has its two weights added.
    pub fn merge_extra(&mut self, extra: &DzList) {
        let scale = 0.5 * self.total / extra.total;
        let mut counts: HashMap<String, u64> = self.entries.iter().map(|e| (e.word.clone(), e.count)).collect();
        for e in &extra.entries {
            *counts.entry(e.word.clone()).or_insert(0) += ((e.count as f64 * scale).round() as u64).max(1);
        }
        *self = Self::from_counts(self.script, counts);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every word with its weight (in order of weight). For tools that measure the engine.
    pub fn iter_words(&self) -> impl Iterator<Item = (&str, u64)> + '_ {
        self.entries.iter().map(|e| (e.word.as_str(), e.count))
    }

    /// True when the exact word (lower case) is in the list.
    pub fn contains(&self, word: &str) -> bool {
        self.words.contains_key(word)
    }

    /// True when the word is in the list, spelled another way (same sounds) or with a negation or pronoun attached
    /// (`makhdemtch`, `choufek`). Such a word is Darija and the spelling correction leaves it alone.
    /// `foreign` tells the words that are plain English or French (`normal`, `master`: Darija speakers use them, but they are
    /// not Darija spellings): another spelling of one of those is a typo, not Darija.
    pub fn knows(&self, word: &str, foreign: impl Fn(&str) -> bool) -> bool {
        if self.script == Script::Arabic {
            return self.knows_ar(word, foreign);
        }
        let w = word.to_lowercase();
        if self.words.contains_key(&w) {
            return true;
        }
        // is this (stem of a) word a Darija spelling: listed, or the same sounds as a listed word that is not plain English or French
        let sounds = |s: &str| {
            s.chars().count() >= 3
                && self.groups.get(&soft_key(s)).is_some_and(|g| self.members[*g as usize].iter().any(|i| !foreign(&self.entries[*i as usize].word)))
        };
        if sounds(&w) {
            return true;
        }
        let listed = |s: &str| self.words.contains_key(s) && !foreign(s);
        // `ma` + [n t y] + a word of the list + [ch] + a pronoun (not, I, you, he, him, her, us, them, to me...)
        const NOT_PREFIX: &[&str] = &["", "ma", "me"];
        const PERSON: &[&str] = &["", "n", "t", "y", "ne", "te", "ye"];
        const NOT_SUFFIX: &[&str] = &["", "ch", "che", "sh", "ach", "ouch", "ouche"];
        const PRONOUN: &[&str] = &["", "ou", "o", "ek", "ak", "ik", "ha", "ah", "na", "kom", "koum", "hom", "houm", "ni", "li", "lek", "lou", "lha", "lna"];
        for np in NOT_PREFIX {
            let Some(r1) = w.strip_prefix(np) else { continue };
            for pp in PERSON {
                let Some(r2) = r1.strip_prefix(pp) else { continue };
                for ns in NOT_SUFFIX {
                    let Some(r3) = r2.strip_suffix(ns) else { continue };
                    for pr in PRONOUN {
                        if np.is_empty() && pp.is_empty() && ns.is_empty() && pr.is_empty() {
                            continue;
                        }
                        let Some(stem) = r3.strip_suffix(pr) else { continue };
                        if stem.chars().count() >= 4 && (listed(stem) || sounds(stem)) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Arabic letters: the word, or another spelling of it (same key), or a listed word with a particle, a negation, a verb prefix or a pronoun around it.
    /// `foreign` tells the words the Arabic (Modern Standard Arabic) list holds too: another spelling of one of those is for the Arabic list to judge.
    fn knows_ar(&self, word: &str, foreign: impl Fn(&str) -> bool) -> bool {
        let w = strip_arabic_marks(word);
        if self.words.contains_key(&w) {
            return true;
        }
        let key = ar_strict_key(&w);
        let sounds = |k: &str| {
            k.chars().count() >= 2 && self.strict.get(k).is_some_and(|ids| ids.iter().any(|i| !foreign(&self.entries[*i as usize].word)))
        };
        if sounds(&key) {
            return true;
        }
        // particle (and, so, with, to, like, the) + `ما` (not) + `ن ت ي` (I, you, he) + a word of the list + `ش` (not) + a pronoun, all as keys
        const PARTICLE: &[&str] = &["", "و", "ف", "ب", "ل", "ك", "ال", "وال", "فال", "بال", "كال", "ول", "وب"];
        const NOT_PREFIX: &[&str] = &["", "ما", "م"];
        const PERSON: &[&str] = &["", "ن", "ت", "ي"];
        const NOT_SUFFIX: &[&str] = &["", "ش"];
        const PRONOUN: &[&str] = &["", "ي", "ك", "ه", "و", "ها", "نا", "كم", "هم", "ني", "لي", "لك", "له", "لو", "لها", "لنا", "لهم", "وا", "ت", "تي"];
        for pa in PARTICLE {
            let Some(r0) = key.strip_prefix(pa) else { continue };
            for np in NOT_PREFIX {
                let Some(r1) = r0.strip_prefix(np) else { continue };
                for pp in PERSON {
                    let Some(r2) = r1.strip_prefix(pp) else { continue };
                    for ns in NOT_SUFFIX {
                        let Some(r3) = r2.strip_suffix(ns) else { continue };
                        for pr in PRONOUN {
                            if pa.is_empty() && np.is_empty() && pp.is_empty() && ns.is_empty() && pr.is_empty() {
                                continue;
                            }
                            let Some(stem) = r3.strip_suffix(pr) else { continue };
                            if stem.chars().count() >= 3 && sounds(stem) {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// True when the word, or another spelling with the same strict key, is in the list.
    fn is_listed(&self, word: &str) -> bool {
        self.words.contains_key(word) || self.strict.contains_key(&ar_strict_key(word))
    }

    /// How much of the list this word is (any spelling with the same key), as a share; 0 when it is not there.
    fn share_of(&self, word: &str) -> f64 {
        self.groups.get(&key_of(self.script, word)).map_or(0.0, |g| self.group_total[*g as usize] / self.total)
    }

    /// The best words for what is typed (lower case): (word, score), best first, with the score a share of the list.
    /// `typed` must be 2 characters or more. The word itself, or another spelling of it, is not offered.
    fn complete(&self, typed: &str, limit: usize) -> Vec<(&str, f64)> {
        if typed.chars().count() < MIN_PREFIX {
            return Vec::new();
        }
        let soft = key_of(self.script, typed);
        let skel = skeleton_of(self.script, &soft);
        let first = soft.chars().next();
        // Latin: the typed consonants alone are a spelling of their own (`bzaf`); Arabic: the same letters without the long vowels (`بزف`) are a slip of the full word
        let strictly_longer = self.script == Script::Latin;
        // for each group of spellings: the best tier it matches at, and the commonest spelling at that tier
        let mut best: HashMap<u32, (u8, &DzEntry)> = HashMap::new();
        let mut done: HashSet<u32> = HashSet::new();
        for e in &self.entries {
            if e.word == typed || (e.soft == soft && soft.chars().count() >= 3) {
                done.insert(e.group); // the typed word is complete: no other spelling of it is offered
                continue;
            }
            let tier = if e.word.starts_with(typed) {
                0
            } else if soft.chars().count() >= MIN_PREFIX && e.soft.starts_with(&soft) {
                1
            } else if skel.chars().count() >= 2 && e.soft.chars().next() == first && e.skel.starts_with(&skel) && (!strictly_longer || e.skel.chars().count() > skel.chars().count()) {
                2
            } else {
                continue;
            };
            match best.get(&e.group) {
                Some((t, cur)) if *t < tier || (*t == tier && cur.count >= e.count) => {}
                _ => {
                    best.insert(e.group, (tier, e));
                }
            }
        }
        let mut scored: Vec<(&str, f64)> = best
            .into_iter()
            .filter(|(g, _)| !done.contains(g))
            .map(|(g, (tier, e))| {
                let factor = [1.0, TIER_SOUND, TIER_SKELETON][tier as usize];
                (e.word.as_str(), self.group_total[g as usize] / self.total * factor)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(b.0)));
        scored.truncate(limit);
        scored
    }
}

impl LocalEngine {
    /// Gives the engine the embedded Darija seed lists, in Latin and in Arabic letters. Dz stays switched off until `set_enabled(Lang::Dz, true)`:
    /// the one switch covers both scripts.
    pub fn add_dz_seed(&mut self) {
        self.dz = Some(DzList::seed());
        self.dz_ar = Some(DzList::seed_ar());
    }

    /// The seed lists, plus `dz.txt` (Latin letters) and `dz-ar.txt` (Arabic letters) of the folder when they are there
    /// (a missing file only means the seed alone).
    pub fn load_dz(&mut self, dir: &Path) -> io::Result<()> {
        let mut list = DzList::seed();
        let mut list_ar = DzList::seed_ar();
        match std::fs::read_to_string(dir.join("dz.txt")) {
            Ok(text) => list.merge_extra(&DzList::parse(&text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        match std::fs::read_to_string(dir.join("dz-ar.txt")) {
            Ok(text) => list_ar.merge_extra(&DzList::parse_ar(&text)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        self.dz = Some(list);
        self.dz_ar = Some(list_ar);
        Ok(())
    }

    /// Replaces the Darija list in Arabic letters.
    pub fn set_dz_ar(&mut self, list: DzList) {
        self.dz_ar = Some(list);
    }

    /// The Darija list in Arabic letters. For tools that measure the engine.
    #[doc(hidden)]
    pub fn dz_list_ar(&self) -> Option<&DzList> {
        self.dz_ar.as_ref()
    }

    /// The Arabic-letters Darija list when it is loaded, switched on, and `only` (the keyboard language) allows it:
    /// the Arabic keyboard types it, so it is an extra pool next to the Arabic list; a French or English keyboard does not.
    fn dz_ar_pool(&self, only: Option<Lang>) -> Option<&DzList> {
        if !self.is_enabled(Lang::Dz) || !only.is_none_or(|o| matches!(o, Lang::Ar | Lang::Dz)) {
            return None;
        }
        self.dz_ar.as_ref()
    }

    /// Replaces the Darija list.
    pub fn set_dz(&mut self, list: DzList) {
        self.dz = Some(list);
    }

    /// The Darija list. For tools that measure the engine.
    #[doc(hidden)]
    pub fn dz_list(&self) -> Option<&DzList> {
        self.dz.as_ref()
    }

    /// The Darija list when it is loaded, switched on, and `only` (the keyboard language) allows it:
    /// an English or French keyboard types Arabizi, so Dz is an extra pool next to them; an Arabic keyboard does not.
    fn dz_pool(&self, only: Option<Lang>) -> Option<&DzList> {
        if !self.is_enabled(Lang::Dz) || !only.is_none_or(|o| matches!(o, Lang::En | Lang::Fr | Lang::Dz)) {
            return None;
        }
        self.dz.as_ref()
    }

    /// True when Dz is on and knows this word, in Latin or in Arabic letters: the spelling correction must not touch it.
    pub fn dz_knows(&self, word: &str) -> bool {
        if is_arabic_word(word) {
            return self.dz_ar_pool(None).is_some_and(|l| l.knows(word, |w| self.is_in_arabic_list(w)));
        }
        self.dz_pool(None).is_some_and(|l| l.knows(word, |w| self.is_plain_en_fr(w)))
    }

    /// True when the Arabic (Modern Standard Arabic) list is on and holds this word, whatever its hamza or dots.
    fn is_in_arabic_list(&self, word: &str) -> bool {
        self.is_enabled(Lang::Ar) && self.lists.get(&Lang::Ar).is_some_and(|l| l.share_of(word) > 0.0)
    }

    /// True when the word is spelled the English or French way (a word of a list that is on).
    fn is_plain_en_fr(&self, word: &str) -> bool {
        [Lang::En, Lang::Fr].iter().any(|l| self.is_enabled(*l) && self.lists.get(l).is_some_and(|list| list.count_of(word) > 0))
    }

    /// A word with digit-letters (`sa7`, `3la`) can only be Darija: its suggestions, or `None` when the word has no digit-letter
    /// (or Dz is not in use) and the usual way applies.
    pub(crate) fn dz_digit_word(&self, only: Option<Lang>, before_caret: &str, max: usize) -> Option<Vec<Suggestion>> {
        let list = self.dz_pool(only)?;
        let word = current_word_dz(before_caret);
        if !word.chars().any(is_dz_digit) {
            return None;
        }
        let found = list.complete(&word.to_lowercase(), max);
        Some(found.into_iter().map(|(w, _)| Suggestion { text: match_case(word, w), lang: Lang::Dz }).collect())
    }

    /// Adds the Darija candidates for `typed` to the English and French ones in `found` (see the module comment).
    pub(crate) fn dz_merge(&self, only: Option<Lang>, before_caret: &str, typed: &str, latin: &[(Lang, &WordList)], max: usize, found: &mut Vec<(String, f64, Lang)>) {
        let Some(list) = self.dz_pool(only) else { return };
        let hits = list.complete(&typed.to_lowercase(), max.max(3) * 2);
        if hits.is_empty() {
            return;
        }
        let before = &before_caret[..before_caret.len() - typed.len()];
        let in_context = self.is_darija_context(list, before, latin);
        if in_context {
            for f in found.iter_mut() {
                f.1 *= OTHER_IN_DZ_CONTEXT;
            }
        }
        let factor = if in_context { DZ_IN_CONTEXT } else { DZ_PRIOR };
        // Outside a Darija text, a Darija word is offered only when it beats the best English or French candidate: otherwise it
        // would fill the free places of the popup in the middle of an English sentence (measured: docs/ACCURACY.md).
        let top = if in_context { 0.0 } else { found.iter().map(|f| f.1).fold(0.0, f64::max) };
        found.extend(hits.into_iter().map(|(w, s)| (w.to_string(), s * factor, Lang::Dz)).filter(|(_, s, _)| *s >= top));
    }

    /// Whether the words just before the one being typed are Darija: a word of the Dz list that English and French do not
    /// know votes for it, a word of English or French that Dz does not know votes against; the nearest words weigh most.
    fn is_darija_context(&self, list: &DzList, before: &str, latin: &[(Lang, &WordList)]) -> bool {
        let words: Vec<&str> = {
            let mut v = Vec::new();
            let mut rest = before;
            while v.len() < CONTEXT_WORDS {
                let w = current_word_dz(rest.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '\''));
                let trimmed = rest.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '\'');
                if w.is_empty() || trimmed.is_empty() {
                    break;
                }
                v.push(w);
                rest = &trimmed[..trimmed.len() - w.len()];
            }
            v
        };
        let (mut dz, mut other) = (0.0f64, 0.0f64);
        for (i, w) in words.iter().enumerate() {
            let weight = (CONTEXT_WORDS - i) as f64;
            let w = w.to_lowercase();
            let in_dz = list.contains(&w) || (w.chars().count() >= 4 && list.knows(&w, |x| self.is_plain_en_fr(x)));
            let in_other = latin.iter().any(|(_, l)| l.share_of(&w) > 0.0);
            match (in_dz, in_other) {
                (true, false) => dz += weight,
                (false, true) => other += weight,
                _ => {}
            }
        }
        dz >= 2.0 && dz > other
    }

    /// Darija in Arabic letters: adds the candidates for the Arabic word being typed (`typed_key`: marks stripped) to the Arabic ones in `found`.
    /// The suggestions carry `Lang::Ar`, not `Lang::Dz`: they are written right to left, and the popup lays out text by the language of the suggestion.
    pub(crate) fn dz_ar_merge(&self, only: Option<Lang>, before_caret: &str, typed: &str, typed_key: &str, ar: Option<&WordList>, max: usize, found: &mut Vec<(String, f64, Lang)>) {
        let Some(list) = self.dz_ar_pool(only) else { return };
        let mut hits: Vec<(String, f64)> = list.complete(typed_key, max.max(3) * 2).into_iter().map(|(w, s)| (w.to_string(), s)).collect();
        // an attached article or particle (`وكيف`, `بالدار`): the stem is completed and dressed again, like the Arabic list does
        if hits.len() < max && !list.is_listed(typed_key) {
            let folded = fold(typed_key);
            let mut tried = 0;
            for particle in AR_PARTICLES {
                if tried == 3 {
                    break;
                }
                let len = particle.chars().count();
                if !folded.starts_with(particle) || typed_key.chars().count() < len + MIN_PREFIX {
                    continue;
                }
                tried += 1;
                let head: String = typed_key.chars().take(len).collect();
                let stem: String = typed_key.chars().skip(len).collect();
                let with_article = particle.ends_with("ال") || *particle == "لل";
                for (word, score) in list.complete(&stem, max) {
                    if with_article && word.starts_with("ال") {
                        continue;
                    }
                    hits.push((format!("{head}{word}"), score * STEM_PENALTY));
                }
            }
        }
        if hits.is_empty() {
            return;
        }
        let before = &before_caret[..before_caret.len() - typed.len()];
        let in_context = self.is_darija_context_ar(list, before, ar);
        if in_context {
            for f in found.iter_mut().filter(|f| f.2 == Lang::Ar) {
                f.1 *= AR_IN_DZ_CONTEXT;
            }
        }
        let factor = if in_context { DZ_AR_IN_CONTEXT } else { DZ_AR_PRIOR };
        // outside a Darija text a Darija word is offered only when it beats the best Arabic candidate: it never fills the free places
        let top = if in_context { 0.0 } else { found.iter().map(|f| f.1).fold(0.0, f64::max) };
        found.extend(hits.into_iter().map(|(w, s)| (w, s * factor, Lang::Ar)).filter(|(_, s, _)| *s >= top));
    }

    /// Whether the Arabic words just before the one being typed are Darija: a word of the Darija list that the Arabic list holds much less
    /// often votes for it, a word of the Arabic list that Darija does not know votes against; the nearest words weigh most.
    fn is_darija_context_ar(&self, list: &DzList, before: &str, ar: Option<&WordList>) -> bool {
        let mut words: Vec<String> = Vec::new();
        let mut rest = before;
        while words.len() < CONTEXT_WORDS {
            let trimmed = rest.trim_end_matches(|c: char| !c.is_alphabetic());
            let w = current_word(trimmed);
            if w.is_empty() || trimmed.is_empty() {
                break;
            }
            if !is_arabic_word(w) {
                break;
            }
            words.push(strip_arabic_marks(w));
            rest = &trimmed[..trimmed.len() - w.len()];
        }
        let (mut dz, mut other) = (0.0f64, 0.0f64);
        for (i, w) in words.iter().enumerate() {
            let weight = (CONTEXT_WORDS - i) as f64;
            let ar_share = ar.map_or(0.0, |l| l.share_of(w));
            let in_dz = list.contains(w) || list.knows(w, |x| self.is_in_arabic_list(x));
            let dz_share = list.share_of(w);
            if in_dz && ar_share < dz_share * AR_CONTEXT_RATIO {
                dz += weight;
            } else if !in_dz && ar_share > 0.0 {
                other += weight;
            }
        }
        dz >= 2.0 && dz > other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sk(w: &str) -> String {
        skeleton(&soft_key(w))
    }

    #[test]
    fn spellings_of_one_word_share_a_key() {
        for group in [
            &["kifach", "kifech", "kifesh"][..],
            &["bezzaf", "bezaf", "bezef"],
            &["khouya", "khoya", "5ouya", "khouia"],
            &["7bibi", "hbibi"],
            &["9ahwa", "qahwa"],
            &["wach", "wech", "wesh", "ouach", "wash"],
            &["hamdoullah", "hamdoulah"],
            &["ouled", "wled"],
            &["sa7a", "saha"],
        ] {
            let k = soft_key(group[0]);
            for w in group.iter() {
                assert_eq!(soft_key(w), k, "{w} vs {}", group[0]);
            }
        }
        assert_ne!(soft_key("wach"), soft_key("wahed"));
        assert_ne!(soft_key("3la"), soft_key("la"), "the ain is a letter for the sound key");
    }

    #[test]
    fn arabic_spellings_of_one_word_share_a_key() {
        assert_eq!(ar_key("ڨاع"), ar_key("قاع"));
        assert_eq!(ar_key("يحفظك"), ar_key("يحفضك"));
        assert_eq!(ar_key("مَلِيح"), ar_key("مليح"));
        assert_eq!(ar_key("تلاتة"), ar_key("ثلاثه"));
        assert_ne!(ar_key("واش"), ar_key("اش"));
        // the loose key drops a lone hamza (`نساء` = `نسا`), the strict one does not: Modern Standard words are not taken for Darija words
        assert_eq!(ar_key("نساء"), ar_key("نسا"));
        assert_ne!(ar_strict_key("نساء"), ar_strict_key("نسا"));
        assert_eq!(ar_skeleton(&ar_key("بزاف")), ar_skeleton(&ar_key("بزف")));
        assert_eq!(ar_skeleton(&ar_key("واش")), "وش");
    }

    #[test]
    fn the_skeleton_drops_vowels_and_the_ain() {
        assert_eq!(sk("bezzaf"), sk("bzaf"));
        assert_eq!(sk("3lach"), sk("lach"));
        assert_eq!(sk("3lach"), "l$");
    }

    #[test]
    fn the_seed_is_clean() {
        let mut seen = HashSet::new();
        let mut words = 0;
        for line in DZ_SEED.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
            let toks: Vec<&str> = line.split_whitespace().collect();
            assert!(toks.len() % 2 == 0, "a word without a weight: {line}");
            for pair in toks.chunks(2) {
                assert!(valid_word(pair[0]), "not a Darija word: {:?}", pair[0]);
                assert!(pair[1].parse::<u64>().is_ok_and(|n| n > 0), "bad weight for {:?}", pair[0]);
                assert!(seen.insert(pair[0]), "listed twice: {:?}", pair[0]);
                words += 1;
            }
        }
        assert!(words >= 1500, "only {words} words");
        assert_eq!(DzList::seed().len(), words);
    }

    #[test]
    fn parse_takes_pairs_defaults_and_skips_what_is_not_darija() {
        let l = DzList::parse("# c\n@50\nwach 900 rani ana 700\nbad_word 5 caf\u{e9} 9 a1 4 sa7a 300 sa7a 100 x 8\n");
        let w: Vec<(&str, u64)> = l.iter_words().collect();
        // rani takes the default weight, ana 700; a word seen twice keeps the larger weight
        assert_eq!(w, vec![("wach", 900), ("ana", 700), ("sa7a", 300), ("rani", 50)]);
    }

    #[test]
    fn a_complete_word_or_its_other_spelling_is_not_offered_back() {
        let l = DzList::parse("bezzaf 1000 bzaf 300 bezaf 200 bazar 5");
        assert!(l.complete("bezzaf", 3).is_empty());
        assert!(l.complete("bezaf", 3).iter().all(|(w, _)| *w != "bezzaf" && *w != "bezaf"));
        let top: Vec<&str> = l.complete("bez", 3).into_iter().map(|x| x.0).collect();
        assert_eq!(top[0], "bezzaf");
        assert!(top.contains(&"bzaf"));
        // the spelling that is being typed is the one offered
        assert_eq!(l.complete("beza", 1)[0].0, "bezaf");
    }

    #[test]
    fn known_words_include_other_spellings_and_negations() {
        let l = DzList::parse("kifach 100 khdemt 50 chouf 80 wach 900");
        let none = |_: &str| false;
        assert!(l.knows("kifach", none) && l.knows("kifech", none) && l.knows("KIFESH", none));
        assert!(l.knows("makhdemtch", none) && l.knows("choufek", none) && l.knows("nchouf", none) && l.knows("matchoufouche", none));
        assert!(!l.knows("receive", none) && !l.knows("tomorrow", none) && !l.knows("teh", none));
        // a plain English word of the list: only the exact spelling is Darija, another spelling is a typo
        let l = DzList::parse("master 100 wach 900");
        assert!(l.knows("master", |w| w == "master") && !l.knows("masster", |w| w == "master"));
    }
}
