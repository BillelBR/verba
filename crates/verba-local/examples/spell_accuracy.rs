//! Measures the spelling correction (`LocalEngine::correct`), per language.
//!
//!   cargo run --release -p verba-local --example spell_accuracy              (the report)
//!   cargo run --release -p verba-local --example spell_accuracy -- --sweep   (the dials, to choose them)
//!
//! Recovery: frequent words (ranks 200 to 3000, 5+ letters) get one typo of each kind and the engine is asked to fix it,
//! like the app does (the typo and a space before the caret). A typo that is itself a word of the list is skipped
//! (nothing could tell). Top-1 / top-3 = the right word is the first / among the first three candidates;
//! "nothing" = no correction offered at all.
//!
//! False alarms: an engine is built from the top 30000 words of a list, and asked about the words ranked 30000 to 50000.
//! They are real words (names, rare words, inflections) that this dictionary lacks, so a good engine says nothing.
//!
//! The typos are made with a fixed seed: the numbers are the same on every run.

use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use verba_core::Lang;
use verba_local::{LocalEngine, SpellParams, WordList};

const ACCENTED: &str = "àâäçéèêëîïôöùûüÿœæ";
const QWERTY: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
const AZERTY: [&str; 3] = ["azertyuiop", "qsdfghjklm", "wxcvbn"];
const ARABIC_KEYS: [&str; 3] = ["ضصثقفغعهخحجد", "شسيبلاتنمكط", "ءورلايزظ"];
const SHIFT: [f32; 3] = [0.0, 0.25, 0.75];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn rows(lang: Lang) -> &'static [&'static str; 3] {
    match lang {
        Lang::En | Lang::Dz => &QWERTY,
        Lang::Fr => &AZERTY,
        Lang::Ar => &ARABIC_KEYS,
    }
}

fn near(lang: Lang, c: char) -> Vec<char> {
    let rows = rows(lang);
    let pos = |ch: char| {
        rows.iter().enumerate().find_map(|(r, row)| row.chars().position(|k| k == ch).map(|i| (i as f32 + SHIFT[r], r as f32)))
    };
    let Some((x, y)) = pos(c) else { return Vec::new() };
    rows.iter()
        .flat_map(|row| row.chars())
        .filter(|k| *k != c)
        .filter(|k| {
            pos(*k).is_some_and(|(kx, ky)| {
                let (dx, dy) = ((kx - x).abs(), (ky - y).abs());
                (dy == 0.0 && dx <= 1.01) || (dy == 1.0 && dx <= 0.76)
            })
        })
        .collect()
}

/// Arabic comparison key: what the engine treats as the same letter.
fn afold(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(*c as u32, 0x064B..=0x065F | 0x0670 | 0x0640))
        .map(|c| match c {
            'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
            'ة' => 'ه',
            'ى' | 'ئ' => 'ي',
            'ؤ' => 'و',
            c => c,
        })
        .collect()
}

