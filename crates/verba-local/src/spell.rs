//! Spelling correction for the word the user has just finished typing.
//!
//! `LocalEngine::correct` is asked every time the text before the caret changes, so it must be quick and quiet:
//! it answers only when a word has just been finished (a space or punctuation follows it), that word is not a word
//! of the language, and one of the known words is clearly what was meant. A false alarm costs more than a miss.
//!
//! How a word is judged:
//! 1. **Known?** Latin: the exact lower-case word is in a list that is on. Arabic: its comparison key is in the list, or it is a
//!    listed stem with an attached particle (`و ف ب ل ك`, `ال`...) and/or a pronoun or plural ending, because the lists
//!    lack inflected forms. Known words are never touched.
//! 2. **Candidates.** All words at edit distance 1 of the typed word (deletion, insertion, substitution, adjacent
//!    transposition) are looked up in a hash table. Latin words are compared by comparison key (accents folded away,
//!    see `fold.rs`), so `tres` finds `très`, and an accent slip counts as no edit at all. Only when nothing is found,
//!    and the word has 6 letters or more, is distance 2 tried, over words with the same first letter.
//! 3. **Ranking.** Frequency share of the candidate, times a factor: accent fix > transposition > keyboard-neighbour
//!    substitution > other edits >> distance 2, and a bonus when the first letter is kept (typos rarely change it).
//! 4. **Gate.** A candidate must be frequent enough for the length of the word (`SpellParams`): short words are
//!    surrounded by other short words, so only very frequent candidates can be trusted there.

use std::collections::{BTreeSet, HashMap};

use verba_core::{Fix, Lang, Suggestion, current_word, is_arabic, is_arabic_mark, is_arabic_word, match_case, strip_arabic_marks};

use crate::fold::{ACCENTED, fold, is_latin_letter};
use crate::{AR_PARTICLES, Entry, LocalEngine, WordList};

/// What kind of edit turns the typed word into a candidate. Typos differ a lot in how reliable they are: two swapped letters
/// almost never turn one word into another, a substituted letter often does (`valley` / `volley`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    /// The accent is missing or wrong (the letters are right).
    Accent,
    Transposition,
    /// A letter replaced by the key next to it.
    Neighbour,
    /// A doubled letter missing or added (`occured`, `begining`).
    Double,
    /// Any other letter added, missing or replaced.
    Other,
    /// Two edits.
    Two,
}

impl Op {
    fn slot(self) -> usize {
        match self {
            Op::Transposition => 0,
            Op::Neighbour => 1,
            Op::Double => 2,
            _ => 3,
        }
    }
}

/// The dials of the correction. `Default` holds the values chosen from the measurements in docs/ACCURACY.md.
/// (Public and hidden only so that `examples/spell_accuracy.rs` can sweep them.)
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct SpellParams {
    /// Latin: the highest rank (0 = the most frequent word) a candidate may have, by kind of edit (transposition,
    /// neighbour key, doubled letter, other) and by length of the typed word (3, 4, 5, 6 or more letters). 0 = never.
    pub latin_rank_limit: [[usize; 4]; 4],
    /// The same for Arabic.
    pub arabic_rank_limit: [[usize; 4]; 4],
    /// An accent fix (`tres` -> `très`) needs a candidate at least this frequent.
    pub accent_rank_limit: usize,
    /// Distance 2 is tried only for words of at least this many letters, and only with candidates this frequent.
    pub d2_min_len: usize,
    pub d2_rank_limit: usize,
    /// How much a distance-2 candidate counts next to a distance-1 one.
    pub d2_factor: f64,
    /// Arabic: a common stem (rank within `arabic_stem_rank`) with a particle or ending counts as a known word.
    pub arabic_morphology: bool,
    pub arabic_stem_rank: usize,
    /// Words that differ only by an ending (`habitat` / `habitats`, `catered` / `caterer`) are different words, not typos.
    pub inflections: bool,
    /// ...but a word that is in the list as a different form of a much more frequent word is still a slip when the other one is this many times more frequent.
    pub inflection_override_ratio: f64,
    /// A word that IS in a list can still be a slip (the subtitle lists hold `wich`, `seperate`, `tres`): it is corrected when a word one edit away
    /// is at least this many times more frequent, by kind of edit (transposition, neighbour key, doubled letter, other) and by length of the
    /// typed word (3, 4, 5, 6 or more letters). Short words have many valid neighbours (`hood` / `good`), long ones almost none.
    pub known_ratio: [[f64; 4]; 4],
    /// Arabic words are short and their roots have many valid neighbours, but the lists carry slips such as `لمذا`: the ratios above are multiplied by this for Arabic.
    pub arabic_known_scale: f64,
    /// The same for a word two edits away (`tommorow`), which is only looked at when nothing is one edit away.
    pub two_known_ratio: f64,
    /// How much a word known only to a language that is not asked for counts as a known word (1 = fully).
    pub other_language_weight: f64,
    /// The same for an accent fix or a missing apostrophe (`tres` / `très`, `dont` / `don't`).
    pub accent_known_ratio: f64,
    /// A substitution that changes the first letter (`mobbed` / `robbed`) is rarely a typo: its rank limit is multiplied by this.
    pub first_letter_scale: f64,
}

