//! The part of Verba that knows nothing about Windows, Mac or Linux:
//! the languages, the text helpers and the contract every engine (local or API) follows.

/// The first languages. More come later, each with its own data pack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    Fr,
    /// Modern Standard Arabic (فصحى).
    Ar,
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
        }
    }
}

/// One candidate shown above the text being typed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub text: String,
    pub lang: Lang,
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