fn is_core_arabic(c: char) -> bool {
    matches!(c as u32, 0x0621..=0x063A | 0x0641..=0x064A)
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

#[derive(Clone)]
struct Case {
    lang: Lang,
    kind: &'static str,
    typo: String,
    target: String,
}

fn load(dir: &Path, lang: Lang) -> String {
    std::fs::read_to_string(dir.join(format!("{}.txt", lang.code()))).unwrap_or_default()
}

/// Words of the engine's list by frequency.
fn ranked(engine: &LocalEngine, lang: Lang) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = engine.list(lang).map(|l| l.iter_words().map(|(w, c)| (w.to_string(), c)).collect()).unwrap_or_default();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

fn pure(lang: Lang, w: &str) -> bool {
    if lang == Lang::Ar { w.chars().all(is_core_arabic) } else { w.chars().all(|c| c.is_ascii_lowercase() || ACCENTED.contains(c)) }
}

fn make_cases(engine: &LocalEngine, lang: Lang, rng: &mut Rng, skip: usize, take: usize) -> (Vec<Case>, usize) {
    let words = ranked(engine, lang);
    let known: HashSet<String> = words.iter().map(|(w, _)| if lang == Lang::Ar { afold(w) } else { w.clone() }).collect();
    let all_latin: HashSet<String> =
        [Lang::En, Lang::Fr].iter().flat_map(|l| ranked(engine, *l).into_iter().map(|(w, _)| w)).collect();
    let letters: Vec<char> = if lang == Lang::Ar {
        "ابتثجحخدذرزسشصضطظعغفقكلمنهوي".chars().collect()
    } else {
        "abcdefghijklmnopqrstuvwxyz".chars().collect()
    };
    let mut cases = Vec::new();
    let mut collisions = 0;
    for (word, _) in words.iter().skip(skip).take(take) {
        let chars: Vec<char> = word.chars().collect();
        if chars.len() < 5 || !pure(lang, word) {
            continue;
        }
        let n = chars.len();
        let mut push = |kind: &'static str, typo: String, cases: &mut Vec<Case>| {
            let blocked = if lang == Lang::Ar { afold(&typo) == afold(word) || known.contains(&afold(&typo)) } else { typo == *word || all_latin.contains(&typo) };
            if blocked {
                collisions += 1;
            } else {
                cases.push(Case { lang, kind, typo, target: word.clone() });
            }
        };
        // transposition
        let i = rng.below(n - 1);
        if chars[i] != chars[i + 1] {
            let mut v = chars.clone();
            v.swap(i, i + 1);
            push("transposition", v.into_iter().collect(), &mut cases);
        }
        // a letter missing
        let i = rng.below(n);
        push("letter missing", chars.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, c)| *c).collect(), &mut cases);
        // a letter too many
        let i = rng.below(n + 1);
        let mut v = chars.clone();
        v.insert(i, letters[rng.below(letters.len())]);
        push("extra letter", v.into_iter().collect(), &mut cases);
        // a neighbouring key
        let i = rng.below(n);
        let nb = near(lang, chars[i]);
        if !nb.is_empty() {
            let mut v = chars.clone();
            v[i] = nb[rng.below(nb.len())];
            push("neighbour key", v.into_iter().collect(), &mut cases);
        }
        // any other letter
        let i = rng.below(n);
        let mut v = chars.clone();
        let c = letters[rng.below(letters.len())];
        if c != v[i] {
            v[i] = c;
            push("other letter", v.into_iter().collect(), &mut cases);
        }
        // two slips in one word (a swap and a missing letter, two neighbouring keys...)
        let mut v = chars.clone();
        for _ in 0..2 {
            match rng.below(4) {
                0 if v.len() > 3 => {
                    let i = rng.below(v.len() - 1);
                    v.swap(i, i + 1);
                }
                1 if v.len() > 4 => {
                    let i = rng.below(v.len());
                    v.remove(i);
                }
                2 => {
                    let i = rng.below(v.len() + 1);
                    v.insert(i, letters[rng.below(letters.len())]);
                }
                _ => {
                    let i = rng.below(v.len());
                    let nb = near(lang, v[i]);
                    if !nb.is_empty() {
                        v[i] = nb[rng.below(nb.len())];
                    }
                }
            }
        }
        push("two slips", v.into_iter().collect(), &mut cases);
        // accents left out (French)
        if lang == Lang::Fr {
            let plain = sloppy_french(word);
            if plain != *word {
                push("accent left out", plain, &mut cases);
            }
        }
    }
    (cases, collisions)
}

#[derive(Default, Clone)]
struct Tally {
    n: usize,
    top1: usize,
    top3: usize,
    nothing: usize,
}

impl Tally {
    fn pct(&self, x: usize) -> f64 {
        if self.n == 0 { 0.0 } else { 100.0 * x as f64 / self.n as f64 }
    }
    fn add(&mut self, o: &Tally) {
        self.n += o.n;
        self.top1 += o.top1;
        self.top3 += o.top3;
        self.nothing += o.nothing;
    }
}

