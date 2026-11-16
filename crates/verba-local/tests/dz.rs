//! Algerian Darija in Latin letters (Lang::Dz): suggestions, the merge with English and French, and the guarantee that
//! the spelling correction never touches or writes a Darija word.
//!
//! The tests that need only the embedded seed run everywhere. The ones that also need the English and French lists skip
//! when those were not downloaded (`scripts/fetch-data.ps1`), like the other test files.

use std::path::Path;

use verba_core::{Engine, Lang, current_word_dz};
use verba_local::{DZ_SEED, DzList, LocalEngine, WordList};

fn texts(v: Vec<verba_core::Suggestion>) -> Vec<String> {
    v.into_iter().map(|s| s.text).collect()
}

/// Small English and French lists, and the Darija seed (switched on).
fn small() -> LocalEngine {
    let mut e = LocalEngine::new();
    e.add(Lang::En, WordList::parse_for(Lang::En, "the 90000\nwhat 8000\nwhich 5000\nwhen 4000\nthey 3000\nback 2500\nbeen 2400\nhave 9000\nhavent 100\n", 100));
    e.add(Lang::Fr, WordList::parse_for(Lang::Fr, "le 90000\nque 8000\nquoi 5000\nbonjour 4000\nbesoin 3000\nmerci 2500\n", 100));
    e.add_dz_seed();
    e.set_enabled(Lang::Dz, true);
    e
}

fn real() -> Option<LocalEngine> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let mut e = LocalEngine::load_dir(&dir).ok()?;
    (e.languages().len() == 3).then(|| {
        e.add_dz_seed();
        e.set_enabled(Lang::Dz, true);
        e
    })
}

#[test]
fn the_seed_is_embedded_and_big_enough() {
    let seed = DzList::seed();
    assert!(seed.len() >= 1500, "{} words", seed.len());
    for w in [
        "wach", "kifach", "bezzaf", "sa7a", "3lach", "9ahwa", "hamdoullah", "khouya", "ndir", "mazal", "tomobil", "kayen", "wesh", "rani", "nta", "nti",
        "sahit", "bessah", "inchallah", "7bibi", "ma3lich", "makach", "flexi", "bortable",
    ] {
        assert!(seed.contains(w), "{w} is not in the seed");
    }
    assert!(DZ_SEED.starts_with('#'));
}

#[test]
fn darija_is_off_until_switched_on() {
    let mut e = LocalEngine::new();
    e.add(Lang::En, WordList::parse_for(Lang::En, "what 100\n", 10));
    e.add_dz_seed();
    // loaded, but not on
    assert!(e.languages().contains(&Lang::Dz) && !e.is_enabled(Lang::Dz) && !e.enabled_languages().contains(&Lang::Dz));
    assert!(e.suggest("wac", 3).is_empty() && e.suggest("sa7", 3).is_empty() && e.suggest_in(Lang::Dz, "wac", 3).is_empty());
    assert!(!e.dz_knows("wach"));
    e.set_enabled(Lang::Dz, true);
    assert_eq!(texts(e.suggest("wac", 1)), ["wach"]);
    assert!(e.enabled_languages().contains(&Lang::Dz));
    assert_eq!(Lang::Dz.code(), "dz");
    assert!(!Lang::Dz.is_rtl());
    // off again
    e.set_enabled(Lang::Dz, false);
    assert!(e.suggest("wac", 3).is_empty());
}

#[test]
fn prefixes_find_darija_words_with_their_digit_letters() {
    let e = small();
    let dz = |t: &str| texts(e.suggest_in(Lang::Dz, t, 3));
    // the examples of the brief
    let bez = dz("bez");
    assert!(bez.contains(&"bezzaf".to_string()) && bez.contains(&"bzaf".to_string()), "{bez:?}");
    assert_eq!(dz("sa7")[0], "sa7a");
    assert!(dz("3la").contains(&"3lach".to_string()), "{:?}", dz("3la"));
    assert!(dz("9ah").contains(&"9ahwa".to_string()));
    assert!(dz("kif").contains(&"kifach".to_string()));
    assert!(dz("khou").contains(&"khouya".to_string()));
    assert!(dz("hamdou").iter().any(|w| w.starts_with("hamdou")));
    assert!(dz("ma3l").contains(&"ma3lich".to_string()));
    assert!(dz("tomo").contains(&"tomobil".to_string()));
    assert!(dz("7bi").iter().any(|w| w.starts_with("7bib")));
    assert!(dz("flex").contains(&"flexi".to_string()));
    // a digit-letter at the start is part of the word, in the middle of a sentence too
    assert!(texts(e.suggest("wach ya 3l", 3)).contains(&"3lach".to_string()));
    // capitals follow what was typed
    assert_eq!(texts(e.suggest_in(Lang::Dz, "Wac", 1)), ["Wach"]);
    // one character is not enough
    assert!(dz("w").is_empty() && dz("3").is_empty());
}

