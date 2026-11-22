//! "My translations": the translations the user wrote by hand for single words, kept in a plain text file next to the personal dictionary.
//!
//! One line per word and direction, in the format of the dictionary itself (`dict.txt`), so a person can read and edit the file:
//!
//! ```text
//! # comments start with #
//! ar>en<TAB>تطبيق<TAB>app; application
//! en>ar<TAB>scroll<TAB>تمرير
//! ```
//!
//! The engine puts these translations in FRONT of the ones the dictionary has for the same word (`LocalEngine::translate_word`). They are for
//! the translation of a selected word; the sentence translator is a neural network and does not read them.
//! The engine knows no folder: the app passes the path in. Writing is atomic (a temporary file, then a rename).

use std::io;
use std::path::Path;

use verba_core::{Lang, is_arabic};

use crate::fold::fold;

/// At most this many entries are kept.
pub const MAX_ENTRIES: usize = 2000;
/// A word has at most this many characters, a translation at most `MAX_TRANSLATION_CHARS`, and there are at most `MAX_TRANSLATIONS` of them.
pub const MAX_WORD_CHARS: usize = 60;
pub const MAX_TRANSLATION_CHARS: usize = 80;
pub const MAX_TRANSLATIONS: usize = 5;

/// One word with the translations the user wrote for it, in one direction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserEntry {
    pub from: Lang,
    pub to: Lang,
    pub word: String,
    pub translations: Vec<String>,
}

impl UserEntry {
    /// What makes two entries the same one (the languages and the word, compared leniently as `add` does): for tools that merge lists.
    pub fn key(&self) -> String {
        format!("{}>{}\t{}", code(self.from).unwrap_or("?"), code(self.to).unwrap_or("?"), fold(&self.word))
    }
}

/// All the entries, in the order they were written.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserTranslations {
    entries: Vec<UserEntry>,
}

fn code(lang: Lang) -> Option<&'static str> {
    match lang {
        Lang::En => Some("en"),
        Lang::Fr => Some("fr"),
        Lang::Ar => Some("ar"),
        Lang::Dz => None,
    }
}

fn lang_of(code: &str) -> Option<Lang> {
    match code {
        "en" => Some(Lang::En),
        "fr" => Some(Lang::Fr),
        "ar" => Some(Lang::Ar),
        _ => None,
    }
}

