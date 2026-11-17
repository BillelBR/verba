//! The bilingual dictionary (`data/words/dict.txt`, built by `scripts/fetch-dict.ps1`), checked on the real file. The tests skip when it was not
//! downloaded, like the other real-data tests. The engine side (lookup, folding, the way through English) is tested on a small fixture in
//! `src/translate.rs`; here the question is whether the DATA is good: well-known words in every direction, inflected forms, and a plausible
//! first candidate for the commonest nouns. Numbers on a larger sample are in docs/ACCURACY.md ("Word translation").

use std::path::Path;

use verba_core::Lang;
use verba_local::LocalEngine;

fn engine() -> Option<LocalEngine> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let e = LocalEngine::load_dir(&dir).ok()?;
    e.has_dictionary().then_some(e)
}

/// The translations of `word` into `to` (with `hint` as the keyboard language), lower case, most common first.
fn tr(e: &LocalEngine, word: &str, to: Lang, hint: Option<Lang>) -> Vec<String> {
    e.translate_word(word, &[to], hint).map(|t| t.candidates.into_iter().map(|c| c.text.to_lowercase()).collect()).unwrap_or_default()
}

/// Cases `(word, [acceptable first candidates])`; returns the ones that fail, as readable lines.
fn first_fails(e: &LocalEngine, from: Lang, to: Lang, cases: &[(&str, &[&str])]) -> Vec<String> {
    cases
        .iter()
        .filter_map(|(w, ok)| {
            let got = tr(e, w, to, Some(from));
            (!got.first().is_some_and(|f| ok.contains(&f.as_str()))).then(|| format!("  {w:?} -> {got:?}, wanted one of {ok:?} first"))
        })
        .collect()
}

fn assert_firsts(name: &str, e: &LocalEngine, from: Lang, to: Lang, cases: &[(&str, &[&str])]) {
    let bad = first_fails(e, from, to, cases);
    assert!(bad.is_empty(), "{name}: {} of {} cases failed:\n{}", bad.len(), cases.len(), bad.join("\n"));
}

/// `word` must be among the first `n` candidates.
fn assert_top(e: &LocalEngine, word: &str, from: Lang, to: Lang, n: usize, want: &[&str]) {
    let got = tr(e, word, to, Some(from));
    assert!(got.iter().take(n).any(|g| want.contains(&g.as_str())), "{word:?} {}>{}: wanted one of {want:?} in the first {n}, got {got:?}", from.code(), to.code());
}

#[test]
fn the_dictionary_is_large_enough_in_every_direction() {
    let Some(e) = engine() else { return };
    let d = e.dictionary();
    for (a, b, min) in [(Lang::En, Lang::Fr, 60_000), (Lang::Fr, Lang::En, 60_000), (Lang::En, Lang::Ar, 25_000), (Lang::Ar, Lang::En, 25_000)] {
        assert!(d.len(a, b) >= min, "{}>{} has only {} headwords", a.code(), b.code(), d.len(a, b));
    }
}

#[test]
fn well_known_words_english_to_french_and_back() {
    let Some(e) = engine() else { return };
    assert_firsts(
        "en>fr",
        &e,
        Lang::En,
        Lang::Fr,
        &[("dog", &["chien"]), ("water", &["eau"]), ("house", &["maison"]), ("book", &["livre"]), ("school", &["école"]), ("bread", &["pain"]), ("friend", &["ami"]), ("thank you", &["merci"])],
    );
    assert_firsts(
        "fr>en",
        &e,
        Lang::Fr,
        Lang::En,
        &[("chien", &["dog"]), ("eau", &["water"]), ("maison", &["house", "home"]), ("livre", &["book"]), ("école", &["school"]), ("pain", &["bread"]), ("ami", &["friend"]), ("merci", &["thanks", "thank you"])],
    );
    // the greeting goes both ways
    assert_top(&e, "hello", Lang::En, Lang::Fr, 3, &["bonjour", "salut", "allô"]);
    assert_top(&e, "bonjour", Lang::Fr, Lang::En, 2, &["hello", "good day", "good morning"]);
}

#[test]
fn well_known_words_with_arabic() {
    let Some(e) = engine() else { return };
    assert_firsts("en>ar", &e, Lang::En, Lang::Ar, &[("water", &["ماء"]), ("book", &["كتاب"]), ("dog", &["كلب"]), ("door", &["باب"]), ("friend", &["صديق"]), ("school", &["مدرسة"])]);
    assert_firsts("ar>en", &e, Lang::Ar, Lang::En, &[("ماء", &["water"]), ("كتاب", &["book"]), ("كلب", &["dog"]), ("باب", &["door"]), ("صديق", &["friend"]), ("مدرسة", &["school"])]);
    assert_top(&e, "hello", Lang::En, Lang::Ar, 3, &["مرحبا", "أهلا"]);
    assert_top(&e, "thank you", Lang::En, Lang::Ar, 2, &["شكرا", "شكرا لك"]);
    assert_top(&e, "شكرا", Lang::Ar, Lang::En, 3, &["thanks", "thank you", "thank"]);
}

