//! Verba's sentence translator.
//!
//! A program of its own, so that `verba.exe` stays small and uses no memory for the network until a sentence is translated.
//! Verba starts it when needed, talks to it over its standard input and output (see `verba_local::sentence` for the messages),
//! and it ends itself after a while without work, which gives the memory back.
//!
//! ```text
//! verba-translate serve --models <folder> [--threads 4] [--idle-secs 600]
//! verba-translate translate <models folder> <ar-en> "text"      (one sentence, for a quick look)
//! verba-translate bench <models folder> <ar-en> <threads> <beam> <runs> "sentence" ...
//! verba-translate batch <models folder> <ar-en> <beam> <input.txt> <output.txt> [--raw]    (one text per line, as the program translates it)
//! ```
//!
//! The models are OPUS-MT networks converted to int8 for CTranslate2, one folder per direction (`ar-en`, `en-ar`, ...), each with
//! `model.bin`, `shared_vocabulary.json`, `config.json`, `source.spm` and `target.spm` (see `scripts/fetch-models.ps1`).

use std::collections::HashMap;
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use ct2rs::tokenizers::sentencepiece::Tokenizer;
use ct2rs::{Config, TranslationOptions, Translator};
use verba_local::sentence::{Request, Response, read_frame, write_frame};

/// The target-language token that a multilingual OPUS-MT model wants at the start of its input. Arabic models have several
/// Arabic varieties; `>>ara<<` is Modern Standard Arabic. The token goes in as one piece: through the sentencepiece model it would
/// be cut into letters and the network would copy it into its answer.
const ARABIC_TARGET_TOKEN: &str = ">>ara<<";
/// How many directions stay in memory (about 100 MB each); the one used least recently is let go first.
const KEEP_LOADED: usize = 2;
const MAX_DECODING_LENGTH: usize = 256;

type Model = Translator<Tokenizer>;

/// Decoding options for an experiment: `VERBA_TR_LENGTH_PENALTY`, `VERBA_TR_COVERAGE_PENALTY`, `VERBA_TR_REPETITION_PENALTY`,
/// `VERBA_TR_PATIENCE`, and `VERBA_TR_DISABLE_UNK=0` to allow the unknown piece again. Without them the options are the library's defaults,
/// except that the unknown piece is never written (see `translate`).
fn tuned(mut options: TranslationOptions<String, String>) -> TranslationOptions<String, String> {
    let number = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<f32>().ok());
    if let Some(v) = number("VERBA_TR_LENGTH_PENALTY") {
        options.length_penalty = v;
    }
    if let Some(v) = number("VERBA_TR_COVERAGE_PENALTY") {
        options.coverage_penalty = v;
    }
    if let Some(v) = number("VERBA_TR_REPETITION_PENALTY") {
        options.repetition_penalty = v;
    }
    if let Some(v) = number("VERBA_TR_PATIENCE") {
        options.patience = v;
    }
    if std::env::var("VERBA_TR_DISABLE_UNK").is_ok_and(|v| v == "0") {
        options.disable_unk = false;
    }
    options
}

struct Engine {
    models_dir: PathBuf,
    threads: usize,
    /// Loaded directions with the time they were last used.
    loaded: HashMap<String, (Model, Instant)>,
}

fn load(dir: &Path, threads: usize) -> Result<Model> {
    let config = Config { num_threads_per_replica: threads, ..Config::default() };
    let tokenizer = Tokenizer::new(dir).with_context(|| format!("cannot read source.spm and target.spm in {}", dir.display()))?;
    Translator::with_tokenizer(dir, tokenizer, &config).with_context(|| format!("cannot load the model in {}", dir.display()))
}

/// Only plain folder names like `ar-en`: the name comes from the other program and ends up in a path.
fn valid_pair(pair: &str) -> bool {
    let mut parts = pair.split('-');
    let ok = |p: Option<&str>| p.is_some_and(|p| p.len() == 2 && p.bytes().all(|b| b.is_ascii_lowercase()));
    ok(parts.next()) && ok(parts.next()) && parts.next().is_none()
}

impl Engine {
    fn new(models_dir: PathBuf, threads: usize) -> Engine {
        Engine { models_dir, threads, loaded: HashMap::new() }
    }

