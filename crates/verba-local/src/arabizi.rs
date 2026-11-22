//! Arabizi to Arabic: Arabic written with Latin letters and digits (`wach rak`, `3lach`, `sa7a`, `9ahwa`) turned into Arabic letters.
//!
//! Arabizi has no standard spelling and leaves most vowels out, so a letter-by-letter table gives words nobody writes (`kifach` is `كيفاش`,
//! and the table alone says `كفاش`). The work is done in two steps, the same ones a reader does:
//!
//! 1. **Look the word up.** Both sides are cut down to their consonants, with the letters that sound alike as one (`kifach` and `كيفاش` are
//!    both `ك ف ش`; `3lach` and `علاش` are `ع ل ش`; Latin `s` may be `س` or `ص`, `t` may be `ت` or `ط`, `h` may be `ه` or `ح`...). The Arabic
//!    words of the word lists with that skeleton are the candidates, and the one that is written most often (words of Darija first) wins.
//! 2. **When no word is found,** the letters are written by rules (`a` is `ا`, `i` is `ي`, `ou` is `و`, an `e` is left out) and the result is
//!    a guess, spelled the way the rules say, not a word anyone has checked.
//!
//! A word that is only digits is left alone. Punctuation is turned into the Arabic one (`,` `?` `;`). The engine builds the index of
//! skeletons at the first use, from the Arabic list and the Darija list in Arabic letters.

use std::collections::HashMap;

/// A score of this or more marks a word of Darija written for Verba (the engine adds it); `word_scored` takes it off again.
pub const SEED: f32 = 10.0;

/// The lowest score (frequency plus the fit of the vowels) at which a word of the lists is taken instead of the spelling by rules.
const MIN_FIT: f32 = -0.3;

/// A key may have at most this many words kept: the best ones.
const KEEP: usize = 12;
/// The most Latin spellings of one word that are tried (letters with two readings multiply).
const MAX_COMBINATIONS: usize = 96;

/// One consonant of an Arabic word (its class) and the long-vowel letters written after it: `ا` for a long `a` (and the final `ة`), `و`, `ي`.
struct ArSlot {
    class: char,
    after: Vec<char>,
}

fn arabic_slots(word: &str) -> Vec<ArSlot> {
    let chars: Vec<char> = word.chars().filter(|c| !is_mark(*c)).collect();
    let mut out: Vec<ArSlot> = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        let last = i + 1 == chars.len();
        // a vowel letter inside or at the end of the word belongs to the consonant before it; at the start it is a consonant of its own
        let vowel = match c {
            'ا' | 'أ' | 'إ' | 'آ' | 'ٱ' | 'ء' => Some('ا'),
            'و' | 'ؤ' => Some('و'),
            'ي' | 'ى' | 'ئ' => Some('ي'),
            'ة' if last => Some('ا'),
            _ => None,
        };
        if let (Some(v), true) = (vowel, i > 0) {
            if c != 'ء' {
                if let Some(slot) = out.last_mut() {
                    slot.after.push(v);
                }
            }
            continue;
        }
        let class = match c {
            'ا' | 'أ' | 'إ' | 'آ' | 'ٱ' | 'ء' => 'ا',
            'و' | 'ؤ' => 'و',
            'ي' | 'ى' | 'ئ' => 'ي',
            'ة' => 'ه',
            'ب' | 'پ' => 'ب',
            'ت' | 'ث' | 'ط' => 'ت',
            'د' | 'ذ' | 'ض' | 'ظ' => 'د',
            'س' | 'ص' => 'س',
            'ش' | 'چ' => 'ش',
            'ف' | 'ڤ' => 'ف',
            'ق' | 'ڨ' | 'گ' => 'ق',
            'ك' | 'ک' => 'ك',
            c if ('\u{0621}'..='\u{064A}').contains(&c) => c,
            _ => return Vec::new(),
        };
        // a doubled letter is one
        if out.last().is_some_and(|s| s.class == class) {
            continue;
        }
        out.push(ArSlot { class, after: Vec::new() });
    }
    out
}

/// The skeleton of an Arabic word: its consonants, the letters that sound alike as one, the long vowels left out except at the start.
pub fn arabic_skeleton(word: &str) -> String {
    arabic_slots(word).iter().map(|s| s.class).collect()
}

fn is_mark(c: char) -> bool {
    matches!(c, '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{0640}')
}

