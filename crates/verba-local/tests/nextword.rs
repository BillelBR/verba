//! Next-word prediction. Most tests use small tables written here, so they always run; the ones on the real tables skip when
//! they were not built (`scripts/fetch-bigrams.ps1`).

use std::path::Path;

use verba_core::Lang;
use verba_local::{LocalEngine, WordList};

fn texts(v: Vec<verba_core::Suggestion>) -> Vec<String> {
    v.into_iter().map(|s| s.text).collect()
}

const EN_TABLE: &str = "\
# a tiny table
would\t1000\tlike:500 you:100 be:90 not:80
like\t5000\tto:1500 a:900 the:700 you:300
i\t9000\twould:900 don't:800 am:700 think:400
i'm\t900\tnot:300 going:200 sure:100
thank\t100\tyou:90 god:1
thank you\t90\tfor:60 very:10 so:5
would like\t300\tto:250 a:20 the:10
the\t9000\tbest:90
";
const FR_TABLE: &str = "\
je\t5000\tsuis:900 ne:800 vais:500
il\t5000\test:900 y:700 a:600
merci\t500\tde:200 pour:150 beaucoup:100
";
const AR_TABLE: &str = "\
شكرا\t500\tجزيلا:200 لك:150 على:100
هل\t900\tيمكنك:100 تريد:90 هناك:80
";

fn engine() -> LocalEngine {
    let mut e = LocalEngine::new();
    e.add(Lang::En, WordList::parse_for(Lang::En, "the 100000\nwould 5000\nlike 5000\nthank 4000\nyou 90000\n", 100));
    e.add(Lang::Fr, WordList::parse_for(Lang::Fr, "je 50000\nil 40000\nmerci 3000\nsuis 20000\n", 100));
    e.add(Lang::Ar, WordList::parse_for(Lang::Ar, "شكرا 3000\nهل 9000\nلك 5000\n", 100));
    e.load_next_words(Lang::En, EN_TABLE);
    e.load_next_words(Lang::Fr, FR_TABLE);
    e.load_next_words(Lang::Ar, AR_TABLE);
    e
}

#[test]
fn the_word_before_the_caret_decides() {
    let e = engine();
    // the context of two words wins when the table has it: `would like` -> `to`, not what follows `like` in general
    assert_eq!(texts(e.next_words(Some(Lang::En), "I would like ", 3)), ["to", "a", "the"]);
    assert_eq!(texts(e.next_words(Some(Lang::En), "I think we would ", 3))[0], "like");
    assert_eq!(texts(e.next_words(Some(Lang::En), "Thank you ", 3))[0], "for");
    // one word of context
    assert_eq!(texts(e.next_words(Some(Lang::En), "so I ", 2)), ["would", "don't"]);
    assert_eq!(texts(e.next_words(Some(Lang::En), "Thank ", 2)), ["you"]);
}

#[test]
fn it_answers_only_after_a_finished_word_and_a_space() {
    let e = engine();
    assert!(!e.next_words(Some(Lang::En), "I would like ", 3).is_empty());
    // still typing the word
    assert!(e.next_words(Some(Lang::En), "I would like", 3).is_empty());
    assert!(e.next_words(Some(Lang::En), "I would li", 3).is_empty());
    // punctuation or a line break between the word and the caret ends the context
    assert!(e.next_words(Some(Lang::En), "I would like, ", 3).is_empty());
    assert!(e.next_words(Some(Lang::En), "I would like.  ", 3).is_empty());
    assert!(e.next_words(Some(Lang::En), "I would like\n", 3).is_empty());
    assert!(e.next_words(Some(Lang::En), "I would like\n ", 3).is_empty());
    assert!(e.next_words(Some(Lang::En), "", 3).is_empty());
    assert!(e.next_words(Some(Lang::En), " ", 3).is_empty());
    assert!(e.next_words(Some(Lang::En), "I would like ", 0).is_empty());
    // several spaces still count; a word after a comma is a word
    assert!(!e.next_words(Some(Lang::En), "I would like   ", 3).is_empty());
    assert_eq!(texts(e.next_words(Some(Lang::En), "Yes, I ", 1)), ["would"]);
}

#[test]
fn nothing_known_means_nothing_offered() {
    let e = engine();
    assert!(e.next_words(Some(Lang::En), "the zxqv ", 3).is_empty(), "an unknown word");
    assert!(e.next_words(Some(Lang::En), "room 12 ", 3).is_empty(), "a number");
    assert!(e.next_words(Some(Lang::En), "mail me at bob@example.com ", 3).is_empty(), "an address");
    assert!(e.next_words(Some(Lang::En), "see https://example.com/like ", 3).is_empty(), "a URL");
    assert!(e.next_words(Some(Lang::En), "a well-known ", 3).is_empty(), "a hyphenated word");
    // a word that is known but whose followers are too rare to matter: the floor of the probability
    let mut tiny = LocalEngine::new();
    tiny.add(Lang::En, WordList::parse("a 5\n", 10));
    tiny.load_next_words(Lang::En, "word\t1000\tx:5 y:4\n");
    assert!(tiny.next_words(Some(Lang::En), "a word ", 3).is_empty());
    // no table loaded at all
    assert!(LocalEngine::new().next_words(None, "I would like ", 3).is_empty());
}