    fn translate(&mut self, pair: &str, beam: usize, chunks: &[String]) -> Result<Vec<String>> {
        if !valid_pair(pair) {
            return Err(anyhow!("not a direction: {pair}"));
        }
        if !self.loaded.contains_key(pair) {
            let dir = self.models_dir.join(pair);
            if !dir.join("model.bin").is_file() {
                return Err(anyhow!("the {pair} model is not installed"));
            }
            // make room first: the oldest direction goes
            while self.loaded.len() >= KEEP_LOADED {
                let oldest = self.loaded.iter().min_by_key(|(_, (_, used))| *used).map(|(name, _)| name.clone());
                match oldest {
                    Some(name) => self.loaded.remove(&name),
                    None => break,
                };
            }
            let model = load(&dir, self.threads)?;
            self.loaded.insert(pair.to_string(), (model, Instant::now()));
        }
        let (model, used) = self.loaded.get_mut(pair).ok_or_else(|| anyhow!("model vanished"))?;
        *used = Instant::now();
        // `disable_unk`: the network cannot write a letter like `á`, `é`, `ö` or a sign like `–` or `°` (its output vocabulary lacks them),
        // so where one belonged it wrote the unknown piece, and the answer had `Fern⁇ndez`, `6⁇`, `⁇ C` (25 of the 1 012 sentences of
        // FLORES-200). With the unknown piece forbidden it writes the best piece it has: `Fernandez`, `35-40`, `degrees Celsius`. The scores
        // do not move (docs/ACCURACY.md, "Sentence translation: FLORES-200").
        let options = tuned(TranslationOptions::<String, String> {
            beam_size: beam,
            max_decoding_length: MAX_DECODING_LENGTH,
            disable_unk: true,
            ..Default::default()
        });
        let answers = if pair.ends_with("-ar") {
            let prefixes: Vec<Vec<&str>> = chunks.iter().map(|_| vec![ARABIC_TARGET_TOKEN]).collect();
            model.translate_batch_with_source_prefix(chunks, &prefixes, &options, None)?
        } else {
            model.translate_batch(chunks, &options, None)?
        };
        Ok(answers.into_iter().map(|(text, _score)| text.trim().to_string()).collect())
    }
}

/// Since startup, in milliseconds (the idle watcher reads it from another thread).
fn now_ms(start: &Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

fn serve(models_dir: PathBuf, threads: usize, idle: Duration) -> Result<()> {
    let start = Instant::now();
    static LAST: AtomicU64 = AtomicU64::new(0);
    static BUSY: AtomicBool = AtomicBool::new(false);
    // Without work for `idle`, the program ends and its memory goes back. Verba starts it again at the next sentence.
    let watcher_start = start;
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let quiet = now_ms(&watcher_start).saturating_sub(LAST.load(Ordering::SeqCst));
            if !BUSY.load(Ordering::SeqCst) && quiet >= idle.as_millis() as u64 {
                std::process::exit(0);
            }
        }
    });

    let mut engine = Engine::new(models_dir, threads);
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = BufWriter::new(io::stdout().lock());
    // the program ends when Verba closes the pipe
    while let Some(frame) = read_frame(&mut input)? {
        BUSY.store(true, Ordering::SeqCst);
        let response = match Request::decode(&frame) {
            Ok(request) => match engine.translate(&request.pair, request.beam, &request.chunks) {
                Ok(answers) => Response::Done(answers),
                Err(e) => Response::Failed(format!("{e:#}")),
            },
            Err(why) => Response::Failed(why),
        };
        write_frame(&mut output, &response.encode())?;
        LAST.store(now_ms(&start), Ordering::SeqCst);
        BUSY.store(false, Ordering::SeqCst);
    }
    Ok(())
}

