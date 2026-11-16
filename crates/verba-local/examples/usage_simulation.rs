//! Simulates people who write the same words more often than the dictionary says, and measures what remembering their usage gains.
//!
//!   cargo run --release -p verba-local --example usage_simulation -- [--lang en|fr|ar|all] [--tune] [--nextword]
//!
//! A synthetic user has a CORE of about 300 words (drawn from a band of the dictionary's frequency ranks, so the vocabulary is skewed away
//! from the dictionary) and writes a fraction of their words from it with a Zipf law; the rest are drawn by the dictionary's own frequency.
//! The text is cut in sentences of 12 words and given to `learn_text` one sentence at a time, as the app does. After 2 000 and after 10 000 words
//! learned, the same user's HELD-OUT text (new draws from the same habits, never given to `learn_text`) is typed prefix by prefix and the engine
//! is asked for 3 completions: top-1 / top-3 hit rate for prefixes of 2, 3 and 4 letters (the engine shows nothing for 1 letter), against the same
//! engine with learning off (empty counts).
//!
//! Users come in three kinds: `specialist` (core from ranks 300..15 000, 35% of the words), `mixed` (core from ranks 50..3 000, 20%) and `dictionary`
//! (no core: writes exactly as the dictionary says; learning must not make it worse). Dials are tuned on users with seeds 1..4 and the reported
//! numbers come from other users (seeds 101..106). `--tune` sweeps the dials and prints the tuning table; `--nextword` simulates habits of
//! word pairs instead.

use std::path::Path;

use verba_core::Lang;
use verba_local::{LocalEngine, UsageParams};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The words of a language that a user may write: 3 to 12 letters, no apostrophe, with their counts, most frequent first.
fn universe(e: &LocalEngine, lang: Lang) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = e
        .list(lang)
        .expect("list")
        .iter_words()
        .filter(|(w, _)| (3..=12).contains(&w.chars().count()) && !w.contains('\''))
        .map(|(w, c)| (w.to_string(), c))
        .collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Specialist,
    Mixed,
    Dictionary,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Specialist => "specialist",
            Kind::Mixed => "mixed",
            Kind::Dictionary => "dictionary",
        }
    }
}

struct User {
    core: Vec<usize>,
    core_cum: Vec<f64>,
    p_core: f64,
}

struct Draw<'a> {
    words: &'a [(String, u64)],
    cum: Vec<f64>,
}

impl<'a> Draw<'a> {
    fn new(words: &'a [(String, u64)]) -> Self {
        let mut acc = 0.0;
        let cum = words
            .iter()
            .map(|w| {
                acc += w.1 as f64;
                acc
            })
            .collect();
        Self { words, cum }
    }
    /// A word drawn by the dictionary's own frequency.
    fn general(&self, rng: &mut Rng) -> usize {
        let x = rng.unit() * self.cum.last().copied().unwrap_or(1.0);
        self.cum.partition_point(|c| *c < x).min(self.words.len() - 1)
    }
}

impl User {
    fn new(kind: Kind, seed: u64, n_words: usize) -> Self {
        let mut rng = Rng(seed.wrapping_mul(7919) + 13);
        let (lo, hi, p_core) = match kind {
            Kind::Specialist => (300, 15_000, 0.35),
            Kind::Mixed => (50, 3_000, 0.20),
            Kind::Dictionary => (0, 1, 0.0),
        };
        let hi = hi.min(n_words);
        let mut core: Vec<usize> = Vec::new();
        while core.len() < if kind == Kind::Dictionary { 0 } else { 300 } {
            let i = lo + (rng.next() as usize) % (hi - lo);
            if !core.contains(&i) {
                core.push(i);
            }
        }
        let mut acc = 0.0;
        let core_cum = (0..core.len())
            .map(|r| {
                acc += 1.0 / (r as f64 + 1.0);
                acc
            })
            .collect();
        Self { core, core_cum, p_core }
    }

