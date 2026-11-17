//! Builds Verba's bilingual dictionary `data/words/dict.txt` from Wiktionary data extracted by wiktextract (kaikki.org).
//!
//! Normally run by `scripts/fetch-dict.ps1`, which downloads the three kaikki.org files and streams them in. Two steps:
//!
//!   1. `extract`: reads ONE kaikki.org JSONL stream (stdin, or `--in <file>`; one entry per line, lines can be very long) and writes a
//!      small tab-separated extract of what the dictionary needs. The stream is read line by line and never held whole.
//!        build_dict extract --lang en --out data/words/src/dict-en.tsv  < kaikki.org-dictionary-English.jsonl
//!   2. `build`: reads the three extracts and the frequency lists (`scripts/fetch-data.ps1`) and writes `dict.txt`.
//!        build_dict build --src data/words/src --out data/words/dict.txt [--max-mb 14] [--report data/words/src/dict-report.txt]
//!
//! What goes in and how translations are chosen is documented in docs/DATA.md ("Word translation") and measured in docs/ACCURACY.md.
//! In short: translations of English entries (en>fr, en>ar, and the same pairs read backwards) and the English glosses of French and Arabic
//! entries (fr>en, ar>en; only short glosses that look like a translation) are scored by the order of the senses, the number of sources and
//! the frequency of the target word; inflected forms of a lemma are headwords that carry its translations.
//! This file uses only the standard library (a small JSON reader for the keys it needs is written below).

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------------------------------------------------------------
// A small JSON reader. wiktextract lines are huge (tens of kilobytes, some megabytes) and mostly hold things we do not need, so
// every object has a context that says which keys are kept; the others are skipped without being copied.
// ---------------------------------------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    Entry,
    Sense,
    Form,
    Tr,
    Word,
    Leaf,
    Skip,
}

fn child_ctx(ctx: Ctx, key: &[u8]) -> Ctx {
    match (ctx, key) {
        (Ctx::Entry, b"word" | b"pos" | b"lang_code") => Ctx::Leaf,
        (Ctx::Entry, b"senses") => Ctx::Sense,
        (Ctx::Entry, b"translations") => Ctx::Tr,
        (Ctx::Entry, b"forms") => Ctx::Form,
        (Ctx::Sense, b"glosses" | b"tags" | b"raw_tags") => Ctx::Leaf,
        (Ctx::Sense, b"form_of" | b"alt_of") => Ctx::Word,
        (Ctx::Sense, b"translations") => Ctx::Tr,
        (Ctx::Form, b"form" | b"tags") => Ctx::Leaf,
        (Ctx::Tr, b"word" | b"lang_code" | b"sense" | b"tags") => Ctx::Leaf,
        (Ctx::Word, b"word" | b"extra") => Ctx::Leaf,
        (Ctx::Leaf, _) => Ctx::Leaf,
        _ => Ctx::Skip,
    }
}

enum V {
    Other,
    Str(String),
    Arr(Vec<V>),
    Obj(Vec<(String, V)>),
}

impl V {
    fn get(&self, key: &str) -> Option<&V> {
        match self {
            V::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn str(&self, key: &str) -> &str {
        match self.get(key) {
            Some(V::Str(s)) => s,
            _ => "",
        }
    }
    fn arr(&self, key: &str) -> &[V] {
        match self.get(key) {
            Some(V::Arr(a)) => a,
            _ => &[],
        }
    }
    fn strings(&self, key: &str) -> Vec<&str> {
        self.arr(key).iter().filter_map(|v| if let V::Str(s) = v { Some(s.as_str()) } else { None }).collect()
    }
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl<'a> Parser<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn skip_string(&mut self) -> Option<()> {
        self.i += 1; // the opening quote
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'"' => {
                    self.i += 1;
                    return Some(());
                }
                b'\\' => self.i += 2,
                _ => self.i += 1,
            }
        }
        None
    }
    fn skip_value(&mut self) -> Option<()> {
        self.ws();
        match *self.b.get(self.i)? {
            b'"' => self.skip_string(),
            b'{' | b'[' => {
                let mut depth = 0usize;
                while self.i < self.b.len() {
                    match self.b[self.i] {
                        b'"' => {
                            self.skip_string()?;
                            continue;
                        }
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth -= 1;
                            if depth == 0 {
                                self.i += 1;
                                return Some(());
                            }
                        }
                        _ => {}
                    }
                    self.i += 1;
                }
                None
            }
            _ => {
                while self.i < self.b.len() && !matches!(self.b[self.i], b',' | b'}' | b']') {
                    self.i += 1;
                }
                Some(())
            }
        }
    }
    fn hex4(&mut self) -> Option<u32> {
        let h = self.b.get(self.i..self.i + 4)?;
        self.i += 4;
        u32::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()
    }
    fn string(&mut self) -> Option<String> {
        self.i += 1; // the opening quote
        let mut out: Vec<u8> = Vec::new();
        loop {
            let start = self.i;
            while self.i < self.b.len() && self.b[self.i] != b'"' && self.b[self.i] != b'\\' {
                self.i += 1;
            }
            out.extend_from_slice(&self.b[start..self.i]);
            match *self.b.get(self.i)? {
                b'"' => {
                    self.i += 1;
                    return Some(String::from_utf8_lossy(&out).into_owned());
                }
                _ => {
                    self.i += 1;
                    let c = *self.b.get(self.i)?;
                    self.i += 1;
                    match c {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' | b'f' => out.push(b' '),
                        b'u' => {
                            let mut cp = self.hex4()?;
                            if (0xD800..0xDC00).contains(&cp) && self.b.get(self.i) == Some(&b'\\') && self.b.get(self.i + 1) == Some(&b'u') {
                                self.i += 2;
                                let lo = self.hex4()?;
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF);
                            }
                            let ch = char::from_u32(cp).unwrap_or('\u{fffd}');
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        other => out.push(other),
                    }
                }
            }
        }
    }
    fn value(&mut self, ctx: Ctx) -> Option<V> {
        if ctx == Ctx::Skip {
            self.skip_value()?;
            return Some(V::Other);
        }
        self.ws();
        match *self.b.get(self.i)? {
            b'"' => Some(V::Str(self.string()?)),
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    if *self.b.get(self.i)? == b']' {
                        self.i += 1;
                        return Some(V::Arr(items));
                    }
                    items.push(self.value(ctx)?);
                    self.ws();
                    if *self.b.get(self.i)? == b',' {
                        self.i += 1;
                    }
                }
            }
            b'{' => {
                self.i += 1;
                let mut fields = Vec::new();
                loop {
                    self.ws();
                    match *self.b.get(self.i)? {
                        b'}' => {
                            self.i += 1;
                            return Some(V::Obj(fields));
                        }
                        b',' => {
                            self.i += 1;
                            continue;
                        }
                        _ => {}
                    }
                    // keys have no escapes in this data
                    let start = self.i + 1;
                    self.skip_string()?;
                    let key = &self.b[start..self.i - 1];
                    self.ws();
                    if *self.b.get(self.i)? != b':' {
                        return None;
                    }
                    self.i += 1;
                    let child = child_ctx(ctx, key);
                    if child == Ctx::Skip {
                        self.skip_value()?;
                    } else {
                        let v = self.value(child)?;
                        fields.push((String::from_utf8_lossy(key).into_owned(), v));
                    }
                }
            }
            _ => {
                self.skip_value()?;
                Some(V::Other)
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Words: the same folding as the engine (crates/verba-local/src/fold.rs), kept here because that module is private.
// ---------------------------------------------------------------------------------------------------------------------------------

fn is_arabic_mark(c: char) -> bool {
    matches!(c as u32, 0x064B..=0x065F | 0x0670 | 0x0640 | 0x06D6..=0x06ED)
}

fn is_arabic_letter(c: char) -> bool {
    matches!(c as u32, 0x0621..=0x063A | 0x0641..=0x064A | 0x0671..=0x06D3)
}

fn fold(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    for c in word.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'â' | 'ä' => out.push('a'),
            'ç' => out.push('c'),
            'é' | 'è' | 'ê' | 'ë' => out.push('e'),
            'î' | 'ï' => out.push('i'),
            'ô' | 'ö' => out.push('o'),
            'ù' | 'û' | 'ü' => out.push('u'),
            'ÿ' => out.push('y'),
            'œ' => out.push_str("oe"),
            'æ' => out.push_str("ae"),
            'أ' | 'إ' | 'آ' | 'ٱ' => out.push('ا'),
            'ة' => out.push('ه'),
            'ى' | 'ئ' => out.push('ي'),
            'ؤ' => out.push('و'),
            c if is_arabic_mark(c) => {}
            c => out.push(c),
        }
    }
    out
}

