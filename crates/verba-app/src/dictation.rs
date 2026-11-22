//! Dictation: speech to text on this PC, with a speech model per language that the person downloads from the settings.
//!
//! - **Packs.** One per language, Arabic and English, each a Whisper model (OpenAI, converted for CTranslate2 by Systran; MIT and Apache-2.0). They are
//!   not in the installer: *Settings > Dictation > Download* fetches the files of one (`curl.exe`, which Windows brings: resumable, with the progress
//!   read from the size of the file), checks the size and the SHA-256 of every file against the catalogue below (the revision of the repository is in
//!   the address, so the files cannot change under it), and only then marks the pack as installed. The folder is `%APPDATA%\\Verba\\dictation\\<pack>`.
//! - **Recording.** The default microphone, 16 kHz mono, through the old `waveIn` interface (no extra program). The recording stops when the person presses the
//!   shortcut again, after 2 seconds of silence once words were heard, or after 28 seconds.
//! - **Writing.** The sound goes to `verba-translate.exe dictate --model <pack folder>` (see `crates/verba-translate/src/dictate.rs`), which keeps the model
//!   loaded while it is used and ends itself after ten minutes without work. A request that is not answered in time stops the program.
//!
//! Nothing is sent over the network except the download of the pack itself. The sound is never written to a file.

use std::io::{BufReader, BufWriter};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Mutex;
use std::time::Duration;

use verba_local::sentence::{read_frame, write_frame};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ───────────── the packs ─────────────

/// One file of a pack: where it comes from, and what it must be (size and SHA-256 in hex).
pub struct PackFile {
    pub name: &'static str,
    pub url: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

/// The speech model of one language.
pub struct Pack {
    /// `ar` or `en`: also the language Whisper is told to write.
    pub id: &'static str,
    pub title: &'static str,
    pub language: &'static str,
    /// The folder under `%APPDATA%\\Verba\\dictation`.
    pub folder: &'static str,
    pub files: &'static [PackFile],
}

impl Pack {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }
}

/// The packs. The files are pinned to a revision of their repository, with the size and the SHA-256 measured on the copies used for
/// docs/ACCURACY.md (`scripts/` has no tool for it: download, hash, paste; the first file of each pack is the smallest, so a wrong address fails fast).
/// Arabic: `Systran/faster-whisper-small` (OpenAI Whisper small, multilingual, written for Arabic). English: `Systran/faster-whisper-small.en`
/// (Whisper small trained on English alone). Both MIT; the preprocessing settings come from `openai/whisper-small` (Apache-2.0).
pub static PACKS: &[Pack] = &[
    Pack {
        id: "ar",
        title: "Arabic",
        language: "ar",
        folder: "whisper-small",
        files: &[
            PackFile { name: "config.json", url: "https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/config.json", bytes: 2370, sha256: "b55496ac7940a7ae47d2c01eab40edfd8701feec1229d9cce3b40014383fb828" },
            PackFile { name: "preprocessor_config.json", url: "https://huggingface.co/openai/whisper-small/resolve/973afd24965f72e36ca33b3055d56a652f456b4d/preprocessor_config.json", bytes: 184990, sha256: "9b5cd03a36fbb8a627c64d98a5b5b126ead95a77720723944487311f0110b666" },
            PackFile { name: "tokenizer.json", url: "https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/tokenizer.json", bytes: 2203239, sha256: "fb7b63191e9bb045082c79fd742a3106a12c99513ab30df4a0d47fa6cb6fd0ab" },
            PackFile { name: "vocabulary.txt", url: "https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/vocabulary.txt", bytes: 459861, sha256: "34ce3fe1c5041027b3f8d42912270993f986dbc4bb34cf27f951e34a1e453913" },
            PackFile { name: "model.bin", url: "https://huggingface.co/Systran/faster-whisper-small/resolve/536b0662742c02347bc0e980a01041f333bce120/model.bin", bytes: 483546902, sha256: "3e305921506d8872816023e4c273e75d2419fb89b24da97b4fe7bce14170d671" },
        ],
    },
    Pack {
        id: "en",
        title: "English",
        language: "en",
        folder: "whisper-small-en",
        files: &[
            PackFile { name: "config.json", url: "https://huggingface.co/Systran/faster-whisper-small.en/resolve/d1d751a5f8271d482d14ca55d9e2deeebbae577f/config.json", bytes: 2657, sha256: "666a9605530ac1f61fa8177f3702b4dacec9966749e42610839fcc32661d5fae" },
            PackFile { name: "preprocessor_config.json", url: "https://huggingface.co/openai/whisper-small.en/resolve/e8727524f962ee844a7319d92be39ac1bd25655a/preprocessor_config.json", bytes: 184990, sha256: "9b5cd03a36fbb8a627c64d98a5b5b126ead95a77720723944487311f0110b666" },
            PackFile { name: "tokenizer.json", url: "https://huggingface.co/Systran/faster-whisper-small.en/resolve/d1d751a5f8271d482d14ca55d9e2deeebbae577f/tokenizer.json", bytes: 2128466, sha256: "929c5252409436dce1b38a75d1abbcb5e132d170d8e324e4e04ed915fa2d22df" },
            PackFile { name: "vocabulary.txt", url: "https://huggingface.co/Systran/faster-whisper-small.en/resolve/d1d751a5f8271d482d14ca55d9e2deeebbae577f/vocabulary.txt", bytes: 422309, sha256: "ff77588746d3a2595d32ab5b69ffd7b95ce2441ac57533cb66fc3eb575a115cf" },
            PackFile { name: "model.bin", url: "https://huggingface.co/Systran/faster-whisper-small.en/resolve/d1d751a5f8271d482d14ca55d9e2deeebbae577f/model.bin", bytes: 483545366, sha256: "62b2a45b05ee59acb4a5341b33ee35e041395d378d418a18acfe4c9e768ee37a" },
        ],
    },
];

