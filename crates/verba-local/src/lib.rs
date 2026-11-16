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
mod dz;
mod fold;
mod spell;

pub use dz::{DZ_SEED, DzList};
#[doc(hidden)]
pub use dz::dz_sound_key;
#[doc(hidden)]
pub use spell::SpellParams;

use std::collections::{HashMap, HashSet};
use std::io;
use std::path::Path;
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

/// Words of one language, sorted by comparison key so that all the words starting the same way sit together.
pub struct WordList {
    entries: Vec<Entry>,
    /// Sum of all the counts, to compare lists of different sizes.
    total: f64,
    /// The lookup tables of the spelling correction, built the first time a correction is asked for (see `spell.rs`).
    spell: OnceLock<spell::Index>,
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
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            let (Some(raw), Some(count)) = (parts.next(), parts.next()) else { continue };
            let Ok(count) = count.parse::<u64>() else { continue };
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
        let mut list = Self { entries, total: 1.0, spell: OnceLock::new() };
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
        best.into_iter().map(|(e, score)| (e.word.as_str(), score)).collect()
    }
}

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
    /// Algerian Darija in Latin letters (see `dz.rs`). Off until switched on.
    dz: Option<DzList>,
}

impl LocalEngine {
    pub fn new() -> Self {
        // Darija is the one language that is off until it is switched on: it is not a language every user types
        Self { lists: HashMap::new(), disabled: HashSet::from([Lang::Dz]), ignored: HashSet::new(), dz: None }
    }

    pub fn add(&mut self, lang: Lang, list: WordList) {
        self.lists.insert(lang, list);
    }

    /// Loads `en.txt`, `fr.txt` and `ar.txt` from a folder; a missing file only means that language is off.
    pub fn load_dir(dir: &Path) -> io::Result<Self> {
        let mut engine = Self::new();
        for lang in [Lang::En, Lang::Fr, Lang::Ar] {
            let path = dir.join(format!("{}.txt", lang.code()));
            match std::fs::read_to_string(&path) {
                Ok(text) => engine.add(lang, WordList::parse_for(lang, &text, 50_000)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
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
        if self.dz.is_some() {
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

    /// How many words the list of a language holds after cleaning (0 when it is not loaded).
    pub fn word_count(&self, lang: Lang) -> usize {
        if lang == Lang::Dz {
            return self.dz.as_ref().map_or(0, DzList::len);
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
        if !arabic {
            self.dz_merge(only, before_caret, typed, &langs, max, &mut found);
        }
        found.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut out: Vec<Suggestion> = Vec::new();
        for (word, _, lang) in found {
            let text = if arabic { word } else { match_case(typed, &word) };
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
