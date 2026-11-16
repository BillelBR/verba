//! How words are compared: the same word typed a little differently still finds the same entry.
//!
//! The key of a word drops what a typist often leaves out:
//! - Latin: capitals and accents (`Été` -> `ete`, `œuvre` -> `oeuvre`);
//! - Arabic: short vowels and tatweel, the hamza on alef (`أ إ آ ٱ` -> `ا`), the dots of taa marbuta (`ة` -> `ه`)
//!   and of alef maqsura (`ى` -> `ي`), and the hamza seats on waw and yaa (`ؤ` -> `و`, `ئ` -> `ي`).
//!
//! Lists are sorted by key, so every word starting the same way sits together.

use verba_core::is_arabic_mark;

/// Accented letters kept in Latin words: the French set, which English loanwords use too.
pub(crate) const ACCENTED: &str = "àâäçéèêëîïôöùûüÿœæ";

/// A letter of the Latin script this engine accepts (lower case): a-z and the French accented ones.
pub(crate) fn is_latin_letter(c: char) -> bool {
    c.is_ascii_lowercase() || ACCENTED.contains(c)
}

/// The Arabic letters of everyday Modern Standard Arabic (hamza to ya, fa to ya, alef wasla).
/// Persian and Urdu letters, and the broken characters of a bad encoding, are not in it.
pub(crate) fn is_core_arabic(c: char) -> bool {
    matches!(c as u32, 0x0621..=0x063A | 0x0641..=0x064A | 0x0671)
}

/// The plain letter an accented or hamza'd letter stands for (`é` -> `e`, `أ` -> `ا`); any other letter is its own.
pub(crate) fn plain(c: char) -> char {
    match c {
        'à' | 'â' | 'ä' => 'a',
        'ç' => 'c',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'î' | 'ï' => 'i',
        'ô' | 'ö' => 'o',
        'ù' | 'û' | 'ü' => 'u',
        'ÿ' => 'y',
        'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
        'ة' => 'ه',
        'ى' | 'ئ' => 'ي',
        'ؤ' => 'و',
        c => c,
    }
}

/// True when `word` starts like `typed`, allowing the typist to have left out accents and hamza: each letter typed is
/// either the letter of the word, or the plain form of the accented letter the word has there. A letter typed WITH an
/// accent must be right (`thé` does not stand for `the`).
pub(crate) fn starts_leniently(word: &str, typed: &str) -> bool {
    let mut letters = word.chars();
    for t in typed.chars() {
        let Some(w) = letters.next() else { return false };
        if t != w && !(plain(t) == t && plain(w) == t) {
            return false;
        }
    }
    true
}

/// The comparison key of a word (see the module comment).
pub(crate) fn fold(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    for c in word.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'â' | 'ä' => out.push('a'),
            'ç' => out.push('c'),
            'é' | 'è' | 'ê' | 'ë' => out.push('e'),
            'î' | 'ï' => out.push('i'),
            'ô' | 'ö' => out.push('o'),
            'ù' | 'û' | 'ü' => out.push('u'),
            'ÿ' => out.push('y'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            'أ' | 'إ' | 'آ' | 'ٱ' => out.push('ا'),
            'ة' => out.push('ه'),
            'ى' | 'ئ' => out.push('ي'),
            'ؤ' => out.push('و'),
            c if is_arabic_mark(c) => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_keys_drop_accents_and_capitals() {
        assert_eq!(fold("Été"), "ete");
        assert_eq!(fold("œuvre"), "oeuvre");
        assert_eq!(fold("Français"), "francais");
    }

    #[test]
    fn a_typed_accent_must_be_right_but_a_missing_one_is_fine() {
        assert!(starts_leniently("très", "tres"));
        assert!(starts_leniently("été", "ete"));
        assert!(starts_leniently("très", "trè"));
        assert!(!starts_leniently("the", "thé"));
        // the accent is typed in one place and left out in another: the accent typed must still match
        assert!(starts_leniently("désolé", "deso"));
        assert!(!starts_leniently("désolé", "dès"));
        assert!(starts_leniently("أنا", "انا"));
        assert!(!starts_leniently("انا", "أنا"));
        assert!(!starts_leniently("الأمر", "الآ"));
        assert!(!starts_leniently("très", "tris"));
    }

    #[test]
    fn arabic_keys_drop_hamza_dots_and_marks() {
        assert_eq!(fold("أنا"), fold("انا"));
        assert_eq!(fold("مدرسة"), fold("مدرسه"));
        assert_eq!(fold("على"), fold("علي"));
        assert_eq!(fold("مَرْحَبًا"), "مرحبا");
        assert_eq!(fold("مسؤول"), fold("مسوول"));
    }
}
