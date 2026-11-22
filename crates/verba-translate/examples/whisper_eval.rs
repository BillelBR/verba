//! Measures how well a Whisper model (CTranslate2 format) writes down speech, on the machine's own CPU:
//!
//!   cargo run --release -p verba-translate --example whisper_eval -- <model folder> <language: ar|fr|en> <utterances.tsv> <audio folder> <out.tsv> [--threads 4] [--beam 5] [--compute int8]
//!
//! `utterances.tsv` has `file name<TAB>reference` per line (the files are 16 kHz mono 16-bit WAV, FLEURS' format). The program writes
//! `file<TAB>hypothesis<TAB>seconds` and prints the load time, the audio length and the time spent, so that the speed is a number too.
//! The words error rate is computed by `scripts/wer.py`-like code outside (see docs/ACCURACY.md); this tool only listens.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use ct2rs::{ComputeType, Config, Whisper, WhisperOptions};

/// Samples of a 16 kHz mono WAV file (16-bit integers or 32-bit floats, FLEURS has the second), as numbers between -1 and 1.
fn read_wav(path: &Path) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path).with_context(|| path.display().to_string())?;
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(anyhow!("{} is not a WAV file", path.display()));
    }
    // the chunks: "fmt " says how the samples are written, "data" holds them
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

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 5 {
        eprintln!("usage: whisper_eval <model folder> <ar|fr|en> <utterances.tsv> <audio folder> <out.tsv> [--threads 4] [--beam 5] [--compute int8]");
        std::process::exit(2);
    }
    let opt = |name: &str, default: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| default.to_string());
    let threads: usize = opt("--threads", "4").parse().unwrap_or(4);
    let beam: usize = opt("--beam", "5").parse().unwrap_or(5);
    let compute = match opt("--compute", "int8").as_str() {
        "float32" => ComputeType::FLOAT32,
        "float16" => ComputeType::FLOAT16,
        "int8_float32" => ComputeType::INT8_FLOAT32,
        _ => ComputeType::INT8,
    };
    let (model, language, tsv, audio, out) = (&args[0], &args[1], &args[2], Path::new(&args[3]), &args[4]);

    let t = Instant::now();
    let whisper = Whisper::new(model, Config { compute_type: compute, num_threads_per_replica: threads, ..Default::default() })?;
    let load = t.elapsed();
    let options = WhisperOptions { beam_size: beam, ..Default::default() };
    let rate = whisper.sampling_rate();

    let (mut audio_seconds, mut work, mut n) = (0.0f64, 0.0f64, 0);
    let mut written = String::new();
    for line in std::fs::read_to_string(tsv)?.lines() {
        let Some((file, _)) = line.split_once('\t') else { continue };
        let samples = read_wav(&audio.join(file))?;
        audio_seconds += samples.len() as f64 / rate as f64;
        let t = Instant::now();
        let text = whisper.generate(&samples, Some(language), false, &options)?.join(" ");
        let secs = t.elapsed().as_secs_f64();
        work += secs;
        n += 1;
        written.push_str(&format!("{file}\t{}\t{secs:.2}\n", text.trim().replace(['\t', '\n'], " ")));
    }
    std::fs::write(out, written)?;
    println!(
        "{model}: loaded in {:.1}s; {n} utterances, {:.0}s of speech decoded in {:.0}s ({:.2}x real time, {:.1}s per utterance), {threads} threads, beam {beam}",
        load.as_secs_f64(),
        audio_seconds,
        work,
        work / audio_seconds.max(0.001),
        work / n.max(1) as f64
    );
    Ok(())
}
