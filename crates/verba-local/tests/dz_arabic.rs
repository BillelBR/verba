//! Algerian Darija written in Arabic letters (the same `Lang::Dz` switch as the Latin script): suggestions, the merge with the Arabic
//! list, and the guarantee that the spelling correction never touches or writes a Darija word.
//!
//! The tests that need only the embedded seed run everywhere. The ones that also need the Arabic list skip when it was not downloaded
//! (`scripts/fetch-data.ps1`), like the other test files.

use std::path::Path;

use verba_core::{Engine, Lang, Suggestion};
use verba_local::{DZ_AR_SEED, DzList, LocalEngine, WordList, dz_ar_key};

fn texts(v: Vec<Suggestion>) -> Vec<String> {
    v.into_iter().map(|s| s.text).collect()
}

/// A small Arabic (MSA) list, a few Darija-looking words in it, and the Darija seed (switched on).
fn small() -> LocalEngine {
    let mut e = LocalEngine::new();
    e.add(
        Lang::Ar,
        WordList::parse_for(Lang::Ar, "في 90000\nعلاقة 3000\nعلاقات 2000\nالى 80000\nهذا 70000\nذهب 6000\nكتاب 5000\nكيف 4000\nماذا 6000\nمدرسة 2500\nقال 9000\n", 100),
    );
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

fn plain() -> Option<LocalEngine> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let e = LocalEngine::load_dir(&dir).ok()?;
    (e.languages().len() == 3).then_some(e)
}

#[test]
fn the_arabic_seed_is_embedded_and_big_enough() {
    let seed = DzList::seed_ar();
    assert!(seed.len() >= 1500, "{} words", seed.len());
    for w in ["واش", "كيفاش", "راني", "بزاف", "علاش", "كاين", "تاع", "دروك", "برك", "خويا", "ماشي", "مليح", "ندير", "نروح", "ماعرفتش", "طوموبيل", "ماكاينش", "عندي", "معايا", "فليكسي"] {
        assert!(seed.contains(w), "{w} is not in the Arabic seed");
    }
    assert!(DZ_AR_SEED.starts_with('#') && DZ_AR_SEED.contains("PolyForm Noncommercial"));
}

#[test]
fn the_arabic_seed_is_clean() {
    let mut seen = std::collections::HashSet::new();
    let mut words = 0;
    for line in DZ_AR_SEED.lines().filter(|l| !l.starts_with('#') && !l.trim().is_empty()) {
        let toks: Vec<&str> = line.split_whitespace().collect();
        assert!(toks.len() % 2 == 0, "a word without a weight, or a space in a word: {line}");
        for pair in toks.chunks(2) {
            assert!(pair[1].parse::<u64>().is_ok_and(|n| n > 0), "bad weight for {:?}", pair[0]);
            assert!(pair[0].chars().all(|c| matches!(c as u32, 0x0621..=0x063A | 0x0641..=0x064A) || "ڨڤپچگ".contains(c)), "not Arabic letters: {:?}", pair[0]);
            assert!(seen.insert(pair[0]), "listed twice: {:?}", pair[0]);
            words += 1;
        }
    }
    assert_eq!(DzList::seed_ar().len(), words, "some words were dropped when the list was read");
}

#[test]
fn darija_in_arabic_letters_is_off_until_switched_on() {
    let mut e = LocalEngine::new();
    e.add(Lang::Ar, WordList::parse_for(Lang::Ar, "في 100\n", 10));
    e.add_dz_seed();
    assert!(e.languages().contains(&Lang::Dz) && !e.is_enabled(Lang::Dz));
    assert!(e.suggest("واش", 3).is_empty() && e.suggest("كيف", 3).is_empty() && e.suggest_in(Lang::Dz, "كيف", 3).is_empty());
    assert!(!e.dz_knows("كيفاش"));
    e.set_enabled(Lang::Dz, true);
    assert!(texts(e.suggest("كيف", 3)).contains(&"كيفاش".to_string()));
    assert!(e.dz_knows("كيفاش"));
    e.set_enabled(Lang::Dz, false);
    assert!(e.suggest("كيف", 3).is_empty());
}

