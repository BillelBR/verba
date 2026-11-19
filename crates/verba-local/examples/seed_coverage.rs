//! What the words written for Verba (`seed/ar-common.txt`) add to the dictionary and to the Arabic word list, and whether they agree with the
//! dictionary where it already has the word. Nothing is changed: it only reads.
//!
//!   cargo run --release -p verba-local --example seed_coverage -- [--dir data/words] [--show 40]
//!
//! For every line: the dictionary WITHOUT the seed is asked for the Arabic word (the lookup that strips particles and pronoun endings included,
//! as the program does). No answer: the line adds a headword. An answer that shares a word with the translations written here: they agree.
//! An answer that shares none: they disagree, and the line is printed to be read (a wrong line here, or a doubtful entry of the dictionary).
//! For a one-word headword, the Arabic list (the cut shipped, without the seed) is asked too: if it lacks the word, the suggestions gain it.

use std::path::PathBuf;

use verba_core::Lang;
use verba_local::{Dictionary, LocalEngine};

/// The words of an English translation, each cut to a rough stem (`accounts` and `account` agree, `said` and `say` do not need to).
fn words_of(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .filter(|w| w != "to" && w != "the" && w != "a")
        .map(|w| {
            let w = w.as_str();
            let stem = w.strip_suffix("ies").map(|x| format!("{x}y")).or_else(|| w.strip_suffix("es").map(str::to_string)).or_else(|| w.strip_suffix('s').map(str::to_string)).or_else(|| w.strip_suffix("ing").map(str::to_string)).or_else(|| w.strip_suffix("ed").map(str::to_string));
            stem.filter(|x| x.len() >= 3).unwrap_or_else(|| w.to_string())
        })
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let dir = PathBuf::from(arg("--dir").unwrap_or_else(|| "data/words".to_string()));
    let show: usize = arg("--show").and_then(|v| v.parse().ok()).unwrap_or(40);

    // the dictionary as the program has it, WITHOUT the seed: the file and the hand-written corrections (`seed/dict-fixes.txt`)
    let mut dict = Dictionary::parse(&std::fs::read_to_string(dir.join("dict.txt")).expect("data/words/dict.txt"));
    dict.apply_fixes(include_str!("../seed/dict-fixes.txt"));
    let engine = LocalEngine::load_lists(&dir, verba_local::list_cap(Lang::Ar)).expect("the lists");
    let seed = include_str!("../seed/ar-common.txt");

    let (mut lines, mut missing, mut agree, mut disagree) = (0usize, 0usize, 0usize, 0usize);
    let (mut single, mut absent_from_list) = (0usize, 0usize);
    let mut disagreements: Vec<String> = Vec::new();
    let mut missing_examples: Vec<String> = Vec::new();
    let mut first_differs: Vec<String> = Vec::new();
    for line in seed.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let mut fields = line.split('\t');
        let (Some(head), Some(tr)) = (fields.next(), fields.next()) else { continue };
        lines += 1;
        let mine: Vec<String> = tr.split(';').flat_map(words_of).collect();
        match dict.translate(head, &[Lang::En], Some(Lang::Ar)) {
            None => {
                missing += 1;
                if missing_examples.len() < 12 {
                    missing_examples.push(format!("{head}\t{tr}"));
                }
            }
            Some(t) => {
                let theirs: Vec<String> = t.candidates.iter().flat_map(|c| words_of(&c.text)).collect();
                if mine.iter().any(|m| theirs.contains(m)) {
                    agree += 1;
                    // they share a word, but the dictionary's FIRST translation is none of mine: the first one shown may be a poor one
                    let first: Vec<String> = t.candidates.first().map(|c| words_of(&c.text)).unwrap_or_default();
                    if !first.iter().any(|f| mine.contains(f)) {
                        first_differs.push(format!("{head}\t mine: {tr}\t dictionary: {}", t.candidates.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join("; ")));
                    }
                } else {
                    disagree += 1;
                    disagreements.push(format!("{head}\t mine: {tr}\t dictionary: {}", t.candidates.iter().map(|c| c.text.as_str()).collect::<Vec<_>>().join("; ")));
                }
            }
        }
        if !head.contains(' ') {
            single += 1;
            if !engine.is_word_of(Lang::Ar, head) {
                absent_from_list += 1;
            }
        }
    }
    println!("lines of the seed: {lines}");
    println!("dictionary: the lookup finds nothing for {missing} ({:.1} %): these add a headword", missing as f64 * 100.0 / lines as f64);
    println!("            the translations share a word with the dictionary's for {agree} ({:.1} %)", agree as f64 * 100.0 / lines as f64);
    println!("            they share none for {disagree} ({:.1} %): to be read", disagree as f64 * 100.0 / lines as f64);
    println!("suggestions: of {single} one-word headwords, the Arabic list lacks {absent_from_list} ({:.1} %): these are added", absent_from_list as f64 * 100.0 / single.max(1) as f64);
    println!("\nexamples of what the dictionary lacked:");
    for m in &missing_examples {
        println!("  {m}");
    }
    println!("\nthe dictionary's first translation is none of mine, though they share a word ({}):", first_differs.len());
    for d in first_differs.iter().take(show) {
        println!("  {d}");
    }
    println!("\ndisagreements (first {show}):");
    for d in disagreements.iter().take(show) {
        println!("  {d}");
    }
}
