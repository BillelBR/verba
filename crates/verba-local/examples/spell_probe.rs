//! Looks at words in the lists: how often each occurs, its rank, and the most frequent words one edit away.
//! For understanding why a word is or is not corrected.
//!
//!   cargo run --release -p verba-local --example spell_probe -- wich seperate tres

use std::path::Path;

use verba_core::Lang;
use verba_local::LocalEngine;

fn one_edit(a: &[char], b: &[char]) -> bool {
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    if a.len() == b.len() {
        let diff: Vec<usize> = (0..a.len()).filter(|i| a[*i] != b[*i]).collect();
        return match diff.len() {
            1 => true,
            2 => diff[1] == diff[0] + 1 && a[diff[0]] == b[diff[1]] && a[diff[1]] == b[diff[0]],
            _ => false,
        };
    }
    let (s, l) = if a.len() < b.len() { (a, b) } else { (b, a) };
    (0..l.len()).any(|i| {
        let mut v = l.to_vec();
        v.remove(i);
        v == s
    })
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let engine = LocalEngine::load_dir(&dir).expect("word lists");
    for word in std::env::args().skip(1) {
        println!("\n== {word}");
        let w: Vec<char> = word.chars().collect();
        for lang in [Lang::En, Lang::Fr, Lang::Ar] {
            let Some(list) = engine.list(lang) else { continue };
            let mut v: Vec<(String, u64)> = list.iter_words().map(|(w, c)| (w.to_string(), c)).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            if let Some((rank, (_, count))) = v.iter().enumerate().find(|(_, (x, _))| *x == word) {
                println!("  {}: IN THE LIST, rank {rank}, count {count}", lang.code());
            }
            let near: Vec<String> = v
                .iter()
                .enumerate()
                .filter(|(_, (x, _))| *x != word && one_edit(&w, &x.chars().collect::<Vec<_>>()))
                .take(6)
                .map(|(r, (x, c))| format!("{x} (rank {r}, count {c})"))
                .collect();
            if !near.is_empty() {
                println!("  {}: one edit away: {}", lang.code(), near.join(", "));
            }
        }
        for only in [None, Some(Lang::En), Some(Lang::Fr)] {
            let f = engine.correct(only, &format!("{word} "), 3);
            println!("  correct({only:?}) -> {:?}", f.map(|f| f.candidates.into_iter().map(|c| c.text).collect::<Vec<_>>()));
        }
    }
}
