//! Sentence translation: everything about it that needs neither Windows nor the neural network.
//!
//! The network itself (CTranslate2 with an OPUS-MT model per direction) runs in a small program of its own,
//! `verba-translate.exe`, so that the main program stays small, starts fast and uses no memory for it until a
//! sentence is translated. This module holds what both sides share:
//!
//! * which direction to translate in (from the script of the text),
//! * how a selection is cut into sentences and put back together (spaces and line breaks stay where they were),
//! * the messages the two programs exchange over a pipe.
//!
//! All of it is plain text handling, tested without a model.

use std::io::{self, Read, Write};
use std::path::Path;

use verba_core::{Lang, is_arabic};

/// The longest selection that is translated, in characters. A longer one gets a notice: the popup shows the whole translation,
/// and a model reads a paragraph, not a chapter.
pub const MAX_CHARS: usize = 1200;
/// A sentence longer than this many words is cut at a comma (or in the middle) before it goes to the model: OPUS-MT models
/// are trained on single sentences and lose words in very long ones.
pub const MAX_CHUNK_WORDS: usize = 60;
/// How many hypotheses the model keeps while it searches (the setting every quality score of the spike was measured with).
pub const BEAM: usize = 4;

// ───────────── which direction ─────────────

/// The script a text is mostly written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Script {
    Arabic,
    Latin,
    /// No letters at all (digits, signs).
    Other,
}

/// Arabic when most of the letters are Arabic letters, Latin (or any other alphabet) when most are not.
pub fn script_of(text: &str) -> Script {
    let (mut arabic, mut other) = (0usize, 0usize);
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        if is_arabic(c) {
            arabic += 1;
        } else {
            other += 1;
        }
    }
    match (arabic, other) {
        (0, 0) => Script::Other,
        (a, o) if a > o => Script::Arabic,
        _ => Script::Latin,
    }
}

/// The direction to translate `text` in, among the directions that are installed: Arabic goes to the language chosen in the settings
/// (then English, then French); Latin letters go to Arabic, from the language of the keyboard when it is known (else English first).
/// `None` when nothing fits (no letters, or the model for that direction is not installed).
pub fn pair_for(text: &str, layout: Option<Lang>, translate_to: Lang, installed: &[(Lang, Lang)]) -> Option<(Lang, Lang)> {
    let has = |from: Lang, to: Lang| installed.contains(&(from, to));
    match script_of(text) {
        Script::Arabic => [translate_to, Lang::En, Lang::Fr].into_iter().filter(|t| *t != Lang::Ar && *t != Lang::Dz).find(|t| has(Lang::Ar, *t)).map(|t| (Lang::Ar, t)),
        Script::Latin => {
            let first = match layout {
                Some(Lang::Fr) => Lang::Fr,
                _ => Lang::En,
            };
            [first, Lang::En, Lang::Fr].into_iter().find(|f| has(*f, Lang::Ar)).map(|f| (f, Lang::Ar))
        }
        Script::Other => None,
    }
}

/// The folder name of a direction: `ar-en`.
pub fn pair_name(from: Lang, to: Lang) -> String {
    format!("{}-{}", from.code(), to.code())
}

fn lang_from_code(code: &str) -> Option<Lang> {
    [Lang::En, Lang::Fr, Lang::Ar].into_iter().find(|l| l.code() == code)
}

/// The directions that have a model in `dir` (one sub-folder per direction, named `ar-en`, holding `model.bin`).
pub fn installed_pairs(dir: &Path) -> Vec<(Lang, Lang)> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return found };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        let Some((from, to)) = name.split_once('-').and_then(|(f, t)| Some((lang_from_code(f)?, lang_from_code(t)?))) else { continue };
        if from != to && entry.path().join("model.bin").is_file() {
            found.push((from, to));
        }
    }
    found.sort_by_key(|(f, t)| (f.code(), t.code()));
    found
}

// ───────────── cutting a selection into sentences ─────────────

/// One sentence with the white space around it. Joining `lead + translation + trail` of every piece gives the translated text
/// with the same line breaks and spaces as the original. A line with no text has an empty `text`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub lead: String,
    pub text: String,
    pub trail: String,
}

fn is_sentence_end(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | '\u{061F}' | '\u{06D4}' | '\u{2026}')
}

fn is_closer(c: char) -> bool {
    matches!(c, '"' | '\'' | ')' | ']' | '\u{00BB}' | '\u{201D}' | '\u{2019}')
}