impl Default for SpellParams {
    fn default() -> Self {
        Self {
            latin_rank_limit: [[1_500, 8_000, 30_000, 50_000], [0, 1_000, 4_000, 12_000], [0, 2_000, 8_000, 30_000], [0, 1_500, 4_000, 10_000]],
            arabic_rank_limit: [[300, 2_000, 8_000, 30_000], [0, 600, 2_500, 8_000], [0, 1_000, 4_000, 15_000], [0, 1_000, 3_000, 8_000]],
            accent_rank_limit: 20_000,
            d2_min_len: 7,
            d2_rank_limit: 4_000,
            d2_factor: 0.02,
            arabic_morphology: true,
            arabic_stem_rank: 12_000,
            inflections: true,
            inflection_override_ratio: 2_000.0,
            known_ratio: [[200.0, 60.0, 60.0, 40.0], [f64::INFINITY, 2_000.0, 600.0, 200.0], [f64::INFINITY, 1_500.0, 150.0, 30.0], [f64::INFINITY, 1_500.0, 800.0, 100.0]],
            arabic_known_scale: 0.3,
            two_known_ratio: 300.0,
            other_language_weight: 0.1,
            accent_known_ratio: 40.0,
            first_letter_scale: 1.0,
        }
    }
}

/// Lookup tables of one word list, built the first time a correction is asked for.
pub(crate) struct Index {
    /// The exact lower-case words and their entries (the "known word" test for Latin).
    words: HashMap<String, u32>,
    /// The largest share of the list that any word has (the word `the`): nothing can be more frequent than this.
    max_share: f64,
    /// Comparison key -> the entries (spellings) that have it.
    keys: HashMap<String, Vec<u32>>,
    /// Entry -> its rank by frequency (0 is the most frequent word).
    rank: Vec<u32>,
    /// First letter of the key -> the entries, for the distance-2 scan.
    by_first: HashMap<char, Vec<u32>>,
    /// The letters that occur in the keys: what an edit may insert or substitute.
    alphabet: Vec<char>,
}

impl Index {
    pub(crate) fn build(entries: &[Entry]) -> Self {
        let mut words = HashMap::with_capacity(entries.len());
        let mut keys: HashMap<String, Vec<u32>> = HashMap::with_capacity(entries.len());
        let mut by_first: HashMap<char, Vec<u32>> = HashMap::new();
        let mut alphabet = BTreeSet::new();
        for (i, e) in entries.iter().enumerate() {
            words.insert(e.word.clone(), i as u32);
            keys.entry(e.key.clone()).or_default().push(i as u32);
            if let Some(c) = e.key.chars().next() {
                by_first.entry(c).or_default().push(i as u32);
            }
            alphabet.extend(e.key.chars());
        }
        let mut order: Vec<u32> = (0..entries.len() as u32).collect();
        order.sort_by(|a, b| entries[*b as usize].count.cmp(&entries[*a as usize].count));
        let mut rank = vec![0u32; entries.len()];
        for (r, i) in order.into_iter().enumerate() {
            rank[i as usize] = r as u32;
        }
        let total: f64 = entries.iter().map(|e| e.count as f64).sum::<f64>().max(1.0);
        let max_share = entries.iter().map(|e| e.count).max().unwrap_or(0) as f64 / total;
        Self { words, max_share, keys, rank, by_first, alphabet: alphabet.into_iter().collect() }
    }
}

