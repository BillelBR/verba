//! Hand-written, human-checkable cases for the spelling correction, on the real word lists: text typed so far (a finished
//! word and a space or punctuation) -> the fix that must be first or among the first three. They skip when the lists were
//! not downloaded (`scripts/fetch-data.ps1`).
//!
//! `KNOWN_GAPS` holds cases that are right but that the data or the guard rails cannot give yet (see docs/ACCURACY.md).

use std::path::Path;

use verba_core::Lang;
use verba_local::LocalEngine;

fn engine() -> Option<LocalEngine> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let e = LocalEngine::load_dir(&dir).ok()?;
    (e.languages().len() == 3).then_some(e)
}

/// Arabic letters that are the same word for the engine (hamza, taa marbuta, alef maqsura).
fn afold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
            'ة' => 'ه',
            'ى' | 'ئ' => 'ي',
            'ؤ' => 'و',
            c => c,
        })
        .collect()
}

/// The cases that fail, as readable lines. `only` is how the app asks when the keyboard says which language it is.
fn failures(e: &LocalEngine, only: Option<Lang>, cases: &[(&str, &str)]) -> Vec<String> {
    cases
        .iter()
        .filter_map(|(typed, want)| {
            let got: Vec<String> = e.correct(only, typed, 3).map(|f| f.candidates.into_iter().map(|s| s.text).collect()).unwrap_or_default();
            (!got.iter().any(|g| afold(g) == afold(want))).then(|| format!("  {typed:?} -> wanted {want:?}, got {got:?}"))
        })
        .collect()
}

fn assert_all(name: &str, e: &LocalEngine, only: Option<Lang>, cases: &[(&str, &str)]) {
    let bad = failures(e, only, cases);
    assert!(bad.is_empty(), "{name}: {} of {} cases failed:\n{}", bad.len(), cases.len(), bad.join("\n"));
}

/// Nothing may be offered for these.
fn assert_quiet(e: &LocalEngine, only: Option<Lang>, texts: &[&str]) {
    let noisy: Vec<String> = texts.iter().filter_map(|t| e.correct(only, t, 3).map(|f| format!("  {t:?} -> {:?}", f.candidates))).collect();
    assert!(noisy.is_empty(), "{} texts got a fix they should not have:\n{}", noisy.len(), noisy.join("\n"));
}

const ENGLISH: &[(&str, &str)] = &[
    ("I recieve ", "receive"),
    ("The package was recieved ", "received"),
    ("teh ", "the"),
    ("Tell me wich ", "which"),
    ("It was definately ", "definitely"),
    ("I stayed becuase ", "because"),
    ("I wnat ", "want"),
    ("Thnaks ", "Thanks"),
    ("Please seperate ", "separate"),
    ("It occured ", "occurred"),
    ("Wait untill ", "until"),
    ("See you tommorow ", "tomorrow"),
    ("He is my freind ", "friend"),
    ("I beleive ", "believe"),
    ("What is your adress ", "address"),
    ("That is wierd ", "weird"),
    ("The goverment ", "government"),
    ("A clean enviroment ", "environment"),
    ("I will accomodate ", "accommodate"),
    ("It was the begining ", "beginning"),
    ("I realy ", "really"),
    ("I dont ", "don't"),
    ("It is thier ", "their"),
    ("I need somthing ", "something"),
    ("A diffrent ", "different"),
    ("A peice ", "piece"),
    ("I recomend ", "recommend"),
    ("It is wich, ", "which"),
    ("It rained. Teh ", "The"),
    ("Thank yuo ", "you"),
];

const FRENCH: &[(&str, &str)] = &[
    ("C'est tres ", "très"),
    ("L'ete ", "été"),
    ("Je suis deja ", "déjà"),
    ("Apres ", "Après"),
    ("Tu parles francais ", "français"),
    ("Je dis bonjuor ", "bonjour"),
    ("Il n'est jamias ", "jamais"),
    ("Elle est toujorus ", "toujours"),
    ("Mais pourqoui ", "pourquoi"),
    ("Merci beacoup ", "beaucoup"),
    ("Je viens maintenat ", "maintenant"),
    ("Il est diferent ", "différent"),
    ("Je viens avce ", "avec"),
    ("Il est dnas ", "dans"),
    ("C'est poru ", "pour"),
    ("Il fait comem ", "comme"),
    ("Oui mias ", "mais"),
    ("Je vais a l'ecole ", "école"),
    ("Je dois etre ", "être"),
    ("C'est la meme ", "même"),
    ("Mon pere ", "père"),
    ("Ma mere ", "mère"),
    ("Ouvre la fenetre ", "fenêtre"),
    ("Il a mal a la tete ", "tête"),
    ("Je ne sias ", "sais"),
    ("Ce n'est pas vari ", "vrai"),
    ("C'est tres, ", "très"),
    ("Il y a un probleme ", "problème"),
    ("Une grande ecole ", "école"),
];