pub fn pack(id: &str) -> Option<&'static Pack> {
    PACKS.iter().find(|p| p.id == id)
}

/// The folder of the packs.
fn root() -> Option<PathBuf> {
    crate::settings::Settings::dir().map(|d| d.join("dictation"))
}

pub fn pack_dir(pack: &Pack) -> Option<PathBuf> {
    root().map(|r| r.join(pack.folder))
}

/// Whether the pack is on this PC: every file has its size, and the mark that the download was checked is there.
pub fn installed(pack: &Pack) -> bool {
    let Some(dir) = pack_dir(pack) else { return false };
    dir.join(".verified").is_file() && pack.files.iter().all(|f| std::fs::metadata(dir.join(f.name)).is_ok_and(|m| m.len() == f.bytes))
}

/// Takes the pack off the disk.
pub fn remove(pack: &Pack) {
    if let Some(dir) = pack_dir(pack) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// The size on disk, for the settings: "484 MB".
pub fn megabytes(bytes: u64) -> String {
    format!("{} MB", (bytes as f64 / 1_000_000.0).round() as u64)
}

// ───────────── the download ─────────────

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Downloading,
    Verifying,
    Failed(String),
}

/// What the download is doing, for the settings window.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub pack: &'static str,
    pub done: u64,
    pub total: u64,
    pub phase: Phase,
}

impl Progress {
    /// 0.0 to 1.0.
    pub fn fraction(&self) -> f32 {
        if self.total == 0 { 0.0 } else { (self.done as f64 / self.total as f64).min(1.0) as f32 }
    }
}

static PROGRESS: Mutex<Option<Progress>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);

/// The download in progress (or the one that failed, until the person looks again).
pub fn progress() -> Option<Progress> {
    PROGRESS.lock().ok().and_then(|p| p.clone())
}

/// Forgets a failed download (the person has read the reason).
pub fn clear_failure() {
    if let Ok(mut p) = PROGRESS.lock() {
        if matches!(p.as_ref().map(|p| &p.phase), Some(Phase::Failed(_))) {
            *p = None;
        }
    }
}