fn run_cases(engine: &LocalEngine, params: &SpellParams, only_lang: bool, cases: &[Case]) -> Vec<(Lang, &'static str, Tally)> {
    let mut out: Vec<(Lang, &'static str, Tally)> = Vec::new();
    for c in cases {
        let slot = match out.iter().position(|(l, k, _)| *l == c.lang && *k == c.kind) {
            Some(i) => i,
            None => {
                out.push((c.lang, c.kind, Tally::default()));
                out.len() - 1
            }
        };
        let t = &mut out[slot].2;
        t.n += 1;
        let fix = engine.correct_with(params, only_lang.then_some(c.lang), &format!("{} ", c.typo), 3);
        match fix {
            None => t.nothing += 1,
            Some(f) => {
                let same = |s: &str| if c.lang == Lang::Ar { afold(s) == afold(&c.target) } else { s.to_lowercase() == c.target };
                match f.candidates.iter().position(|s| same(&s.text)) {
                    Some(0) => {
                        t.top1 += 1;
                        t.top3 += 1;
                    }
                    Some(_) => t.top3 += 1,
                    None => {}
                }
            }
        }
    }
    out
}

/// A single-language engine from the top `n` words of a list, and the words of the top `m` that are not in it.
fn tail_engine(text: &str, lang: Lang, small: usize, big: usize) -> (LocalEngine, Vec<String>, Vec<String>) {
    let small_list = WordList::parse_for(lang, text, small);
    let big_list = WordList::parse_for(lang, text, big);
    let in_small: HashSet<&str> = small_list.iter_words().map(|(w, _)| w).collect();
    let mut tail: Vec<(String, u64)> = big_list.iter_words().filter(|(w, _)| !in_small.contains(w)).map(|(w, c)| (w.to_string(), c)).collect();
    tail.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let tail: Vec<String> = tail.into_iter().map(|(w, _)| w).filter(|w| w.chars().count() >= 3 && pure(lang, w)).collect();
    let mut head: Vec<(String, u64)> = small_list.iter_words().map(|(w, c)| (w.to_string(), c)).collect();
    head.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let head: Vec<String> = head.into_iter().skip(200).take(4800).map(|(w, _)| w).collect();
    let mut engine = LocalEngine::new();
    engine.add(lang, small_list);
    (engine, tail, head)
}

/// Words made from listed ones by attaching an ending (Latin: the plural -s, English -ed and -ing) or the Arabic particles and
/// endings. Many are real words that the dictionary lacks; some are not words. Only forms absent from the list are returned.
fn made_forms(lang: Lang, head: &[String], present: &HashSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for w in head.iter().filter(|w| w.chars().count() >= 3) {
        let forms: Vec<String> = match lang {
            Lang::Ar => {
                let mut f: Vec<String> = ["و", "ب", "ل", "ك", "ف", "وال", "بال"].iter().map(|p| format!("{p}{w}")).collect();
                if !w.starts_with("ال") {
                    f.push(format!("ال{w}"));
                }
                f.extend(["ها", "هم", "ات", "ون", "ين", "ك", "نا", "ي"].iter().map(|s| format!("{w}{s}")));
                f
            }
            Lang::En => vec![format!("{w}s"), format!("{w}ed"), format!("{w}ing")],
            Lang::Fr => vec![format!("{w}s"), format!("{w}e"), format!("{w}es")],
            Lang::Dz => Vec::new(),
        };
        out.extend(forms.into_iter().filter(|f| !present.contains(&if lang == Lang::Ar { afold(f) } else { f.clone() })));
    }
    out
}

fn flagged(engine: &LocalEngine, params: &SpellParams, lang: Lang, words: &[String]) -> usize {
    words.iter().filter(|w| engine.correct_with(params, Some(lang), &format!("{w} "), 3).is_some()).count()
}

struct Data {
    full: LocalEngine,
    cases: Vec<Case>,
    /// The same typos of rarer words (ranks 3000 to 12000): do fixes still work where the dictionary thins out?
    rare_cases: Vec<Case>,
    collisions: Vec<(Lang, usize)>,
    tails: Vec<(Lang, LocalEngine, Vec<String>, Vec<String>, Vec<String>)>,
    /// Words that ARE in the dictionary (ranks 2000 to 30000), by language: mostly valid, some are slips the lists carry (`wich`).
    known: Vec<(Lang, Vec<String>)>,
    /// How long the very first `correct` call of each language takes (it builds the lookup tables).
    first_call: Vec<(Lang, std::time::Duration)>,
}

fn prepare(dir: &Path) -> Data {
    let full = LocalEngine::load_dir(dir).expect("word lists");
    let first_call = [Lang::En, Lang::Fr, Lang::Ar]
        .iter()
        .map(|l| {
            let t = Instant::now();
            let _ = full.correct(Some(*l), if *l == Lang::Ar { "مرجبا " } else { "teh " }, 3);
            (*l, t.elapsed())
        })
        .collect();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut cases = Vec::new();
    let mut rare_cases = Vec::new();
    let mut collisions = Vec::new();
    for lang in [Lang::En, Lang::Fr, Lang::Ar] {
        let (c, col) = make_cases(&full, lang, &mut rng, 200, 2800);
        cases.extend(c);
        collisions.push((lang, col));
        let (c, _) = make_cases(&full, lang, &mut rng, 3000, 9000);
        rare_cases.extend(c);
    }
    let mut tails = Vec::new();
    for lang in [Lang::En, Lang::Fr, Lang::Ar] {
        let text = load(dir, lang);
        let (engine, tail, head) = tail_engine(&text, lang, 30_000, 50_000);
        let present: HashSet<String> = engine.list(lang).unwrap().iter_words().map(|(w, _)| if lang == Lang::Ar { afold(w) } else { w.to_string() }).collect();
        let made = made_forms(lang, &head, &present);
        tails.push((lang, engine, tail, head, made));
    }
    let known = [Lang::En, Lang::Fr, Lang::Ar]
        .iter()
        .map(|l| (*l, ranked(&full, *l).into_iter().skip(2000).take(28_000).map(|(w, _)| w).filter(|w| w.chars().count() >= 3 && pure(*l, w)).collect()))
        .collect();
    Data { full, cases, rare_cases, collisions, tails, known, first_call }
}

fn name(l: Lang) -> &'static str {
    match l {
        Lang::En => "English",
        Lang::Fr => "French",
        Lang::Ar => "Arabic",
        Lang::Dz => "Darija",
    }
}

