//! Hand-written, human-checkable cases on the real word lists: text typed so far -> a word that must be among the
//! 3 suggestions. They skip when the lists were not downloaded (`scripts/fetch-data.ps1`).
//!
//! `KNOWN_GAPS` holds cases that are right but that the data cannot give yet (see docs/ACCURACY.md).

use std::path::Path;

use verba_core::{Engine, Lang};
use verba_local::LocalEngine;

fn engine() -> Option<LocalEngine> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let e = LocalEngine::load_dir(&dir).ok()?;
    (e.languages().len() == 3).then_some(e)
}

/// The cases that fail, as readable lines.
fn failures(e: &LocalEngine, cases: &[(&str, &str)]) -> Vec<String> {
    cases
        .iter()
        .filter_map(|(typed, want)| {
            let got: Vec<String> = e.suggest(typed, 3).into_iter().map(|s| s.text).collect();
            (!got.iter().any(|g| g == want)).then(|| format!("  {typed:?} -> wanted {want:?}, got {got:?}"))
        })
        .collect()
}

fn assert_all(name: &str, e: &LocalEngine, cases: &[(&str, &str)]) {
    let bad = failures(e, cases);
    assert!(bad.is_empty(), "{name}: {} of {} cases failed:\n{}", bad.len(), cases.len(), bad.join("\n"));
}

const ENGLISH: &[(&str, &str)] = &[
    ("I would lik", "like"),
    ("Thank yo", "you"),
    ("What are you doin", "doing"),
    ("I don't kn", "know"),
    ("Can you hel", "help"),
    ("See you tomor", "tomorrow"),
    ("Happy birth", "birthday"),
    ("Good mor", "morning"),
    ("I'm really sor", "sorry"),
    ("I want to go hom", "home"),
    ("Where is the bathro", "bathroom"),
    ("It was a beautif", "beautiful"),
    ("Thanks for everyth", "everything"),
    ("Let me kno", "know"),
    ("How abo", "about"),
    ("I will see you lat", "later"),
    ("Have a nice wee", "week"),
    ("We need to talk abo", "about"),
    ("Don't wor", "worry"),
    ("I'll call you tonig", "tonight"),
    ("Please be caref", "careful"),
    ("He is my best frie", "friend"),
    ("Hel", "Hello"),
    ("HEL", "HELLO"),
    ("See you on Wedn", "Wednesday"),
    ("I don't underst", "understand"),
    ("That's incredi", "incredible"),
    ("Do you have any quest", "questions"),
    ("I think you are rig", "right"),
    ("Please wai", "wait"),
    ("Don't forg", "forget"),
    ("don", "don't"),
    ("I'm going to the airp", "airport"),
    ("That was awes", "awesome"),
    ("You are welc", "welcome"),
    ("I can", "can't"),
];

const FRENCH: &[(&str, &str)] = &[
    ("Je voudrais un caf", "café"),
    ("Bonjo", "Bonjour"),
    ("bonjo", "bonjour"),
    ("Merc", "Merci"),
    ("Je ne sais pas pourqu", "pourquoi"),
    ("Comment allez-vous aujourd", "aujourd'hui"),
    ("Je suis très cont", "content"),
    ("J'ai beaucou", "beaucoup"),
    ("Qu'est-ce que tu veu", "veux"),
    ("Il faut qu'on parl", "parler"),
    ("On se voit dem", "demain"),
    ("Je vais appe", "appeler"),
    ("C'est vraim", "vraiment"),
    ("Je suis désol", "désolé"),
    ("Merci pour vot", "votre"),
    ("Je suis fati", "fatigué"),
    ("C'est très impor", "important"),
    ("Je ne compr", "comprends"),
    ("J'aimerais savoir si vous ête", "êtes"),
    ("Tres", "Très"),
    ("Je suis ete", "été"),
    ("Il y a beaucoup de mon", "monde"),
    ("Bonne anné", "année"),
    ("Joyeux anniv", "anniversaire"),
    ("Je suis en retar", "retard"),
    ("Tu es mon meil", "meilleur"),
    ("Qu'est-ce qui se pass", "passe"),
    ("Je suis au trav", "travail"),
    ("Il y a chaq", "chaque"),
    ("J'ai besoin d'aid", "aide"),
    ("Nous sommes en vacan", "vacances"),
    ("Merci de votre répon", "réponse"),
    ("À bientô", "bientôt"),
    ("Je vous souhaite une bonne journ", "journée"),
    ("C'est possi", "possible"),
    ("C'est l'am", "ami"),
    ("Je suis desole", "désolé"),
    ("Il est tres gent", "gentil"),
    ("Le cafe est pret", "prêt"),
];