#[test]
fn prefixes_find_darija_words_written_in_arabic_letters() {
    let e = small();
    let dz = |t: &str| texts(e.suggest_in(Lang::Dz, t, 3));
    assert!(dz("وا").contains(&"واش".to_string()), "{:?}", dz("وا"));
    assert!(dz("كيف").contains(&"كيفاش".to_string()));
    assert!(dz("علا").contains(&"علاش".to_string()), "{:?}", dz("علا"));
    assert!(dz("بز").contains(&"بزاف".to_string()), "{:?}", dz("بز"));
    assert!(dz("كاي").contains(&"كاين".to_string()));
    assert!(dz("خوي").contains(&"خويا".to_string()));
    assert!(dz("دري").iter().any(|w| w.starts_with("دري")) || dz("دور").iter().any(|w| w.starts_with("دور")));
    assert!(dz("طومو").contains(&"طوموبيل".to_string()));
    assert!(dz("ماكا").iter().any(|w| w.starts_with("ماكا")));
    // short vowels typed
    assert!(texts(e.suggest_in(Lang::Dz, "كَيْف", 3)).contains(&"كيفاش".to_string()));
    // one letter is not enough
    assert!(dz("و").is_empty());
}

#[test]
fn spelling_variants_find_the_same_word() {
    let e = small();
    let dz = |t: &str| texts(e.suggest_in(Lang::Dz, t, 3));
    // the gaf of the Algerian keyboard, the hamza and the dots: the same word
    assert_eq!(dar("ڨاع"), dar("قاع"));
    // typed with the letter the list does not hold, the listed spelling is offered
    assert!(dz("قا").contains(&"ڨاع".to_string()) || dz("قا").contains(&"قاع".to_string()), "{:?}", dz("قا"));
    // a complete word, or another spelling of it, is not offered back
    for done in ["واش", "كيفاش", "بزاف", "علاش"] {
        assert!(dz(done).iter().all(|w| dz_ar_key(w) != dz_ar_key(done)), "{done} -> {:?}", dz(done));
    }
    // vowel letters left out: `بزف` finds `بزاف`, `كيفش` finds `كيفاش`
    assert!(dz("بزف").contains(&"بزاف".to_string()) || dz("بز").contains(&"بزاف".to_string()));
    assert!(dz("كفا").iter().any(|w| w == "كيفاش") || dz("كيفا").contains(&"كيفاش".to_string()));
}

fn dar(w: &str) -> String {
    dz_ar_key(w)
}

#[test]
fn an_attached_article_or_particle_is_kept() {
    let e = small();
    let dz = |t: &str| texts(e.suggest_in(Lang::Dz, t, 3));
    // `و` + `كيفاش`, `ب` + a listed word
    assert!(dz("وكيفا").contains(&"وكيفاش".to_string()), "{:?}", dz("وكيفا"));
}

#[test]
fn darija_is_an_extra_pool_next_to_the_arabic_list() {
    let e = small();
    // where the Arabic list has nothing, Darija answers; the suggestions are tagged Arabic so that the popup is laid out right to left
    let got = e.suggest("واش", 3);
    assert!(got.is_empty() || got.iter().all(|s| s.lang == Lang::Ar));
    let got = e.suggest("وا", 3);
    assert_eq!(texts(got.clone())[0], "واش");
    assert!(got.iter().all(|s| s.lang == Lang::Ar));
    // an Arabic keyboard gets it; a French or English keyboard does not (those write Latin letters)
    assert!(!e.suggest_in(Lang::Ar, "وا", 3).is_empty());
    assert!(e.suggest_in(Lang::Fr, "وا", 3).is_empty() && e.suggest_in(Lang::En, "وا", 3).is_empty());
    // where the Arabic list knows the prefix well, it wins: Darija never fills the free places
    for typed in ["علا", "كتا", "هذ", "قا", "مدر"] {
        let sug = e.suggest(typed, 3);
        let dz_words = ["علاش", "كتاب"];
        assert!(!sug.iter().any(|s| s.text == "علاش") || typed != "علا", "{typed:?} -> {sug:?}");
        let _ = dz_words;
    }
    assert_eq!(texts(e.suggest("علا", 1)), ["علاقة"]);
}

#[test]
fn the_words_before_tell_darija_from_modern_standard_arabic() {
    let e = small();
    // `علا`: the Arabic list says `علاقة`, Darija says `علاش`
    assert_eq!(texts(e.suggest("ذهب الى علا", 1)), ["علاقة"]);
    assert_eq!(texts(e.suggest("قال هذا علا", 1)), ["علاقة"]);
    assert_eq!(texts(e.suggest("واش راني علا", 1)), ["علاش"], "{:?}", e.suggest("واش راني علا", 3));
    assert_eq!(texts(e.suggest("كيفاش راك علا", 1)), ["علاش"]);
    // one Darija word alone as the nearest word is enough, a Darija word far before MSA words is not
    assert_eq!(texts(e.suggest("ياخي علا", 1)), ["علاش"], "{:?}", e.suggest("ياخي علا", 3));
    assert_eq!(texts(e.suggest("واش ذهب الى علا", 1)), ["علاقة"]);
}