/// Arabic without short vowels, tatweel and the dagger alef (hamza and dots are kept: they are spelling).
fn bare(s: &str) -> String {
    s.chars().filter(|c| !is_arabic_mark(*c)).collect()
}

fn is_latin(c: char) -> bool {
    c.is_ascii_alphabetic() || "àâäçéèêëîïôöùûüÿœæÀÂÄÇÉÈÊËÎÏÔÖÙÛÜŸŒÆ".contains(c)
}

/// A word of the language `lang` (en, fr, ar): up to 3 words, only the letters of that language, apostrophes, hyphens and spaces.
fn script_ok(lang: &str, s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.chars().count() > 40 || s.split(' ').count() > 3 {
        return false;
    }
    let mut letters = 0;
    for c in s.chars() {
        if lang == "ar" {
            if is_arabic_letter(c) {
                letters += 1;
            } else if !(is_arabic_mark(c) || c == ' ' || c == '-') {
                return false;
            }
        } else if is_latin(c) {
            letters += 1;
        } else if !(c == ' ' || c == '-' || c == '\'' || c == '\u{2019}') {
            return false;
        }
    }
    // a lone letter between words ("amie f copine") is a grammar label that slipped into the data
    let lone = lang != "ar" && s.split(' ').count() > 1 && s.split(' ').any(|w| w.chars().count() == 1 && !matches!(w, "à" | "a" | "y" | "i" | "A" | "I"));
    letters >= 1 && !lone && !s.starts_with(['-', '\'', ' ']) && !s.ends_with(['-', '\'', ' '])
}

