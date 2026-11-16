//! Deciding, from two readings of the text before the caret, which word the user has just finished typing.
//!
//! Only a word that was being typed in the previous reading and is now followed by a separator counts. Text that appears
//! all at once (a paste, a change of window) never matches, so it is not learned. Pure functions: no Windows calls.

use verba_core::is_arabic_mark;

fn in_word(c: char) -> bool {
    c.is_alphabetic() || is_arabic_mark(c) || c == '\'' || c == '\u{2019}' || c == '-'
}

/// The run of word characters at the end of `s`, without an apostrophe or hyphen at its start.
fn tail_word(s: &str) -> &str {
    let mut start = s.len();
    for (i, c) in s.char_indices().rev() {
        if in_word(c) {
            start = i;
        } else {
            break;
        }
    }
    s[start..].trim_start_matches(['\'', '\u{2019}', '-'])
}

/// The word that was just finished, and the separators typed after it, when `now` is `prev` plus the end of a word.
/// `None` when nothing was finished, or when what followed the word was more than spaces and punctuation (a digit: an
/// identifier, a number).
pub fn finished_word(prev: &str, now: &str) -> Option<(String, String)> {
    let head = now.trim_end_matches(|c: char| !in_word(c));
    let sep = &now[head.len()..];
    if sep.is_empty() || sep.chars().any(char::is_alphanumeric) {
        return None;
    }
    let word = tail_word(head);
    if word.is_empty() || !prev.chars().last().is_some_and(in_word) {
        return None;
    }
    // the word being typed in the previous reading must be the start of the word that is finished now
    // (a fast typist can finish several letters between two readings)
    let typed = tail_word(prev).to_lowercase();
    if typed.is_empty() || !word.to_lowercase().starts_with(&typed) {
        return None;
    }
    Some((word.to_string(), sep.to_string()))
}

/// Whether the separators end a sentence: that is when what was collected is learned.
pub fn ends_sentence(sep: &str) -> bool {
    sep.chars().any(|c| matches!(c, '.' | '!' | '?' | '\u{061F}' | '\n' | '\r'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_followed_by_a_space_is_finished() {
        assert_eq!(finished_word("I would lik", "I would like "), Some(("like".into(), " ".into())));
        assert_eq!(finished_word("hello", "hello, "), Some(("hello".into(), ", ".into())));
    }

    #[test]
    fn nothing_is_finished_while_typing_or_after_a_space() {
        assert_eq!(finished_word("I wou", "I woul"), None);
        assert_eq!(finished_word("I would ", "I would  "), None);
        assert_eq!(finished_word("", "hello "), None);
    }

    #[test]
    fn pasted_text_or_another_window_is_not_learned() {
        assert_eq!(finished_word("my name is Sno", "completely different text "), None);
        assert_eq!(finished_word("abc", "xyz "), None);
    }

    #[test]
    fn a_word_with_a_digit_after_it_is_not_learned() {
        assert_eq!(finished_word("abc", "abc1 "), None);
        assert_eq!(finished_word("ref", "ref2024 "), None);
    }

    #[test]
    fn arabic_and_apostrophes_work() {
        assert_eq!(finished_word("مرح", "مرحبا "), Some(("مرحبا".into(), " ".into())));
        assert_eq!(finished_word("don", "don't "), Some(("don't".into(), " ".into())));
    }

    #[test]
    fn sentence_ends_are_found() {
        assert!(ends_sentence(". "));
        assert!(ends_sentence("\n"));
        assert!(ends_sentence("\u{061F} "));
        assert!(!ends_sentence(", "));
        assert!(!ends_sentence(" "));
    }
}
