//! Proof that an engine whose user has never used learning answers exactly as the engine did before usage counts existed.
//!
//! The files `golden_synthetic.txt` and `golden_real.txt` hold the answers (one line per question: `typed<TAB>answers separated by |`) written by
//! the engine of commit e80bf34 (Verba 0.4.0), before `usage` was added to `personal.rs`. The test asks the same questions of the engine now
//! and compares byte for byte. Set `VERBA_WRITE_GOLDEN=1` to write the files again (only do that on purpose: it erases the proof).
//!
//! The synthetic lists are built here, so that part always runs; the real lists are used when they were downloaded.

use std::path::Path;

use verba_core::{Engine, Lang};
use verba_local::{LocalEngine, WordList};

/// A small deterministic generator (no dependency).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

/// `n` made-up words from syllables, with Zipf-like counts, as the text of a list.
fn synthetic_list(seed: u64, syllables: &[&str], n: usize) -> (String, Vec<String>) {
    let mut rng = Lcg(seed);
    let mut words: Vec<String> = Vec::new();
    while words.len() < n {
        let parts = 2 + rng.next() % 3;
        let w: String = (0..parts).map(|_| syllables[(rng.next() as usize) % syllables.len()]).collect();
        if !words.contains(&w) {
            words.push(w);
        }
    }
    let text: String = words.iter().enumerate().map(|(i, w)| format!("{w} {}\n", 5_000_000 / (i as u64 + 1) + 3)).collect();
    (text, words)
}

const LATIN: &[&str] = &["ba", "ka", "ri", "to", "me", "su", "lan", "dor", "ve", "pi", "co", "ta", "ren", "mo", "sel", "ni", "gu", "fa", "ho", "wen", "ar", "is", "ex", "un"];
const ARABIC: &[&str] = &["كت", "اب", "مد", "رس", "ة", "من", "في", "لى", "عل", "هذ", "ال", "قا", "لت", "وم", "ين", "بح", "ر", "سم"];

fn synthetic_engine() -> (LocalEngine, Vec<(Lang, Vec<String>)>) {
    let mut e = LocalEngine::new();
    let mut sets = Vec::new();
    for (lang, seed, syl) in [(Lang::En, 11, LATIN), (Lang::Fr, 29, LATIN), (Lang::Ar, 7, ARABIC)] {
        let (text, words) = synthetic_list(seed, syl, 1500);
        e.add(lang, WordList::parse_for(lang, &text, 5000));
        sets.push((lang, words));
    }
    (e, sets)
}

/// The questions: every 2 to 4 letter beginning of the 500 commonest words, as typed alone, after a word, with a capital, and per language.
fn questions(sets: &[(Lang, Vec<String>)]) -> Vec<(Option<Lang>, String)> {
    let mut q = Vec::new();
    for (lang, words) in sets {
        for (i, w) in words.iter().take(250).enumerate() {
            let chars: Vec<char> = w.chars().collect();
            for len in 2..=4.min(chars.len().saturating_sub(1)) {
                let prefix: String = chars[..len].iter().collect();
                q.push((Some(*lang), prefix.clone()));
                if i % 3 == 0 {
                    q.push((None, format!("see {prefix}")));
                }
                if i % 5 == 0 && !lang.is_rtl() {
                    let mut c = prefix.chars();
                    let cap: String = c.next().unwrap().to_uppercase().chain(c).collect();
                    q.push((Some(*lang), format!("Then {cap}")));
                }
            }
        }
    }
    q
}

fn answer(e: &LocalEngine, only: Option<Lang>, typed: &str) -> String {
    let got = match only {
        Some(l) => e.suggest_in(l, typed, 3),
        None => e.suggest(typed, 3),
    };
    got.into_iter().map(|s| format!("{}:{}", s.text, s.lang.code())).collect::<Vec<_>>().join("|")
}

fn render(e: &LocalEngine, qs: &[(Option<Lang>, String)]) -> String {
    qs.iter().map(|(only, typed)| format!("{}\t{}\t{}\n", only.map_or("-", |l| l.code()), typed, answer(e, *only, typed))).collect()
}

fn check(name: &str, now: String) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(name);
    if std::env::var("VERBA_WRITE_GOLDEN").is_ok() {
        std::fs::write(&path, &now).unwrap();
        return;
    }
    let before = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{name} is missing"));
    let before = before.replace("\r\n", "\n");
    let diffs: Vec<String> = before.lines().zip(now.lines()).filter(|(a, b)| a != b).take(10).map(|(a, b)| format!("  was {a:?}\n  now {b:?}")).collect();
    assert!(diffs.is_empty() && before.lines().count() == now.lines().count(), "{name}: the answers changed:\n{}", diffs.join("\n"));
}

#[test]
fn an_engine_that_never_learned_answers_as_before_synthetic_lists() {
    let (e, sets) = synthetic_engine();
    let qs = questions(&sets);
    assert!(qs.len() > 1000);
    check("golden_synthetic.txt", render(&e, &qs));
}

#[test]
fn an_engine_that_never_learned_answers_as_before_real_lists() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let Ok(e) = LocalEngine::load_dir(&dir) else { return };
    if e.languages().len() < 3 {
        return;
    }
    let mut qs: Vec<(Option<Lang>, String)> = Vec::new();
    for lang in [Lang::En, Lang::Fr, Lang::Ar] {
        let mut words: Vec<(&str, u64)> = e.list(lang).unwrap().iter_words().collect();
        words.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        for (i, (w, _)) in words.iter().take(1200).enumerate() {
            if i % 3 != 0 {
                continue;
            }
            let chars: Vec<char> = w.chars().collect();
            for len in 2..=4.min(chars.len().saturating_sub(1)) {
                let prefix: String = chars[..len].iter().collect();
                qs.push((Some(lang), prefix.clone()));
                if i % 6 == 0 {
                    qs.push((None, format!("I would {prefix}")));
                }
            }
        }
    }
    check("golden_real.txt", render(&e, &qs));
}
