//! Next-word prediction: the caret is right after a finished word and a space (`I would like |`), and the engine
//! offers the words that most often follow (`to`, `a`, `the`).
//!
//! Two sources, the user's own habits first:
//! 1. the pairs of words the user's own text taught the engine (see `personal.rs`);
//! 2. a bundled table per language: for each of the commonly used words, the words that most often come next, built
//!    by `scripts/fetch-bigrams.ps1` from a sentence corpus (see docs/DATA.md). A few frequent two-word contexts
//!    (`would like` -> `to`) back it up when that was measured to help.
//!
//! When nothing is known about the word before the caret, nothing is offered: no frequent word is ever used as a filler.
//!
//! This file also holds the word scanner that the learning in `personal.rs` and the table builder share, so that text
//! is cut into words the same way everywhere.

use std::collections::{HashMap, HashSet};

use verba_core::{Lang, Suggestion, is_arabic, is_arabic_mark, match_case};

use crate::clean::clean_token;
use crate::{LocalEngine, WordList};

// ───────────── cutting text into words ─────────────

/// French elision stems (`l'ami`): they are never a word to predict or to predict from, so they cut the text there.
const ELISIONS: &[&str] = &["l", "d", "j", "c", "n", "m", "t", "s", "qu", "lorsqu", "puisqu", "jusqu", "quelqu"];

/// One-letter words that are real words and very frequent.
fn single_letter_word(lang: Lang, c: char) -> bool {
    if lang == Lang::En {
        matches!(c, 'a' | 'i')
    } else if lang == Lang::Fr {
        matches!(c, 'a' | 'à' | 'y')
    } else {
        false
    }
}

/// How the text is cut for one language.
#[derive(Clone, Copy)]
pub(crate) struct Rules {
    pub lang: Lang,
}

impl Rules {
    pub(crate) fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn elision(self) -> bool {
        self.lang == Lang::Fr
    }

    /// The table word for a raw token: lower case, no short vowels, a clean word of the language. `None` for anything else
    /// (digits, hyphens, URLs, odd letters...).
    pub(crate) fn word(self, raw: &str) -> Option<String> {
        let mut chars = raw.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            let lower = c.to_lowercase().next().unwrap_or(c);
            return single_letter_word(self.lang, lower).then(|| lower.to_string());
        }
        clean_token(raw)
    }
}

/// What the scanner found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Tok {
    /// A word (already converted by the rules).
    Word(String),
    /// Something that is not a word (a number, an address, an elision `l'`): the text is cut here and no pair is made across it.
    Barrier,
    /// Punctuation or a line break: the text is cut here. `sentence_end` for `. ! ? ؟ …` and line breaks.
    Break { sentence_end: bool },
}

pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
    pub tok: Tok,
}

/// Characters that belong to a raw token. `_ @ / \ #` are in it so that identifiers, e-mail addresses, paths and
/// URLs end up as one token that is then rejected.
fn is_token_char(c: char) -> bool {
    c.is_alphanumeric() || is_arabic_mark(c) || matches!(c, '_' | '@' | '/' | '\\' | '#')
}