const ARABIC: &[(&str, &str)] = &[
    ("قال مرجبا ", "مرحبا"),
    ("قال كشرا ", "شكرا"),
    ("ذهبت الى اليم ", "اليوم"),
    ("لا اعرف لمذا ", "لماذا"),
    ("هل انت بخري ", "بخير"),
    ("ماذا تفعلل ", "تفعل"),
    ("نحن نحب الجميغ ", "الجميع"),
    ("تعال الى مععك ", "معك"),
    ("اجلس هننا ", "هنا"),
    ("السلام علكيم ", "عليكم"),
    ("قال صباخ ", "صباح"),
    ("صباح الخري ", "الخير"),
    ("هذا الطععام ", "الطعام"),
    ("هذا صحييح ", "صحيح"),
    ("كيف الححال ", "الحال"),
    ("اذهب الى الببيت ", "البيت"),
    ("هذا الرجلل ", "الرجل"),
    ("نحن في المردسة ", "المدرسة"),
    ("هذا الكتتاب ", "الكتاب"),
    ("هذه الحيااة ", "الحياة"),
    ("كل النااس ", "الناس"),
    ("انا اريدد ", "أريد"),
    ("مااذا ", "ماذا"),
    ("انا اعررف ", "اعرف"),
    ("هذا فطق ", "فقط"),
    ("هل رايتهم اليومم ", "اليوم"),
];

#[test]
fn english_typos_are_fixed() {
    let Some(e) = engine() else { return };
    assert_all("English", &e, Some(Lang::En), ENGLISH);
}

#[test]
fn english_typos_are_fixed_when_the_keyboard_is_not_followed() {
    let Some(e) = engine() else { return };
    // `dont` is a French word too: without the keyboard saying English, it is not a slip
    let cases: Vec<(&str, &str)> = ENGLISH.iter().copied().filter(|(t, _)| *t != "I dont ").collect();
    assert_all("English (all languages)", &e, None, &cases);
}

#[test]
fn french_typos_and_missing_accents_are_fixed() {
    let Some(e) = engine() else { return };
    assert_all("French", &e, Some(Lang::Fr), FRENCH);
}

#[test]
fn arabic_typos_are_fixed() {
    let Some(e) = engine() else { return };
    assert_all("Arabic", &e, Some(Lang::Ar), ARABIC);
}

#[test]
fn a_correct_word_is_left_alone() {
    let Some(e) = engine() else { return };
    assert_quiet(
        &e,
        None,
        &[
            "hello ", "I would like ", "Thank you ", "this is the best ", "everything ", "knowledge ", "tomorrow, ", "Comment allez-vous ", "bonjour ",
            "très ", "où ", "مرحبا ", "شكرا ", "الكتاب ", "والكتاب ", "بالمدرسة ", "كتابه ", "المدرسة،",
        ],
    );
}

#[test]
fn words_that_only_lack_an_ending_in_the_list_are_left_alone() {
    let Some(e) = engine() else { return };
    // valid forms (plural, past, -ing, feminine, plural of Arabic) that a 50000-word list may lack
    assert_quiet(&e, Some(Lang::En), &["The habitats ", "The averages ", "Many rescuers ", "two floorboards ", "She salutes ", "bulldogs "]);
    assert_quiet(&e, Some(Lang::Fr), &["Il admire ", "Elles admirent ", "les fraternités ", "un profiteur "]);
}

#[test]
fn a_word_in_the_other_language_is_not_a_typo() {
    let Some(e) = engine() else { return };
    // French typed on an English keyboard (or the other way round) is a word of a language that is on
    assert_quiet(&e, Some(Lang::En), &["I live in a maison ", "C'est magnifique ", "bonjour "]);
    assert_quiet(&e, Some(Lang::Fr), &["It is the weather ", "hello "]);
}

#[test]
fn names_acronyms_and_odd_words_are_left_alone() {
    let Some(e) = engine() else { return };
    assert_quiet(
        &e,
        None,
        &[
            "I met John yesterday and Jonh ", // a capital letter in the middle of a sentence: a name
            "We work with NASA ",
            "He has an iPhone ",
            "the camelCase ",
            "see www.exmple ",
            "well-knwon ",
            "abc123 ",
            "ab ",
            "to ",
            "Je vois Piere ",
        ],
    );
}

