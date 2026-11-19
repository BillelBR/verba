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

/// `text` was selected: how can it be read as typed on the wrong keyboard?
pub fn offer(text: &str, installed: &[InstalledLayout], engine: &LocalEngine) -> Outcome {
    let layouts = layouts_from(installed);
    let mut langs: Vec<Lang> = layouts.iter().map(|l| l.lang).collect();
    langs.sort_by_key(|l| l.code());
    langs.dedup();
    if langs.len() < 2 {
        return Outcome::NeedTwoLanguages;
    }
    let found = wrongkeys::candidates(text, &layouts, &|lang, word| engine.is_word_of(lang, word));
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
        match offer("qwert ytuq", &installed, &engine()) {
            Outcome::Found(found) => {
                assert_eq!(found[0], Reading { text: "مرحبا عالم".into(), lang: Lang::Ar, label: "EN \u{2192} AR".into() });
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn one_language_of_keyboards_is_not_enough() {
        let installed = vec![layout(Some(Lang::En), EN), layout(Some(Lang::En), EN), layout(None, "abcdefghij")];
        assert_eq!(offer("hello", &installed, &engine()), Outcome::NeedTwoLanguages);
        assert_eq!(offer("hello", &[], &engine()), Outcome::NeedTwoLanguages);
    }

    #[test]
    fn text_that_is_right_is_left_alone() {
        let installed = vec![layout(Some(Lang::En), EN), layout(Some(Lang::Ar), AR)];
        assert_eq!(offer("hello world", &installed, &engine()), Outcome::NothingToFix);
        assert_eq!(offer("مرحبا عالم", &installed, &engine()), Outcome::NothingToFix);
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
                match offer(&typed, &installed, &engine) {
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
            assert_eq!(offer(text, &installed, &engine), Outcome::NothingToFix, "{text:?} is right as it is");
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
                let back = match offer(&typed, &installed, &engine) {
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

    #[test]
    fn the_modifier_numbers_of_windows_and_of_the_engine_are_the_same() {
        assert_eq!(verba_win::layouts::SHIFT, wrongkeys::SHIFT);
        assert_eq!(verba_win::layouts::ALTGR, wrongkeys::ALTGR);
    }
}
