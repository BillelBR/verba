//! The personal dictionary: words the user keeps, optional learning, and the files. Most tests use small word lists written here, so they
//! always run; the ones on the real lists skip when the lists were not downloaded (`scripts/fetch-data.ps1`).

use std::path::{Path, PathBuf};

use verba_core::{Engine, Lang};
use verba_local::{LocalEngine, PersonalDict, WordList};

fn texts(v: Vec<verba_core::Suggestion>) -> Vec<String> {
    v.into_iter().map(|s| s.text).collect()
}

/// A small English list: very common words, a few ordinary ones.
fn small_engine() -> LocalEngine {
    let mut e = LocalEngine::new();
    e.add(
        Lang::En,
        WordList::parse_for(
            Lang::En,
            "the 1000000000\nyou 900000\npossible 5000000\nposition 100000\npositive 20000\npostcard 30\npose 400000\nreceive 20000\nwant 50000\nwhich 60000\nbecause 40000\nmeeting 30000\n",
            1000,
        ),
    );
    e.add(Lang::Ar, WordList::parse_for(Lang::Ar, "كتاب 50000\nمدرسة 40000\nمرحبا 30000\n", 1000));
    e
}

fn real_engine() -> Option<LocalEngine> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let e = LocalEngine::load_dir(&dir).ok()?;
    (e.languages().len() == 3).then_some(e)
}

