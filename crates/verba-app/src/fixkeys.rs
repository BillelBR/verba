//! "Wrong keyboard": text typed with the wrong layout, put right.
//!
//! The glue between the layouts Windows has installed (`verba_win::layouts`) and the pure work of `verba_local::wrongkeys`: it builds
//! the key tables, asks the word lists which reading is made of real words, and says what the popup should show. Nothing here touches
//! Windows' windows or keys, so it is tested.

use verba_core::Lang;
use verba_local::LocalEngine;
use verba_local::wrongkeys::{self, Key, KeyMap, Layout};
use verba_win::layouts::InstalledLayout;

/// The longest text put right at once (a long paragraph, not a whole document).
pub const MAX_CHARS: usize = 6000;
/// How much of a selection is read: a little more than the limit, so that a selection that is too long is told so.
pub const READ_CAP: usize = MAX_CHARS + 1000;

/// One reading of the selected text, as the popup shows it.
#[derive(Debug, PartialEq)]
pub struct Reading {
    pub text: String,
    /// The language the text is in now.
    pub lang: Lang,
    /// `EN → AR`: typed on the English keyboard, meant for the Arabic one.
    pub label: String,
}

/// What pressing the shortcut leads to.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Only one language of keyboards is installed in Windows: there is nothing to turn the text into.
    NeedTwoLanguages,
    /// No other keyboard turns the text into words (it is right as it is, or it is not text typed on a keyboard).
    NothingToFix,
    Found(Vec<Reading>),
}

/// The key tables of the installed layouts that have a word list (English, French, Arabic).
pub fn layouts_from(installed: &[InstalledLayout]) -> Vec<Layout> {
    installed
        .iter()
        .filter_map(|l| {
            let lang = l.lang?;
            let mut map = KeyMap::new();
            // the plain keys come first in the list, then Shift, then AltGr: the first key that gives a text is the one used to go back
            for (scan, mods, text) in &l.keys {
                map.insert(Key { scan: *scan, mods: *mods }, text);
            }
            (!map.is_empty()).then(|| Layout { name: l.name.clone(), lang, map })
        })
        .collect()
}

/// The language the user wants a text in when no reading is made of real words (a name, a dialect word that the lists lack): Arabic.
const LAST_RESORT: Lang = Lang::Ar;

