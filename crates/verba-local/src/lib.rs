//! The local engine: finishes the word being typed from a frequency list, with no internet.
//!
//! A list is a text file with one `word count` per line (the format of the open FrequencyWords
//! lists). The files are not part of the repository: `scripts/fetch-data.ps1` downloads them.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use verba_core::{Engine, Lang, Suggestion, current_word, is_arabic_word, match_case, strip_arabic_marks};

/// Fewest letters typed before suggestions show.
const MIN_PREFIX: usize = 2;

/// Words of one language, sorted so that all the words starting the same way sit together.
pub struct WordList {
    /// (word in lower case, how often it occurs), sorted by word.
    words: Vec<(String, u64)>,
    /// Sum of all the counts, to compare lists of different sizes.
    total: f64,
}

impl WordList {
    /// Reads `word count` lines. Lines that are not a word (punctuation, numbers) are skipped.
    pub fn parse(text: &str, max_words: usize) -> Self {
        let mut words: Vec<(String, u64)> = Vec::new();
        for line in text.lines().take(max_words * 2) {
            let mut parts = line.split_whitespace();
            let (Some(word), Some(count)) = (parts.next(), parts.next()) else { continue };
            let Ok(count) = count.parse::<u64>() else { continue };
            if !word.chars().any(|c| c.is_alphabetic()) || word.chars().any(|c| c.is_ascii_digit()) {
                continue;
            }
            let key = strip_arabic_marks(&word.to_lowercase());
            if key.is_empty() {
                continue;
            }
            words.push((key, count));
            if words.len() >= max_words {
                break;
            }
        }
        words.sort_by(|a, b| a.0.cmp(&b.0));
        // the same word twice (for example with and without a mark): keep the more frequent one
        words.dedup_by(|b, a| {
            if a.0 == b.0 {
                a.1 = a.1.max(b.1);
                true
            } else {
                false
            }
        });
        let total = words.iter().map(|w| w.1 as f64).sum::<f64>().max(1.0);
        Self { words, total }
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// The most frequent words that start with `prefix` (not the prefix itself), as (word, share of the list).
    fn complete(&self, prefix: &str, limit: usize) -> Vec<(&str, f64)> {
        let start = self.words.partition_point(|w| w.0.as_str() < prefix);
        let mut best: Vec<(&str, u64)> = Vec::new();
        for (word, count) in &self.words[start..] {
            if !word.starts_with(prefix) {
                break;
            }
            if word.len() == prefix.len() {
                continue;
            }
            // keep `best` sorted from the most to the least frequent, at most `limit` long
            let at = best.partition_point(|b| b.1 >= *count);
            if at < limit {
                best.insert(at, (word.as_str(), *count));
                best.truncate(limit);
            }
        }
        best.into_iter().map(|(w, c)| (w, c as f64 / self.total)).collect()
    }
}

/// Word completion for the languages whose lists were loaded.
pub struct LocalEngine {
    lists: HashMap<Lang, WordList>,
}

impl LocalEngine {
    pub fn new() -> Self {
        Self { lists: HashMap::new() }
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
                Ok(text) => engine.add(lang, WordList::parse(&text, 50_000)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        Ok(engine)
    }

    /// The languages that have a list.
    pub fn languages(&self) -> Vec<Lang> {
        let mut v: Vec<Lang> = self.lists.keys().copied().collect();
        v.sort_by_key(|l| l.code());
        v
    }
}

impl Default for LocalEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine for LocalEngine {
    fn suggest(&self, before_caret: &str, max: usize) -> Vec<Suggestion> {
        let typed = current_word(before_caret);
        if typed.chars().filter(|c| c.is_alphabetic()).count() < MIN_PREFIX {
            return Vec::new();
        }
        let arabic = is_arabic_word(typed);
        let prefix = strip_arabic_marks(&typed.to_lowercase());

        // an Arabic word is looked up in Arabic; a Latin word in every Latin list (English, French)
        let mut found: Vec<(String, f64, Lang)> = Vec::new();
        for (lang, list) in &self.lists {
            if lang.is_rtl() != arabic {
                continue;
            }
            for (word, share) in list.complete(&prefix, max) {
                found.push((word.to_string(), share, *lang));
            }
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
}