impl WordList {
    fn index(&self) -> &Index {
        self.spell.get_or_init(|| Index::build(&self.entries))
    }
}

// ───────────── keyboards ─────────────

const QWERTY: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
const AZERTY: [&str; 3] = ["azertyuiop", "qsdfghjklm", "wxcvbn"];
/// The standard Arabic layout, with each letter written as its comparison key (hamza seats folded).
const ARABIC_KEYS: [&str; 3] = ["ضصثقفغعهخحجد", "شسيبلاتنمكط", "ءورلايزظ"];
/// How far each row is shifted to the right of the one above, in key widths.
const ROW_SHIFT: [f32; 3] = [0.0, 0.25, 0.75];

fn key_pos(rows: &[&str; 3], c: char) -> Option<(f32, f32)> {
    for (r, row) in rows.iter().enumerate() {
        if let Some(i) = row.chars().position(|k| k == c) {
            return Some((i as f32 + ROW_SHIFT[r], r as f32));
        }
    }
    None
}

/// Whether two letters are next to each other on the keyboard of the language (the usual cause of a typo).
fn neighbours(lang: Lang, a: char, b: char) -> bool {
    let rows = match lang {
        Lang::En | Lang::Dz => &QWERTY,
        Lang::Fr => &AZERTY,
        Lang::Ar => &ARABIC_KEYS,
    };
    let (Some((xa, ya)), Some((xb, yb))) = (key_pos(rows, a), key_pos(rows, b)) else { return false };
    let (dx, dy) = ((xa - xb).abs(), (ya - yb).abs());
    a != b && ((dy == 0.0 && dx <= 1.01) || (dy == 1.0 && dx <= 0.76))
}

// ───────────── the word at the caret ─────────────

/// Characters that end a word: spaces and the usual punctuation (not the apostrophe and the hyphen, which live inside words).
fn is_delimiter(c: char) -> bool {
    c.is_whitespace() || matches!(c, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '"' | '»' | '”' | '…' | '،' | '؛' | '؟')
}

/// The elision stems of French (`l'ete`): a word after one of these and an apostrophe is a word like any other.
const ELISIONS: &[&str] = &["l", "d", "j", "c", "n", "m", "t", "s", "qu", "lorsqu", "puisqu", "jusqu", "quelqu"];

/// The finished word at the end of the text: (text before it, the word, what follows it), when the text ends with
/// a word and 1 to 3 delimiters. `None` for anything that is not a plain word (digits, URLs, hyphens...).
fn finished_word(before: &str) -> Option<(&str, &str, &str)> {
    let mut end = before.len();
    let mut delimiters = 0;
    for (i, c) in before.char_indices().rev() {
        if !is_delimiter(c) {
            break;
        }
        delimiters += 1;
        end = i;
    }
    if delimiters == 0 || delimiters > 3 {
        return None;
    }
    let word = current_word(&before[..end]);
    if word.is_empty() {
        return None;
    }
    let start = end - word.len();
    let head = &before[..start];
    match head.chars().next_back() {
        None => {}
        // part of a number, a compound, an address or a path: not a word to correct
        Some(c) if c.is_ascii_digit() || matches!(c, '-' | '_' | '@' | '/' | '\\' | '#' | '.' | '&' | '+' | '=' | '<' | '$' | '%') => return None,
        Some('\'' | '’') => {
            // only a French elision: `l'ete`; `o'clock`, `rock'n` and the like are left alone
            let stem: String = head.trim_end_matches(['\'', '’']).chars().rev().take_while(|c| c.is_alphabetic()).collect::<Vec<_>>().into_iter().rev().collect();
            if !ELISIONS.contains(&stem.to_lowercase().as_str()) {
                return None;
            }
        }
        Some(_) => {}
    }
    Some((head, word, &before[end..]))
}