fn bench(args: &[String]) -> Result<()> {
    let [models, pair, threads, beam, runs, sentences @ ..] = args else {
        return Err(anyhow!("usage: verba-translate bench <models folder> <pair> <threads> <beam> <runs> <sentence>..."));
    };
    let (threads, beam, runs): (usize, usize, usize) = (threads.parse()?, beam.parse()?, runs.parse()?);
    let t = Instant::now();
    let mut engine = Engine::new(PathBuf::from(models), threads);
    // the first call loads the model
    let first = engine.translate(pair, beam, &sentences[..1.min(sentences.len())])?;
    println!("LOAD_AND_FIRST_MS {:.1} | {}", t.elapsed().as_secs_f64() * 1000.0, first.join(" "));
    for sentence in sentences {
        let batch = [sentence.clone()];
        let mut out = String::new();
        for _ in 0..2 {
            out = engine.translate(pair, beam, &batch)?.remove(0);
        }
        let mut times: Vec<f64> = Vec::new();
        for _ in 0..runs {
            let t = Instant::now();
            out = engine.translate(pair, beam, &batch)?.remove(0);
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(|a, b| a.total_cmp(b));
        println!("WORDS {} MEDIAN_MS {:.1} MIN_MS {:.1} | {}", sentence.split_whitespace().count(), times[times.len() / 2], times[0], out);
    }
    Ok(())
}

/// Translates a file, one text per line, the way the program does it: the line is cut into sentences, the chat abbreviations and the spoken
/// Arabic words are replaced (not with `--raw`), the sentences go to the network in batches. For measuring (docs/ACCURACY.md).
fn batch(args: &[String]) -> Result<()> {
    let (models, pair, beam, input, output) = match args {
        [models, pair, beam, input, output, ..] => (models, pair, beam.parse::<usize>()?, input, output),
        _ => return Err(anyhow!("usage: verba-translate batch <models folder> <pair> <beam> <input.txt> <output.txt> [--raw]")),
    };
    let raw = args.iter().any(|a| a == "--raw");
    let text = std::fs::read_to_string(input).with_context(|| format!("cannot read {input}"))?;
    let lines: Vec<&str> = text.lines().collect();
    let mut engine = Engine::new(PathBuf::from(models), std::thread::available_parallelism().map_or(4, |n| n.get().min(8)));
    let slang = verba_local::slang::shipped();
    let from_arabic = pair.starts_with("ar");
    let t = Instant::now();
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        let translated = verba_local::sentence::translate_with(line, |chunks| {
            let prepared: Vec<String> = if raw {
                chunks.to_vec()
            } else if from_arabic {
                chunks.iter().map(|c| verba_local::colloquial::shipped().expand(&slang.expand_into(c, verba_core::Lang::Ar))).collect()
            } else {
                chunks.iter().map(|c| slang.expand(c)).collect()
            };
            engine.translate(pair, beam, &prepared).map_err(|e| format!("{e:#}"))
        });
        // an empty line stays an empty line; a failure is written as an empty line too and counted
        out.push_str(translated.unwrap_or_default().replace('\n', " ").trim());
        out.push('\n');
        if (i + 1) % 100 == 0 {
            eprintln!("{} of {} lines, {:.0} s", i + 1, lines.len(), t.elapsed().as_secs_f64());
        }
    }
    std::fs::write(output, out).with_context(|| format!("cannot write {output}"))?;
    println!("{} lines in {:.0} s -> {output}", lines.len(), t.elapsed().as_secs_f64());
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("serve") => {
            let option = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1));
            let models = option("--models").ok_or_else(|| anyhow!("--models <folder> is needed"))?;
            let threads = option("--threads").and_then(|t| t.parse().ok()).unwrap_or(4);
            let idle = option("--idle-secs").and_then(|t| t.parse().ok()).unwrap_or(600);
            serve(PathBuf::from(models), threads, Duration::from_secs(idle))
        }
        Some("translate") => {
            let [_, models, pair, text] = args.as_slice() else {
                return Err(anyhow!("usage: verba-translate translate <models folder> <pair> <text>"));
            };
            let mut engine = Engine::new(PathBuf::from(models), 4);
            for line in engine.translate(pair, verba_local::sentence::BEAM, &[text.clone()])? {
                println!("{line}");
            }
            Ok(())
        }
        Some("bench") => bench(&args[1..]),
        Some("batch") => batch(&args[1..]),
        Some("tokens") => {
            // verba-translate tokens <models folder> <pair> <text>...: the pieces the source side makes of each text (`<unk>`: a character
            // the model does not know)
            let [_, models, pair, texts @ ..] = args.as_slice() else {
                return Err(anyhow!("usage: verba-translate tokens <models folder> <pair> <text>..."));
            };
            let tokenizer = Tokenizer::new(Path::new(models).join(pair)).context("cannot read source.spm")?;
            for text in texts {
                let pieces = ct2rs::Tokenizer::encode(&tokenizer, text).map_err(|e| anyhow!("{e}"))?;
                println!("{text}\t{}", pieces.join(" "));
            }
            Ok(())
        }
        _ => Err(anyhow!("usage: verba-translate serve --models <folder> | translate <models> <pair> <text> | bench ...")),
    }
}

#[cfg(test)]
mod tests {
    use super::valid_pair;

    #[test]
    fn only_plain_direction_names_are_accepted() {
        assert!(valid_pair("ar-en"));
        assert!(valid_pair("en-ar"));
        for bad in ["", "ar", "ar-en-fr", "../ar-en", "ar-en\\x", "AR-EN", "a-en", "ar/en", "ar-e1"] {
            assert!(!valid_pair(bad), "{bad}");
        }
    }
}