    fn word(&self, draw: &Draw, rng: &mut Rng) -> usize {
        if !self.core.is_empty() && rng.unit() < self.p_core {
            let x = rng.unit() * self.core_cum.last().copied().unwrap_or(1.0);
            self.core[self.core_cum.partition_point(|c| *c < x).min(self.core.len() - 1)]
        } else {
            draw.general(rng)
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Hits {
    n: [usize; 3],
    top1: [usize; 3],
    top3: [usize; 3],
}

impl Hits {
    fn add(&mut self, o: &Hits) {
        for i in 0..3 {
            self.n[i] += o.n[i];
            self.top1[i] += o.top1[i];
            self.top3[i] += o.top3[i];
        }
    }
    fn pct(a: usize, n: usize) -> f64 {
        if n == 0 { 0.0 } else { 100.0 * a as f64 / n as f64 }
    }
    /// Mean over the prefix lengths 2, 3, 4.
    fn mean3(&self) -> f64 {
        (0..3).map(|i| Self::pct(self.top3[i], self.n[i])).sum::<f64>() / 3.0
    }
    fn mean1(&self) -> f64 {
        (0..3).map(|i| Self::pct(self.top1[i], self.n[i])).sum::<f64>() / 3.0
    }
}

/// Types the held-out words prefix by prefix (2, 3 and 4 letters) and counts the hits.
fn evaluate(e: &LocalEngine, lang: Lang, held: &[String]) -> Hits {
    let mut h = Hits::default();
    for w in held {
        let chars: Vec<char> = w.chars().collect();
        for (slot, len) in [2usize, 3, 4].into_iter().enumerate() {
            if chars.len() <= len {
                continue;
            }
            let prefix: String = chars[..len].iter().collect();
            let got = e.suggest_in(lang, &prefix, 3);
            h.n[slot] += 1;
            h.top1[slot] += usize::from(got.first().is_some_and(|s| s.text == *w));
            h.top3[slot] += usize::from(got.iter().any(|s| s.text == *w));
        }
    }
    h
}

struct Run {
    off: Hits,
    at2k: Hits,
    at10k: Hits,
}

fn run_user(e: &mut LocalEngine, lang: Lang, words: &[(String, u64)], kind: Kind, seed: u64, params: &UsageParams) -> Run {
    let draw = Draw::new(words);
    let user = User::new(kind, seed, words.len());
    let mut rng = Rng(seed * 1_000_003 + 17);
    let held: Vec<String> = (0..1500).map(|_| words[user.word(&draw, &mut rng)].0.clone()).collect();
    e.personal_mut().clear_learned();
    e.personal_mut().set_usage_params(params.clone());
    let off = evaluate(e, lang, &held);
    let learn = |e: &mut LocalEngine, n: usize, rng: &mut Rng| {
        for _ in 0..n / 12 {
            let s: Vec<&str> = (0..12).map(|_| words[user.word(&draw, rng)].0.as_str()).collect();
            e.learn_text(&format!("{}.", s.join(" ")));
        }
    };
    learn(e, 2000, &mut rng);
    let at2k = evaluate(e, lang, &held);
    learn(e, 8000, &mut rng);
    let at10k = evaluate(e, lang, &held);
    Run { off, at2k, at10k }
}

fn lang_of(code: &str) -> Lang {
    match code {
        "fr" => Lang::Fr,
        "ar" => Lang::Ar,
        _ => Lang::En,
    }
}

fn row(name: &str, runs: &[Run]) {
    let (mut off, mut a, mut b) = (Hits::default(), Hits::default(), Hits::default());
    for r in runs {
        off.add(&r.off);
        a.add(&r.at2k);
        b.add(&r.at10k);
    }
    let by_len = |h: &Hits, o: &Hits| -> String {
        (0..3).map(|i| format!("{:+5.1}", Hits::pct(h.top3[i], h.n[i]) - Hits::pct(o.top3[i], o.n[i]))).collect::<Vec<_>>().join(" ")
    };
    println!(
        "{name:<22} top-3 off {:5.1}% | 2k words {:5.1}% ({:+5.1}) | 10k words {:5.1}% ({:+5.1}) [gain by prefix 2/3/4: {}] | top-1 off {:5.1}% -> 10k {:5.1}% ({:+5.1})",
        off.mean3(),
        a.mean3(),
        a.mean3() - off.mean3(),
        b.mean3(),
        b.mean3() - off.mean3(),
        by_len(&b, &off),
        off.mean1(),
        b.mean1(),
        b.mean1() - off.mean1()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let value = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let langs: Vec<&str> = match value("--lang").as_deref() {
        Some("fr") => vec!["fr"],
        Some("ar") => vec!["ar"],
        Some("en") => vec!["en"],
        _ => vec!["en", "fr", "ar"],
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let mut e = LocalEngine::load_dir(&dir).expect("run scripts/fetch-data.ps1 first");
    if flag("--drift") {
        drift(&mut e, &langs);
        return;
    }
    if flag("--nextword") {
        nextword(&mut e, &langs);
        return;
    }
    let kinds = [Kind::Specialist, Kind::Mixed, Kind::Dictionary];

    if flag("--tune") {
        let base = UsageParams::default();
        let mut grid: Vec<(String, UsageParams)> = vec![("learning off (reference)".into(), UsageParams { max_boost: 1.0, ..base.clone() })];
        for min_z in [0.0, 2.0, 3.0, 4.0, 6.0] {
            for k in [2.4, 3.0] {
                grid.push((format!("z>={min_z} k={k}"), UsageParams { min_z, k, ..base.clone() }));
            }
        }
        for code in &langs {
            let lang = lang_of(code);
            let words = universe(&e, lang);
            println!("\n=== tuning, {code}: users with seeds 1..4; mean top-3 over prefixes 2-4, change against learning off, after 10k words ===");
            for (name, p) in &grid {
                let mut line = format!("{name:<26}");
                let mut score = 0.0;
                for kind in kinds {
                    let runs: Vec<Run> = (1..=4).map(|s| run_user(&mut e, lang, &words, kind, s, p)).collect();
                    let (mut off, mut b) = (Hits::default(), Hits::default());
                    for r in &runs {
                        off.add(&r.off);
                        b.add(&r.at10k);
                    }
                    let d = b.mean3() - off.mean3();
                    line.push_str(&format!(" {}:{:+5.2}", &kind.name()[..4], d));
                    score += if kind == Kind::Dictionary { 3.0 * d.min(0.0) } else { d };
                }
                println!("{line}   score {score:+.2}");
            }
        }
        return;
    }

    let params = UsageParams::default();
    println!("dials: {params:?}");
    for code in &langs {
        let lang = lang_of(code);
        let words = universe(&e, lang);
        println!("\n=== {code}: {} words in the universe; users with seeds 101..106 (not used for tuning) ===", words.len());
        for kind in kinds {
            let runs: Vec<Run> = (101..=106).map(|s| run_user(&mut e, lang, &words, kind, s, &params)).collect();
            row(kind.name(), &runs);
        }
    }
}

// ───────────── next-word habits ─────────────

fn nextword(e: &mut LocalEngine, langs: &[&str]) {
    for code in langs {
        let lang = lang_of(code);
        let words = universe(e, lang);
        let draw = Draw::new(&words);
        let table_text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../data/words/{code}.bigrams.txt"))).unwrap_or_default();
        // the rows of the bundled table, as text, to draw followers by their counts
        let mut rows: std::collections::HashMap<String, Vec<(String, u32)>> = std::collections::HashMap::new();
        for line in table_text.lines().filter(|l| !l.starts_with('#')) {
            let mut f = line.split('\t');
            let (Some(k), Some(_), Some(list)) = (f.next(), f.next(), f.next()) else { continue };
            if k.contains(' ') {
                continue;
            }
            let v: Vec<(String, u32)> = list.split_whitespace().filter_map(|i| i.rsplit_once(':').map(|(w, c)| (w.to_string(), c.parse().unwrap_or(1)))).collect();
            rows.insert(k.to_string(), v);
        }
        println!("\n=== next word, {code}: users with seeds 101..106; top-3 hit rate of `next_words`, learning off vs after 10k words ===");
        for (name, habit) in [("habits (45% own pairs)", 0.45), ("dictionary-like (0%)", 0.0)] {
            for pair_min in [1u32, 2, 3, 4] {
                let (mut off, mut on, mut off_core, mut on_core) = ([0usize; 2], [0usize; 2], [0usize; 2], [0usize; 2]);
                for seed in 101..=106u64 {
                    let mut rng = Rng(seed * 31 + 5);
                    // 300 core words that have a row in the table, each with 2 favourite followers of the user's own
                    let mut core: Vec<(String, [usize; 2])> = Vec::new();
                    let keys: Vec<&String> = rows.keys().filter(|k| !k.contains(' ') && k.chars().count() >= 3).collect();
                    let mut sorted: Vec<&&String> = keys.iter().collect();
                    sorted.sort();
                    while core.len() < 300 && !sorted.is_empty() {
                        let k = sorted[(rng.next() as usize) % sorted.len()].to_string();
                        if core.iter().all(|c| c.0 != k) {
                            let f = [100 + (rng.next() as usize) % 4000, 100 + (rng.next() as usize) % 4000];
                            core.push((k, f));
                        }
                    }
                    let follow = |prev: &str, rng: &mut Rng| -> String {
                        if let Some((_, fav)) = core.iter().find(|c| c.0 == prev) {
                            if rng.unit() < habit {
                                return words[if rng.unit() < 0.6 { fav[0] } else { fav[1] }].0.clone();
                            }
                        }
                        if let Some(r) = rows.get(prev) {
                            if rng.unit() < 0.85 {
                                let total: u32 = r.iter().map(|x| x.1).sum();
                                let mut x = (rng.unit() * total as f64) as u32;
                                for (w, c) in r {
                                    if x < *c {
                                        return w.clone();
                                    }
                                    x -= c;
                                }
                            }
                        }
                        words[draw.general(rng)].0.clone()
                    };
                    let sentence = |rng: &mut Rng, n: usize| -> Vec<String> {
                        // sentences start from a core word half of the time, so that habits are exercised
                        let mut s = vec![if rng.unit() < 0.5 { core[(rng.next() as usize) % core.len()].0.clone() } else { words[draw.general(rng)].0.clone() }];
                        while s.len() < n {
                            let next = follow(s.last().unwrap(), rng);
                            s.push(next);
                        }
                        s
                    };
                    let held: Vec<Vec<String>> = (0..250).map(|_| sentence(&mut rng, 8)).collect();
                    let score = |e: &LocalEngine, acc: &mut [usize; 2], acc_core: &mut [usize; 2]| {
                        for s in &held {
                            for i in 1..s.len() {
                                let ctx = format!("{} ", s[i.saturating_sub(2)..i].join(" "));
                                let got = e.next_words(Some(lang), &ctx, 3);
                                let hit = got.iter().any(|x| x.text == s[i]);
                                acc[0] += 1;
                                acc[1] += usize::from(hit);
                                if core.iter().any(|c| c.0 == s[i - 1]) {
                                    acc_core[0] += 1;
                                    acc_core[1] += usize::from(hit);
                                }
                            }
                        }
                    };
                    e.personal_mut().clear_learned();
                    e.personal_mut().set_usage_params(UsageParams { pair_min, ..UsageParams::default() });
                    score(e, &mut off, &mut off_core);
                    for _ in 0..(10_000 / 12) {
                        let s = sentence(&mut rng, 12);
                        e.learn_text(&format!("{}.", s.join(" ")));
                    }
                    score(e, &mut on, &mut on_core);
                }
                let p = |a: [usize; 2]| 100.0 * a[1] as f64 / a[0].max(1) as f64;
                println!(
                    "{name:<24} pair used at {pair_min}: all positions {:5.1}% -> {:5.1}% ({:+5.1}) | after a core word {:5.1}% -> {:5.1}% ({:+5.1})",
                    p(off),
                    p(on),
                    p(on) - p(off),
                    p(off_core),
                    p(on_core),
                    p(on_core) - p(off_core)
                );
            }
        }
    }
}

// ───────────── habits that change ─────────────

/// A user who wrote 10 000 words with one core vocabulary, then 4 000 with another (a new job, a new project): the top-3 rate on the new
/// habits, for several half-lives of the counts.
fn drift(e: &mut LocalEngine, langs: &[&str]) {
    for code in langs {
        let lang = lang_of(code);
        let words = universe(e, lang);
        let draw = Draw::new(&words);
        println!("\n=== {code}: 10k words of a first vocabulary, then 4k of a second one; top-3 on the second (mean of prefixes 2-4, 4 users), by half-life ===");
        for half_life in [100.0, 400.0, 1000.0, 4000.0, 1.0e9] {
            let (mut off, mut on) = (Hits::default(), Hits::default());
            let mut stationary = (Hits::default(), Hits::default());
            for seed in 201..=204u64 {
                let a = User::new(Kind::Specialist, seed, words.len());
                let b = User::new(Kind::Specialist, seed + 500, words.len());
                let mut rng = Rng(seed * 77 + 3);
                let held: Vec<String> = (0..1500).map(|_| words[b.word(&draw, &mut rng)].0.clone()).collect();
                let held_a: Vec<String> = (0..1500).map(|_| words[a.word(&draw, &mut rng)].0.clone()).collect();
                e.personal_mut().clear_learned();
                e.personal_mut().set_usage_params(UsageParams { half_life, ..UsageParams::default() });
                off.add(&evaluate(e, lang, &held));
                stationary.0.add(&evaluate(e, lang, &held_a));
                for (user, n) in [(&a, 10_000), (&b, 4_000)] {
                    for _ in 0..n / 12 {
                        let s: Vec<&str> = (0..12).map(|_| words[user.word(&draw, &mut rng)].0.as_str()).collect();
                        e.learn_text(&format!("{}.", s.join(" ")));
                    }
                    if n == 10_000 {
                        stationary.1.add(&evaluate(e, lang, &held_a));
                    }
                }
                on.add(&evaluate(e, lang, &held));
            }
            println!(
                "half-life {half_life:>12.0} sentences: new habits {:5.1}% -> {:5.1}% ({:+5.1}) | (the first vocabulary, right after its 10k words: {:5.1}% -> {:5.1}%)",
                off.mean3(),
                on.mean3(),
                on.mean3() - off.mean3(),
                stationary.0.mean3(),
                stationary.1.mean3()
            );
        }
    }
}