fn clean_field(s: &str) -> String {
    s.chars().map(|c| if c == '\t' || c == '\n' || c == '\r' { ' ' } else { c }).collect()
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Step 1: extract
// ---------------------------------------------------------------------------------------------------------------------------------

/// Sense tags that make a sense unwanted: rude (the rule is in docs/DATA.md) or not current.
const RUDE: &[&str] = &["vulgar", "offensive", "derogatory", "slur", "ethnic", "pejorative", "obscene", "sexual", "profanity", "ethnic-slur"];
const NOT_CURRENT: &[&str] = &["obsolete", "archaic", "historical", "rare", "misspelling", "nonstandard", "proscribed", "pronunciation-spelling", "eye-dialect", "uncommon", "neologism", "nonce-word", "misconstruction", "dialectal", "obsolete-form"];
const FORM_JUNK: &[&str] = &["table-tags", "inflection-template", "romanization", "multiword-construction", "class", "obsolete", "archaic", "dialectal", "rare", "nonstandard", "alternative", "pronunciation-spelling", "misspelling", "proscribed", "transliteration", "error-misspelling", "error-unknown-tag"];
const POS_OK: &[&str] = &["noun", "verb", "adj", "adv", "intj", "name", "phrase", "prep", "pron", "conj", "det", "num", "article", "particle", "contraction", "postp", "prep_phrase", "adv_phrase"];

fn has_any(tags: &[String], set: &[&str]) -> bool {
    tags.iter().any(|t| set.contains(&t.as_str()))
}

/// A translation's sense text and an English gloss say the same thing (loosely): used to drop translations of a rude sense.
fn same_sense(gloss: &str, sense: &str) -> bool {
    let norm = |s: &str| {
        let s = s.to_lowercase();
        let s = s.trim().trim_end_matches(['.', ':']).to_string();
        for p in ["a ", "an ", "the ", "to "] {
            if let Some(r) = s.strip_prefix(p) {
                return r.to_string();
            }
        }
        s
    };
    let (g, s) = (norm(gloss), norm(sense));
    s.len() >= 5 && (g == s || g.contains(&s) || (g.len() >= 8 && s.contains(&g)))
}

#[derive(Default)]
struct ExtractStats {
    lines: u64,
    bad_lines: u64,
    entries: u64,
    out_lines: u64,
}

fn extract(lang: &str, mut input: Box<dyn BufRead>, out: &mut dyn Write) -> std::io::Result<ExtractStats> {
    let mut st = ExtractStats::default();
    let mut buf: Vec<u8> = Vec::with_capacity(1 << 20);
    let (mut last_word, mut ord) = (String::new(), 0u32);
    loop {
        buf.clear();
        if input.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        st.lines += 1;
        let mut p = Parser { b: &buf, i: 0 };
        let Some(entry) = p.value(Ctx::Entry) else {
            st.bad_lines += 1;
            continue;
        };
        if entry.str("lang_code") != lang {
            continue;
        }
        let word = entry.str("word").trim();
        let pos = entry.str("pos");
        if !script_ok(lang, word) || !POS_OK.contains(&pos) {
            continue;
        }
        st.entries += 1;
        if word == last_word {
            ord += 1;
        } else {
            last_word = word.to_string();
            ord = 0;
        }
        let word = clean_field(word);
        let (mut n_sense, mut n_rude) = (0u32, 0u32);
        let mut rude_glosses: Vec<String> = Vec::new();
        let mut lines: Vec<String> = Vec::new();
        let mut tr_seen: Vec<(String, String)> = Vec::new();
        // the sense text of the entry-level translations, in order: the position of a text is the "group" of its translations
        let mut groups: Vec<String> = Vec::new();
        for (si, s) in entry.arr("senses").iter().enumerate() {
            let tags: Vec<String> = s.strings("tags").into_iter().chain(s.strings("raw_tags")).map(|t| t.to_lowercase().replace(' ', "-")).collect();
            let rude = has_any(&tags, RUDE);
            let stale = has_any(&tags, NOT_CURRENT);
            let mut lemmas: Vec<(&str, char)> = Vec::new();
            for l in s.arr("form_of") {
                lemmas.push((l.str("word"), 'o'));
            }
            for l in s.arr("alt_of") {
                lemmas.push((l.str("word"), 'a'));
            }
            if !lemmas.is_empty() || tags.iter().any(|t| t == "form-of" || t == "alt-of") {
                if !rude && !stale {
                    // French "comparative of bon (better)": the English word in brackets is a translation of this form
                    if lang == "fr" {
                        for l in s.arr("form_of") {
                            let extra = l.str("extra").trim().trim_matches('"');
                            if !extra.is_empty() && extra.chars().all(|c| c.is_ascii_alphabetic() || c == ' ') && extra.split(' ').count() <= 2 {
                                lines.push(format!("G\t{word}\t{pos}\t{ord}\t{si}\t{}\t{}", tags.join(","), clean_field(extra)));
                            }
                        }
                    }
                    for (l, kind) in lemmas {
                        let l = l.trim();
                        if script_ok(lang, l) && fold(l) != fold(&word) {
                            lines.push(format!("F\t{word}\t{}\t{pos}\t{kind}", clean_field(l)));
                        }
                    }
                }
                continue;
            }
            let glosses = s.strings("glosses");
            if glosses.is_empty() {
                continue;
            }
            n_sense += 1;
            if rude {
                n_rude += 1;
                if let Some(g) = glosses.last() {
                    rude_glosses.push(g.to_string());
                }
                continue;
            }
            if stale {
                continue;
            }
            if lang != "en" {
                let g: Vec<String> = glosses.iter().map(|g| clean_field(g)).collect();
                lines.push(format!("G\t{word}\t{pos}\t{ord}\t{si}\t{}\t{}", tags.join(","), g.join("\t")));
            }
            for t in s.arr("translations") {
                push_translation(t, si, &word, pos, ord, &mut tr_seen, &mut lines);
            }
        }
        if lang == "en" {
            for t in entry.arr("translations") {
                let sense = t.str("sense");
                let gi = match groups.iter().position(|g| g == sense) {
                    Some(i) => i,
                    None => {
                        groups.push(sense.to_string());
                        groups.len() - 1
                    }
                };
                if rude_glosses.iter().any(|g| same_sense(g, sense)) {
                    continue;
                }
                push_translation(t, gi, &word, pos, ord, &mut tr_seen, &mut lines);
            }
            lines.push(format!("A\t{word}\t{pos}\t{n_sense}\t{n_rude}"));
        }
        // inflected forms listed by the lemma
        let mut seen: HashSet<String> = HashSet::new();
        for f in entry.arr("forms") {
            let tags: Vec<String> = f.strings("tags").into_iter().map(str::to_string).collect();
            if has_any(&tags, FORM_JUNK) {
                continue;
            }
            // Arabic verb tables also list the verbal noun and the participles: words with a meaning of their own, not inflections of the verb
            if lang == "ar" && has_any(&tags, &["noun-from-verb", "participle"]) {
                continue;
            }
            let form = f.str("form").trim();
            if form.is_empty() || !script_ok(lang, form) || form.contains(" + ") {
                continue;
            }
            let key = fold(form);
            if key == fold(&word) || !seen.insert(key) {
                continue;
            }
            lines.push(format!("F\t{}\t{word}\t{pos}\tf", clean_field(form)));
        }
        for l in lines {
            out.write_all(l.as_bytes())?;
            out.write_all(b"\n")?;
            st.out_lines += 1;
        }
    }
    Ok(st)
}

fn push_translation(t: &V, group: usize, word: &str, pos: &str, ord: u32, seen: &mut Vec<(String, String)>, lines: &mut Vec<String>) {
    let lc = t.str("lang_code");
    if lc != "fr" && lc != "ar" {
        return;
    }
    let target = t.str("word").trim();
    if target.is_empty() || !script_ok(lc, target) {
        return;
    }
    let tags: Vec<String> = t.strings("tags").into_iter().map(|t| t.to_lowercase().replace(' ', "-")).collect();
    if has_any(&tags, RUDE) {
        return;
    }
    let key = (lc.to_string(), fold(target));
    if seen.contains(&key) {
        return;
    }
    seen.push(key);
    lines.push(format!("T\t{word}\t{pos}\t{ord}\t{group}\t{lc}\t{}\t{}\t{}", clean_field(target), tags.join(","), clean_field(t.str("sense"))));
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Step 2: build
// ---------------------------------------------------------------------------------------------------------------------------------

const LANGS: [&str; 3] = ["en", "fr", "ar"];
const MAX_TRANSLATIONS: usize = 5;
/// French and Arabic words commoner than this rank of their frequency list get a direct French<->Arabic line.
const DIRECT_FR_AR_RANK: u32 = 30_000;
const SRC_TABLE: usize = 0;
const SRC_GLOSS: usize = 1;
const SRC_GLOSS_BACK: usize = 2;

struct Cand {
    text: String,
    /// The strongest evidence of each kind: translation table, gloss of the headword's own entry, gloss read backwards.
    hits: [Vec<f32>; 3],
    count: u32,
}

impl Cand {
    /// The weight of the evidence of one kind: the strongest hit in full, the next ones counting less and less (the same translation
    /// in several senses of the word is a sign that it is a general translation, but a verb with three rare senses must not win).
    fn evidence(&self, src: usize) -> f32 {
        let mut h = self.hits[src].clone();
        h.sort_by(|a, b| b.partial_cmp(a).unwrap());
        h.iter().take(4).enumerate().map(|(i, x)| x * 0.5f32.powi(i as i32)).sum()
    }
}

#[derive(Default)]
struct Head {
    display: String,
    cands: HashMap<String, Cand>,
}

type Table = HashMap<String, Head>;

struct Freq {
    /// by the folded word (what the engine looks up)
    rank: HashMap<String, u32>,
    /// by the word as written (lower case, Arabic without short vowels): `علي` and `على` have one folded key but are different words
    exact: HashMap<String, u32>,
}

impl Freq {
    fn load(path: &Path) -> Self {
        let (mut rank, mut exact) = (HashMap::new(), HashMap::new());
        if let Ok(text) = std::fs::read_to_string(path) {
            for (i, line) in text.lines().enumerate() {
                if let Some(w) = line.split_whitespace().next() {
                    rank.entry(fold(w)).or_insert(i as u32);
                    exact.entry(bare(&w.to_lowercase())).or_insert(i as u32);
                }
            }
        }
        Freq { rank, exact }
    }
    fn rank_of(&self, text: &str) -> Option<(u32, bool)> {
        if let Some(&r) = self.exact.get(&bare(&text.to_lowercase())) {
            return Some((r, true));
        }
        self.rank.get(&fold(text)).map(|&r| (r, false))
    }
    /// 1 for the commonest word, 0 for a word that is not in the list (or at the end of it); a spelling that only matches after folding counts less.
    fn factor(&self, text: &str) -> f32 {
        match self.rank_of(text) {
            Some((r, exact)) => (1.0 - ((1 + r) as f32).ln() / 60_000f32.ln()).max(0.0) * if exact { 1.0 } else { 0.6 },
            None => 0.0,
        }
    }
    /// A word (or every word of a short phrase) of the list.
    fn knows(&self, text: &str) -> bool {
        text.split(' ').all(|w| self.rank_of(w).is_some())
    }
}

fn uppercase_count(s: &str) -> usize {
    s.chars().filter(|c| c.is_uppercase()).count()
}

fn add(table: &mut Table, head: &str, target: &str, score: f32, src: usize) {
    let hkey = fold(head);
    // the target is NOT folded: `suicidé` and `suicide`, `où` and `ou` are different translations (only case and Arabic vowels are ignored)
    let tkey = bare(&target.to_lowercase());
    if hkey.is_empty() || tkey.is_empty() {
        return;
    }
    // a head with capitals (`ON`, `Son`, `Maison`) is a name or an abbreviation: it must not outweigh the ordinary word of the same letters
    let score = if head.chars().any(char::is_uppercase) { score * 0.4 } else { score };
    let h = table.entry(hkey).or_default();
    if h.display.is_empty() || uppercase_count(head) < uppercase_count(&h.display) {
        h.display = head.to_string();
    }
    let c = h.cands.entry(tkey).or_insert_with(|| Cand { text: target.to_string(), hits: [Vec::new(), Vec::new(), Vec::new()], count: 0 });
    if uppercase_count(target) < uppercase_count(&c.text) {
        c.text = target.to_string();
    }
    c.hits[src].push(score);
    c.count += 1;
}

/// How much a sense or translation tagged like this counts: 0 drops it.
fn tag_factor(tags: &[&str], lang: &str) -> f32 {
    let mut f = 1.0f32;
    for t in tags {
        match *t {
            "obsolete" | "archaic" | "dated" | "rare" | "historical" | "transliteration" | "nonstandard" | "misspelling" | "proscribed" | "uncommon" => return 0.0,
            "slang" | "informal" | "colloquial" | "humorous" | "euphemistic" | "figuratively" | "idiomatic" | "dialectal" | "regional" => f = f.min(0.55),
            t if lang == "ar" && t.ends_with("-arabic") => {
                // Maghrebi dialects count a little (Algerian users), the other dialects do not: this is a Modern Standard Arabic dictionary
                if t.starts_with("moroccan") || t.starts_with("algerian") || t.starts_with("tunisian") || t.starts_with("maghrebi") || t.starts_with("hassaniya") {
                    f = f.min(0.5);
                } else if t != "classical-arabic" && t != "modern-standard-arabic" && t != "standard-arabic" {
                    return 0.0;
                }
            }
            "quebec" | "canada" | "belgium" | "switzerland" | "africa" | "us" | "uk" | "australia" | "india" => f = f.min(0.7),
            _ => {}
        }
    }
    f
}

fn rude_tag(tags: &[&str]) -> bool {
    tags.iter().any(|t| RUDE.contains(t))
}

fn pos_weight(ord: u32) -> f32 {
    1.0 / (1.0 + 0.2 * ord as f32)
}
fn group_weight(g: usize) -> f32 {
    1.0 / (1.0 + 0.3 * g as f32)
}

// ---- glosses -> short translations ----

const REJECT_WORDS: &[&str] = &[
    "that", "which", "who", "whose", "whom", "used", "usually", "especially", "something", "someone", "somebody", "anything", "certain", "various", "being", "having", "etc", "e.g", "i.e", "surname", "forename", "given", "name", "named", "female", "male", "town", "village", "commune", "municipality", "river", "department", "region", "province", "city", "also", "whether", "whatever", "typically", "generally", "often", "sometimes", "if", "when", "where", "how", "because", "followed", "preceded", "indicating", "denoting", "referring", "meaning", "expressing", "representing", "designating", "forming", "verb", "noun", "adjective", "adverb", "preposition", "particle", "conjunction", "pronoun", "jussive", "subjunctive", "belong", "belongs", "things", "its", "his", "her", "their", "our", "my", "your", "one's", "someone's", "somebody's",
];
const GENERIC_HEADS: &[&str] = &["act", "state", "process", "quality", "condition", "form", "type", "kind", "piece", "member", "sort", "variety", "means", "person", "instance", "example", "part", "set", "group", "collection", "number", "amount", "way", "manner", "result", "fact", "action", "practice", "method", "system", "case", "lack", "feminine", "masculine", "plural", "singular", "alternative", "abbreviation", "initialism", "synonym", "contraction", "diminutive", "augmentative", "inflection", "short", "obsolete", "dated", "nonstandard", "misspelling", "past", "present", "future", "first-person", "second-person", "third-person", "imperative", "subjunctive", "indicative", "infinitive", "participle", "gerund", "comparative", "superlative", "relating", "pertaining", "belonging", "resembling", "characterized", "capable", "full"];

fn strip_brackets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' | '[' | '{' | '“' | '«' => depth += 1,
            ')' | ']' | '}' | '”' | '»' => depth = (depth - 1).max(0),
            c if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// The short translations hidden in one English gloss ("greetings; hello (general salutation)" -> greetings, hello). Strict:
/// one to three plain words, nothing that reads like a definition.
fn gloss_atoms(gloss: &str, verb: bool) -> Vec<String> {
    let g = strip_brackets(gloss);
    let mut out = Vec::new();
    for part in g.split(|c| c == ';' || c == ',' || c == '/' || c == '|').flat_map(|p| p.split(" or ")) {
        let mut a = part.trim().trim_end_matches(['.', ':', '!', '?', ' ']).trim().to_string();
        if a.is_empty() {
            continue;
        }
        let lower = a.to_lowercase();
        let mut cut = 0;
        for p in ["to ", "a ", "an ", "the "] {
            if lower.starts_with(p) {
                cut = p.len();
                break;
            }
        }
        if cut == 3 && !verb && lower.starts_with("to ") {
            // "to ..." on a word that is not a verb is a purpose clause, not a translation
            continue;
        }
        a = a[cut..].trim().to_string();
        let words: Vec<&str> = a.split(' ').filter(|w| !w.is_empty()).collect();
        if words.is_empty() || words.len() > 3 {
            continue;
        }
        let joined = words.join(" ");
        if joined.chars().count() < 2 || !joined.chars().all(|c| c.is_alphabetic() || c == ' ' || c == '-' || c == '\'' || c == '’') || !joined.chars().any(|c| c.is_alphabetic()) {
            continue;
        }
        if !joined.chars().all(|c| is_latin(c) || c == ' ' || c == '-' || c == '\'' || c == '’') {
            continue;
        }
        let lw: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        // a capital letter on a function word ("In particular", "Of life") is the start of a sentence, not a translation
        if words.len() > 1 && words[0].chars().next().is_some_and(char::is_uppercase) && matches!(lw[0].as_str(), "in" | "on" | "at" | "of" | "for" | "with" | "by" | "as" | "to" | "and" | "or" | "from" | "when" | "while" | "if" | "the" | "an" | "a" | "that" | "this" | "such" | "especially" | "particularly" | "usually" | "often" | "used") {
            continue;
        }
        if words.len() == 1 {
            if matches!(lw[0].as_str(), "used" | "etc" | "also" | "alternative" | "abbreviation" | "initialism" | "see" | "synonym" | "form" | "plural" | "feminine" | "masculine" | "surname" | "forename" | "toponym" | "name" | "patronymic" | "unisex" | "ellipsis" | "short" | "clipping" | "variant") {
                continue;
            }
        } else {
            if lw.iter().any(|w| REJECT_WORDS.contains(&w.as_str())) {
                continue;
            }
            if lw.iter().any(|w| w.ends_with("'s") || w.ends_with("’s")) {
                continue;
            }
            if GENERIC_HEADS.contains(&lw[0].as_str()) {
                continue;
            }
            if lw.last().is_some_and(|w| matches!(w.as_str(), "of" | "the" | "a" | "an" | "with" | "and")) && lw.len() > 1 && lw.last().map(String::as_str) != Some("of") {
                continue;
            }
        }
        // "to be or become good" would leave a bare "be": that is half of a definition, not a translation
        if matches!(lw.as_slice(), [w] if w == "be" || w == "become" || w == "do" || w == "make") && g.split_whitespace().count() > 2 {
            continue;
        }
        out.push(joined);
    }
    out
}

fn read_lines(path: &Path) -> Vec<String> {
    let text = std::fs::read(path).unwrap_or_else(|e| {
        eprintln!("cannot read {}: {e} (run scripts/fetch-dict.ps1)", path.display());
        std::process::exit(1);
    });
    String::from_utf8_lossy(&text).lines().map(str::to_string).collect()
}

struct Report(Vec<String>);
impl Report {
    fn line(&mut self, s: String) {
        println!("{s}");
        self.0.push(s);
    }
}

fn a_static(l: &str) -> &'static str {
    lang_static(l)
}

fn lang_static(l: &str) -> &'static str {
    match l {
        "en" => "en",
        "fr" => "fr",
        _ => "ar",
    }
}

fn build(src: &Path, words: &Path, out: &Path, max_mb: f64, report_path: Option<&Path>, with_direct_fr_ar: bool) {
    let mut rep = Report(Vec::new());
    let freq: HashMap<&str, Freq> = LANGS.iter().map(|l| (*l, Freq::load(&words.join(format!("{l}.txt"))))).collect();
    let mut dirs: HashMap<(&'static str, &'static str), Table> = HashMap::new();
    let mut forms: HashMap<&'static str, HashMap<String, Vec<(String, String)>>> = HashMap::new();

    // ---- English extract: translation tables, and which English words are rude only
    let en_lines = read_lines(&src.join("dict-en.tsv"));
    let mut sense_count: HashMap<String, (u32, u32)> = HashMap::new();
    for l in &en_lines {
        let f: Vec<&str> = l.split('\t').collect();
        if f[0] == "A" && f.len() >= 5 {
            let e = sense_count.entry(fold(f[1])).or_default();
            e.0 += f[3].parse::<u32>().unwrap_or(0);
            e.1 += f[4].parse::<u32>().unwrap_or(0);
        }
    }
    let rude_only: HashSet<&String> = sense_count.iter().filter(|(_, (n, r))| *n > 0 && r * 3 >= n * 2).map(|(k, _)| k).collect();
    let known_en = |key: &str| sense_count.get(key).is_some_and(|(n, r)| r < n) || freq["en"].rank.contains_key(key);
    let mut n_tr = 0u64;
    let mut tr_pairs: HashMap<&str, u64> = HashMap::new();
    for l in &en_lines {
        let f: Vec<&str> = l.split('\t').collect();
        match f[0] {
            "T" if f.len() >= 9 => {
                let (w, ord, g, tl, target) = (f[1], f[3].parse::<u32>().unwrap_or(0), f[4].parse::<usize>().unwrap_or(0), lang_static(f[5]), f[6]);
                if rude_only.contains(&fold(w)) {
                    continue;
                }
                let tags: Vec<&str> = f[7].split(',').filter(|t| !t.is_empty()).collect();
                let factor = tag_factor(&tags, tl);
                if factor == 0.0 || rude_tag(&tags) {
                    continue;
                }
                let target = if tl == "ar" { bare(target) } else { target.to_string() };
                let w_disp = w.to_string();
                let base = pos_weight(ord) * group_weight(g) * factor;
                add(dirs.entry(("en", tl)).or_default(), &w_disp, &target, base, SRC_TABLE);
                add(dirs.entry((tl, "en")).or_default(), &target, &w_disp, base * 0.85, SRC_TABLE);
                n_tr += 1;
                *tr_pairs.entry(tl).or_default() += 1;
            }
            "F" if f.len() >= 5 => {
                forms.entry("en").or_default().entry(fold(f[1])).or_default().push((f[1].to_string(), fold(f[2])));
            }
            _ => {}
        }
    }
    rep.line(format!("English extract: {} lines, {n_tr} translation pairs ({:?})", en_lines.len(), tr_pairs));

    // ---- French and Arabic extracts: glosses
    let mut gloss_heads: HashMap<&str, u64> = HashMap::new();
    let mut gloss_atoms_kept: HashMap<&str, u64> = HashMap::new();
    for l in ["fr", "ar"] {
        let lines = read_lines(&src.join(format!("dict-{l}.tsv")));
        let l = lang_static(l);
        let mut with_atoms: HashSet<String> = HashSet::new();
        let mut all_heads: HashSet<String> = HashSet::new();
        for line in &lines {
            let f: Vec<&str> = line.split('\t').collect();
            match f[0] {
                "G" if f.len() >= 7 => {
                    let (w, pos, ord, si) = (f[1], f[2], f[3].parse::<u32>().unwrap_or(0), f[4].parse::<usize>().unwrap_or(0));
                    all_heads.insert(fold(w));
                    let tags: Vec<&str> = f[5].split(',').filter(|t| !t.is_empty()).collect();
                    let factor = tag_factor(&tags, "en") * if pos == "name" { 0.4 } else { 1.0 };
                    if factor == 0.0 || rude_tag(&tags) {
                        continue;
                    }
                    let head = if l == "ar" { bare(w) } else { w.to_string() };
                    for (li, g) in f[6..].iter().enumerate() {
                        let lw = if li == 0 { 1.0 } else { 0.8 };
                        for (ai, atom) in gloss_atoms(g, pos == "verb").into_iter().enumerate() {
                            let score = pos_weight(ord) * group_weight(si) * factor * lw / (1.0 + 0.4 * ai as f32);
                            let akey = fold(&atom);
                            if rude_only.contains(&akey) {
                                continue;
                            }
                            add(dirs.entry((l, "en")).or_default(), &head, &atom, score, SRC_GLOSS);
                            // read backwards (English word -> this word): only for English words that are known words
                            if known_en(&akey) {
                                add(dirs.entry(("en", l)).or_default(), &atom, &head, score * 0.8, SRC_GLOSS_BACK);
                            }
                            with_atoms.insert(fold(w));
                            *gloss_atoms_kept.entry(l).or_default() += 1;
                        }
                    }
                }
                "F" if f.len() >= 5 => {
                    forms.entry(l).or_default().entry(fold(f[1])).or_default().push((f[1].to_string(), fold(f[2])));
                }
                _ => {}
            }
        }
        gloss_heads.insert(l, all_heads.len() as u64);
        let dropped_common = freq[l].rank.iter().filter(|(k, r)| **r < 20_000 && all_heads.contains(*k) && !with_atoms.contains(*k)).count();
        rep.line(format!("{l}: {} headwords with glosses, {} got at least one short gloss; among the 20 000 commonest words of the list, {dropped_common} have glosses but none short enough", all_heads.len(), with_atoms.len()));
    }

    // ---- final lists: score, cut, then forms
    /// headword -> (written form, translations with their scores, for an inflected form the rank of its commonest lemma in the frequency list)
    type Final = HashMap<String, (String, Vec<(String, f32)>, Option<u32>)>;
    let mut finals: HashMap<(&'static str, &'static str), Final> = HashMap::new();
    for (&(from, to), table) in &dirs {
        let tf = &freq[to];
        let mut out_map: Final = HashMap::new();
        for (hkey, head) in table {
            if !script_head_ok(from, &head.display) || head_dropped(from, hkey) {
                continue;
            }
            let mut v: Vec<(f32, &Cand)> = head
                .cands
                .values()
                .map(|c| {
                    // the main evidence of the direction, plus part of the other kind (two sources agreeing is the best sign)
                    let [t, g, b] = [c.evidence(0), c.evidence(1), c.evidence(2)];
                    let (p, q) = if to == "en" { (g, 0.9 * t) } else { (t, 0.9 * b) };
                    let e = p.max(q) + 0.45 * p.min(q) + 0.04 * (c.count as f32).ln();
                    let known = tf.knows(&c.text);
                    // the same word in both languages (`suicide`, `magazine`) is listed only when the data says so: a little extra
                    let cognate = if from != "ar" && to != "ar" && fold(&c.text) == *hkey { 0.3 } else { 0.0 };
                    (e * if known { 1.0 } else { 0.5 } + tf.factor(&c.text) + cognate, c)
                })
                .collect();
            v.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap().then_with(|| a.1.text.cmp(&b.1.text)));
            if std::env::var("DICT_DEBUG").is_ok_and(|w| w == format!("{from}>{to}:{hkey}")) {
                for (s, c) in v.iter().take(12) {
                    eprintln!("  {from}>{to} {hkey}: {:<14} final {s:.2} evidence {:?} n {}", c.text, [c.evidence(0), c.evidence(1), c.evidence(2)], c.count);
                }
            }
            let best = v.first().map(|x| x.0).unwrap_or(0.0);
            let list: Vec<(String, f32)> = v.iter().filter(|x| x.0 >= 0.3 * best).take(MAX_TRANSLATIONS).map(|x| (x.1.text.clone(), x.0)).collect();
            if !list.is_empty() {
                out_map.insert(hkey.clone(), (if from == "ar" { bare(&head.display) } else { head.display.clone() }, list, None));
            }
        }
        finals.insert((from, to), out_map);
    }
    let mut lemma_counts: HashMap<(&'static str, &'static str), usize> = finals.iter().map(|(k, v)| (*k, v.len())).collect();
    let mut lemma_counts_extra: Vec<((&'static str, &'static str), usize)> = Vec::new();

    // inflected forms: a form that is not a headword gets the translations of its lemma(s); a form that is a word of its own (`saw`)
    // keeps its own translations too, and all of them are put in order of their scores
    let mut form_added: HashMap<(&'static str, &'static str), usize> = HashMap::new();
    for (&(from, to), map) in finals.iter_mut() {
        let Some(fm) = forms.get(from) else { continue };
        let mut adds: Vec<(String, String, Vec<(String, f32)>, u32)> = Vec::new();
        for (fkey, lemmas) in fm {
            if fkey.contains(' ') || head_dropped(from, fkey) {
                continue;
            }
            let own_list: Vec<(String, f32)> = map.get(fkey).map(|(_, l, _)| l.clone()).unwrap_or_default();
            let mut pool: Vec<(String, f32)> = own_list.clone();
            let mut seen_lemma: Vec<&String> = Vec::new();
            let mut disp = String::new();
            let mut lemma_rank = 1_000_000u32;
            for (d, lk) in lemmas {
                if lk.contains(' ') || seen_lemma.contains(&lk) {
                    continue;
                }
                seen_lemma.push(lk);
                lemma_rank = lemma_rank.min(freq[from].rank.get(lk).copied().unwrap_or(1_000_000));
                if disp.is_empty() || uppercase_count(d) < uppercase_count(&disp) {
                    disp = d.clone();
                }
                if let Some((_, ll, _)) = map.get(lk) {
                    for (t, sc) in ll {
                        let w = if own_list.is_empty() { 0.95 } else { 0.6 };
                        match pool.iter_mut().find(|(x, _)| x == t) {
                            Some(e) => e.1 = e.1.max(*sc * w),
                            None => pool.push((t.clone(), *sc * w)),
                        }
                    }
                }
            }
            pool.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(&b.0)));
            pool.truncate(MAX_TRANSLATIONS);
            if pool != own_list {
                adds.push((fkey.clone(), if from == "ar" { bare(&disp) } else { disp }, pool, lemma_rank));
            }
        }
        let n = adds.len();
        for (k, d, l, lr) in adds {
            match map.get_mut(&k) {
                Some(e) => e.1 = l,
                None => {
                    map.insert(k, (d, l, Some(lr)));
                }
            }
        }
        form_added.insert((from, to), n);
    }

    // ---- French <-> Arabic directly: the translations of the first English translation(s) (the engine would also go through English,
    // but with up to 3 English words, which lets the weak senses in)
    if with_direct_fr_ar {
        for (a, b, c) in [("fr", "en", "ar"), ("ar", "en", "fr")] {
            let (Some(ab), Some(bc)) = (finals.get(&(a, b)), finals.get(&(b, c))) else { continue };
            let mut out_map: Final = HashMap::new();
            for (k, (d, l, form_of)) in ab {
                // only headwords (a form goes through its lemma's English words at run time) among the commonest of the language
                let top = l.first().map_or(0.0, |x| x.1);
                if top <= 0.0 || form_of.is_some() || freq[a].rank.get(k).is_none_or(|&r| r >= DIRECT_FR_AR_RANK) {
                    continue;
                }
                let mut pool: Vec<(String, f32)> = Vec::new();
                for (m, ms) in l.iter().filter(|x| x.1 >= 0.75 * top).take(2) {
                    let Some((_, cl, _)) = bc.get(&fold(m)) else { continue };
                    for (t, ts) in cl {
                        let w = (ms / top).powi(2) * ts;
                        match pool.iter_mut().find(|(x, _)| x == t) {
                            Some(e) => e.1 = e.1.max(w),
                            None => pool.push((t.clone(), w)),
                        }
                    }
                }
                pool.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap().then_with(|| x.0.cmp(&y.0)));
                pool.truncate(MAX_TRANSLATIONS);
                if !pool.is_empty() {
                    out_map.insert(k.clone(), (d.clone(), pool, *form_of));
                }
            }
            lemma_counts_extra.push(((a_static(a), a_static(c)), out_map.values().filter(|v| v.2.is_none()).count()));
            finals.insert((a_static(a), a_static(c)), out_map);
        }
    }

    for (k, n) in lemma_counts_extra {
        lemma_counts.insert(k, n);
    }

    // ---- size: keep the commonest headwords first while the budget allows
    let budget = (max_mb * 1024.0 * 1024.0) as usize;
    let mut lines: Vec<(f32, usize, String, String)> = Vec::new(); // priority, direction index, headword key, line
    let order = [("en", "fr"), ("fr", "en"), ("en", "ar"), ("ar", "en"), ("fr", "ar"), ("ar", "fr")];
    let mut total = 0usize;
    for (di, &(from, to)) in order.iter().enumerate() {
        let Some(map) = finals.get(&(from, to)) else { continue };
        let ff = &freq[from];
        for (k, (d, l, form_of)) in map {
            let line = format!("{from}>{to}	{d}	{}
", l.iter().map(|x| x.0.as_str()).collect::<Vec<_>>().join("; "));
            // priority (lower is more important): headwords first, by rank in the language's list (words that are not in the list
            // after, one-word before several-word); then the forms that are words of the list; then the other forms, those of the
            // commonest lemmas first
            let prio = match form_of {
                None => match ff.rank.get(k) {
                    Some(&r) => r as f32,
                    None => 1_000_000.0 + if k.contains(' ') { 500_000.0 } else { 0.0 } + k.len() as f32,
                },
                Some(lr) => match ff.rank.get(k) {
                    Some(&r) => 2_000_000.0 + r as f32,
                    None => 3_000_000.0 + *lr as f32,
                },
            };
            total += line.len();
            lines.push((prio, di, k.clone(), line));
        }
    }
    {
        let mut by_tier = [(0usize, 0usize); 3]; // lines and bytes: headwords, forms in the frequency list, other forms
        for (prio, _, _, line) in &lines {
            let t = if *prio < 2_000_000.0 { 0 } else if *prio < 3_000_000.0 { 1 } else { 2 };
            by_tier[t].0 += 1;
            by_tier[t].1 += line.len();
        }
        rep.line(format!(
            "by kind: headwords {} lines {:.1} MB; forms of listed words {} lines {:.1} MB; other forms {} lines {:.1} MB",
            by_tier[0].0, by_tier[0].1 as f64 / 1048576.0, by_tier[1].0, by_tier[1].1 as f64 / 1048576.0, by_tier[2].0, by_tier[2].1 as f64 / 1048576.0
        ));
    }
    rep.line(format!("all candidates: {} lines, {:.1} MB (budget {max_mb} MB)", lines.len(), total as f64 / 1048576.0));
    lines.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap().then(a.1.cmp(&b.1)).then_with(|| a.2.cmp(&b.2)));
    let mut kept: Vec<(usize, String, String)> = Vec::new();
    let mut size = 0usize;
    let mut cut = 0usize;
    for (_, di, k, line) in lines {
        if size + line.len() > budget {
            cut += 1;
            continue;
        }
        size += line.len();
        kept.push((di, k, line));
    }
    kept.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let mut per_dir = [0usize; 6];
    for k in &kept {
        per_dir[k.0] += 1;
    }

    let header = "# Verba bilingual dictionary (English-French, English-Arabic, and French-Arabic worked out from them). One line per headword: direction<TAB>headword<TAB>translations separated by ';', most common first.\n\
# Built by crates/verba-local/examples/build_dict.rs from Wiktionary data extracted by wiktextract (https://kaikki.org), (c) Wiktionary contributors,\n\
# licensed CC BY-SA 4.0 and GFDL (https://en.wiktionary.org/wiki/Wiktionary:Copyrights). Changed: entries reduced to short word translations, ranked, inflected forms added.\n\
# This file is a derivative and stays under CC BY-SA 4.0. Arabic is written without short vowels. See docs/DATA.md.\n";
    if let Some(d) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(d).expect("cannot create the output folder");
    }
    let mut w = BufWriter::new(File::create(out).expect("cannot write dict.txt"));
    w.write_all(header.as_bytes()).unwrap();
    for (_, _, line) in &kept {
        w.write_all(line.as_bytes()).unwrap();
    }
    w.flush().unwrap();
    for (di, &(from, to)) in order.iter().enumerate() {
        let lemmas = lemma_counts.get(&(from, to)).copied().unwrap_or(0);
        let fa = form_added.get(&(from, to)).copied().unwrap_or(0);
        rep.line(format!("{from}>{to}: {lemmas} headwords from translations or glosses + {fa} forms/updated forms; written {}", per_dir[di]));
    }
    rep.line(format!("written: {} lines, {:.2} MB ({cut} lines left out for the size budget) -> {}", kept.len(), (size + header.len()) as f64 / 1048576.0, out.display()));
    if let Some(rp) = report_path {
        let _ = std::fs::write(rp, rep.0.join("\n") + "\n");
    }
}