/// A capital letter at the start of a sentence is normal; anywhere else it makes the word a name, which is left alone.
fn at_sentence_start(head: &str) -> bool {
    let trimmed = head.trim_end();
    trimmed.is_empty() || head[trimmed.len()..].contains('\n') || trimmed.ends_with(['.', '!', '?', '؟', '…'])
}

// ───────────── candidates ─────────────

/// The words one edit away from the typed word, each with how much it counts and what kind of edit it is.
fn edits1(lang: Lang, chars: &[char], alphabet: &[char]) -> HashMap<String, (f64, Op)> {
    const TRANSPOSITION: f64 = 1.5;
    const NEIGHBOUR: f64 = 1.4;
    const DOUBLE: f64 = 1.2;
    let n = chars.len();
    let mut out: HashMap<String, (f64, Op)> = HashMap::new();
    let mut put = |s: String, factor: f64, op: Op| {
        let e = out.entry(s).or_insert((0.0, op));
        if factor > e.0 {
            *e = (factor, op);
        }
    };
    // the typed word has one letter too many
    for i in 0..n {
        let s: String = chars.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, c)| *c).collect();
        let doubled = (i > 0 && chars[i] == chars[i - 1]) || (i + 1 < n && chars[i] == chars[i + 1]);
        put(s, if doubled { DOUBLE } else { 1.0 }, if doubled { Op::Double } else { Op::Other });
    }
    // two letters swapped
    for i in 0..n.saturating_sub(1) {
        if chars[i] != chars[i + 1] {
            let mut v = chars.to_vec();
            v.swap(i, i + 1);
            put(v.into_iter().collect(), TRANSPOSITION, Op::Transposition);
        }
    }
    // one letter replaced
    for i in 0..n {
        for &c in alphabet {
            if c != chars[i] {
                let mut v = chars.to_vec();
                v[i] = c;
                let near = neighbours(lang, chars[i], c);
                put(v.into_iter().collect(), if near { NEIGHBOUR } else { 1.0 }, if near { Op::Neighbour } else { Op::Other });
            }
        }
    }
    // the typed word misses one letter
    for i in 0..=n {
        for &c in alphabet {
            let mut v = chars.to_vec();
            v.insert(i, c);
            let doubled = (i > 0 && chars[i - 1] == c) || (i < n && chars[i] == c);
            if c == '\'' {
                // a missing apostrophe (`dont`, `cant`) is as common and as safe as a missing accent
                put(v.into_iter().collect(), 2.0, Op::Accent);
            } else {
                put(v.into_iter().collect(), if doubled { 1.1 } else { 1.0 }, if doubled { Op::Double } else { Op::Other });
            }
        }
    }
    out
}

/// Damerau-Levenshtein distance of two keys when it is at most 2.
fn within_two(a: &[char], b: &[char]) -> Option<u8> {
    if a.len().abs_diff(b.len()) > 2 {
        return None;
    }
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0u8; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i.min(255) as u8;
    }
    for j in 0..=m {
        d[0][j] = j.min(255) as u8;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = u8::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    (d[n][m] <= 2).then_some(d[n][m])
}

/// Arabic pronoun and plural endings, as comparison keys (`ه` stands for `ة` too). Longest first.
const AR_SUFFIXES: &[&str] = &["هما", "كما", "هم", "هن", "ها", "كم", "كن", "نا", "ات", "ون", "ين", "ان", "ه", "ك", "ي"];