#[test]
fn a_darija_word_is_never_a_typo_and_never_a_fix() {
    let Some(e) = real() else { return };
    let words = [
        "واش", "كيفاش", "راني", "بزاف", "علاش", "كاين", "تاع", "دروك", "برك", "خويا", "ماشي", "مليح", "ندير", "نروح", "طوموبيل", "فليكسي", "غدوة", "ڨاع",
        // other spellings
        "كيفاش", "كيفاس", "واشي", "بزااف", "علاس",
        // with a negation, a verb prefix, a pronoun, a particle
        "ماعرفتش", "مافهمتش", "ماكاينش", "مانقدرش", "ماعندوش", "ماشفتهاش", "نشوفك", "وكيفاش", "بالطوموبيل", "كيفاشك",
    ];
    for w in words {
        for text in [format!("{w} "), format!("واش راني {w}، "), format!("ذهبت الى {w}. ")] {
            for only in [None, Some(Lang::Ar), Some(Lang::Dz)] {
                assert!(e.correct(only, &text, 3).is_none(), "{text:?} ({only:?}) was fixed: {:?}", e.correct(only, &text, 3));
            }
        }
    }
    // a fix is never a Darija-only word (a word of the Darija list that the Arabic list does not hold)
    let seed = DzList::seed_ar();
    let ar = e.list(Lang::Ar).unwrap();
    let only_darija: std::collections::HashSet<String> = seed.iter_words().map(|(w, _)| w.to_string()).filter(|w| !ar.contains_word(w)).collect();
    let typos = ["مرجبا", "شكرا", "لمذا", "مدرسه", "الكتتاب", "ذهبت الى اليم", "اعرف هذا الرجلل", "كيف الححال", "هل انت بخري", "صباح الخري", "تعال الى مععك", "هذا الطععام"];
    let mut fixes = 0;
    for t in typos {
        if let Some(f) = e.correct(None, &format!("{t} "), 3) {
            fixes += 1;
            assert!(f.candidates.iter().all(|c| !only_darija.contains(&c.text)), "{t:?} -> {:?}", f.candidates);
        }
    }
    assert!(fixes >= 6, "only {fixes} of the typos got a fix");
}

#[test]
fn modern_standard_arabic_typos_are_still_fixed_with_darija_on() {
    let (Some(on), Some(off)) = (real(), plain()) else { return };
    let cases = [
        ("قال مرجبا ", "مرحبا"),
        ("لا اعرف لمذا ", "لماذا"),
        ("هل انت بخري ", "بخير"),
        ("ماذا تفعلل ", "تفعل"),
        ("نحن نحب الجميغ ", "الجميع"),
        ("تعال الى مععك ", "معك"),
        ("السلام علكيم ", "عليكم"),
        ("قال صباخ ", "صباح"),
        ("هذا صحييح ", "صحيح"),
        ("كيف الححال ", "الحال"),
        ("اذهب الى الببيت ", "البيت"),
        ("هذا الرجلل ", "الرجل"),
        ("نحن في المردسة ", "المدرسة"),
        ("هذا الكتتاب ", "الكتاب"),
        ("كل النااس ", "الناس"),
        ("مااذا ", "ماذا"),
    ];
    let fold = |s: &str| s.replace(['أ', 'إ', 'آ'], "ا").replace('ة', "ه").replace('ى', "ي");
    let mut lost = Vec::new();
    for (typed, want) in cases {
        let got = |e: &LocalEngine| -> Vec<String> { e.correct(Some(Lang::Ar), typed, 3).map(|f| f.candidates.into_iter().map(|c| fold(&c.text)).collect()).unwrap_or_default() };
        let (g_on, g_off) = (got(&on), got(&off));
        assert!(g_off.contains(&fold(want)), "without Dz: {typed:?} -> {g_off:?}");
        if !g_on.contains(&fold(want)) {
            lost.push(format!("{typed:?} -> wanted {want:?}, got {g_on:?}"));
        }
    }
    assert!(lost.is_empty(), "typos no longer fixed with Dz on:\n{}", lost.join("\n"));
}