/// What a Latin letter or digit can be written as, as skeleton classes. `None` is a vowel (left out of the skeleton, except at the start).
fn latin_classes(c: char) -> Option<&'static [char]> {
    Some(match c {
        'b' | 'p' => &['ب'],
        't' | '6' => &['ت'],
        'j' => &['ج'],
        '7' => &['ح'],
        'h' => &['ه', 'ح'],
        '5' => &['خ'],
        'd' => &['د'],
        'r' => &['ر'],
        'z' => &['ز', 'د'],
        's' => &['س'],
        '3' => &['ع'],
        '8' => &['غ'],
        'f' | 'v' => &['ف'],
        '9' | 'q' => &['ق'],
        'k' => &['ك', 'ق'],
        'g' => &['ق', 'ك', 'غ', 'ج'],
        'c' => &['ك', 'س'],
        'x' => &['ك'],
        'l' => &['ل'],
        'm' => &['م'],
        'n' => &['ن'],
        _ => return None,
    })
}

/// A token of the text: a word of Latin letters and digits (with apostrophes inside), or whatever lies between two of them.
#[derive(Debug, PartialEq)]
enum Piece<'a> {
    Word(&'a str),
    Other(&'a str),
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '\'' || c == '\u{2019}'
}

fn pieces(text: &str) -> Vec<Piece<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut in_word = None;
    for (i, c) in text.char_indices() {
        let w = is_word_char(c);
        match in_word {
            None => in_word = Some(w),
            Some(prev) if prev != w => {
                let s = &text[start..i];
                out.push(if prev { Piece::Word(s) } else { Piece::Other(s) });
                start = i;
                in_word = Some(w);
            }
            _ => {}
        }
    }
    if let Some(prev) = in_word {
        let s = &text[start..];
        out.push(if prev { Piece::Word(s) } else { Piece::Other(s) });
    }
    out
}

/// The letters of a word as the parts the rules work on: `kh`, `gh`, `ch`, `sh` are one consonant, `ou` and `oo` one vowel.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Part {
    /// A consonant or digit-letter, as the character that decides its classes (`kh` is `5`, `gh` is `8`, `ch` is `$`).
    Cons(char),
    Vowel(char),
    /// `w` and `y`, which are consonants at the start of a word and vowels inside it.
    Glide(char),
    /// The hamza digit.
    Hamza,
}

fn parts(word: &str) -> Vec<Part> {
    let chars: Vec<char> = word.chars().filter(|c| *c != '\'' && *c != '\u{2019}').flat_map(char::to_lowercase).collect();
    let mut out: Vec<Part> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let part = match (c, next) {
            ('k', Some('h')) => {
                i += 1;
                Part::Cons('5')
            }
            ('g', Some('h')) => {
                i += 1;
                Part::Cons('8')
            }
            ('c', Some('h')) | ('s', Some('h')) => {
                i += 1;
                Part::Cons('$')
            }
            // `ou` at the start is a w (ouach, ouled), inside a word it is the vowel u
            ('o', Some('u')) | ('o', Some('o')) => {
                let at_start = i == 0;
                i += 1;
                if at_start { Part::Glide('w') } else { Part::Vowel('u') }
            }
            ('a' | 'e' | 'i' | 'o' | 'u', _) => Part::Vowel(c),
            ('w' | 'y', _) => Part::Glide(c),
            ('2', _) => Part::Hamza,
            _ => Part::Cons(c),
        };
        // a doubled consonant is one
        if let (Part::Cons(a), Some(Part::Cons(b))) = (part, out.last().copied()) {
            if a == b {
                i += 1;
                continue;
            }
        }
        out.push(part);
        i += 1;
    }
    out
}

/// One consonant of a Latin word (the classes it may be) and the vowels written after it (`a e i o u`).
struct LatSlot {
    classes: Vec<char>,
    after: Vec<char>,
}