/// Cuts a line (no line break inside) into sentences. A full stop, "!", "?", Arabic "؟" or "…" ends a sentence when white space
/// (or the end of the line) follows it, so "3.5" and "e.g.x" stay whole.
fn sentences_of(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        current.push(c);
        i += 1;
        if is_sentence_end(c) {
            // the rest of "?!", "...", and a closing quote or bracket belong to this sentence
            while i < chars.len() && (is_sentence_end(chars[i]) || is_closer(chars[i])) {
                current.push(chars[i]);
                i += 1;
            }
            // a lowercase letter next ("... \"stop!\" and left", "e.g. this") means the sentence goes on
            let continues = chars[i..].iter().find(|c| !c.is_whitespace()).is_some_and(|c| c.is_lowercase());
            if (i >= chars.len() || chars[i].is_whitespace()) && !continues {
                // the white space after it goes with it (to its trail)
                while i < chars.len() && chars[i].is_whitespace() {
                    current.push(chars[i]);
                    i += 1;
                }
                out.push(std::mem::take(&mut current));
            }
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// A sentence of more than `MAX_CHUNK_WORDS` words, cut at commas and semicolons (Arabic ones too) so that every part stays short;
/// a part with no such sign is cut after the word limit.
fn split_long(sentence: &str) -> Vec<String> {
    let words: Vec<&str> = sentence.split_whitespace().collect();
    if words.len() <= MAX_CHUNK_WORDS {
        return vec![sentence.to_string()];
    }
    let mut parts: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for word in words {
        current.push(word);
        let soft = word.ends_with([',', ';', ':', '\u{060C}', '\u{061B}']);
        // cut at a sign once the part is long enough to stand alone, and at the limit whatever the word is
        if (soft && current.len() >= MAX_CHUNK_WORDS / 3) || current.len() >= MAX_CHUNK_WORDS {
            parts.push(current.join(" "));
            current.clear();
        }
    }
    if !current.is_empty() {
        parts.push(current.join(" "));
    }
    parts
}

/// Cuts `text` into the pieces to translate. Spaces, tabs and line breaks stay in `lead` and `trail`, never in `text`.
pub fn split(text: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    for raw_line in text.split_inclusive('\n') {
        let (line, eol) = match raw_line.strip_suffix("\r\n").or_else(|| raw_line.strip_suffix('\n')) {
            Some(body) => (body, &raw_line[body.len()..]),
            None => (raw_line, ""),
        };
        if line.trim().is_empty() {
            pieces.push(Piece { lead: line.to_string(), text: String::new(), trail: eol.to_string() });
            continue;
        }
        let mut first = true;
        let pieces_before = pieces.len();
        for sentence in sentences_of(line) {
            let body_end = sentence.trim_end().len();
            let (body, trail) = sentence.split_at(body_end);
            let start = body.len() - body.trim_start().len();
            let (lead, body) = body.split_at(start);
            let parts = split_long(body);
            let last = parts.len() - 1;
            for (n, part) in parts.into_iter().enumerate() {
                pieces.push(Piece {
                    lead: if first && n == 0 { lead.to_string() } else { String::new() },
                    text: part,
                    trail: if n == last { trail.to_string() } else { " ".to_string() },
                });
            }
            first = false;
        }
        // the line break goes after the last piece of the line
        if let Some(last) = pieces[pieces_before..].last_mut() {
            last.trail.push_str(eol);
        }
    }
    pieces
}

/// Translates `text` sentence by sentence with `translate` (which gets the texts of the pieces and returns one translation each)
/// and puts the answers back between the original spaces and line breaks.
pub fn translate_with(text: &str, translate: impl FnOnce(&[String]) -> Result<Vec<String>, String>) -> Result<String, String> {
    let pieces = split(text);
    let chunks: Vec<String> = pieces.iter().filter(|p| !p.text.is_empty()).map(|p| p.text.clone()).collect();
    if chunks.is_empty() {
        return Err("nothing to translate".to_string());
    }
    let translated = translate(&chunks)?;
    if translated.len() != chunks.len() {
        return Err(format!("the translator answered {} sentences for {}", translated.len(), chunks.len()));
    }
    let mut answers = translated.into_iter();
    let mut out = String::new();
    for piece in &pieces {
        out.push_str(&piece.lead);
        if !piece.text.is_empty() {
            out.push_str(answers.next().unwrap_or_default().trim());
        }
        out.push_str(&piece.trail);
    }
    Ok(out)
}

// ───────────── the messages between the two programs ─────────────

/// A message is a 4-byte length (little endian) and that many bytes of UTF-8. Nothing else travels over the pipe.
const MAX_FRAME: usize = 16 * 1024 * 1024;

pub fn write_frame(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

/// The next message; `None` when the other side closed the pipe cleanly between two messages.
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    match r.read_exact(&mut len) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "message too long"));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok(Some(payload))
}

/// What the main program asks: translate these sentences in this direction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// The direction, as a folder name (`ar-en`).
    pub pair: String,
    pub beam: usize,
    /// One sentence each, without line breaks.
    pub chunks: Vec<String>,
}

