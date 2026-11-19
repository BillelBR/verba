//! Text typed with the wrong keyboard layout, put right.
//!
//! A whole paragraph typed in Arabic while the keyboard was English comes out as `lkhvh`: each letter is the key that, on the Arabic
//! layout, gives the Arabic letter. So the way back is exact: find the **key** (its scan code, with Shift or AltGr) that gives each
//! character on the layout the text was typed with, and write what the same key gives on the layout it was meant for.
//!
//! Nothing here touches Windows. A `KeyMap` is plain data (Windows builds one per installed layout, see `verba-win`), so the whole
//! thing is tested with small maps. Which layout the text was typed with is not asked: every pair of layouts is tried and the result
//! that is made of real words (in the word lists) is offered first.

use std::collections::HashMap;
use verba_core::{Lang, strip_arabic_marks};

/// Shift held with the key.
pub const SHIFT: u8 = 1;
/// AltGr (Ctrl+Alt) held with the key.
pub const ALTGR: u8 = 2;

/// A key of the keyboard: where it is (the scan code does not change with the layout) and which modifiers were held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub scan: u16,
    pub mods: u8,
}

/// What every key of one layout gives, both ways.
#[derive(Clone, Debug, Default)]
pub struct KeyMap {
    by_text: HashMap<String, Key>,
    by_key: HashMap<Key, String>,
    /// The most characters one key gives (the Arabic lam-alef key gives two).
    longest: usize,
}

impl KeyMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tells that `key` gives `text`. Insert the plain keys first, then Shift, then AltGr: when two keys give the same text, the
    /// first one is the one used to go back. Control characters are not text and are ignored.
    pub fn insert(&mut self, key: Key, text: &str) {
        if text.is_empty() || text.chars().any(char::is_control) {
            return;
        }
        if self.by_key.contains_key(&key) {
            return;
        }
        self.by_key.insert(key, text.to_string());
        self.by_text.entry(text.to_string()).or_insert(key);
        self.longest = self.longest.max(text.chars().count());
    }

    /// How many keys the layout has (a layout that gave nothing is useless).
    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }
}

/// Writes `text` (typed with the layout `from`) as the same keys give on the layout `to`.
/// A character that no key of `from` gives (a line break, a letter from a third layout) stays as it is, and so does one whose key
/// gives nothing on `to`.
pub fn convert(text: &str, from: &KeyMap, to: &KeyMap) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let mut taken = 0;
        // the longest piece first: one key can give two characters
        for len in (1..=from.longest.min(chars.len() - i)).rev() {
            let piece: String = chars[i..i + len].iter().collect();
            if let Some(key) = from.by_text.get(&piece) {
                match to.by_key.get(key) {
                    Some(t) => out.push_str(t),
                    None => out.push_str(&piece),
                }
                taken = len;
                break;
            }
        }
        if taken == 0 {
            out.push(chars[i]);
            taken = 1;
        }
        i += taken;
    }
    out
}

/// An installed layout: a name for the user, the language it types, and its keys.
#[derive(Clone, Debug)]
pub struct Layout {
    pub name: String,
    pub lang: Lang,
    pub map: KeyMap,
}

/// One way to read the text: typed on layout `from` (an index of the list given), meant for layout `to`.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub from: usize,
    pub to: usize,
    pub text: String,
    /// The share (0 to 1, by letters) of its words that are real words of the language of `to`.
    pub score: f64,
}

/// A result needs at least this share of real words, and must be clearly better than the text as it is.
const MIN_SCORE: f64 = 0.34;
const MARGIN: f64 = 0.2;
/// Text that no language recognises at all (under this share) is probably gibberish: then a weaker result is still worth offering.
const UNRECOGNISED: f64 = 0.15;
const WEAK_SCORE: f64 = 0.1;
/// Only results close to the best one are shown, and never more than this many.
const KEEP_NEAR_BEST: f64 = 0.2;
pub const MAX_CANDIDATES: usize = 3;

/// The share, by letters, of the words (two letters or more) of `text` that `known` says are words of `lang`.
pub fn real_word_share(text: &str, lang: Lang, known: &dyn Fn(Lang, &str) -> bool) -> f64 {
    let (mut found, mut all) = (0usize, 0usize);
    for raw in text.split(|c: char| !c.is_alphabetic() && !verba_core::is_arabic_mark(c)) {
        let word = if lang == Lang::Ar { strip_arabic_marks(raw) } else { raw.to_string() };
        let letters = word.chars().count();
        if letters < 2 {
            continue;
        }
        all += letters;
        if known(lang, &word.to_lowercase()) {
            found += letters;
        }
    }
    if all == 0 { 0.0 } else { found as f64 / all as f64 }
}

