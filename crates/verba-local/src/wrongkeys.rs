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

    /// How unlikely it is that `text` was typed on this layout: a character that no key gives costs 2, one that needs AltGr costs 1
    /// (people rarely type with it). Spaces and line breaks are free. A text typed on the English keyboard has `[`, `]` and `;` that the
    /// French layout only gives with AltGr or not at all, and a text typed on the French one has `ù` and `è` that the English layout lacks.
    pub fn strain(&self, text: &str) -> usize {
        let chars: Vec<char> = text.chars().collect();
        let (mut cost, mut i) = (0, 0);
        while i < chars.len() {
            let mut taken = 0;
            for len in (1..=self.longest.min(chars.len() - i)).rev() {
                let piece: String = chars[i..i + len].iter().collect();
                if let Some(key) = self.by_text.get(&piece) {
                    if key.mods & ALTGR != 0 {
                        cost += 1;
                    }
                    taken = len;
                    break;
                }
            }
            if taken == 0 {
                if !chars[i].is_whitespace() && !chars[i].is_control() {
                    cost += 2;
                }
                taken = 1;
            }
            i += taken;
        }
        cost
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

/// How many characters of `text` `convert` leaves as they are (not a key of `from`, or a key that gives nothing on `to`): spaces and line
/// breaks do not count. A right reading turns every character; one made with the wrong layout leaves some behind.
pub fn unconverted(text: &str, from: &KeyMap, to: &KeyMap) -> usize {
    let chars: Vec<char> = text.chars().collect();
    let (mut left, mut i) = (0, 0);
    while i < chars.len() {
        let mut taken = 0;
        for len in (1..=from.longest.min(chars.len() - i)).rev() {
            let piece: String = chars[i..i + len].iter().collect();
            if let Some(key) = from.by_text.get(&piece) {
                if !to.by_key.contains_key(key) {
                    left += 1;
                }
                taken = len;
                break;
            }
        }
        if taken == 0 {
            if !chars[i].is_whitespace() && !chars[i].is_control() {
                left += 1;
            }
            taken = 1;
        }
        i += taken;
    }
    left
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
    /// How many letters of the reading are in real words: a reading needs some evidence of its own, not one short word found by chance.
    pub evidence: usize,
    /// How hard it is to have typed the text as it is on the layout `from` (see `KeyMap::strain`): of two layouts that explain the text
    /// equally well, the one that needs fewer odd keys is the one it was typed on.
    pub strain: usize,
}

/// A result needs at least this share of real words, and must be clearly better than the text as it is: better by `MARGIN`, or
/// (when most words of the text were right on both layouts, so there is little room left) leaving at most this part of the words
/// that were not real words before.
const MIN_SCORE: f64 = 0.34;
const MARGIN: f64 = 0.2;
const MOST_FIXED: f64 = 0.3;
/// The letters of real words a reading needs: three for Arabic (a word of three letters is a word), four for English and French, whose
/// lists hold many short strings that wrong letters make by chance (`all`, `lf`); and there the words of two letters do not count at all.
fn evidence_needed(lang: Lang) -> usize {
    if lang == Lang::Ar { 3 } else { 4 }
}
fn shortest_word(lang: Lang) -> usize {
    if lang == Lang::Ar { 2 } else { 3 }
}
/// Text that no language recognises at all (under this share) is a name or a word the lists lack: `forced` can still turn it.
const UNRECOGNISED: f64 = 0.15;
/// Only results close to the best one are shown, and never more than this many.
const KEEP_NEAR_BEST: f64 = 0.2;
pub const MAX_CANDIDATES: usize = 3;

/// The share, by letters, of the words (two letters or more) of `text` that `known` says are words of `lang`.
pub fn real_word_share(text: &str, lang: Lang, known: &dyn Fn(Lang, &str) -> bool) -> f64 {
    real_word_share_from(text, lang, known, 2)
}

/// The same, counting only the words of `min_letters` letters or more. Wrong letters often make a word of two letters by chance
/// (`og`, `ho`), so as evidence that a text is right as it is, only longer words are taken.
fn real_word_share_from(text: &str, lang: Lang, known: &dyn Fn(Lang, &str) -> bool, min_letters: usize) -> f64 {
    let (found, all) = word_counts(text, lang, known, min_letters);
    if all == 0 { 0.0 } else { found as f64 / all as f64 }
}

/// (the letters that are in real words, all the letters of the words) of `text`, counting only words of `min_letters` letters or more as real.
fn word_counts(text: &str, lang: Lang, known: &dyn Fn(Lang, &str) -> bool, min_letters: usize) -> (usize, usize) {
    let (mut found, mut all) = (0usize, 0usize);
    for token in text.split_whitespace() {
        // a word with something stuck inside it that its language does not have is not evidence, but its letters are still there to count
        if is_suspicious(token, lang) {
            all += token.chars().filter(|c| !c.is_whitespace()).count();
            continue;
        }
        for raw in token.split(|c: char| !c.is_alphabetic() && !verba_core::is_arabic_mark(c)) {
            let word = if lang == Lang::Ar { strip_arabic_marks(raw) } else { raw.to_string() };
            let letters = word.chars().count();
            if letters < 2 {
                continue;
            }
            all += letters;
            if letters >= min_letters && known(lang, &word.to_lowercase()) {
                found += letters;
            }
        }
    }
    (found, all)
}

/// Symbols that right text almost never has stuck to a word, in any language: letters typed on the wrong layout come out as these (an
/// Arabic letter is `]`, `$` or `^` on an English or a French keyboard). A `;`, a `:` or a `,` is only odd inside a word.
const GLUED_ANYWHERE: &[char] = &['[', ']', '{', '}', '<', '>', '$', '^', '|', '\\', '~', '@', '#', '%', '&', '*', '+', '='];
const GLUED_INSIDE: &[char] = &[';', ':', ','];

/// The ASCII marks that right Arabic text has at the start of a word (a quote, a bracket, a hyphen) and at its end (a full stop, `!`, `:`, a
/// quote, a bracket, a hyphen). Anywhere else, and every other ASCII mark (`'`, `,`, `;`, `?`, `/`), is what a key that was not turned
/// leaves behind: Arabic has its own comma, semicolon and question mark.
const ARABIC_LEADING_OK: &[char] = &['"', '(', '-'];
const ARABIC_TRAILING_OK: &[char] = &['.', '!', ':', '"', ')', '-'];

/// How many characters of a word (the text between two spaces) right text of `lang` would not have there: for Arabic every character of
/// it that is not an Arabic letter or mark, apart from the marks above at its two ends; for the other languages a symbol of the lists
/// above stuck to it, and a `;`, `:` or `,` inside it. A word with no letter (a number, a mark) has none.
fn token_strays(token: &str, lang: Lang) -> usize {
    if !token.chars().any(char::is_alphabetic) {
        return 0;
    }
    if lang == Lang::Ar {
        let core = token.trim_start_matches(|c| ARABIC_LEADING_OK.contains(&c)).trim_end_matches(|c| ARABIC_TRAILING_OK.contains(&c));
        return core.chars().filter(|&c| !(verba_core::is_arabic(c) || verba_core::is_arabic_mark(c))).count();
    }
    let anywhere = token.chars().filter(|c| GLUED_ANYWHERE.contains(c)).count();
    let core = token.trim_matches(|c: char| !c.is_alphanumeric() && !verba_core::is_arabic_mark(c));
    anywhere + core.chars().filter(|c| GLUED_INSIDE.contains(c)).count()
}

/// A word that right text of `lang` does not have (see `token_strays`): it is no evidence that the text is right.
fn is_suspicious(token: &str, lang: Lang) -> bool {
    token_strays(token, lang) > 0
}

/// How many characters of `text` right text of `lang` would not have: a reading with many of them used the wrong layout.
fn stray(text: &str, lang: Lang) -> usize {
    text.split_whitespace().map(|token| token_strays(token, lang)).sum()
}

/// The ways `text` can be read as typed on the wrong layout, best first (empty when the text is fine as it is, or nothing makes sense).
/// `known(lang, word)` says whether a word is a real word of that language. `prefer` is the language of the keyboard that was active when
/// the text was selected: of two readings that are equally good, the one typed on that keyboard comes first.
pub fn candidates(text: &str, layouts: &[Layout], prefer: Option<Lang>, known: &dyn Fn(Lang, &str) -> bool) -> Vec<Candidate> {
    let as_typed = as_typed_share(text, layouts, known);
    let mut all: Vec<Candidate> = Vec::new();
    for (from, f) in layouts.iter().enumerate() {
        for (to, t) in layouts.iter().enumerate() {
            if from == to || f.lang == t.lang {
                continue;
            }
            let converted = convert(text, &f.map, &t.map);
            // the same letters (only a punctuation mark differs, as between two layouts that share the letters of the text) is no reading
            if converted.chars().filter(|c| c.is_alphabetic()).eq(text.chars().filter(|c| c.is_alphabetic())) {
                continue;
            }
            let (found, letters) = word_counts(&converted, t.lang, known, shortest_word(t.lang));
            let score = if letters == 0 { 0.0 } else { found as f64 / letters as f64 };
            let strain = f.map.strain(text) + stray(&converted, t.lang) + unconverted(text, &f.map, &t.map);
            all.push(Candidate { from, to, text: converted, score, evidence: found, strain });
        }
    }
    // best first; of equal readings the layout the text is easiest to have typed on, then the active keyboard, then the order Windows gives
    let on_active = |c: &Candidate| prefer != Some(layouts[c.from].lang);
    all.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.strain.cmp(&b.strain))
            .then(on_active(a).cmp(&on_active(b)))
            .then(a.from.cmp(&b.from))
            .then(a.to.cmp(&b.to))
    });
    let clearly_better = |score: f64| score >= MIN_SCORE && score > as_typed && (score >= as_typed + MARGIN || 1.0 - score <= (1.0 - as_typed) * MOST_FIXED);
    // the keyboard that is active is the one the text was most likely typed on: a reading from another layout needs more evidence than a
    // short word found by chance (when the active keyboard is the one the text is turned into, it says nothing about the source)
    let needed = |c: &Candidate| {
        let from_other = prefer.is_some_and(|p| p != layouts[c.from].lang && p != layouts[c.to].lang);
        evidence_needed(layouts[c.to].lang) + if from_other { 2 } else { 0 }
    };
    let strict: Vec<Candidate> = all.iter().filter(|c| c.evidence >= needed(c) && clearly_better(c.score)).cloned().collect();
    let chosen = strict;
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

