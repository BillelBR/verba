//! Writes the comments of "Algerian Dialect" as sentence files for the next-word tools: `id<TAB>comment`, one per line.
//!
//!   cargo run --release -p verba-local --example dz_comments -- --csv data/dz/ar-youtube/comments.csv --from 1 --to 25000 --out train.txt
//!
//! `--from` and `--to` are comment numbers (1 based, both included). The files stay under `data/` (never in the repository): they are used
//! to build the Darija part of the next-word table and to measure it on comments the table was not built from (see docs/ACCURACY.md).

#[path = "build_dz_ar.rs"]
#[allow(dead_code)]
mod corpus;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |n: &str| args.iter().position(|a| a == n).and_then(|i| args.get(i + 1)).cloned();
    let (Some(csv), Some(out)) = (arg("--csv"), arg("--out")) else {
        eprintln!("usage: dz_comments --csv <comments.csv> --out <file> [--from 1] [--to 45000]");
        std::process::exit(2);
    };
    let from: usize = arg("--from").and_then(|s| s.parse().ok()).unwrap_or(1);
    let to: usize = arg("--to").and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
    let text = std::fs::read_to_string(&csv).expect("the csv");
    let rows = corpus::comments(&text);
    let mut written = String::new();
    let mut n = 0;
    for (i, row) in rows.iter().enumerate() {
        let number = i + 1;
        if number < from || number > to {
            continue;
        }
        let one_line: String = row.split_whitespace().collect::<Vec<_>>().join(" ");
        if one_line.is_empty() {
            continue;
        }
        written.push_str(&format!("{number}\t{one_line}\n"));
        n += 1;
    }
    std::fs::write(&out, written).expect("write");
    println!("{n} comments ({from}..{to}) -> {out}");
}