/// The ways `text` can be read as typed on the wrong layout, best first (empty when the text is fine as it is, or nothing makes sense).
/// `known(lang, word)` says whether a word is a real word of that language.
pub fn candidates(text: &str, layouts: &[Layout], known: &dyn Fn(Lang, &str) -> bool) -> Vec<Candidate> {
    let as_typed = layouts.iter().map(|l| real_word_share(text, l.lang, known)).fold(0.0, f64::max);
    let mut all: Vec<Candidate> = Vec::new();
    for (from, f) in layouts.iter().enumerate() {
        for (to, t) in layouts.iter().enumerate() {
            if from == to || f.lang == t.lang {
                continue;
            }
            let converted = convert(text, &f.map, &t.map);
            if converted == text {
                continue;
            }
            let score = real_word_share(&converted, t.lang, known);
            all.push(Candidate { from, to, text: converted, score });
        }
    }
    // best first; on a tie the layout that comes first in the list (the order Windows gives is the user's own)
    all.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then(a.from.cmp(&b.from)).then(a.to.cmp(&b.to)));
    let strict: Vec<Candidate> = all.iter().filter(|c| c.score >= MIN_SCORE && c.score > as_typed + MARGIN).cloned().collect();
    let chosen = if !strict.is_empty() {
        strict
    } else if as_typed < UNRECOGNISED {
        all.into_iter().filter(|c| c.score >= WEAK_SCORE).collect()
    } else {
        Vec::new()
    };
    let best = chosen.first().map_or(0.0, |c| c.score);
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for c in chosen {
        // another layout that gives the same text is not another choice
        if c.score + KEEP_NEAR_BEST < best || seen.contains(&c.text) {
            continue;
        }
        seen.push(c.text.clone());
        out.push(c);
        if out.len() == MAX_CANDIDATES {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small keyboard: the scan codes 0x10.. are the keys of a row, each given its text (unshifted, then shifted).
    fn row(first_scan: u16, plain: &[&str], shifted: &[&str]) -> KeyMap {
        let mut m = KeyMap::new();
        for (i, t) in plain.iter().enumerate() {
            m.insert(Key { scan: first_scan + i as u16, mods: 0 }, t);
        }
        for (i, t) in shifted.iter().enumerate() {
            m.insert(Key { scan: first_scan + i as u16, mods: SHIFT }, t);
        }
        m
    }

    fn chars(s: &str) -> Vec<&str> {
        s.split_inclusive(|_: char| true).collect()
    }

    /// Row of letters q..p, then a..l, then z..m, with the space key (0x39), like a QWERTY keyboard.
    fn qwerty() -> KeyMap {
        let mut m = row(0x10, &chars("qwertyuiop"), &chars("QWERTYUIOP"));
        for (k, t) in row(0x1E, &chars("asdfghjkl"), &chars("ASDFGHJKL")).by_key {
            m.insert(k, &t);
        }
        for (k, t) in row(0x2C, &chars("zxcvbnm"), &chars("ZXCVBNM")).by_key {
            m.insert(k, &t);
        }
        m.insert(Key { scan: 0x39, mods: 0 }, " ");
        m
    }

    /// The French keyboard differs from it by a few keys: a and q, z and w, and m next to l.
    fn azerty() -> KeyMap {
        let mut m = row(0x10, &chars("azertyuiop"), &chars("AZERTYUIOP"));
        for (k, t) in row(0x1E, &chars("qsdfghjkl"), &chars("QSDFGHJKL")).by_key {
            m.insert(k, &t);
        }
        for (k, t) in row(0x2C, &chars("wxcvbn,"), &chars("WXCVBN?")).by_key {
            m.insert(k, &t);
        }
        m.insert(Key { scan: 0x39, mods: 0 }, " ");
        m
    }

    /// The same keys with Arabic letters (the real layouts have more keys; this is enough to test the logic). The key at 0x2B gives
    /// two characters (lam-alef), like the real Arabic layouts.
    fn arabic() -> KeyMap {
        let mut m = row(0x10, &chars("ضصثقفغعهخح"), &[]);
        for (k, t) in row(0x1E, &chars("شسيبلاتنم"), &[]).by_key {
            m.insert(k, &t);
        }
        for (k, t) in row(0x2C, &chars("ئءؤرذى"), &[]).by_key {
            m.insert(k, &t);
        }
        m.insert(Key { scan: 0x2B, mods: 0 }, "لا");
        m.insert(Key { scan: 0x39, mods: 0 }, " ");
        m
    }

    #[test]
    fn the_same_key_gives_the_letter_of_the_other_layout() {
        // a on the French keyboard is the key of q on the English one
        assert_eq!(convert("azerty", &azerty(), &qwerty()), "qwerty");
        assert_eq!(convert("qwerty", &qwerty(), &azerty()), "azerty");
        // Shift is kept
        assert_eq!(convert("Azerty", &azerty(), &qwerty()), "Qwerty");
    }

    #[test]
    fn going_there_and_back_gives_the_text_again() {
        let text = "the quick brown fox";
        let arabic_text = convert(text, &qwerty(), &arabic());
        assert_ne!(arabic_text, text);
        assert_eq!(convert(&arabic_text, &arabic(), &qwerty()), text);
    }

    #[test]
    fn what_no_key_gives_stays_as_it_is() {
        // a line break, a digit and a letter that this keyboard has no key for are left alone
        assert_eq!(convert("hi\n42 é", &qwerty(), &arabic()), "اه\n42 é");
        // a key that gives nothing on the other layout keeps the character
        assert_eq!(convert("Q", &qwerty(), &arabic()), "Q");
    }

    #[test]
    fn a_key_that_gives_two_characters_is_read_as_one_key() {
        let mut us = qwerty();
        us.insert(Key { scan: 0x2B, mods: 0 }, "\\");
        // lam-alef (two characters) came from the key of the backslash
        assert_eq!(convert("لا", &arabic(), &us), "\\");
        // and the way back writes both characters
        assert_eq!(convert("\\", &us, &arabic()), "لا");
        // lam on its own and alef on its own are still their own keys
        assert_eq!(convert("لب", &arabic(), &qwerty()), "gf");
    }

    #[test]
    fn controls_and_empty_texts_are_not_keys() {
        let mut m = KeyMap::new();
        m.insert(Key { scan: 1, mods: 0 }, "");
        m.insert(Key { scan: 2, mods: 0 }, "\u{1}");
        m.insert(Key { scan: 3, mods: 0 }, "a");
        assert_eq!(m.len(), 1);
        // the first key given for a text is the one used to come back
        m.insert(Key { scan: 4, mods: 0 }, "a");
        assert_eq!(convert("a", &m, &row(0, &["x", "y", "z", "w", "v"], &[])), "w");
    }

    fn layouts() -> Vec<Layout> {
        vec![
            Layout { name: "US".into(), lang: Lang::En, map: qwerty() },
            Layout { name: "FR".into(), lang: Lang::Fr, map: azerty() },
            Layout { name: "AR".into(), lang: Lang::Ar, map: arabic() },
        ]
    }

    /// Real words: English `the quick fox` and `where`, French `quel`, Arabic words made from the keys above.
    fn known(lang: Lang, word: &str) -> bool {
        match lang {
            Lang::En => ["the", "quick", "fox", "where", "she", "he"].contains(&word),
            Lang::Fr => ["quel", "elle", "les"].contains(&word),
            Lang::Ar => ["شسي", "ضصث", "سيب", "قفغ", "بست"].contains(&word),
            Lang::Dz => false,
        }
    }

    #[test]
    fn real_words_are_counted_by_letters() {
        assert_eq!(real_word_share("the quick xyz", Lang::En, &known), 8.0 / 11.0);
        assert_eq!(real_word_share("a b", Lang::En, &known), 0.0, "one-letter words do not count");
        assert_eq!(real_word_share("", Lang::En, &known), 0.0);
        // Arabic marks do not stop a word from being found
        assert_eq!(real_word_share("شَسِي", Lang::Ar, &known), 1.0);
    }

    #[test]
    fn arabic_typed_on_the_english_keyboard_is_found_and_offered_first() {
        // `ash sdi` are the keys of the Arabic words شسي and سيب
        let found = candidates("asd sdf", &layouts(), &known);
        assert!(!found.is_empty());
        assert_eq!(found[0].text, "شسي سيب");
        assert_eq!((found[0].from, found[0].to), (0, 2));
    }

    #[test]
    fn the_french_keyboard_is_told_from_the_english_one_by_the_words() {
        // `quel elle` meant for the French keyboard, typed while the English one was active: the key of q is the key of a there
        let on_french = convert("quel elle", &azerty(), &qwerty());
        let found = candidates(&on_french, &layouts(), &known);
        assert!(found.iter().any(|c| c.text == "quel elle" && c.to == 1), "{found:?}");
    }

    #[test]
    fn text_that_is_right_already_is_left_alone() {
        assert!(candidates("the quick fox", &layouts(), &known).is_empty());
        assert!(candidates("شسي سيب", &layouts(), &known).is_empty());
        assert!(candidates("", &layouts(), &known).is_empty());
    }

    #[test]
    fn gibberish_that_no_reading_makes_sense_of_gives_nothing() {
        assert!(candidates("zzzz xxxx", &layouts(), &known).is_empty());
    }

    #[test]
    fn a_weak_reading_is_offered_only_for_text_no_language_recognises() {
        // one Arabic word of four is in the list: 3 letters of 10 is under the strict share, but the text is gibberish as it is
        let found = candidates("asd xcv bn hj", &layouts(), &known);
        assert_eq!(found.first().map(|c| c.text.as_str()), Some("شسي ءؤر ذى ات"));
        // text that is half English words is not turned into Arabic by a weak reading
        assert!(candidates("the quick xcv bnm", &layouts(), &known).is_empty());
    }

    #[test]
    fn at_most_three_results_and_never_two_with_the_same_text() {
        let found = candidates("asd sdf", &layouts(), &known);
        assert!(found.len() <= MAX_CANDIDATES);
        for (i, a) in found.iter().enumerate() {
            assert!(found.iter().skip(i + 1).all(|b| b.text != a.text));
        }
    }
}
