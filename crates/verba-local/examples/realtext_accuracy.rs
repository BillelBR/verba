//! How well the word lists finish the word being typed on REAL text (sentences nobody built a list from), by size of list.
//!
//!   cargo run --release -p verba-local --example realtext_accuracy -- --dir data/eval/full --max-words 100000 --lang en \
//!       --tsv data/eval/src/eng_sentences.tsv [--sentences 20000]
//!   ... --lines data/eval/src/wiki_en.txt        (any text, one sentence per line, instead of --tsv)
//!
//! `--dir` holds `en.txt`, `fr.txt` and `ar.txt` in the format of the shipped lists (the FrequencyWords `_full` files work: the list is cut
//! to `--max-words` after the cleaning of the engine). Only the lists are loaded: no next-word table, no dictionary, so that the numbers
//! say what the list does, and a table built from the same sentences cannot flatter them.
//!
//! Method: every word of 4 letters or more in the text is typed letter by letter after the words before it (the last 48 characters, as the
//! program sees them), and the engine is asked for 3 suggestions after each letter, like the popup. A hit means the word is among them.
//! Names (a capital letter inside a sentence), words in capitals, words stuck to an apostrophe, a hyphen or a digit, and words in another
//! script are left out. Reported: how many of the words the list has at all, the hit rate within 2, 3 and 4 letters, the letters saved
//! per word (the length of the word minus the letters typed when it first appears; 0 when it never does), and the cost (load time,
//! memory, time of one suggestion).
//!
//! `--typos` measures the spelling correction instead, on the same words: every word of 5 letters or more gets one typo (a letter left out,
//! two letters swapped, a letter doubled, a letter replaced; the same typo at every run), and the engine is asked about it as the program
//! is (the typo and a space before the caret). A bigger list also hides more typos (a typo that is itself a word of the list cannot be
//! seen) and may offer rarer words: this shows how much. Reported: typos hidden by the list, typos with a correction offered, and the
//! right word first / among the three offered.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use verba_core::{Lang, is_arabic, is_arabic_mark, strip_arabic_marks};
use verba_local::LocalEngine;

