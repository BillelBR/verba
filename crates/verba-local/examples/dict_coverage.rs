//! How much of REAL Arabic text the dictionary translates: for every Arabic word of a text, does the lookup (particles and pronoun endings
//! stripped, as the program does it) find a translation? Reads only.
//!
//!   cargo run --release -p verba-local --example dict_coverage -- --dict data/words/dict.txt --lines data/eval/src/wiki_ar.txt [--tsv file] [--show 40]
//!
//! `--tsv` is a Tatoeba or Leipzig file (`id<TAB>language<TAB>sentence` or `id<TAB>sentence`), `--lines` one sentence per line. The words are
//! the Arabic words of 3 letters or more. Reported: the share of the words of the text (every occurrence) and of its distinct words that have
//! a translation, by how common the word is in the text, and the commonest words that have none (to see what kind of word the dictionary lacks).
//! The dictionary is read with the hand-written corrections and the written words (`Dictionary::parse_with_fixes`), as the program loads it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use verba_core::{Lang, is_arabic, is_arabic_mark, strip_arabic_marks};
use verba_local::Dictionary;

/// Counts the bytes that are alive, to report the memory of the dictionary.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        LIVE.fetch_add(n, Ordering::Relaxed);
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let dict_path = PathBuf::from(arg("--dict").unwrap_or_else(|| "data/words/dict.txt".to_string()));
    let show: usize = arg("--show").and_then(|v| v.parse().ok()).unwrap_or(40);

    let mut sentences: Vec<String> = Vec::new();
    if let Some(p) = arg("--tsv") {
        let raw = std::fs::read_to_string(&p).expect("the --tsv file");
        for line in raw.lines() {
            let mut f = line.splitn(3, '\t');
            if let (Some(_), Some(second)) = (f.next(), f.next()) {
                sentences.push(f.next().unwrap_or(second).to_string());
            }
        }
    }
    if let Some(p) = arg("--lines") {
        let raw = std::fs::read_to_string(&p).expect("the --lines file");
        sentences.extend(raw.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string));
    }
    assert!(!sentences.is_empty(), "give --tsv or --lines");

    let text = std::fs::read_to_string(&dict_path).unwrap_or_else(|e| panic!("{}: {e}", dict_path.display()));
    let before = LIVE.load(Ordering::Relaxed);
    let t0 = Instant::now();
    let dict = Dictionary::parse_with_fixes(&text);
    let (load, memory) = (t0.elapsed(), LIVE.load(Ordering::Relaxed).saturating_sub(before));

    // the words of the text and how often each occurs
    let mut counts: HashMap<String, usize> = HashMap::new();
    for s in &sentences {
        let mut word = String::new();
        for c in s.chars().chain(std::iter::once(' ')) {
            if is_arabic(c) || is_arabic_mark(c) {
                word.push(c);
            } else if !word.is_empty() {
                let plain = strip_arabic_marks(&word);
                if plain.chars().count() >= 3 && plain.chars().all(is_arabic) {
                    *counts.entry(plain).or_insert(0) += 1;
                }
                word.clear();
            }
        }
    }
    // `--dump file`: every covered word with its first translations, to compare two runs
    let mut dump = String::new();
    let (mut tokens, mut covered_tokens, mut covered_types) = (0usize, 0usize, 0usize);
    let mut missing: Vec<(&String, usize)> = Vec::new();
    for (word, n) in &counts {
        tokens += n;
        if let Some(t) = dict.translate(word, &[Lang::En], Some(Lang::Ar)) {
            dump.push_str(&format!("{word}	{n}	{}
", t.candidates.iter().take(3).map(|c| c.text.as_str()).collect::<Vec<_>>().join("; ")));
            covered_tokens += n;
            covered_types += 1;
        } else {
            missing.push((word, *n));
        }
    }
    missing.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    if let Some(path) = arg("--dump") {
        std::fs::write(path, dump).expect("cannot write the dump");
    }
    let pct = |a: usize, b: usize| a as f64 * 100.0 / b.max(1) as f64;
    println!("dictionary {} ({:.1} MB), {} headwords ar>en; read in {:.2} s, {:.0} MB in memory", dict_path.display(), text.len() as f64 / 1048576.0, dict.len(Lang::Ar, Lang::En), load.as_secs_f64(), memory as f64 / 1048576.0);
    println!("text: {} sentences, {} Arabic words (3 letters or more), {} distinct", sentences.len(), tokens, counts.len());
    println!("translated: {:.1} % of the words of the text, {:.1} % of the distinct words", pct(covered_tokens, tokens), pct(covered_types, counts.len()));
    // by how common the word is: the words seen at least 20, 5, 2 times, and once
    for (label, min, max) in [("seen 20 times or more", 20, usize::MAX), ("seen 5 to 19 times", 5, 19), ("seen 2 to 4 times", 2, 4), ("seen once", 1, 1)] {
        let types: Vec<(&String, &usize)> = counts.iter().filter(|(_, n)| **n >= min && **n <= max).collect();
        let covered = types.iter().filter(|(w, _)| dict.translate(w, &[Lang::En], Some(Lang::Ar)).is_some()).count();
        println!("  {label:<24} {:>7} distinct words, {:.1} % translated", types.len(), pct(covered, types.len()));
    }
    println!("the commonest words without a translation:");
    for (w, n) in missing.iter().take(show) {
        println!("  {n:>5}  {w}");
    }
}
