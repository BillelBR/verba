//! Measures how well the local engine finishes words, per language.
//!
//!   cargo run --release -p verba-local --example accuracy -- [--top 3000]
//!
//! Method: take the most frequent N words (4+ letters, plain letters only) of each downloaded list,
//! type each one letter by letter into the engine and ask for 3 suggestions, like the popup does.
//! The target set is chosen here, with its own fixed filter, so the numbers stay comparable while the
//! engine's own cleaning changes. All three languages are loaded together, as in the real program:
//! an English word can be outranked by a French one, and that counts as a miss.
//!
//! It also reports "sloppy typing" (Arabic without hamza / taa marbuta / alef maqsura, French without
//! accents) and the time one `suggest` call takes.

use std::path::Path;
use std::time::Instant;

use verba_core::{Engine, Lang, is_arabic, strip_arabic_marks};
use verba_local::LocalEngine;

const FRENCH_LETTERS: &str = "àâäçéèêëîïôöùûüÿœæ";

/// Pieces of English contractions that the lists hold as if they were words (`don` of `don't`...). They are not
/// targets: nobody wants `didn` suggested.
const NOT_WORDS: &[&str] = &[
    "didn", "doesn", "isn", "aren", "wasn", "weren", "haven", "hasn", "hadn", "couldn", "wouldn", "shouldn", "mustn", "needn", "shan",
    "mightn", "puisqu", "quelqu", "jusqu", "lorsqu", "aujourd",
];

/// The most frequent `n` plain words (4+ letters, one script, no stretched noise) of a list, as written in it.
fn targets(dir: &Path, lang: Lang, n: usize) -> Vec<String> {
    let text = std::fs::read_to_string(dir.join(format!("{}.txt", lang.code()))).unwrap_or_default();
    // Subtitle lists hold words of the other language (`the` in the French list). A word that is at least 15 times more
    // frequent in the other Latin list is not a target: it belongs there.
    let other = match lang {
        Lang::En => Some(Lang::Fr),
        Lang::Fr => Some(Lang::En),
        Lang::Ar => None,
    };
    let share = |l: Lang| -> std::collections::HashMap<String, f64> {
        let t = std::fs::read_to_string(dir.join(format!("{}.txt", l.code()))).unwrap_or_default();
        let rows: Vec<(String, f64)> = t
            .lines()
            .filter_map(|ln| {
                let mut p = ln.split_whitespace();
                Some((p.next()?.to_lowercase(), p.next()?.parse::<f64>().ok()?))
            })
            .collect();
        let total: f64 = rows.iter().map(|r| r.1).sum();
        let mut m = std::collections::HashMap::new();
        for (w, c) in rows {
            *m.entry(w).or_insert(0.0) += c / total;
        }
        m
    };
    let mine = share(lang);
    let theirs = other.map(share);
    let mut out = Vec::new();
    for line in text.lines() {
        let Some(raw) = line.split_whitespace().next() else { continue };
        if let Some(t) = &theirs {
            let w = raw.to_lowercase();
            if t.get(&w).is_some_and(|o| *o >= 15.0 * mine.get(&w).copied().unwrap_or(0.0)) {
                continue;
            }
        }
        let word = if lang == Lang::Ar { strip_arabic_marks(raw) } else { raw.to_lowercase() };
        let chars: Vec<char> = word.chars().collect();
        if chars.len() < 4 {
            continue;
        }
        let pure = if lang == Lang::Ar {
            chars.iter().all(|c| is_arabic(*c) && c.is_alphabetic())
        } else {
            chars.iter().all(|c| c.is_ascii_lowercase() || FRENCH_LETTERS.contains(*c))
        };
        if !pure || NOT_WORDS.contains(&word.as_str()) || chars.windows(3).any(|w| w[0] == w[1] && w[1] == w[2]) {
            continue;
        }
        out.push(word);
        if out.len() >= n {
            break;
        }
    }
    out
}

fn prefix(word: &str, p: usize) -> String {
    word.chars().take(p).collect()
}

fn hits(engine: &LocalEngine, typed: &str, target: &str, k: usize) -> Option<usize> {
    engine.suggest(typed, 3).iter().position(|s| s.text.to_lowercase() == target).filter(|i| *i < k)
}

/// What a typist who skips the hamza, the dotted taa marbuta and the dotted yaa types.
fn sloppy_arabic(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
            'ة' => 'ه',
            'ى' => 'ي',
            c => c,
        })
        .collect()
}

fn sloppy_french(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'ç' => 'c',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ÿ' => 'y',
            c => c,
        })
        .collect()
}