#[test]
fn another_spelling_of_the_word_typed_is_not_offered_and_the_typed_spelling_is_kept() {
    let e = small();
    let dz = |t: &str| texts(e.suggest_in(Lang::Dz, t, 3));
    // `bezaf`, `bzaf` and `bezzaf` are one word: a finished one gets nothing
    for done in ["bezzaf", "bezaf", "bzaf", "kifach", "kifech", "wach", "wesh", "hamdoullah"] {
        assert!(dz(done).iter().all(|w| verba_local::dz_sound_key(w) != verba_local::dz_sound_key(done)), "{done} -> {:?}", dz(done));
    }
    // the typist who writes `kifech` is offered `kifech`, the one who writes `kifach` is offered `kifach`
    assert_eq!(dz("kife")[0], "kifech");
    assert_eq!(dz("kifa")[0], "kifach");
    assert_eq!(dz("bezaf".get(..4).unwrap())[0], "bezaf");
}

#[test]
fn darija_is_an_extra_pool_next_to_english_and_french() {
    let e = small();
    // where English and French have nothing, Darija answers (any keyboard layout of the Latin script)
    assert_eq!(texts(e.suggest("wac", 1)), ["wach"]);
    assert_eq!(texts(e.suggest_in(Lang::En, "wac", 1)), ["wach"], "an English keyboard types Arabizi");
    assert_eq!(texts(e.suggest_in(Lang::Fr, "wac", 1)), ["wach"], "a French keyboard too");
    assert!(e.suggest_in(Lang::Ar, "wac", 3).is_empty(), "an Arabic keyboard does not");
    assert_eq!(e.suggest_in(Lang::En, "wac", 1)[0].lang, Lang::Dz);
    // where English or French know the prefix, they win: Darija never fills the free places
    for typed in ["wha", "whi", "the", "bac", "bee", "bon", "bes", "qu"] {
        let got = e.suggest(typed, 3);
        assert!(got.iter().all(|s| s.lang != Lang::Dz), "{typed:?} -> {got:?}");
    }
    // asking for Darija only leaves English and French out
    assert!(e.suggest_in(Lang::Dz, "wha", 3).iter().all(|s| s.lang == Lang::Dz));
    // an Arabic word is not Darija
    assert!(e.suggest("مرح", 3).is_empty());
}

#[test]
fn the_words_before_tell_darija_from_english() {
    // `ba`: English `back` and `been`, Darija `bach` ("so that") and `ba3d` ("after")
    let mut e = LocalEngine::new();
    e.add(Lang::En, WordList::parse_for(Lang::En, "the 90000\nback 300\nbasic 200\nbag 150\nwhat 8000\n", 100));
    e.add_dz_seed();
    e.set_enabled(Lang::Dz, true);
    let top = |t: &str| e.suggest(t, 1).first().map(|s| s.lang);
    // alone, and after English words, English leads
    assert_eq!(top("ba"), Some(Lang::En));
    assert_eq!(top("what the ba"), Some(Lang::En));
    assert_eq!(top("the ba"), Some(Lang::En));
    // after Darija words, Darija leads: one word is enough when it is the last one and English does not know it
    assert_eq!(top("wach ba"), Some(Lang::Dz));
    assert_eq!(top("wach rani ba"), Some(Lang::Dz));
    assert_eq!(top("ya khouya kifach ba"), Some(Lang::Dz));
    // a word with digit-letters is Darija for sure
    assert_eq!(top("sa7a khouya ba"), Some(Lang::Dz));
    // the lean fades: three English words after the Darija ones
    assert_eq!(top("wach rani what the ba"), Some(Lang::En));
    // one Darija word far before English words does not decide
    assert_eq!(top("wach what the ba"), Some(Lang::En));
}

