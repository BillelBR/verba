//! Measures the Algerian Darija (Arabizi) suggestions: does Dz find its words, and does it leave English and French alone.
//!
//!   cargo run --release -p verba-local --example dz_accuracy -- [--misses 30]
//!
//! Needs `data/words/{en,fr}.txt` (scripts/fetch-data.ps1). The optional real-text check also needs the NArabizi treebank
//! in `data/dz/narabizi/` (see docs/DATA.md); it is skipped without it.
//!
//! IMPORTANT: the Darija list is a seed written by the Verba authors. Every number measured on its own words
//! (sections 1 and 2) is optimistic: the list knows the words it is asked about. Section 3 uses real text nobody wrote the list from.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use verba_core::{Engine, Lang};
use verba_local::{DzList, LocalEngine, dz_sound_key};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let misses: usize = args.iter().position(|a| a == "--misses").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(0);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Ok(mut engine) = LocalEngine::load_dir(&root.join("data/words")) else { return println!("no word lists: run scripts/fetch-data.ps1") };
    engine.add_dz_seed();
    let seed = DzList::seed();
    println!("Darija seed: {} words\n", seed.len());

    // ---------------------------------------------------------------- 1. the seed's own words (optimistic)
    engine.set_enabled(Lang::Dz, true);
    let words: Vec<(String, u64)> = seed.iter_words().filter(|(w, _)| w.chars().count() >= 4).map(|(w, c)| (w.to_string(), c)).collect();
    println!("## 1. Seed words typed letter by letter, Dz on with English and French (OPTIMISTIC: the list knows these words)\n");
    println!("{} words of 4+ characters.\n", words.len());
    println!("| context | prefix | top-1 | top-3 | top-3 (this spelling or another spelling of the word) |");
    println!("|---|---|---|---|---|");
    for (context, label) in [("", "none"), ("wach rani ", "Darija before"), ("i think ", "English before")] {
        for n in [2usize, 3, 4] {
            let (mut t1, mut t3, mut s3, mut total) = (0, 0, 0, 0);
            let mut missed = Vec::new();
            for (w, _) in &words {
                if w.chars().count() <= n {
                    continue;
                }
                total += 1;
                let prefix: String = w.chars().take(n).collect();
                let got: Vec<String> = engine.suggest(&format!("{context}{prefix}"), 3).into_iter().map(|s| s.text).collect();
                if got.first() == Some(w) {
                    t1 += 1;
                }
                if got.contains(w) {
                    t3 += 1;
                } else if misses > 0 && n == 3 && context.is_empty() && missed.len() < misses {
                    missed.push(format!("{prefix} -> {got:?} (wanted {w})"));
                }
                if got.iter().any(|g| g == w || dz_sound_key(g) == dz_sound_key(w)) {
                    s3 += 1;
                }
            }
            println!("| {label} | {n} | {:.1}% | {:.1}% | {:.1}% |", pct(t1, total), pct(t3, total), pct(s3, total));
            for m in missed {
                println!("    miss: {m}");
            }
        }
    }

    // ---------------------------------------------------------------- 1b. spelling variants held out
    println!("\n## 1b. A spelling held out: another spelling of the word is still offered\n");
    let mut groups: HashMap<String, Vec<(String, u64)>> = HashMap::new();
    for (w, c) in seed.iter_words() {
        groups.entry(dz_sound_key(w)).or_default().push((w.to_string(), c));
    }
    let (mut total, mut hit) = (0, 0);
    let mut failures = Vec::new();
    for members in groups.values().filter(|g| g.len() >= 2) {
        for (held, _) in members.iter().filter(|(w, _)| w.chars().count() >= 4) {
            // a list without the held-out spelling: is a sibling offered when its prefix is typed?
            let text: String = seed.iter_words().filter(|(w, _)| w != held).map(|(w, c)| format!("{w} {c}\n")).collect();
            let mut e = LocalEngine::new();
            e.set_dz(DzList::parse(&text));
            e.set_enabled(Lang::Dz, true);
            let prefix: String = held.chars().take(3).collect();
            total += 1;
            let got: Vec<String> = e.suggest_in(Lang::Dz, &prefix, 3).into_iter().map(|s| s.text).collect();
            if got.iter().any(|g| dz_sound_key(g) == dz_sound_key(held)) {
                hit += 1;
            } else if failures.len() < misses {
                failures.push(format!("{prefix} -> {got:?} (wanted a spelling of {held})"));
            }
            if total >= 400 {
                break;
            }
        }
        if total >= 400 {
            break;
        }
    }
    println!("{total} held-out spellings (each typed to 3 characters, Dz alone): a sibling spelling is in the top 3 for {:.1}%", pct(hit, total));
    for f in failures {
        println!("    miss: {f}");
    }

    // ---------------------------------------------------------------- 2. false alarms
    println!("\n## 2. False alarms: normal English and French typed with Dz on\n");
    let plain = LocalEngine::load_dir(&root.join("data/words")).unwrap(); // the same lists, no Dz at all
    let target = |engine: &LocalEngine, lang: Lang| -> Vec<String> {
        let mut v: Vec<(String, u64)> = engine.list(lang).map(|l| l.iter_words().map(|(w, c)| (w.to_string(), c)).collect()).unwrap_or_default();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v.into_iter().map(|x| x.0).filter(|w| w.len() >= 4 && w.chars().all(|c| c.is_ascii_lowercase())).take(3000).collect()
    };
    let (en_words, fr_words) = (target(&plain, Lang::En), target(&plain, Lang::Fr));
    println!("The 3000 most frequent plain words (4+ letters) of each list, typed letter by letter, 3 suggestions. A false alarm is a Darija suggestion in the popup.\n");
    println!("| language | context | prefix | Dz in top-1 | Dz in top-3 | top-3 changed vs Dz off |");
    println!("|---|---|---|---|---|---|");
    for (lang, name, contexts) in [(Lang::En, "English", ["", "i think ", "the "]), (Lang::Fr, "French", ["", "je pense ", "le "])] {
        let words = if lang == Lang::En { &en_words } else { &fr_words };
        for context in contexts {
            for n in [2usize, 3, 4] {
                let (mut a1, mut a3, mut changed, mut total) = (0, 0, 0, 0);
                let mut shown: Vec<String> = Vec::new();
                for w in words {
                    if w.len() <= n {
                        continue;
                    }
                    total += 1;
                    let typed = format!("{context}{}", &w[..n]);
                    let on = engine.suggest_in(lang, &typed, 3);
                    let off = plain.suggest_in(lang, &typed, 3);
                    if on.first().is_some_and(|s| s.lang == Lang::Dz) {
                        a1 += 1;
                    }
                    if on.iter().any(|s| s.lang == Lang::Dz) {
                        a3 += 1;
                        if misses > 0 && n == 3 && context.is_empty() && shown.len() < misses {
                            shown.push(format!("{typed} -> {:?}", on.iter().map(|s| s.text.as_str()).collect::<Vec<_>>()));
                        }
                    }
                    if on.iter().map(|s| &s.text).ne(off.iter().map(|s| &s.text)) {
                        changed += 1;
                    }
                }
                let ctx = if context.is_empty() { "none" } else { context.trim() };
                println!("| {name} | {ctx} | {n} | {:.2}% | {:.2}% | {:.2}% |", pct(a1, total), pct(a3, total), pct(changed, total));
                for s in shown {
                    println!("    shown: {s}");
                }
            }
        }
    }
    // the spelling correction with Dz on, against the same engine without Dz
    println!("\n### Spelling correction: typos of the 3000 most frequent words (5+ letters), Dz on against no Dz\n");
    println!("| language | typo | recovered (top-1) with Dz | recovered without Dz |");
    println!("|---|---|---|---|");
    for (lang, name, words) in [(Lang::En, "English", &en_words), (Lang::Fr, "French", &fr_words)] {
        let long: Vec<&String> = words.iter().filter(|w| w.len() >= 5).collect();
        for (kind, typo) in [("two letters swapped", 0usize), ("a letter missing", 1), ("a letter doubled", 2)] {
            let (mut with, mut without, mut n) = (0, 0, 0);
            for w in &long {
                let c: Vec<char> = w.chars().collect();
                let t: String = match typo {
                    0 => {
                        let mut v = c.clone();
                        v.swap(2, 3);
                        v.into_iter().collect()
                    }
                    1 => c.iter().enumerate().filter(|(i, _)| *i != 2).map(|(_, ch)| *ch).collect(),
                    _ => {
                        let mut v = c.clone();
                        v.insert(3, c[3]);
                        v.into_iter().collect()
                    }
                };
                if t == **w || plain.list(lang).is_some_and(|l| l.iter_words().any(|(x, _)| x == t)) {
                    continue; // the typo is a word of the language
                }
                n += 1;
                let top = |e: &LocalEngine| e.correct(Some(lang), &format!("{t} "), 3).and_then(|f| f.candidates.first().map(|s| s.text.clone()));
                with += usize::from(top(&engine).as_deref() == Some(w.as_str()));
                without += usize::from(top(&plain).as_deref() == Some(w.as_str()));
            }
            println!("| {name} | {kind} ({n} typos) | {:.1}% | {:.1}% |", pct(with, n), pct(without, n));
        }
        let (mut a, mut b, mut n) = (0, 0, 0);
        for w in &long {
            n += 1;
            a += usize::from(engine.correct(Some(lang), &format!("{w} "), 3).is_some());
            b += usize::from(plain.correct(Some(lang), &format!("{w} "), 3).is_some());
        }
        println!("| {name} | correct words wrongly fixed ({n} words) | {:.2}% | {:.2}% |", pct(a, n), pct(b, n));
    }

    // with Dz loaded but switched off, every call must equal an engine that never heard of Dz
    {
        let mut off = LocalEngine::load_dir(&root.join("data/words")).unwrap();
        off.add_dz_seed();
        off.set_enabled(Lang::Dz, false);
        let mut same = true;
        for w in en_words.iter().chain(fr_words.iter()).take(2000) {
            let t = &w[..3];
            same &= off.suggest(t, 3) == plain.suggest(t, 3) && off.correct(None, &format!("{w}x "), 3) == plain.correct(None, &format!("{w}x "), 3);
        }
        println!("
Dz loaded but switched off: identical to an engine without Dz on 2000 prefixes and typos: {same}");
    }

    // ---------------------------------------------------------------- 3. real text
    let dir = root.join("data/dz/narabizi");
    println!("\n## 3. Real Algerian Arabizi (NArabizi treebank, CC BY-SA 4.0, newspaper forum comments and song lyrics)\n");
    let sets = [("dev (2 small files, used while writing the seed)", vec!["qaf_arabizi-ud-dev.conllu", "qaf_arabizi-ud-test.conllu"]), ("sealed (the large file, never read while writing the seed)", vec!["qaf_arabizi-ud-train.conllu"])];
    for (title, files) in sets {
        let mut sentences: Vec<Vec<Tok>> = Vec::new();
        for f in &files {
            if let Ok(t) = std::fs::read_to_string(dir.join(f)) {
                sentences.extend(read_conllu(&t));
            }
        }
        if sentences.is_empty() {
            println!("(no NArabizi files in {})", dir.display());
            break;
        }
        report_real(&engine, title, &sentences, misses);
    }

    // The optional list (scripts/fetch-dz.ps1) merged in: built here from the SEALED file only, then tried on the dev files, so the
    // numbers are not measured on the text the extra list was made from.
    if let Ok(t) = std::fs::read_to_string(dir.join("qaf_arabizi-ud-train.conllu")) {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for s in read_conllu(&t) {
            for tok in s.iter().filter(|t| t.darija && !t.proper) {
                *counts.entry(tok.form.clone()).or_default() += 1;
            }
        }
        let text: String = counts.iter().filter(|(_, n)| **n >= 2).map(|(w, n)| format!("{w} {n}\n")).collect();
        let mut list = DzList::seed();
        let extra = DzList::parse(&text);
        list.merge_extra(&extra);
        let mut with_extra = LocalEngine::load_dir(&root.join("data/words")).unwrap();
        println!("### With the optional list (built from the sealed file, {} words) merged in, measured on the dev files\n", extra.len());
        with_extra.set_dz(list);
        with_extra.set_enabled(Lang::Dz, true);
        let mut sentences: Vec<Vec<Tok>> = Vec::new();
        for f in ["qaf_arabizi-ud-dev.conllu", "qaf_arabizi-ud-test.conllu"] {
            if let Ok(t) = std::fs::read_to_string(dir.join(f)) {
                sentences.extend(read_conllu(&t));
            }
        }
        report_real(&with_extra, "dev, seed + optional list", &sentences, 0);
    }
}

