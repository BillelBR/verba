//! Game and chat abbreviations (`gg`, `gn`, `brb`, `see u`, `afk` ...).
//!
//! A small glossary (`seed/chat-slang.txt`) with what each one means in English, French and Arabic. It is used in two places:
//!
//! * a selected abbreviation is translated from it (word translation), before the dictionary when the line is marked `!` and after it
//!   otherwise;
//! * in a sentence that is translated from English, the abbreviations marked `*` are written out first (`gn guys` becomes
//!   `Good night guys`), because the network has never seen `gn` and translates it as noise. In a sentence translated from Arabic
//!   they are written out in Arabic (`gg يا شباب` becomes `لعبة جيدة يا شباب`), and so are their spellings in Arabic letters
//!   (`جي جي`).
//!
//! Format and flags: see the header of the file.

use std::collections::HashMap;
use std::sync::OnceLock;

use verba_core::Lang;

/// One meaning of one or more spellings.
#[derive(Debug, Default, Clone)]
pub struct SlangEntry {
    /// The meaning in each language, the main one first. `en` is the abbreviation written out.
    pub en: Vec<String>,
    pub fr: Vec<String>,
    pub ar: Vec<String>,
    /// The abbreviation wins over the dictionary's own entry for the same spelling.
    pub priority: bool,
    /// It is written out inside sentences before they are translated.
    pub in_sentences: bool,
}

#[derive(Default)]
pub struct Slang {
    entries: Vec<SlangEntry>,
    keys: HashMap<String, usize>,
}

/// A spelling as a key: lower case, letters and digits only, any other run of characters (spaces, a slash, a full stop) as one space.
/// An apostrophe joins (`what's` -> `whats`). `GG!` -> `gg`, `See  U` -> `see u`, `k/d` -> `k d`.
pub fn slang_key(text: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if gap && !out.is_empty() {
                out.push(' ');
            }
            gap = false;
            out.extend(c.to_lowercase());
        } else if c != '\'' && c != '\u{2019}' {
            gap = true;
        }
    }
    out
}

impl Slang {
    pub fn parse(text: &str) -> Slang {
        let mut slang = Slang::default();
        for line in text.lines() {
            let line = line.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let flags: String = line.chars().take_while(|c| *c == '!' || *c == '*').collect();
            let rest = &line[flags.len()..];
            let fields: Vec<&str> = rest.split('|').map(str::trim).collect();
            if fields.len() < 4 {
                continue;
            }
            let list = |f: &str| -> Vec<String> {
                if f == "-" { vec![] } else { f.split(';').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect() }
            };
            let index = slang.entries.len();
            slang.entries.push(SlangEntry {
                en: list(fields[1]),
                fr: list(fields[2]),
                ar: list(fields[3]),
                priority: flags.contains('!'),
                in_sentences: flags.contains('*'),
            });
            for spelling in fields[0].split(',') {
                let key = slang_key(spelling);
                if !key.is_empty() {
                    slang.keys.entry(key).or_insert(index);
                }
            }
        }
        slang
    }

    /// The entry for a selected word or phrase (`GG`, `see u`), `None` when it is not an abbreviation of the glossary.
    pub fn find(&self, text: &str) -> Option<&SlangEntry> {
        self.keys.get(&slang_key(text)).map(|&i| &self.entries[i])
    }

    /// Writes out the abbreviations marked `*` in a sentence, whole words only: `see u tomorrow` -> `See you tomorrow`.
    /// A one-letter abbreviation (`u`, `r`, `k`) is only taken when it is written in lower case (`U` may be a vitamin).
    /// The first word of the text and a word written with a capital (`Brb`) get a capital in front of the written-out form;
    /// `GG` in the middle of a sentence does not shout.
    pub fn expand(&self, text: &str) -> String {
        self.expand_into(text, Lang::En)
    }

    /// The same, written out in `into`: English (the default) or Arabic (an abbreviation with no Arabic meaning stays as it is).
    pub fn expand_into(&self, text: &str, into: Lang) -> String {
        let arabic = into == Lang::Ar;
        let mut words: Vec<(usize, usize)> = Vec::new();
        let mut start: Option<usize> = None;
        for (i, c) in text.char_indices() {
            let inside = c.is_alphanumeric() || ((c == '\'' || c == '\u{2019}') && start.is_some());
            match (inside, start) {
                (true, None) => start = Some(i),
                (false, Some(s)) => {
                    words.push((s, i));
                    start = None;
                }
                _ => {}
            }
        }
        if let Some(s) = start {
            words.push((s, text.len()));
        }

        let mut out = String::new();
        let mut copied = 0; // up to here, `text` is in `out`
        let mut i = 0;
        while i < words.len() {
            let mut replaced = false;
            for len in (1..=3usize).rev() {
                if i + len > words.len() {
                    continue;
                }
                // the words of a phrase are separated by white space only
                let spaced = (i..i + len - 1).all(|j| {
                    let between = &text[words[j].1..words[j + 1].0];
                    !between.is_empty() && between.chars().all(char::is_whitespace)
                });
                if !spaced {
                    continue;
                }
                let (from, to) = (words[i].0, words[i + len - 1].1);
                let token = &text[from..to];
                let Some(entry) = self.find(token).filter(|e| e.in_sentences) else { continue };
                let Some(written) = (if arabic { entry.ar.first() } else { entry.en.first() }) else { continue };
                if len == 1 && token.chars().count() == 1 && token.chars().any(char::is_uppercase) {
                    continue;
                }
                let at_start = text[..from].chars().all(|c| !c.is_alphanumeric());
                let capital = !arabic && (at_start || (token.chars().next().is_some_and(char::is_uppercase) && token.chars().any(char::is_lowercase)));
                out.push_str(&text[copied..from]);
                if capital {
                    let mut chars = written.chars();
                    if let Some(f) = chars.next() {
                        out.extend(f.to_uppercase());
                        out.push_str(chars.as_str());
                    }
                } else {
                    out.push_str(written);
                }
                copied = to;
                i += len;
                replaced = true;
                break;
            }
            if !replaced {
                i += 1;
            }
        }
        out.push_str(&text[copied..]);
        out
    }
}

