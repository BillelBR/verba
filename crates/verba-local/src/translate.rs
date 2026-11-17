//! Word translation from a bilingual dictionary, offline.
//!
//! The dictionary is one text file, `dict.txt`, built from open data by `examples/build_dict.rs` (see `docs/DATA.md`). One line per
//! headword and direction:
//!
//! ```text
//! # comments start with #
//! en>fr<TAB>hello<TAB>bonjour; salut; allô
//! fr>en<TAB>bonjour<TAB>hello; good morning
//! en>ar<TAB>hello<TAB>مرحبا; أهلا
//! ```
//!
//! Directions are written with the language codes (`en`, `fr`, `ar`). Translations are in order of how common they are. A headword
//! may be an inflected form (`dogs`) that carries the translations of its lemma, so a plural or a conjugated verb is found too.
//! Between two languages that have no direct list (French and Arabic) the lookup goes through English.
//!
//! Words are looked up without case, accents, hamza or short vowels (the same folding as the suggestions), so `cafe` finds `café`.

use std::collections::HashMap;

use verba_core::{Lang, Suggestion, is_arabic_word, match_case};

use crate::LocalEngine;
use crate::fold::fold;

/// How many translations one lookup gives at most.
const MAX_TRANSLATIONS: usize = 5;
/// Through English: how many English words of the middle step are followed.
const PIVOT_WIDTH: usize = 3;

/// All the directions of the dictionary.
#[derive(Default)]
pub struct Dictionary {
    pairs: HashMap<(Lang, Lang), HashMap<String, Vec<String>>>,
    /// Algerian Darija written in Latin letters (a small glossary shipped with the program).
    latin: LatinGlossary,
}

/// One word of the Latin Darija glossary, with its translations.
#[derive(Default)]
struct LatinEntry {
    en: Vec<String>,
    fr: Vec<String>,
    ar: Vec<String>,
}

/// Darija in Latin letters (`wach`, `bezzaf`, `3andi`): there is no standard spelling, so a word is found by the spellings listed for it,
/// then by its consonants alone (`bzaf`, `bezaf`, `bzzaf` all have `bzf`), unless two entries share them.
#[derive(Default)]
struct LatinGlossary {
    entries: Vec<LatinEntry>,
    exact: HashMap<String, usize>,
    /// consonant skeleton -> entry; `None` when two entries have the same skeleton (then it is not used)
    skeleton: HashMap<String, Option<usize>>,
    /// spellings (keys) that are Darija even though English or French has the same word (marked with `!` in the file)
    priority: std::collections::HashSet<String>,
}

/// A Latin word as a key: lower case, letters and digits only, doubled letters made single (`bezzaf` -> `bezaf`).
fn latin_key(word: &str) -> String {
    let mut out = String::new();
    for c in word.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() && out.chars().last() != Some(c) {
            out.push(c);
        }
    }
    out
}

/// The key without vowels (`bezaf` -> `bzf`); digits stay, they stand for consonants.
fn skeleton(key: &str) -> String {
    key.chars().filter(|c| !"aeiouy".contains(*c)).collect()
}

impl LatinGlossary {
    fn parse(text: &str) -> Self {
        let mut g = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('|').map(str::trim).collect();
            if fields.len() < 4 {
                continue;
            }
            let list = |f: &str| -> Vec<String> { if f == "-" { vec![] } else { f.split(';').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect() } };
            let index = g.entries.len();
            g.entries.push(LatinEntry { en: list(fields[1]), fr: list(fields[2]), ar: list(fields[3]) });
            for spelling in fields[0].split(',') {
                let spelling = spelling.trim();
                let first = spelling.starts_with('!');
                let key = latin_key(spelling);
                if first && !key.is_empty() {
                    g.priority.insert(key.clone());
                }
                if key.is_empty() {
                    continue;
                }
                g.exact.entry(key.clone()).or_insert(index);
                let sk = skeleton(&key);
                if sk.chars().count() >= 3 {
                    match g.skeleton.get(&sk) {
                        Some(Some(other)) if *other != index => {
                            g.skeleton.insert(sk, None);
                        }
                        Some(_) => {}
                        None => {
                            g.skeleton.insert(sk, Some(index));
                        }
                    }
                }
            }
        }
        g
    }

    fn find(&self, word: &str) -> Option<&LatinEntry> {
        let key = latin_key(word);
        if let Some(&i) = self.exact.get(&key) {
            return self.entries.get(i);
        }
        let sk = skeleton(&key);
        if sk.chars().count() >= 3 {
            if let Some(Some(i)) = self.skeleton.get(&sk) {
                return self.entries.get(*i);
            }
        }
        None
    }
}

