//! Prints what the bilingual dictionary gives for the words on the command line, and measures its load.
//!
//!   cargo run --release -p verba-local --example dict_probe -- hello chien كتاب
//!   cargo run --release -p verba-local --example dict_probe -- --to ar --from fr maison     # only into Arabic, French keyboard
//!   cargo run --release -p verba-local --example dict_probe -- --bench                       # load time and memory of data/words/dict.txt
//!   cargo run --release -p verba-local --example dict_probe -- --coverage 5000               # share of the commonest words of each list that have a translation
//!   cargo run --release -p verba-local --example dict_probe -- --dir some\folder ...         # another folder than data/words
//!
//! Needs `data/words/dict.txt` (scripts/fetch-dict.ps1).

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use verba_core::Lang;
use verba_local::{Dictionary, LocalEngine};

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
static A: Counting = Counting;

fn lang_of(code: &str) -> Option<Lang> {
    match code {
        "en" => Some(Lang::En),
        "fr" => Some(Lang::Fr),
        "ar" => Some(Lang::Ar),
        _ => None,
    }
}

fn bench(dir: &Path) {
    let path = dir.join("dict.txt");
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!("{}: {:.2} MB", path.display(), bytes as f64 / 1048576.0);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("cannot read {}: {e} (run scripts/fetch-dict.ps1)", path.display());
        std::process::exit(1);
    });
    // the memory of the parsed dictionary: bytes alive after parsing minus before (the text itself is counted out)
    let before = LIVE.load(Ordering::Relaxed);
    let d = Dictionary::parse(&text);
    let after = LIVE.load(Ordering::Relaxed);
    println!("memory of the parsed dictionary: {:.1} MB (the {:.1} MB of text is released after loading)", (after - before) as f64 / 1048576.0, text.len() as f64 / 1048576.0);
    for (a, b) in [("en", "fr"), ("fr", "en"), ("en", "ar"), ("ar", "en")] {
        println!("  {a}>{b}: {} headwords", d.len(lang_of(a).unwrap(), lang_of(b).unwrap()));
    }
    drop(d);
    let mut times = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let d = Dictionary::parse(&text);
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(&d);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("Dictionary::parse of the text in memory: median {:.0} ms (best {:.0}, worst {:.0}, 5 runs)", times[2], times[0], times[4]);
    let mut times = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        let e = LocalEngine::load_dir(dir).expect("load_dir");
        times.push(t.elapsed().as_secs_f64() * 1000.0);
        std::hint::black_box(&e);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("LocalEngine::load_dir (lists, next-word tables and dictionary, with reading the files): median {:.0} ms (best {:.0}, worst {:.0}, 3 runs)", times[1], times[0], times[2]);
    let baseline = {
        let tmp = std::env::temp_dir().join("verba-dict-bench-nodict");
        let _ = std::fs::create_dir_all(&tmp);
        for f in ["en.txt", "fr.txt", "ar.txt", "en.bigrams.txt", "fr.bigrams.txt", "ar.bigrams.txt"] {
            let _ = std::fs::copy(dir.join(f), tmp.join(f));
        }
        let mut ms = Vec::new();
        for _ in 0..3 {
            let t = Instant::now();
            let e = LocalEngine::load_dir(&tmp).expect("load_dir without the dictionary");
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
            std::hint::black_box(&e);
        }
        let _ = std::fs::remove_dir_all(&tmp);
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ms[1]
    };
    println!("LocalEngine::load_dir without dict.txt: median {baseline:.0} ms (3 runs, files cached)");
}

/// For the `n` commonest words of each frequency list: how many have a translation into each of the other languages.
/// "plain" leaves out what is not a word of the list's own language: tokens with a digit, an apostrophe or a hyphen, and one-letter tokens.
fn coverage_report(dir: &Path, n: usize) {
    let text = std::fs::read_to_string(dir.join("dict.txt")).expect("dict.txt");
    let d = Dictionary::parse(&text);
    for from in [Lang::En, Lang::Fr, Lang::Ar] {
        let list = std::fs::read_to_string(dir.join(format!("{}.txt", from.code()))).expect("word list");
        let words: Vec<&str> = list.lines().filter_map(|l| l.split_whitespace().next()).take(n).collect();
        let plain: Vec<&str> = words.iter().copied().filter(|w| w.chars().count() >= 2 && w.chars().all(|c| c.is_alphabetic() || (from == Lang::Ar && c as u32 >= 0x064B && c as u32 <= 0x0652))).collect();
        for to in [Lang::En, Lang::Fr, Lang::Ar] {
            if to == from {
                continue;
            }
            let hint = Some(from);
            let count = |set: &[&str]| set.iter().filter(|w| d.translate(w, &[to], hint).is_some_and(|t| t.from == from)).count();
            let direct = |set: &[&str]| set.iter().filter(|w| d.translate(w, &[to], hint).is_some_and(|t| t.from == from && !t.via_english)).count();
            let (a, b, c, e) = (count(&words), words.len(), count(&plain), plain.len());
            println!(
                "{}>{}: top {n}: {a}/{b} = {:.1}% ({} directly); plain words only: {c}/{e} = {:.1}%",
                from.code(),
                to.code(),
                100.0 * a as f64 / b as f64,
                direct(&words),
                100.0 * c as f64 / e as f64
            );
        }
    }
}

fn main() {
    let mut dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let (mut targets, mut hint, mut words, mut run_bench, mut coverage): (Vec<Lang>, Option<Lang>, Vec<String>, bool, Option<usize>) = (vec![], None, vec![], false, None);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dir" => dir = PathBuf::from(it.next().unwrap_or_default()),
            "--to" => targets = it.next().unwrap_or_default().split(',').filter_map(lang_of).collect(),
            "--from" => hint = it.next().as_deref().and_then(lang_of),
            "--bench" => run_bench = true,
            "--coverage" => coverage = it.next().and_then(|n| n.parse().ok()),
            _ => words.push(a),
        }
    }
    if run_bench {
        bench(&dir);
        if words.is_empty() && coverage.is_none() {
            return;
        }
    }
    if let Some(n) = coverage {
        coverage_report(&dir, n);
        if words.is_empty() {
            return;
        }
    }
    if words.is_empty() {
        eprintln!("usage: dict_probe [--dir folder] [--to en,fr,ar] [--from en|fr] [--bench] word ...");
        std::process::exit(2);
    }
    let text = std::fs::read_to_string(dir.join("dict.txt")).unwrap_or_else(|e| {
        eprintln!("cannot read {}: {e} (run scripts/fetch-dict.ps1)", dir.join("dict.txt").display());
        std::process::exit(1);
    });
    let d = Dictionary::parse(&text);
    if targets.is_empty() {
        targets = vec![Lang::En, Lang::Fr, Lang::Ar];
    }
    for w in &words {
        let mut any = false;
        for &to in &targets {
            if let Some(t) = d.translate(w, &[to], hint) {
                any = true;
                let list: Vec<&str> = t.candidates.iter().map(|c| c.text.as_str()).collect();
                println!("{w}  [{}>{}{}]  {}", t.from.code(), t.to.code(), if t.via_english { " via English" } else { "" }, list.join("; "));
            }
        }
        if !any {
            println!("{w}  --");
        }
    }
}
