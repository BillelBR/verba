//! Suggestions for programmers: the words of a language (`fn`, `useState`, `println!`), the commands of a terminal (`git`, `cargo`, `docker`), what comes
//! after them (`git ch` -> `checkout`, `cherry-pick`; `git commit -` -> `-m`, `--amend`).
//!
//! Everything comes from `seed/dev-terms.txt` (format in its header). Two kinds of help:
//!
//! * **a command line**: when the line being typed holds a command Verba knows (`git`, `npm`, `docker compose` ...), the word under the caret is completed from
//!   what that command takes: its subcommands (the `[sub ...]` lists) or, when the word starts with a dash, its flags (the `[flags ...]` lists);
//! * **code**: otherwise the word under the caret (a dotted name such as `console.l` counts as one) is completed from the words of the languages
//!   (`[words language kind]`). The languages seen in the text just before the caret (a `fn` or a `let mut` says Rust, a `def` says Python) are put first.
//!
//! The case matters in code: a word typed in capitals is matched with the capitals, a lower-case one with any case (`use` finds `useState`).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// What the user is offered, and how many characters before the caret it replaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevHit {
    /// How many characters before the caret were typed of the word (what the choice replaces).
    pub typed: usize,
    /// The choices, best first; the text already ends with a space when one is to follow (a keyword, a command, a flag) and has none when it is not
    /// (a function, `.push`, `println!`).
    pub choices: Vec<String>,
}

#[derive(Debug, Clone)]
struct Term {
    text: String,
    /// The language of a `[words ...]` entry (`any` for the ones every language shares).
    tag: String,
    /// A space follows it once chosen.
    space: bool,
    /// Its place in its list: the earlier, the more common.
    rank: usize,
}

#[derive(Debug, Default)]
pub struct Dev {
    words: Vec<Term>,
    commands: Vec<Term>,
    subs: HashMap<String, Vec<Term>>,
    flags: HashMap<String, Vec<Term>>,
    /// A word that belongs to one language only, with that language (`fn` -> rust): how the language of the text is recognised.
    exclusive: HashMap<String, String>,
}

/// The shortest dotted or plain word of code that is completed (a word that starts with a dot, `.pu`, may be shorter: the dot says what it is).
const MIN_CODE_LETTERS: usize = 3;
/// The shortest command name that is completed.
const MIN_COMMAND_LETTERS: usize = 2;

fn is_code_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '$' | '.' | '#' | '@' | ':')
}