fn pct(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args.iter().position(|a| a == "--top").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(3000);
    let show_misses: usize = args.iter().position(|a| a == "--misses").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(0);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let engine = match LocalEngine::load_dir(&dir) {
        Ok(e) if e.languages().len() == 3 => e,
        _ => {
            eprintln!("word lists missing: run scripts/fetch-data.ps1");
            return;
        }
    };

    println!("## Lists after cleaning\n");
    println!("| Language | lines in the file | words kept | dropped |");
    println!("|---|---|---|---|");
    for lang in [Lang::En, Lang::Fr, Lang::Ar] {
        let lines = std::fs::read_to_string(dir.join(format!("{}.txt", lang.code()))).map(|t| t.lines().count()).unwrap_or(0);
        let kept = engine.word_count(lang);
        println!("| {} | {} | {} | {} ({:.1}%) |", lang.code(), lines, kept, lines.saturating_sub(kept), pct(lines.saturating_sub(kept), lines));
    }
    println!();

    println!("## Completion accuracy (top {n} words of each list, 4+ letters; 3 suggestions)\n");
    println!("| Language | words | top-1 @2 | @3 | @4 | top-3 @2 | @3 | @4 | letters saved / word | saved % |");
    println!("|---|---|---|---|---|---|---|---|---|---|");
    let mut all_prefixes: Vec<(Lang, String, String)> = Vec::new();
    for lang in [Lang::En, Lang::Fr, Lang::Ar] {
        let list = targets(&dir, lang, n);
        let mut top1 = [0usize; 3];
        let mut top3 = [0usize; 3];
        let mut total = [0usize; 3];
        let (mut saved_sum, mut saved_pct_sum) = (0usize, 0.0f64);
        for w in &list {
            let len = w.chars().count();
            for (i, p) in [2usize, 3, 4].into_iter().enumerate() {
                if len > p {
                    total[i] += 1;
                    match hits(&engine, &prefix(w, p), w, 3) {
                        Some(0) => {
                            top1[i] += 1;
                            top3[i] += 1;
                        }
                        Some(_) => top3[i] += 1,
                        None => {}
                    }
                }
            }
            // the first prefix (2 letters or more) from which the word is among the 3
            let mut saved = 0;
            for p in 2..len {
                if hits(&engine, &prefix(w, p), w, 3).is_some() {
                    saved = len - p;
                    break;
                }
            }
            saved_sum += saved;
            saved_pct_sum += saved as f64 / len as f64;
            for p in 2..len {
                all_prefixes.push((lang, prefix(w, p), w.clone()));
            }
        }
        println!(
            "| {} | {} | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.2} | {:.1}% |",
            lang.code(),
            list.len(),
            pct(top1[0], total[0]),
            pct(top1[1], total[1]),
            pct(top1[2], total[2]),
            pct(top3[0], total[0]),
            pct(top3[1], total[1]),
            pct(top3[2], total[2]),
            saved_sum as f64 / list.len().max(1) as f64,
            100.0 * saved_pct_sum / list.len().max(1) as f64,
        );
    }

    if show_misses > 0 {
        for lang in [Lang::En, Lang::Fr, Lang::Ar] {
            println!("\nmisses at 3 letters, {}:", lang.code());
            let mut shown = 0;
            for w in targets(&dir, lang, n) {
                if w.chars().count() > 3 && hits(&engine, &prefix(&w, 3), &w, 3).is_none() {
                    let got: Vec<String> = engine.suggest(&prefix(&w, 3), 3).into_iter().map(|s| s.text).collect();
                    println!("  {} -> typed {} -> {:?}", w, prefix(&w, 3), got);
                    shown += 1;
                    if shown == show_misses {
                        break;
                    }
                }
            }
        }
    }

    println!("\n## Sloppy typing (only words where the typed prefix really differs from the spelling)\n");
    println!("| Case | words | top-3 @3 | top-3 @4 |");
    println!("|---|---|---|---|");
    for (name, lang, sloppy) in [
        ("Arabic without hamza / taa marbuta / yaa", Lang::Ar, sloppy_arabic as fn(&str) -> String),
        ("French without accents", Lang::Fr, sloppy_french as fn(&str) -> String),
    ] {
        let list = targets(&dir, lang, n);
        let mut c = [(0usize, 0usize); 2];
        for w in &list {
            for (i, p) in [3usize, 4].into_iter().enumerate() {
                if w.chars().count() <= p {
                    continue;
                }
                let real = prefix(w, p);
                let typed = sloppy(&real);
                if typed == real {
                    continue;
                }
                c[i].0 += 1;
                if hits(&engine, &typed, w, 3).is_some() {
                    c[i].1 += 1;
                }
            }
        }
        println!("| {} | {} | {:.1}% | {:.1}% |", name, c[0].0, pct(c[0].1, c[0].0), pct(c[1].1, c[1].0));
    }

    // speed: every prefix of every target word, as if someone typed them all
    let mut times: Vec<u128> = Vec::with_capacity(all_prefixes.len());
    for (_, typed, _) in &all_prefixes {
        let t = Instant::now();
        let r = engine.suggest(typed, 3);
        std::hint::black_box(&r);
        times.push(t.elapsed().as_nanos());
    }
    times.sort_unstable();
    let at = |q: f64| times[((times.len() as f64 - 1.0) * q) as usize] as f64 / 1000.0;
    println!("\n## Speed of one suggest() call ({} calls, release build)\n", times.len());
    println!("| p50 | p95 | p99 | max |");
    println!("|---|---|---|---|");
    println!("| {:.0} µs | {:.0} µs | {:.0} µs | {:.0} µs |", at(0.5), at(0.95), at(0.99), *times.last().unwrap() as f64 / 1000.0);
}