/// The answer to a lookup.
#[derive(Clone, Debug, PartialEq)]
pub struct Translation {
    /// The language the word was found in, and the one it was translated into.
    pub from: Lang,
    pub to: Lang,
    /// The word as it was asked.
    pub word: String,
    /// The translations, most common first, in the target language.
    pub candidates: Vec<Suggestion>,
    /// The way went through English because there is no direct list between the two languages.
    pub via_english: bool,
}

fn lang_of(code: &str) -> Option<Lang> {
    match code {
        "en" => Some(Lang::En),
        "fr" => Some(Lang::Fr),
        "ar" => Some(Lang::Ar),
        _ => None,
    }
}

/// The word without the punctuation around it (a selection often carries a comma or a full stop).
fn clean(word: &str) -> &str {
    word.trim_matches(|c: char| c.is_whitespace() || (!c.is_alphanumeric() && c != '\'' && c != '-' && c != '\u{2019}'))
}

impl Dictionary {
    /// Reads the text of `dict.txt`. A line that is not understood is skipped.
    pub fn parse(text: &str) -> Self {
        let mut dict = Self::default();
        for line in text.lines() {
            let line = line.trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split('\t');
            let (Some(dir), Some(head), Some(tr)) = (fields.next(), fields.next(), fields.next()) else { continue };
            let Some((a, b)) = dir.split_once('>') else { continue };
            let (Some(from), Some(to)) = (lang_of(a), lang_of(b)) else { continue };
            let key = fold(clean(head));
            if key.is_empty() {
                continue;
            }
            let list = dict.pairs.entry((from, to)).or_default().entry(key).or_default();
            for t in tr.split(';').map(str::trim).filter(|t| !t.is_empty()) {
                if !list.iter().any(|x| x == t) {
                    list.push(t.to_string());
                }
            }
        }
        dict
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// `parse`, then the hand-written corrections that ship with the program (`seed/dict-fixes.txt`). This is what the engine loads.
    pub fn parse_with_fixes(text: &str) -> Self {
        let mut dict = Self::parse(text);
        dict.apply_fixes(include_str!("../seed/dict-fixes.txt"));
        dict.latin = LatinGlossary::parse(include_str!("../seed/dz-latin-glossary.txt"));
        dict
    }

    /// Puts hand-written translations in front of the ones a headword already has (same file format as `parse`).
    pub fn apply_fixes(&mut self, text: &str) {
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split('\t');
            let (Some(dir), Some(head), Some(tr)) = (fields.next(), fields.next(), fields.next()) else { continue };
            let Some((a, b)) = dir.split_once('>') else { continue };
            let (Some(from), Some(to)) = (lang_of(a), lang_of(b)) else { continue };
            let key = fold(clean(head));
            let mut list: Vec<String> = tr.split(';').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
            let old = self.pairs.entry((from, to)).or_default().entry(key).or_default();
            list.extend(old.drain(..).filter(|t| !tr.split(';').any(|f| f.trim() == t)));
            *old = list;
        }
    }

    /// How many headwords the direction has.
    pub fn len(&self, from: Lang, to: Lang) -> usize {
        self.pairs.get(&(from, to)).map_or(0, |m| m.len())
    }

    fn direct(&self, from: Lang, to: Lang, word: &str) -> Option<&Vec<String>> {
        self.pairs.get(&(from, to))?.get(&fold(word)).filter(|v| !v.is_empty())
    }