/// One-letter Latin headwords are letters, not words (`t`, `u`), except the real one-letter words.
fn head_dropped(lang: &str, key: &str) -> bool {
    lang != "ar" && key.chars().count() == 1 && !matches!(key, "a" | "i" | "y" | "o" | "à")
}

fn script_head_ok(lang: &str, head: &str) -> bool {
    script_ok(lang, head)
}

// ---------------------------------------------------------------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/words");
    let get = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    match args.first().map(String::as_str) {
        Some("extract") => {
            let lang = get("--lang").unwrap_or_default();
            if !LANGS.contains(&lang.as_str()) {
                eprintln!("usage: build_dict extract --lang en|fr|ar [--in <jsonl file>] --out <tsv>   (reads standard input without --in)");
                std::process::exit(2);
            }
            let Some(out) = get("--out") else {
                eprintln!("--out is needed");
                std::process::exit(2);
            };
            let input: Box<dyn BufRead> = match get("--in") {
                Some(p) if p != "-" => Box::new(BufReader::with_capacity(1 << 20, File::open(&p).unwrap_or_else(|e| {
                    eprintln!("cannot open {p}: {e}");
                    std::process::exit(1);
                }))),
                _ => Box::new(BufReader::with_capacity(1 << 20, std::io::stdin().lock())),
            };
            let out = PathBuf::from(out);
            if let Some(d) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(d).expect("cannot create the output folder");
            }
            let mut w = BufWriter::new(File::create(&out).expect("cannot write the extract"));
            let st = extract(&lang, input, &mut w).unwrap_or_else(|e| {
                eprintln!("extract failed: {e}");
                std::process::exit(1);
            });
            w.flush().unwrap();
            println!("{lang}: {} lines read ({} unreadable), {} entries kept, {} records -> {}", st.lines, st.bad_lines, st.entries, st.out_lines, out.display());
            if st.lines < 1000 || st.entries < 1000 || st.bad_lines * 100 > st.lines {
                eprintln!("the input does not look like a kaikki.org dictionary (too few entries, or too many unreadable lines)");
                std::process::exit(1);
            }
        }
        Some("build") => {
            let src = get("--src").map(PathBuf::from).unwrap_or_else(|| root.join("src"));
            let words = get("--words").map(PathBuf::from).unwrap_or(root.clone());
            let out = get("--out").map(PathBuf::from).unwrap_or_else(|| root.join("dict.txt"));
            let max_mb: f64 = get("--max-mb").and_then(|s| s.parse().ok()).unwrap_or(14.0);
            let report = get("--report").map(PathBuf::from);
            let direct = !args.iter().any(|a| a == "--no-fr-ar");
            build(&src, &words, &out, max_mb, report.as_deref(), direct);
        }
        _ => {
            eprintln!("usage:\n  build_dict extract --lang en|fr|ar [--in <jsonl>] --out <tsv>\n  build_dict build [--src dir] [--words dir] [--out dict.txt] [--max-mb 14] [--report file]");
            std::process::exit(2);
        }
    }
}