/// Cuts `text` into words, barriers and breaks. `convert` turns a raw token into a word (or refuses it).
pub(crate) fn scan(text: &str, elision: bool, mut convert: impl FnMut(&str) -> Option<String>) -> Vec<Span> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let (pos, c) = chars[i];
        if is_token_char(c) {
            let mut j = i;
            while j < n {
                let cj = chars[j].1;
                if is_token_char(cj) {
                    j += 1;
                    continue;
                }
                let next = chars.get(j + 1).map(|x| x.1);
                // an apostrophe, a hyphen or a dot between two letters stays in the token; so does the `:` of `http://` and `C:\`
                let joins = match cj {
                    '\'' | '’' | '-' | '.' => next.is_some_and(|x| x.is_alphanumeric()),
                    ':' => matches!(next, Some('/' | '\\')),
                    _ => false,
                };
                if !joins {
                    break;
                }
                j += 1;
            }
            let end = chars.get(j).map_or(text.len(), |x| x.0);
            let mut start = pos;
            if elision {
                if let Some(p) = text[start..end].find(['\'', '’']) {
                    let stem = text[start..start + p].to_lowercase();
                    if ELISIONS.contains(&stem.as_str()) {
                        let after = start + p + text[start + p..].chars().next().map_or(1, char::len_utf8);
                        out.push(Span { start, end: after, tok: Tok::Barrier });
                        start = after;
                    }
                }
            }
            if start < end {
                let tok = match convert(&text[start..end]) {
                    Some(w) => Tok::Word(w),
                    None => Tok::Barrier,
                };
                out.push(Span { start, end, tok });
            }
            i = j;
        } else {
            let end = pos + c.len_utf8();
            if c == '\n' || c == '\r' {
                out.push(Span { start: pos, end, tok: Tok::Break { sentence_end: true } });
            } else if !c.is_whitespace() {
                out.push(Span { start: pos, end, tok: Tok::Break { sentence_end: matches!(c, '.' | '!' | '?' | '؟' | '…' | '。') } });
            }
            i += 1;
        }
    }
    out
}

/// The runs of consecutive words of a text (a number, a break or an address ends a run).
pub(crate) fn runs(text: &str, rules: Rules) -> Vec<Vec<String>> {
    let mut all: Vec<Vec<String>> = Vec::new();
    let mut cur: Vec<String> = Vec::new();
    for span in scan(text, rules.elision(), |raw| rules.word(raw)) {
        match span.tok {
            Tok::Word(w) => cur.push(w),
            _ => {
                if !cur.is_empty() {
                    all.push(std::mem::take(&mut cur));
                }
            }
        }
    }
    if !cur.is_empty() {
        all.push(cur);
    }
    all
}

/// The runs of consecutive words of a text, cut the way the tables are built. For tools that measure the engine.
#[doc(hidden)]
pub fn word_runs(lang: Lang, text: &str) -> Vec<Vec<String>> {
    runs(text, Rules::new(lang))
}

/// How many bytes before the caret are looked at.
const WINDOW: usize = 240;