#[test]
fn a_darija_word_is_never_a_typo_and_never_a_fix() {
    let Some(e) = real() else { return };
    let mut off = LocalEngine::load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words")).unwrap();
    off.add_dz_seed(); // loaded but off
    let words = [
        "wach", "kifach", "kifech", "kifesh", "bezzaf", "bezaf", "bzaf", "khouya", "hamdoullah", "hamdoulah", "inchallah", "inchalah", "nchallah", "bessah",
        "chouf", "chwiya", "dyali", "ghodwa", "mazal", "rani", "ntouma", "tomobil", "bortable", "flexi", "makach", "kayen", "zwina", "drari", "mchit",
        // other spellings and negations of listed words
        "wech", "ouach", "kifah", "bezef", "makhdemtch", "choufek", "matchoufouche", "ma3rftch", "mabghitch", "nroh", "mazelt",
    ];
    for w in words {
        for text in [format!("{w} "), format!("wach rani {w}, "), format!("Hello {w}. ")] {
            for only in [None, Some(Lang::En), Some(Lang::Fr), Some(Lang::Dz)] {
                assert!(e.correct(only, &text, 3).is_none(), "{text:?} ({only:?}) was fixed: {:?}", e.correct(only, &text, 3));
            }
        }
    }
    // (with Dz off some of these ARE flagged: the switch matters)
    let flagged = words.iter().filter(|w| off.correct(None, &format!("{w} "), 3).is_some()).count();
    assert!(flagged >= 5, "only {flagged} of them are flagged without Dz: the test proves little");
    // a word with digit-letters is not a word to correct, Dz or not
    for w in ["sa7a ", "3lach ", "9ahwa ", "ma3lich ", "7bibi "] {
        assert!(e.correct(None, w, 3).is_none(), "{w:?}");
    }
    // a fix is never a Darija word, and Dz alone gives no fix
    for typo in ["I recieve ", "teh ", "Tell me wich ", "Je dis bonjuor ", "wnat ", "freind ", "seperate ", "beacoup ", "pourqoui "] {
        for only in [None, Some(Lang::En), Some(Lang::Fr)] {
            if let Some(f) = e.correct(only, typo, 3) {
                assert!(f.candidates.iter().all(|c| c.lang != Lang::Dz), "{typo:?} -> {:?}", f.candidates);
            }
        }
        assert!(e.correct(Some(Lang::Dz), typo, 3).is_none(), "Dz alone has nothing to correct with: {typo:?}");
    }
}

#[test]
fn english_and_french_typos_are_still_fixed_with_darija_on() {
    let Some(e) = real() else { return };
    let english = [
        ("I recieve ", "receive"),
        ("teh ", "the"),
        ("Tell me wich ", "which"),
        ("It was definately ", "definitely"),
        ("I stayed becuase ", "because"),
        ("I wnat ", "want"),
        ("Please seperate ", "separate"),
        ("It occured ", "occurred"),
        ("See you tommorow ", "tomorrow"),
        ("He is my freind ", "friend"),
        ("I beleive ", "believe"),
        ("The goverment ", "government"),
        ("I realy ", "really"),
        ("It is thier ", "their"),
        ("A diffrent ", "different"),
    ];
    let french = [
        ("C'est tres ", "très"),
        ("Je suis deja ", "déjà"),
        ("Je dis bonjuor ", "bonjour"),
        ("Il n'est jamias ", "jamais"),
        ("Mais pourqoui ", "pourquoi"),
        ("Merci beacoup ", "beaucoup"),
        ("Je viens maintenat ", "maintenant"),
        ("Il est dnas ", "dans"),
        ("Je dois etre ", "être"),
        ("Mon pere ", "père"),
    ];
    for (only, cases) in [(Lang::En, &english[..]), (Lang::Fr, &french[..])] {
        for (typed, want) in cases {
            let got: Vec<String> = e.correct(Some(only), typed, 3).map(|f| f.candidates.into_iter().map(|c| c.text).collect()).unwrap_or_default();
            assert!(got.iter().any(|g| g == want), "{typed:?} -> wanted {want:?}, got {got:?}");
        }
    }
}

