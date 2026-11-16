//! Measures Algerian Darija written in Arabic letters: does it find its words, does it leave Modern Standard Arabic alone, and does the
//! spelling correction still do its job.
//!
//!   cargo run --release -p verba-local --example dz_ar_accuracy -- [--misses 20]
//!
//! Needs `data/words/ar.txt` (scripts/fetch-data.ps1). The real-text part also needs the YouTube comments of
//! `scripts/fetch-dz-ar.ps1` in `data/dz/ar-youtube/comments.csv` (CC BY 4.0); it is skipped without them.
//!
//! IMPORTANT: the Darija list is a seed written by the Verba authors. Every number measured on its own words (sections 1 and 1b) is
//! optimistic: the list knows the words it is asked about. Section 3 uses real comments that nobody wrote the list from.

#[path = "build_dz_ar.rs"]
#[allow(dead_code)]
mod corpus;

use std::collections::HashSet;
use std::path::Path;

use verba_core::{Engine, Lang};
use verba_local::{DzList, LocalEngine, dz_ar_key};

fn pct(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 }
}

fn main() {
    let misses: usize = std::env::args().position(|a| a == "--misses").and_then(|i| std::env::args().nth(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(0);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(plain) = LocalEngine::load_dir(&root.join("data/words")) else { return println!("no word lists: run scripts/fetch-data.ps1") };
    let mut engine = LocalEngine::load_dir(&root.join("data/words")).unwrap();
    engine.add_dz_seed();
    engine.set_enabled(Lang::Dz, true);
    let seed = DzList::seed_ar();
    println!("Darija (Arabic letters) seed: {} words\n", seed.len());

    // ---------------------------------------------------------------- 1. the seed's own words (optimistic)
    let words: Vec<String> = seed.iter_words().map(|(w, _)| w.to_string()).filter(|w| w.chars().count() >= 4).collect();
    println!("## 1. Seed words typed letter by letter, Dz on with the Arabic list (OPTIMISTIC: the list knows these words)\n");
    println!("{} words of 4+ letters, 3 suggestions. The word counts as found when this spelling, or another spelling with the same key, is in the popup.\n", words.len());
    println!("| context before | letters | top-1 | top-3 | top-3 (any spelling) |");
    println!("|---|---|---|---|---|");
    for (context, label) in [("", "nothing"), ("واش راني ", "Darija (واش راني)"), ("ذهبت الى ", "Modern Standard (ذهبت الى)")] {
        for n in [2usize, 3, 4] {
            let (mut t1, mut t3, mut s3, mut total) = (0, 0, 0, 0);
            let mut missed = Vec::new();
            for w in &words {
                if w.chars().count() <= n {
                    continue;
                }
                total += 1;
                let prefix: String = w.chars().take(n).collect();
                let got: Vec<String> = engine.suggest(&format!("{context}{prefix}"), 3).into_iter().map(|s| s.text).collect();
                t1 += usize::from(got.first() == Some(w));
                t3 += usize::from(got.contains(w));
                s3 += usize::from(got.iter().any(|g| g == w || dz_ar_key(g) == dz_ar_key(w)));
                if misses > 0 && n == 3 && context.is_empty() && !got.contains(w) && missed.len() < misses {
                    missed.push(format!("{prefix} -> {got:?} (wanted {w})"));
                }
            }
            println!("| {label} | {n} | {:.1}% | {:.1}% | {:.1}% |", pct(t1, total), pct(t3, total), pct(s3, total));
            for m in missed {
                println!("    miss: {m}");
            }
        }
    }

    println!("\n## 1b. A spelling held out: another spelling of the word is still offered\n");
    let all: Vec<(String, u64)> = seed.iter_words().map(|(w, c)| (w.to_string(), c)).collect();
    let mut by_key: std::collections::HashMap<String, Vec<&str>> = std::collections::HashMap::new();
    for (w, _) in &all {
        by_key.entry(dz_ar_key(w)).or_default().push(w);
    }
    let (mut total, mut hit) = (0, 0);
    for members in by_key.values().filter(|g| g.len() >= 2) {
        for held in members.iter().filter(|w| w.chars().count() >= 4) {
            let text: String = all.iter().filter(|(w, _)| w != held).map(|(w, c)| format!("{w} {c}\n")).collect();
            let mut e = LocalEngine::new();
            e.set_dz_ar(DzList::parse_ar(&text));
            e.set_enabled(Lang::Dz, true);
            let prefix: String = held.chars().take(3).collect();
            total += 1;
            hit += usize::from(e.suggest_in(Lang::Dz, &prefix, 3).iter().any(|s| dz_ar_key(&s.text) == dz_ar_key(held)));
            if total >= 400 {
                break;
            }
        }
        if total >= 400 {
            break;
        }
    }
    println!("{total} held-out spellings (typed to 3 letters, Darija alone): a sibling spelling is in the top 3 for {:.1}%", pct(hit, total));

    // ---------------------------------------------------------------- 2. false alarms on Modern Standard Arabic
    println!("\n## 2. False alarms: Modern Standard Arabic typed with Dz on\n");
    let mut ranked: Vec<(String, u64)> = plain.list(Lang::Ar).unwrap().iter_words().map(|(w, c)| (w.to_string(), c)).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let msa: Vec<String> = ranked.iter().map(|x| x.0.clone()).filter(|w| w.chars().count() >= 4).take(3000).collect();
    println!("The 3000 most frequent words (4+ letters) of the Arabic list, typed letter by letter, 3 suggestions. A false alarm is a suggestion that is\nnot among the 3 the Arabic list alone gives (a Darija word came in), and a first-place false alarm is one that changes the first suggestion.\n");
    println!("| letters | any new suggestion in the top 3 | a Darija-only word (the Arabic list lacks it) in the top 3 | it is the first suggestion |");
    println!("|---|---|---|---|");
    let reference = plain.list(Lang::Ar).unwrap();
    let mut shown = Vec::new();
    for n in [2usize, 3, 4] {
        let (mut any, mut only, mut first, mut total) = (0, 0, 0, 0);
        for w in &msa {
            if w.chars().count() <= n {
                continue;
            }
            total += 1;
            let typed: String = w.chars().take(n).collect();
            let on = engine.suggest_in(Lang::Ar, &typed, 3);
            let off = plain.suggest_in(Lang::Ar, &typed, 3);
            let new: Vec<&str> = on.iter().map(|s| s.text.as_str()).filter(|t| !off.iter().any(|o| o.text == *t)).collect();
            any += usize::from(!new.is_empty());
            if new.iter().any(|t| !reference.contains_word(t)) {
                only += 1;
                first += usize::from(on.first().map(|s| s.text.as_str()) != off.first().map(|s| s.text.as_str()));
                if n == 3 && shown.len() < misses {
                    shown.push(format!("{typed} -> {:?} (without Dz {:?})", on.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), off.iter().map(|s| s.text.as_str()).collect::<Vec<_>>()));
                }
            }
        }
        println!("| {n} | {:.2}% | {:.2}% | {:.2}% |", pct(any, total), pct(only, total), pct(first, total));
    }
    for s in shown {
        println!("    shown: {s}");
    }
    {
        let mut off = LocalEngine::load_dir(&root.join("data/words")).unwrap();
        off.add_dz_seed();
        off.set_enabled(Lang::Dz, false);
        let mut same = true;
        for w in msa.iter().take(1500) {
            let t: String = w.chars().take(3).collect();
            let typo: String = w.chars().take(w.chars().count() - 1).collect::<String>() + "ي";
            same &= off.suggest(&t, 3) == plain.suggest(&t, 3) && off.correct(None, &format!("{typo} "), 3) == plain.correct(None, &format!("{typo} "), 3);
        }
        println!("\nDz loaded but switched off: identical to an engine without Dz on 1500 prefixes and typos: {same}");
    }

    // spelling correction of ordinary typos
    println!("\n### Spelling correction on Modern Standard Arabic, Dz on against no Dz\n");
    println!("Typos of the 3000 most frequent words (5+ letters, a typo that is itself a listed word is skipped), first fix right / right fix among the first 3:\n");
    println!("| typo | with Dz | without Dz |");
    println!("|---|---|---|");
    let long: Vec<&String> = msa.iter().filter(|w| w.chars().count() >= 5).collect();
    let afold = |s: &str| ar_fold(s);
    for (kind, typo) in [("two letters swapped", 0usize), ("a letter doubled", 1), ("a letter dropped", 2)] {
        let (mut a1, mut a3, mut b1, mut b3, mut n) = (0, 0, 0, 0, 0);
        let mut lost: Vec<String> = Vec::new();
        for w in &long {
            let c: Vec<char> = w.chars().collect();
            let t: String = match typo {
                0 => {
                    let mut v = c.clone();
                    v.swap(2, 3);
                    v.into_iter().collect()
                }
                1 => {
                    let mut v = c.clone();
                    v.insert(3, c[3]);
                    v.into_iter().collect()
                }
                _ => c.iter().enumerate().filter(|(i, _)| *i != 3).map(|(_, ch)| *ch).collect(),
            };
            if t == **w || plain.list(Lang::Ar).unwrap().contains_word(&t) {
                continue;
            }
            n += 1;
            let ask = |e: &LocalEngine| -> Vec<String> { e.correct(Some(Lang::Ar), &format!("{t} "), 3).map(|f| f.candidates.into_iter().map(|c| afold(&c.text)).collect()).unwrap_or_default() };
            let (on, off) = (ask(&engine), ask(&plain));
            let want = afold(w);
            a1 += usize::from(on.first() == Some(&want));
            a3 += usize::from(on.contains(&want));
            b1 += usize::from(off.first() == Some(&want));
            b3 += usize::from(off.contains(&want));
            if misses > 0 && off.first() == Some(&want) && on.first() != Some(&want) && lost.len() < misses {
                lost.push(format!("{t} -> {:?} (without Dz {:?}; wanted {w})", on, off));
            }
        }
        for l in lost.drain(..) {
            println!("    lost: {l}");
        }
        println!("| {kind} ({n} typos) | {:.1}% / {:.1}% | {:.1}% / {:.1}% |", pct(a1, n), pct(a3, n), pct(b1, n), pct(b3, n));
    }
    println!("\n(The full harness of docs/ACCURACY.md: `cargo run --release -p verba-local --example spell_accuracy -- --dz` against the same without `--dz`.)");

    {
        let started = std::time::Instant::now();
        let mut calls = 0;
        for w in msa.iter().take(2000) {
            let typed: String = w.chars().take(3).collect();
            let _ = engine.suggest(&format!("واش راني {typed}"), 3);
            calls += 1;
        }
        let on = started.elapsed().as_secs_f64() * 1e6 / calls as f64;
        let started = std::time::Instant::now();
        for w in msa.iter().take(2000) {
            let typed: String = w.chars().take(3).collect();
            let _ = plain.suggest(&format!("واش راني {typed}"), 3);
        }
        println!("\nOne suggest() call (release build): {on:.0} microseconds with Dz on, {:.0} without (average over {calls} calls)", started.elapsed().as_secs_f64() * 1e6 / calls as f64);
    }
    if std::env::args().any(|a| a == "--quick") {
        return;
    }
    // ---------------------------------------------------------------- 3. real comments
    let csv = root.join("data/dz/ar-youtube/comments.csv");
    let Ok(text) = std::fs::read_to_string(&csv) else {
        println!("\n## 3. Real text\n\n(no comments in {}: run scripts/fetch-dz-ar.ps1)", csv.display());
        return;
    };
    let rows = corpus::comments(&text);
    println!("\n## 3. Real Algerian comments in Arabic letters (Mendeley Data, CC BY 4.0, {} comments)\n", rows.len());
    // dev: rows 0..5000 (read while the seed was being finished), extra: 5000..25000 (the optional list is built from these), sealed: 25000..
    let (dev, rest) = rows.split_at(5000.min(rows.len()));
    let (extra_rows, sealed) = rest.split_at(20_000.min(rest.len()));
    let reference = plain.list(Lang::Ar).unwrap();
    for (title, set) in [("dev (comments 1 to 5000, looked at while finishing the seed)", dev), ("sealed (comments 25001 to the end, never looked at)", sealed)] {
        report_real(title, set, &engine, &plain, &seed, misses);
    }
    // the optional list, built from the middle part and tried on the sealed part
    let built = corpus::build(extra_rows, reference, 4);
    let extra = DzList::parse_ar(&built);
    let mut with_extra = LocalEngine::load_dir(&root.join("data/words")).unwrap();
    with_extra.add_dz_seed();
    let mut merged = DzList::seed_ar();
    merged.merge_extra(&extra);
    with_extra.set_dz_ar(merged);
    with_extra.set_enabled(Lang::Dz, true);
    println!("### With the optional list ({} words built from comments 5001 to 25000) merged in, measured on the sealed part\n", extra.len());
    report_real("sealed, seed + optional list", sealed, &with_extra, &plain, &seed, 0);
}

/// The same Arabic letters for the engine: hamza, ta marbuta, alef maqsura.
fn ar_fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
            'ة' => 'ه',
            'ى' | 'ئ' => 'ي',
            'ؤ' => 'و',
            c => c,
        })
        .collect()
}