/// The last two words of the text when it ends with a word and spaces (`I would like ` -> `would`, `like`), oldest first.
/// Empty when the caret is not right after a finished word and a space, or when that word is not a plain word.
/// `convert` makes the words: it decides what a word is (the table rules, or the looser ones of the personal dictionary).
pub(crate) fn tail_words(before_caret: &str, elision: bool, convert: impl FnMut(&str) -> Option<String>) -> Vec<String> {
    let trimmed = before_caret.trim_end_matches([' ', '\u{a0}', '\u{202f}']);
    if trimmed.len() == before_caret.len() || trimmed.is_empty() {
        return Vec::new();
    }
    let mut from = before_caret.len().saturating_sub(WINDOW);
    while !before_caret.is_char_boundary(from) {
        from += 1;
    }
    let window = &before_caret[from..];
    let spans = scan(window, elision, convert);
    let Some(last) = spans.last() else { return Vec::new() };
    if !matches!(last.tok, Tok::Word(_)) || window[last.end..].chars().any(|c| !matches!(c, ' ' | '\u{a0}' | '\u{202f}')) {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for span in spans.into_iter().rev() {
        match span.tok {
            Tok::Word(w) if out.len() < 2 => out.push(w),
            _ => break,
        }
    }
    out.reverse();
    out
}

// ───────────── the bundled tables ─────────────

/// What follows one context: how often the context was seen followed by a counted word, and the best followers.
struct Row {
    total: u32,
    next: Box<[(Box<str>, u32)]>,
}

/// Next-word counts of one language: for each frequent word (`would`) its followers (`like`, `you`...), and for a few
/// frequent pairs (`would like`) theirs.
#[derive(Default)]
pub struct NextTable {
    rows: HashMap<Box<str>, Row>,
}

/// The dials of the prediction. `Default` holds the values chosen from the measurements in docs/ACCURACY.md.
/// (Public and hidden so that `examples/nextword_accuracy.rs` can sweep them.)
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct NextParams {
    /// A word is offered only when it follows with at least this probability.
    pub min_prob: f64,
    /// Use the two previous words when the table knows them.
    pub trigram: bool,
    /// How much a two-word context counts: its weight is `total / (total + tri_k)`.
    pub tri_k: f64,
}

impl Default for NextParams {
    fn default() -> Self {
        Self { min_prob: 0.02, trigram: true, tri_k: 20.0 }
    }
}

impl NextTable {
    /// Reads the table text (`context<TAB>total<TAB>word:count word:count ...`, `#` lines are comments). Lines that make no
    /// sense are skipped, so a damaged file gives a smaller table, never an error.
    pub fn parse(text: &str) -> Self {
        let mut rows = HashMap::new();
        for line in text.lines() {
            let line = line.trim_start_matches('\u{feff}');
            if line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('\t');
            let (Some(key), Some(total), Some(list)) = (parts.next(), parts.next(), parts.next()) else { continue };
            let Ok(total) = total.trim().parse::<u32>() else { continue };
            if key.is_empty() || total == 0 {
                continue;
            }
            let next: Vec<(Box<str>, u32)> = list
                .split_whitespace()
                .filter_map(|item| {
                    let (w, c) = item.rsplit_once(':')?;
                    Some((w.into(), c.parse::<u32>().ok().filter(|c| *c > 0)?))
                })
                .collect();
            if !next.is_empty() {
                rows.insert(key.into(), Row { total, next: next.into_boxed_slice() });
            }
        }
        Self { rows }
    }

    /// The table as text (what `parse` reads), with the given comment lines on top.
    pub fn to_text(&self, header: &str) -> String {
        let mut keys: Vec<(&Box<str>, &Row)> = self.rows.iter().collect();
        // pairs of words after the single words, each group by how often the context occurs
        keys.sort_by(|a, b| a.0.contains(' ').cmp(&b.0.contains(' ')).then(b.1.total.cmp(&a.1.total)).then_with(|| a.0.cmp(b.0)));
        let mut out = String::new();
        for line in header.lines() {
            out.push_str("# ");
            out.push_str(line);
            out.push('\n');
        }
        for (key, row) in keys {
            out.push_str(&format!("{key}\t{}\t", row.total));
            let items: Vec<String> = row.next.iter().map(|(w, c)| format!("{w}:{c}")).collect();
            out.push_str(&items.join(" "));
            out.push('\n');
        }
        out
    }

    /// How many contexts the table holds (single words and pairs).
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The likeliest next words after `w2` (and `w1` before it), best first, with their probability.
    pub fn predict(&self, w1: Option<&str>, w2: &str, params: &NextParams) -> Vec<(&str, f64)> {
        let two = self.rows.get(w2);
        let three = if params.trigram { w1.and_then(|w1| self.rows.get(format!("{w1} {w2}").as_str())) } else { None };
        let weight = three.map_or(0.0, |r| r.total as f64 / (r.total as f64 + params.tri_k));
        let mut scores: Vec<(&str, f64)> = Vec::new();
        if let Some(r) = two {
            for (w, c) in r.next.iter() {
                scores.push((w, (1.0 - weight) * *c as f64 / r.total as f64));
            }
        }
        if let Some(r) = three {
            for (w, c) in r.next.iter() {
                let p = weight * *c as f64 / r.total as f64;
                match scores.iter_mut().find(|s| s.0 == &**w) {
                    Some(s) => s.1 += p,
                    None => scores.push((w, p)),
                }
            }
        }
        scores.retain(|s| s.1 >= params.min_prob);
        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(b.0)));
        scores
    }
}

// ───────────── building a table from sentences ─────────────