#[test]
fn french_and_arabic_meet_directly_for_common_words_and_through_english_for_the_rest() {
    let Some(e) = engine() else { return };
    assert_firsts("fr>ar", &e, Lang::Fr, Lang::Ar, &[("chien", &["كلب"]), ("eau", &["ماء"]), ("livre", &["كتاب"]), ("pharmacie", &["صيدلية"]), ("mairie", &["بلدية"])]);
    assert_firsts("ar>fr", &e, Lang::Ar, Lang::Fr, &[("كتاب", &["livre"]), ("ماء", &["eau"]), ("مدرسة", &["école"]), ("باب", &["porte"])]);
    // the direct list is used for a common word, English for a form that has no direct line
    let t = e.translate_word("chien", &[Lang::Ar], Some(Lang::Fr)).unwrap();
    assert!(!t.via_english);
    let t = e.translate_word("parlons", &[Lang::Ar], Some(Lang::Fr)).unwrap();
    assert!(!t.candidates.is_empty());
}

#[test]
fn inflected_forms_carry_the_translations_of_their_lemma() {
    let Some(e) = engine() else { return };
    assert_top(&e, "dogs", Lang::En, Lang::Fr, 1, &["chien"]);
    assert_top(&e, "went", Lang::En, Lang::Fr, 2, &["aller", "partir"]);
    assert_top(&e, "children", Lang::En, Lang::Fr, 2, &["enfant", "gamin"]);
    assert_top(&e, "parlons", Lang::Fr, Lang::En, 3, &["speak", "talk"]);
    assert_top(&e, "chiens", Lang::Fr, Lang::En, 2, &["dog", "dogs"]);
    assert_top(&e, "mangeons", Lang::Fr, Lang::En, 2, &["eat"]);
    assert_top(&e, "الكتاب", Lang::Ar, Lang::En, 3, &["book", "the book"]);
    assert_top(&e, "كتبت", Lang::Ar, Lang::En, 3, &["write", "wrote", "book", "books"]);
}

#[test]
fn accents_case_hamza_and_short_vowels_do_not_matter() {
    let Some(e) = engine() else { return };
    assert_top(&e, "ecole", Lang::Fr, Lang::En, 1, &["school"]);
    assert_top(&e, "ÉCOLE", Lang::Fr, Lang::En, 1, &["school"]);
    assert_top(&e, "Dog,", Lang::En, Lang::Fr, 1, &["chien"]);
    // short vowels written, and a hamza left out
    assert_top(&e, "كِتَاب", Lang::Ar, Lang::En, 1, &["book"]);
    assert_top(&e, "أُمّ", Lang::Ar, Lang::En, 2, &["mother", "mum"]);
    assert_top(&e, "ام", Lang::Ar, Lang::En, 2, &["mother", "mum"]);
}

#[test]
fn a_word_with_several_meanings_shows_them() {
    let Some(e) = engine() else { return };
    // `bank`: the building and the edge of a river
    assert_top(&e, "bank", Lang::En, Lang::Fr, 1, &["banque"]);
    let got = tr(&e, "bank", Lang::Fr, Some(Lang::En));
    assert!(got.iter().any(|g| g == "rive" || g == "berge"), "{got:?}");
    // `light`: lamp, not heavy, not dark
    let got = tr(&e, "light", Lang::Fr, Some(Lang::En));
    for want in ["lumière", "léger"] {
        assert!(got.iter().any(|g| g == want), "light -> {got:?} lacks {want}");
    }
}

#[test]
fn everyday_words_an_algerian_arabic_speaker_looks_up() {
    let Some(e) = engine() else { return };
    assert_firsts(
        "fr>ar",
        &e,
        Lang::Fr,
        Lang::Ar,
        &[("bonjour", &["مرحبا", "أهلا"]), ("merci", &["شكرا"]), ("pharmacie", &["صيدلية"]), ("voiture", &["سيارة"]), ("mosquée", &["مسجد"]), ("demain", &["غدا"]), ("médecin", &["دكتور", "طبيب"]), ("mairie", &["بلدية"])],
    );
    assert_top(&e, "ordonnance", Lang::Fr, Lang::Ar, 3, &["وصفة", "وصفة طبية"]);
    assert_firsts("en>ar", &e, Lang::En, Lang::Ar, &[("good morning", &["صباح الخير"]), ("tomorrow", &["غدا"]), ("market", &["سوق"]), ("bread", &["خبز"]), ("money", &["مال", "نقود"])]);
}