impl Dev {
    pub fn parse(text: &str) -> Dev {
        let mut dev = Dev::default();
        enum Into {
            None,
            Words(String, bool),
            Commands,
            Subs(String),
            Flags(String),
        }
        let mut into = Into::None;
        let mut owner: HashMap<String, String> = HashMap::new();
        for line in text.lines() {
            let line = line.trim();
            // a comment is a '#' followed by a space (a word such as `#include` is a term)
            if line.is_empty() || line == "#" || line.starts_with("# ") {
                continue;
            }
            if let Some(head) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                let parts: Vec<&str> = head.split_whitespace().collect();
                into = match parts.as_slice() {
                    ["words", tag, kind] => Into::Words(tag.to_string(), *kind != "api"),
                    ["cmd"] => Into::Commands,
                    ["sub", path @ ..] if !path.is_empty() => Into::Subs(path.join(" ")),
                    ["flags", path @ ..] if !path.is_empty() => Into::Flags(path.join(" ")),
                    _ => Into::None,
                };
                continue;
            }
            // a trailing comment
            let term = line.split(" #").next().unwrap_or(line).trim();
            if term.is_empty() || term.contains(char::is_whitespace) {
                continue;
            }
            match &into {
                Into::None => {}
                Into::Words(tag, space) => {
                    let rank = dev.words.iter().filter(|t| t.tag == *tag).count();
                    dev.words.push(Term { text: term.to_string(), tag: tag.clone(), space: *space, rank });
                    if tag != "any" {
                        owner.entry(term.to_string()).or_insert_with(|| tag.clone());
                    }
                }
                Into::Commands => {
                    let rank = dev.commands.len();
                    dev.commands.push(Term { text: term.to_string(), tag: String::new(), space: true, rank });
                }
                Into::Subs(path) => {
                    let list = dev.subs.entry(path.clone()).or_default();
                    let rank = list.len();
                    list.push(Term { text: term.to_string(), tag: String::new(), space: true, rank });
                }
                Into::Flags(path) => {
                    let list = dev.flags.entry(path.clone()).or_default();
                    let rank = list.len();
                    list.push(Term { text: term.to_string(), tag: String::new(), space: true, rank });
                }
            }
        }
        // a word of two languages does not tell which one is written
        let mut tags_of: HashMap<&str, HashSet<&str>> = HashMap::new();
        for t in dev.words.iter().filter(|t| t.tag != "any") {
            tags_of.entry(t.text.as_str()).or_default().insert(t.tag.as_str());
        }
        dev.exclusive = owner.into_iter().filter(|(w, _)| tags_of.get(w.as_str()).is_some_and(|s| s.len() == 1)).collect();
        dev
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.commands.is_empty()
    }

    /// How many words of each language the text holds (only the words that belong to one language count): what the languages "seen" are made of.
    pub fn languages_in(&self, text: &str) -> Vec<(String, usize)> {
        let mut seen: HashMap<&str, usize> = HashMap::new();
        for token in text.split(|c: char| !is_code_char(c) || c == '.').filter(|t| !t.is_empty()) {
            if let Some(tag) = self.exclusive.get(token) {
                *seen.entry(tag.as_str()).or_insert(0) += 1;
            }
        }
        let mut out: Vec<(String, usize)> = seen.into_iter().map(|(t, n)| (t.to_string(), n)).collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }

    /// What to offer for the word before the caret, or `None` when nothing fits. `leaning` is how much each language has been seen lately.
    pub fn suggest(&self, before_caret: &str, leaning: &[(String, f32)], max: usize) -> Option<DevHit> {
        if max == 0 {
            return None;
        }
        let line = before_caret.rsplit(['\n', '\r']).next().unwrap_or("");
        let segment = line.rsplit(['|', ';', '&']).next().unwrap_or(line);
        let ends_in_space = segment.ends_with(char::is_whitespace);
        let mut tokens: Vec<&str> = segment.split_whitespace().collect();
        let current = if ends_in_space || tokens.is_empty() { "" } else { tokens.pop().unwrap_or("") };
        let current = if segment.trim().is_empty() { "" } else { current };

        // after a command Verba knows
        if let Some(start) = tokens.iter().take(4).position(|t| self.is_command(&t.to_lowercase())) {
            let path: Vec<String> = tokens[start..].iter().take_while(|t| !t.starts_with('-')).map(|t| t.to_lowercase()).collect();
            let mut found: Vec<(String, usize, usize, bool)> = Vec::new(); // text, depth, rank, space
            if current.starts_with('-') {
                for depth in (1..=path.len()).rev() {
                    if let Some(list) = self.flags.get(&path[..depth].join(" ")) {
                        found.extend(list.iter().map(|t| (t.text.clone(), depth, t.rank, t.space)));
                    }
                }
            } else if let Some(list) = self.subs.get(&path.join(" ")) {
                found.extend(list.iter().map(|t| (t.text.clone(), 0, t.rank, t.space)));
            }
            let typed: String = current.to_string();
            let low = typed.to_lowercase();
            found.retain(|(text, ..)| text.to_lowercase().starts_with(&low) && *text != typed);
            if !found.is_empty() {
                // the flags of the deepest command first, then the most common
                found.sort_by(|a, b| b.1.cmp(&a.1).then(a.2.cmp(&b.2)));
                let choices = unique(found.into_iter().map(|(t, _, _, space)| if space { format!("{t} ") } else { t }), max);
                return Some(DevHit { typed: typed.chars().count(), choices });
            }
            // a flag, a subcommand or an argument that Verba does not know: a command line is not code, so nothing is offered
            return None;
        }

        let mut scored: Vec<(String, f64)> = Vec::new();
        let mut typed_len = 0;
        // the first word of a command line: a command's name
        if tokens.iter().take(4).all(|t| !self.is_command(&t.to_lowercase())) && !current.is_empty() && current.chars().filter(|c| c.is_alphanumeric()).count() >= MIN_COMMAND_LETTERS && !current.contains(['.', '(', '{', '=']) {
            let low = current.to_lowercase();
            for t in self.commands.iter().filter(|t| t.text.to_lowercase().starts_with(&low) && t.text != current) {
                scored.push((format!("{} ", t.text), 900.0 - t.rank as f64));
                typed_len = current.chars().count();
            }
        }

        // the words of code
        let token: String = {
            let tail: Vec<char> = before_caret.chars().rev().take_while(|c| is_code_char(*c)).collect();
            tail.into_iter().rev().collect()
        };
        let keys = {
            let mut keys = Vec::new();
            // after an arrow (`$pdo->pre`): the arrow is part of the word
            let before_token = &before_caret[..before_caret.len() - token.len()];
            if before_token.ends_with("->") {
                keys.push(format!("->{token}"));
            }
            keys.push(token.clone());
            if let Some(dot) = token.rfind('.') {
                keys.push(token[dot..].to_string()); // `.pu`
                keys.push(token[dot + 1..].to_string()); // `pu`
            }
            if let Some(colon) = token.rfind(':') {
                if colon > 0 && token.as_bytes()[colon - 1] == b':' {
                    keys.push(token[colon - 1..].to_string()); // `::cre`
                }
                keys.push(token[colon + 1..].to_string());
            }
            keys
        };
        for key in keys {
            let letters = key.chars().filter(|c| c.is_alphanumeric()).count();
            let enough = if key.starts_with(['.', ':']) || key.starts_with("->") { letters >= 1 } else { letters >= MIN_CODE_LETTERS };
            if !enough {
                continue;
            }
            let case_sensitive = key.chars().any(char::is_uppercase);
            let low = key.to_lowercase();
            let total: f32 = leaning.iter().map(|(_, v)| v).sum::<f32>().max(1.0);
            let mut hits: Vec<(String, f64)> = Vec::new();
            for t in &self.words {
                let matches = if case_sensitive { t.text.starts_with(&key) } else { t.text.to_lowercase().starts_with(&low) };
                if !matches || t.text == key {
                    continue;
                }
                let boost = leaning.iter().find(|(tag, _)| *tag == t.tag).map_or(0.0, |(_, v)| (v / total) as f64);
                let score = (1000.0 - t.rank as f64) * (1.0 + 2.0 * boost);
                hits.push((if t.space { format!("{} ", t.text) } else { t.text.clone() }, score));
            }
            if !hits.is_empty() {
                // a command name and a code word may have the same span: both are offered; a different span is the code word's alone
                let span = key.chars().count();
                if typed_len != 0 && typed_len != span {
                    scored.clear();
                }
                typed_len = span;
                scored.extend(hits);
                break;
            }
        }
        if scored.is_empty() {
            return None;
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let choices = unique(scored.into_iter().map(|s| s.0), max);
        Some(DevHit { typed: typed_len, choices })
    }

    fn is_command(&self, name: &str) -> bool {
        self.commands.iter().any(|t| t.text == name)
            || self.subs.keys().chain(self.flags.keys()).any(|k| k.split(' ').next() == Some(name))
    }
}