fn set_progress(pack: &'static str, done: u64, total: u64, phase: Phase) {
    if let Ok(mut p) = PROGRESS.lock() {
        *p = Some(Progress { pack, done, total, phase });
    }
}

pub fn cancel_download() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// The free bytes of the disk the folder is on.
fn free_bytes(dir: &Path) -> Option<u64> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::PCWSTR;
    // the folder may not exist yet: ask about the nearest one that does
    let mut at = dir;
    while !at.exists() {
        at = at.parent()?;
    }
    let wide: Vec<u16> = at.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None).ok()? };
    Some(free)
}

/// The hash line of `certutil -hashfile <file> SHA256`, in lower case.
pub fn parse_certutil(output: &str) -> Option<String> {
    output
        .lines()
        .map(|l| l.replace(' ', "").to_lowercase())
        .find(|l| l.len() == 64 && l.chars().all(|c| c.is_ascii_hexdigit()))
}

fn sha256_of(path: &Path) -> Result<String, String> {
    let out = Command::new("certutil")
        .arg("-hashfile")
        .arg(path)
        .arg("SHA256")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("could not check the file: {e}"))?;
    parse_certutil(&String::from_utf8_lossy(&out.stdout)).ok_or_else(|| "could not read the checksum of a file".to_string())
}

/// Starts the download of a pack on a thread of its own. `notify` is called about twice a second while it works and at the end. Returns at once;
/// an error here is why it did not start (another download runs, no room on the disk).
pub fn start_download(pack: &'static Pack, notify: impl Fn() + Send + 'static) -> Result<(), String> {
    if progress().is_some_and(|p| !matches!(p.phase, Phase::Failed(_))) {
        return Err("Another download is running.".to_string());
    }
    let dir = pack_dir(pack).ok_or("Verba has no folder for its files.")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not make the folder: {e}"))?;
    // the disk must hold the files with some room to spare (a full disk breaks other programs, not only this one)
    let need = pack.total_bytes() + pack.total_bytes() / 10 + 200_000_000;
    if let Some(free) = free_bytes(&dir) {
        if free < need {
            return Err(format!("Not enough room on the disk: {} are needed and {} are free.", megabytes(need), megabytes(free)));
        }
    }
    CANCEL.store(false, Ordering::SeqCst);
    set_progress(pack.id, 0, pack.total_bytes(), Phase::Downloading);
    std::thread::spawn(move || {
        let result = download_all(pack, &dir, &notify);
        match result {
            Ok(()) => {
                let _ = std::fs::write(dir.join(".verified"), "Every file of this pack had the size and the SHA-256 it should have.\n");
                if let Ok(mut p) = PROGRESS.lock() {
                    *p = None;
                }
            }
            Err(why) => {
                let cancelled = CANCEL.load(Ordering::SeqCst);
                if cancelled {
                    if let Ok(mut p) = PROGRESS.lock() {
                        *p = None;
                    }
                } else {
                    set_progress(pack.id, 0, pack.total_bytes(), Phase::Failed(why));
                }
            }
        }
        notify();
    });
    Ok(())
}