/// How much of `text` is made of real words as it is, in the best of the languages of the layouts (words of three letters or more).
/// A text is only taken as right when it has the evidence a reading needs (see `evidence_needed`): one short word found by chance (`tor`,
/// `imf`) does not make `tor vq\e]q` right text.
fn as_typed_share(text: &str, layouts: &[Layout], known: &dyn Fn(Lang, &str) -> bool) -> f64 {
    layouts
        .iter()
        .map(|l| {
            let (found, all) = word_counts(text, l.lang, known, 3);
            if all == 0 || found < evidence_needed(l.lang) { 0.0 } else { found as f64 / all as f64 }
        })
        .fold(0.0, f64::max)
}

/// The last resort, for a caller that knows what the user wants (the text turned into the language `to`, whatever the words): when no
/// reading is made of real words and the text is not made of real words as it is either (a name, a word the lists do not have), the text
/// is turned into the layout of `to` from the layout it is easiest to have typed on (see `KeyMap::strain`), the active keyboard `prefer`
/// first when that does not tell. `None` when there is no such layout or no letter changes.
pub fn forced(text: &str, layouts: &[Layout], prefer: Option<Lang>, to: Lang, known: &dyn Fn(Lang, &str) -> bool) -> Option<Candidate> {
    if as_typed_share(text, layouts, known) >= UNRECOGNISED {
        return None;
    }
    let to_index = layouts.iter().position(|l| l.lang == to)?;
    let reading = |i: usize| convert(text, &layouts[i].map, &layouts[to_index].map);
    let strain_of = |i: usize| layouts[i].map.strain(text) + stray(&reading(i), to) + unconverted(text, &layouts[i].map, &layouts[to_index].map);
    let from = (0..layouts.len()).filter(|&i| layouts[i].lang != to).min_by_key(|&i| (strain_of(i), Some(layouts[i].lang) != prefer, i))?;
    let converted = reading(from);
    if converted.chars().filter(|c| c.is_alphabetic()).eq(text.chars().filter(|c| c.is_alphabetic())) {
        return None;
    }
    let (found, letters) = word_counts(&converted, to, known, shortest_word(to));
    let score = if letters == 0 { 0.0 } else { found as f64 / letters as f64 };
    Some(Candidate { from, to: to_index, text: converted, score, evidence: found, strain: strain_of(from) })
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
        let found = candidates("asd sdf", &layouts(), None, &known);
        assert!(!found.is_empty());
        assert_eq!(found[0].text, "شسي سيب");
        assert_eq!((found[0].from, found[0].to), (0, 2));
    }

    #[test]
    fn the_french_keyboard_is_told_from_the_english_one_by_the_words() {
        // `quel elle` meant for the French keyboard, typed while the English one was active: the key of q is the key of a there
        let on_french = convert("quel elle", &azerty(), &qwerty());
        let found = candidates(&on_french, &layouts(), None, &known);
        assert!(found.iter().any(|c| c.text == "quel elle" && c.to == 1), "{found:?}");
    }

    #[test]
    fn text_whose_words_mostly_survive_the_wrong_keyboard_is_still_put_right() {
        // most keys are the same on both layouts: only `quick` changes (q is a on the French one), so 17 letters of 22 are real words as typed
        let text = "the fox she he the fox quick";
        let typed = convert(text, &qwerty(), &azerty());
        assert_eq!(typed, "the fox she he the fox auick");
        let found = candidates(&typed, &layouts(), None, &known);
        assert_eq!(found.first().map(|c| c.text.as_str()), Some(text));
        // one word that is not in the list is not a reason to change a text that is right
        assert!(candidates("the fox she he the fox zzzzz", &layouts(), None, &known).is_empty());
    }

    #[test]
    fn a_short_word_made_by_chance_does_not_hide_the_wrong_keyboard() {
        // `he` is a real English word: on its own it must not make `asd he` look like right text (2 letters of 5 would leave the Arabic
        // reading, 3 of 5, too close to it); only words of three letters or more are evidence that a text is right
        let found = candidates("asd he", &layouts(), None, &known);
        assert_eq!(found.first().map(|c| c.text.as_str()), Some("شسي اث"), "{found:?}");
    }

    #[test]
    fn the_active_keyboard_comes_first_between_equal_readings() {
        // `qwe`-keys give the same words on the English and on the French layout here (q, w and e are in the same places): a tie
        let text = "ghj ghj";
        let mut l = layouts();
        // a second Arabic-reading source: the French layout gets the same keys as the English one for this text
        l[1].map = qwerty();
        let arabic_word = |lang: Lang, w: &str| lang == Lang::Ar && w.chars().count() == 3 && w.chars().all(verba_core::is_arabic);
        let on_english = candidates(text, &l, Some(Lang::En), &arabic_word);
        let on_french = candidates(text, &l, Some(Lang::Fr), &arabic_word);
        assert_eq!(on_english.first().map(|c| c.from), Some(0));
        assert_eq!(on_french.first().map(|c| c.from), Some(1));
    }

    #[test]
    fn the_last_resort_turns_text_into_the_wanted_language_whatever_the_words() {
        // names that no list has: no reading is made of real words, yet the user wants Arabic
        assert!(candidates("ghj kl", &layouts(), None, &known).is_empty(), "no reading is made of real words");
        let forced_reading = forced("ghj kl", &layouts(), Some(Lang::En), Lang::Ar, &known).expect("a reading");
        assert_eq!(forced_reading.text, convert("ghj kl", &qwerty(), &arabic()));
        assert_eq!((forced_reading.from, forced_reading.to), (0, 2));
        // typed on the French keyboard: the French layout is the source
        let on_french = forced("ghj kl", &layouts(), Some(Lang::Fr), Lang::Ar, &known).expect("a reading");
        assert_eq!(on_french.from, 1);
        // text that is made of real words as it is is left alone, and so is a text that no key changes
        assert!(forced("the quick fox", &layouts(), Some(Lang::En), Lang::Ar, &known).is_none());
        assert!(forced("", &layouts(), None, Lang::Ar, &known).is_none());
        // no layout of the wanted language: nothing to do
        assert!(forced("sdf", &layouts()[..2], None, Lang::Ar, &known).is_none());
    }

    #[test]
    fn a_reading_that_changes_only_a_punctuation_mark_is_no_reading() {
        // two layouts that give the same letters and differ by the key of the full stop: nothing to put right in `aa aa.`
        let one = |dot: &str| {
            let mut m = KeyMap::new();
            m.insert(Key { scan: 1, mods: 0 }, "a");
            m.insert(Key { scan: 2, mods: 0 }, dot);
            m.insert(Key { scan: 3, mods: 0 }, " ");
            m
        };
        let two = vec![Layout { name: "en".into(), lang: Lang::En, map: one(".") }, Layout { name: "fr".into(), lang: Lang::Fr, map: one("/") }];
        // `aa` is a word of two letters of the French list: as evidence it is ignored, so only the letters rule keeps `aa aa/` away
        let fr_aa = |lang: Lang, w: &str| lang == Lang::Fr && w == "aa";
        assert!(candidates("aa aa.", &two, None, &fr_aa).is_empty());
    }

    #[test]
    fn symbols_stuck_to_a_word_are_not_what_right_text_looks_like() {
        let l = layouts();
        assert_eq!(as_typed_share("the fox", &l, &known), 1.0);
        assert_eq!(as_typed_share("the] fox", &l, &known), 0.0, "three letters of real words are not enough evidence (four are needed) once `the]` is out");
        assert_eq!(as_typed_share("the quick] fox", &l, &known), 6.0 / 12.0, "the word with a symbol stuck to it is not evidence, and the symbol is one more character that is not a word");
        assert_eq!(as_typed_share("the]", &l, &known), 0.0);
        assert_eq!(as_typed_share("the$ quick", &l, &known), 5.0 / 9.0);
        // a semicolon or a comma ends a word as it does in right text; inside a word it is odd
        assert_eq!(as_typed_share("the fox; quick,", &l, &known), 1.0);
        assert_eq!(as_typed_share("th;e", &l, &known), 0.0);
    }

    #[test]
    fn an_arabic_word_with_something_left_inside_is_no_evidence() {
        // `شسي` is a word of the list; with a comma, a digit or a Latin letter in the middle it is a key that was not turned
        assert_eq!(real_word_share("شسي", Lang::Ar, &known), 1.0);
        assert_eq!(real_word_share("شسي،", Lang::Ar, &known), 1.0, "a comma after the word is punctuation");
        assert_eq!(real_word_share("ش,سي", Lang::Ar, &known), 0.0);
        assert_eq!(real_word_share("شس4ي", Lang::Ar, &known), 0.0);
        assert_eq!(real_word_share("شسيa", Lang::Ar, &known), 0.0);
        // a suspicious word still counts in what the text is made of, with all its characters (three letters and a comma here)
        assert_eq!(real_word_share("شسي ش,سي", Lang::Ar, &known), 3.0 / 7.0);
    }

    #[test]
    fn one_short_word_found_by_chance_is_not_enough_evidence() {
        // Arabic letters read as English: a word of three letters is not enough (four are needed), a word of four is; two never count
        let english = |lang: Lang, w: &str| lang == Lang::En && ["asd", "asdf", "as"].contains(&w);
        assert!(candidates("شسي", &layouts(), None, &english).is_empty(), "three letters of English by chance");
        let four = candidates("شسيب", &layouts(), None, &english);
        assert_eq!(four.first().map(|c| (c.text.as_str(), c.to)), Some(("asdf", 0)));
        assert!(candidates("شس", &layouts(), None, &english).is_empty(), "two letters never count for English");
        // the same three letters read as Arabic are enough
        let arabic = |lang: Lang, w: &str| lang == Lang::Ar && w == "شسي";
        assert_eq!(candidates("asd", &layouts(), None, &arabic).first().map(|c| c.text.as_str()), Some("شسي"));
    }

    #[test]
    fn a_symbol_left_in_an_arabic_reading_makes_the_reading_cost_more() {
        assert_eq!(stray("شسي، ب", Lang::Ar), 0, "the Arabic comma is Arabic");
        assert_eq!(stray("شسي. ب!", Lang::Ar), 0, "a full stop and ! are fine");
        assert_eq!(stray("شس'ي ب4", Lang::Ar), 2, "a quote and a digit are not");
        assert_eq!(stray("a b]", Lang::En), 1);
        assert_eq!(stray("ش:ب :شب شب:", Lang::Ar), 2, "a colon inside a word or at its start is a key left unturned; at the end it is punctuation");
        assert!(is_suspicious("بئ'", Lang::Ar), "a quote at the end of an Arabic word is a key left unturned");
        assert!(is_suspicious(":ام", Lang::Ar) && is_suspicious("ع:ب", Lang::Ar) && !is_suspicious("ملاحظة:", Lang::Ar));
        assert!(!is_suspicious("شسي.", Lang::Ar));
        assert!(!is_suspicious("2024", Lang::Ar), "a number has no letters");
    }

    #[test]
    fn characters_that_no_key_turns_are_counted() {
        // `[` has a key on the English layout but the Arabic one has none for it here; the space is free
        let mut us = qwerty();
        us.insert(Key { scan: 0x1A, mods: 0 }, "[");
        assert_eq!(unconverted("a [", &us, &arabic()), 1);
        assert_eq!(unconverted("as", &us, &arabic()), 0);
        // a character that is no key of the layout it was supposedly typed on is left too
        assert_eq!(unconverted("aé", &us, &arabic()), 1);
    }

    #[test]
    fn a_reading_from_another_layout_than_the_active_one_needs_more_than_a_short_word_by_chance() {
        // `qwe` on the English keys is the Arabic word ضصث (3 letters); on the French keys it is not a word
        let ar = |lang: Lang, w: &str| lang == Lang::Ar && w == "ضصث";
        let l = layouts();
        assert_eq!(candidates("qwe", &l, None, &ar).first().map(|c| c.text.as_str()), Some("ضصث"));
        assert_eq!(candidates("qwe", &l, Some(Lang::En), &ar).first().map(|c| c.text.as_str()), Some("ضصث"), "typed on the active keyboard");
        assert!(candidates("qwe", &l, Some(Lang::Fr), &ar).is_empty(), "three letters by chance, from a layout that is not the active one");
        // with the Arabic keyboard active nothing is known about the source: the short word is enough
        assert_eq!(candidates("qwe", &l, Some(Lang::Ar), &ar).first().map(|c| c.text.as_str()), Some("ضصث"));
    }

    #[test]
    fn text_that_is_right_already_is_left_alone() {
        assert!(candidates("the quick fox", &layouts(), None, &known).is_empty());
        assert!(candidates("شسي سيب", &layouts(), None, &known).is_empty());
        assert!(candidates("", &layouts(), None, &known).is_empty());
    }

    #[test]
    fn gibberish_that_no_reading_makes_sense_of_gives_nothing() {
        assert!(candidates("zzzz xxxx", &layouts(), None, &known).is_empty());
    }

    #[test]
    fn a_weak_reading_is_not_offered_by_the_words_but_the_last_resort_gives_it() {
        // one Arabic word of four is in the list: 3 letters of 10 is under the strict share, so the words do not offer it
        assert!(candidates("asd xcv bn hj", &layouts(), None, &known).is_empty());
        // the caller that wants Arabic gets it from the last resort, since the text is no words as it is
        let last = forced("asd xcv bn hj", &layouts(), Some(Lang::En), Lang::Ar, &known).expect("a reading");
        assert_eq!(last.text, "شسي ءؤر ذى ات");
        // text that is half English words is not turned into Arabic by the last resort
        assert!(candidates("the quick xcv bnm", &layouts(), None, &known).is_empty());
        assert!(forced("the quick xcv bnm", &layouts(), Some(Lang::En), Lang::Ar, &known).is_none());
    }

    #[test]
    fn the_layout_the_text_is_easiest_to_type_on_is_the_one_it_was_typed_on() {
        // English has `[` on a plain key; this French has it only with AltGr (and `]` not at all)
        let mut us = KeyMap::new();
        let mut fr = KeyMap::new();
        let mut ar = KeyMap::new();
        for (scan, en, ar_letter) in [(1u16, "a", "\u{628}"), (2, "[", "\u{62c}"), (3, "]", "\u{62f}")] {
            us.insert(Key { scan, mods: 0 }, en);
            ar.insert(Key { scan, mods: 0 }, ar_letter);
        }
        fr.insert(Key { scan: 1, mods: 0 }, "a");
        fr.insert(Key { scan: 2, mods: ALTGR }, "[");
        assert_eq!(us.strain("a[ ]"), 0);
        assert_eq!(fr.strain("a[ ]"), 1 + 2, "AltGr costs 1 and a missing key 2; the space is free");
        let l = vec![
            Layout { name: "us".into(), lang: Lang::En, map: us },
            Layout { name: "fr".into(), lang: Lang::Fr, map: fr },
            Layout { name: "ar".into(), lang: Lang::Ar, map: ar },
        ];
        let nothing = |_: Lang, _: &str| false;
        // the French keyboard is the active one, but only the English layout can have typed `a[ ]`
        let r = forced("a[ ]", &l, Some(Lang::Fr), Lang::Ar, &nothing).expect("a reading");
        assert_eq!((r.from, r.text.as_str()), (0, "\u{628}\u{62c} \u{62f}"));
    }

    #[test]
    fn at_most_three_results_and_never_two_with_the_same_text() {
        let found = candidates("asd sdf", &layouts(), None, &known);
        assert!(found.len() <= MAX_CANDIDATES);
        for (i, a) in found.iter().enumerate() {
            assert!(found.iter().skip(i + 1).all(|b| b.text != a.text));
        }
    }
}
