//! Dictation: speech to text with Whisper (CTranslate2), in the same program as the sentence translator so that Verba carries one engine.
//!
//! ```text
//! verba-translate dictate --model <folder> [--threads 4] [--idle-secs 600]      (what Verba starts; talks over stdin and stdout)
//! verba-translate transcribe <model folder> <ar|en|fr|auto> <file.wav>          (16 kHz mono WAV, for a quick look)
//! ```
//!
//! A model folder is one Whisper model in CTranslate2 format (`model.bin`, `config.json`, `tokenizer.json`, `vocabulary.txt`,
//! `preprocessor_config.json`). Verba downloads one per language (the Arabic pack and the English pack) into its own folder and starts the program
//! with the folder of the language being dictated.
//!
//! The messages are frames of `verba_local::sentence` (a length, then the bytes). A request is one byte for the length of a language code,
//! the code (`ar`, `en`; empty lets Whisper decide), then the sound: 16 kHz, mono, 16-bit little-endian numbers. The answer is `T` and the text
//! (UTF-8), or `E` and the reason. The model is read at the first request (about 3 seconds) and the program ends after a while without work,
//! like the translator, which gives the memory back.

use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use ct2rs::{ComputeType, Config, Whisper, WhisperOptions};
use verba_local::sentence::{read_frame, write_frame};

/// Whisper works on windows of 30 seconds: a longer sound is cut by Whisper itself, but dictation is one sentence or two.
const SAMPLE_RATE: usize = 16_000;
/// A sound shorter than this holds no words (a click of the key).
const MIN_SAMPLES: usize = SAMPLE_RATE * 3 / 10;

/// The messages between Verba and this program.
pub fn encode_request(language: &str, pcm: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + language.len() + pcm.len() * 2);
    out.push(language.len() as u8);
    out.extend_from_slice(language.as_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// The language code and the samples of a request, as numbers between -1 and 1.
pub fn decode_request(frame: &[u8]) -> Result<(String, Vec<f32>)> {
    let n = *frame.first().ok_or_else(|| anyhow!("empty request"))? as usize;
    if frame.len() < 1 + n || n > 8 {
        return Err(anyhow!("bad request"));
    }
    let language = std::str::from_utf8(&frame[1..1 + n]).context("language code")?.to_string();
    let samples = frame[1 + n..].chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect();
    Ok((language, samples))
}

fn model(folder: &Path, threads: usize) -> Result<Whisper> {
    Whisper::new(folder, Config { compute_type: ComputeType::INT8, num_threads_per_replica: threads, ..Default::default() })
        .with_context(|| format!("cannot read the speech model in {}", folder.display()))
}

/// The text of a sound. An empty sound gives an empty text.
fn transcribe(whisper: &Whisper, language: &str, samples: &[f32]) -> Result<String> {
    if samples.len() < MIN_SAMPLES {
        return Ok(String::new());
    }
    let options = WhisperOptions { beam_size: 5, max_length: 224, ..Default::default() };
    let lang = (!language.is_empty() && language != "auto").then_some(language);
    let text = whisper.generate(samples, lang, false, &options)?.join(" ");
    Ok(text.trim().to_string())
}

/// What Verba starts: answers requests until the pipe closes or nothing was asked for `idle`.
pub fn serve(folder: PathBuf, threads: usize, idle: Duration) -> Result<()> {
    let start = Instant::now();
    static LAST: AtomicU64 = AtomicU64::new(0);
    static BUSY: AtomicBool = AtomicBool::new(false);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(5));
            let quiet = (start.elapsed().as_millis() as u64).saturating_sub(LAST.load(Ordering::SeqCst));
            if !BUSY.load(Ordering::SeqCst) && quiet >= idle.as_millis() as u64 {
                std::process::exit(0);
            }
        }
    });
    let mut whisper: Option<Whisper> = None;
    let mut input = BufReader::new(io::stdin().lock());
    let mut output = BufWriter::new(io::stdout().lock());
    while let Some(frame) = read_frame(&mut input)? {
        BUSY.store(true, Ordering::SeqCst);
        let answer = decode_request(&frame).and_then(|(language, samples)| {
            if whisper.is_none() {
                whisper = Some(model(&folder, threads)?);
            }
            transcribe(whisper.as_ref().ok_or_else(|| anyhow!("no model"))?, &language, &samples)
        });
        let mut reply = Vec::new();
        match answer {
            Ok(text) => {
                reply.push(b'T');
                reply.extend_from_slice(text.as_bytes());
            }
            Err(e) => {
                reply.push(b'E');
                reply.extend_from_slice(format!("{e:#}").as_bytes());
            }
        }
        write_frame(&mut output, &reply)?;
        LAST.store(start.elapsed().as_millis() as u64, Ordering::SeqCst);
        BUSY.store(false, Ordering::SeqCst);
    }
    Ok(())
}

/// Samples of a 16 kHz mono WAV file (16-bit integers or 32-bit floats), as numbers between -1 and 1.
fn read_wav(path: &Path) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path).with_context(|| path.display().to_string())?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(anyhow!("{} is not a WAV file", path.display()));
    }
    let (mut at, mut format, mut bits) = (12, 1u16, 16u16);
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if id == b"fmt " && at + 24 <= bytes.len() {
            format = u16::from_le_bytes([bytes[at + 8], bytes[at + 9]]);
            bits = u16::from_le_bytes([bytes[at + 22], bytes[at + 23]]);
        }
        if id == b"data" {
            let end = (at + 8 + size).min(bytes.len());
            let data = &bytes[at + 8..end];
            return match (format, bits) {
                (3, 32) => Ok(data.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()),
                (1, 16) => Ok(data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect()),
                _ => Err(anyhow!("{}: format {format}, {bits} bits is not read", path.display())),
            };
        }
        at += 8 + size + (size & 1);
    }
    Err(anyhow!("{}: no data chunk", path.display()))
}

/// `verba-translate transcribe <model folder> <language> <file.wav>`.
pub fn transcribe_file(args: &[String]) -> Result<()> {
    let [folder, language, wav] = args else {
        return Err(anyhow!("usage: verba-translate transcribe <model folder> <ar|en|fr|auto> <file.wav>"));
    };
    let whisper = model(Path::new(folder), 4)?;
    let samples = read_wav(Path::new(wav))?;
    println!("{}", transcribe(&whisper, language, &samples)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_comes_back_as_it_went() {
        let pcm: Vec<i16> = vec![0, 1000, -1000, 32767, -32768];
        let frame = encode_request("ar", &pcm);
        let (language, samples) = decode_request(&frame).unwrap();
        assert_eq!(language, "ar");
        assert_eq!(samples.len(), 5);
        assert!((samples[1] - 1000.0 / 32768.0).abs() < 1e-6 && (samples[4] + 1.0).abs() < 1e-6);
        // no language: Whisper decides
        assert_eq!(decode_request(&encode_request("", &pcm)).unwrap().0, "");
    }

    #[test]
    fn a_broken_request_is_an_error_not_a_panic() {
        assert!(decode_request(&[]).is_err());
        assert!(decode_request(&[5, b'a']).is_err(), "the language is cut");
        assert!(decode_request(&[200, 1, 2, 3]).is_err(), "a language code of 200 letters");
    }
}