fn download_all(pack: &'static Pack, dir: &Path, notify: &dyn Fn()) -> Result<(), String> {
    let total = pack.total_bytes();
    let mut done_before = 0u64;
    for file in pack.files {
        let target = dir.join(file.name);
        // a file that is there from an earlier try, whole and right, is kept
        if std::fs::metadata(&target).is_ok_and(|m| m.len() == file.bytes) && sha256_of(&target).is_ok_and(|h| h == file.sha256) {
            done_before += file.bytes;
            set_progress(pack.id, done_before, total, Phase::Downloading);
            continue;
        }
        let part = dir.join(format!("{}.part", file.name));
        let mut child = Command::new("curl.exe")
            .args(["-L", "--fail", "--silent", "--show-error", "--retry", "3", "--retry-delay", "2", "-C", "-", "-o"])
            .arg(&part)
            .arg(file.url)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|_| "Windows has no curl.exe here, so Verba cannot download.".to_string())?;
        loop {
            if CANCEL.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("cancelled".to_string());
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        let mut why = String::new();
                        if let Some(mut e) = child.stderr.take() {
                            let _ = std::io::Read::read_to_string(&mut e, &mut why);
                        }
                        let why = why.trim();
                        return Err(format!("The download of {} failed{}", file.name, if why.is_empty() { ".".to_string() } else { format!(": {why}") }));
                    }
                    break;
                }
                Ok(None) => {
                    let got = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
                    set_progress(pack.id, done_before + got.min(file.bytes), total, Phase::Downloading);
                    notify();
                    std::thread::sleep(Duration::from_millis(500));
                }
                Err(e) => return Err(format!("Could not follow the download: {e}")),
            }
        }
        set_progress(pack.id, done_before + file.bytes, total, Phase::Verifying);
        notify();
        let size = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if size != file.bytes {
            let _ = std::fs::remove_file(&part);
            return Err(format!("{} has {size} bytes, not {}: the download was cut. Try again.", file.name, file.bytes));
        }
        if sha256_of(&part)? != file.sha256 {
            let _ = std::fs::remove_file(&part);
            return Err(format!("{} is not the file that was expected (its checksum differs). Try again later.", file.name));
        }
        std::fs::rename(&part, &target).map_err(|e| format!("Could not put {} in place: {e}", file.name))?;
        done_before += file.bytes;
        set_progress(pack.id, done_before, total, Phase::Downloading);
    }
    Ok(())
}

// ───────────── the microphone ─────────────

/// Tells when speech has begun and when it has ended, from the loudness of each short piece of sound.
#[derive(Debug, Default)]
pub struct Vad {
    /// The quietest piece seen: the noise of the room.
    floor: f32,
    seen: u32,
    heard: bool,
    silent_ms: u32,
    total_ms: u32,
    /// The loudest piece, to tell a recording of silence from one of speech.
    peak: f32,
}

/// Silence that ends a recording once words were heard, and the longest recording.
pub const SILENCE_STOP_MS: u32 = 2000;
pub const MAX_MS: u32 = 28_000;
/// Below this loudness (of a full scale of 1.0) a recording has no speech in it, whatever the noise floor says.
const MIN_SPEECH_RMS: f32 = 0.012;

impl Vad {
    /// The loudness (root mean square, 0 to 1) of a piece of `ms` milliseconds.
    pub fn rms(chunk: &[i16]) -> f32 {
        if chunk.is_empty() {
            return 0.0;
        }
        (chunk.iter().map(|s| (*s as f64 / 32768.0).powi(2)).sum::<f64>() / chunk.len() as f64).sqrt() as f32
    }

    pub fn feed(&mut self, chunk: &[i16], ms: u32) {
        let rms = Self::rms(chunk);
        self.total_ms += ms;
        self.peak = self.peak.max(rms);
        self.floor = if self.seen == 0 { rms } else { self.floor.min(rms) };
        self.seen += 1;
        // speech is clearly above the noise of the room, and above a floor of its own
        let threshold = MIN_SPEECH_RMS.max(self.floor * 3.0);
        if rms >= threshold {
            self.heard = true;
            self.silent_ms = 0;
        } else if self.heard {
            self.silent_ms += ms;
        }
    }

    pub fn heard_speech(&self) -> bool {
        self.heard
    }

    /// Whether the recording should end now by itself.
    pub fn should_stop(&self) -> bool {
        (self.heard && self.silent_ms >= SILENCE_STOP_MS) || self.total_ms >= MAX_MS
    }

    /// Every sample was zero: Windows gave nothing (the microphone is blocked in its privacy settings, or muted at the driver).
    pub fn dead(&self) -> bool {
        self.seen > 3 && self.peak == 0.0
    }

    pub fn seconds(&self) -> f32 {
        self.total_ms as f32 / 1000.0
    }
}

/// A recording in progress.
pub struct Recorder {
    hwi: windows::Win32::Media::Audio::HWAVEIN,
    headers: Vec<Box<windows::Win32::Media::Audio::WAVEHDR>>,
    buffers: Vec<Box<[i16]>>,
    pub samples: Vec<i16>,
    pub vad: Vad,
}