fn report_real(title: &str, rows: &[String], engine: &LocalEngine, plain: &LocalEngine, seed: &DzList, misses: usize) {
    let reference = plain.list(Lang::Ar).unwrap();
    println!("### {title}\n");
    // tokens with the words before them
    let mut toks: Vec<(String, String)> = Vec::new(); // (before, token)
    for r in rows {
        let ws = corpus::words(r);
        for (i, w) in ws.iter().enumerate() {
            if w.chars().count() >= 3 {
                toks.push((ws[..i].iter().rev().take(3).rev().cloned().collect::<Vec<_>>().join(" "), w.clone()));
            }
        }
    }
    let total = toks.len();
    let in_msa = |w: &str| reference.contains_word(w);
    let outside: Vec<&(String, String)> = toks.iter().filter(|t| !in_msa(&t.1)).collect();
    let known = toks.iter().filter(|t| engine.dz_knows(&t.1)).count();
    let out_known = outside.iter().filter(|t| engine.dz_knows(&t.1)).count();
    println!("{} comments, {} words of 3+ letters ({} not in the Arabic list: {:.1}%).\n", rows.len(), total, outside.len(), pct(outside.len(), total));
    println!("- the Darija list knows {:.1}% of all the words, and **{:.1}% of the words the Arabic list does not have** (listed, another spelling, or with a negation / pronoun)", pct(known, total), pct(out_known, outside.len()));
    // what the spelling correction does to the words the Arabic list does not have
    let flag = |e: &LocalEngine, set: &[&(String, String)]| set.iter().filter(|t| e.correct(Some(Lang::Ar), &format!("{} {} ", t.0, t.1), 3).is_some()).count();
    let sample: Vec<&(String, String)> = outside.iter().copied().take(6000).collect();
    println!("- spelling correction on {} of those words: {:.1}% get a Fix with Dz on, {:.1}% without Dz", sample.len(), pct(flag(engine, &sample), sample.len()), pct(flag(plain, &sample), sample.len()));
    let in_sample: Vec<&(String, String)> = toks.iter().filter(|t| in_msa(&t.1)).take(6000).collect();
    println!("- and on {} words the Arabic list has: {:.1}% get a Fix with Dz on, {:.1}% without", in_sample.len(), pct(flag(engine, &in_sample), in_sample.len()), pct(flag(plain, &in_sample), in_sample.len()));
    // every 4th word keeps the run short (the sets are still tens of thousands of words)
    let toks: Vec<(String, String)> = toks.into_iter().enumerate().filter(|(i, _)| i % 4 == 0).map(|(_, t)| t).collect();
    println!("\nCompletion: every 4th word typed as a prefix after the (up to 3) words before it, 3 suggestions, Dz on / without Dz:\n");
    println!("| set | letters | words | exact word in top-3 (on) | (off) | top-1 (on) | (off) |");
    println!("|---|---|---|---|---|---|---|");
    for (label, filter) in [("all words", 0), ("words the Arabic list lacks", 1), ("words the Darija list knows", 2)] {
        for n in [2usize, 3] {
            let (mut t3on, mut t3off, mut t1on, mut t1off, mut count) = (0, 0, 0, 0, 0);
            let mut gone = Vec::new();
            for (before, w) in &toks {
                if w.chars().count() <= n + 1 {
                    continue;
                }
                let ok = match filter {
                    0 => true,
                    1 => !in_msa(w),
                    _ => engine.dz_knows(w),
                };
                if !ok {
                    continue;
                }
                count += 1;
                let typed: String = w.chars().take(n).collect();
                let text = if before.is_empty() { typed.clone() } else { format!("{before} {typed}") };
                let (on, off) = (engine.suggest(&text, 3), plain.suggest(&text, 3));
                let (hit_on, hit_off) = (on.iter().any(|s| s.text == *w), off.iter().any(|s| s.text == *w));
                t3on += usize::from(hit_on);
                t3off += usize::from(hit_off);
                t1on += usize::from(on.first().is_some_and(|s| s.text == *w));
                t1off += usize::from(off.first().is_some_and(|s| s.text == *w));
                if misses > 0 && filter == 0 && n == 3 && hit_off && !hit_on && gone.len() < misses {
                    gone.push(format!("{text} -> {:?} (without Dz {:?})", on.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), off.iter().map(|s| s.text.as_str()).collect::<Vec<_>>()));
                }
            }
            println!("| {label} | {n} | {count} | {:.1}% | {:.1}% | {:.1}% | {:.1}% |", pct(t3on, count), pct(t3off, count), pct(t1on, count), pct(t1off, count));
            for g in gone {
                println!("    lost: {g}");
            }
        }
    }
    let _ = seed;
    let distinct: HashSet<&str> = toks.iter().map(|t| t.1.as_str()).collect();
    println!("\n({} distinct words)\n", distinct.len());
}