    /// An Arabic word with its attached little words taken off: the article and the one-letter prepositions in front (`والكتاب`,
    /// `بالبيت`, `للمدرسة`) and the pronoun endings behind (`سيارتك`, `رأيتك`). The first form the dictionary knows wins; the word itself
    /// is tried first, so a real word is never mistaken for a form of another one.
    fn direct_arabic(&self, from: Lang, to: Lang, word: &str) -> Option<&Vec<String>> {
        if let Some(list) = self.direct(from, to, word) {
            return Some(list);
        }
        let folded = fold(word);
        let chars: Vec<char> = folded.chars().collect();
        const PREFIXES: [&str; 15] = ["", "وال", "بال", "كال", "فال", "لل", "ال", "و", "ف", "ب", "ك", "ل", "س", "وب", "ول"];
        const SUFFIXES: [&str; 16] = ["", "اتها", "اتهم", "اتنا", "اتك", "اته", "اتي", "ك", "ه", "ها", "هم", "هن", "كم", "نا", "ي", "ات"];
        for prefix in PREFIXES {
            let pl = prefix.chars().count();
            let starts = chars.len() > pl && chars.iter().take(pl).copied().eq(prefix.chars());
            let rests: Vec<String> = if starts { vec![chars[pl..].iter().collect()] } else { vec![] };
            // `للمدرسة` is `ل` + `ال` + `مدرسة`: the article lost its alef after the preposition
            let rests = if prefix == "لل" && starts { vec![chars[pl..].iter().collect(), format!("ال{}", chars[pl..].iter().collect::<String>())] } else { rests };
            for rest in rests {
                for suffix in SUFFIXES {
                    let sl = suffix.chars().count();
                    let rc: Vec<char> = rest.chars().collect();
                    if rc.len() < sl + 2 || !rc[rc.len() - sl..].iter().copied().eq(suffix.chars()) {
                        continue;
                    }
                    let core: String = rc[..rc.len() - sl].iter().collect();
                    // the round `ة` turns into `ت` before an ending (`سيارتك`), and a plural `ات` hangs on the stem of `سيارة`
                    let mut forms = vec![core.clone()];
                    if let Some(stem) = core.strip_suffix('ت') {
                        forms.push(format!("{stem}ه"));
                    }
                    if suffix.starts_with("ات") {
                        forms.push(format!("{core}ه"));
                    }
                    let bare: Vec<String> = forms.iter().filter_map(|f| f.strip_prefix("ال").map(str::to_string)).collect();
                    forms.extend(bare);
                    for candidate in forms {
                        if candidate.chars().count() >= 2 && (prefix != "" || !suffix.is_empty()) {
                            if let Some(list) = self.direct(from, to, &candidate) {
                                return Some(list);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// The translations of `word` from `from` into `to`: direct, else through English. Returns the list and whether English was used.
    fn lookup(&self, from: Lang, to: Lang, word: &str) -> Option<(Vec<String>, bool)> {
        let find = |to: Lang, word: &str| if from == Lang::Ar { self.direct_arabic(from, to, word) } else { self.direct(from, to, word) };
        if let Some(list) = find(to, word) {
            return Some((list.clone(), false));
        }
        if from == Lang::En || to == Lang::En {
            return None;
        }
        let middle = find(Lang::En, word)?;
        let mut out: Vec<String> = Vec::new();
        for english in middle.iter().take(PIVOT_WIDTH) {
            if let Some(list) = self.direct(Lang::En, to, english) {
                for t in list {
                    if !out.contains(t) {
                        out.push(t.clone());
                    }
                }
            }
        }
        (!out.is_empty()).then_some((out, true))
    }

    /// Translates a word. `targets` are the languages to translate into, in order of preference (the language the word is in is
    /// skipped); `from_hint` is the language of the keyboard, used when the word exists in both English and French.
    pub fn translate(&self, word: &str, targets: &[Lang], from_hint: Option<Lang>) -> Option<Translation> {
        let asked = clean(word);
        if asked.is_empty() {
            return None;
        }
        // which language the word is in: the script tells Arabic; for Latin letters the keyboard decides when both know the word
        let sources: Vec<Lang> = if is_arabic_word(asked) {
            vec![Lang::Ar]
        } else {
            match from_hint {
                Some(Lang::Fr) => vec![Lang::Fr, Lang::En],
                _ => vec![Lang::En, Lang::Fr],
            }
        };
        if self.latin.priority.contains(&latin_key(asked)) && !is_arabic_word(asked) {
            if let Some(t) = self.darija(asked, targets) {
                return Some(t);
            }
        }
        for from in sources {
            for &to in targets.iter().filter(|t| **t != from) {
                if let Some((list, via_english)) = self.lookup(from, to, asked) {
                    let candidates = list
                        .into_iter()
                        .take(MAX_TRANSLATIONS)
                        .map(|t| Suggestion { text: if to == Lang::Ar { t } else { match_case(asked, &t) }, lang: to })
                        .collect();
                    return Some(Translation { from, to, word: asked.to_string(), candidates, via_english });
                }
            }
        }
        // a Latin word that neither English nor French knows may be Algerian Darija in Latin letters
        self.darija(asked, targets)
    }

    /// A Latin word looked up in the Darija glossary.
    fn darija(&self, asked: &str, targets: &[Lang]) -> Option<Translation> {
        if asked.chars().all(|c| c.is_ascii_alphanumeric() || c == '\'' || c == '-') {
            if let Some(entry) = self.latin.find(asked) {
                for &to in targets {
                    let list = match to {
                        Lang::En => &entry.en,
                        Lang::Fr => &entry.fr,
                        Lang::Ar => &entry.ar,
                        _ => continue,
                    };
                    if !list.is_empty() {
                        let candidates = list
                            .iter()
                            .take(MAX_TRANSLATIONS)
                            .map(|t| Suggestion { text: if to == Lang::Ar { t.clone() } else { match_case(asked, t) }, lang: to })
                            .collect();
                        return Some(Translation { from: Lang::Dz, to, word: asked.to_string(), candidates, via_english: false });
                    }
                }
            }
        }
        None
    }
}

impl LocalEngine {
    /// Gives the engine the text of a dictionary file (`dict.txt`).
    pub fn load_dictionary(&mut self, text: &str) {
        self.dict = Dictionary::parse_with_fixes(text);
    }

    /// Whether a dictionary was loaded.
    pub fn has_dictionary(&self) -> bool {
        !self.dict.is_empty()
    }

    /// Translates one word with the dictionary. See `Dictionary::translate`.
    pub fn translate_word(&self, word: &str, targets: &[Lang], from_hint: Option<Lang>) -> Option<Translation> {
        self.dict.translate(word, targets, from_hint)
    }

    /// The dictionary itself (for the measuring tools).
    #[doc(hidden)]
    pub fn dictionary(&self) -> &Dictionary {
        &self.dict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "# a small dictionary\n\
        en>fr\thello\tbonjour; salut; allô\n\
        en>fr\tdogs\tchiens\n\
        en>fr\tcafe\tcafé; bar\n\
        fr>en\tbonjour\thello; good morning\n\
        fr>en\tcafé\tcoffee; cafe\n\
        fr>en\tfilm\tmovie\n\
        en>fr\tfilm\tfilm\n\
        en>ar\thello\tمرحبا; أهلا\n\
        en>ar\tcoffee\tقهوة\n\
        ar>en\tمرحبا\thello\n\
        ar>en\tقهوة\tcoffee\n\
        ar>en\tسيارة\tcar\n\
        ar>en\tكتاب\tbook\n\
        ar>en\tمدرسة\tschool\n\
        ar>fr\tقهوة\tcafé\n\
        broken line without tabs\n\
        xx>yy\ta\tb\n";

    fn dict() -> Dictionary {
        Dictionary::parse(FIXTURE)
    }

    #[test]
    fn a_word_is_translated_in_the_order_of_the_file() {
        let t = dict().translate("hello", &[Lang::Fr], None).unwrap();
        assert_eq!((t.from, t.to), (Lang::En, Lang::Fr));
        let words: Vec<&str> = t.candidates.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(words, ["bonjour", "salut", "allô"]);
        assert!(t.candidates.iter().all(|s| s.lang == Lang::Fr));
        assert!(!t.via_english);
    }

    #[test]
    fn arabic_script_means_arabic_and_the_target_skips_the_source_language() {
        // the user wants Arabic, but the word is Arabic: the next target is used
        let t = dict().translate("مرحبا", &[Lang::Ar, Lang::En], None).unwrap();
        assert_eq!((t.from, t.to), (Lang::Ar, Lang::En));
        assert_eq!(t.candidates[0].text, "hello");
    }

    #[test]
    fn case_accents_and_punctuation_do_not_matter() {
        let d = dict();
        let t = d.translate("Hello,", &[Lang::Fr], None).unwrap();
        assert_eq!(t.candidates[0].text, "Bonjour", "a capital stays a capital");
        assert!(d.translate("CAFE", &[Lang::Fr], None).is_some());
        assert!(d.translate("  dogs. ", &[Lang::Fr], None).is_some());
        // the folding makes `cafe` find `café` in the other direction too
        assert_eq!(d.translate("cafe", &[Lang::En], Some(Lang::Fr)).unwrap().candidates[0].text, "coffee");
    }

    #[test]
    fn the_keyboard_decides_when_both_english_and_french_know_the_word() {
        let d = dict();
        let en = d.translate("film", &[Lang::Fr, Lang::En], Some(Lang::En)).unwrap();
        assert_eq!((en.from, en.to), (Lang::En, Lang::Fr));
        let fr = d.translate("film", &[Lang::En, Lang::Fr], Some(Lang::Fr)).unwrap();
        assert_eq!((fr.from, fr.to), (Lang::Fr, Lang::En));
    }

    #[test]
    fn french_to_arabic_goes_through_english_when_there_is_no_direct_list() {
        let d = dict();
        let t = d.translate("café", &[Lang::Ar], Some(Lang::Fr)).unwrap();
        assert_eq!((t.from, t.to), (Lang::Fr, Lang::Ar));
        assert_eq!(t.candidates[0].text, "قهوة");
        assert!(t.via_english);
        // and a direct list is used when there is one
        let direct = d.translate("قهوة", &[Lang::Fr], None).unwrap();
        assert!(!direct.via_english);
        assert_eq!(direct.candidates[0].text, "café");
    }

    #[test]
    fn arabic_words_with_little_words_attached_are_found() {
        let d = dict();
        for (asked, expected) in [("سيارتك", "car"), ("والكتاب", "book"), ("للمدرسة", "school"), ("بالكتاب", "book"), ("سياراتها", "car"), ("المدرسة", "school")] {
            let t = d.translate(asked, &[Lang::En], None).unwrap_or_else(|| panic!("{asked} not found"));
            assert_eq!(t.candidates[0].text, expected, "{asked}");
        }
        // a word that is itself in the dictionary is taken as it is
        assert_eq!(d.translate("قهوة", &[Lang::En], None).unwrap().candidates[0].text, "coffee");
        assert!(d.translate("ببب", &[Lang::En], None).is_none());
    }

    #[test]
    fn hand_written_fixes_go_in_front() {
        let mut d = Dictionary::parse("ar>en\tخبر\ttry; news\n");
        d.apply_fixes("# c\nar>en\tخبر\tnews\nar>fr\tخبر\tnouvelle\n");
        let en: Vec<String> = d.translate("خبر", &[Lang::En], None).unwrap().candidates.into_iter().map(|c| c.text).collect();
        assert_eq!(en, ["news", "try"]);
        assert_eq!(d.translate("خبر", &[Lang::Fr], None).unwrap().candidates[0].text, "nouvelle");
        // the shipped fixes are well formed: every line is read
        let mut shipped = Dictionary::default();
        shipped.apply_fixes(include_str!("../seed/dict-fixes.txt"));
        assert!(shipped.len(Lang::Ar, Lang::En) >= 10 && shipped.len(Lang::Ar, Lang::Fr) >= 10);
    }

    #[test]
    fn darija_in_latin_letters_is_translated_when_english_and_french_do_not_know_the_word() {
        let d = Dictionary::parse_with_fixes("en>fr\tfilm\tfilm\n");
        let t = d.translate("bezzaf", &[Lang::En], None).unwrap();
        assert_eq!((t.from, t.to), (Lang::Dz, Lang::En));
        assert_eq!(t.candidates[0].text, "a lot");
        // other spellings of the same word
        for spelling in ["bzaf", "bezaf", "Bzzaf", "BEZZEF"] {
            assert_eq!(d.translate(spelling, &[Lang::Fr], None).unwrap().candidates[0].text.to_lowercase(), "beaucoup", "{spelling}");
        }
        assert_eq!(d.translate("wach", &[Lang::Ar], None).unwrap().candidates[0].text, "ماذا");
        assert_eq!(d.translate("3andi", &[Lang::En], None).unwrap().candidates[0].text, "I have");
        assert_eq!(d.translate("Dima", &[Lang::Fr], None).unwrap().candidates[0].text, "Toujours", "a capital stays a capital");
        // a French or English word is not taken for Darija
        assert_eq!(d.translate("film", &[Lang::Fr], None).unwrap().from, Lang::En);
        assert!(d.translate("qwertz", &[Lang::En], None).is_none());
    }

    #[test]
    fn a_word_that_is_not_there_gives_nothing() {
        let d = dict();
        assert!(d.translate("zzzz", &[Lang::Fr, Lang::En, Lang::Ar], None).is_none());
        assert!(d.translate("", &[Lang::Fr], None).is_none());
        assert!(d.translate("...", &[Lang::Fr], None).is_none());
    }

    #[test]
    fn bad_lines_are_skipped_and_the_rest_is_kept() {
        let d = dict();
        assert_eq!(d.len(Lang::En, Lang::Fr), 4);
        assert!(!d.is_empty());
        assert!(Dictionary::parse("").is_empty());
    }

    #[test]
    fn the_engine_carries_the_dictionary() {
        let mut e = LocalEngine::new();
        assert!(!e.has_dictionary());
        e.load_dictionary(FIXTURE);
        assert!(e.has_dictionary());
        assert_eq!(e.translate_word("hello", &[Lang::Ar], None).unwrap().candidates[0].text, "مرحبا");
    }
}