fn one_line(text: &str) -> String {
    text.replace(['\r', '\n'], " ")
}

impl Request {
    /// Lines: the direction, the beam, then one sentence per line.
    pub fn encode(&self) -> Vec<u8> {
        let mut text = format!("{}\n{}", self.pair, self.beam);
        for chunk in &self.chunks {
            text.push('\n');
            text.push_str(&one_line(chunk));
        }
        text.into_bytes()
    }

    pub fn decode(bytes: &[u8]) -> Result<Request, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "the request is not UTF-8".to_string())?;
        let mut lines = text.split('\n');
        let pair = lines.next().filter(|p| !p.is_empty()).ok_or("the request has no direction")?.to_string();
        let beam = lines.next().and_then(|b| b.parse().ok()).filter(|b| (1..=16).contains(b)).ok_or("the request has no valid beam")?;
        Ok(Request { pair, beam, chunks: lines.map(str::to_string).collect() })
    }
}

/// What the translator answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    /// One translation per sentence asked, in the same order.
    Done(Vec<String>),
    Failed(String),
}

impl Response {
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Response::Done(items) => {
                let mut text = String::from("OK");
                for item in items {
                    text.push('\n');
                    text.push_str(&one_line(item));
                }
                text.into_bytes()
            }
            Response::Failed(why) => format!("ERR\n{}", one_line(why)).into_bytes(),
        }
    }

    pub fn decode(bytes: &[u8]) -> Result<Response, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "the answer is not UTF-8".to_string())?;
        let mut lines = text.split('\n');
        match lines.next() {
            Some("OK") => Ok(Response::Done(lines.map(str::to_string).collect())),
            Some("ERR") => Ok(Response::Failed(lines.next().unwrap_or("unknown error").to_string())),
            _ => Err("the answer is not understood".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joined(text: &str) -> String {
        split(text).iter().map(|p| format!("{}{}{}", p.lead, p.text, p.trail)).collect()
    }

    #[test]
    fn the_script_decides_the_direction() {
        assert_eq!(script_of("هل يمكنك أن ترسل لي الملف؟"), Script::Arabic);
        assert_eq!(script_of("Can you send me the file?"), Script::Latin);
        assert_eq!(script_of("12 + 3 = 15"), Script::Other);
        // a few Latin words inside an Arabic sentence do not change it
        assert_eq!(script_of("سأرسل الملف عبر WhatsApp غدا صباحا"), Script::Arabic);
        let both = [(Lang::Ar, Lang::En), (Lang::En, Lang::Ar)];
        assert_eq!(pair_for("مرحبا بك في بيتنا", None, Lang::En, &both), Some((Lang::Ar, Lang::En)));
        assert_eq!(pair_for("Welcome to our home", None, Lang::En, &both), Some((Lang::En, Lang::Ar)));
        assert_eq!(pair_for("123", None, Lang::En, &both), None);
        // the model for the other direction is not installed: nothing to do rather than a wrong direction
        assert_eq!(pair_for("Welcome to our home", None, Lang::En, &[(Lang::Ar, Lang::En)]), None);
        // French is only chosen when its model exists
        assert_eq!(pair_for("مرحبا بك", None, Lang::Fr, &both), Some((Lang::Ar, Lang::En)));
        assert_eq!(pair_for("مرحبا بك", None, Lang::Fr, &[(Lang::Ar, Lang::En), (Lang::Ar, Lang::Fr)]), Some((Lang::Ar, Lang::Fr)));
        assert_eq!(pair_for("Bonjour tout le monde", Some(Lang::Fr), Lang::En, &[(Lang::En, Lang::Ar), (Lang::Fr, Lang::Ar)]), Some((Lang::Fr, Lang::Ar)));
    }

    #[test]
    fn a_selection_is_cut_into_sentences_and_keeps_its_spaces() {
        let text = "Hello there. How are you?  I am fine!\nSecond line.";
        let pieces = split(text);
        assert_eq!(pieces.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["Hello there.", "How are you?", "I am fine!", "Second line."]);
        assert_eq!(pieces[1].trail, "  ");
        assert_eq!(pieces[2].trail, "\n");
        assert_eq!(joined(text), text, "putting the pieces back gives the original");
    }

    #[test]
    fn numbers_and_abbreviations_without_a_space_stay_whole() {
        let pieces = split("It costs 3.5 dollars (about 2.1 euros) in total.");
        assert_eq!(pieces.len(), 1);
        let pieces = split("He said \"stop!\" and left. Then it rained.");
        assert_eq!(pieces.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(), ["He said \"stop!\" and left.", "Then it rained."]);
    }

    #[test]
    fn arabic_sentences_end_at_the_arabic_question_mark_too() {
        let text = "هل أنت بخير؟ نعم، أنا بخير. شكرا لك!";
        let pieces = split(text);
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0].text, "هل أنت بخير؟");
        assert_eq!(joined(text), text);
    }

    #[test]
    fn blank_lines_and_windows_line_breaks_survive() {
        let text = "  First.\r\n\r\n\tSecond one here\r\nlast";
        assert_eq!(joined(text), text);
        let pieces = split(text);
        assert_eq!(pieces.iter().filter(|p| !p.text.is_empty()).count(), 3);
        assert_eq!(pieces[0].lead, "  ");
        assert!(pieces[1].text.is_empty() && pieces[1].trail == "\r\n");
        assert_eq!(pieces[2].lead, "\t");
    }

    #[test]
    fn a_very_long_sentence_is_cut_at_a_comma() {
        let part = "one two three four five six seven eight nine ten, ";
        let text = format!("{}{}{}end.", part.repeat(4), part.repeat(4), part.repeat(4));
        let pieces = split(&text);
        assert!(pieces.len() >= 2, "{} pieces", pieces.len());
        assert!(pieces.iter().all(|p| p.text.split_whitespace().count() <= MAX_CHUNK_WORDS));
        // nothing is lost: the words are the same
        let words = |s: &str| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(words(&text), words(&joined(&text)));
        // no commas at all: still cut at the limit
        let plain = "word ".repeat(150);
        assert!(split(&plain).iter().all(|p| p.text.split_whitespace().count() <= MAX_CHUNK_WORDS));
    }

    #[test]
    fn the_answers_go_back_between_the_original_spaces() {
        let out = translate_with("Hello there.  How are you?\nBye.", |chunks| Ok(chunks.iter().map(|c| format!("<{c}>")).collect())).unwrap();
        assert_eq!(out, "<Hello there.>  <How are you?>\n<Bye.>");
        assert!(translate_with("   \n ", |_| Ok(vec![])).is_err());
        assert!(translate_with("One. Two.", |_| Ok(vec!["only one".into()])).is_err(), "a missing answer is an error, not a half translation");
        assert_eq!(translate_with("One.", |_| Err("boom".into())), Err("boom".to_string()));
        // an answer's own padding is trimmed, the original spacing is kept
        assert_eq!(translate_with("One. Two.", |_| Ok(vec![" a ".into(), "b".into()])).unwrap(), "a b");
    }

    #[test]
    fn messages_survive_the_pipe() {
        let request = Request { pair: "ar-en".into(), beam: 4, chunks: vec!["مرحبا بك.".into(), "two\nlines".into()] };
        let mut wire = Vec::new();
        write_frame(&mut wire, &request.encode()).unwrap();
        write_frame(&mut wire, &Response::Done(vec!["Welcome.".into()]).encode()).unwrap();
        let mut reader = wire.as_slice();
        let got = Request::decode(&read_frame(&mut reader).unwrap().unwrap()).unwrap();
        assert_eq!((got.pair.as_str(), got.beam, got.chunks.len()), ("ar-en", 4, 2));
        assert_eq!(got.chunks[1], "two lines", "a line break inside a sentence would break the format, so it becomes a space");
        assert_eq!(Response::decode(&read_frame(&mut reader).unwrap().unwrap()).unwrap(), Response::Done(vec!["Welcome.".into()]));
        assert!(read_frame(&mut reader).unwrap().is_none(), "the end of the pipe is not an error");
        assert_eq!(Response::decode(&Response::Failed("no model".into()).encode()).unwrap(), Response::Failed("no model".into()));
        assert!(Request::decode(b"ar-en\nzero").is_err());
        assert!(Request::decode(b"").is_err());
        // a truncated message is an error (the translator died in the middle of an answer)
        let mut cut = &wire[..6];
        assert!(read_frame(&mut cut).is_err());
        // a length that cannot be real
        let mut huge: &[u8] = &[255, 255, 255, 255, 0];
        assert!(read_frame(&mut huge).is_err());
    }

    #[test]
    fn only_folders_with_a_model_count_as_installed() {
        let dir = std::env::temp_dir().join(format!("verba-sentence-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for name in ["ar-en", "en-ar", "en-fr", "readme", "xx-yy", "en-en"] {
            std::fs::create_dir_all(dir.join(name)).unwrap();
        }
        std::fs::write(dir.join("ar-en").join("model.bin"), b"x").unwrap();
        std::fs::write(dir.join("en-ar").join("model.bin"), b"x").unwrap();
        std::fs::write(dir.join("en-en").join("model.bin"), b"x").unwrap();
        assert_eq!(installed_pairs(&dir), vec![(Lang::Ar, Lang::En), (Lang::En, Lang::Ar)]);
        assert!(installed_pairs(&dir.join("missing")).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
