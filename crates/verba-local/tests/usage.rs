//! Usage counts: the words of the dictionary that the user writes often are ranked higher among the completions. The proof that an engine
//! whose user never used learning is unchanged is in `golden_suggest.rs`. Small lists are written here, so these tests always run.

use std::path::PathBuf;

use verba_core::Lang;
use verba_local::{LocalEngine, PersonalDict, UsageParams, WordList};

fn texts(v: Vec<verba_core::Suggestion>) -> Vec<String> {
    v.into_iter().map(|s| s.text).collect()
}

/// `the` dominates the total, the others are ordered by their counts.
const EN: &str = "the 1000000\nage 200000\nagree 30000\nagent 20000\nagenda 10000\nagency 3000\nagile 2000\nagenesis 50\n\
                  people 100000\npeers 4000\npeerless 3\nresult 80000\nresume 20000\nrest 100000\nwhich 60000\nwich 10\nwith 200000\n";

fn engine() -> LocalEngine {
    let mut e = LocalEngine::new();
    e.add(Lang::En, WordList::parse_for(Lang::En, EN, 1000));
    e.add(Lang::Ar, WordList::parse_for(Lang::Ar, "كتاب 50000\nكتابة 30000\nكتب 20000\nمدرسة 40000\n", 1000));
    e
}