/// The commonest nouns of each language and the translations a reader would accept as the FIRST one.
#[test]
fn the_commonest_nouns_get_a_plausible_first_candidate() {
    let Some(e) = engine() else { return };
    assert_firsts(
        "en>fr",
        &e,
        Lang::En,
        Lang::Fr,
        &[
            ("time", &["temps", "fois"]),
            ("man", &["homme"]),
            ("thing", &["chose"]),
            ("way", &["façon", "manière", "chemin", "voie"]),
            ("life", &["vie"]),
            ("day", &["jour", "journée"]),
            ("people", &["peuple", "gens", "monde"]),
            ("night", &["nuit", "soir", "soirée"]),
            ("god", &["dieu"]),
            ("money", &["argent"]),
            ("home", &["maison", "foyer", "domicile"]),
            ("father", &["père"]),
            ("mother", &["mère"]),
            ("friend", &["ami", "copain"]),
            ("world", &["monde"]),
            ("name", &["nom"]),
            ("hand", &["main"]),
            ("job", &["travail", "emploi", "boulot", "poste", "job"]),
            ("girl", &["fille"]),
            ("year", &["année", "an"]),
        ],
    );
    assert_firsts(
        "fr>en",
        &e,
        Lang::Fr,
        Lang::En,
        &[
            ("temps", &["time", "weather"]),
            ("homme", &["man"]),
            ("chose", &["thing"]),
            ("vie", &["life"]),
            ("jour", &["day"]),
            ("nuit", &["night"]),
            ("dieu", &["god"]),
            ("argent", &["money", "silver"]),
            ("maison", &["house", "home"]),
            ("père", &["father"]),
            ("mère", &["mother"]),
            ("ami", &["friend"]),
            ("monde", &["world", "people"]),
            ("nom", &["name"]),
            ("main", &["hand"]),
            ("travail", &["work", "job"]),
            ("fille", &["girl", "daughter"]),
            ("garçon", &["boy"]),
            ("année", &["year"]),
            ("eau", &["water"]),
        ],
    );
    assert_firsts(
        "en>ar",
        &e,
        Lang::En,
        Lang::Ar,
        &[
            ("time", &["زمن", "وقت"]),
            ("man", &["رجل"]),
            ("thing", &["شيء"]),
            ("way", &["طريق", "طريقة"]),
            ("life", &["حياة"]),
            ("day", &["يوم"]),
            ("night", &["ليل", "ليلة"]),
            ("money", &["مال", "نقود"]),
            ("home", &["بيت", "منزل"]),
            ("father", &["أب", "والد"]),
            ("mother", &["أم", "والدة"]),
            ("friend", &["صديق"]),
            ("world", &["عالم"]),
            ("name", &["اسم"]),
            ("hand", &["يد"]),
            ("girl", &["فتاة", "بنت"]),
            ("year", &["سنة", "عام"]),
            ("water", &["ماء"]),
            ("door", &["باب"]),
            ("head", &["رأس"]),
        ],
    );
    assert_firsts(
        "ar>en",
        &e,
        Lang::Ar,
        Lang::En,
        &[
            ("سنة", &["year"]),
            ("رجل", &["man"]),
            ("شيء", &["thing", "something"]),
            ("طريقة", &["way", "method"]),
            ("حياة", &["life"]),
            ("يوم", &["day"]),
            ("ناس", &["people"]),
            ("ليل", &["night"]),
            ("الله", &["god", "allah"]),
            ("مال", &["money", "property", "wealth"]),
            ("بيت", &["house", "home"]),
            ("أب", &["father"]),
            ("أم", &["mother"]),
            ("صديق", &["friend"]),
            ("عالم", &["world"]),
            ("يد", &["hand"]),
            ("عمل", &["work", "job"]),
            ("ماء", &["water"]),
            ("باب", &["door"]),
            ("رأس", &["head"]),
        ],
    );
}

#[test]
fn most_of_the_commonest_words_have_a_translation() {
    let Some(e) = engine() else { return };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    for (lang, to, min) in [(Lang::En, Lang::Fr, 0.88), (Lang::Fr, Lang::En, 0.88), (Lang::En, Lang::Ar, 0.85), (Lang::Ar, Lang::En, 0.78)] {
        let Ok(list) = std::fs::read_to_string(dir.join(format!("{}.txt", lang.code()))) else { return };
        let words: Vec<&str> = list.lines().filter_map(|l| l.split_whitespace().next()).filter(|w| w.chars().count() >= 3 && w.chars().all(char::is_alphabetic)).take(1000).collect();
        let found = words.iter().filter(|w| e.translate_word(w, &[to], Some(lang)).is_some_and(|t| t.from == lang)).count();
        let share = found as f64 / words.len() as f64;
        assert!(share >= min, "{}>{}: only {:.1}% of the commonest {} words have a translation (wanted {:.0}%)", lang.code(), to.code(), share * 100.0, words.len(), min * 100.0);
    }
}

#[test]
fn rude_only_words_are_left_out_but_ordinary_words_with_a_rude_sense_stay() {
    let Some(e) = engine() else { return };
    // the rule is in docs/DATA.md: a word whose senses are (mostly) vulgar or offensive is dropped; a word that is also an ordinary
    // word keeps its ordinary senses (`dog` has a derogatory sense, `cock` is a bird and a part of a gun)
    for w in ["fuck", "slut"] {
        assert!(e.translate_word(w, &[Lang::Fr], Some(Lang::En)).is_none(), "{w} should not be in the dictionary");
    }
    assert_top(&e, "dog", Lang::En, Lang::Fr, 1, &["chien"]);
    let got = tr(&e, "dog", Lang::Fr, Some(Lang::En));
    assert!(!got.iter().any(|g| g.contains("pute") || g.contains("salope")), "{got:?}");
}
