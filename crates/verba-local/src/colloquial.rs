//! Spoken Arabic written in chat (Algerian Darija, and the commonest Levantine, Gulf and Egyptian words) turned into Modern Standard
//! Arabic before a sentence goes to the translation network, which was trained on written Modern Standard Arabic only
//! (`seed/ar-colloquial.txt` has the reasons and the format).

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::fold::fold;

#[derive(Default)]
pub struct Colloquial {
    /// folded spelling (words joined by one space) -> modern standard Arabic
    map: HashMap<String, String>,
    /// the longest spelling, in words
    longest: usize,
}

/// One word as a key: the Algerian and Gulf letters that Modern Standard Arabic does not have count as their nearest letter
/// (`ڨ` as `ق`), then the usual folding (short vowels, hamza and alef forms, `ة`/`ه`, `ى`/`ي` do not matter).
fn word_key(word: &str) -> String {
    let mapped: String = word
        .chars()
        .map(|c| match c {
            '\u{06A8}' => '\u{0642}', // ڨ
            '\u{06AF}' | '\u{06A9}' => '\u{0643}', // گ ک
            '\u{06A4}' => '\u{0641}', // ڤ
            '\u{067E}' => '\u{0628}', // پ
            '\u{0686}' => '\u{062C}', // چ
            _ => c,
        })
        .collect();
    fold(&mapped)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || verba_core::is_arabic_mark(c)
}

/// The words of a text with their byte ranges.
fn words_of(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        match (is_word_char(c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    out
}

impl Colloquial {
    pub fn parse(text: &str) -> Colloquial {
        let mut c = Colloquial::default();
        for line in text.lines() {
            let line = line.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((spellings, msa)) = line.split_once('|') else { continue };
            let msa = msa.trim();
            if msa.is_empty() {
                continue;
            }
            for spelling in spellings.split(',') {
                let key: Vec<String> = words_of(spelling.trim()).iter().map(|&(a, b)| word_key(&spelling.trim()[a..b])).collect();
                if key.is_empty() {
                    continue;
                }
                c.longest = c.longest.max(key.len());
                c.map.entry(key.join(" ")).or_insert_with(|| msa.to_string());
            }
        }
        c
    }

    /// The text with every known spoken word or phrase replaced (the longest phrase at each place wins). Spaces and punctuation stay.
    pub fn expand(&self, text: &str) -> String {
        let words = words_of(text);
        let mut out = String::new();
        let mut copied = 0;
        let mut i = 0;
        while i < words.len() {
            let mut replaced = false;
            for len in (1..=self.longest.min(words.len() - i)).rev() {
                let spaced = (i..i + len - 1).all(|j| {
                    let between = &text[words[j].1..words[j + 1].0];
                    !between.is_empty() && between.chars().all(char::is_whitespace)
                });
                if !spaced {
                    continue;
                }
                let key = (i..i + len).map(|j| word_key(&text[words[j].0..words[j].1])).collect::<Vec<_>>().join(" ");
                let found: Option<String> = match self.map.get(&key) {
                    Some(msa) => Some(msa.clone()),
                    // a noun with the article (`الطوموبيل`) or with `لل` (`للسبيطار`): the little word stays in front of the standard word
                    None if len == 1 => ["ال", "لل"].iter().find_map(|article| {
                        key.strip_prefix(article)
                            .filter(|rest| rest.chars().count() >= 3)
                            .and_then(|rest| self.map.get(rest))
                            .filter(|msa| !msa.contains(' ') && !msa.starts_with("ال"))
                            .map(|msa| format!("{article}{msa}"))
                    }),
                    None => None,
                };
                if let Some(msa) = found {
                    out.push_str(&text[copied..words[i].0]);
                    out.push_str(&msa);
                    copied = words[i + len - 1].1;
                    i += len;
                    replaced = true;
                    break;
                }
            }
            if !replaced {
                i += 1;
            }
        }
        out.push_str(&text[copied..]);
        out
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}

/// The list that ships with the program.
pub fn shipped() -> &'static Colloquial {
    static SHIPPED: OnceLock<Colloquial> = OnceLock::new();
    SHIPPED.get_or_init(|| Colloquial::parse(include_str!("../seed/ar-colloquial.txt")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_words_become_modern_standard() {
        let c = shipped();
        assert_eq!(c.expand("بغيت نروح للدار دروك"), "أردت أذهب للدار الآن");
        assert_eq!(c.expand("شحال الثمن؟ غالي بزاف"), "كم الثمن؟ غالي كثيرا");
        assert_eq!(c.expand("يا زلمة شو بدك؟"), "يا رجل ماذا تريد؟");
        // a phrase wins over its words: "واش راك" is "how are you", not two words
        assert_eq!(c.expand("واش راك؟ كلش مليح؟"), "كيف حالك؟ كل شيء جيد؟");
        assert_eq!(c.expand("نتا فين راك دروك؟"), "أنت أين أنت الآن؟");
    }

    #[test]
    fn spelling_variants_and_the_algerian_letters_are_found() {
        let c = shipped();
        // ڨ for ق, hamza and alef forms, short vowels
        assert_eq!(c.expand("نڨدر"), "أستطيع");
        assert_eq!(c.expand("اوكي"), "حسنا");
        assert_eq!(c.expand("أوكي"), "حسنا");
        assert_eq!(c.expand("دَروك"), "الآن");
        assert_eq!(c.expand("مَاشِي"), "ليس");
        // the article stays: the car, the office
        assert_eq!(c.expand("هادي هي الطوموبيل"), "هذه هي السيارة");
        assert_eq!(c.expand("في البيرو"), "في المكتب");
        assert_eq!(c.expand("نمشي للسبيطار"), "أذهب للمستشفى");
        // but a word that only looks like it has an article is not touched
        assert_eq!(c.expand("القلم"), "القلم");
    }

    #[test]
    fn modern_standard_text_and_other_scripts_are_left_alone() {
        let c = shipped();
        for text in ["أريد أن أذهب إلى المدرسة غدا.", "الصحة جيدة", "هل يمكنك أن ترسل لي الملف قبل الساعة الخامسة؟", "Hello world", "وليد ذهب مع دراهم كثيرة", ""] {
            assert_eq!(c.expand(text), text, "{text}");
        }
        // inside a longer word nothing happens
        assert_eq!(c.expand("شوارع"), "شوارع");
        assert_eq!(c.expand("ماشية"), "ماشية");
    }

    #[test]
    fn the_list_is_read_completely() {
        let text = include_str!("../seed/ar-colloquial.txt");
        let lines = text.lines().filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#')).count();
        let c = Colloquial::parse(text);
        assert!(lines >= 150, "{lines} lines");
        assert!(c.len() >= lines, "every line gives at least one spelling ({} for {lines})", c.len());
        assert!(text.lines().filter(|l| !l.starts_with('#')).all(|l| l.trim().is_empty() || l.contains('|')), "a line without | was not understood");
    }
}