fn dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("verba-usage-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// The user writes `word` this many times among other words, one sentence at a time.
fn write_often(e: &mut LocalEngine, word: &str, times: usize) {
    for _ in 0..times {
        e.learn_text(&format!("the people with the {word} with the rest."));
    }
}

#[test]
fn a_word_the_user_writes_often_moves_up() {
    let mut e = engine();
    assert_eq!(texts(e.suggest_in(Lang::En, "ag", 3)), ["age", "agree", "agent"]);
    write_often(&mut e, "agency", 30);
    let got = texts(e.suggest_in(Lang::En, "ag", 3));
    assert!(got.contains(&"agency".to_string()), "{got:?}");
    assert_eq!(&got[..1], ["age"], "a very common word still leads");
    // more writing, more rise: it passes the words a few times more frequent than it
    write_often(&mut e, "agency", 300);
    assert_eq!(texts(e.suggest_in(Lang::En, "ag", 3))[0..2], ["age", "agency"]);
    assert!(e.personal().usage_count("agency") > 100.0);
}

#[test]
fn nothing_changes_before_a_word_was_written_twice() {
    let mut e = engine();
    e.learn_text("the agency is here");
    assert_eq!(texts(e.suggest_in(Lang::En, "ag", 3)), ["age", "agree", "agent"]);
    assert!((e.personal().usage_count("agency") - 1.0).abs() < 0.01);
    e.learn_text("an agency");
    assert!(e.personal().usage_count("agency") > 1.9);
}

#[test]
fn the_boost_is_bounded_so_a_rare_word_never_passes_a_much_better_match() {
    let mut e = engine();
    // `peerless` is 1 300 times rarer than `peers`: written a thousand times, it still stays behind
    for _ in 0..1000 {
        e.learn_text("the rest with the peerless.");
    }
    assert!(e.personal().usage_count("peerless") > 900.0);
    assert_eq!(texts(e.suggest_in(Lang::En, "pee", 3))[0], "peers");
    // the same word, with a boost allowed to be 10 000 times, would pass: the bound is what protects
    e.personal_mut().set_usage_params(UsageParams { max_boost: 1.0e6, k: 1.0e6, min_z: 0.0, ..UsageParams::default() });
    assert_eq!(texts(e.suggest_in(Lang::En, "pee", 3))[0], "peerless");
    // and with the bound off (1), nothing moves at all
    e.personal_mut().set_usage_params(UsageParams { max_boost: 1.0, ..UsageParams::default() });
    assert_eq!(texts(e.suggest_in(Lang::En, "pee", 3)), ["peers", "peerless"]);
}

#[test]
fn the_boost_never_exceeds_the_bound() {
    let mut e = engine();
    write_often(&mut e, "agile", 2000);
    // agile is 100 times rarer than age; the default bound is below that, so it cannot lead
    assert!(UsageParams::default().max_boost < 100.0);
    assert_eq!(texts(e.suggest_in(Lang::En, "ag", 1)), ["age"]);
}

#[test]
fn the_order_of_two_words_is_the_same_whatever_the_number_of_letters_typed() {
    // the boost depends on the word, not on how much of it is typed: no flip-flop while the user types
    let mut e = engine();
    write_often(&mut e, "agency", 60);
    write_often(&mut e, "agile", 25);
    let position = |typed: &str, word: &str| texts(e.suggest_in(Lang::En, typed, 8)).iter().position(|w| w == word);
    for (a, b) in [("agency", "agent"), ("agency", "agile"), ("agile", "agenesis"), ("agency", "agenda")] {
        let order = |typed: &str| match (position(typed, a), position(typed, b)) {
            (Some(x), Some(y)) => Some(x < y),
            _ => None,
        };
        let seen: Vec<Option<bool>> = ["ag", "age", "agen", "agi"].iter().map(|t| order(t)).collect();
        let known: Vec<bool> = seen.into_iter().flatten().collect();
        assert!(known.windows(2).all(|w| w[0] == w[1]), "{a} / {b}: {known:?}");
    }
}

#[test]
fn it_follows_the_script_and_the_typing_without_accents_or_hamza() {
    let mut e = engine();
    for _ in 0..40 {
        e.learn_text("كتابة كتابة كتابة كتب مدرسة.");
    }
    // Arabic: `كتا` offers the user's word first, `كتب` is not offered for 3 letters that it does not start with
    let got = texts(e.suggest_in(Lang::Ar, "كتا", 3));
    assert_eq!(got, ["كتابة", "كتاب"], "{got:?}");
    // French without accents finds the accented word the user writes
    let mut f = LocalEngine::new();
    f.add(Lang::Fr, WordList::parse_for(Lang::Fr, "de 1000000\nété 5000\netable 6000\nétage 4000\nétudes 3000\n", 100));
    assert_eq!(texts(f.suggest_in(Lang::Fr, "et", 3))[0], "etable");
    for _ in 0..80 {
        f.learn_text("de été de été de été.");
    }
    assert_eq!(texts(f.suggest_in(Lang::Fr, "et", 3))[0], "été");
    assert_eq!(texts(f.suggest_in(Lang::Fr, "ét", 3))[0], "été");
}

#[test]
fn the_same_refusals_as_before_apply_to_usage() {
    let mut e = engine();
    let junk = "people@age.org people@age.org people@age.org https://agency.com/agent https://agency.com/agent \
                AGENT AGENT AGENT age2 age2 age2 agent_x agent_x agent_x a-b-c a-b-c to to to of of of";
    e.learn_text(junk);
    let dir = dir("refusals");
    e.personal_mut().save(&dir.join("p.txt"), &dir.join("l.txt")).unwrap();
    let text = std::fs::read_to_string(dir.join("l.txt")).unwrap();
    let words: Vec<&str> = text.lines().filter(|l| l.starts_with("u\t")).map(|l| l.split('\t').nth(1).unwrap()).collect();
    assert!(words.is_empty(), "nothing from e-mails, URLs, acronyms, digits, identifiers or short words: {words:?}");
    assert_eq!(e.personal().usage_len(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_word_the_correction_takes_for_a_typo_is_counted_but_never_boosted() {
    let mut e = engine();
    // `wich` is in the list (rare) but is a slip for `which`
    let before = texts(e.suggest_in(Lang::En, "wi", 3));
    write_often(&mut e, "wich", 20);
    let dir = dir("typo");
    e.personal_mut().save(&dir.join("p.txt"), &dir.join("l.txt")).unwrap();
    let text = std::fs::read_to_string(dir.join("l.txt")).unwrap();
    let line = |w: &str| text.lines().find(|l| l.starts_with(&format!("u\t{w}\t"))).map(str::to_string);
    assert!(line("wich").unwrap().ends_with("typo"), "{:?}", line("wich"));
    assert!(line("people").unwrap().ends_with('-'));
    assert_eq!(texts(e.suggest_in(Lang::En, "wi", 3)), before, "a slip is never ranked up");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn counts_fade_and_forgotten_words_leave() {
    let mut e = engine();
    e.personal_mut().set_usage_params(UsageParams { half_life: 50.0, ..UsageParams::default() });
    write_often(&mut e, "agency", 40);
    let fresh = e.personal().usage_count("agency");
    assert!(fresh > 30.0, "{fresh}");
    // 50 sentences later the count has halved; 500 later it is gone from the ranking
    for _ in 0..50 {
        e.learn_text("people want the rest.");
    }
    let half = e.personal().usage_count("agency");
    assert!((half / fresh - 0.5).abs() < 0.05, "{half} of {fresh}");
    for _ in 0..500 {
        e.learn_text("people want the rest.");
    }
    assert!(e.personal().usage_count("agency") < 0.1);
    assert_eq!(texts(e.suggest_in(Lang::En, "ag", 3)), ["age", "agree", "agent"], "an old habit no longer changes the ranking");
}

#[test]
fn the_number_of_counted_words_is_capped_and_the_often_written_survive() {
    let mut e = LocalEngine::new();
    // 6 500 different dictionary words, then one that is written often
    let mut list = String::from("the 100000000\n");
    let word = |n: usize| {
        let mut n = n;
        let mut s = String::from("zz");
        loop {
            s.push((b'a' + (n % 26) as u8) as char);
            n /= 26;
            if n == 0 {
                return s;
            }
        }
    };
    for n in 0..6500 {
        list.push_str(&format!("{} {}\n", word(n), 1000 + n));
    }
    e.add(Lang::En, WordList::parse(&list, 10_000));
    write_often_plain(&mut e, &word(0), 30);
    for chunk in 0..65 {
        let text: Vec<String> = (0..100).map(|i| word(chunk * 100 + i)).collect();
        e.learn_text(&text.join(" "));
    }
    assert!(e.personal().usage_len() <= 5000, "{}", e.personal().usage_len());
    assert!(e.personal().usage_count(&word(0)) > 20.0, "the word written often is kept");
}

fn write_often_plain(e: &mut LocalEngine, word: &str, times: usize) {
    for _ in 0..times {
        e.learn_text(&format!("the {word} the {word}."));
    }
}

#[test]
fn clearing_what_was_learned_clears_the_usage_and_gives_the_old_answers_back() {
    let mut e = engine();
    let before: Vec<Vec<String>> = ["ag", "peo", "re", "wi"].iter().map(|t| texts(e.suggest_in(Lang::En, t, 3))).collect();
    write_often(&mut e, "agency", 300);
    assert!(e.personal().usage_len() > 0);
    e.personal_mut().clear_learned();
    assert_eq!(e.personal().usage_len(), 0);
    let after: Vec<Vec<String>> = ["ag", "peo", "re", "wi"].iter().map(|t| texts(e.suggest_in(Lang::En, t, 3))).collect();
    assert_eq!(before, after);
}

#[test]
fn removing_a_word_forgets_its_usage() {
    let mut e = engine();
    write_often(&mut e, "agency", 50);
    assert!(e.personal_mut().remove_word("agency"));
    assert_eq!(e.personal().usage_count("agency"), 0.0);
    write_often(&mut e, "agency", 50);
    assert_eq!(e.personal().usage_count("agency"), 0.0, "and it is not counted again");
}

// ───────────── files ─────────────

#[test]
fn usage_is_saved_in_plain_text_and_comes_back() {
    let d = dir("roundtrip");
    let (w, l) = (d.join("personal.txt"), d.join("learned.txt"));
    let mut e = engine();
    write_often(&mut e, "agency", 40);
    e.personal_mut().save(&w, &l).unwrap();
    let text = std::fs::read_to_string(&l).unwrap();
    assert!(text.lines().any(|l| l.starts_with("utotal\t")));
    assert!(text.lines().any(|l| l.starts_with("u\tagency\t")), "{text}");
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        assert!(!line.contains(' '), "no sentence: {line:?}");
    }
    let mut e2 = engine();
    e2.set_personal(PersonalDict::load(&w, &l));
    assert!((e2.personal().usage_count("agency") - e.personal().usage_count("agency")).abs() < 0.01);
    assert_eq!(texts(e.suggest_in(Lang::En, "ag", 3)), texts(e2.suggest_in(Lang::En, "ag", 3)));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn files_written_before_usage_existed_still_load() {
    let d = dir("old");
    let (w, l) = (d.join("personal.txt"), d.join("learned.txt"));
    std::fs::write(&w, "Positiv\n").unwrap();
    // the 0.4.0 format: words and pairs only
    std::fs::write(
        &l,
        "# Verba learned words (word, how often, when) and word pairs. No sentences are stored.\n# Delete this file to forget everything that was learned.\n\
         clock\t12\nw\tzorblax\tzorblax\t4\t9\t-\nb\tthank\tyou\t3\t11\n",
    )
    .unwrap();
    let mut e = engine();
    e.set_personal(PersonalDict::load(&w, &l));
    assert!(e.personal().contains("zorblax") && e.personal().contains("Positiv"));
    assert_eq!(e.personal().usage_len(), 0);
    assert_eq!(texts(e.suggest_in(Lang::En, "ag", 3)), ["age", "agree", "agent"]);
    // learning goes on from there
    write_often(&mut e, "agency", 5);
    e.personal_mut().save(&w, &l).unwrap();
    let again = PersonalDict::load(&w, &l);
    assert!(again.contains("zorblax") && again.usage_count("agency") > 4.0);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn damaged_usage_lines_are_skipped_and_the_rest_loads() {
    let d = dir("damaged");
    let (w, l) = (d.join("personal.txt"), d.join("learned.txt"));
    std::fs::write(
        &l,
        "clock\t5\nutotal\tnan\t3\nutotal\tabc\nu\tagency\t7.50\t4\t-\nu\tbad\nu\t\t\t\t\nu\tx1y\t3\t1\t-\nu\tneg\t-2\t1\t-\nu\tinf\tinf\t1\t-\n\
         u\tpeople\t1e40\t1\t-\nu\tAGE\t3\t1\t-\nfuture\tline\twe\tdo\tnot\tknow\nu\tagent\t2.00\t5\ttypo\n\u{0}\u{1}garbage\n",
    )
    .unwrap();
    let p = PersonalDict::load(&w, &l);
    assert!((p.usage_count("agency") - 7.5).abs() < 0.2);
    assert_eq!(p.usage_count("bad"), 0.0);
    assert_eq!(p.usage_count("x1y"), 0.0);
    assert_eq!(p.usage_count("neg"), 0.0);
    assert!(p.usage_count("people") <= 1_000_000.0);
    assert_eq!(p.usage_count("AGE"), 0.0, "keys are written folded: an upper-case key is not one");
    // binary garbage in place of the file
    std::fs::write(&l, [0xffu8, 0xfe, 0, 1, 2, 3, b'u', b'\t', 255]).unwrap();
    assert_eq!(PersonalDict::load(&w, &l).usage_len(), 0);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn the_words_file_never_holds_usage() {
    let d = dir("words-file");
    let (w, l) = (d.join("personal.txt"), d.join("learned.txt"));
    let mut e = engine();
    e.personal_mut().add_word("Positiv");
    write_often(&mut e, "agency", 20);
    e.personal_mut().save(&w, &l).unwrap();
    assert_eq!(std::fs::read_to_string(&w).unwrap().lines().filter(|l| !l.starts_with('#')).collect::<Vec<_>>(), ["Positiv"]);
    assert!(e.personal().words() == ["Positiv"], "usage words are not personal words: {:?}", e.personal().words());
    let _ = std::fs::remove_dir_all(d);
}

// ───────────── the user's own pairs ─────────────

#[test]
fn pairs_the_user_repeats_come_first() {
    let mut e = engine();
    e.load_next_words(Lang::En, "thank\t100\tyou:90 god:6 them:4\n");
    assert_eq!(texts(e.next_words(Some(Lang::En), "thank ", 3)), ["you", "god", "them"]);
    e.learn_text("thank goodness.");
    assert_eq!(texts(e.next_words(Some(Lang::En), "thank ", 1)), ["you"], "once is not a habit");
    e.learn_text("thank goodness.");
    assert_eq!(texts(e.next_words(Some(Lang::En), "thank ", 3)), ["goodness", "you", "god"]);
    for _ in 0..3 {
        e.learn_text("thank them.");
    }
    assert_eq!(texts(e.next_words(Some(Lang::En), "thank ", 2)), ["them", "goodness"]);
}