/// The dials of the table builder (what is kept).
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct BuildParams {
    /// Single-word contexts kept (the most frequent words) and how many followers each keeps.
    pub contexts: usize,
    pub per_context: usize,
    /// A follower seen fewer times than this is dropped.
    pub min_count: u32,
    /// Two-word contexts: how many are kept (0 = none), how often a context must occur, how many followers each keeps, and the follower's least count.
    pub pair_contexts: usize,
    pub pair_min_total: u32,
    pub pair_per_context: usize,
    pub pair_min_count: u32,
    /// A word that is this many times more frequent in the corpus than in the reference list is a trait of the corpus
    /// (the names Tom and Mary in Tatoeba), not of the language: it is never predicted.
    pub corpus_ratio: f64,
    /// Words the reference list does not hold are never predicted (one-letter words are exempt).
    pub need_in_reference: bool,
    /// Words seen fewer times than this in the corpus are never predicted.
    pub min_word_count: u64,
}

impl Default for BuildParams {
    fn default() -> Self {
        Self {
            contexts: 10_000,
            per_context: 6,
            min_count: 2,
            pair_contexts: 20_000,
            pair_min_total: 15,
            pair_per_context: 5,
            pair_min_count: 3,
            corpus_ratio: 8.0,
            need_in_reference: true,
            min_word_count: 5,
        }
    }
}

impl BuildParams {
    /// The values chosen for a language from the measurements in docs/ACCURACY.md. Arabic has a small corpus, and its words carry attached
    /// particles that the subtitle list lacks, so there the reference list only removes what it knows to be a trait of the corpus.
    pub fn for_lang(lang: Lang) -> Self {
        if lang.is_rtl() {
            Self { need_in_reference: false, min_word_count: 2, min_count: 1, ..Self::default() }
        } else {
            Self::default()
        }
    }
}

const BREAK: u32 = u32::MAX;

/// Collects sentences and turns them into a `NextTable`.
#[doc(hidden)]
pub struct NextBuilder {
    rules: Rules,
    ids: HashMap<String, u32>,
    words: Vec<String>,
    counts: Vec<u64>,
    /// The words of every sentence as ids, runs separated by `BREAK`.
    flat: Vec<u32>,
    seen: HashSet<u64>,
}

