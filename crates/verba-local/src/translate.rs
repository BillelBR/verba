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
use std::sync::OnceLock;

use verba_core::{Lang, Suggestion, is_arabic_word, match_case};

use crate::LocalEngine;
use crate::fold::fold;
use crate::slang::SlangEntry;

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

/// An abbreviation of the chat glossary in the first target language it has a meaning in. The English meaning (`gg` -> `good game`)
/// has `from == to`: the popup calls that "Slang". The meaning keeps a capital letter the abbreviation had (`Brb` -> `Be right back`),
/// but `GG` does not turn it into capitals.
fn slang_translation(entry: &SlangEntry, asked: &str, targets: &[Lang]) -> Option<Translation> {
    for &to in targets {
        let list = match to {
            Lang::En => &entry.en,
            Lang::Fr => &entry.fr,
            Lang::Ar => &entry.ar,
            _ => continue,
        };
        if list.is_empty() {
            continue;
        }
        let mut letters = asked.chars();
        let capital = letters.next().is_some_and(char::is_uppercase) && !letters.any(char::is_uppercase);
        let candidates = list
            .iter()
            .take(MAX_TRANSLATIONS)
            .map(|t| {
                let text = if to != Lang::Ar && capital {
                    let mut c = t.chars();
                    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                } else {
                    t.clone()
                };
                Suggestion { text, lang: to }
            })
            .collect();
        return Some(Translation { from: Lang::En, to, word: asked.to_string(), candidates, via_english: false });
    }
    None
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

    /// `parse`, then the hand-written corrections that ship with the program (`seed/dict-fixes.txt`) and the common words written for Verba
    /// (`seed/ar-common.txt`). This is what the engine loads.
    pub fn parse_with_fixes(text: &str) -> Self {
        let mut dict = Self::parse(text);
        dict.apply_fixes(include_str!("../seed/dict-fixes.txt"));
        dict.apply_extras(include_str!("../seed/ar-common.txt"));
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

    /// Adds the words of a seed file written for Verba (`seed/ar-common.txt`: an Arabic word, a TAB, its English translations) without
    /// reordering anything: a headword the dictionary lacks gets them, one that has translations gets the missing ones at the END. Each
    /// translation that is one English word also gets the Arabic word as a translation, at the end, in the direction `en>ar` (a phrase, or a
    /// note in brackets, is not turned round).
    pub fn apply_extras(&mut self, text: &str) {
        for line in text.lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut fields = line.split('\t');
            let (Some(head), Some(tr)) = (fields.next(), fields.next()) else { continue };
            let head = head.trim();
            let key = fold(clean(head));
            if key.is_empty() {
                continue;
            }
            let glosses: Vec<&str> = tr.split(';').map(str::trim).filter(|t| !t.is_empty()).collect();
            let list = self.pairs.entry((Lang::Ar, Lang::En)).or_default().entry(key).or_default();
            for g in &glosses {
                if !list.iter().any(|x| x == g) {
                    list.push((*g).to_string());
                }
            }
            for g in glosses {
                if g.chars().any(|c| c.is_whitespace() || c == '(' || c == ')') {
                    continue;
                }
                let key = fold(clean(g));
                if key.is_empty() {
                    continue;
                }
                let list = self.pairs.entry((Lang::En, Lang::Ar)).or_default().entry(key).or_default();
                if !list.iter().any(|x| x == head) {
                    list.push(head.to_string());
                }
            }
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
        // game and chat abbreviations (`gg`, `gn`, `see u`): some win over the dictionary, the others only fill its gaps
        let slang_hit = if is_arabic_word(asked) { None } else { crate::slang::shipped().find(asked) };
        if let Some(t) = slang_hit.filter(|e| e.priority).and_then(|e| slang_translation(e, asked, targets)) {
            return Some(t);
        }
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
        if let Some(t) = slang_hit.and_then(|e| slang_translation(e, asked, targets)) {
            return Some(t);
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

/// The answer of the user's own translations and the dictionary's for one word. When both found the word in the same direction, the user's
/// translations come first, then the dictionary's that are not the same (at most `MAX_TRANSLATIONS`); when the directions differ the user's
/// answer is the one given, because the person wrote it for this word.
fn merge_user(user: Option<Translation>, base: Option<Translation>) -> Option<Translation> {
    match (user, base) {
        (None, base) => base,
        (user, None) => user,
        (Some(mut user), Some(base)) => {
            if (user.from, user.to) == (base.from, base.to) {
                for c in base.candidates {
                    if !user.candidates.iter().any(|u| u.text.to_lowercase() == c.text.to_lowercase()) {
                        user.candidates.push(c);
                    }
                }
                user.candidates.truncate(MAX_TRANSLATIONS);
            }
            user.via_english = false;
            Some(user)
        }
    }
}

impl LocalEngine {
    /// Gives the engine the text of a dictionary file (`dict.txt`), read now.
    pub fn load_dictionary(&mut self, text: &str) {
        self.dict = OnceLock::from(Dictionary::parse_with_fixes(text));
        self.dict_path = None;
    }

    /// Whether there is a dictionary: one was given, or `dict.txt` is there to be read at the first translation.
    pub fn has_dictionary(&self) -> bool {
        self.dict_path.is_some() || self.dict.get().is_some_and(|d| !d.is_empty())
    }

    /// Translates one word with the dictionary. See `Dictionary::translate`. The first call reads the dictionary file. The translations the user
    /// wrote for the word (`set_user_translations`) come first.
    pub fn translate_word(&self, word: &str, targets: &[Lang], from_hint: Option<Lang>) -> Option<Translation> {
        let base = self.dictionary().translate(word, targets, from_hint);
        if self.user_tr.is_empty() {
            return base;
        }
        merge_user(self.user_dict.translate(word, targets, from_hint), base)
    }

    /// Gives the engine the translations the user wrote.
    pub fn set_user_translations(&mut self, translations: crate::UserTranslations) {
        let mut dict = Dictionary::default();
        dict.apply_fixes(&translations.to_dictionary_text());
        self.user_dict = dict;
        self.user_tr = translations;
    }

    /// The translations the user wrote.
    pub fn user_translations(&self) -> &crate::UserTranslations {
        &self.user_tr
    }

    /// The dictionary if it was read already, without reading it (the settings window shows its size only then).
    pub fn dictionary_if_read(&self) -> Option<&Dictionary> {
        self.dict.get()
    }

    /// The dictionary itself (for the measuring tools). Reads the file if it was not read yet; an unreadable file gives an empty dictionary.
    pub fn dictionary(&self) -> &Dictionary {
        self.dict.get_or_init(|| match &self.dict_path {
            Some(path) => std::fs::read_to_string(path).map(|text| Dictionary::parse_with_fixes(&text)).unwrap_or_default(),
            None => Dictionary::default(),
        })
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
    fn written_words_are_added_without_reordering_and_the_english_side_gets_the_arabic_word() {
        let mut d = Dictionary::parse("ar>en\tخبر\ttry; news\nen>ar\tprogram\tبرنامج\nen>ar\tstate\tحالة; دولة\n");
        d.apply_extras("# c\nحاسوب\tcomputer\nخبر\tnews; message\nبرامج\tprograms; software\nدولة\tstate; country\nكلمة المرور\tpassword\nطبيبة\tdoctor (f.)\n");
        let en = |w: &str| d.translate(w, &[Lang::En], Some(Lang::Ar)).map(|t| t.candidates.into_iter().map(|c| c.text).collect::<Vec<_>>());
        // a missing headword is added; one that has translations keeps its order and gets the missing ones last
        assert_eq!(en("حاسوب"), Some(vec!["computer".to_string()]));
        assert_eq!(en("خبر"), Some(vec!["try".to_string(), "news".to_string(), "message".to_string()]));
        // a phrase is a headword too
        assert_eq!(en("كلمة المرور"), Some(vec!["password".to_string()]));
        // the English side: the Arabic word is added at the end, never in front; a one-word translation only
        let ar = |w: &str| d.translate(w, &[Lang::Ar], Some(Lang::En)).map(|t| t.candidates.into_iter().map(|c| c.text).collect::<Vec<_>>());
        assert_eq!(ar("computer"), Some(vec!["حاسوب".to_string()]));
        assert_eq!(ar("state"), Some(vec!["حالة".to_string(), "دولة".to_string()]), "already there: not repeated, not moved");
        assert_eq!(ar("password"), Some(vec!["كلمة المرور".to_string()]), "a phrase on the Arabic side is fine");
        assert!(ar("doctor").is_none(), "a translation with a note in brackets is not turned round");
        // the shipped file is well formed: every line has an Arabic headword and translations
        let shipped = include_str!("../seed/ar-common.txt");
        let lines: Vec<&str> = shipped.lines().filter(|l| !l.is_empty() && !l.starts_with('#')).collect();
        assert!(lines.len() > 500, "{} lines", lines.len());
        for line in &lines {
            let mut f = line.split('\t');
            let (head, tr) = (f.next().unwrap(), f.next().unwrap_or_else(|| panic!("no translation: {line}")));
            assert!(head.chars().any(verba_core::is_arabic) && !tr.trim().is_empty(), "{line}");
            assert!(f.next().is_none(), "more than two fields: {line}");
        }
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
    fn the_translations_the_user_wrote_come_before_the_dictionarys() {
        let mut e = LocalEngine::new();
        e.load_dictionary("ar>en\tخبر\ttry; news\nen>ar\tprogram\tبرنامج\n");
        let text = |t: Option<Translation>| t.map(|t| t.candidates.into_iter().map(|c| c.text).collect::<Vec<_>>());
        let before = text(e.translate_word("خبر", &[Lang::En], Some(Lang::Ar))).unwrap();
        assert!(before.contains(&"news".to_string()) && !before.contains(&"my news".to_string()), "{before:?}");
        let mut mine = crate::UserTranslations::new();
        mine.add(Lang::Ar, Lang::En, "خبر", "my news; news").unwrap();
        mine.add(Lang::Ar, Lang::En, "قرقوشة", "app").unwrap();
        e.set_user_translations(mine);
        // mine first, then the dictionary's that are not the same
        let after = text(e.translate_word("خبر", &[Lang::En], Some(Lang::Ar))).unwrap();
        assert_eq!(&after[..2], ["my news", "news"], "{after:?}");
        assert!(after.len() <= 5 && after.iter().filter(|t| t.as_str() == "news").count() == 1, "{after:?}");
        // a word the dictionary does not have, and the same word with the article and a preposition
        assert_eq!(text(e.translate_word("قرقوشة", &[Lang::En], Some(Lang::Ar))), Some(vec!["app".to_string()]));
        assert_eq!(text(e.translate_word("بالقرقوشة", &[Lang::En], Some(Lang::Ar))), Some(vec!["app".to_string()]));
        // the dictionary alone for the other words
        assert_eq!(text(e.translate_word("program", &[Lang::Ar], None)), Some(vec!["برنامج".to_string()]));
        // taking them away gives the dictionary back
        e.set_user_translations(crate::UserTranslations::new());
        assert!(e.translate_word("قرقوشة", &[Lang::En], Some(Lang::Ar)).is_none());
    }

    #[test]
    fn the_dictionary_file_is_read_at_the_first_translation_not_before() {
        let dir = std::env::temp_dir().join(format!("verba-lazy-dict-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("dict.txt"), "en>fr\thello\tbonjour\n").unwrap();
        let mut e = LocalEngine::new();
        e.dict_path = Some(dir.join("dict.txt"));
        assert!(e.has_dictionary(), "the file is there");
        assert!(e.dict.get().is_none(), "but it was not read");
        let t = e.translate_word("hello", &[Lang::Fr], None).expect("translated");
        assert_eq!(t.candidates[0].text, "bonjour");
        assert!(e.dict.get().is_some(), "read by the first translation");
        // a file that is not there or cannot be read is an empty dictionary, not a failure
        let mut missing = LocalEngine::new();
        missing.dict_path = Some(dir.join("nope.txt"));
        assert!(missing.translate_word("hello", &[Lang::Fr], None).is_none());
        let _ = std::fs::remove_dir_all(&dir);
        e.dict_path = None;
    }

    #[test]
    fn the_engine_carries_the_dictionary() {
        let mut e = LocalEngine::new();
        assert!(!e.has_dictionary());
        e.load_dictionary(FIXTURE);
        assert!(e.has_dictionary());
        assert_eq!(e.translate_word("hello", &[Lang::Ar], None).unwrap().candidates[0].text, "مرحبا");
    }

    #[test]
    fn game_and_chat_abbreviations_are_explained_in_the_language_asked() {
        // even without any dictionary entry: the glossary that ships with the program
        let d = Dictionary::parse(FIXTURE);
        let into = |word: &str, to: Lang| d.translate(word, &[to], None).map(|t| (t.from, t.to, t.candidates.into_iter().map(|c| c.text).collect::<Vec<_>>()));
        // English meaning: written out (the popup calls it "Slang": from == to)
        assert_eq!(into("GG", Lang::En), Some((Lang::En, Lang::En, vec!["good game".to_string()])));
        assert_eq!(into("gn!", Lang::En).unwrap().2, vec!["good night"]);
        // a capital on the abbreviation gives a capital, shouting does not
        assert_eq!(into("Brb", Lang::En).unwrap().2, vec!["Be right back"]);
        assert_eq!(into("BRB", Lang::En).unwrap().2, vec!["be right back"]);
        // into French and Arabic, and two words ("see u")
        assert_eq!(into("gg", Lang::Fr).unwrap().2[0], "bien joué");
        assert_eq!(into("gg", Lang::Ar).unwrap().2, vec!["لعبة جيدة", "أحسنت"]);
        assert_eq!(into("See U", Lang::Fr).unwrap().2[0], "à plus");
        // `r` has no Arabic: the next language asked for is used
        assert_eq!(d.translate("r", &[Lang::Ar, Lang::Fr], None).unwrap().to, Lang::Fr);
        // a word of the dictionary that is also a game word keeps the dictionary's meaning (not marked with `!`)
        assert_eq!(d.translate("film", &[Lang::Fr], Some(Lang::En)).unwrap().candidates[0].text, "film");
        // an abbreviation marked with `!` wins over the dictionary
        let mut with_op = Dictionary::parse(FIXTURE);
        with_op.apply_fixes("en>fr	op	opération
");
        assert_eq!(with_op.translate("op", &[Lang::Fr], None).unwrap().candidates[0].text, "trop fort");
        // and ordinary words are not touched
        assert!(d.translate("hello", &[Lang::Fr], None).unwrap().candidates[0].text == "bonjour");
    }
}
