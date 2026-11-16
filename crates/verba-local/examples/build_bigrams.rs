//! Builds a language's next-word table (`data/words/<code>.bigrams.txt`) from a Tatoeba sentence export.
//!
//!   cargo run --release -p verba-local --example build_bigrams -- --lang en --tsv data\words\src\eng_sentences.tsv --out data\words\en.bigrams.txt
//!
//! Normally run by `scripts/fetch-bigrams.ps1`, which downloads the sentences first. The table keeps counts only: no sentence is stored.
//! It needs the unigram list of the language (`scripts/fetch-data.ps1`) to leave out the words that are a trait of the corpus
//! (the names Tom and Mary in Tatoeba), see docs/DATA.md.

use std::path::{Path, PathBuf};

use verba_core::Lang;
use verba_local::{BuildParams, NextBuilder, WordList};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let (mut lang, mut tsv, mut out): (Lang, Option<PathBuf>, Option<PathBuf>) = (Lang::En, None, None);
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--lang" => {
                lang = match it.next().as_deref() {
                    Some("fr") => Lang::Fr,
                    Some("ar") => Lang::Ar,
                    _ => Lang::En,
                }
            }
            "--tsv" => tsv = it.next().map(PathBuf::from),
            "--out" => out = it.next().map(PathBuf::from),
            _ => {}
        }
    }
    let (Some(tsv), Some(out)) = (tsv, out) else {
        eprintln!("usage: build_bigrams --lang en|fr|ar --tsv <tatoeba sentences.tsv> --out <file>");
        std::process::exit(2);
    };
    let list = std::fs::read_to_string(root.join(format!("{}.txt", lang.code()))).unwrap_or_else(|e| {
        eprintln!("the word list data/words/{}.txt is needed (run scripts/fetch-data.ps1): {e}", lang.code());
        std::process::exit(1);
    });
    let reference = WordList::parse_for(lang, &list, 50_000);
    let corpus = std::fs::read(&tsv).unwrap_or_else(|e| {
        eprintln!("cannot read {}: {e}", tsv.display());
        std::process::exit(1);
    });
    let corpus = String::from_utf8_lossy(&corpus);

    let mut builder = NextBuilder::new(lang);
    let mut sentences = 0usize;
    for line in corpus.lines() {
        // Tatoeba export: id <TAB> language code <TAB> sentence
        if let Some(text) = line.splitn(3, '\t').nth(2) {
            sentences += usize::from(builder.add_sentence(text));
        }
    }
    let table = builder.build(&reference, &BuildParams::for_lang(lang));
    let header = format!(
        "Verba next-word table ({}): for each frequent word (or pair of words) the words that most often follow it, as counts.\n\
         Built from {sentences} sentences of Tatoeba (https://tatoeba.org), (c) Tatoeba contributors, licensed CC BY 2.0 FR.\n\
         Counts only: no sentence is stored. Format: context<TAB>times seen<TAB>word:count word:count ...",
        lang.code()
    );
    let text = table.to_text(&header);
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).expect("cannot create the output folder");
    }
    std::fs::write(&out, &text).expect("cannot write the table");
    println!("{}: {} sentences, {} words -> {} contexts, {:.0} KB in {}", lang.code(), sentences, builder.token_count(), table.len(), text.len() as f64 / 1024.0, out.display());
}