fn hash64(s: &str) -> u64 {
    // FNV-1a: only used to skip sentences that are in the corpus twice
    let mut h = 0xcbf29ce484222325u64;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

impl NextBuilder {
    pub fn new(lang: Lang) -> Self {
        Self {
            rules: Rules::new(lang),
            ids: HashMap::new(),
            words: Vec::new(),
            counts: Vec::new(),
            flat: Vec::new(),
            seen: HashSet::new(),
        }
    }

    /// Adds one sentence. A sentence that was added before (same words, any capitals) is skipped. Returns whether it was new.
    pub fn add_sentence(&mut self, text: &str) -> bool {
        self.add_sentence_times(text, 1)
    }

    /// Adds one sentence as if it had been seen `times` times: a corpus of everyday sentences can weigh more than a bigger corpus of another
    /// kind of text. A sentence that was added before is skipped. Returns whether it was new.
    pub fn add_sentence_times(&mut self, text: &str, times: usize) -> bool {
        if !self.seen.insert(hash64(&text.trim().to_lowercase())) {
            return false;
        }
        for run in runs(text, self.rules) {
            for _ in 0..times.max(1) {
                for w in &run {
                    let id = match self.ids.get(w) {
                        Some(id) => *id,
                        None => {
                            let id = self.words.len() as u32;
                            self.ids.insert(w.clone(), id);
                            self.words.push(w.clone());
                            self.counts.push(0);
                            id
                        }
                    };
                    self.counts[id as usize] += 1;
                    self.flat.push(id);
                }
                self.flat.push(BREAK);
            }
        }
        true
    }

    /// Whether this sentence (same words, any capitals) was added before.
    pub fn has_sentence(&self, text: &str) -> bool {
        self.seen.contains(&hash64(&text.trim().to_lowercase()))
    }

    /// How many words (tokens) were added.
    pub fn token_count(&self) -> u64 {
        self.counts.iter().sum()
    }

    /// The most frequent words of the corpus, with their counts. For tools.
    pub fn top_words(&self, n: usize) -> Vec<(&str, u64)> {
        let mut v: Vec<(&str, u64)> = self.words.iter().map(String::as_str).zip(self.counts.iter().copied()).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        v.truncate(n);
        v
    }

    /// The most frequent words that may be predicted (see `BuildParams::corpus_ratio`): the baseline "always offer the commonest words".
    pub fn top_allowed(&self, reference: &WordList, p: &BuildParams, n: usize) -> Vec<String> {
        let allowed = self.allowed(reference, p);
        let mut v: Vec<(&str, u64)> = self.words.iter().map(String::as_str).zip(self.counts.iter().copied()).zip(allowed).filter(|(_, a)| *a).map(|(x, _)| x).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
        v.into_iter().take(n).map(|x| x.0.to_string()).collect()
    }

    /// Which words may be predicted: not a trait of the corpus (see `BuildParams::corpus_ratio`).
    fn allowed(&self, reference: &WordList, p: &BuildParams) -> Vec<bool> {
        let total = self.token_count().max(1) as f64;
        self.words
            .iter()
            .zip(&self.counts)
            .map(|(w, &c)| {
                if w.chars().count() == 1 {
                    return true;
                }
                if c < p.min_word_count {
                    return false;
                }
                let theirs = reference.share_of(w);
                if theirs <= 0.0 {
                    return !p.need_in_reference;
                }
                c as f64 / total <= p.corpus_ratio * theirs
            })
            .collect()
    }

    pub fn build(&self, reference: &WordList, p: &BuildParams) -> NextTable {
        let allowed = self.allowed(reference, p);
        let ok = |id: u32| id != BREAK && allowed[id as usize];
        let key2 = |a: u32, b: u32| ((a as u64) << 32) | b as u64;

        // pairs
        let mut pairs: HashMap<u64, u32> = HashMap::new();
        for w in self.flat.windows(2) {
            if ok(w[0]) && ok(w[1]) {
                *pairs.entry(key2(w[0], w[1])).or_insert(0) += 1;
            }
        }
        let mut by_first: HashMap<u32, Vec<(u32, u32)>> = HashMap::new();
        for (k, c) in &pairs {
            by_first.entry((*k >> 32) as u32).or_default().push((*k as u32, *c));
        }
        let mut firsts: Vec<(u32, u64)> = by_first.iter().map(|(a, v)| (*a, v.iter().map(|x| x.1 as u64).sum())).collect();
        firsts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        firsts.truncate(p.contexts);

        let mut rows: HashMap<Box<str>, Row> = HashMap::new();
        for (a, total) in &firsts {
            let mut followers = by_first[a].clone();
            followers.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
            followers.retain(|f| f.1 >= p.min_count);
            followers.truncate(p.per_context);
            if followers.is_empty() {
                continue;
            }
            let next: Vec<(Box<str>, u32)> = followers.iter().map(|(b, c)| (self.words[*b as usize].as_str().into(), *c)).collect();
            rows.insert(self.words[*a as usize].as_str().into(), Row { total: (*total).min(u32::MAX as u64) as u32, next: next.into_boxed_slice() });
        }

        // pairs of words as contexts
        if p.pair_contexts > 0 && self.words.len() < (1 << 21) {
            let eligible: HashSet<u64> = pairs.iter().filter(|(_, c)| **c >= p.pair_min_total).map(|(k, _)| *k).collect();
            let mut triples: HashMap<u64, u32> = HashMap::new();
            let pack = |a: u32, b: u32, c: u32| ((a as u64) << 42) | ((b as u64) << 21) | c as u64;
            for w in self.flat.windows(3) {
                if ok(w[0]) && ok(w[1]) && ok(w[2]) && eligible.contains(&key2(w[0], w[1])) {
                    *triples.entry(pack(w[0], w[1], w[2])).or_insert(0) += 1;
                }
            }
            let mut by_pair: HashMap<u64, Vec<(u32, u32)>> = HashMap::new();
            for (k, c) in &triples {
                let (a, b, third) = ((*k >> 42) as u32, ((*k >> 21) & 0x1f_ffff) as u32, (*k & 0x1f_ffff) as u32);
                by_pair.entry(key2(a, b)).or_default().push((third, *c));
            }
            let mut ctx: Vec<(u64, u32)> = by_pair.iter().map(|(k, v)| (*k, v.iter().map(|x| x.1).sum())).collect();
            ctx.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            ctx.truncate(p.pair_contexts);
            for (k, total) in ctx {
                let mut followers = by_pair[&k].clone();
                followers.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
                followers.retain(|f| f.1 >= p.pair_min_count);
                followers.truncate(p.pair_per_context);
                if followers.is_empty() {
                    continue;
                }
                let key = format!("{} {}", self.words[(k >> 32) as usize], self.words[(k & 0xffff_ffff) as usize]);
                let next: Vec<(Box<str>, u32)> = followers.iter().map(|(c, n)| (self.words[*c as usize].as_str().into(), *n)).collect();
                rows.insert(key.into(), Row { total, next: next.into_boxed_slice() });
            }
        }
        NextTable { rows }
    }
}

// ───────────── the engine ─────────────

impl LocalEngine {
    /// Loads the bundled next-word table of a language from its text (see `NextTable::parse`).
    pub fn load_next_words(&mut self, lang: Lang, text: &str) {
        self.next_tables.insert(lang, NextTable::parse(text));
    }

    /// Loads `<code>.bigrams.txt` for every language whose word list is loaded; a missing file only means no prediction for it.
    pub(crate) fn load_next_words_dir(&mut self, dir: &std::path::Path) {
        for lang in self.languages() {
            if let Ok(text) = std::fs::read_to_string(dir.join(format!("{}.bigrams.txt", lang.code()))) {
                self.load_next_words(lang, &text);
            }
        }
    }

    /// Whether a bundled table is loaded for the language.
    pub fn has_next_words(&self, lang: Lang) -> bool {
        self.next_tables.get(&lang).is_some_and(|t| !t.is_empty())
    }

    /// The table of a language. For tools that measure the engine.
    #[doc(hidden)]
    pub fn next_table(&self, lang: Lang) -> Option<&NextTable> {
        self.next_tables.get(&lang)
    }

    /// Words likely to come next, for when the caret is right after a finished word and a space (`I would like ` -> `to`, `a`, ...).
    /// `only` limits the language like `suggest_in`. Empty when the caret is not in that place, when the word before it is not a
    /// plain word, or when nothing is known about it: it never fills the popup with words that are merely common.
    /// The user's own pairs of words (see `learn_text`) come first, then the bundled table.
    pub fn next_words(&self, only: Option<Lang>, before_caret: &str, max: usize) -> Vec<Suggestion> {
        self.next_words_with(&NextParams::default(), only, before_caret, max)
    }

    /// `next_words` with other dials (for measuring).
    #[doc(hidden)]
    pub fn next_words_with(&self, params: &NextParams, only: Option<Lang>, before_caret: &str, max: usize) -> Vec<Suggestion> {
        if max == 0 {
            return Vec::new();
        }
        let trimmed = before_caret.trim_end_matches([' ', '\u{a0}', '\u{202f}']);
        let Some(last) = trimmed.chars().next_back().filter(|_| trimmed.len() < before_caret.len()) else { return Vec::new() };
        let arabic = is_arabic(last) || is_arabic_mark(last);

        // the languages of this script that are on and have a table (and a word list, so that the language is really loaded)
        let mut langs: Vec<(Lang, &WordList)> = Vec::new();
        for lang in self.languages() {
            if lang.is_rtl() == arabic && self.is_enabled(lang) && only.is_none_or(|o| o == lang) {
                if let Some(list) = self.lists.get(&lang) {
                    langs.push((lang, list));
                }
            }
        }
        if langs.is_empty() {
            return Vec::new();
        }
        let bias = if !arabic && langs.len() > 1 { self.context_bias(trimmed, &langs) } else { vec![1.0; langs.len()] };
        let lead = langs.iter().zip(&bias).max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|(l, _)| l.0).unwrap_or(langs[0].0);

        let mut out: Vec<Suggestion> = Vec::new();
        let push = |text: String, lang: Lang, out: &mut Vec<Suggestion>| {
            if !out.iter().any(|s| s.text == text) {
                out.push(Suggestion { text, lang });
            }
        };

        // 1. what the user's own text taught
        for word in self.personal.next_after(before_caret, arabic, max) {
            push(self.personal.form_of(&word, arabic).unwrap_or_else(|| display_form(lead, &word)), lead, &mut out);
        }

        // 2. the bundled tables, each language weighted by how well the words before fit it
        let mut found: Vec<(String, f64, Lang)> = Vec::new();
        for ((lang, _), factor) in langs.iter().zip(&bias) {
            let Some(table) = self.next_tables.get(lang) else { continue };
            let rules = Rules::new(*lang);
            let tail = tail_words(before_caret, rules.elision(), |raw| rules.word(raw));
            let Some((w2, rest)) = tail.split_last().map(|(w2, rest)| (w2.as_str(), rest)) else { continue };
            for (w, p) in table.predict(rest.last().map(String::as_str), w2, params) {
                found.push((w.to_string(), p * factor, *lang));
            }
        }
        found.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (word, _, lang) in found {
            push(self.personal.form_of(&word, arabic).unwrap_or_else(|| display_form(lang, &word)), lang, &mut out);
        }
        out.truncate(max);
        out
    }
}

