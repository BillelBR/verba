//! Builds the optional list of real Algerian words in Arabic letters (`data/words/dz-ar.txt`) from the Mendeley "Algerian Dialect"
//! YouTube comments (Benmounah and Boulesnane, CC BY 4.0). Normally run by `scripts/fetch-dz-ar.ps1`, which downloads the file first.
//!
//!   cargo run --release -p verba-local --example build_dz_ar -- --csv data\dz\ar-youtube\comments.csv --out data\words\dz-ar.txt
//!
//! What it keeps: every word of the comments (Arabic letters only, short vowels dropped) that the Modern Standard Arabic list
//! (`data/words/ar.txt`, 50 000 words) does not have and that occurs at least `--min` times (default 4): the dialect words, the
//! spellings and the names of the comments. The comments themselves are not stored, only `word count` lines.

use std::collections::HashMap;
use std::path::Path;

use verba_core::{Lang, strip_arabic_marks};
use verba_local::WordList;

/// The text column of the CSV (quoted fields, doubled quotes, line breaks inside quotes).
pub fn comments(csv: &str) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    let (mut field, mut first, mut in_quotes, mut at_start) = (String::new(), true, false, true);
    let mut chars = csv.chars().peekable();
    let mut first_row = true;
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    if first {
                        field.push('"');
                    }
                } else {
                    in_quotes = false;
                }
            } else if first {
                field.push(c);
            }
        } else if c == '"' && at_start {
            in_quotes = true;
            at_start = false;
        } else if c == ',' {
            first = false;
            at_start = true;
        } else if c == '\n' {
            if !first_row {
                rows.push(std::mem::take(&mut field));
            } else {
                field.clear();
            }
            first_row = false;
            first = true;
            at_start = true;
        } else if c != '\r' {
            at_start = false;
            if first {
                field.push(c);
            }
        }
    }
    rows
}

/// The Arabic words of a comment, short vowels dropped.
pub fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if matches!(c as u32, 0x0621..=0x063A | 0x0641..=0x064A | 0x0671 | 0x064B..=0x065F | 0x0670 | 0x0640) || "ڨڤپچگ".contains(c) {
            cur.push(c);
        } else if !cur.is_empty() {
            let w = strip_arabic_marks(&std::mem::take(&mut cur));
            let cs: Vec<char> = w.chars().collect();
            if (2..=24).contains(&cs.len()) && !cs.windows(3).any(|p| p[0] == p[1] && p[1] == p[2]) {
                out.push(w);
            }
        }
    }
    out
}

/// `word count` lines for the words of `rows` that the reference list lacks and that occur at least `min` times.
pub fn build(rows: &[String], reference: &WordList, min: usize) -> String {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for r in rows {
        for w in words(r) {
            *counts.entry(w).or_default() += 1;
        }
    }
    let mut v: Vec<(String, usize)> = counts.into_iter().filter(|(w, n)| *n >= min && !reference.contains_word(w)).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v.into_iter().map(|(w, n)| format!("{w} {n}\n")).collect()
}

fn main() {
    let (mut csv, mut out, mut min): (Option<String>, Option<String>, usize) = (None, None, 4);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--csv" => csv = it.next(),
            "--out" => out = it.next(),
            "--min" => min = it.next().and_then(|s| s.parse().ok()).unwrap_or(4),
            _ => {}
        }
    }
    let (Some(csv), Some(out)) = (csv, out) else {
        eprintln!("usage: build_dz_ar --csv <comments.csv> --out <dz-ar.txt> [--min 4]");
        std::process::exit(2);
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let list = std::fs::read_to_string(root.join("ar.txt")).unwrap_or_else(|e| {
        eprintln!("the word list data/words/ar.txt is needed (run scripts/fetch-data.ps1): {e}");
        std::process::exit(1);
    });
    let reference = WordList::parse_for(Lang::Ar, &list, 50_000);
    let text = std::fs::read_to_string(&csv).unwrap_or_else(|e| {
        eprintln!("cannot read {csv}: {e}");
        std::process::exit(1);
    });
    let rows = comments(&text);
    let built = build(&rows, &reference, min);
    std::fs::write(&out, &built).expect("write the list");
    println!("{} comments, {} words kept -> {out}", rows.len(), built.lines().count());
}