const ARABIC: &[(&str, &str)] = &[
    ("شكر", "شكرا"),
    ("مرح", "مرحبا"),
    ("أريد أن أذه", "أذهب"),
    ("لا أستطي", "أستطيع"),
    ("ماذا تفع", "تفعل"),
    ("هذا الكت", "الكتاب"),
    ("في المدر", "المدرسة"),
    ("ذهبت إلى المدين", "المدينة"),
    ("من فضل", "فضلك"),
    ("صباح الخي", "الخير"),
    ("مساء الخي", "الخير"),
    ("ما اسم", "اسمي"),
    ("هل تتحد", "تتحدث"),
    ("إن شاء الل", "الله"),
    ("ليس لدي وق", "وقت"),
    ("أنا آسف جد", "جدا"),
    ("هذا مستحي", "مستحيل"),
    ("الكل", "الكلام"),
    ("المدر", "المدرسة"),
    ("الحيا", "الحياة"),
    ("الوق", "الوقت"),
    ("الرج", "الرجل"),
    ("اليو", "اليوم"),
    // short vowels typed
    ("مَرْحَ", "مرحبا"),
    ("شُكْ", "شكرا"),
    ("السَّلا", "السلام"),
    // without hamza, taa marbuta, dots of yaa
    ("اري", "أريد"),
    ("اعتق", "أعتقد"),
    ("احتا", "أحتاج"),
    ("المدرسه", "المدرسة"),
    ("مدرسه", "مدرسة"),
    ("حقيق", "حقيقة"),
    ("هذه الحيا", "الحياة"),
    ("مسوول", "مسؤول"),
    // attached particles and the article
    ("والكتا", "والكتاب"),
    ("بالمدر", "بالمدرسة"),
    ("للمدر", "للمدرسة"),
    ("وبالوق", "وبالوقت"),
];

/// Right answers the word lists cannot give, because a single word's frequency decides the order and the phrase is not
/// in the data. Run them with `cargo test -p verba-local --test sanity -- --ignored` to see where they stand.
const KNOWN_GAPS: &[(&str, &str)] = &[
    // `اسمك` is 4th: `اسمع`, `اسمي`, `اسمه` are each more frequent as single words
    ("ما اسم", "اسمك"),
    // `الحمد لل` -> `لله` needs the previous word (a bigram); alone `للغاية`, `للتو`, `للمنزل` are more frequent
    ("الحمد لل", "لله"),
    // a spelling that is a common word of its own (`انا` is a very frequent way to write `أنا`) is not "corrected" by a completion
    ("انا", "أنا"),
    ("اكتب", "أكتب"),
    ("ان", "أن"),
];

#[test]
#[ignore = "documents what the word lists cannot do yet"]
fn known_gaps() {
    let Some(e) = engine() else { return };
    assert_all("known gaps", &e, KNOWN_GAPS);
}

#[test]
fn english_sanity() {
    let Some(e) = engine() else { return };
    assert_all("english", &e, ENGLISH);
}

#[test]
fn french_sanity() {
    let Some(e) = engine() else { return };
    assert_all("french", &e, FRENCH);
}

#[test]
fn arabic_sanity() {
    let Some(e) = engine() else { return };
    assert_all("arabic", &e, ARABIC);
}

#[test]
fn a_language_switched_off_never_shows() {
    let Some(mut e) = engine() else { return };
    e.set_enabled(Lang::Fr, false);
    for typed in ["bon", "mer", "je suis tr", "dem"] {
        assert!(e.suggest(typed, 3).iter().all(|s| s.lang != Lang::Fr), "French shown for {typed:?}");
    }
    // English still answers, and fills the places
    assert_eq!(e.suggest("hel", 3).len(), 3);
    e.set_enabled(Lang::En, false);
    assert!(e.suggest("hel", 3).is_empty());
}

#[test]
fn the_words_before_choose_the_language() {
    let Some(e) = engine() else { return };
    let lang = |t: &str| e.suggest(t, 1).first().map(|s| s.lang);
    // after French words the French reading leads; after English words the English one does
    assert_eq!(lang("je suis tr"), Some(Lang::Fr));
    assert_eq!(lang("i am tr"), Some(Lang::En));
    assert_eq!(lang("je pense que c'est un bon pro"), Some(Lang::Fr));
    assert_eq!(lang("i think this is a good pro"), Some(Lang::En));
}

#[test]
fn no_suggestion_is_noise() {
    let Some(e) = engine() else { return };
    // never a fragment of a contraction, a hyphenated or numbered token, or a stretched sound
    for typed in ["do", "is", "wa", "ha", "ah", "mm", "aa", "1s", "c'", "qu", "aujo", "peut"] {
        for s in e.suggest(typed, 3) {
            assert!(!s.text.contains('-') && !s.text.chars().any(|c| c.is_ascii_digit()), "{typed:?} -> {:?}", s.text);
            assert!(!s.text.starts_with('\''), "{typed:?} -> {:?}", s.text);
            let cs: Vec<char> = s.text.chars().collect();
            assert!(!cs.windows(3).any(|w| w[0] == w[1] && w[1] == w[2]), "{typed:?} -> {:?}", s.text);
        }
    }
    for typed in ["did", "doe", "isn", "wouldn", "ll", "ve"] {
        assert!(e.suggest(typed, 3).iter().all(|s| !["didn", "doesn", "isn", "wouldn", "ll", "ve"].contains(&s.text.as_str())), "{typed:?}");
    }
}