/// An Arabic key that is a listed stem with a particle and/or an ending attached.
fn arabic_known(idx: &Index, key: &str, max_stem_rank: usize) -> bool {
    if idx.keys.contains_key(key) {
        return true;
    }
    let len = key.chars().count();
    let prefixes = std::iter::once("").chain(AR_PARTICLES.iter().copied());
    for p in prefixes {
        let Some(rest) = key.strip_prefix(p) else { continue };
        for s in std::iter::once("").chain(AR_SUFFIXES.iter().copied()) {
            if p.is_empty() && s.is_empty() {
                continue;
            }
            let Some(stem) = rest.strip_suffix(s) else { continue };
            // the stem must be a common word: `ك` + `شرا` is not a reason to accept `كشرا`
            let common = idx.keys.get(stem).is_some_and(|ids| ids.iter().any(|i| idx.rank[*i as usize] as usize <= max_stem_rank));
            if stem.chars().count() >= 3 && len >= 4 && common {
                return true;
            }
        }
    }
    false
}

/// Endings that turn a word into another form of itself (plural, tense, adverb...). Two words that differ only by such
/// endings are different words, not a typo of each other. As comparison keys (accents folded). `""` is "no ending".
const EN_ENDINGS: &[&str] = &[
    "", "s", "es", "ed", "d", "r", "ing", "ings", "er", "ers", "est", "ly", "y", "ies", "ied", "ier", "iest", "al", "ful", "ness", "ment",
    "ments", "able", "ible", "ish", "ous", "ive", "ion", "ions", "ity", "ally", "ily", "en", "ic", "ist", "ists", "age", "ey", "ie", "ys", "ier", "ers",
];
const FR_ENDINGS: &[&str] = &[
    "", "s", "x", "e", "es", "ee", "ees", "er", "ez", "ent", "ant", "ait", "ais", "aient", "ons", "ions", "eur", "eurs", "euse", "euses", "ement",
    "ment", "ique", "iques", "if", "ive", "al", "ale", "ales", "aux", "eux", "ier", "iere", "ien", "iens", "ienne", "ois", "oise", "u", "ue", "us",
    "ues", "i", "ie", "is", "ies", "it", "ir", "a", "as", "ai", "ont", "era", "erai", "eras", "erez", "eront", "iez", "tion", "tions", "te", "tes",
];
/// Arabic prefixes (and, so, with, for, like, the, future, verb person) and endings (pronouns, plural, tense), as comparison keys.
const AR_PREFIXES: &[&str] = &[
    "", "و", "ف", "ب", "ل", "ك", "ال", "وال", "فال", "بال", "كال", "لل", "ولل", "ول", "وب", "وك", "فل", "فب", "س", "وس", "فس", "ي", "ت", "ن", "ا", "سي",
    "ست", "سن", "سا", "وي", "وت", "ون", "في", "فت",
];
const AR_AFFIX_ENDINGS: &[&str] = &[
    "", "ه", "ها", "هم", "هن", "هما", "ك", "كم", "كن", "كما", "ي", "نا", "ات", "ون", "ين", "ان", "وا", "ت", "تم", "تن", "تما", "ن", "ني", "تني", "تك",
    "تها", "تهم", "تنا", "ا", "يه", "ته", "تي",
];

/// True when the two keys differ only by their endings (Latin) or only by affixes around a shared stem (Arabic).
fn inflection_variant(lang: Lang, typed: &str, cand: &str) -> bool {
    if lang.is_rtl() {
        for p in AR_PREFIXES {
            let Some(rest) = typed.strip_prefix(p) else { continue };
            for s in AR_AFFIX_ENDINGS {
                let Some(stem) = rest.strip_suffix(s) else { continue };
                if stem.chars().count() < 3 {
                    continue;
                }
                if let Some(pos) = cand.find(stem) {
                    if AR_PREFIXES.contains(&&cand[..pos]) && AR_AFFIX_ENDINGS.contains(&&cand[pos + stem.len()..]) {
                        return true;
                    }
                }
            }
        }
        return false;
    }
    let endings = if lang == Lang::Fr { FR_ENDINGS } else { EN_ENDINGS };
    // the same stem (3 letters or more) with two different endings, or with an ending and none
    endings.iter().any(|et| {
        typed.strip_suffix(et).is_some_and(|stem| {
            stem.chars().count() >= 3 && endings.iter().any(|ec| cand.strip_suffix(ec).is_some_and(|other| other == stem))
        })
    })
}