#[test]
fn with_darija_off_nothing_changes() {
    let Some(mut e) = real() else { return };
    e.set_enabled(Lang::Dz, false);
    let plain = LocalEngine::load_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words")).unwrap();
    let mut n = 0;
    for w in plain.list(Lang::En).unwrap().iter_words().chain(plain.list(Lang::Fr).unwrap().iter_words()).filter(|(w, c)| w.len() >= 5 && *c > 1000).take(1500) {
        let (w, _) = w;
        assert_eq!(e.suggest(&w[..3], 3), plain.suggest(&w[..3], 3), "{w}");
        assert_eq!(e.suggest_in(Lang::En, &w[..2], 3), plain.suggest_in(Lang::En, &w[..2], 3), "{w}");
        let typo = format!("{}{} ", &w[..2], &w[3..]);
        assert_eq!(e.correct(None, &typo, 3), plain.correct(None, &typo, 3), "{typo}");
        n += 1;
    }
    assert!(n > 500);
    // Darija words get what they got before: nothing special
    assert_eq!(e.suggest("wac", 3), plain.suggest("wac", 3));
    assert_eq!(e.correct(None, "bezzaf ", 3), plain.correct(None, "bezzaf ", 3));
}

/// The prefixes of the most frequent English and French words, typed with Darija on: how often a Darija word is shown.
#[test]
fn ordinary_english_and_french_are_not_hijacked() {
    let Some(e) = real() else { return };
    for lang in [Lang::En, Lang::Fr] {
        let mut words: Vec<(String, u64)> = e.list(lang).unwrap().iter_words().map(|(w, c)| (w.to_string(), c)).collect();
        words.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let words: Vec<String> = words.into_iter().map(|x| x.0).filter(|w| w.len() >= 4 && w.chars().all(|c| c.is_ascii_lowercase())).take(3000).collect();
        let (mut shown, mut total) = (0, 0);
        for w in &words {
            for n in [2usize, 3, 4] {
                total += 1;
                if e.suggest_in(lang, &w[..n], 3).iter().any(|s| s.lang == Lang::Dz) {
                    shown += 1;
                }
            }
        }
        let rate = 100.0 * shown as f64 / total as f64;
        println!("{lang:?}: a Darija suggestion in {shown} of {total} prefixes ({rate:.2}%)");
        assert!(rate < 1.5, "{lang:?}: Darija shows up in {rate:.2}% of the prefixes of ordinary words");
    }
}

/// The seed's own words, so optimistic: a regression guard, not a measurement of real accuracy (see docs/ACCURACY.md).
#[test]
fn seed_words_are_found_from_their_first_letters() {
    let Some(e) = real() else { return };
    let words: Vec<String> = DzList::seed().iter_words().map(|(w, _)| w.to_string()).filter(|w| w.chars().count() >= 5).collect();
    let (mut hit, mut total) = (0, 0);
    for w in &words {
        total += 1;
        let typed = format!("wach rani {}", w.chars().take(4).collect::<String>());
        if e.suggest(&typed, 3).iter().any(|s| &s.text == w) {
            hit += 1;
        }
    }
    let rate = 100.0 * hit as f64 / total as f64;
    println!("seed words found in the top 3 after 4 characters, in a Darija sentence: {rate:.1}%");
    assert!(rate > 70.0, "{rate:.1}%");
}

#[test]
fn a_downloaded_list_is_merged_with_the_seed() {
    let dir = std::env::temp_dir().join(format!("verba-dz-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("dz.txt"), "zazouzi 40\nwach 10\n").unwrap();
    let mut e = LocalEngine::new();
    e.load_dz(&dir).unwrap();
    e.set_enabled(Lang::Dz, true);
    assert_eq!(texts(e.suggest_in(Lang::Dz, "zazo", 1)), ["zazouzi"]);
    assert!(e.word_count(Lang::Dz) > DzList::seed().len());
    assert!(e.dz_knows("zazouzi") && e.dz_knows("wach"));
    // no dz.txt: the seed alone
    let empty = std::env::temp_dir().join(format!("verba-dz-test-empty-{}", std::process::id()));
    std::fs::create_dir_all(&empty).unwrap();
    let mut e = LocalEngine::new();
    e.load_dz(&empty).unwrap();
    assert_eq!(e.word_count(Lang::Dz), DzList::seed().len());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn the_word_with_digit_letters_is_found_by_the_core_helper() {
    assert_eq!(current_word_dz("ya khouya sa7"), "sa7");
    assert_eq!(current_word_dz("wach rani 3l"), "3l");
}