/// A word of the table as it is written mid-sentence: `i` and `i'm` take a capital.
fn display_form(lang: Lang, word: &str) -> String {
    if lang == Lang::En && (word == "i" || word.starts_with("i'")) { match_case("I", word) } else { word.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str, lang: Lang) -> Vec<Vec<String>> {
        runs(text, Rules::new(lang))
    }

    #[test]
    fn text_is_cut_into_runs_of_plain_words() {
        assert_eq!(words("I would like a cup, please.", Lang::En), [vec!["i", "would", "like", "a", "cup"], vec!["please"]]);
        // numbers, addresses and hyphenated words are not words and cut the run
        assert_eq!(words("see www.example.com for 3 well-known things", Lang::En), [vec!["see"], vec!["for"], vec!["things"]]);
        assert_eq!(words("mail me at a.b@x.org now", Lang::En), [vec!["mail", "me", "at"], vec!["now"]]);
        assert_eq!(words("open C:\\Users\\me\\file.txt and read", Lang::En), [vec!["open"], vec!["and", "read"]]);
        assert_eq!(words("don't stop", Lang::En), [vec!["don't", "stop"]]);
    }

    #[test]
    fn french_elisions_cut_the_run() {
        assert_eq!(words("je pense qu'il est là", Lang::Fr), [vec!["je", "pense"], vec!["il", "est", "là"]]);
        assert_eq!(words("l'ami est à la maison", Lang::Fr), [vec!["ami", "est", "à", "la", "maison"]]);
        assert_eq!(words("aujourd'hui il pleut", Lang::Fr), [vec!["aujourd'hui", "il", "pleut"]]);
    }

    #[test]
    fn arabic_words_lose_their_short_vowels() {
        assert_eq!(words("كَيْفَ حالُك؟", Lang::Ar), [vec!["كيف", "حالك"]]);
    }

    fn tail(text: &str) -> Vec<String> {
        let rules = Rules::new(Lang::En);
        tail_words(text, false, |raw| rules.word(raw))
    }

    #[test]
    fn the_tail_is_the_last_two_words_before_a_space() {
        assert_eq!(tail("I would like "), ["would", "like"]);
        assert_eq!(tail("like "), ["like"]);
        assert_eq!(tail("Say hello. Then "), ["then"], "a full stop ends the context");
        assert_eq!(tail("Yes, I "), ["i"]);
        assert!(tail("I would like").is_empty(), "no space yet: the word is still being typed");
        assert!(tail("I would like, ").is_empty());
        assert!(tail("I would like\n").is_empty());
        assert!(tail("room 12 ").is_empty());
        assert!(tail("mail bob@x.org ").is_empty());
        assert!(tail("").is_empty());
        assert!(tail("  ").is_empty());
    }

    #[test]
    fn a_table_round_trips_through_its_text() {
        let text = "# comment\nwould\t100\tlike:60 you:20 not:10\nwould like\t50\tto:40 a:5\nbroken line\nbad\tx\tfoo:1\nempty\t5\t\n";
        let t = NextTable::parse(text);
        assert_eq!(t.len(), 2);
        let again = NextTable::parse(&t.to_text("a\nb"));
        assert_eq!(again.len(), 2);
        let p = again.predict(None, "would", &NextParams { min_prob: 0.0, trigram: false, tri_k: 20.0 });
        assert_eq!(p.iter().map(|x| x.0).collect::<Vec<_>>(), ["like", "you", "not"]);
    }

    #[test]
    fn the_two_word_context_backs_up_the_one_word_context() {
        let t = NextTable::parse("like\t100\tto:21 a:20 you:10\nwould like\t100\tto:90 a:5\n");
        let on = NextParams { min_prob: 0.0, trigram: true, tri_k: 20.0 };
        let off = NextParams { trigram: false, ..on.clone() };
        let with = t.predict(Some("would"), "like", &on);
        assert_eq!(with[0].0, "to");
        assert!(with[0].1 > t.predict(None, "like", &off)[0].1);
        // an unknown first word falls back to the single word
        assert_eq!(t.predict(Some("they"), "like", &on)[0].0, "to");
        assert!(t.predict(None, "zzz", &on).is_empty());
    }

    #[test]
    fn rare_followers_are_cut_by_the_probability_floor() {
        let t = NextTable::parse("like\t1000\tto:500 a:10\n");
        let p = t.predict(None, "like", &NextParams::default());
        assert_eq!(p.iter().map(|x| x.0).collect::<Vec<_>>(), ["to"]);
    }

    #[test]
    fn the_builder_keeps_followers_and_skips_corpus_traits_and_duplicates() {
        let reference = WordList::parse("i 500\nwould 400\nlike 300\nto 900\ntea 100\ncoffee 100\nwant 200\nthe 800\ntom 1\n", 100);
        let mut b = NextBuilder::new(Lang::En);
        let mut corpus = String::new();
        for _ in 0..10 {
            corpus.push_str("I would like to tea.\n");
        }
        let mut added = 0;
        for line in corpus.lines() {
            added += usize::from(b.add_sentence(line));
        }
        assert_eq!(added, 1, "the same sentence is counted once");
        for i in 0..12 {
            b.add_sentence(&format!("I want the tea {}.", "x".repeat(i % 2))); // `tea` after `the`
            b.add_sentence(&format!("Tom likes tom{i}"));
        }
        for _ in 0..3 {
            b.add_sentence("I would like to coffee");
            b.add_sentence("I would like to tea now");
        }
        let p = BuildParams { min_word_count: 1, min_count: 1, ..BuildParams::default() };
        let t = b.build(&reference, &p);
        let got = t.predict(None, "would", &NextParams { min_prob: 0.0, trigram: false, tri_k: 1.0 });
        assert_eq!(got[0].0, "like");
        // `tom` is frequent in the corpus and nearly absent from the reference: never predicted
        assert!(t.predict(None, "i", &NextParams { min_prob: 0.0, ..NextParams::default() }).iter().all(|x| x.0 != "tom"));
    }
}