#[test]
fn with_darija_off_nothing_changes_in_arabic() {
    let (Some(mut e), Some(off)) = (real(), plain()) else { return };
    e.set_enabled(Lang::Dz, false);
    let mut words: Vec<(String, u64)> = off.list(Lang::Ar).unwrap().iter_words().map(|(w, c)| (w.to_string(), c)).collect();
    words.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let mut n = 0;
    for (w, _) in words.iter().filter(|(w, _)| w.chars().count() >= 5).take(1500) {
        let cs: Vec<char> = w.chars().collect();
        let prefix: String = cs.iter().take(3).collect();
        assert_eq!(e.suggest(&prefix, 3), off.suggest(&prefix, 3), "{w}");
        assert_eq!(e.suggest_in(Lang::Ar, &prefix, 3), off.suggest_in(Lang::Ar, &prefix, 3), "{w}");
        let typo: String = cs.iter().take(cs.len() - 1).collect::<String>() + "ي" + "ي";
        assert_eq!(e.correct(None, &format!("{typo} "), 3), off.correct(None, &format!("{typo} "), 3), "{typo}");
        n += 1;
    }
    assert!(n > 500);
    // Darija words get what they got before: nothing special
    for t in ["وا", "كيف", "بزا", "علا"] {
        assert_eq!(e.suggest(t, 3), off.suggest(t, 3), "{t}");
    }
    for w in ["واش ", "كيفاش ", "ماعرفتش ", "بزاف "] {
        assert_eq!(e.correct(None, w, 3), off.correct(None, w, 3), "{w}");
    }
}

/// How often a Darija word shows up while typing ordinary Modern Standard Arabic words.
#[test]
fn ordinary_modern_standard_arabic_is_not_hijacked() {
    let (Some(on), Some(off)) = (real(), plain()) else { return };
    let mut words: Vec<(String, u64)> = off.list(Lang::Ar).unwrap().iter_words().map(|(w, c)| (w.to_string(), c)).collect();
    words.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let words: Vec<String> = words.into_iter().map(|x| x.0).filter(|w| w.chars().count() >= 4).take(3000).collect();
    let (mut shown, mut first, mut total) = (0, 0, 0);
    for w in &words {
        for n in [2usize, 3, 4] {
            if w.chars().count() <= n {
                continue;
            }
            let typed: String = w.chars().take(n).collect();
            total += 1;
            let a = on.suggest_in(Lang::Ar, &typed, 3);
            let b = off.suggest_in(Lang::Ar, &typed, 3);
            if a.iter().any(|s| !b.iter().any(|o| o.text == s.text)) {
                shown += 1;
                first += usize::from(a.first().map(|s| &s.text) != b.first().map(|s| &s.text));
            }
        }
    }
    let rate = 100.0 * shown as f64 / total as f64;
    println!("a Darija suggestion enters the top 3 for {shown} of {total} prefixes of ordinary words ({rate:.2}%), first place {first}");
    assert!(rate < 2.0, "Darija shows up in {rate:.2}% of the prefixes of ordinary words");
}

/// The seed's own words, so optimistic: a regression guard, not a measurement of real accuracy (see docs/ACCURACY.md).
#[test]
fn seed_words_are_found_from_their_first_letters() {
    let Some(e) = real() else { return };
    let words: Vec<String> = DzList::seed_ar().iter_words().map(|(w, _)| w.to_string()).filter(|w| w.chars().count() >= 5).collect();
    let (mut hit, mut total) = (0, 0);
    for w in &words {
        total += 1;
        let typed = format!("واش راني {}", w.chars().take(4).collect::<String>());
        if e.suggest(&typed, 3).iter().any(|s| dz_ar_key(&s.text) == dz_ar_key(w)) {
            hit += 1;
        }
    }
    let rate = 100.0 * hit as f64 / total as f64;
    println!("seed words found in the top 3 after 4 letters, in a Darija sentence: {rate:.1}%");
    assert!(rate > 70.0, "{rate:.1}%");
}

#[test]
fn a_downloaded_list_is_merged_with_the_arabic_seed() {
    let dir = std::env::temp_dir().join(format!("verba-dzar-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("dz-ar.txt"), "زعبولة 40\nواش 10\n").unwrap();
    let mut e = LocalEngine::new();
    e.load_dz(&dir).unwrap();
    e.set_enabled(Lang::Dz, true);
    assert_eq!(texts(e.suggest_in(Lang::Dz, "زعب", 1)), ["زعبولة"]);
    assert!(e.dz_knows("زعبولة") && e.dz_knows("واش"));
    let _ = std::fs::remove_dir_all(&dir);
}