fn latin_slots(word: &str) -> Vec<LatSlot> {
    let mut out: Vec<LatSlot> = Vec::new();
    for (i, p) in parts(word).iter().enumerate() {
        match *p {
            Part::Cons('$') => out.push(LatSlot { classes: vec!['ش'], after: Vec::new() }),
            Part::Cons('5') => out.push(LatSlot { classes: vec!['خ'], after: Vec::new() }),
            Part::Cons('8') => out.push(LatSlot { classes: vec!['غ'], after: Vec::new() }),
            Part::Cons(c) => {
                if let Some(k) = latin_classes(c) {
                    out.push(LatSlot { classes: k.to_vec(), after: Vec::new() });
                }
            }
            Part::Vowel(_) | Part::Hamza if i == 0 => out.push(LatSlot { classes: vec!['ا'], after: Vec::new() }),
            Part::Glide('w') if i == 0 => out.push(LatSlot { classes: vec!['و'], after: Vec::new() }),
            Part::Glide(_) if i == 0 => out.push(LatSlot { classes: vec!['ي'], after: Vec::new() }),
            Part::Vowel(v) => {
                if let Some(s) = out.last_mut() {
                    s.after.push(v);
                }
            }
            Part::Glide(g) => {
                if let Some(s) = out.last_mut() {
                    s.after.push(if g == 'w' { 'u' } else { 'i' });
                }
            }
            Part::Hamza => {}
        }
    }
    out
}

/// The skeletons a Latin word may stand for: one set of classes per consonant, and every combination (at most `MAX_COMBINATIONS`).
fn latin_keys(word: &str) -> Vec<String> {
    let slots = latin_slots(word);
    if slots.is_empty() {
        return Vec::new();
    }
    let mut keys = vec![String::new()];
    for slot in &slots {
        let mut next = Vec::with_capacity(keys.len() * slot.classes.len());
        'outer: for k in &keys {
            for &c in &slot.classes {
                if next.len() >= MAX_COMBINATIONS {
                    break 'outer;
                }
                // a doubled class is one, as in the Arabic skeleton
                let mut k2 = k.clone();
                if !k2.ends_with(c) {
                    k2.push(c);
                }
                next.push(k2);
            }
        }
        keys = next;
    }
    keys.sort();
    keys.dedup();
    keys
}

/// How well the vowels of an Arabic word agree with the vowels of the Latin word, as a number added to the word's score. The vowel letters
/// of Arabic are only some of the vowels of the Latin spelling (`a` and `e` are often not written), so only what disagrees counts much:
/// a long vowel the Latin does not have (`نحبو` for `n7ab`), a different one (`كبار` for `kbira`), or a word that ends in a vowel on one side
/// only. A vowel letter that the Latin has too is a small plus (`معاك` for `m3ak`, `شوف` for `chouf`).
fn vowel_fit(latin: &[LatSlot], arabic: &str) -> f32 {
    let ar = arabic_slots(arabic);
    if ar.len() != latin.len() {
        return 0.0;
    }
    let compatible = |letter: char, lat: &[char]| match letter {
        'ا' => lat.iter().any(|v| matches!(v, 'a' | 'e')),
        'و' => lat.iter().any(|v| matches!(v, 'u' | 'o')),
        _ => lat.iter().any(|v| matches!(v, 'i' | 'e')),
    };
    let mut score = 0.0;
    for (i, (l, a)) in latin.iter().zip(&ar).enumerate() {
        let last = i + 1 == ar.len();
        for (n, &letter) in a.after.iter().enumerate() {
            if a.after[..n].contains(&letter) {
                // the same vowel letter twice after one consonant (بزااف): a spelling nobody writes
                score -= 0.3;
            } else if compatible(letter, &l.after) {
                score += 0.12;
            } else {
                score -= if last { 0.6 } else { 0.3 };
            }
        }
        // a vowel the Latin spells out that Arabic always writes, and the Arabic does not
        if l.after.contains(&'i') && !a.after.contains(&'ي') {
            score -= 0.1;
        }
        if l.after.iter().any(|v| matches!(v, 'u' | 'o')) && !a.after.contains(&'و') {
            score -= 0.1;
        }
        if last {
            let latin_ends_in_vowel = l.after.iter().any(|v| matches!(v, 'a' | 'i' | 'u' | 'o'));
            if latin_ends_in_vowel && a.after.is_empty() {
                score -= 0.4;
            }
        }
    }
    score
}

