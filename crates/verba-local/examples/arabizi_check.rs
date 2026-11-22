//! How well `LocalEngine::arabizi_to_arabic` turns Arabizi into Arabic letters, on pairs written by hand (`docs/eval/arabizi-pairs.tsv`):
//!
//!   cargo run --release -p verba-local --example arabizi_check -- [--dir data/words] [--pairs docs/eval/arabizi-pairs.tsv] [--show]
//!
//! Reported per word: the share that is spelled exactly as in the file (vowel marks and hamza on the alef ignored), the share whose
//! consonants are right (the spelling differs only in long vowels), and how many words were found in the lists against spelled by rules.

use std::path::PathBuf;
use std::time::Instant;

use verba_core::strip_arabic_marks;
use verba_local::{LocalEngine, arabic_skeleton};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn plain(w: &str) -> String {
    strip_arabic_marks(w).replace(['أ', 'إ', 'آ'], "ا").replace('ى', "ي").replace('ة', "ه")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = PathBuf::from(arg(&args, "--dir").unwrap_or_else(|| "data/words".into()));
    let pairs = arg(&args, "--pairs").unwrap_or_else(|| "docs/eval/arabizi-pairs.tsv".into());
    let show = args.iter().any(|a| a == "--show");
    let mut engine = LocalEngine::load_dir(&dir).expect("the folder");
    engine.add_dz_seed();
    let (mut words, mut exact, mut skeleton, mut found, mut found_exact, mut rules, mut rules_exact) = (0, 0, 0, 0, 0, 0, 0);
    let t = Instant::now();
    let mut first = true;
    for line in std::fs::read_to_string(&pairs).expect("the pairs").lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let (latin, want) = line.split_once('\t').expect("a tab");
        let got = engine.arabizi_to_arabic(latin);
        if first {
            println!("index built and first text converted in {:?}", t.elapsed());
            first = false;
        }
        let Some(got) = got else {
            println!("NOT TURNED: {latin}");
            continue;
        };
        let (g, w): (Vec<&str>, Vec<&str>) = (got.text.split_whitespace().collect(), want.split_whitespace().collect());
        if g.len() != w.len() {
            println!("word count differs: {latin} -> {} (want {want})", got.text);
            continue;
        }
        for (i, (a, b)) in g.iter().zip(&w).enumerate() {
            let l = latin.split_whitespace().nth(i).unwrap_or("");
            if !b.chars().any(verba_core::is_arabic) {
                continue; // a word that stays Latin
            }
            words += 1;
            let ok = plain(a) == plain(b);
            let sk = arabic_skeleton(a) == arabic_skeleton(b);
            let is_found = engine.arabizi_to_arabic(l).is_some_and(|r| r.found == r.words);
            exact += usize::from(ok);
            skeleton += usize::from(sk);
            if is_found {
                found += 1;
                found_exact += usize::from(ok);
            } else {
                rules += 1;
                rules_exact += usize::from(ok);
            }
            if show || !ok {
                println!("{} {l:12} -> {a:10} (want {b}){}", if ok { " ok " } else { "MISS" }, if is_found { "" } else { "  [rules]" });
            }
        }
    }
    let pct = |n: usize, d: usize| if d == 0 { 0.0 } else { n as f64 * 100.0 / d as f64 };
    println!("\n{words} words: exact {:.1} %, right consonants {:.1} %", pct(exact, words), pct(skeleton, words));
    println!("found in the lists: {found} words, exact {:.1} %; spelled by rules: {rules} words, exact {:.1} %", pct(found_exact, found), pct(rules_exact, rules));
}