#[test]
fn i_is_written_with_a_capital() {
    let e = engine();
    assert_eq!(texts(e.next_words(Some(Lang::En), "so what do ", 3)).len(), 0);
    let mut e2 = engine();
    e2.load_next_words(Lang::En, "that\t100\ti:50 he:30 i'm:20\n");
    assert_eq!(texts(e2.next_words(Some(Lang::En), "I think that ", 3)), ["I", "he", "I'm"]);
}

#[test]
fn contractions_are_one_word_and_french_elisions_cut() {
    let e = engine();
    assert_eq!(texts(e.next_words(Some(Lang::En), "Well I'm ", 3)), ["not", "going", "sure"]);
    // French: `qu'il` -> the word before the caret is `il`
    assert_eq!(texts(e.next_words(Some(Lang::Fr), "Je pense qu'il ", 3)), ["est", "y", "a"]);
    // right after an elision there is no space, hence nothing
    assert!(e.next_words(Some(Lang::Fr), "Je pense qu'", 3).is_empty());
}

#[test]
fn the_keyboard_language_and_the_script_choose_the_table() {
    let e = engine();
    assert_eq!(texts(e.next_words(Some(Lang::Fr), "Merci ", 3)), ["de", "pour", "beaucoup"]);
    assert!(e.next_words(Some(Lang::En), "Merci ", 3).is_empty(), "no English table has `merci`");
    // without a keyboard language: the Latin tables that are on, the words before leaning to one of them
    assert_eq!(texts(e.next_words(None, "Merci ", 1)), ["de"]);
    assert_eq!(texts(e.next_words(None, "I would like ", 1)), ["to"]);
    // Arabic text goes to the Arabic table, whatever the keyboard
    assert_eq!(texts(e.next_words(None, "شكرا ", 3)), ["جزيلا", "لك", "على"]);
    assert_eq!(texts(e.next_words(Some(Lang::Ar), "قلت له هل ", 2)), ["يمكنك", "تريد"]);
    assert!(e.next_words(Some(Lang::En), "شكرا ", 3).is_empty());
    assert!(e.next_words(Some(Lang::Ar), "Merci ", 3).is_empty());
    // the suggestions say which language they are
    assert!(e.next_words(None, "Merci ", 3).iter().all(|s| s.lang == Lang::Fr));
}

#[test]
fn a_language_that_is_off_gives_nothing() {
    let mut e = engine();
    e.set_enabled(Lang::Fr, false);
    assert!(e.next_words(Some(Lang::Fr), "Merci ", 3).is_empty());
    assert!(e.next_words(None, "Merci ", 3).is_empty());
    e.set_enabled(Lang::Fr, true);
    assert!(!e.next_words(None, "Merci ", 3).is_empty());
}

#[test]
fn a_damaged_table_gives_a_smaller_table_not_an_error() {
    let mut e = engine();
    e.load_next_words(Lang::En, "garbage\nwould\t10\tlike:5\n\u{feff}x\t\t\n\t\t\nbad\t-4\ta:1\nok\t10\tfine:x\n");
    assert_eq!(texts(e.next_words(Some(Lang::En), "I would ", 3)), ["like"]);
    assert!(e.next_words(Some(Lang::En), "ok ", 3).is_empty());
    e.load_next_words(Lang::En, "");
    assert!(!e.has_next_words(Lang::En));
    assert!(e.next_words(Some(Lang::En), "I would ", 3).is_empty());
}

// ───────────── the user's own habits ─────────────

#[test]
fn the_users_own_pairs_come_first_once_seen_twice() {
    let mut e = engine();
    assert_eq!(texts(e.next_words(Some(Lang::En), "thank ", 3)), ["you"]);
    e.learn_text("Thank them for the help.");
    assert_eq!(texts(e.next_words(Some(Lang::En), "thank ", 3)), ["you"], "one sighting is not a habit");
    e.learn_text("Thank them again.");
    assert_eq!(texts(e.next_words(Some(Lang::En), "thank ", 3)), ["them", "you"]);
    // the more often seen comes first
    for _ in 0..3 {
        e.learn_text("thank goodness");
    }
    assert_eq!(texts(e.next_words(Some(Lang::En), "thank ", 3)), ["goodness", "them", "you"]);
    // no more than asked
    assert_eq!(texts(e.next_words(Some(Lang::En), "thank ", 1)), ["goodness"]);
}

#[test]
fn the_users_pairs_work_without_any_bundled_table_and_never_cross_a_break() {
    let mut e = LocalEngine::new();
    e.add(Lang::En, WordList::parse("the 100\n", 10));
    for _ in 0..2 {
        e.learn_text("Call Positiv now. Then, send it to positiv headquarters\nmeeting notes");
    }
    // seen four times, it is a personal word now, and offered the way it was written in the middle of a sentence
    assert_eq!(texts(e.next_words(Some(Lang::En), "please call ", 3)), ["Positiv"]);
    assert!(!texts(e.next_words(Some(Lang::En), "to ", 3)).contains(&"now".to_string()));
    // `now` ends a sentence: `then` after it is not a pair; `headquarters` / `meeting` are on two lines
    assert!(e.next_words(Some(Lang::En), "now ", 3).is_empty());
    assert!(e.next_words(Some(Lang::En), "headquarters ", 3).is_empty());
    // a personal word is offered as it was added
    e.personal_mut().add_word("POSITIV");
    assert_eq!(texts(e.next_words(Some(Lang::En), "please call ", 3)), ["POSITIV"]);
}