#[test]
fn only_a_finished_word_is_checked() {
    let Some(e) = engine() else { return };
    assert_quiet(&e, None, &["I recieve", "teh", "I recieve. . . . ", "", " ", "مرجبا"]);
    assert!(e.correct(None, "I recieve ", 3).is_some());
    assert!(e.correct(None, "I recieve, ", 3).is_some());
    assert!(e.correct(None, "I recieve? ", 3).is_some());
    assert!(e.correct(None, "(I recieve) ", 3).is_some());
}

#[test]
fn the_fix_keeps_what_was_typed_after_the_word_and_the_capitals() {
    let Some(e) = engine() else { return };
    let f = e.correct(None, "Please send me teh, ", 3).expect("a fix");
    assert_eq!((f.original.as_str(), f.trailing.as_str(), f.candidates[0].text.as_str()), ("teh", ", ", "the"));
    let f = e.correct(None, "It rained. Teh\n", 3).expect("a fix at the start of a sentence");
    assert_eq!((f.original.as_str(), f.trailing.as_str(), f.candidates[0].text.as_str()), ("Teh", "\n", "The"));
    assert_eq!(f.candidates[0].lang, Lang::En);
    let f = e.correct(None, "مرجبا،", 3).expect("a fix");
    assert_eq!((f.original.as_str(), f.trailing.as_str()), ("مرجبا", "،"));
}

#[test]
fn the_languages_asked_for_decide_where_fixes_come_from() {
    let Some(e) = engine() else { return };
    let fr = e.correct(None, "Je dis bonjuor ", 3).expect("a fix");
    assert_eq!(fr.candidates[0].lang, Lang::Fr);
    assert!(e.correct(Some(Lang::En), "Je dis bonjuor ", 3).is_none(), "no English word is near it");
    assert_eq!(e.correct(Some(Lang::Fr), "Je dis bonjuor ", 3).expect("a fix").candidates[0].text, "bonjour");
    // an Arabic word is never fixed from the Latin lists, nor the other way round
    assert!(e.correct(Some(Lang::En), "مرجبا ", 3).is_none());
    assert!(e.correct(Some(Lang::Ar), "recieve ", 3).is_none());
}

#[test]
fn a_language_that_is_off_gives_no_fix() {
    let Some(mut e) = engine() else { return };
    assert!(e.correct(None, "I recieve ", 3).is_some());
    e.set_enabled(Lang::En, false);
    assert!(e.correct(None, "I recieve ", 3).is_none());
    assert!(e.correct(Some(Lang::En), "I recieve ", 3).is_none());
    e.set_enabled(Lang::En, true);
    e.set_enabled(Lang::Ar, false);
    assert!(e.correct(None, "مرجبا ", 3).is_none());
}

#[test]
fn a_dismissed_word_and_a_repeated_word_are_left_alone() {
    let Some(mut e) = engine() else { return };
    // the same word earlier in the text: the user meant it
    assert_quiet(&e, None, &["recieve and recieve ", "Teh teh "]);
    e.ignore_word("Recieve");
    assert_quiet(&e, None, &["I recieve "]);
}

#[test]
fn a_french_word_that_already_has_its_accent_is_not_an_accent_slip() {
    let Some(e) = engine() else { return };
    assert_quiet(&e, Some(Lang::Fr), &["Il décore ", "Elle dérobe ", "Ils intercepte "]);
}

#[test]
fn max_limits_the_number_of_candidates() {
    let Some(e) = engine() else { return };
    assert!(e.correct(None, "I recieve ", 1).expect("a fix").candidates.len() == 1);
    assert!(e.correct(None, "I recieve ", 0).is_none());
}

/// Right fixes that the data or the guard rails cannot give yet. Kept visible; run with `-- --ignored` to see them.
const KNOWN_GAPS: &[(&str, &str)] = &[
    // both are words of the language: a spelling checker cannot tell the slip from the verb form without reading the sentence
    ("Je l'ai trouve ", "trouvé"),
    ("I want to go their ", "there"),
    // an extra ت ك ه ي or ن at the end of a word looks like a pronoun or plural ending, and such forms are words
    ("اين كنتت ", "كنت"),
    ("ليس لدي الوقتت ", "الوقت"),
    ("لا اعرف ذلكك ", "ذلك"),
    // the lists hold the slip itself, and 308 times fewer than the right word: not enough to be sure for a 4-letter word
    ("انا قللت ", "قلت"),
    // `سيرا` + the ending `ة` looks like a word to the endings rule
    ("اركب السيراة ", "السيارة"),
];

#[test]
#[ignore = "documents what the correction cannot do yet"]
fn known_gaps() {
    let Some(e) = engine() else { return };
    let bad = failures(&e, None, KNOWN_GAPS);
    println!("{} of {} known gaps still fail:\n{}", bad.len(), KNOWN_GAPS.len(), bad.join("\n"));
}
