//! Cleaning the frequency lists. They come from subtitles, so they hold plenty of things that are
//! not words a person wants suggested. Each rule below removes one kind of noise seen in the real lists.

use verba_core::{Lang, is_arabic, strip_arabic_marks};

use crate::fold::{is_core_arabic, is_latin_letter};

/// English contractions are split in the lists (`don't` -> `don` + `'t`). The stems are not words on their
/// own, so each becomes the whole contraction. (`won` and `can` are real words too: their `won't` and `can't`
/// are added from what is left of the `'t` count, see `WordList::build`.)
const EN_NT_STEMS: &[&str] = &[
    "don", "didn", "doesn", "isn", "aren", "wasn", "weren", "haven", "hasn", "hadn", "couldn", "wouldn", "shouldn", "ain", "mustn",
    "needn", "shan", "mightn",
];

/// Bare fragments of contractions that are not words (`'ll` and `'ve` lost their apostrophe in some lines).
const EN_FRAGMENTS: &[&str] = &["ll", "ve", "re"];

/// French elision stems cut from `qu'`, `jusqu'`, `aujourd'hui`... Nobody types them alone.
const FR_FRAGMENTS: &[&str] = &["ll", "ve", "qu", "puisqu", "quelqu", "jusqu", "lorsqu", "aujourd"];

/// Sounds and noises, not words to suggest.
const NOISE: &[&str] = &[
    "uh", "um", "umm", "uhh", "hm", "hmm", "mm", "mmm", "ah", "ahh", "aah", "eh", "huh", "ooh", "ha", "haha", "hahaha", "hehe",
    "shh", "zzz", "argh", "grr", "tsk", "psst",
];

/// Cleans one token. `None` when it is not a word worth suggesting.
///
/// Rules, each for a kind of noise found in the subtitle lists:
/// 1. 2 to 24 letters: single letters cannot be completed, very long strings are glitches.
/// 2. Digits, symbols and hyphens: `1st`, `80s`, `peut-être` cannot be matched by the letters typed before the caret.
/// 3. One script per word, from a known alphabet: drops a Greek omicron inside `you`, the broken text of a bad
///    encoding (`ãªtre`, `ظˆط`), Persian letters and Latin letters stuck to Arabic ones.
/// 4. An apostrophe only inside a word, once (`aujourd'hui`, `don't`): drops `'s`, `l'`, `c'`, `qu'`.
/// 5. No letter three times in a row: `aaah`, `noooo`, `mmm` are stretched sounds.
/// 6. A short list of noises (`uh`, `hmm`...).
pub(crate) fn clean_token(raw: &str) -> Option<String> {
    let raw = raw.replace('’', "'");
    let lower: String = raw.chars().flat_map(char::to_lowercase).collect();
    let arabic = lower.chars().any(is_arabic);
    // short vowels and the stretching tatweel are not part of the word (rule 3 is checked on what is left)
    let word = if arabic { strip_arabic_marks(&lower) } else { lower };
    let chars: Vec<char> = word.chars().collect();
    let n = chars.len();
    if !(2..=24).contains(&n) {
        return None;
    }
    if arabic {
        if !chars.iter().all(|c| is_core_arabic(*c)) {
            return None;
        }
    } else {
        let apostrophes = chars.iter().filter(|c| **c == '\'').count();
        if apostrophes > 1 || chars[0] == '\'' || chars[n - 1] == '\'' {
            return None;
        }
        if !chars.iter().all(|c| *c == '\'' || is_latin_letter(*c)) {
            return None;
        }
        if NOISE.contains(&word.as_str()) {
            return None;
        }
    }
    if chars.windows(3).any(|w| w[0] == w[1] && w[1] == w[2]) {
        return None;
    }
    Some(word)
}

/// What a language-specific pass does with a raw token before the general cleaning.
pub(crate) enum Special {
    /// A stem that is really a contraction: `don` -> `don't`.
    Contraction(String),
    /// The `'t` of every negative contraction (its count is shared out later).
    NotContraction,
    Drop,
    /// Nothing special.
    None,
}

pub(crate) fn special(lang: Lang, raw: &str) -> Special {
    let w = raw.to_lowercase();
    match lang {
        Lang::En => {
            if w == "'t" || w == "’t" {
                Special::NotContraction
            } else if EN_NT_STEMS.contains(&w.as_str()) {
                Special::Contraction(format!("{w}'t"))
            } else if EN_FRAGMENTS.contains(&w.as_str()) {
                Special::Drop
            } else {
                Special::None
            }
        }
        Lang::Fr => {
            if FR_FRAGMENTS.contains(&w.as_str()) {
                Special::Drop
            } else {
                Special::None
            }
        }
        Lang::Ar | Lang::Dz => Special::None,
    }
}
