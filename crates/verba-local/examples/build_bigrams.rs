//! Builds a language's next-word table (`data/words/<code>.bigrams.txt`) from a Tatoeba sentence export.
//!
//!   cargo run --release -p verba-local --example build_bigrams -- --lang en --tsv data\words\src\eng_sentences.tsv --out data\words\en.bigrams.txt
//!   ... --lang ar --tsv data\words\src\ara_sentences.tsv@6 --tsv data\words\src\ara_wikipedia_2021_100K-sentences.txt --out ...
//!
//! `--tsv` can be given several times; `@6` after a path makes each sentence of that file count 6 times (the everyday sentences of Tatoeba
//! against the many more sentences of Wikipedia, which are another kind of text, see docs/ACCURACY.md). A line is Tatoeba's
//! `id<TAB>language<TAB>sentence` or the Leipzig Corpora Collection's `id<TAB>sentence`. A file with `wikipedia` in its name is credited as
//! Wikipedia text, a file with `algerian` in its name (the comments of the "Algerian Dialect" dataset, see `dz_comments.rs`) as those, any other as
//! Tatoeba.
//!
//! Normally run by `scripts/fetch-bigrams.ps1`, which downloads the sentences first. The table keeps counts only: no sentence is stored.
//! It needs the unigram list of the language (`scripts/fetch-data.ps1`) to leave out the words that are a trait of the corpus
//! (the names Tom and Mary in Tatoeba), see docs/DATA.md.

use std::path::{Path, PathBuf};

use verba_core::Lang;
use verba_local::{BuildParams, NextBuilder, WordList};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let (mut lang, mut out): (Lang, Option<PathBuf>) = (Lang::En, None);
    let mut tsvs: Vec<(PathBuf, usize)> = Vec::new();
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
            "--tsv" => {
                if let Some(spec) = it.next() {
                    match spec.rsplit_once('@').and_then(|(p, w)| w.parse::<usize>().ok().map(|w| (p.to_string(), w))) {
                        Some((p, w)) => tsvs.push((PathBuf::from(p), w)),
                        None => tsvs.push((PathBuf::from(spec), 1)),
                    }
                }
            }
            "--out" => out = it.next().map(PathBuf::from),
            _ => {}
        }
    }
    let (false, Some(out)) = (tsvs.is_empty(), out) else {
        eprintln!("usage: build_bigrams --lang en|fr|ar --tsv <sentences.tsv>[@weight] [--tsv ...] --out <file>");
        std::process::exit(2);
    };
    let list = std::fs::read_to_string(root.join(format!("{}.txt", lang.code()))).unwrap_or_else(|e| {
        eprintln!("the word list data/words/{}.txt is needed (run scripts/fetch-data.ps1): {e}", lang.code());
        std::process::exit(1);
    });
    let reference = WordList::parse_for(lang, &list, 50_000);
    let mut builder = NextBuilder::new(lang);
    if lang == Lang::Ar {
        // the words of Algerian Darija (written for Verba) are not a trait of the comments, whatever the subtitle list says
        for (word, _) in verba_local::DzList::seed_ar().iter_words() {
            builder.keep_word(word);
        }
    }
    let (mut sentences, mut tatoeba, mut wikipedia, mut comments) = (0usize, 0usize, 0usize, 0usize);
    for (tsv, weight) in &tsvs {
        let corpus = std::fs::read(tsv).unwrap_or_else(|e| {
            eprintln!("cannot read {}: {e}", tsv.display());
            std::process::exit(1);
        });
        let corpus = String::from_utf8_lossy(&corpus);
        let name = tsv.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let (from_wikipedia, from_comments) = (name.contains("wikipedia"), name.contains("algerian"));
        let mut added = 0usize;
        for line in corpus.lines() {
            // Tatoeba export: id <TAB> language code <TAB> sentence; Leipzig: id <TAB> sentence
            let mut f = line.splitn(3, '\t');
            let (Some(_), Some(second)) = (f.next(), f.next()) else { continue };
            let text = f.next().unwrap_or(second);
            added += usize::from(builder.add_sentence_times(text, *weight));
        }
        sentences += added;
        if from_wikipedia {
            wikipedia += added;
        } else if from_comments {
            comments += added;
        } else {
            tatoeba += added;
        }
    }
    let table = builder.build(&reference, &BuildParams::for_lang(lang));
    let mut credits = String::new();
    if tatoeba > 0 {
        credits.push_str(&format!("Built from {tatoeba} sentences of Tatoeba (https://tatoeba.org), (c) Tatoeba contributors, licensed CC BY 2.0 FR.\n"));
    }
    if wikipedia > 0 {
        credits.push_str(&format!(
            "Also from {wikipedia} sentences of Wikipedia (https://www.wikipedia.org), (c) Wikipedia contributors, licensed CC BY-SA 4.0, as collected by the Leipzig Corpora Collection (https://wortschatz.uni-leipzig.de).\n"
        ));
    }
    if comments > 0 {
        credits.push_str(&format!(
            "Also from {comments} comments of Algerian press channels on YouTube, the dataset \"Algerian Dialect\" by Zakaria Benmounah and Abdennour Boulesnane (Mendeley Data V2, doi 10.17632/zzwg3nnhsz.2), licensed CC BY 4.0.\n"
        ));
    }
    let header = format!(
        "Verba next-word table ({}): for each frequent word (or pair of words) the words that most often follow it, as counts.\n\
         {credits}Counts only: no sentence is stored. Format: context<TAB>times seen<TAB>word:count word:count ...",
        lang.code()
    );
    let text = table.to_text(&header);
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).expect("cannot create the output folder");
    }
    std::fs::write(&out, &text).expect("cannot write the table");
    println!("{}: {} sentences, {} words -> {} contexts, {:.0} KB in {}", lang.code(), sentences, builder.token_count(), table.len(), text.len() as f64 / 1024.0, out.display());
}