/// A candidate: the entry, and how much it counts relative to its frequency.
struct Cand {
    entry: u32,
    factor: f64,
    op: Op,
}

fn candidates(list: &WordList, lang: Lang, typed: &str, key: &str, params: &SpellParams) -> Vec<Cand> {
    let idx = list.index();
    let arabic = lang.is_rtl();
    let chars: Vec<char> = key.chars().collect();
    let first = chars.first().copied();
    // the most frequent spelling among the entries of a key
    let best_of = |ids: &Vec<u32>| ids.iter().copied().max_by_key(|i| list.entries[*i as usize].count);
    let mut out: Vec<Cand> = Vec::new();

    // the typed letters are right but an accent is missing or wrong: `tres` -> `très`
    // (a word that already has an accent is not missing one: `décore` is not a slip for `décoré`)
    if !arabic && !typed.chars().any(|c| ACCENTED.contains(c)) {
        if let Some(ids) = idx.keys.get(key) {
            for &i in ids {
                if list.entries[i as usize].word != typed {
                    out.push(Cand { entry: i, factor: 3.0, op: Op::Accent });
                }
            }
        }
    }
    for (s, (factor, op)) in edits1(lang, &chars, &idx.alphabet) {
        if s == key || s.starts_with('\'') || s.ends_with('\'') {
            continue;
        }
        if let Some(i) = idx.keys.get(&s).and_then(best_of) {
            let first_letter = if s.chars().next() == first { 1.25 } else { 0.7 };
            out.push(Cand { entry: i, factor: factor * first_letter, op });
        }
    }
    if out.is_empty() && chars.len() >= params.d2_min_len {
        if let Some(bucket) = first.and_then(|c| idx.by_first.get(&c)) {
            for &i in bucket {
                let e = &list.entries[i as usize];
                if idx.rank[i as usize] as usize > params.d2_rank_limit {
                    continue;
                }
                let theirs: Vec<char> = e.key.chars().collect();
                if within_two(&chars, &theirs) == Some(2) {
                    out.push(Cand { entry: i, factor: params.d2_factor, op: Op::Two });
                }
            }
        }
    }
    out
}

/// The most frequent letters count for the length of the typed word: 3, 4, 5, 6 or more.
fn length_slot(letters: usize) -> usize {
    letters.saturating_sub(3).min(3)
}

impl LocalEngine {
    /// Makes the correction leave this word alone from now on (the user dismissed it, or it is a name they use).
    pub fn ignore_word(&mut self, word: &str) {
        self.ignored.insert(word.to_lowercase());
    }

    /// A correction for the last word of `before_caret`, when that word is finished (a space or punctuation follows it)
    /// and is not a word of the language. `only` limits the languages the fixes come from, like `suggest_in`; a word that any
    /// language that is on knows is never touched (French typed with an English keyboard is not a typo).
    /// `None` when there is nothing to fix.
    pub fn correct(&self, only: Option<Lang>, before_caret: &str, max: usize) -> Option<Fix> {
        self.correct_with(&SpellParams::default(), only, before_caret, max)
    }