/// The letters written by rules, for a word the index does not know: a guess.
fn spell_by_rules(word: &str) -> String {
    let parts = parts(word);
    let mut out = String::new();
    for (i, p) in parts.iter().enumerate() {
        match *p {
            Part::Cons('$') => out.push('ش'),
            Part::Cons('5') => out.push('خ'),
            Part::Cons('8') => out.push('غ'),
            Part::Cons('g') => out.push('ڨ'),
            Part::Cons('v') => out.push('ڤ'),
            Part::Cons('p') => out.push('پ'),
            Part::Cons('c') => out.push('ك'),
            Part::Cons('x') => out.push_str("كس"),
            Part::Cons('h') => out.push('ه'),
            Part::Cons('6') => out.push('ط'),
            Part::Cons('9') => out.push('ق'),
            Part::Cons('q') => out.push('ق'),
            Part::Cons(c) => {
                if let Some(k) = latin_classes(c) {
                    out.push(k[0]);
                }
            }
            Part::Vowel(v) => match (i == 0, v) {
                (true, _) => out.push('ا'),
                (false, 'a') => out.push('ا'),
                (false, 'i') => out.push('ي'),
                (false, 'u' | 'o') => out.push('و'),
                // an e is not written
                _ => {}
            },
            Part::Glide('w') => out.push('و'),
            Part::Glide(_) => out.push('ي'),
            Part::Hamza => out.push(if i == 0 { 'أ' } else { 'ء' }),
        }
    }
    out
}

/// What the index holds: for each skeleton, the best Arabic words with a score.
#[derive(Default)]
pub struct ArabiziIndex {
    by_key: HashMap<String, Vec<(String, f32)>>,
}

impl ArabiziIndex {
    /// Builds the index from `(word, score)` pairs: a score is how common the word is (any scale, only the order counts).
    pub fn build(words: impl Iterator<Item = (String, f32)>) -> Self {
        let mut by_key: HashMap<String, Vec<(String, f32)>> = HashMap::new();
        for (word, score) in words {
            let key = arabic_skeleton(&word);
            if key.is_empty() || word.chars().count() > 20 {
                continue;
            }
            let list = by_key.entry(key).or_default();
            if list.iter().any(|(w, _)| *w == word) {
                continue;
            }
            list.push((word, score));
            list.sort_by(|a, b| b.1.total_cmp(&a.1));
            list.truncate(KEEP);
        }
        Self { by_key }
    }

    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// One word turned into Arabic letters, whether a word of the lists was found for it, and how sure that is (the score of the word: `SEED` or more when it is
    /// a word of Darija written for Verba).
    pub fn word_scored(&self, latin: &str) -> (String, bool, f32) {
        let slots = latin_slots(latin);
        let best = latin_keys(latin)
            .iter()
            .filter_map(|k| self.by_key.get(k))
            .flat_map(|list| list.iter())
            .map(|(w, score)| {
                let (base, seed) = if *score >= SEED { (*score - SEED, true) } else { (*score, false) };
                (w, base + vowel_fit(&slots, w), seed)
            })
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            // every word with these consonants has vowels that disagree with the Latin: the rules are a better guess than a wrong word
            Some((_, score, false)) if score < MIN_FIT => (spell_by_rules(latin), false, 0.0),
            Some((w, score, seed)) => (w.clone(), true, if seed { SEED + score } else { score }),
            None => (spell_by_rules(latin), false, 0.0),
        }
    }

    /// One word turned into Arabic letters, and whether a word of the lists was found for it.
    pub fn word(&self, latin: &str) -> (String, bool) {
        let (w, found, _) = self.word_scored(latin);
        (w, found)
    }

    /// A text turned into Arabic letters: `(text, words turned, words found in the lists)`. A word that is only digits stays; punctuation
    /// becomes the Arabic one. `keep` says which words are not Arabizi and stay as they are (a French word in a Darija sentence).
    pub fn convert(&self, text: &str, keep: impl Fn(&str) -> bool) -> (String, usize, usize) {
        let (mut out, mut turned, mut found) = (String::new(), 0, 0);
        for piece in pieces(text) {
            match piece {
                Piece::Word(w) => {
                    let (ar, known, score) = self.word_scored(w);
                    // a word the caller keeps (French) stays, unless it is also a word of Darija
                    if !w.chars().any(|c| c.is_ascii_alphabetic()) || (keep(w) && score < SEED) {
                        out.push_str(w);
                    } else {
                        turned += 1;
                        found += usize::from(known);
                        out.push_str(&ar);
                    }
                }
                Piece::Other(o) => out.extend(o.chars().map(|c| match c {
                    ',' => '\u{060C}',
                    '?' => '\u{061F}',
                    ';' => '\u{061B}',
                    c => c,
                })),
            }
        }
        (out, turned, found)
    }
}