/// `text` was selected: how can it be read as typed on the wrong keyboard? `active` is the language of the keyboard that was active when
/// the shortcut was pressed: of two equal readings the one typed on it comes first, and it is the source when nothing is made of real words.
pub fn offer(text: &str, installed: &[InstalledLayout], engine: &LocalEngine, active: Option<Lang>) -> Outcome {
    let layouts = layouts_from(installed);
    let mut langs: Vec<Lang> = layouts.iter().map(|l| l.lang).collect();
    langs.sort_by_key(|l| l.code());
    langs.dedup();
    if langs.len() < 2 {
        return Outcome::NeedTwoLanguages;
    }
    let known = |lang: Lang, word: &str| engine.is_word_of(lang, word);
    let mut found = wrongkeys::candidates(text, &layouts, active, &known);
    if found.is_empty() && text.chars().any(char::is_alphabetic) && !text.chars().any(verba_core::is_arabic) {
        // letters of another script, and no reading made of real words: the user wants Arabic, so give it, words or not
        found.extend(wrongkeys::forced(text, &layouts, active, LAST_RESORT, &known));
    }
    if found.is_empty() {
        return Outcome::NothingToFix;
    }
    Outcome::Found(
        found
            .into_iter()
            .map(|c| Reading {
                label: format!("{} \u{2192} {}", layouts[c.from].lang.code().to_uppercase(), layouts[c.to].lang.code().to_uppercase()),
                lang: layouts[c.to].lang,
                text: c.text,
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use verba_local::WordList;

    /// A layout whose keys 0x10.. give `plain`, 0x39 the space bar.
    fn layout(lang: Option<Lang>, plain: &str) -> InstalledLayout {
        let mut keys: Vec<(u16, u8, String)> = plain.chars().enumerate().map(|(i, c)| (0x10 + i as u16, 0, c.to_string())).collect();
        keys.push((0x39, 0, " ".to_string()));
        InstalledLayout { name: "test".into(), lang, keys }
    }

    fn engine() -> LocalEngine {
        let mut e = LocalEngine::new();
        e.add(Lang::En, WordList::parse("hello 900\nworld 800\nthe 700\n", 100));
        e.add(Lang::Ar, WordList::parse("مرحبا 900\nعالم 800\n", 100));
        e
    }

    // The keys 0x10.. of two small layouts. The Arabic one gives the letters of `مرحبا عالم` (m r h b a, then ayn and lam), and
    // the English one the letters q w e r t y u for the same keys: so `مرحبا عالم` typed on the English layout is `qwert ytuq`.
    const EN: &str = "qwertyu";
    const AR: &str = "مرحباعل";

    #[test]
    fn arabic_typed_on_an_english_keyboard_is_offered_back_as_arabic() {
        let installed = vec![layout(Some(Lang::En), EN), layout(Some(Lang::Ar), AR)];
        match offer("qwert ytuq", &installed, &engine(), None) {
            Outcome::Found(found) => {
                assert_eq!(found[0], Reading { text: "مرحبا عالم".into(), lang: Lang::Ar, label: "EN \u{2192} AR".into() });
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn one_language_of_keyboards_is_not_enough() {
        let installed = vec![layout(Some(Lang::En), EN), layout(Some(Lang::En), EN), layout(None, "abcdefghij")];
        assert_eq!(offer("hello", &installed, &engine(), None), Outcome::NeedTwoLanguages);
        assert_eq!(offer("hello", &[], &engine(), None), Outcome::NeedTwoLanguages);
    }

    #[test]
    fn text_that_is_right_is_left_alone() {
        let installed = vec![layout(Some(Lang::En), EN), layout(Some(Lang::Ar), AR)];
        assert_eq!(offer("hello world", &installed, &engine(), None), Outcome::NothingToFix);
        assert_eq!(offer("مرحبا عالم", &installed, &engine(), None), Outcome::NothingToFix);
    }

    #[test]
    fn a_name_that_no_list_has_is_still_turned_into_arabic_from_the_active_keyboard() {
        let installed = vec![layout(Some(Lang::En), EN), layout(Some(Lang::Fr), EN), layout(Some(Lang::Ar), AR)];
        // `qwq` is no word in any list, and nothing reads as words, but the user wants Arabic
        match offer("qwq", &installed, &engine(), Some(Lang::Fr)) {
            Outcome::Found(found) => {
                assert_eq!(found.len(), 1);
                assert_eq!((found[0].text.as_str(), found[0].lang, found[0].label.as_str()), ("\u{645}\u{631}\u{645}", Lang::Ar, "FR \u{2192} AR"));
            }
            other => panic!("{other:?}"),
        }
        // Arabic letters are already Arabic: nothing to turn them into
        assert_eq!(offer("\u{645}\u{631}\u{645}", &installed, &engine(), Some(Lang::Fr)), Outcome::NothingToFix);
        // no letters at all: nothing to do
        assert_eq!(offer("12 34", &installed, &engine(), None), Outcome::NothingToFix);
    }

    #[test]
    fn a_layout_without_a_language_or_without_keys_is_left_out() {
        let empty = InstalledLayout { name: "empty".into(), lang: Some(Lang::Fr), keys: Vec::new() };
        let installed = vec![layout(None, "abc"), empty, layout(Some(Lang::En), EN)];
        assert_eq!(layouts_from(&installed).len(), 1);
    }

    /// The keyboards of the author's PC (English US, French AZERTY, Arabic 102 AZERTY) and the real word lists: sentences are "typed" on
    /// every wrong keyboard (by the same keys) and must come back. Only reads layouts and files; no key is sent.
    ///   cargo test -p verba-app fixkeys -- --ignored --nocapture
    #[test]
    #[ignore = "needs the three keyboards installed and data/words (scripts/fetch-data.ps1)"]
    fn sentences_typed_on_every_wrong_keyboard_come_back_with_the_real_layouts_and_lists() {
        let installed = verba_win::layouts::installed();
        let layouts = layouts_from(&installed);
        assert!(layouts.len() >= 3, "needs English, French and Arabic keyboards; found {}", layouts.len());
        let mut engine = LocalEngine::load_dir(std::path::Path::new("../../data/words")).expect("data/words");
        // the app always adds the built-in Darija lists
        engine.add_dz_seed();
        let samples: [(Lang, &str); 9] = [
            (Lang::Ar, "مرحبا كيف حالك اليوم"),
            (Lang::Ar, "أريد أن أرسل لك الملف قبل الساعة الخامسة"),
            (Lang::Ar, "شكرا جزيلا على مساعدتك"),
            (Lang::Fr, "bonjour, comment allez-vous aujourd'hui ?"),
            (Lang::Fr, "je voudrais vous envoyer le fichier avant cinq heures."),
            (Lang::Fr, "merci beaucoup pour votre aide, à demain !"),
            (Lang::En, "can you send me the file before five o'clock?"),
            (Lang::En, "thank you very much for your help."),
            (Lang::En, "hello, how are you doing today?"),
        ];
        let (mut total, mut first, mut anywhere) = (0, 0, 0);
        for (lang, text) in samples {
            let right = layouts.iter().find(|l| l.lang == lang).expect("layout of the language");
            for wrong in layouts.iter().filter(|l| l.lang != lang) {
                // what appears when the keys of the right layout are pressed while the wrong one is active
                let typed = wrongkeys::convert(text, &right.map, &wrong.map);
                total += 1;
                match offer(&typed, &installed, &engine, Some(wrong.lang)) {
                    Outcome::Found(found) => {
                        if found[0].text == text {
                            first += 1;
                        }
                        if found.iter().any(|r| r.text == text) {
                            anywhere += 1;
                        } else {
                            eprintln!("NOT FOUND ({:?} typed on {:?}): {typed:?} -> {:?}", lang, wrong.lang, found.iter().map(|r| r.text.as_str()).collect::<Vec<_>>());
                        }
                    }
                    other => {
                        let known = |l: Lang, w: &str| engine.is_word_of(l, w);
                        eprintln!(
                            "NOT FOUND ({:?} typed on {:?}): {typed:?} -> {other:?}; share as typed {:.2}, share when put right {:.2}",
                            lang,
                            wrong.lang,
                            wrongkeys::real_word_share(&typed, lang, &known),
                            wrongkeys::real_word_share(text, lang, &known)
                        );
                    }
                }
            }
        }
        // text that is typed right must be left alone
        for (_, text) in samples {
            assert_eq!(offer(text, &installed, &engine, None), Outcome::NothingToFix, "{text:?} is right as it is");
        }
        // Algerian Darija in Arabic letters (what the author writes): reported, not asserted, because it depends on the Darija list
        let darija = [
            "واش راك خويا كيفاش حالك",
            "نحب نروح للسوق درك",
            "ماشي مشكل نشوفك بعد الصلاة",
        ];
        let arabic = layouts.iter().find(|l| l.lang == Lang::Ar).expect("Arabic layout");
        for text in darija {
            for wrong in layouts.iter().filter(|l| l.lang != Lang::Ar) {
                let typed = wrongkeys::convert(text, &arabic.map, &wrong.map);
                let back = match offer(&typed, &installed, &engine, Some(wrong.lang)) {
                    Outcome::Found(f) => format!("first reading right: {}, {} readings", f[0].text == text, f.len()),
                    other => format!("{other:?}"),
                };
                eprintln!("DARIJA on {:?}: {back}", wrong.lang);
            }
        }
        eprintln!("{total} cases: {first} offered first, {anywhere} offered among the readings");
        assert_eq!(anywhere, total, "every sentence must come back among the readings");
        assert!(first * 10 >= total * 9, "at least 9 of 10 come back as the first reading ({first} of {total})");
    }

    /// Thousands of Arabic sentences made of the real list's words, "typed" on the English and on the French keyboard of this PC, by length.
    /// Reports where it fails. Only reads layouts and files.
    ///   cargo test -p verba-app fixkeys::tests::stress -- --ignored --nocapture
    #[test]
    #[ignore = "needs the three keyboards installed and data/words"]
    fn stress_arabic_sentences_typed_on_the_english_and_french_keyboards() {
        let installed = verba_win::layouts::installed();
        let layouts = layouts_from(&installed);
        let mut engine = LocalEngine::load_dir(std::path::Path::new("../../data/words")).expect("data/words");
        engine.add_dz_seed();
        let arabic = layouts.iter().find(|l| l.lang == Lang::Ar).expect("Arabic layout");
        let words: Vec<String> = engine
            .list(Lang::Ar)
            .expect("Arabic list")
            .iter_words()
            .map(|(w, _)| w.to_string())
            .filter(|w| w.chars().count() >= 2 && w.chars().all(verba_core::is_arabic))
            .take(30_000)
            .collect();
        assert!(words.len() > 10_000, "only {} words", words.len());
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move |n: usize| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((state >> 33) as usize) % n
        };
        // (number of words, how many sentences) -> results
        let mut table: Vec<(usize, usize, usize, usize, Vec<String>)> = Vec::new();
        for n_words in [1usize, 2, 3, 5, 8, 12, 20] {
            let (mut total, mut first, mut any) = (0usize, 0usize, 0usize);
            let mut failures: Vec<String> = Vec::new();
            for _ in 0..300 {
                // the words come from the top of the list (common ones) more often than from the tail
                let pick = |next: &mut dyn FnMut(usize) -> usize| {
                    let span = if next(4) == 0 { words.len() } else { 3000 };
                    words[next(span)].clone()
                };
                let mut text = (0..n_words).map(|_| pick(&mut next)).collect::<Vec<_>>().join(" ");
                match next(4) {
                    0 => text.push_str(" \u{61f}"),
                    1 => text.push('.'),
                    _ => {}
                }
                for wrong in layouts.iter().filter(|l| l.lang != Lang::Ar) {
                    let typed = wrongkeys::convert(&text, &arabic.map, &wrong.map);
                    total += 1;
                    match offer(&typed, &installed, &engine, Some(wrong.lang)) {
                        Outcome::Found(f) => {
                            if f[0].text == text {
                                first += 1;
                            }
                            if f.iter().any(|r| r.text == text) {
                                any += 1;
                            } else if failures.len() < 4 {
                                failures.push(format!("{typed:?} -> {:?} (wanted {text:?})", f[0].text));
                            }
                        }
                        other => {
                            if failures.len() < 4 {
                                failures.push(format!("{typed:?} -> {other:?} (wanted {text:?})"));
                            }
                        }
                    }
                }
            }
            table.push((n_words, total, first, any, failures));
        }
        // text typed RIGHT on the English and the French keyboards (sentences from those lists) must not be turned into anything
        let mut rights: Vec<(usize, usize, usize, Vec<String>)> = Vec::new();
        for n_words in [1usize, 2, 3, 5, 8, 12] {
            let (mut total, mut touched) = (0usize, 0usize);
            let mut examples: Vec<String> = Vec::new();
            for lang in [Lang::En, Lang::Fr] {
                let list: Vec<String> = engine
                    .list(lang)
                    .expect("list")
                    .iter_words()
                    .map(|(w, _)| w.to_string())
                    .filter(|w| w.chars().count() >= 3 && w.chars().all(char::is_alphabetic))
                    .take(5000)
                    .collect();
                for _ in 0..200 {
                    let text = (0..n_words).map(|_| list[next(list.len())].clone()).collect::<Vec<_>>().join(" ");
                    total += 1;
                    if let Outcome::Found(f) = offer(&text, &installed, &engine, Some(lang)) {
                        touched += 1;
                        if examples.len() < 3 {
                            examples.push(format!("{text:?} -> {:?}", f[0].text));
                        }
                    }
                }
            }
            rights.push((n_words, total, touched, examples));
        }
        for (n, total, touched, examples) in &rights {
            eprintln!("RIGHT TEXT {n:>2} words: {total} cases, {touched} wrongly offered a change ({:.1}%)", *touched as f64 * 100.0 / *total as f64);
            for e in examples {
                eprintln!("      {e}");
            }
        }
        for (n, total, first, any, failures) in &table {
            eprintln!("{n:>2} words: {total} cases, {first} first ({:.1}%), {any} among the readings ({:.1}%)", *first as f64 * 100.0 / *total as f64, *any as f64 * 100.0 / *total as f64);
            for f in failures {
                eprintln!("      FAIL {f}");
            }
        }
    }

    /// Words the lists do not have (names, rare words: random Arabic letters), alone and among real words, typed on the English and the French
    /// keyboard. With the keyboard that was active (the usual case) and with the Arabic one active (the user switched after typing).
    ///   cargo test -p verba-app fixkeys::tests::unknown -- --ignored --nocapture
    #[test]
    #[ignore = "needs the three keyboards installed and data/words"]
    fn unknown_words_typed_on_the_wrong_keyboard() {
        let installed = verba_win::layouts::installed();
        let layouts = layouts_from(&installed);
        let mut engine = LocalEngine::load_dir(std::path::Path::new("../../data/words")).expect("data/words");
        engine.add_dz_seed();
        let arabic = layouts.iter().find(|l| l.lang == Lang::Ar).expect("Arabic layout");
        let real: Vec<String> = engine
            .list(Lang::Ar)
            .expect("Arabic list")
            .iter_words()
            .map(|(w, _)| w.to_string())
            .filter(|w| w.chars().count() >= 3 && w.chars().all(verba_core::is_arabic))
            .take(3000)
            .collect();
        let letters: Vec<char> = "\u{627}\u{628}\u{62a}\u{62b}\u{62c}\u{62d}\u{62e}\u{62f}\u{630}\u{631}\u{632}\u{633}\u{634}\u{635}\u{636}\u{637}\u{638}\u{639}\u{63a}\u{641}\u{642}\u{643}\u{644}\u{645}\u{646}\u{647}\u{648}\u{64a}".chars().collect();
        let mut state: u64 = 0x1234_5678_9ABC_DEF1;
        let mut next = move |n: usize| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((state >> 33) as usize) % n
        };
        let random_word = |next: &mut dyn FnMut(usize) -> usize| -> String {
            let n = 3 + next(4);
            (0..n).map(|_| letters[next(letters.len())]).collect()
        };
        // (label, how many unknown words, how many real words)
        for (label, unknown, known_words) in [("only unknown words", 1usize, 0usize), ("two unknown words", 2, 0), ("1 unknown among 2 real", 1, 2), ("1 unknown among 5 real", 1, 5), ("2 unknown among 4 real", 2, 4)] {
            for active_arabic in [false, true] {
                let (mut total, mut first, mut any) = (0usize, 0usize, 0usize);
                let mut failures: Vec<String> = Vec::new();
                for _ in 0..300 {
                    let mut parts: Vec<String> = (0..unknown).map(|_| random_word(&mut next)).collect();
                    parts.extend((0..known_words).map(|_| real[next(real.len())].clone()));
                    // shuffle (Fisher-Yates)
                    for i in (1..parts.len()).rev() {
                        parts.swap(i, next(i + 1));
                    }
                    let text = parts.join(" ");
                    for wrong in layouts.iter().filter(|l| l.lang != Lang::Ar) {
                        let typed = wrongkeys::convert(&text, &arabic.map, &wrong.map);
                        let active = if active_arabic { Lang::Ar } else { wrong.lang };
                        total += 1;
                        match offer(&typed, &installed, &engine, Some(active)) {
                            Outcome::Found(f) => {
                                if f[0].text == text {
                                    first += 1;
                                }
                                if f.iter().any(|r| r.text == text) {
                                    any += 1;
                                } else if failures.len() < 2 {
                                    failures.push(format!("{typed:?} -> {:?} (wanted {text:?})", f[0].text));
                                }
                            }
                            other => {
                                if failures.len() < 2 {
                                    failures.push(format!("{typed:?} -> {other:?} (wanted {text:?})"));
                                }
                            }
                        }
                    }
                }
                eprintln!(
                    "{label:<24} keyboard {}: {total} cases, {first} first ({:.1}%), {any} among the readings ({:.1}%)",
                    if active_arabic { "Arabic active" } else { "the typed one active" },
                    first as f64 * 100.0 / total as f64,
                    any as f64 * 100.0 / total as f64
                );
                for f in failures {
                    eprintln!("      FAIL {f}");
                }
            }
        }
    }

    /// Prints what the keys of a few Arabic texts give on the English and on the French keyboard of this PC: the text to give to a
    /// development build to look at the popup (`VERBA_TEST_FIXKEYS`, see main.rs). Only reads layouts.
    ///   cargo test -p verba-app fixkeys::tests::print_typed -- --ignored --nocapture
    #[test]
    #[ignore = "prints; needs the three keyboards installed"]
    fn print_typed_samples_for_the_background_check() {
        let layouts = layouts_from(&verba_win::layouts::installed());
        let arabic = layouts.iter().find(|l| l.lang == Lang::Ar).expect("Arabic layout");
        for (label, text) in [
            ("darija", "واش راك خويا كيفاش حالك اليوم"),
            ("names", "بلال براهيمي ونسيم"),
            ("long", "أريد أن أرسل لك الملف قبل الساعة الخامسة مساء، وشكرا جزيلا على مساعدتك."),
        ] {
            for wrong in layouts.iter().filter(|l| l.lang != Lang::Ar) {
                eprintln!("TYPED|{label}|{}|{}", wrong.lang.code(), wrongkeys::convert(text, &arabic.map, &wrong.map));
            }
        }
    }

    #[test]
    fn the_modifier_numbers_of_windows_and_of_the_engine_are_the_same() {
        assert_eq!(verba_win::layouts::SHIFT, wrongkeys::SHIFT);
        assert_eq!(verba_win::layouts::ALTGR, wrongkeys::ALTGR);
    }
}