/// The text without control characters, with every run of white space as one space and none at the ends.
fn tidy(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The words of the translations the user typed: separated by `;`, by the Arabic `،` or by a line break.
fn split_translations(text: &str) -> Vec<String> {
    text.split([';', '\u{060C}', '\n', '\r']).map(tidy).filter(|t| !t.is_empty()).collect()
}

impl UserTranslations {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entries(&self) -> &[UserEntry] {
        &self.entries
    }

    /// Adds a translation, or replaces the ones already written for the same word and direction. `translations` is what the person typed:
    /// one or more translations separated by `;`, `،` or a line break. The error is a sentence for the person.
    pub fn add(&mut self, from: Lang, to: Lang, word: &str, translations: &str) -> Result<(), String> {
        if from == to || code(from).is_none() || code(to).is_none() {
            return Err("Choose two different languages: Arabic, English or French.".to_string());
        }
        let word = tidy(word);
        if word.is_empty() {
            return Err("Type the word.".to_string());
        }
        if word.chars().count() > MAX_WORD_CHARS {
            return Err(format!("The word is too long (at most {MAX_WORD_CHARS} characters)."));
        }
        if !word.chars().any(char::is_alphabetic) {
            return Err("The word needs letters.".to_string());
        }
        if (from == Lang::Ar) != word.chars().any(is_arabic) {
            return Err(if from == Lang::Ar { "This word has no Arabic letters." } else { "This word is written in Arabic letters." }.to_string());
        }
        let list = split_translations(translations);
        if list.is_empty() {
            return Err("Type at least one translation.".to_string());
        }
        if list.len() > MAX_TRANSLATIONS {
            return Err(format!("At most {MAX_TRANSLATIONS} translations of a word."));
        }
        if list.iter().any(|t| t.chars().count() > MAX_TRANSLATION_CHARS) {
            return Err(format!("A translation is too long (at most {MAX_TRANSLATION_CHARS} characters)."));
        }
        // into Arabic at least one translation has Arabic letters; into English or French none has
        if (to == Lang::Ar) != list.iter().any(|t| t.chars().any(is_arabic)) {
            return Err(if to == Lang::Ar { "The translation has no Arabic letters." } else { "The translation is written in Arabic letters." }.to_string());
        }
        let key = fold(&word);
        if let Some(entry) = self.entries.iter_mut().find(|e| e.from == from && e.to == to && fold(&e.word) == key) {
            entry.word = word;
            entry.translations = list;
            return Ok(());
        }
        if self.entries.len() >= MAX_ENTRIES {
            return Err(format!("There are already {MAX_ENTRIES} translations: remove some first."));
        }
        self.entries.push(UserEntry { from, to, word, translations: list });
        Ok(())
    }

    /// Removes the entry at `index`. Returns whether there was one.
    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.entries.len() {
            self.entries.remove(index);
            true
        } else {
            false
        }
    }

    /// The entries as lines of the dictionary format, without a header: what `Dictionary::apply_fixes` reads.
    pub fn to_dictionary_text(&self) -> String {
        let mut out = String::new();
        for e in &self.entries {
            if let (Some(f), Some(t)) = (code(e.from), code(e.to)) {
                out.push_str(&format!("{f}>{t}\t{}\t{}\n", e.word, e.translations.join("; ")));
            }
        }
        out
    }

    /// The text of the file: an explanation, then the entries.
    pub fn render(&self) -> String {
        let mut out = String::from(
            "# Verba: my translations. One line per word: direction (ar>en, en>ar, fr>en...), a TAB, the word, a TAB, its translations separated by ;\n\
             # You can edit this file; Verba reads it again when it changes. They come before the dictionary's translations when a word is translated.\n",
        );
        out.push_str(&self.to_dictionary_text());
        out
    }

    /// Reads the text of the file. A line that is not understood, or that does not pass the rules of `add`, is skipped.
    pub fn parse(text: &str) -> Self {
        let mut me = Self::new();
        for line in text.lines() {
            let line = line.trim_start_matches('\u{feff}');
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            let mut f = line.split('\t');
            let (Some(dir), Some(word), Some(tr)) = (f.next(), f.next(), f.next()) else { continue };
            let Some((a, b)) = dir.trim().split_once('>') else { continue };
            let (Some(from), Some(to)) = (lang_of(a), lang_of(b)) else { continue };
            let _ = me.add(from, to, word, tr);
        }
        me
    }

    /// Reads the file. A file that is not there or cannot be read gives no entries.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    /// Writes the file, atomically.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        crate::personal::write_atomic(path, &self.render())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_translation_is_added_checked_and_replaced() {
        let mut t = UserTranslations::new();
        assert!(t.add(Lang::Ar, Lang::En, "  تطبيق ", "app; application").is_ok());
        assert_eq!(t.entries()[0], UserEntry { from: Lang::Ar, to: Lang::En, word: "تطبيق".into(), translations: vec!["app".into(), "application".into()] });
        // the same word again (written with a vowel mark) replaces it
        assert!(t.add(Lang::Ar, Lang::En, "تَطْبِيق", "app").is_ok());
        assert_eq!(t.len(), 1);
        assert_eq!(t.entries()[0].translations, ["app"]);
        // another direction is another entry; the translations may be separated by `;`, the Arabic comma or a line break
        assert!(t.add(Lang::En, Lang::Ar, "scroll", "تمرير، تصفح\nلفّ").is_ok());
        assert_eq!(t.entries()[1].translations, ["تمرير", "تصفح", "لفّ"]);
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn what_cannot_be_kept_is_refused_with_a_sentence() {
        let mut t = UserTranslations::new();
        assert!(t.add(Lang::Ar, Lang::Ar, "كتاب", "book").is_err(), "the same language twice");
        assert!(t.add(Lang::Ar, Lang::En, "   ", "book").is_err(), "no word");
        assert!(t.add(Lang::Ar, Lang::En, "كتاب", " ; ").is_err(), "no translation");
        assert!(t.add(Lang::Ar, Lang::En, "123", "x").is_err(), "no letters");
        assert!(t.add(Lang::Ar, Lang::En, "book", "كتاب").is_err(), "an Arabic direction for a Latin word");
        assert!(t.add(Lang::En, Lang::Ar, "كتاب", "book").is_err(), "a Latin direction for an Arabic word");
        assert!(t.add(Lang::Ar, Lang::En, "كتاب", "a; b; c; d; e; f").is_err(), "more than five");
        assert!(t.add(Lang::Ar, Lang::En, &"ك".repeat(61), "book").is_err(), "a word that is too long");
        assert!(t.add(Lang::Ar, Lang::En, "كتاب", &"x".repeat(81)).is_err(), "a translation that is too long");
        assert!(t.add(Lang::Ar, Lang::En, "كتاب", "كتاب").is_err(), "an Arabic translation for an English target");
        assert!(t.is_empty());
        for e in [t.add(Lang::Ar, Lang::En, "كتاب", "").unwrap_err(), t.add(Lang::Ar, Lang::En, "book", "x").unwrap_err()] {
            assert!(e.ends_with('.'), "a sentence: {e}");
        }
    }

    #[test]
    fn an_entry_is_removed_by_its_place() {
        let mut t = UserTranslations::new();
        t.add(Lang::Ar, Lang::En, "كتاب", "book").unwrap();
        t.add(Lang::Ar, Lang::En, "بيت", "house").unwrap();
        assert!(t.remove(0));
        assert_eq!(t.entries()[0].word, "بيت");
        assert!(!t.remove(5));
    }

    #[test]
    fn the_file_round_trips_and_skips_what_it_does_not_understand() {
        let mut t = UserTranslations::new();
        t.add(Lang::Ar, Lang::En, "تطبيق", "app; application").unwrap();
        t.add(Lang::En, Lang::Ar, "scroll", "تمرير").unwrap();
        let text = t.render();
        assert!(text.starts_with('#'));
        assert_eq!(UserTranslations::parse(&text), t);
        let messy = "\u{feff}# comment\n\nxx>en\tword\tx\nar>en\tكتاب\n ar>en\tبيت\thouse \nar>en\tbook\tكتاب\nar>en\tكتاب\tbook\n";
        let p = UserTranslations::parse(messy);
        assert_eq!(p.len(), 2, "{:?}", p.entries());
        assert_eq!((p.entries()[0].word.as_str(), p.entries()[0].translations[0].as_str()), ("بيت", "house"));
    }

    #[test]
    fn the_file_is_written_and_read_back() {
        let dir = std::env::temp_dir().join(format!("verba-usertr-{}", std::process::id()));
        let path = dir.join("my-translations.txt");
        let mut t = UserTranslations::new();
        t.add(Lang::Ar, Lang::En, "تطبيق", "app").unwrap();
        t.save(&path).unwrap();
        assert_eq!(UserTranslations::load(&path), t);
        assert!(UserTranslations::load(&dir.join("nope.txt")).is_empty(), "a missing file is no entries");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