fn report(d: &Data, params: &SpellParams) {
    for (only, title) in [(false, "all three languages on (the keyboard is not followed)"), (true, "one language (the keyboard says which)")] {
        println!("\n## Recovery of typos, {title}\n");
        println!("| Language | Typo | typos | top-1 | top-3 | nothing offered |");
        println!("|---|---|---|---|---|---|");
        let rows = run_cases(&d.full, params, only, &d.cases);
        for lang in [Lang::En, Lang::Fr, Lang::Ar] {
            let mut sum = Tally::default();
            for (l, kind, t) in rows.iter().filter(|r| r.0 == lang) {
                println!("| {} | {} | {} | {:.1}% | {:.1}% | {:.1}% |", name(*l), kind, t.n, t.pct(t.top1), t.pct(t.top3), t.pct(t.nothing));
                sum.add(t);
            }
            println!("| **{}** | **all** | {} | **{:.1}%** | **{:.1}%** | {:.1}% |", name(lang), sum.n, sum.pct(sum.top1), sum.pct(sum.top3), sum.pct(sum.nothing));
        }
    }
    println!("\n## Recovery of typos in rarer words (ranks 3000 to 12000), one language\n");
    println!("| Language | typos | top-1 | top-3 | nothing offered |");
    println!("|---|---|---|---|---|");
    let rows = run_cases(&d.full, params, true, &d.rare_cases);
    for lang in [Lang::En, Lang::Fr, Lang::Ar] {
        let mut sum = Tally::default();
        rows.iter().filter(|r| r.0 == lang).for_each(|r| sum.add(&r.2));
        println!("| {} | {} | {:.1}% | {:.1}% | {:.1}% |", name(lang), sum.n, sum.pct(sum.top1), sum.pct(sum.top3), sum.pct(sum.nothing));
    }
    println!("\n(typos skipped because they are words of a list: {})", d.collisions.iter().map(|(l, n)| format!("{} {}", name(*l), n)).collect::<Vec<_>>().join(", "));

    println!("\n## False alarms (valid words the dictionary lacks; lower is better)\n");
    println!("| Language | tested | flagged | share |");
    println!("|---|---|---|---|");
    for (lang, engine, tail, _, made) in &d.tails {
        let f = flagged(engine, params, *lang, tail);
        println!("| {}: words ranked 30000 to 50000 | {} | {} | **{:.1}%** |", name(*lang), tail.len(), f, 100.0 * f as f64 / tail.len().max(1) as f64);
        let f = flagged(engine, params, *lang, made);
        let what = if *lang == Lang::Ar { "particles and endings added to listed words" } else { "-s / -ed / -ing / -e / -es added to listed words (some are not words)" };
        println!("| {}: {} | {} | {} | {:.1}% |", name(*lang), what, made.len(), f, 100.0 * f as f64 / made.len().max(1) as f64);
    }

    println!("
## Words that are in the dictionary (ranks 2000 to 30000, one language): how many get a fix
");
    println!("| Language | tested | flagged | share |");
    println!("|---|---|---|---|");
    for (lang, words) in &d.known {
        let f = flagged(&d.full, params, *lang, words);
        println!("| {} | {} | {} | {:.1}% |", name(*lang), words.len(), f, 100.0 * f as f64 / words.len().max(1) as f64);
    }

    // speed: every call of a mixed sample, after the lookup tables are built
    let mut sample: Vec<(Option<Lang>, String)> = d.cases.iter().step_by(5).map(|c| (None, format!("{} ", c.typo))).collect();
    for (lang, _, tail, _, _) in &d.tails {
        sample.extend(tail.iter().step_by(10).map(|w| (Some(*lang), format!("The {w}, "))));
    }
    // all three tables built before timing
    for l in [Lang::En, Lang::Fr, Lang::Ar] {
        let _ = d.full.correct(Some(l), "warm ", 3);
    }
    let mut times: Vec<u128> = Vec::with_capacity(sample.len());
    for (only, text) in &sample {
        let t = Instant::now();
        let _ = d.full.correct_with(params, *only, text, 3);
        times.push(t.elapsed().as_nanos());
    }
    times.sort_unstable();
    let at = |p: f64| times[((times.len() - 1) as f64 * p) as usize] as f64 / 1000.0;
    println!("\n## Speed of one `correct` call ({} calls, release build)\n", times.len());
    println!("| p50 | p95 | p99 | max |");
    println!("|---|---|---|---|");
    println!("| {:.0} µs | {:.0} µs | {:.0} µs | {:.0} µs |", at(0.5), at(0.95), at(0.99), *times.last().unwrap() as f64 / 1000.0);
    println!("
The first call of a language builds its lookup tables once: {}.", d.first_call.iter().map(|(l, t)| format!("{} {:.0} ms", name(*l), t.as_secs_f64() * 1000.0)).collect::<Vec<_>>().join(", "));
}

/// Several settings of the dials, side by side: how many typos are fixed (top-1, all languages, one language each) against
/// how many valid words are wrongly flagged.
fn sweep(d: &Data) {
    let scaled = |f: f64| {
        let mut p = SpellParams::default();
        let s = |v: usize| ((v as f64 * f) as usize).min(1_000_000);
        p.latin_rank_limit = p.latin_rank_limit.map(|row| row.map(s));
        p.arabic_rank_limit = p.arabic_rank_limit.map(|row| row.map(s));
        p.accent_rank_limit = s(p.accent_rank_limit);
        p.d2_rank_limit = s(p.d2_rank_limit);
        p
    };
    let mut variants: Vec<(String, SpellParams)> = vec![("default".into(), SpellParams::default())];
    for f in [0.5, 2.0] {
        variants.push((format!("rank limits x{f}"), scaled(f)));
    }
    for f in [0.5, 2.0] {
        let d0 = SpellParams::default();
        variants.push((
            format!("known-word ratios x{f}"),
            SpellParams { known_ratio: d0.known_ratio.map(|row| row.map(|r| r * f)), accent_known_ratio: d0.accent_known_ratio * f, two_known_ratio: d0.two_known_ratio * f, ..d0 },
        ));
    }
    variants.push(("known words never corrected".into(), SpellParams { known_ratio: [[f64::INFINITY; 4]; 4], accent_known_ratio: f64::INFINITY, two_known_ratio: f64::INFINITY, ..SpellParams::default() }));
    variants.push(("first-letter substitutions x0.25".into(), SpellParams { first_letter_scale: 0.25, ..SpellParams::default() }));
    variants.push(("no distance 2".into(), SpellParams { d2_min_len: 999, ..SpellParams::default() }));
    variants.push(("no 'same word, other ending' rule".into(), SpellParams { inflections: false, ..SpellParams::default() }));
    variants.push(("no Arabic stem + affix known rule".into(), SpellParams { arabic_morphology: false, ..SpellParams::default() }));
    // a fifth of the cases, to keep the sweep short
    let quarter = |v: &[Case]| -> Vec<Case> { v.iter().step_by(5).cloned().collect() };
    let (cases, rare) = (quarter(&d.cases), quarter(&d.rare_cases));
    println!("| setting | English top-1 | French top-1 | Arabic top-1 | English rarer | French rarer | Arabic rarer | English false (OOV) | French false (OOV) | Arabic false (OOV) | English false (listed) | French false (listed) | Arabic false (listed) |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for (label, p) in &variants {
        let top1 = |cases: &[Case], lang: Lang| {
            let rows = run_cases(&d.full, p, true, cases);
            let mut t = Tally::default();
            rows.iter().filter(|r| r.0 == lang).for_each(|r| t.add(&r.2));
            t.pct(t.top1)
        };
        let fa = |lang: Lang| {
            let (_, e, tail, _, _) = d.tails.iter().find(|t| t.0 == lang).unwrap();
            let sample: Vec<String> = tail.iter().step_by(4).cloned().collect();
            100.0 * flagged(e, p, lang, &sample) as f64 / sample.len().max(1) as f64
        };
        let fk = |lang: Lang| {
            let (_, words) = d.known.iter().find(|k| k.0 == lang).unwrap();
            let sample: Vec<String> = words.iter().step_by(8).cloned().collect();
            100.0 * flagged(&d.full, p, lang, &sample) as f64 / sample.len().max(1) as f64
        };
        println!(
            "| {label} | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% |",
            top1(&cases, Lang::En), top1(&cases, Lang::Fr), top1(&cases, Lang::Ar),
            top1(&rare, Lang::En), top1(&rare, Lang::Fr), top1(&rare, Lang::Ar),
            fa(Lang::En), fa(Lang::Fr), fa(Lang::Ar), fk(Lang::En), fk(Lang::Fr), fk(Lang::Ar)
        );
    }
}

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    if !dir.join("en.txt").exists() {
        eprintln!("no word lists: run scripts/fetch-data.ps1");
        return;
    }
    let data = prepare(&dir);
    if std::env::args().any(|a| a == "--d2") {
        // what looking two edits away brings (words with two slips) and costs (false alarms), side by side
        println!("| setting | English two-slips top-3 | French | Arabic | English false (OOV) | French false (OOV) | Arabic false (OOV) |");
        println!("|---|---|---|---|---|---|---|");
        for (label, p) in [("distance 2 on (default)", SpellParams::default()), ("distance 2 off", SpellParams { d2_min_len: 999, ..SpellParams::default() })] {
            let two: Vec<Case> = data.cases.iter().filter(|c| c.kind == "two slips").cloned().collect();
            let rows = run_cases(&data.full, &p, true, &two);
            let top3 = |lang: Lang| rows.iter().filter(|r| r.0 == lang).map(|r| r.2.pct(r.2.top3)).next().unwrap_or(0.0);
            let fa = |lang: Lang| {
                let (_, e, tail, _, _) = data.tails.iter().find(|t| t.0 == lang).unwrap();
                100.0 * flagged(e, &p, lang, tail) as f64 / tail.len().max(1) as f64
            };
            println!("| {label} | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% |", top3(Lang::En), top3(Lang::Fr), top3(Lang::Ar), fa(Lang::En), fa(Lang::Fr), fa(Lang::Ar));
        }
    } else if std::env::args().any(|a| a == "--flagged-known") {
        // every 25th flagged dictionary word with its first fix and how much more frequent the fix is
        for (lang, words) in &data.known {
            println!("
## {} dictionary words that get a fix (word -> fix)", name(*lang));
            let mut shown = 0;
            let mut n = 0;
            for w in words {
                if let Some(f) = data.full.correct(Some(*lang), &format!("{w} "), 3) {
                    n += 1;
                    if n % 25 == 0 && shown < 45 {
                        println!("{w} -> {}", f.candidates[0].text);
                        shown += 1;
                    }
                }
            }
        }
    } else if std::env::args().any(|a| a == "--flagged") {
        // a look at what is wrongly flagged: every 120th flagged word with its first candidate
        for (lang, engine, tail, _, made) in &data.tails {
            println!("
## {} flagged tail words (word -> first fix)", name(*lang));
            let mut shown = 0;
            for (i, w) in tail.iter().chain(made.iter().take(0)).enumerate() {
                if let Some(f) = engine.correct(Some(*lang), &format!("{w} "), 3) {
                    if i % 60 == 0 && shown < 40 {
                        println!("{w} -> {}", f.candidates[0].text);
                        shown += 1;
                    }
                }
            }
            let _ = made;
        }
    } else if std::env::args().any(|a| a == "--sweep") {
        sweep(&data);
    } else {
        report(&data, &SpellParams::default());
    }
}