/// Counts the bytes that are alive, to report the memory of a list.
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        LIVE.fetch_add(l.size(), Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        LIVE.fetch_add(n, Ordering::Relaxed);
        LIVE.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;

const MIN_LETTERS: usize = 4;
const CONTEXT: usize = 48;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

/// The words of a sentence worth testing: (index of its first character, the word as written).
fn words_of(sentence: &str, lang: Lang, skipped: &mut usize) -> Vec<(usize, String)> {
    let chars: Vec<char> = sentence.chars().collect();
    let is_word_char = |c: char| c.is_alphabetic() || (lang == Lang::Ar && is_arabic_mark(c));
    let sticks = |c: char| matches!(c, '\'' | '\u{2019}' | '-' | '\u{2010}' | '\u{2011}') || c.is_ascii_digit();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !is_word_char(chars[i]) {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && is_word_char(chars[i]) {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        let stuck = (start > 0 && sticks(chars[start - 1])) || (i < chars.len() && sticks(chars[i]));
        let plain = if lang == Lang::Ar { strip_arabic_marks(&word) } else { word.clone() };
        let letters = plain.chars().count();
        let script_ok = if lang == Lang::Ar { plain.chars().all(is_arabic) } else { plain.chars().all(|c| c.is_alphabetic() && !is_arabic(c)) };
        let name = lang != Lang::Ar
            && ((start > 0 && word.chars().next().is_some_and(char::is_uppercase)) || (letters > 1 && word.chars().all(char::is_uppercase)));
        if letters < MIN_LETTERS || !script_ok || stuck || name {
            *skipped += 1;
            continue;
        }
        out.push((start, word));
    }
    out
}

fn normal(text: &str, lang: Lang) -> String {
    let lower = text.to_lowercase();
    if lang == Lang::Ar { strip_arabic_marks(&lower) } else { lower }
}

/// One typo of a word, always the same for the same word: (the typo, what kind).
fn typo_of(word: &str, lang: Lang) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();
    if chars.len() < 5 {
        return None;
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in word.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    let alphabet: Vec<char> = match lang {
        Lang::Ar => "\u{627}\u{628}\u{62a}\u{62b}\u{62c}\u{62d}\u{62e}\u{62f}\u{631}\u{633}\u{634}\u{639}\u{641}\u{642}\u{643}\u{644}\u{645}\u{646}\u{647}\u{648}\u{64a}".chars().collect(),
        _ => "abcdefghijklmnopqrstuvwxyz".chars().collect(),
    };
    // never the first letter (people rarely get it wrong), never the last (a typo there is a different word more often)
    let at = 1 + (h >> 8) as usize % (chars.len() - 2);
    let mut out = chars.clone();
    match h % 4 {
        0 => {
            out.remove(at);
        }
        1 => out.swap(at, at + 1),
        2 => out.insert(at, chars[at]),
        _ => {
            let other = alphabet[(h >> 20) as usize % alphabet.len()];
            if other == chars[at] {
                return None;
            }
            out[at] = other;
        }
    }
    let typo: String = out.into_iter().collect();
    (typo != word).then_some(typo)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = PathBuf::from(arg(&args, "--dir").unwrap_or_else(|| "data/words".to_string()));
    let max_words: usize = arg(&args, "--max-words").and_then(|v| v.parse().ok()).unwrap_or(50_000);
    let max_sentences: usize = arg(&args, "--sentences").and_then(|v| v.parse().ok()).unwrap_or(20_000);
    let lang = match arg(&args, "--lang").as_deref() {
        Some("fr") => Lang::Fr,
        Some("ar") => Lang::Ar,
        _ => Lang::En,
    };

    // the text
    let (source, mut texts): (String, Vec<String>) = if let Some(path) = arg(&args, "--tsv") {
        let raw = std::fs::read_to_string(&path).expect("the --tsv file");
        (path, raw.lines().filter_map(|l| l.splitn(3, '\t').nth(2)).map(str::to_string).collect())
    } else if let Some(path) = arg(&args, "--lines") {
        let raw = std::fs::read_to_string(&path).expect("the --lines file");
        (path, raw.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
    } else {
        eprintln!("give --tsv <Tatoeba sentences> or --lines <one sentence per line>");
        std::process::exit(2);
    };
    // an even sample of the text, the same one at every run
    if texts.len() > max_sentences {
        let stride = texts.len() / max_sentences;
        texts = texts.into_iter().step_by(stride.max(1)).take(max_sentences).collect();
    }

    // the engine (lists only), with its cost
    let before = LIVE.load(Ordering::Relaxed);
    let t = Instant::now();
    // `--shipped`: the folder is loaded as the program loads it (each list at its own size, the next-word tables, the dictionary file set
    // aside to be read at the first translation), to measure the cost of what ships
    let engine = if args.iter().any(|a| a == "--shipped") {
        LocalEngine::load_dir(&dir).expect("the folder")
    } else {
        LocalEngine::load_lists_seeded(&dir, max_words, args.iter().any(|a| a == "--seed")).expect("the lists")
    };
    let load = t.elapsed();
    let memory = LIVE.load(Ordering::Relaxed).saturating_sub(before);
    let list_len = engine.word_count(lang);
    let pct = |n: usize, d: usize| if d == 0 { 0.0 } else { n as f64 * 100.0 / d as f64 };
    let known: HashSet<String> = engine.list(lang).map(|l| l.iter_words().map(|(w, _)| normal(w, lang)).collect()).unwrap_or_default();

    if args.iter().any(|a| a == "--typos") {
        let (mut typos, mut hidden, mut offered, mut first, mut top3) = (0usize, 0usize, 0usize, 0usize, 0usize);
        let (mut skipped_words, mut calls, mut call_time) = (0usize, 0u64, std::time::Duration::ZERO);
        let mut seen: HashSet<String> = HashSet::new();
        // the words to spoil are the same at every size of list: those of the list in `--base-dir` (the shipped one), else of 50 000
        let base_dir = arg(&args, "--base-dir").map(PathBuf::from).unwrap_or_else(|| dir.clone());
        let base: HashSet<String> = LocalEngine::load_lists(&base_dir, 50_000).expect("the lists").list(lang).map(|l| l.iter_words().map(|(w, _)| normal(w, lang)).collect()).unwrap_or_default();
        for sentence in &texts {
            let chars: Vec<char> = sentence.chars().collect();
            for (start, word) in words_of(sentence, lang, &mut skipped_words) {
                let plain = if lang == Lang::Ar { strip_arabic_marks(&word) } else { word.clone() };
                let target = normal(&plain, lang);
                // only words the shipped size of list (50 000) has: a typo of a rare word is another question
                if !base.contains(&target) || !seen.insert(target.clone()) {
                    continue;
                }
                let Some(typo) = typo_of(&target, lang) else { continue };
                typos += 1;
                // (counted, but asked all the same: whether the engine leaves such a typo alone is what is measured)
                if known.contains(&typo) {
                    hidden += 1;
                }
                let context: String = chars[..start].iter().collect();
                let before: String = format!("{context}{typo} ");
                let tail: String = { let c: Vec<char> = before.chars().collect(); c[c.len().saturating_sub(CONTEXT)..].iter().collect() };
                let t = Instant::now();
                let fix = engine.correct(Some(lang), &tail, 3);
                call_time += t.elapsed();
                calls += 1;
                if let Some(fix) = fix {
                    offered += 1;
                    let texts: Vec<String> = fix.candidates.iter().map(|c| normal(&c.text, lang)).collect();
                    if texts.first() == Some(&target) {
                        first += 1;
                    }
                    if texts.iter().any(|t| *t == target) {
                        top3 += 1;
                    }
                }
            }
        }
        println!("lang {} | text {} | lists cap {} -> {} words after cleaning", lang.code(), source.rsplit(['\\', '/']).next().unwrap_or(&source), max_words, list_len);
        println!(
            "typos {typos} | the typo is a word of the list {:.1} % | of ALL the typos: a correction offered {:.1} %, right word first {:.1} %, among three {:.1} %",
            pct(hidden, typos),
            pct(offered, typos),
            pct(first, typos),
            pct(top3, typos)
        );
        println!("one correction takes {:.0} us on average (the first one builds the index; load {:.2} s)", call_time.as_secs_f64() * 1e6 / calls.max(1) as f64, load.as_secs_f64());
        return;
    }
    let (mut words, mut in_list, mut skipped) = (0usize, 0usize, 0usize);
    let mut hit_within = [0usize; 5]; // within 1, 2, 3, 4 letters (index = letters)
    let (mut hit_any, mut hit_any_in_list) = (0usize, 0usize);
    let (mut saved, mut letters_total) = (0usize, 0usize);
    let (mut calls, mut call_time) = (0u64, std::time::Duration::ZERO);
    for sentence in &texts {
        let chars: Vec<char> = sentence.chars().collect();
        for (start, word) in words_of(sentence, lang, &mut skipped) {
            let plain = if lang == Lang::Ar { strip_arabic_marks(&word) } else { word.clone() };
            let target = normal(&plain, lang);
            let typed_chars: Vec<char> = plain.chars().collect();
            let len = typed_chars.len();
            words += 1;
            letters_total += len;
            let listed = known.contains(&target);
            if listed {
                in_list += 1;
            }
            let context: Vec<char> = chars[..start].to_vec();
            let mut first_hit: Option<usize> = None;
            for k in 1..len {
                let mut full: Vec<char> = context.clone();
                full.extend(&typed_chars[..k]);
                let tail: String = full[full.len().saturating_sub(CONTEXT)..].iter().collect();
                let t = Instant::now();
                let found = engine.suggest_in(lang, &tail, 3);
                call_time += t.elapsed();
                calls += 1;
                if found.iter().any(|s| normal(&s.text, lang) == target) {
                    first_hit = Some(k);
                    break;
                }
            }
            if let Some(k) = first_hit {
                hit_any += 1;
                if listed {
                    hit_any_in_list += 1;
                }
                saved += len - k;
                for within in k..=4 {
                    hit_within[within] += 1;
                }
            }
        }
    }
    println!(
        "lang {} | text {} ({} sentences) | lists {} cap {} -> {} words after cleaning",
        lang.code(),
        source.rsplit(['\\', '/']).next().unwrap_or(&source),
        texts.len(),
        dir.display(),
        max_words,
        list_len
    );
    println!("words tested {words} (left out: {skipped}); the list has {:.1} % of them", pct(in_list, words));
    println!(
        "hit within 2 letters {:.1} % | 3 letters {:.1} % | 4 letters {:.1} % | at any point {:.1} % (of the words the list has: {:.1} %)",
        pct(hit_within[2], words),
        pct(hit_within[3], words),
        pct(hit_within[4], words),
        pct(hit_any, words),
        pct(hit_any_in_list, in_list)
    );
    println!(
        "letters saved per word {:.2} of {:.2} ({:.1} %) | load {:.2} s, memory {:.1} MB, {:.0} us per suggestion",
        saved as f64 / words.max(1) as f64,
        letters_total as f64 / words.max(1) as f64,
        pct(saved, letters_total),
        load.as_secs_f64(),
        memory as f64 / 1048576.0,
        call_time.as_secs_f64() * 1e6 / calls.max(1) as f64
    );
}