/// The glossary that ships with the program.
pub fn shipped() -> &'static Slang {
    static SHIPPED: OnceLock<Slang> = OnceLock::new();
    SHIPPED.get_or_init(|| Slang::parse(include_str!("../seed/chat-slang.txt")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spellings_are_found_whatever_the_case_and_punctuation() {
        let s = shipped();
        for typed in ["gg", "GG", "GG!", "  Gg, ", "ggs", "see u", "See U", "see  u", "CYA", "g2g", "GTG", "k/d", "kd", "Sup?"] {
            assert!(s.find(typed).is_some(), "{typed}");
        }
        assert_eq!(s.find("GG").unwrap().en[0], "good game");
        assert_eq!(s.find("see u").unwrap().en[0], "see you");
        assert!(s.find("gg").unwrap().priority && s.find("gg").unwrap().in_sentences);
        assert!(s.find("hello").is_none() && s.find("").is_none());
        // a game word that is not an abbreviation is explained, but not written into sentences
        let nerf = s.find("nerf").unwrap();
        assert!(!nerf.in_sentences && !nerf.priority && !nerf.ar.is_empty() && !nerf.fr.is_empty());
    }

    #[test]
    fn every_line_of_the_glossary_is_complete() {
        let text = include_str!("../seed/chat-slang.txt");
        let s = Slang::parse(text);
        let lines = text.lines().filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#')).count();
        assert_eq!(s.entries.len(), lines, "a line was not understood");
        for (i, e) in s.entries.iter().enumerate() {
            assert!(!e.en.is_empty(), "entry {i} has no English");
            assert!(!e.fr.is_empty() || e.in_sentences, "entry {i} has no French");
            // an entry that is written into sentences must read as a plain phrase there
            if e.in_sentences {
                assert!(!e.en[0].contains(['(', ')', '/']), "{}", e.en[0]);
            }
        }
        assert!(s.keys.len() > 120, "{} spellings", s.keys.len());
    }

    #[test]
    fn abbreviations_are_written_out_in_a_sentence() {
        let s = shipped();
        assert_eq!(s.expand("gg guys"), "Good game guys");
        assert_eq!(s.expand("that was ez gg"), "that was easy good game");
        assert_eq!(s.expand("see u tomorrow"), "See you tomorrow");
        assert_eq!(s.expand("ok, brb, gotta pee"), "ok, be right back, gotta pee");
        assert_eq!(s.expand("Brb"), "Be right back");
        assert_eq!(s.expand("i love u so much"), "i love you so much");
        assert_eq!(s.expand("thx 4 the carry, gn!"), "Thanks 4 the carry, good night!");
        // GG in the middle does not shout
        assert_eq!(s.expand("well, GG everyone"), "well, good game everyone");
    }

    #[test]
    fn abbreviations_are_written_out_in_arabic_for_a_sentence_that_goes_to_english() {
        let s = shipped();
        assert_eq!(s.expand_into("gg يا شباب", Lang::Ar), "لعبة جيدة يا شباب");
        assert_eq!(s.expand_into("لحظة brb", Lang::Ar), "لحظة سأعود حالا");
        assert_eq!(s.expand_into("انا afk الآن", Lang::Ar), "انا بعيد عن لوحة المفاتيح الآن");
        // the spellings in Arabic letters
        assert_eq!(s.expand_into("شكرا يا صديقي، جي جي", Lang::Ar), "شكرا يا صديقي، لعبة جيدة");
        // no capital letters in Arabic, and an abbreviation with no Arabic meaning stays ("r" has none)
        assert_eq!(s.expand_into("Gg", Lang::Ar), "لعبة جيدة");
        assert_eq!(s.expand_into("r", Lang::Ar), "r");
    }

    #[test]
    fn what_is_not_an_abbreviation_is_left_alone() {
        let s = shipped();
        for text in ["Vitamin U is not a thing", "the nerf was big", "He said gggg", "I am a pro", "good game", "Hello world, how are you?"] {
            assert_eq!(s.expand(text), text, "{text}");
        }
        // inside a longer word nothing happens
        assert_eq!(s.expand("the ggplot library"), "the ggplot library");
        // a phrase split by punctuation is not one phrase ("see u"), but the lone "u" is still "you"
        assert_eq!(s.expand("see, u know"), "see, you know");
        // Arabic text passes through
        assert_eq!(s.expand("مرحبا بك في بيتنا."), "مرحبا بك في بيتنا.");
        assert_eq!(s.expand(""), "");
    }
}