const SAMPLE_RATE: u32 = 16_000;
/// A buffer holds 100 milliseconds.
const BUFFER_SAMPLES: usize = 1600;
const BUFFERS: usize = 8;

impl Recorder {
    /// Opens the default microphone and starts listening.
    pub fn start() -> Result<Self, String> {
        use windows::Win32::Media::Audio::{
            CALLBACK_NULL, HWAVEIN, WAVE_FORMAT_PCM, WAVE_MAPPER, WAVEFORMATEX, WAVEHDR, waveInAddBuffer, waveInOpen, waveInPrepareHeader, waveInStart,
        };
        unsafe {
            let format = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: 1,
                nSamplesPerSec: SAMPLE_RATE,
                nAvgBytesPerSec: SAMPLE_RATE * 2,
                nBlockAlign: 2,
                wBitsPerSample: 16,
                cbSize: 0,
            };
            let mut hwi = HWAVEIN::default();
            if windows::Win32::Media::Audio::waveInGetNumDevs() == 0 {
                return Err("No microphone was found: connect one, or check that Windows sees it (Settings, System, Sound, Input).".to_string());
            }
            let opened = waveInOpen(Some(&mut hwi), WAVE_MAPPER, &format, None, None, CALLBACK_NULL);
            if opened != 0 {
                return Err(format!("Verba could not open the microphone (error {opened}): is it allowed in Windows, Settings, Privacy and security, Microphone, \"Let desktop apps access your microphone\"?"));
            }
            let mut me = Recorder { hwi, headers: Vec::new(), buffers: Vec::new(), samples: Vec::new(), vad: Vad::default() };
            for _ in 0..BUFFERS {
                let mut buffer = vec![0i16; BUFFER_SAMPLES].into_boxed_slice();
                let mut header = Box::new(WAVEHDR {
                    lpData: windows::core::PSTR(buffer.as_mut_ptr() as *mut u8),
                    dwBufferLength: (BUFFER_SAMPLES * 2) as u32,
                    ..Default::default()
                });
                waveInPrepareHeader(hwi, &mut *header, std::mem::size_of::<WAVEHDR>() as u32);
                waveInAddBuffer(hwi, &mut *header, std::mem::size_of::<WAVEHDR>() as u32);
                me.buffers.push(buffer);
                me.headers.push(header);
            }
            if waveInStart(hwi) != 0 {
                me.close();
                return Err("Verba could not start the microphone.".to_string());
            }
            Ok(me)
        }
    }

    /// Takes what the microphone gave since the last call. Returns whether the recording should end by itself.
    pub fn poll(&mut self) -> bool {
        use windows::Win32::Media::Audio::{WAVEHDR, waveInAddBuffer};
        const WHDR_DONE: u32 = 1;
        unsafe {
            for i in 0..self.headers.len() {
                if self.headers[i].dwFlags & WHDR_DONE == 0 {
                    continue;
                }
                let got = (self.headers[i].dwBytesRecorded as usize / 2).min(BUFFER_SAMPLES);
                let chunk = self.buffers[i][..got].to_vec();
                self.vad.feed(&chunk, (got as u32 * 1000) / SAMPLE_RATE);
                self.samples.extend_from_slice(&chunk);
                self.headers[i].dwFlags &= !WHDR_DONE;
                self.headers[i].dwBytesRecorded = 0;
                waveInAddBuffer(self.hwi, &mut *self.headers[i], std::mem::size_of::<WAVEHDR>() as u32);
            }
        }
        self.vad.should_stop()
    }

    fn close(&mut self) {
        use windows::Win32::Media::Audio::{WAVEHDR, waveInClose, waveInReset, waveInUnprepareHeader};
        unsafe {
            waveInReset(self.hwi);
            for h in &mut self.headers {
                waveInUnprepareHeader(self.hwi, &mut **h, std::mem::size_of::<WAVEHDR>() as u32);
            }
            waveInClose(self.hwi);
        }
        self.headers.clear();
    }

    /// Stops listening and gives the whole recording.
    pub fn finish(mut self) -> (Vec<i16>, Vad) {
        self.poll();
        self.close();
        (std::mem::take(&mut self.samples), std::mem::take(&mut self.vad))
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        if !self.headers.is_empty() {
            self.close();
        }
    }
}