#[test]
fn the_users_pairs_in_arabic() {
    let mut e = engine();
    for _ in 0..2 {
        e.learn_text("أهلا بك في بيتنا");
    }
    assert_eq!(texts(e.next_words(None, "أهلا ", 3)), ["بك"]);
    assert!(e.next_words(Some(Lang::En), "أهلا ", 3).is_empty());
}

// ───────────── the real tables ─────────────

fn real() -> Option<LocalEngine> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let e = LocalEngine::load_dir(&dir).ok()?;
    (e.languages().len() == 3 && e.has_next_words(Lang::En) && e.has_next_words(Lang::Fr) && e.has_next_words(Lang::Ar)).then_some(e)
}

/// (text before the caret, a word that must be among the 3 offered)
const EN: &[(&str, &str)] = &[
    ("I would like ", "to"),
    ("Thank you ", "for"),
    ("Thank you very ", "much"),
    ("How are ", "you"),
    ("Do you want ", "to"),
    ("I don't ", "know"),
    ("What do you ", "think"),
    ("Let me ", "know"),
    ("I am going ", "to"),
    ("I look forward ", "to"),
    ("Please let ", "me"),
];
const FR: &[(&str, &str)] = &[
    ("Je ne ", "sais"),
    ("Merci ", "beaucoup"),
    ("Est-ce que ", "tu"),
    ("Je voudrais ", "un"),
    ("Qu'est-ce que ", "tu"),
    ("Il y ", "a"),
    ("Bonne ", "journée"),
    ("S'il vous ", "plaît"),
    ("Je suis ", "désolé"),
    ("Comment ", "allez"),
    ("Je vais ", "à"),
    ("C'est ", "un"),
];
const AR: &[(&str, &str)] = &[
    ("شكرا ", "جزيلا"),
    ("كيف ", "حالك"),
    ("هل ", "أنت"),
    ("لا ", "أستطيع"),
    ("ما ", "هو"),
    ("أنا ", "لا"),
];

#[test]
fn real_tables_give_sensible_answers() {
    let Some(e) = real() else { return };
    for (lang, cases) in [(Lang::En, EN), (Lang::Fr, FR), (Lang::Ar, AR)] {
        let bad: Vec<String> = cases
            .iter()
            .filter_map(|(typed, want)| {
                let got = texts(e.next_words(Some(lang), typed, 3));
                (!got.iter().any(|g| g == want)).then(|| format!("  {typed:?} -> wanted {want:?}, got {got:?}"))
            })
            .collect();
        // a handful of misses is normal for a table built from translation sentences
        assert!(bad.len() * 4 <= cases.len(), "{lang:?}: {} of {} cases failed:\n{}", bad.len(), cases.len(), bad.join("\n"));
    }
}

#[test]
fn real_tables_do_not_make_up_answers() {
    let Some(e) = real() else { return };
    for typed in ["the zxqvk ", "I would like, ", "I would like", "call 555 ", "mail bob@example.com ", "Tom ", "Mary "] {
        assert!(e.next_words(Some(Lang::En), typed, 3).is_empty(), "{typed:?}");
    }
    // the names of the corpus are never offered
    for typed in ["I want to ask ", "I said to ", "I gave it to ", "Please tell ", "I saw ", "I know "] {
        let got = texts(e.next_words(Some(Lang::En), typed, 6));
        assert!(!got.contains(&"tom".to_string()) && !got.contains(&"Tom".to_string()) && !got.contains(&"mary".to_string()), "{typed:?} -> {got:?}");
    }
}

/// Right, and what a person would expect, but out of reach of these tables: they hold the 6 commonest followers of each word (and of
/// the commonest pairs of words), built from translation sentences, so everyday phrases of a message are often not in them.
/// Run with `-- --ignored` to see which.
#[test]
#[ignore = "documents what the tables cannot do yet"]
fn known_gaps() {
    let Some(e) = real() else { return };
    let cases = [
        (Lang::En, "See you ", "tomorrow"),
        (Lang::En, "Could you please ", "send"),
        (Lang::En, "Happy ", "birthday"),
        (Lang::En, "Good ", "morning"),
        (Lang::Ar, "هل ", "يمكنك"),
        (Lang::Ar, "لا ", "أعرف"),
    ];
    let bad: Vec<String> = cases
        .iter()
        .filter_map(|(lang, typed, want)| {
            let got = texts(e.next_words(Some(*lang), typed, 3));
            (!got.iter().any(|g| g == want)).then(|| format!("  {typed:?} -> wanted {want:?}, got {got:?}"))
        })
        .collect();
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
