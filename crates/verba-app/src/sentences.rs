//! The connection to `verba-translate.exe`, the program that translates sentences.
//!
//! It is started the first time a sentence is translated, kept while it is used, and ends itself after ten minutes without work.
//! Verba talks to it over its standard input and output with the messages of `verba_local::sentence`. If it ended (or was stopped)
//! between two sentences, the next request starts it again.

use std::io::{BufReader, BufWriter};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

use verba_core::Lang;
use verba_local::sentence::{BEAM, Request, Response, installed_pairs, pair_name, read_frame, translate_with, write_frame};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Threads the network may use: more gave nothing in the measurements (4 and 8 threads took the same time).
const THREADS: &str = "4";
const IDLE_SECS: &str = "600";

struct Session {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Drop for Session {
    fn drop(&mut self) {
        // closing the pipe already ends the translator; this makes sure it is gone when Verba quits
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct Translator {
    exe: Option<PathBuf>,
    models: Option<PathBuf>,
    session: Mutex<Option<Session>>,
}

/// Looks for a file or folder in the usual places: the environment variable, next to the program and up to five folders above it
/// (a development build runs from `target\release`), then the current folder.
fn find(variable: &str, relative: &[&str], is_ok: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var(variable) {
        candidates.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        for up in exe.ancestors().skip(1).take(6) {
            for rel in relative {
                candidates.push(up.join(rel));
            }
        }
    }
    for rel in relative {
        candidates.push(PathBuf::from(rel));
    }
    candidates.into_iter().find(|c| is_ok(c))
}

impl Translator {
    pub fn find() -> Translator {
        let exe = find("VERBA_TRANSLATE_EXE", &["verba-translate.exe", "target/mt/release/verba-translate.exe"], |p| p.is_file());
        let models = find("VERBA_MODELS", &["models"], |p| p.is_dir());
        Translator { exe, models, session: Mutex::new(None) }
    }

    /// The directions that can be translated now: the program and a model for the direction are both installed.
    pub fn installed(&self) -> Vec<(Lang, Lang)> {
        match (&self.exe, &self.models) {
            (Some(_), Some(models)) => installed_pairs(models),
            _ => Vec::new(),
        }
    }

    fn start(&self) -> Result<Session, String> {
        let (exe, models) = self.exe.as_ref().zip(self.models.as_ref()).ok_or("sentence translation is not installed")?;
        let mut child = Command::new(exe)
            .args(["serve", "--models"])
            .arg(models)
            .args(["--threads", THREADS, "--idle-secs", IDLE_SECS])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("could not start the translator: {e}"))?;
        let input = child.stdin.take().ok_or("no pipe to the translator")?;
        let output = child.stdout.take().ok_or("no pipe from the translator")?;
        Ok(Session { child, input: BufWriter::new(input), output: BufReader::new(output) })
    }

    /// One request, with one more try after starting the translator again (it may have ended by itself while Verba was idle).
    fn ask(&self, request: &Request) -> Result<Vec<String>, String> {
        let mut slot = self.session.lock().map_err(|_| "the translator is stuck".to_string())?;
        let mut last = String::new();
        for _ in 0..2 {
            if slot.is_none() {
                *slot = Some(self.start()?);
            }
            let session = slot.as_mut().ok_or("no translator")?;
            let outcome = write_frame(&mut session.input, &request.encode())
                .and_then(|_| read_frame(&mut session.output))
                .map_err(|e| e.to_string())
                .and_then(|frame| frame.ok_or_else(|| "the translator closed".to_string()))
                .and_then(|frame| Response::decode(&frame));
            match outcome {
                Ok(Response::Done(items)) => return Ok(items),
                Ok(Response::Failed(why)) => return Err(why),
                Err(why) => {
                    // the translator is gone or confused: throw the session away, the next try starts a fresh one
                    last = why;
                    *slot = None;
                }
            }
        }
        Err(last)
    }

    /// Translates a whole selection: sentence by sentence, with its spaces and line breaks kept. Blocks until the answer is there
    /// (a second for a sentence; the first time also loads the model), so it runs on a thread of its own.
    pub fn translate(&self, from: Lang, to: Lang, text: &str) -> Result<String, String> {
        translate_with(text, |chunks| {
            // the network has never seen `gg` or `gn`: English chat abbreviations are written out first (`gn guys` -> `Good night guys`)
            let chunks: Vec<String> = if from == Lang::En { chunks.iter().map(|c| verba_local::slang::shipped().expand(c)).collect() } else { chunks.to_vec() };
            self.ask(&Request { pair: pair_name(from, to), beam: BEAM, chunks })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verba_local::sentence::{Script, script_of};

    /// Needs the built translator and the models (`scripts/build-translate.ps1`, `scripts/fetch-models.ps1`):
    ///   cargo test -p verba-app sentences -- --ignored --nocapture
    /// It starts the real program and translates; no keys are sent and no window is touched.
    #[test]
    #[ignore = "needs target/mt/release/verba-translate.exe and the ar-en and en-ar models in models/"]
    fn the_real_translator_answers_and_comes_back_after_it_was_stopped() {
        let t = Translator::find();
        let pairs = t.installed();
        assert!(pairs.contains(&(Lang::Ar, Lang::En)) && pairs.contains(&(Lang::En, Lang::Ar)), "installed: {pairs:?}");

        let text = "  مرحبا بك في بيتنا. هل أنت بخير؟\n\nشكرا لك على كل شيء.\n";
        let english = t.translate(Lang::Ar, Lang::En, text).unwrap();
        println!("{english:?}");
        assert!(english.starts_with("  ") && english.ends_with('\n') && english.contains("\n\n"), "spaces and line breaks stay: {english:?}");
        assert_eq!(script_of(&english), Script::Latin);
        assert!(english.to_lowercase().contains("welcome") || english.to_lowercase().contains("home"), "{english:?}");

        let arabic = t.translate(Lang::En, Lang::Ar, "Can you send me the file before five? Thank you.").unwrap();
        println!("{arabic:?}");
        assert_eq!(script_of(&arabic), Script::Arabic, "{arabic:?}");
        assert!(!arabic.contains(">>"), "the target-language token must not come back: {arabic:?}");

        // the translator is stopped from outside (it ended by itself, or the user ended it): the next request starts it again
        *t.session.lock().unwrap() = None;
        assert!(t.translate(Lang::Ar, Lang::En, "شكرا لك.").unwrap().to_lowercase().contains("thank"));

        // a direction without a model is an error with a reason, not a hang
        let err = t.translate(Lang::Fr, Lang::Ar, "Bonjour tout le monde.").unwrap_err();
        assert!(err.contains("not installed"), "{err}");
    }
}