    /// `correct` with other dials (for measuring).
    #[doc(hidden)]
    pub fn correct_with(&self, params: &SpellParams, only: Option<Lang>, before_caret: &str, max: usize) -> Option<Fix> {
        if max == 0 {
            return None;
        }
        let (head, word, trailing) = finished_word(before_caret)?;
        let arabic = is_arabic_word(word);
        // one script per word
        let one_script = if arabic { word.chars().all(|c| is_arabic(c) || is_arabic_mark(c)) } else { !word.chars().any(is_arabic) };
        if !one_script {
            return None;
        }
        let letters: Vec<char> = if arabic { strip_arabic_marks(word).chars().collect() } else { word.chars().collect() };
        if letters.len() < 3 || letters.len() > 24 {
            return None;
        }

        // names, acronyms, camelCase: left alone
        if !arabic {
            let upper = letters.iter().filter(|c| c.is_uppercase()).count();
            let first_upper = letters[0].is_uppercase();
            if upper > 1 || (upper == 1 && !first_upper) {
                return None;
            }
            if first_upper && !at_sentence_start(head) {
                return None;
            }
        }

        let lower = if arabic { strip_arabic_marks(word) } else { word.to_lowercase() };
        if self.ignored.contains(&lower) || self.personal.contains(&lower) {
            return None;
        }
        // the same word written earlier in the visible text: meant
        if head.split(|c: char| !c.is_alphabetic()).any(|w| !w.is_empty() && w.to_lowercase() == lower) {
            return None;
        }
        if !arabic && !lower.chars().all(is_latin_letter) {
            return None;
        }
        // Algerian Darija has no standard spelling: a word it knows is never a typo, and is never replaced
        if self.dz_knows(&lower) {
            return None;
        }

        // the lists of this script that are on: any of them knowing the word is enough; the fixes come from `only`
        let mut known_in: Vec<(Lang, &WordList)> = Vec::new();
        let mut fix_from: Vec<(Lang, &WordList)> = Vec::new();
        for lang in [Lang::En, Lang::Fr, Lang::Ar] {
            if lang.is_rtl() == arabic && self.is_enabled(lang) {
                if let Some(list) = self.lists.get(&lang) {
                    known_in.push((lang, list));
                    if only.is_none_or(|o| o == lang) {
                        fix_from.push((lang, list));
                    }
                }
            }
        }
        if fix_from.is_empty() {
            return None;
        }

        let key = fold(&lower);
        // how common the typed word is in the lists that have it (0 when none does). A word that is in a list may still be
        // a slip, if a word one edit away is far more frequent: see `known_ratio`.
        let mut typed_share = 0.0f64;
        for (lang, list) in &known_in {
            let idx = list.index();
            // a word of a language that is not the one being typed (`dont`, French, on an English keyboard) counts for much less
            let weight = if fix_from.iter().any(|(l, _)| l == lang) { 1.0 } else { params.other_language_weight };
            let share = if arabic {
                match idx.keys.get(&key) {
                    Some(ids) => ids.iter().map(|i| list.entries[*i as usize].count).max().unwrap_or(0) as f64 / list.total,
                    // a listed stem with a particle or an ending is a word, and never touched
                    None if params.arabic_morphology && arabic_known(idx, &key, params.arabic_stem_rank) => return None,
                    None => 0.0,
                }
            } else {
                idx.words.get(&lower).map_or(0.0, |i| list.entries[*i as usize].count as f64 / list.total)
            };
            typed_share = typed_share.max(share * weight);
        }
        if typed_share > 0.0 {
            // quick exit: nothing is frequent enough to be the word that was meant
            let smallest_ratio = params.known_ratio.iter().flatten().copied().fold(params.accent_known_ratio.min(params.two_known_ratio), f64::min);
            let top = fix_from.iter().map(|(_, l)| l.index().max_share).fold(0.0, f64::max);
            if typed_share * smallest_ratio > top {
                return None;
            }
        }

        // with English and French both giving fixes, the words before this one lean the choice
        let bias = if !arabic && fix_from.len() > 1 { self.context_bias(head, &fix_from) } else { vec![1.0; fix_from.len()] };

        let slot = length_slot(letters.len());
        let table = |lang: Lang, op: Op| -> usize {
            match op {
                Op::Accent => params.accent_rank_limit,
                Op::Two => params.d2_rank_limit,
                op if lang.is_rtl() => params.arabic_rank_limit[op.slot()][slot],
                op => params.latin_rank_limit[op.slot()][slot],
            }
        };
        let mut found: Vec<(String, f64, Lang)> = Vec::new();
        for ((lang, list), lean) in fix_from.iter().zip(&bias) {
            let idx = list.index();
            for c in candidates(list, *lang, &lower, &key, params) {
                let e = &list.entries[c.entry as usize];
                let mut limit = table(*lang, c.op);
                if matches!(c.op, Op::Neighbour | Op::Other) && e.key.chars().next() != key.chars().next() {
                    limit = (limit as f64 * params.first_letter_scale) as usize;
                }
                if idx.rank[c.entry as usize] as usize > limit {
                    continue;
                }
                if params.inflections && c.op != Op::Accent && inflection_variant(*lang, &key, &e.key) {
                    // another form of the same word: a rare form of a word is not a slip, unless the other form is overwhelmingly
                    // more frequent (`realy` is `real` + `y`, `really` is `real` + `ly`, but nobody means `realy`)
                    if typed_share == 0.0 || (e.count as f64 / list.total) < params.inflection_override_ratio * typed_share {
                        continue;
                    }
                }
                if typed_share > 0.0 {
                    let ratio = match c.op {
                        Op::Accent => params.accent_known_ratio,
                        Op::Two => params.two_known_ratio,
                        op => params.known_ratio[op.slot()][slot],
                    } * if lang.is_rtl() { params.arabic_known_scale } else { 1.0 };
                    if e.count as f64 / list.total < ratio * typed_share {
                        continue;
                    }
                }
                found.push((e.word.clone(), e.count as f64 / list.total * c.factor * lean, *lang));
            }
        }
        found.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut candidates: Vec<Suggestion> = Vec::new();
        for (text, _, lang) in found {
            let text = if arabic { text } else { match_case(word, &text) };
            if text != word && !candidates.iter().any(|s| s.text == text) {
                candidates.push(Suggestion { text, lang });
            }
            if candidates.len() == max {
                break;
            }
        }
        if candidates.is_empty() {
            return None;
        }
        Some(Fix { original: word.to_string(), trailing: trailing.to_string(), candidates })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_finished_word() {
        assert_eq!(finished_word("I recieve "), Some(("I ", "recieve", " ")));
        assert_eq!(finished_word("teh, "), Some(("", "teh", ", ")));
        assert_eq!(finished_word("مرحبا،"), Some(("", "مرحبا", "،")));
        assert_eq!(finished_word("l'ete "), Some(("l'", "ete", " ")));
    }

    #[test]
    fn a_word_still_being_typed_is_not_finished() {
        assert_eq!(finished_word("recieve"), None);
        assert_eq!(finished_word("teh....."), None, "more than three delimiters");
        assert_eq!(finished_word("  "), None);
    }

    #[test]
    fn numbers_compounds_addresses_and_odd_apostrophes_are_not_words() {
        assert_eq!(finished_word("abc123def "), None);
        assert_eq!(finished_word("well-knwon "), None);
        assert_eq!(finished_word("www.exmple "), None);
        assert_eq!(finished_word("o'clck "), None);
    }

    #[test]
    fn sentence_starts() {
        assert!(at_sentence_start(""));
        assert!(at_sentence_start("Hello. "));
        assert!(at_sentence_start("one\n"));
        assert!(at_sentence_start("Really? "));
        assert!(!at_sentence_start("then "));
    }

    #[test]
    fn keyboard_neighbours() {
        assert!(neighbours(Lang::En, 'e', 'r'));
        assert!(neighbours(Lang::En, 'q', 'w'));
        assert!(neighbours(Lang::En, 'a', 'w'));
        assert!(!neighbours(Lang::En, 'q', 'p'));
        // on a French keyboard `a` is where `q` is on an English one
        assert!(neighbours(Lang::Fr, 'a', 'z'));
        assert!(!neighbours(Lang::Fr, 'a', 'p'));
        assert!(neighbours(Lang::Ar, 'ب', 'ل'));
    }

    #[test]
    fn distance_is_measured_with_transpositions() {
        let d = |a: &str, b: &str| within_two(&a.chars().collect::<Vec<_>>(), &b.chars().collect::<Vec<_>>());
        assert_eq!(d("definately", "definitely"), Some(1));
        assert_eq!(d("recieve", "receive"), Some(1), "a swapped pair is one edit");
        assert_eq!(d("abcdef", "abxxef"), Some(2));
        assert_eq!(d("abcdef", "xyzdef"), None);
        assert_eq!(d("abc", "abcdef"), None, "too different in length");
    }
}