/// Whether a text looks like Arabizi: some word has a digit-letter inside (`3lach`, `sa7a`), or at least half of its words are Darija
/// (`known`). A plain English or French text is not.
pub fn looks_like_arabizi(text: &str, known: impl Fn(&str) -> bool) -> bool {
    let words: Vec<&str> = pieces(text).into_iter().filter_map(|p| if let Piece::Word(w) = p { Some(w) } else { None }).collect();
    let letters = |w: &str| w.chars().any(|c| c.is_ascii_alphabetic());
    let words: Vec<&str> = words.into_iter().filter(|w| letters(w)).collect();
    if words.is_empty() {
        return false;
    }
    let digit_letter = |w: &str| {
        let cs: Vec<char> = w.chars().collect();
        cs.iter().enumerate().any(|(i, c)| {
            matches!(c, '2' | '3' | '5' | '6' | '7' | '8' | '9')
                && (cs.get(i.wrapping_sub(1)).is_some_and(|p| p.is_ascii_alphabetic()) || cs.get(i + 1).is_some_and(|n| n.is_ascii_alphabetic()))
        })
    };
    words.iter().any(|w| digit_letter(w)) || words.iter().filter(|w| known(&w.to_lowercase())).count() * 2 >= words.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> ArabiziIndex {
        ArabiziIndex::build(
            [("كيفاش", 9.0), ("واش", 9.0), ("علاش", 8.0), ("بزاف", 8.0), ("كفاءة", 1.0), ("صحة", 7.0), ("قهوة", 7.0), ("راني", 6.0), ("نتا", 6.0), ("حبيبي", 5.0)]
                .into_iter()
                .map(|(w, s)| (w.to_string(), s)),
        )
    }

    #[test]
    fn an_arabic_word_and_its_arabizi_have_the_same_skeleton() {
        assert_eq!(arabic_skeleton("كيفاش"), "كفش");
        assert_eq!(arabic_skeleton("علاش"), "علش");
        assert_eq!(arabic_skeleton("بزاف"), "بزف");
        // a word with vowel marks, the long vowel at the start kept, a doubled letter one
        assert_eq!(arabic_skeleton("أَنَا"), "ان");
        assert_eq!(arabic_skeleton("وَاش"), "وش");
        assert!(latin_keys("kifach").contains(&"كفش".to_string()));
        assert!(latin_keys("3lach").contains(&"علش".to_string()));
        assert!(latin_keys("bezzaf").contains(&"بزف".to_string()));
        assert!(latin_keys("ouach").contains(&"وش".to_string()), "ou at the start is a w");
        assert!(latin_keys("sa7a").contains(&"سح".to_string()));
        assert!(latin_keys("9ahwa").contains(&"قه".to_string()));
    }

    #[test]
    fn words_of_the_index_are_found_by_their_arabizi() {
        let i = index();
        for (latin, arabic) in [("kifach", "كيفاش"), ("kifech", "كيفاش"), ("wach", "واش"), ("ouach", "واش"), ("3lach", "علاش"), ("bezzaf", "بزاف"), ("bzaf", "بزاف"), ("sa7a", "صحة"), ("9ahwa", "قهوة"), ("rani", "راني"), ("nta", "نتا"), ("7bibi", "حبيبي"), ("hbibi", "حبيبي")] {
            assert_eq!(i.word(latin), (arabic.to_string(), true), "{latin}");
        }
    }

    #[test]
    fn a_word_that_is_not_in_the_index_is_written_by_the_rules() {
        let i = index();
        let (w, known) = i.word("khouya");
        assert!(!known);
        assert_eq!(w, "خويا");
        assert_eq!(i.word("mchi").0, "مشي");
        assert_eq!(i.word("3chiri").0, "عشيري");
    }

    #[test]
    fn a_text_keeps_its_numbers_and_gets_arabic_punctuation() {
        let i = index();
        let (t, turned, found) = i.convert("wach rak, 2024? kifach;", |_| false);
        assert_eq!(t, "واش راك\u{060C} 2024\u{061F} كيفاش\u{061B}");
        assert_eq!((turned, found), (3, 2));
        // a word the caller keeps (French) stays Latin
        assert_eq!(i.convert("wach merci", |w| w == "merci").0, "واش merci");
    }

    #[test]
    fn what_looks_like_arabizi_and_what_does_not() {
        let known = |w: &str| matches!(w, "wach" | "kifach" | "rak");
        assert!(looks_like_arabizi("3lach", known), "a digit-letter");
        assert!(looks_like_arabizi("wach rak", known), "Darija words");
        assert!(!looks_like_arabizi("hello there", known));
        assert!(!looks_like_arabizi("call me at 3 pm", known), "a digit alone is a number");
        assert!(!looks_like_arabizi("2024", known));
    }
}