// ───────────── the writer: verba-translate.exe dictate ─────────────

struct Session {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
    /// The pack whose model it has loaded.
    pack: &'static str,
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);
/// A request not answered after this long is given up: the program is stopped. (Reading the model takes about 3 seconds, writing about 6.)
const ANSWER_WITHIN: Duration = Duration::from_secs(90);

/// The request frame: the length of the language code, the code, then the sound.
pub fn encode_request(language: &str, pcm: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + language.len() + pcm.len() * 2);
    out.push(language.len() as u8);
    out.extend_from_slice(language.as_bytes());
    for s in pcm {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// What the program answered: `T` and the text, or `E` and the reason.
pub fn decode_reply(frame: &[u8]) -> Result<String, String> {
    match frame.split_first() {
        Some((b'T', text)) => Ok(String::from_utf8_lossy(text).trim().to_string()),
        Some((b'E', why)) => Err(String::from_utf8_lossy(why).to_string()),
        _ => Err("the speech program gave an answer that is not understood".to_string()),
    }
}

/// Writes down a recording with the model of the pack. Blocks until the answer is there, so it runs on a thread of its own.
pub fn transcribe(exe: &Path, pack: &'static Pack, pcm: &[i16]) -> Result<String, String> {
    use std::os::windows::io::AsRawHandle;
    let dir = pack_dir(pack).ok_or("Verba has no folder for its files.")?;
    if !installed(pack) {
        return Err(format!("The {} dictation pack is not installed.", pack.title));
    }
    let mut slot = SESSION.lock().map_err(|_| "the speech program is stuck".to_string())?;
    // another language was used before: its program is ended, this one's is started
    if slot.as_ref().is_some_and(|s| s.pack != pack.id) {
        *slot = None;
    }
    for attempt in 0..2 {
        if slot.is_none() {
            let mut child = Command::new(exe)
                .args(["dictate", "--model"])
                .arg(&dir)
                .args(["--threads", "4", "--idle-secs", "600"])
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map_err(|e| format!("could not start the speech program: {e}"))?;
            let input = child.stdin.take().ok_or("no pipe to the speech program")?;
            let output = child.stdout.take().ok_or("no pipe from the speech program")?;
            *slot = Some(Session { child, input: BufWriter::new(input), output: BufReader::new(output), pack: pack.id });
        }
        let session = slot.as_mut().ok_or("no speech program")?;
        let handle = session.child.as_raw_handle() as usize;
        let (done, waiting) = mpsc::channel::<()>();
        let watchdog = std::thread::spawn(move || {
            if matches!(waiting.recv_timeout(ANSWER_WITHIN), Err(RecvTimeoutError::Timeout)) {
                unsafe {
                    let _ = windows::Win32::System::Threading::TerminateProcess(windows::Win32::Foundation::HANDLE(handle as *mut _), 1);
                }
                true
            } else {
                false
            }
        });
        let outcome = write_frame(&mut session.input, &encode_request(pack.language, pcm))
            .and_then(|_| read_frame(&mut session.output))
            .map_err(|e| e.to_string())
            .and_then(|frame| frame.ok_or_else(|| "the speech program closed".to_string()))
            .and_then(|frame| decode_reply(&frame));
        let _ = done.send(());
        if watchdog.join().unwrap_or(false) {
            *slot = None;
            return Err("the speech program did not answer in time".to_string());
        }
        match outcome {
            Ok(text) => return Ok(text),
            // it answered that it could not write (a damaged model...): the reason is the answer
            Err(why) if why.starts_with("cannot read") || attempt == 1 => {
                *slot = None;
                return Err(why);
            }
            // it was gone (it ended by itself between two dictations): one more try with a fresh one
            Err(_) => *slot = None,
        }
    }
    Err("the speech program could not be started".to_string())
}

/// Ends the speech program (a pack was removed, or Verba quits).
pub fn end_session() {
    if let Ok(mut s) = SESSION.lock() {
        *s = None;
    }
}

/// (Development builds, for the checks of `VERBA_TEST_DICTATION`.) The samples of a 16 kHz mono WAV file with 16-bit integers or 32-bit floats.
#[cfg(debug_assertions)]
pub fn read_wav_pcm(path: &Path) -> Result<Vec<i16>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let (mut at, mut format, mut bits) = (12usize, 1u16, 16u16);
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if &bytes[at..at + 4] == b"fmt " {
            format = u16::from_le_bytes([bytes[at + 8], bytes[at + 9]]);
            bits = u16::from_le_bytes([bytes[at + 22], bytes[at + 23]]);
        }
        if &bytes[at..at + 4] == b"data" {
            let data = &bytes[at + 8..(at + 8 + size).min(bytes.len())];
            return match (format, bits) {
                (3, 32) => Ok(data.chunks_exact(4).map(|b| (f32::from_le_bytes([b[0], b[1], b[2], b[3]]) * 32767.0) as i16).collect()),
                (1, 16) => Ok(data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()),
                _ => Err(format!("format {format}, {bits} bits")),
            };
        }
        at += 8 + size + (size & 1);
    }
    Err("no data".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_is_well_formed_and_pinned() {
        assert!(pack("ar").is_some() && pack("en").is_some() && pack("fr").is_none());
        for p in PACKS {
            assert_eq!(p.language, p.id);
            assert!(p.files.iter().any(|f| f.name == "model.bin") && p.files.iter().any(|f| f.name == "preprocessor_config.json"), "{}", p.id);
            for f in p.files {
                assert!(f.url.starts_with("https://huggingface.co/") && f.url.ends_with(f.name), "{}", f.url);
                // pinned to a revision (40 hex digits), never to `main`
                let rev = f.url.split("/resolve/").nth(1).and_then(|r| r.split('/').next()).unwrap_or("");
                assert!(rev.len() == 40 && rev.chars().all(|c| c.is_ascii_hexdigit()), "{}", f.url);
                assert!(f.sha256.len() == 64 && f.sha256.chars().all(|c| c.is_ascii_hexdigit()), "{}", f.name);
                assert!(f.bytes > 0);
            }
        }
        // each pack is a few hundred megabytes: the size the settings announce
        for p in PACKS {
            let mb = p.total_bytes() / 1_000_000;
            assert!((100..700).contains(&mb), "{} is {mb} MB", p.id);
        }
    }

    #[test]
    fn the_checksum_is_read_from_the_output_of_certutil() {
        // certutil writes the 32 bytes as pairs of hex digits separated by spaces, between a header line and a trailer line
        let bytes: Vec<String> = (0..32).map(|i| format!("{:02x}", i * 7)).collect();
        let out = format!("SHA256 hash of file C:\\x\\model.bin:\r\n{}\r\nCertUtil: -hashfile command completed successfully.\r\n", bytes.join(" "));
        assert_eq!(parse_certutil(&out), Some(bytes.concat()));
        assert_eq!(parse_certutil("CertUtil: -hashfile command FAILED: 0x80070002\r\n"), None);
        // the line without spaces, in capitals
        let hex = "ABCDEF0123456789".repeat(4);
        assert_eq!(parse_certutil(&format!("hash:\n{hex}\ndone")).as_deref(), Some(hex.to_lowercase().as_str()));
    }

    #[test]
    fn a_pack_is_installed_only_when_every_file_is_there_whole_and_was_verified() {
        // a small pack of our own in a temporary folder
        static FILES: &[PackFile] = &[PackFile { name: "a.bin", url: "https://example.org/x/resolve/0000000000000000000000000000000000000000/a.bin", bytes: 3, sha256: "" }];
        static PACK: Pack = Pack { id: "xx", title: "Test", language: "xx", folder: "verba-test-pack", files: FILES };
        let tmp = std::env::temp_dir().join(format!("verba-appdata-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        // `Settings::dir` reads APPDATA: this test points it at the temporary folder (the other tests do not read it)
        unsafe { std::env::set_var("APPDATA", &tmp) };
        let dir = pack_dir(&PACK).unwrap();
        assert!(!installed(&PACK), "nothing yet");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.bin"), b"abc").unwrap();
        assert!(!installed(&PACK), "the files are there but nobody checked them");
        std::fs::write(dir.join(".verified"), "x").unwrap();
        assert!(installed(&PACK));
        std::fs::write(dir.join("a.bin"), b"ab").unwrap();
        assert!(!installed(&PACK), "a file of the wrong size");
        remove(&PACK);
        assert!(!dir.exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// `n` pieces of 100 ms: loud ones (speech) or quiet ones (noise of the room).
    fn feed(vad: &mut Vad, pieces: &[(bool, usize)]) {
        for (loud, n) in pieces {
            for i in 0..*n {
                let level = if *loud { 6000 } else { 40 };
                let chunk: Vec<i16> = (0..1600).map(|k| if (k + i) % 2 == 0 { level } else { -level }).collect();
                vad.feed(&chunk, 100);
            }
        }
    }

    #[test]
    fn the_recording_ends_after_two_seconds_of_silence_once_words_were_heard() {
        let mut vad = Vad::default();
        feed(&mut vad, &[(false, 10)]);
        assert!(!vad.heard_speech() && !vad.should_stop(), "a quiet room is not the end of anything: nobody spoke yet");
        feed(&mut vad, &[(true, 15), (false, 10)]);
        assert!(vad.heard_speech() && !vad.should_stop(), "one second of silence after speech");
        feed(&mut vad, &[(false, 11)]);
        assert!(vad.should_stop(), "more than two seconds");
        // speech again before the end resets the silence
        let mut vad = Vad::default();
        feed(&mut vad, &[(true, 5), (false, 15), (true, 5), (false, 15)]);
        assert!(!vad.should_stop());
    }

    #[test]
    fn a_long_recording_is_cut_and_a_silent_device_is_noticed() {
        let mut vad = Vad::default();
        feed(&mut vad, &[(true, 285)]);
        assert!(vad.should_stop(), "28 seconds is the longest");
        let mut dead = Vad::default();
        for _ in 0..10 {
            dead.feed(&[0i16; 1600], 100);
        }
        assert!(dead.dead() && !dead.heard_speech());
        assert!(!vad.dead());
        assert!((vad.seconds() - 28.5).abs() < 0.01);
    }

    #[test]
    fn a_request_and_its_answer_are_framed_as_the_speech_program_expects() {
        let frame = encode_request("ar", &[1, -2, 3]);
        assert_eq!(&frame[..3], &[2, b'a', b'r']);
        assert_eq!(frame.len(), 3 + 6);
        assert_eq!(i16::from_le_bytes([frame[5], frame[6]]), -2, "samples are little-endian 16-bit numbers");
        assert_eq!(decode_reply(b"T hello ").unwrap(), "hello");
        assert_eq!(decode_reply("Tمرحبا".as_bytes()).unwrap(), "مرحبا");
        assert_eq!(decode_reply(b"Ecannot read the speech model").unwrap_err(), "cannot read the speech model");
        assert!(decode_reply(b"").is_err() && decode_reply(b"Zx").is_err());
    }

    #[test]
    fn the_size_is_said_in_megabytes() {
        assert_eq!(megabytes(483_546_902), "484 MB");
        assert_eq!(megabytes(0), "0 MB");
        let p = Progress { pack: "ar", done: 50, total: 200, phase: Phase::Downloading };
        assert!((p.fraction() - 0.25).abs() < 1e-6);
        assert_eq!(Progress { total: 0, ..p.clone() }.fraction(), 0.0);
        assert_eq!(Progress { done: 500, ..p }.fraction(), 1.0);
    }
}