/// A fresh folder in the temp directory, removed at the end of the test.
struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> Self {
        let p = std::env::temp_dir().join(format!("verba-personal-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ───────────── the words the user keeps ─────────────

#[test]
fn words_are_added_found_listed_and_removed() {
    let mut d = PersonalDict::new();
    assert!(d.add_word("Positiv"));
    assert!(!d.add_word("Positiv"), "the same word again is not new");
    assert!(d.add_word("مرحبا"));
    assert!(d.contains("positiv") && d.contains("POSITIV") && d.contains("Positiv"));
    assert!(d.contains("مَرْحَبًا"), "short vowels do not matter");
    assert_eq!(d.words(), ["Positiv", "مرحبا"]);
    assert!(d.remove_word("POSITIV"));
    assert!(!d.contains("Positiv"));
    assert!(!d.remove_word("Positiv"), "already gone");
    assert_eq!(d.words(), ["مرحبا"]);
}

#[test]
fn only_single_words_can_be_added() {
    let mut d = PersonalDict::new();
    for bad in ["", " ", "a", "two words", "abc123", "x@y.org", "http://a.b", "line\nbreak", "-dash", "tail-", "ab\u{627}c"] {
        assert!(!d.add_word(bad), "{bad:?} must be refused");
    }
    assert!(d.words().is_empty());
    assert!(d.add_word("  O'Brien  "));
    assert!(d.add_word("anti-spam"));
    assert_eq!(d.words(), ["anti-spam", "O'Brien"]);
}

#[test]
fn a_personal_word_is_never_flagged_as_a_typo() {
    let mut e = small_engine();
    let flagged = |e: &LocalEngine| e.correct(None, "I recieve ", 3).is_some();
    assert!(flagged(&e), "this typo is flagged without the word");
    e.personal_mut().add_word("recieve");
    assert!(!flagged(&e));
    e.personal_mut().remove_word("recieve");
    assert!(flagged(&e), "removing the word brings the correction back");
}

#[test]
fn ignore_word_stays_a_session_only_variant() {
    let mut e = small_engine();
    e.ignore_word("recieve");
    assert!(e.correct(None, "I recieve ", 3).is_none());
    assert!(e.personal().words().is_empty(), "an ignored word is not a personal word and is never saved");
    assert!(!e.personal().contains("recieve"));
}

#[test]
fn personal_words_are_offered_and_keep_their_capitals() {
    let mut e = small_engine();
    e.personal_mut().add_word("Positiv");
    e.personal_mut().add_word("iPhone");
    // a longer prefix: the user's word comes before the rare dictionary words
    assert_eq!(texts(e.suggest_in(Lang::En, "see posit", 3))[0], "Positiv");
    // typed with a capital or in capitals
    assert_eq!(texts(e.suggest_in(Lang::En, "see POSIT", 3))[0], "POSITIV");
    // a word that has capitals of its own keeps them whatever was typed
    assert_eq!(texts(e.suggest_in(Lang::En, "got an iph", 1)), ["iPhone"]);
    assert_eq!(texts(e.suggest_in(Lang::En, "got an Iph", 1)), ["iPhone"]);
    // the word typed in full is not offered back
    assert!(!texts(e.suggest_in(Lang::En, "Positiv", 3)).contains(&"Positiv".to_string()));
    // removed: gone
    e.personal_mut().remove_word("positiv");
    assert!(!texts(e.suggest_in(Lang::En, "see posit", 3)).contains(&"Positiv".to_string()));
}

#[test]
fn a_very_common_word_still_wins_on_a_short_prefix_but_a_rare_one_does_not() {
    let mut e = small_engine();
    e.personal_mut().add_word("Postiv");
    // `pos`: `possible` and `pose` are common words and beat the user's word; the less common `position` and the rare `positive`
    // and `postcard` do not
    let got = texts(e.suggest_in(Lang::En, "x pos", 3));
    assert_eq!(got, ["possible", "pose", "Postiv"]);
    assert_eq!(texts(e.suggest_in(Lang::En, "x post", 3)), ["Postiv", "postcard"]);
    // the same on the real lists: a short prefix gives common words, a longer one the user's own word
    let Some(mut r) = real_engine() else { return };
    r.personal_mut().add_word("Positiv");
    let two = texts(r.suggest_in(Lang::En, "see po", 3));
    assert!(two.contains(&"people".to_string()) || two.contains(&"police".to_string()), "{two:?}");
    assert_eq!(texts(r.suggest_in(Lang::En, "see posi", 3))[0], "Positiv");
    let three = texts(r.suggest_in(Lang::En, "see pos", 3));
    assert!(three.contains(&"Positiv".to_string()), "{three:?}");
}

#[test]
fn personal_words_follow_their_script() {
    let mut e = small_engine();
    e.personal_mut().add_word("كتابتي");
    e.personal_mut().add_word("Katab");
    // Arabic words are offered for Arabic typing only, Latin words for Latin typing only
    assert!(texts(e.suggest_in(Lang::Ar, "كتاب", 3)).contains(&"كتابتي".to_string()));
    assert!(!texts(e.suggest_in(Lang::En, "kata", 3)).contains(&"كتابتي".to_string()));
    assert!(texts(e.suggest_in(Lang::En, "kata", 3)).contains(&"Katab".to_string()));
    assert!(texts(e.suggest_in(Lang::Ar, "كتا", 3)).iter().all(|w| w != "Katab"));
    // Arabic: the hamza and dots left out still find the word
    e.personal_mut().add_word("أسامة");
    assert!(texts(e.suggest_in(Lang::Ar, "اسا", 3)).contains(&"أسامة".to_string()));
}

#[test]
fn a_language_that_is_off_offers_no_personal_word() {
    let mut e = small_engine();
    e.personal_mut().add_word("Positiv");
    e.set_enabled(Lang::En, false);
    assert!(e.suggest("see posit", 3).is_empty());
}

#[test]
fn the_real_lists_still_answer_the_same_when_the_dictionary_is_empty() {
    let Some(e) = real_engine() else { return };
    assert!(texts(e.suggest("I would lik", 3)).contains(&"like".to_string()));
    assert!(e.personal().words().is_empty());
}

// ───────────── learning ─────────────

fn learn(e: &mut LocalEngine, text: &str, times: usize) {
    for _ in 0..times {
        e.learn_text(text);
    }
}

#[test]
fn a_word_becomes_personal_after_three_sightings() {
    let mut e = small_engine();
    e.learn_text("We are testing Zorblax today");
    e.learn_text("zorblax again");
    assert!(!e.personal().contains("zorblax"), "seen twice: not yet");
    assert!(e.suggest_in(Lang::En, "zorb", 3).is_empty());
    e.learn_text("and zorblax once more");
    assert!(e.personal().contains("zorblax"));
    assert_eq!(e.personal().learned_words().len(), 1);
    assert!(e.personal().is_learned("zorblax"));
    assert_eq!(texts(e.suggest_in(Lang::En, "zorb", 3)), ["Zorblax"], "offered as it was first written in the middle of a sentence");
    // and it is never flagged
    assert!(e.correct(None, "we use zorblax ", 3).is_none());
}

#[test]
fn three_sightings_in_one_text_count() {
    let mut e = small_engine();
    e.learn_text("blorptastic blorptastic blorptastic");
    assert!(e.personal().contains("blorptastic"));
}

#[test]
fn words_of_the_dictionary_and_other_things_are_never_learned() {
    let mut e = small_engine();
    let junk = "the the the possible possible possible \
        12345 12345 12345 \
        https://example.com/page https://example.com/page https://example.com/page \
        bob@example.com bob@example.com bob@example.com \
        C:\\Users\\bob\\notes.txt C:\\Users\\bob\\notes.txt C:\\Users\\bob\\notes.txt \
        /usr/local/bin /usr/local/bin /usr/local/bin \
        deadbeefcafe deadbeefcafe deadbeefcafe \
        abc123def abc123def abc123def \
        NASA NASA NASA \
        getUserNameFromDb getUserNameFromDb getUserNameFromDb \
        snake_case_name snake_case_name snake_case_name \
        aaaah aaaah aaaah \
        supercalifragilisticexpialidocious supercalifragilisticexpialidocious supercalifragilisticexpialidocious \
        xy xy xy \
        well-known-thing well-known-thing well-known-thing";
    e.learn_text(junk);
    assert!(e.personal().words().is_empty(), "learned: {:?}", e.personal().words());
}

#[test]
fn a_word_typed_three_times_that_looks_like_a_typo_is_not_learned() {
    let mut e = small_engine();
    for _ in 0..4 {
        e.learn_text("I recieve it");
    }
    assert!(!e.personal().contains("recieve"), "the correction takes it for a typo of `receive`");
    // but the user can still keep it on purpose
    e.personal_mut().add_word("recieve");
    assert!(e.personal().contains("recieve"));
}

#[test]
fn a_word_the_user_removed_is_not_learned_again() {
    let mut e = small_engine();
    learn(&mut e, "zorblax", 3);
    assert!(e.personal().contains("zorblax"));
    assert!(e.personal_mut().remove_word("zorblax"));
    learn(&mut e, "zorblax", 5);
    assert!(!e.personal().contains("zorblax"));
    // adding it by hand lifts the ban
    assert!(e.personal_mut().add_word("zorblax"));
    assert!(e.personal().contains("zorblax"));
}

#[test]
fn learned_words_keep_a_capital_typed_mid_sentence() {
    let mut e = small_engine();
    e.learn_text("Zorblax is nice. Zorblax is big. Zorblax was here and we saw Quillon go. Quillon came back. Then Quillon left.");
    let words = e.personal().words();
    assert!(words.contains(&"Zorblax".to_string()) || words.contains(&"zorblax".to_string()), "{words:?}");
    // `Quillon` was seen capitalised once in the middle of a sentence: that is how it is offered
    assert!(e.personal().learned_words().contains(&"Quillon".to_string()), "{:?}", e.personal().learned_words());
}

#[test]
fn arabic_words_are_learned_per_script() {
    let mut e = small_engine();
    learn(&mut e, "ذهبت إلى الزقزوقة", 3);
    assert!(e.personal().contains("الزقزوقة"));
    assert!(texts(e.suggest_in(Lang::Ar, "الزقز", 3)).contains(&"الزقزوقة".to_string()));
    // a known word with a particle attached is a known word
    learn(&mut e, "بالكتاب", 3);
    assert!(!e.personal().contains("بالكتاب"));
}

#[test]
fn the_number_of_learned_words_and_pairs_is_capped() {
    let mut e = small_engine();
    // 7000 different words, each seen once, then one word seen often: the often-seen one survives
    for chunk in 0..70 {
        let text: Vec<String> = (0..100).map(|i| format!("qq{}", letters(chunk * 100 + i))).collect();
        e.learn_text(&text.join(" "));
    }
    learn(&mut e, "survivor", 3);
    let (counting, learned, pairs) = e.personal().learned_sizes();
    assert!(counting + learned <= 5000, "{counting} + {learned}");
    assert!(pairs <= 20_000);
    assert!(e.personal().contains("survivor"), "a promoted word is evicted last");
    // pairs: a long text of distinct pairs is capped too
    let many: Vec<String> = (0..30_000).map(|i| format!("pp{}", letters(i))).collect();
    for part in many.chunks(1000) {
        e.learn_text(&part.join(" "));
    }
    assert!(e.personal().learned_sizes().2 <= 20_000);
}

/// Digits are not allowed in learned words: write a number in letters.
fn letters(n: usize) -> String {
    let mut n = n;
    let mut s = String::new();
    loop {
        s.push((b'a' + (n % 26) as u8) as char);
        n /= 26;
        if n == 0 {
            return s;
        }
    }
}

#[test]
fn clear_learned_forgets_what_was_learned_and_keeps_what_was_added() {
    let mut e = small_engine();
    e.personal_mut().add_word("Positiv");
    learn(&mut e, "thank you zorblax", 3);
    assert!(e.personal().contains("zorblax"));
    assert!(!e.next_words(Some(Lang::En), "thank ", 3).is_empty());
    e.personal_mut().clear_learned();
    assert!(!e.personal().contains("zorblax"));
    assert!(e.personal().contains("Positiv"));
    assert_eq!(e.personal().learned_sizes(), (0, 0, 0));
    assert!(e.next_words(Some(Lang::En), "thank ", 3).is_empty());
}

// ───────────── files ─────────────

#[test]
fn files_round_trip_and_are_plain_text() {
    let dir = TempDir::new("roundtrip");
    let (words, learned) = (dir.file("sub").join("personal.txt"), dir.file("sub").join("learned.txt"));
    let mut e = small_engine();
    e.personal_mut().add_word("Positiv");
    e.personal_mut().add_word("مرحبتين");
    learn(&mut e, "we love zorblax and zorblax loves us", 3);
    e.personal_mut().remove_word("loves");
    assert!(e.personal().is_dirty());
    e.personal_mut().save(&words, &learned).unwrap();
    assert!(!e.personal().is_dirty());
    assert!(!dir.file("sub").join("personal.txt.tmp").exists() && !dir.file("sub").join("learned.txt.tmp").exists(), "no temp file is left");

    let text = std::fs::read_to_string(&words).unwrap();
    assert!(text.lines().any(|l| l == "Positiv") && text.lines().any(|l| l == "مرحبتين"));
    let again = PersonalDict::load(&words, &learned);
    assert_eq!(again.words(), e.personal().words());
    assert_eq!(again.learned_sizes(), e.personal().learned_sizes());
    assert!(!again.is_dirty());
    // the pairs came back too
    let mut e2 = small_engine();
    e2.set_personal(again);
    assert_eq!(e.next_words(Some(Lang::En), "we ", 3), e2.next_words(Some(Lang::En), "we ", 3));
    // saving again with nothing changed writes nothing
    let before = std::fs::metadata(&words).unwrap().modified().unwrap();
    e2.personal_mut().save(&words, &learned).unwrap();
    assert_eq!(std::fs::metadata(&words).unwrap().modified().unwrap(), before);
}

#[test]
fn the_words_file_can_be_edited_by_hand() {
    let dir = TempDir::new("edit");
    let words = dir.file("personal.txt");
    // a byte order mark and Windows line endings, as Notepad writes them; comments; a duplicate; junk lines
    std::fs::write(&words, "\u{feff}# my words\r\nSnow\r\n\r\n  Positiv  \r\nsnow\r\nnot a word\r\n123\r\n- Zorblax\r\n").unwrap();
    let d = PersonalDict::load(&words, &dir.file("missing.txt"));
    assert_eq!(d.words(), ["Positiv", "snow"], "the last of two spellings of a word wins");
    let mut e = small_engine();
    e.set_personal(d);
    learn(&mut e, "zorblax", 5);
    assert!(!e.personal().contains("zorblax"), "a `-` line forbids learning the word");
}

#[test]
fn missing_and_corrupt_files_give_an_empty_or_smaller_dictionary() {
    let dir = TempDir::new("corrupt");
    let (w, l) = (dir.file("personal.txt"), dir.file("learned.txt"));
    assert!(PersonalDict::load(&w, &l).words().is_empty(), "missing files");
    // binary garbage, not UTF-8
    std::fs::write(&w, [0xff, 0xfe, 0x00, 0x80, b'\n', b'o', b'k', b'a', b'y', b'\n', 0xc3, 0x28]).unwrap();
    std::fs::write(&l, [0u8, 1, 2, 255, 254, 10, 119, 9, 9, 9]).unwrap();
    let d = PersonalDict::load(&w, &l);
    assert_eq!(d.words(), ["okay"]);
    // half-written lines, bad numbers, impossible words, huge counts
    std::fs::write(
        &l,
        "clock\tnotanumber\nw\tgoodword\tgoodword\t5\t3\t-\nw\tbad\nw\t\t\t\t\t\nw\tx1y\tx1y\t9\t9\t-\nb\tgood\tpair\t4\t2\nb\tonly\nb\t\t\t\t\nw\thuge\thuge\t99999999999999\t1\t-\nw\tneg\tneg\t-3\t1\t-\n",
    )
    .unwrap();
    let d = PersonalDict::load(&w, &l);
    assert!(d.contains("goodword"));
    assert!(!d.contains("x1y") && !d.contains("neg") && !d.contains("bad"));
    assert_eq!(d.learned_sizes().2, 1);
    // a folder where a file should be: no panic, an empty dictionary
    assert!(PersonalDict::load(&dir.0, &dir.0).words().is_empty());
}

#[test]
fn saving_into_a_missing_folder_creates_it_and_a_failure_is_an_error_not_a_panic() {
    let dir = TempDir::new("mkdir");
    let mut d = PersonalDict::new();
    d.add_word("Positiv");
    d.save(&dir.file("a").join("b").join("personal.txt"), &dir.file("a").join("b").join("learned.txt")).unwrap();
    assert!(dir.file("a").join("b").join("personal.txt").exists());
    // the target is a folder: the rename fails, the error comes back and no temp file stays
    let blocked = dir.file("blocker");
    std::fs::create_dir_all(&blocked).unwrap();
    d.add_word("Another");
    assert!(d.save(&blocked, &dir.file("l.txt")).is_err());
    assert!(!dir.file("blocker.tmp").exists());
    assert!(d.is_dirty(), "still unsaved");
}

#[test]
fn what_is_stored_never_holds_a_sentence() {
    let dir = TempDir::new("privacy");
    let (words, learned) = (dir.file("personal.txt"), dir.file("learned.txt"));
    let mut e = small_engine();
    e.personal_mut().add_word("Positiv");
    let secret = "my password is hunter two and the meeting about zorblax is on friday";
    learn(&mut e, secret, 3);
    e.personal_mut().save(&words, &learned).unwrap();
    for path in [&words, &learned] {
        let text = std::fs::read_to_string(path).unwrap();
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            // every line is one word, or one record of single words and numbers separated by tabs: never words separated by spaces
            assert!(!line.contains(' ') || line.starts_with("- "), "{line:?}");
            let longest_run = line.split('\t').filter(|f| f.chars().all(char::is_alphabetic)).count();
            assert!(longest_run <= 3, "{line:?}");
        }
        assert!(!text.contains("hunter two"), "no sentence in {path:?}");
    }
    // only pairs of neighbours are kept, never three words in a row
    let text = std::fs::read_to_string(&learned).unwrap();
    assert!(text.contains("b\tzorblax\tis"));
    assert!(!text.contains("zorblax\tis\ton"));
}

#[test]
fn learning_reads_only_the_end_of_a_very_long_text() {
    let mut e = small_engine();
    let mut text = String::from("zorblax zorblax zorblax ");
    text.push_str(&"filler ".repeat(10_000));
    e.learn_text(&text);
    assert!(!e.personal().contains("zorblax"), "the start of a text longer than 20 000 characters is not read");
    e.learn_text("small zorblax zorblax zorblax");
    assert!(e.personal().contains("zorblax"));
}

#[test]
fn engine_works_with_the_trait_and_without_any_personal_data() {
    let e = small_engine();
    assert_eq!(texts(e.suggest("see posi", 3)), ["position", "positive"]);
    assert!(e.personal().words().is_empty());
}
