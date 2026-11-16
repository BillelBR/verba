//! The part of Verba that knows nothing about Windows, Mac or Linux:
//! the languages, the text helpers and the contract every engine (local or API) follows.

/// The first languages. More come later, each with its own data pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    Fr,
    /// Modern Standard Arabic (فصحى).
    Ar,
    /// Algerian Darija written in Latin letters ("Arabizi": `wach`, `kifach`, `sa7a`, `3lach`, `9ahwa`).
    /// Left to right, Latin script, no standard spelling. Off until it is switched on (see `verba-local`).
    Dz,
}

impl Lang {
    /// Written right to left.
    pub fn is_rtl(self) -> bool {
        matches!(self, Lang::Ar)
    }

    /// The code used in file names and settings.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
            Lang::Ar => "ar",
            Lang::Dz => "dz",
        }
    }
}

/// One candidate shown above the text being typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub text: String,
    pub lang: Lang,
}

/// A correction offered for a word the user has just finished typing (`teh ` -> `the`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    /// The word as typed.
    pub original: String,
    /// What was typed right after it: a space, `, ` and so on. The fix replaces `original` and puts this back,
    /// so the text after the word is left as the user typed it.
    pub trailing: String,
    /// The corrected spellings, best first, with the same capital letters as `original`.
    pub candidates: Vec<Suggestion>,
}

/// What every engine can do. A local engine and an API engine implement the same thing,
/// so the rest of the app never needs to know which one is running.
pub trait Engine {
    /// Up to `max` ways to finish the word being typed, or to continue the text before the caret.
    /// The languages to try are the ones the engine has data for.
    fn suggest(&self, before_caret: &str, max: usize) -> Vec<Suggestion>;
}

/// Arabic letters and marks (the main Arabic blocks and presentation forms).
pub fn is_arabic(c: char) -> bool {
    matches!(c as u32, 0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}

/// Marks that do not change which word it is: short vowels, shadda, the stretching tatweel.
pub fn is_arabic_mark(c: char) -> bool {
    matches!(c as u32, 0x064B..=0x065F | 0x0670 | 0x0640 | 0x06D6..=0x06ED)
}

/// The word being typed: the letters right before the caret. Empty when the caret follows a space,
/// a digit or punctuation. An apostrophe ends the word (`l'ami` -> `ami`), which suits French.
pub fn current_word(before_caret: &str) -> &str {
    let mut start = before_caret.len();
    for (i, c) in before_caret.char_indices().rev() {
        if c.is_alphabetic() || is_arabic_mark(c) {
            start = i;
        } else {
            break;
        }
    }
    &before_caret[start..]
}

/// The digits Algerians type for Arabic letters: `2` (hamza), `3` (ain), `5` (kh), `6` (ta), `7` (ha), `8` (gh), `9` (qaf).
pub fn is_dz_digit(c: char) -> bool {
    matches!(c, '2' | '3' | '5' | '6' | '7' | '8' | '9')
}

/// Like `current_word`, but for Algerian Arabizi: the digit-letters belong to the word (`sa7`, `3la`, `9ahwa`).
/// Empty when there is no letter in it (a number), and a run of two digits stops the word (`ab33` is not a word).
/// For a word without digits it is the same as `current_word`.
pub fn current_word_dz(before_caret: &str) -> &str {
    let mut start = before_caret.len();
    let mut prev_digit = false;
    for (i, c) in before_caret.char_indices().rev() {
        let digit = is_dz_digit(c);
        if c.is_alphabetic() || is_arabic_mark(c) || (digit && !prev_digit) {
            start = i;
            prev_digit = digit;
        } else {
            break;
        }
    }
    let word = &before_caret[start..];
    if word.chars().any(|c| c.is_alphabetic()) { word } else { "" }
}

/// True when the word is written in Arabic script.
pub fn is_arabic_word(word: &str) -> bool {
    word.chars().any(is_arabic)
}

/// Removes the short vowels and tatweel, so a typed word can be matched against a plain list.
pub fn strip_arabic_marks(word: &str) -> String {
    word.chars().filter(|c| !is_arabic_mark(*c)).collect()
}

/// Gives a suggestion the same capital letters as what was typed: `He` -> `Hello`, `HE` -> `HELLO`.
pub fn match_case(typed: &str, suggestion: &str) -> String {
    let mut chars = typed.chars();
    let first_upper = chars.next().is_some_and(|c| c.is_uppercase());
    let rest_has_upper = chars.any(|c| c.is_uppercase());
    let typed_len = typed.chars().count();
    if first_upper && rest_has_upper && typed_len > 1 {
        suggestion.to_uppercase()
    } else if first_upper {
        let mut s = suggestion.chars();
        match s.next() {
            Some(f) => f.to_uppercase().collect::<String>() + s.as_str(),
            None => String::new(),
        }
    } else {
        suggestion.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_word_before_the_caret() {
        assert_eq!(current_word("hello wor"), "wor");
        assert_eq!(current_word("hello "), "");
        assert_eq!(current_word("l'ami"), "ami");
        assert_eq!(current_word("été"), "été");
        assert_eq!(current_word("مرحبا بك"), "بك");
        assert_eq!(current_word("what is 12"), "");
    }

    #[test]
    fn darija_digit_letters_belong_to_the_word() {
        assert_eq!(current_word_dz("wach rani sa7"), "sa7");
        assert_eq!(current_word_dz("3la"), "3la");
        assert_eq!(current_word_dz("ya 9ahwa"), "9ahwa");
        assert_eq!(current_word_dz("ma3lich"), "ma3lich");
        // numbers, digit runs and the usual words
        assert_eq!(current_word_dz("what is 12"), "");
        assert_eq!(current_word_dz("room 35"), "");
        assert_eq!(current_word_dz("ab33"), "");
        assert_eq!(current_word_dz("x 100m"), "m");
        assert_eq!(current_word_dz("hello wor"), "wor");
        assert_eq!(current_word_dz("l'ami"), "ami");
        assert_eq!(current_word_dz("hello "), "");
        assert_eq!(current_word_dz("مرحبا بك"), "بك");
    }

    #[test]
    fn arabic_marks_do_not_break_a_word() {
        let w = "مَرْحَ";
        assert_eq!(current_word(w), w);
        assert_eq!(strip_arabic_marks(w), "مرح");
    }

    #[test]
    fn keeps_the_capitals() {
        assert_eq!(match_case("He", "hello"), "Hello");
        assert_eq!(match_case("HE", "hello"), "HELLO");
        assert_eq!(match_case("he", "hello"), "hello");
        assert_eq!(match_case("H", "hello"), "Hello");
    }

    #[test]
    fn tells_the_script() {
        assert!(is_arabic_word("مرحبا"));
        assert!(!is_arabic_word("hello"));
    }
}