struct Tok {
    form: String,
    darija: bool,
    proper: bool,
}

fn read_conllu(text: &str) -> Vec<Vec<Tok>> {
    let mut out = Vec::new();
    let mut cur: Vec<Tok> = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else if !line.starts_with('#') {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 10 || f[0].contains('-') {
                continue;
            }
            cur.push(Tok { form: f[1].to_lowercase(), darija: f[9].contains("LangO=ar_dz"), proper: f[3] == "PROPN" });
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn report_real(engine: &LocalEngine, title: &str, sentences: &[Vec<Tok>], misses: usize) {
    let valid = |w: &str| w.len() >= 2 && w.chars().all(|c| c.is_ascii_lowercase() || "2356789".contains(c)) && w.chars().any(|c| c.is_ascii_lowercase());
    let toks: Vec<(&Tok, String)> = sentences
        .iter()
        .flat_map(|s| {
            s.iter().enumerate().map(move |(i, t)| (t, s[..i].iter().map(|x| x.form.as_str()).collect::<Vec<_>>().join(" ")))
        })
        .filter(|(t, _)| t.darija && !t.proper && valid(&t.form))
        .collect();
    let distinct: HashSet<&str> = toks.iter().map(|(t, _)| t.form.as_str()).collect();
    let known = toks.iter().filter(|(t, _)| engine.dz_knows(&t.form)).count();
    let list = engine.dz_list().unwrap();
    let exact = toks.iter().filter(|(t, _)| list.contains(&t.form)).count();
    println!("### {title}\n");
    println!("{} sentences, {} Darija word tokens ({} distinct; names left out).\n", sentences.len(), toks.len(), distinct.len());
    println!("- the Darija list knows the token (listed, or another spelling of a listed word): **{:.1}%** of tokens (exactly as spelled: {:.1}%)", pct(known, toks.len()), pct(exact, toks.len()));
    let mut unknown: HashMap<&str, usize> = HashMap::new();
    for (t, _) in &toks {
        if !engine.dz_knows(&t.form) {
            *unknown.entry(t.form.as_str()).or_default() += 1;
        }
    }
    println!("\n| prefix typed | tokens | exact word in top-3 | right word (any spelling) in top-3 | same, only tokens the list knows | any Darija suggestion shown |");
    println!("|---|---|---|---|---|---|");
    for n in [2usize, 3] {
        let (mut total, mut exact3, mut sound3, mut any, mut kn, mut kn_hit) = (0, 0, 0, 0, 0, 0);
        for (t, before) in &toks {
            if t.form.chars().count() <= n + 1 {
                continue;
            }
            total += 1;
            let prefix: String = t.form.chars().take(n).collect();
            let typed = if before.is_empty() { prefix } else { format!("{before} {prefix}") };
            let got = engine.suggest(&typed, 3);
            if got.iter().any(|s| s.text == t.form) {
                exact3 += 1;
            }
            let right = got.iter().any(|s| dz_sound_key(&s.text) == dz_sound_key(&t.form));
            sound3 += usize::from(right);
            if engine.dz_knows(&t.form) {
                kn += 1;
                kn_hit += usize::from(right);
            }
            if got.iter().any(|s| s.lang == Lang::Dz) {
                any += 1;
            }
        }
        println!("| {n} | {total} | {:.1}% | {:.1}% | {:.1}% ({kn} tokens) | {:.1}% |", pct(exact3, total), pct(sound3, total), pct(kn_hit, kn), pct(any, total));
    }
    {
        let started = std::time::Instant::now();
        let mut calls = 0;
        for (t, before) in toks.iter().take(1500) {
            let prefix: String = t.form.chars().take(3).collect();
            let _ = engine.suggest(&format!("{before} {prefix}"), 3);
            calls += 1;
        }
        println!("\nOne suggest() call with Dz on (release build): {:.0} microseconds on average over {calls} calls", started.elapsed().as_secs_f64() * 1e6 / calls as f64);
    }
    if misses > 0 {
        let mut u: Vec<(&str, usize)> = unknown.into_iter().collect();
        u.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        println!("\nmost frequent tokens the seed does not know: {:?}", u.iter().take(misses).collect::<Vec<_>>());
    }
    println!();
}

fn pct(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 }
}