fn unique(items: impl Iterator<Item = String>, max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
        if out.len() == max {
            break;
        }
    }
    out
}

/// The terms that ship with the program.
pub fn shipped() -> &'static Dev {
    static SHIPPED: OnceLock<Dev> = OnceLock::new();
    SHIPPED.get_or_init(|| Dev::parse(include_str!("../seed/dev-terms.txt")))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
[words rust kw]
fn
let
impl
[words rust api]
println!
Vec::new
[words js api]
useState
useEffect
console.log
.push
.pop
[words python kw]
def
import
[words any kw]
return
function
[cmd]
git
ls
cargo
[sub git]
commit
checkout
cherry-pick
status
[flags git commit]
-m
--amend
[flags git]
--version
[sub cargo]
build
run
[flags cargo build]
--release
";

    fn dev() -> Dev {
        Dev::parse(SAMPLE)
    }

    fn texts(h: Option<DevHit>) -> Vec<String> {
        h.map(|h| h.choices).unwrap_or_default()
    }

    #[test]
    fn a_subcommand_is_completed_after_its_command() {
        let d = dev();
        let h = d.suggest("$ git ch", &[], 3).unwrap();
        assert_eq!(h.typed, 2);
        assert_eq!(h.choices, ["checkout ", "cherry-pick "]);
        // right after the space: the most common ones, with nothing typed
        let h = d.suggest("git ", &[], 3).unwrap();
        assert_eq!((h.typed, h.choices[0].as_str()), (0, "commit "));
        // a prompt before the command, and a chain of commands
        assert_eq!(texts(d.suggest("PS C:\\work> cargo bu", &[], 3)), ["build "]);
        assert_eq!(texts(d.suggest("cargo build && git sta", &[], 3)), ["status "]);
    }

    #[test]
    fn flags_come_with_the_dash_and_the_deepest_command_first() {
        let d = dev();
        assert_eq!(texts(d.suggest("git commit -", &[], 5)), ["-m ", "--amend ", "--version "]);
        assert_eq!(texts(d.suggest("git commit --a", &[], 3)), ["--amend "]);
        assert_eq!(texts(d.suggest("cargo build --re", &[], 3)), ["--release "]);
        // a flag of nothing known: nothing, not a word of code
        assert!(d.suggest("cargo run --zzz", &[], 3).is_none());
        // an argument position (after `git commit -m`, a message): nothing
        assert!(d.suggest("git commit -m \"fix", &[], 3).is_none());
    }

    #[test]
    fn a_command_name_is_completed_at_the_start_of_a_line() {
        let d = dev();
        assert_eq!(texts(d.suggest("gi", &[], 3)), ["git "]);
        assert_eq!(texts(d.suggest("ls -la\ncar", &[], 3)), ["cargo "]);
        // one letter is too little
        assert!(d.suggest("g", &[], 3).is_none());
    }

    #[test]
    fn code_words_are_completed_with_their_case_and_their_dots() {
        let d = dev();
        let h = d.suggest("const [a, setA] = useS", &[], 3).unwrap();
        assert_eq!((h.typed, h.choices.as_slice()), (4, &["useState".to_string()][..]));
        // typed in lower case, a camelCase word is still found
        assert_eq!(texts(d.suggest("x = uses", &[], 3)), ["useState"]);
        assert_eq!(texts(d.suggest("x = useeff", &[], 3)), ["useEffect"]);
        // a dotted name is one word; a method after a dot starts with the dot
        assert_eq!(texts(d.suggest("console.l", &[], 3)), ["console.log"]);
        let h = d.suggest("list.pu", &[], 3).unwrap();
        assert_eq!((h.typed, h.choices.as_slice()), (3, &[".push".to_string()][..]));
        // a keyword is followed by a space, a function is not
        assert_eq!(texts(d.suggest("    ret", &[], 3)), ["return "]);
        assert_eq!(texts(d.suggest("    prin", &[], 3)), ["println!"]);
        // capitals are matched as typed
        assert!(d.suggest("x = UseS", &[], 3).is_none());
    }

    #[test]
    fn the_language_seen_in_the_text_comes_first() {
        let d = dev();
        // `fn` is Rust only, `def` Python only; `function` is shared
        let rust = d.languages_in("pub fn main() { let x = 1; }");
        assert_eq!(rust[0].0, "rust");
        let py = d.languages_in("def run():\n  import os");
        assert_eq!(py[0].0, "python");
        assert!(d.languages_in("hello world").is_empty());
        // with Rust leaning, `de` finds nothing of python first; with Python leaning `def` is offered
        let lean_py = [("python".to_string(), 3.0f32)];
        assert_eq!(texts(d.suggest("de", &lean_py, 3)), Vec::<String>::new(), "two letters are too few for code");
        assert_eq!(texts(d.suggest("fun", &lean_py, 3)), ["function "]);
        let lean_rust = [("rust".to_string(), 3.0f32)];
        assert_eq!(texts(d.suggest("im", &lean_rust, 3)), Vec::<String>::new());
        assert_eq!(texts(d.suggest("imp", &lean_rust, 3))[0], "impl ");
        assert_eq!(texts(d.suggest("imp", &lean_py, 3))[0], "import ");
    }

    #[test]
    fn the_shipped_terms_load() {
        let d = shipped();
        assert!(!d.is_empty());
        assert_eq!(d.suggest("$ git ch", &[], 3).map(|h| h.choices[0].clone()).as_deref(), Some("checkout "));
        assert!(d.suggest("git commit -", &[], 3).unwrap().choices.contains(&"--amend ".to_string()));
        assert!(d.suggest("npm ru", &[], 3).unwrap().choices.contains(&"run ".to_string()));
        assert!(d.suggest("docker compose u", &[], 3).unwrap().choices.contains(&"up ".to_string()));
        assert!(d.suggest("cargo bu", &[], 3).unwrap().choices.contains(&"build ".to_string()));
        assert_eq!(d.languages_in("let mut v = Vec::new(); impl Foo for Bar")[0].0, "rust");
    }

    #[test]
    fn real_lines_get_the_right_help() {
        let d = shipped();
        let first = |text: &str, lean: &[(String, f32)]| d.suggest(text, lean, 3).map(|h| h.choices);
        let show = |text: &str| first(text, &[]);
        assert_eq!(show("let v = Vec::n").unwrap()[0], "Vec::new");
        assert_eq!(show("let x = items.fil").unwrap()[0], ".filter(");
        assert_eq!(show("const [a, setA] = useSt").unwrap()[0], "useState");
        assert_eq!(show("console.er").unwrap()[0], "console.error");
        assert_eq!(show("$pdo->pre").unwrap()[0], "->prepare(");
        assert!(first("User::fi", &[("php".to_string(), 2.0)]).unwrap().contains(&"::find(".to_string()));
        assert_eq!(show("#inc").unwrap()[0], "#include ");
        assert_eq!(show("std::col").unwrap()[0], "std::collections::");
        assert_eq!(show("Get-Chi").unwrap()[0], "Get-ChildItem ");
        assert_eq!(show("git push --force-w").unwrap()[0], "--force-with-lease ");
        assert_eq!(show("docker run -").unwrap()[0], "-d ");
        assert_eq!(show("kubectl get po").unwrap()[0], "pods ");
        assert_eq!(show("SELECT * FROM users WHERE id IN (SEL").unwrap()[0], "SELECT ");
        // plain prose after a command is left alone, and so is a word nobody knows
        assert!(show("git commit -m \"fix the bug").is_none());
        assert!(show("qqqzzz").is_none());
        // the language of the lines before the caret orders the words they share with others
        let rust = d.languages_in("pub fn main() { let mut x = 1; match x {");
        let lean: Vec<(String, f32)> = rust.iter().map(|(t, n)| (t.clone(), *n as f32)).collect();
        assert_eq!(first("    imp", &lean).unwrap()[0], "impl ");
        assert!(d.suggest("ret", &lean, 3).unwrap().choices[0].starts_with("return"));
    }
}
