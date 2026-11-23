//! Verba for Windows.
//!
//! A tray program. While it is on, it looks at the word you are typing, in any app, and shows
//! suggestions in a small popup above it. Tab accepts the first one, Esc closes the popup.
//!
//!   Ctrl+Alt+Shift+Q   quit
//!   Ctrl+Alt+Shift+V   pause / resume
//!
//! Privacy: it reads only the few letters before the caret, only in text fields that are not
//! password fields, and keeps nothing. The keyboard hook looks at one thing: whether the key is
//! Tab or Esc while the popup is open.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicUsize, Ordering};

mod backup;
mod diagnostics;
mod dictation;
mod fixkeys;
mod sync;
mod update;
mod keys;
mod learning;
mod sentences;
mod settings;
mod settings_window;

use keys::{Binding, DEFAULT_KEYS, Keys};
use settings::{Settings, ThemeMode};
use verba_core::{Engine, Lang, Suggestion, current_word, current_word_dz};
use verba_local::{LocalEngine, PersonalDict, UserEntry, UserTranslations};
use verba_win::{Anchor, AnchorKind, Locator, Picked, Look, Overlay, Snapshot, Theme, describe_foreground, foreground_app, run_with_splash};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOD_ALT,
    MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, SendInput, UnregisterHotKey, VIRTUAL_KEY, VK_BACK, VK_CONTROL,
    VK_MENU, VK_SHIFT,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallNextHookEx, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, HHOOK, HICON, ICONINFO, KBDLLHOOKSTRUCT,
    MB_ICONINFORMATION, MB_OK, MessageBoxW, MF_GRAYED, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, SetWindowsHookExW, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_COMMAND, WM_DESTROY, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONUP,
    WM_NULL, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const WM_TRAY: u32 = WM_APP + 1;
const WM_ACCEPT: u32 = WM_APP + 2;
const WM_DISMISS: u32 = WM_APP + 3;
/// A fresh reading from the worker thread is waiting in `LATEST`.
const WM_SNAPSHOT: u32 = WM_APP + 4;
/// A key moved the highlight: wParam is 0 for the left key, 1 for the right key.
const WM_MOVE: u32 = WM_APP + 5;
/// The reader thread read the word to translate: it is waiting in `PICKED`.
const WM_TRANSLATE: u32 = WM_APP + 6;
/// A notice has been on the screen long enough: wParam is its number.
const WM_NOTICE_END: u32 = WM_APP + 7;
/// A sentence translation is done (or failed): its result is waiting in `SENTENCE_DONE`.
const WM_SENTENCE: u32 = WM_APP + 8;
/// The reader thread read the selection to put right (typed with the wrong keyboard): it is waiting in `FIXKEYS_PICKED`.
const WM_FIXKEYS: u32 = WM_APP + 9;
/// The answer of a check for a newer version arrived (see `update.rs`).
const WM_UPDATE: u32 = WM_APP + 10;
/// The download of a dictation pack moved (or ended): the settings window draws again.
const WM_DICTATION: u32 = WM_APP + 11;
/// A dictation was written down (or failed): the text is typed.
const WM_DICTATED: u32 = WM_APP + 12;
/// The timer that listens to the microphone while a dictation is recorded.
const TIMER_DICTATE: usize = 4711;
/// wParam of `WM_ACCEPT` that means "the highlighted one" (a click sends the number of the candidate instead).
const ACCEPT_HIGHLIGHTED: usize = usize::MAX;
/// How often the worker thread looks at the app being typed in.
const POLL_MS: u64 = 120;
const HOTKEY_QUIT: i32 = 1;
const HOTKEY_PAUSE: i32 = 2;
const HOTKEY_APP: i32 = 3;
const HOTKEY_TRANSLATE: i32 = 4;
const HOTKEY_FIXKEYS: i32 = 5;
const HOTKEY_UNDO: i32 = 6;
const HOTKEY_DICTATE: i32 = 7;
const HOTKEY_QUICK: i32 = 8;
const CMD_PAUSE: usize = 1001;
const CMD_QUIT: usize = 1002;
const CMD_APP: usize = 1003;
const CMD_SETTINGS: usize = 1004;
const CMD_CHECK: usize = 1005;
const TRAY_ID: u32 = 1;
/// Stamped on the keys Verba itself sends, so its own keyboard hook can tell them from anyone else's
/// (remote-desktop tools and accessibility programs also send keys, and those must still work).
const VERBA_KEY_MARK: usize = 0x5645_5242; // "VERB"

// what the keyboard hook needs to know without touching the rest of the program
static POPUP_VISIBLE: AtomicBool = AtomicBool::new(false);
/// The translate shortcut was pressed: the reader thread should read the word now.
static TRANSLATE_REQUEST: AtomicBool = AtomicBool::new(false);
/// What the reader thread read for a translation (outer `Some` = new, not yet taken; inner `None` = nothing readable).
static PICKED: Mutex<Option<Option<Picked>>> = Mutex::new(None);
/// What the thread that translated a selection found: its request number, the direction, and the text or the reason it failed.
static SENTENCE_DONE: Mutex<Option<(usize, Lang, Lang, Result<String, String>)>> = Mutex::new(None);
/// The shortcut that is registered with Windows now.
static TRANSLATE_KEY: Mutex<Option<Binding>> = Mutex::new(None);
/// The wrong-keyboard shortcut was pressed: the reader thread should read the selection now.
static FIXKEYS_REQUEST: AtomicBool = AtomicBool::new(false);
/// What the reader thread read for it (outer `Some` = new, not yet taken; inner `None` = no readable selection).
static FIXKEYS_PICKED: Mutex<Option<Option<Picked>>> = Mutex::new(None);
/// The wrong-keyboard shortcut that is registered with Windows now.
static FIXKEYS_KEY: Mutex<Option<Binding>> = Mutex::new(None);
static UNDO_KEY: Mutex<Option<Binding>> = Mutex::new(None);
static DICTATE_KEY: Mutex<Option<Binding>> = Mutex::new(None);
static QUICK_KEY: Mutex<Option<Binding>> = Mutex::new(None);
/// The quick window is open (a second press of its key does not open another).
static QUICK_OPEN: AtomicBool = AtomicBool::new(false);
/// What the thread that wrote a dictation down found: the text, or why not.
static DICTATED: Mutex<Option<Result<String, String>>> = Mutex::new(None);
/// What the thread that asked for a newer version found: whether the person asked (a message box answers) or the daily check did (a short
/// notice, and only when there is news), and `Ok(Some(tag))` for a newer version, `Ok(None)` for none, or the reason it could not be told.
static UPDATE_DONE: Mutex<Option<(bool, Result<Option<String>, String>)>> = Mutex::new(None);
/// The shortcuts Windows refused (another program holds the key), by name, for the diagnostics.
static REFUSED_SHORTCUTS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// How many candidates the popup shows now.
static POPUP_COUNT: AtomicUsize = AtomicUsize::new(0);
/// The key (virtual key code) whose press the hook kept from the app, so that its release is kept too; 0 when none.
static KEPT_KEY: AtomicU32 = AtomicU32::new(0);
/// The keys that drive the popup, as the user set them (the hook reads them here, it cannot reach the settings).
static KEYS: Mutex<Keys> = Mutex::new(DEFAULT_KEYS);
static CONTROLLER: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static PAUSED: AtomicBool = AtomicBool::new(false);
static QUITTING: AtomicBool = AtomicBool::new(false);
/// What the worker thread last read (outer `Some` = new, not yet taken; inner `None` = no text field has the focus).
static LATEST: Mutex<Option<Option<Snapshot>>> = Mutex::new(None);
/// A copy of the settings for the reader thread (which decides, before reading anything, whether the app in front is allowed).
static RULES: Mutex<Option<Settings>> = Mutex::new(None);
/// The last program the user was working in (not the taskbar, not Verba): what "Turn off in this app" means in the tray menu.
static LAST_APP: Mutex<String> = Mutex::new(String::new());

struct App {
    overlay: Overlay,
    engine: LocalEngine,
    settings: Settings,
    /// The tray icon when suggestions are on, and the dimmed one when they are off.
    icon_on: HICON,
    icon_off: HICON,
    /// What the popup shows now.
    shown: Vec<Suggestion>,
    /// How many characters of the word are typed already (they are replaced when a suggestion is accepted).
    typed_len: usize,
    /// The text for which the user pressed Esc: stay closed until it changes.
    dismissed: Option<String>,
    /// The text before the caret at the last reading (what Esc remembers).
    last_text: String,
    /// Set while the popup shows a spelling fix instead of word completions.
    fix: Option<FixState>,
    /// The candidate the accept key will pick (the previous and next keys move it), where the popup is, and its label ("Fix").
    selected: usize,
    anchor: Option<Anchor>,
    lead: Option<String>,
    /// Set while the popup shows translations: how many characters before the caret to erase when one is chosen (0: the word was
    /// selected, typing replaces it).
    translating: Option<usize>,
    /// While the popup shows readings of text typed on the wrong keyboard: the label of each (`EN to AR`), shown for the highlighted one.
    pair_labels: Vec<String>,
    /// The program that translates sentences (started at the first one), and the number of the last request: an answer for an
    /// older one is dropped (the user pressed Esc, moved on, or asked again).
    sentences: Arc<sentences::Translator>,
    sentence_seq: usize,
    /// A short notice is on the screen (not a list to choose from), and the number of the last one.
    notice: bool,
    notice_seq: usize,
    /// The words typed since the last end of a sentence, waiting to be learned (only collected when the user turned learning on).
    learn_buf: String,
    learn_words: usize,
    /// The personal dictionary files, the time `personal.txt` was last changed (a person may edit it), and when it was last looked at.
    files: Option<(PathBuf, PathBuf)>,
    words_stamp: Option<SystemTime>,
    /// The file of the translations the user wrote (`my-translations.txt`) and the time it was last changed.
    tr_file: Option<PathBuf>,
    tr_stamp: Option<SystemTime>,
    last_check: Instant,
    /// The last spelling fix the user accepted, for the undo shortcut.
    last_fix: Option<LastFix>,
    /// The sentence being translated (for the word-by-word translation when the translator fails), the text before the caret when it was asked
    /// (a reading with other text means the user moved on), and the languages.
    sentence_text: String,
    sentence_ctx: Option<String>,
    sentence_langs: Option<(Lang, Lang)>,
    /// The times of the files of the sync folder, of `personal.txt` and of `my-translations.txt` when the last sync looked, and when it did.
    sync_seen: (Option<SystemTime>, Option<SystemTime>, Option<SystemTime>),
    sync_checked: Instant,
    /// The microphone, while a dictation is being recorded, and the pack that will write it down.
    recorder: Option<dictation::Recorder>,
    dictation_pack: Option<&'static dictation::Pack>,
    /// A recording is being written down (a second one waits for it).
    dictation_busy: bool,
    /// The language of the keyboard of the last field that was read.
    last_layout: Option<Lang>,
}

/// What the undo shortcut needs to put a corrected word back.
struct LastFix {
    /// What was typed in place of the word (the fix and what followed the word), and what it replaced.
    corrected: String,
    original: String,
    /// The word as the user had written it.
    word: String,
    at: Instant,
}

/// What accepting a spelling fix has to replace.
struct FixState {
    /// The word and what followed it, in characters: all of it is erased and typed again.
    erase: usize,
    original: String,
    trailing: String,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn main() {
    unsafe {
        // one copy at a time
        // (a development build can run next to the real one: a test names its own instance)
        let mut instance = String::from("Local\\VerbaSingleInstance");
        if cfg!(debug_assertions) {
            instance.push_str(&std::env::var("VERBA_TEST_INSTANCE").unwrap_or_default());
        }
        let instance: Vec<u16> = instance.encode_utf16().chain(std::iter::once(0)).collect();
        let _mutex = CreateMutexW(None, true, PCWSTR(instance.as_ptr()));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }

        let settings = Settings::load();
        // the word lists, the personal dictionary and the built-in Darija lists: read on another thread while the splash plays.
        // Windows starts Verba with `--startup` at sign-in: then there is no splash, it just appears in the tray.
        let load = || {
            let mut engine = load_engine();
            let files = Settings::dir().map(|d| (d.join("personal.txt"), d.join("learned.txt")));
            if let Some((words, learned)) = &files {
                engine.set_personal(PersonalDict::load(words, learned));
            }
            let words_stamp = files.as_ref().and_then(|(w, _)| stamp_of(w));
            let tr_file = Settings::dir().map(|d| d.join("my-translations.txt"));
            if let Some(path) = &tr_file {
                engine.set_user_translations(UserTranslations::load(path));
            }
            let tr_stamp = tr_file.as_deref().and_then(stamp_of);
            // (Darija: the seed lists, and `dz-ar.txt` of the data folder when the installer put it there; see `load_engine`)
            (engine, files, words_stamp, tr_file, tr_stamp)
        };
        let (mut engine, files, words_stamp, tr_file, tr_stamp) =
            if std::env::args().any(|a| a == "--startup") { load() } else { run_with_splash(look_for(&settings), load) };
        for lang in [Lang::En, Lang::Fr, Lang::Ar, Lang::Dz] {
            engine.set_enabled(lang, settings.lang_on(lang));
        }
        PAUSED.store(settings.paused, Ordering::SeqCst);
        share_keys(&settings.keys);
        share_rules(&settings);
        let overlay = Overlay::new().expect("could not make the popup window");
        let (icon_on, icon_off) = (make_icon(false), make_icon(true));

        let hinst: HINSTANCE = GetModuleHandleW(None).expect("module handle").into();
        let class = w!("VerbaController");
        RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(controller_proc), hInstance: hinst, lpszClassName: class, ..Default::default() });
        // a window that is never shown: it receives the timer, the hotkeys and the tray messages
        let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW, class, w!("Verba"), WS_POPUP, 0, 0, 0, 0, None, None, Some(hinst), None)
            .expect("controller window");
        CONTROLLER.store(hwnd.0 as isize, Ordering::SeqCst);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::SeqCst);

        APP.with(|a| {
            *a.borrow_mut() =
                Some(App { overlay, engine, settings, icon_on, icon_off, shown: Vec::new(), typed_len: 0, dismissed: None, last_text: String::new(), fix: None, selected: 0, anchor: None, lead: None, translating: None, pair_labels: Vec::new(), sentences: Arc::new(sentences::Translator::find()), sentence_seq: 0, notice: false, notice_seq: 0, learn_buf: String::new(), learn_words: 0, files, words_stamp, tr_file, tr_stamp, last_check: Instant::now(), last_fix: None, sentence_text: String::new(), sentence_ctx: None, sentence_langs: None, sync_seen: (None, None, None), sync_checked: Instant::now() - Duration::from_secs(3600), recorder: None, dictation_pack: None, dictation_busy: false, last_layout: None });
        });

        let mods = MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
        let quit_ok = RegisterHotKey(Some(hwnd), HOTKEY_QUIT, mods, 'Q' as u32).is_ok();
        if !quit_ok {
            log("could not register Ctrl+Alt+Shift+Q (another program uses it); use the tray menu to quit");
        }
        note_shortcut("quit", quit_ok);
        note_shortcut("pause", RegisterHotKey(Some(hwnd), HOTKEY_PAUSE, mods, 'V' as u32).is_ok());
        note_shortcut("this app on/off", RegisterHotKey(Some(hwnd), HOTKEY_APP, mods, 'A' as u32).is_ok());
        with_app(|app| sync_translate_hotkey(hwnd, app.settings.keys.translate));
        with_app(|app| sync_fixkeys_hotkey(hwnd, app.settings.keys.fixkeys));
        with_app(|app| sync_undo_hotkey(hwnd, app.settings.keys.undo));
        with_app(|app| sync_dictate_hotkey(hwnd, app.settings.keys.dictate));
        with_app(|app| sync_quick_hotkey(hwnd, app.settings.keys.quick));

        with_app(|app| app.overlay.set_click_target(hwnd, WM_ACCEPT));
        add_tray(hwnd);
        spawn_reader();
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), Some(hinst), 0).ok();
        if hook.is_none() {
            log("keyboard hook failed: Tab and Esc will not work");
        }

        // the daily check for a newer version, when the person turned it on (a test never goes to the network)
        let quiet_test = cfg!(debug_assertions) && std::env::var("VERBA_TEST_QUIET").is_ok();
        if !quiet_test && with_app_ret(|app| app.settings.check_updates).unwrap_or(false) {
            check_for_update_if_due();
        }
        // (a development build run by a test can try the dictation without a keyboard: VERBA_TEST_DICTATION=download:<pack> | file:<pack>:<wav> | mic;
        // what happens is written to `dictation-test.txt` in the settings folder; the sound of the microphone is never kept)
        if cfg!(debug_assertions) {
            if let Ok(what) = std::env::var("VERBA_TEST_DICTATION") {
                let exe = app_sentences_exe();
                if what.starts_with("quick:") {
                    // the quick window's translation reads the app, which only this thread can: it runs here
                    test_dictation(&what, exe);
                } else {
                    std::thread::spawn(move || test_dictation(&what, exe));
                }
            }
        }
        // (a development build run by a test syncs once at start, VERBA_TEST_SYNC=1, and writes what happened to `sync-test.txt`)
        if cfg!(debug_assertions) && std::env::var("VERBA_TEST_SYNC").is_ok() {
            let said = match sync_now() {
                Ok(report) => format!("{report:?}"),
                Err(why) => format!("error: {why}"),
            };
            if let Some(dir) = Settings::dir() {
                let _ = std::fs::write(dir.join("sync-test.txt"), said);
            }
        }
        // (a development build can open the settings window by itself, VERBA_TEST_SETTINGS=1, to look at it)
        if cfg!(debug_assertions) && std::env::var("VERBA_TEST_SETTINGS").is_ok() {
            open_settings();
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if let Some(h) = hook {
            let _ = UnhookWindowsHookEx(h);
        }
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_QUIT);
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_PAUSE);
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_APP);
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_UNDO);
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_DICTATE);
        let _ = UnregisterHotKey(Some(hwnd), HOTKEY_QUICK);
    }
}

/// Where the word lists are: `VERBA_DATA`, else `data/words` next to the project (found from the program's own folder).
fn load_engine() -> LocalEngine {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(p) = std::env::var("VERBA_DATA") {
        candidates.push(PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        for up in exe.ancestors().skip(1).take(5) {
            candidates.push(up.join("data").join("words"));
        }
    }
    candidates.push(PathBuf::from("data/words"));
    for dir in candidates {
        if let Ok(engine) = LocalEngine::load_dir(&dir) {
            if !engine.languages().is_empty() {
                log(&format!("word lists: {} ({:?})", dir.display(), engine.languages()));
                // the seed lists of Darija, and the words of real Algerian comments (`dz-ar.txt`) when they are in the folder
                let mut engine = engine;
                if engine.load_dz(&dir).is_err() {
                    engine.add_dz_seed();
                }
                return engine;
            }
        }
    }
    log("no word lists found: run scripts/fetch-data.ps1");
    let mut engine = LocalEngine::new();
    engine.add_dz_seed();
    engine
}

fn log(text: &str) {
    #[cfg(debug_assertions)]
    println!("verba: {text}");
    #[cfg(not(debug_assertions))]
    let _ = text;
}

/// The popup's look: Windows' theme and accent colour, unless the user chose light or dark.
fn look_for(settings: &Settings) -> Look {
    match settings.theme {
        ThemeMode::Auto => Look::system(),
        ThemeMode::Light => Look::with_theme(Theme::Light),
        ThemeMode::Dark => Look::with_theme(Theme::Dark),
    }
}

const RUN_KEY: windows::core::PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");

pub(crate) fn autostart_enabled() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, w!("Verba"), RRF_RT_REG_SZ, None, None, None).is_ok() }
}

/// Start with Windows: one value in the user's Run key (no admin rights, nothing installed).
pub(crate) fn set_autostart(on: bool) {
    unsafe {
        if on {
            let Ok(exe) = std::env::current_exe() else { return };
            // `--startup` tells the program that Windows started it at sign-in: no splash then
            let value: Vec<u16> = format!("\"{}\" --startup", exe.display()).encode_utf16().chain(std::iter::once(0)).collect();
            let _ = RegSetKeyValueW(HKEY_CURRENT_USER, RUN_KEY, w!("Verba"), REG_SZ.0, Some(value.as_ptr() as *const _), (value.len() * 2) as u32);
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, w!("Verba"));
        }
    }
}

// ───────────── the controller window ─────────────

unsafe extern "system" fn controller_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_SNAPSHOT => {
            let latest = LATEST.lock().ok().and_then(|mut l| l.take());
            if let Some(snap) = latest {
                with_app(|app| apply(app, snap));
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            match wp.0 as i32 {
                HOTKEY_QUIT => quit(),
                HOTKEY_PAUSE => update_settings(|s| s.paused = !s.paused),
                // the app the user is in right now (the hotkey does not move the focus)
                HOTKEY_APP => toggle_app(foreground_app()),
                // the reader thread reads the word (UI Automation must not be called from this thread)
                HOTKEY_TRANSLATE => TRANSLATE_REQUEST.store(true, Ordering::SeqCst),
                // the same for the selection to put right
                HOTKEY_FIXKEYS => FIXKEYS_REQUEST.store(true, Ordering::SeqCst),
                // puts back the word the last fix replaced
                HOTKEY_UNDO => with_app(undo_last_fix),
                // starts a dictation, or ends the one being recorded
                HOTKEY_DICTATE => with_app(toggle_dictation),
                // the quick translation window (it runs its own message loop: not inside the borrow of the app)
                HOTKEY_QUICK => quick_window(),
                _ => {}
            }
            LRESULT(0)
        }
        WM_TRAY => {
            let event = (lp.0 as u32) & 0xFFFF;
            if event == WM_RBUTTONUP {
                show_menu(hwnd);
            } else if event == WM_LBUTTONUP {
                // a click on the icon opens the settings; the menu is on the right button
                open_settings();
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match wp.0 & 0xFFFF {
                CMD_PAUSE => update_settings(|s| s.paused = !s.paused),
                // the tray menu took the focus, so "this app" is the last one the reader saw in front
                CMD_APP => toggle_app(LAST_APP.lock().ok().map(|a| a.clone()).filter(|a| !a.is_empty())),
                CMD_SETTINGS => open_settings(),
                CMD_CHECK => field_check(),
                CMD_QUIT => quit(),
                _ => {}
            }
            LRESULT(0)
        }
        // a key (Tab) or a click on a candidate: wParam is the candidate's number
        WM_ACCEPT => {
            with_app(|app| {
                let index = if wp.0 == ACCEPT_HIGHLIGHTED { app.selected } else { wp.0 };
                accept(app, index)
            });
            LRESULT(0)
        }
        WM_TRANSLATE => {
            let picked = PICKED.lock().ok().and_then(|mut p| p.take());
            if let Some(picked) = picked {
                with_app(|app| show_translation(app, picked));
            }
            LRESULT(0)
        }
        windows::Win32::UI::WindowsAndMessaging::WM_TIMER if wp.0 == TIMER_DICTATE => {
            with_app(listen);
            LRESULT(0)
        }
        WM_DICTATION => {
            settings_window::refresh();
            LRESULT(0)
        }
        WM_DICTATED => {
            let done = DICTATED.lock().ok().and_then(|mut d| d.take());
            if let Some(result) = done {
                with_app(|app| type_dictation(app, result));
            }
            LRESULT(0)
        }
        WM_UPDATE => {
            let done = UPDATE_DONE.lock().ok().and_then(|mut d| d.take());
            if let Some((asked, result)) = done {
                if asked {
                    settings_window::show_update_result(result);
                } else if let Ok(Some(tag)) = result {
                    let text = format!("Verba {tag} is available. Settings > General has the link.");
                    with_app(|app| notice(app, None, &text));
                }
            }
            LRESULT(0)
        }
        WM_FIXKEYS => {
            let picked = FIXKEYS_PICKED.lock().ok().and_then(|mut p| p.take());
            if let Some(picked) = picked {
                with_app(|app| show_fixkeys(app, picked));
            }
            LRESULT(0)
        }
        WM_SENTENCE => {
            let done = SENTENCE_DONE.lock().ok().and_then(|mut d| d.take());
            if let Some((seq, from, to, result)) = done {
                with_app(|app| show_sentence(app, seq, from, to, result));
            }
            LRESULT(0)
        }
        WM_NOTICE_END => {
            with_app(|app| {
                if app.notice && app.notice_seq == wp.0 {
                    hide(app);
                }
            });
            LRESULT(0)
        }
        WM_MOVE => {
            with_app(|app| move_highlight(app, if wp.0 == 0 { -1 } else { 1 }));
            LRESULT(0)
        }
        WM_DISMISS => {
            with_app(|app| {
                // Esc on a shown fix means "this word is fine": stop flagging it for the rest of the session
                if let Some(fix) = app.fix.take() {
                    app.engine.ignore_word(&fix.original);
                }
                app.dismissed = Some(app.last_text.clone());
                app.sentence_seq += 1;
                hide(app);
            });
            LRESULT(0)
        }
        WM_DESTROY => {
            with_app(|app| {
                flush_learning(app);
                save_personal(app);
            });
            remove_tray(hwnd);
            quit();
            LRESULT(0)
        }
        m if m == TASKBAR_CREATED.load(Ordering::SeqCst) && m != 0 => {
            // Explorer restarted: the tray icon is gone, put it back
            add_tray(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// Like `with_app`, and gives back what `f` answers (`None` when the app is busy: a borrow is taken further up).
fn with_app_ret<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|a| a.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

fn with_app(f: impl FnOnce(&mut App)) {
    APP.with(|a| {
        if let Ok(mut b) = a.try_borrow_mut() {
            if let Some(app) = b.as_mut() {
                f(app);
            }
        }
    });
}

fn hide(app: &mut App) {
    app.overlay.hide();
    app.shown.clear();
    app.fix = None;
    app.selected = 0;
    app.translating = None;
    app.pair_labels.clear();
    app.notice = false;
    POPUP_COUNT.store(0, Ordering::SeqCst);
    POPUP_VISIBLE.store(false, Ordering::SeqCst);
}

fn quit() {
    QUITTING.store(true, Ordering::SeqCst);
    unsafe { PostQuitMessage(0) };
}

/// The reading is done on a thread of its own. Asking another program about its text field can take a while
/// (or never answer if that program is stuck), and the keyboard hook shares the main thread's message loop:
/// if the main thread waited on another program, that program's keystrokes would wait on the hook, and both would stall.
fn spawn_reader() {
    std::thread::spawn(|| {
        let Ok(locator) = Locator::new() else { return };
        let mut last: Option<(i32, i32, i32, i32, String, Option<Lang>)> = None;
        let mut was_paused = false;
        // (a development build can be told to read no text field of the user's, VERBA_TEST_QUIET=1: only the requests above are served)
        let quiet = cfg!(debug_assertions) && std::env::var("VERBA_TEST_QUIET").is_ok();
        while !QUITTING.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
            // the translate shortcut was pressed: read the selected word (or the one before the caret) now
            if TRANSLATE_REQUEST.swap(false, Ordering::SeqCst) {
                let front = foreground_app();
                let allowed = RULES.lock().ok().and_then(|r| r.as_ref().map(|st| st.allows_app(front.as_deref().unwrap_or("")))).unwrap_or(true);
                let mut picked = if allowed { locator.pick(48) } else { None };
                // (a development build can be told which word to translate and where, to look at the popup without a focused field)
                if cfg!(debug_assertions) {
                    if let Ok(word) = std::env::var("VERBA_TEST_PICK") {
                        picked = Some(Picked {
                            text: word,
                            anchor: Anchor { rect: windows::Win32::Foundation::RECT { left: 640, top: 520, right: 643, bottom: 540 }, kind: AnchorKind::Caret },
                            layout: None,
                            app: String::new(),
                            selected: true,
                        });
                    }
                }
                if let Ok(mut slot) = PICKED.lock() {
                    *slot = Some(picked);
                }
                let controller = CONTROLLER.load(Ordering::SeqCst);
                if controller != 0 {
                    unsafe {
                        let _ = PostMessageW(Some(HWND(controller as *mut _)), WM_TRANSLATE, WPARAM(0), LPARAM(0));
                    }
                }
            }
            // the wrong-keyboard shortcut was pressed: read the selected text now (never a word: the whole selection or nothing)
            if FIXKEYS_REQUEST.swap(false, Ordering::SeqCst) {
                let front = foreground_app();
                let allowed = RULES.lock().ok().and_then(|r| r.as_ref().map(|st| st.allows_app(front.as_deref().unwrap_or("")))).unwrap_or(true);
                #[allow(unused_mut)]
                let mut picked = if allowed { locator.pick_selection(fixkeys::READ_CAP) } else { None };
                // (a development build can be given the selected text and the keyboard that is active, to look at the popup without
                // a focused field and without a key being pressed: VERBA_TEST_FIXKEYS=<text> VERBA_TEST_ACTIVE=en|fr|ar)
                if cfg!(debug_assertions) {
                    if let Ok(text) = std::env::var("VERBA_TEST_FIXKEYS") {
                        let layout = match std::env::var("VERBA_TEST_ACTIVE").unwrap_or_default().as_str() {
                            "en" => Some(Lang::En),
                            "fr" => Some(Lang::Fr),
                            "ar" => Some(Lang::Ar),
                            _ => None,
                        };
                        picked = Some(Picked {
                            text,
                            anchor: Anchor { rect: windows::Win32::Foundation::RECT { left: 640, top: 520, right: 643, bottom: 540 }, kind: AnchorKind::Caret },
                            layout,
                            app: String::new(),
                            selected: true,
                        });
                    }
                }
                if let Ok(mut slot) = FIXKEYS_PICKED.lock() {
                    *slot = Some(picked);
                }
                let controller = CONTROLLER.load(Ordering::SeqCst);
                if controller != 0 {
                    unsafe {
                        let _ = PostMessageW(Some(HWND(controller as *mut _)), WM_FIXKEYS, WPARAM(0), LPARAM(0));
                    }
                }
            }
            let paused = PAUSED.load(Ordering::SeqCst);
            // which program is in front: remembered for the tray menu, and checked BEFORE reading anything,
            // so a program the user switched Verba off in is never read at all
            let app_name = foreground_app();
            if let Some(name) = &app_name {
                if let Ok(mut last_app) = LAST_APP.lock() {
                    if last_app.as_str() != name.as_str() {
                        *last_app = name.clone();
                    }
                }
            }
            let allowed = RULES.lock().ok().and_then(|r| r.as_ref().map(|s| s.allows_app(app_name.as_deref().unwrap_or("")))).unwrap_or(true);
            let snap = if paused || !allowed || quiet { None } else { locator.snapshot(48) };
            // wake the main thread only when something changed
            let key = snap.as_ref().map(|s| (s.anchor.rect.left, s.anchor.rect.top, s.anchor.rect.right, s.anchor.rect.bottom, s.before_caret.clone(), s.layout));
            if key == last && paused == was_paused {
                continue;
            }
            // (debug builds only; the text itself is never written out)
            log(&format!("reader: {}", snap.as_ref().map_or("no text field".to_string(), |s| format!("{:?}, {} letters before the caret", s.anchor.kind, s.before_caret.chars().count()))));
            last = key;
            was_paused = paused;
            if let Ok(mut latest) = LATEST.lock() {
                *latest = Some(snap);
            }
            let controller = CONTROLLER.load(Ordering::SeqCst);
            if controller != 0 {
                unsafe {
                    let _ = PostMessageW(Some(HWND(controller as *mut _)), WM_SNAPSHOT, WPARAM(0), LPARAM(0));
                }
            }
        }
    });
}

/// A new reading: ask the engine about the word before the caret and show, move or hide the popup.
fn apply(app: &mut App, snap: Option<Snapshot>) {
    reload_personal_if_edited(app);
    let Some(snap) = snap else {
        // no text field any more (another window, a password box): what was collected is learned now, not carried over
        flush_learning(app);
        app.last_text.clear();
        if POPUP_VISIBLE.load(Ordering::SeqCst) {
            hide(app);
        }
        return;
    };
    // a reading with the same text before the caret as when the sentence was asked (the field's rectangle moved a pixel, the caret blinked) is the
    // same selection: what is shown ("Translating...", then the translation) stays as it is
    let same_selection = app.sentence_ctx.as_deref() == Some(snap.before_caret.as_str());
    if same_selection && POPUP_VISIBLE.load(Ordering::SeqCst) {
        return;
    }
    app.translating = None;
    // the selection may be gone: a translation still on its way is for text that is no longer there
    if !same_selection {
        app.sentence_seq += 1;
        app.sentence_ctx = None;
    }
    app.last_layout = snap.layout;
    let previous = std::mem::replace(&mut app.last_text, snap.before_caret.clone());
    if app.settings.learn && app.settings.allows_app(&snap.app) {
        if let Some((word, separators)) = learning::finished_word(&previous, &snap.before_caret) {
            app.learn_buf.push_str(&word);
            app.learn_buf.push_str(&separators);
            app.learn_words += 1;
            if learning::ends_sentence(&separators) || app.learn_words >= 25 {
                flush_learning(app);
            }
        }
    }
    match &app.dismissed {
        Some(d) if *d == snap.before_caret => return, // Esc was pressed for this text: stay closed
        Some(_) => app.dismissed = None,              // the user typed on: allowed again
        None => {}
    }
    // the program may have changed since the reading was taken
    if !app.settings.allows_app(&snap.app) {
        if POPUP_VISIBLE.load(Ordering::SeqCst) {
            hide(app);
        }
        return;
    }
    let found = match snap.layout {
        // the keyboard says which language is being typed: suggest in that one only
        Some(lang) if app.settings.follow_keyboard => app.engine.suggest_in(lang, &snap.before_caret, 3),
        // another layout (German, say), or "guess from the letters" is chosen
        _ => app.engine.suggest(&snap.before_caret, 3),
    };
    log(&format!("engine: {} suggestions (keyboard: {:?})", found.len(), snap.layout));
    if found.is_empty() {
        // no word is being typed: if the one just finished looks misspelled, offer the fix (only when the user turned it on)
        if app.settings.correction {
            let only = if app.settings.follow_keyboard { snap.layout } else { None };
            if let Some(fix) = app.engine.correct(only, &snap.before_caret, 3) {
                if !fix.candidates.is_empty() {
                    app.fix = Some(FixState {
                        erase: fix.original.chars().count() + fix.trailing.chars().count(),
                        original: fix.original,
                        trailing: fix.trailing,
                    });
                    present(app, snap.anchor, fix.candidates, Some("Fix".to_string()));
                    return;
                }
            }
        }
        // after a finished word and a space: the likely next word (only when the user turned it on)
        if app.settings.predict_next {
            let only = if app.settings.follow_keyboard { snap.layout } else { None };
            let next = app.engine.next_words(only, &snap.before_caret, 3);
            if !next.is_empty() {
                app.fix = None;
                app.typed_len = 0;
                present(app, snap.anchor, next, None);
                return;
            }
        }
        if POPUP_VISIBLE.load(Ordering::SeqCst) {
            hide(app);
        }
        return;
    }
    app.fix = None;
    // with Darija on, digit-letters (sa7, 3la) belong to the word
    let typed = if app.settings.darija { current_word_dz(&snap.before_caret) } else { current_word(&snap.before_caret) };
    app.typed_len = typed.chars().count();
    present(app, snap.anchor, found, None);
}

/// A translation was asked for: the reader thread read the word (or found nothing).
fn show_translation(app: &mut App, picked: Option<Picked>) {
    let Some(picked) = picked else {
        notice(app, None, "Click in a text field, or select a word, then press the shortcut.");
        return;
    };
    let anchor = Some(picked.anchor);
    let words = picked.text.split_whitespace().count();
    // more than a few words: a sentence, whether or not the word dictionary is installed
    if words > 3 || picked.text.chars().count() > 40 {
        start_sentence(app, picked);
        return;
    }
    if !app.engine.has_dictionary() {
        if picked.selected && words >= 2 && !app.sentences.installed().is_empty() {
            start_sentence(app, picked);
            return;
        }
        notice(app, anchor, "The dictionary is not installed.");
        return;
    }
    // into the language chosen in the settings; a word already in it goes to English, then French, then Arabic
    let targets = [app.settings.translate_to, Lang::En, Lang::Fr, Lang::Ar];
    match app.engine.translate_word(&picked.text, &targets, picked.layout) {
        Some(t) => {
            // a game or chat abbreviation written out in its own language (gg -> good game) is not a translation: "Slang"
            let label = if t.from == t.to { "Slang".to_string() } else { format!("{} \u{2192} {}", t.from.code().to_uppercase(), t.to.code().to_uppercase()) };
            app.fix = None;
            app.typed_len = 0;
            present(app, picked.anchor, t.candidates, Some(label));
            // present() ran with the old state: this popup is a translation
            app.translating = Some(if picked.selected { 0 } else { picked.text.chars().count() });
        }
        None => {
            // two or three selected words that are not in the dictionary as such: the sentence translator may know them, and when it is not
            // there (or fails) the words are translated one by one
            if picked.selected && words >= 2 {
                start_sentence(app, picked);
                return;
            }
            let text = format!("No translation for \u{201C}{}\u{201D}.", picked.text.trim());
            notice(app, anchor, &text);
        }
    }
}

/// A selection of more than a word: translate it with the sentence translator, on a thread of its own (the first time it also loads
/// the model, which takes a moment). The popup says so meanwhile, and shows the translation when it is there.
fn start_sentence(app: &mut App, picked: Picked) {
    let anchor = Some(picked.anchor);
    if !picked.selected {
        notice(app, anchor, "Select the sentence first, then press the shortcut.");
        return;
    }
    if picked.text.chars().count() > verba_local::sentence::MAX_CHARS {
        let text = format!("Select at most {} characters at a time.", verba_local::sentence::MAX_CHARS);
        notice(app, anchor, &text);
        return;
    }
    let installed = app.sentences.installed();
    // without the sentence translator, or for a pair it has no model for, the words are translated one by one
    let pair = if installed.is_empty() { None } else { verba_local::sentence::pair_for(&picked.text, picked.layout, app.settings.translate_to, &installed) };
    let Some((from, to)) = pair else {
        let (from, to) = word_pair(&picked.text, picked.layout, app.settings.translate_to);
        app.anchor = Some(picked.anchor);
        if !show_words(app, picked.anchor, &picked.text, from, to) {
            let why = if installed.is_empty() { "Sentence translation is not installed, and the dictionary knows none of these words." } else { "This language pair is not installed (Arabic and English are)." };
            notice(app, anchor, why);
        }
        return;
    };
    app.sentence_seq += 1;
    let seq = app.sentence_seq;
    app.anchor = Some(picked.anchor);
    app.sentence_text = picked.text.clone();
    app.sentence_ctx = Some(app.last_text.clone());
    app.sentence_langs = Some((from, to));
    show_notice(app, anchor, "Translating\u{2026}", false);
    let translator = Arc::clone(&app.sentences);
    let text = picked.text;
    std::thread::spawn(move || {
        let result = translator.translate(from, to, &text);
        if let Ok(mut slot) = SENTENCE_DONE.lock() {
            *slot = Some((seq, from, to, result));
        }
        let controller = CONTROLLER.load(Ordering::SeqCst);
        if controller != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(controller as *mut _)), WM_SENTENCE, WPARAM(0), LPARAM(0));
            }
        }
    });
}

/// The translator answered: show the translation (Tab types it over the selection) or say why not.
fn show_sentence(app: &mut App, seq: usize, from: Lang, to: Lang, result: Result<String, String>) {
    if seq != app.sentence_seq {
        return; // the user pressed Esc, moved on or asked again
    }
    let anchor = app.anchor;
    match result {
        Ok(text) => {
            let Some(anchor) = anchor else { return };
            let label = format!("{} \u{2192} {}", from.code().to_uppercase(), to.code().to_uppercase());
            app.fix = None;
            app.typed_len = 0;
            present(app, anchor, vec![Suggestion { text, lang: to }], Some(label));
            // the selection is still there: typing replaces it
            app.translating = Some(0);
        }
        Err(why) => {
            log(&format!("sentence translation failed: {why}"));
            // the translator failed or did not answer: the words are translated one by one instead
            let text = std::mem::take(&mut app.sentence_text);
            let shown = anchor.is_some_and(|a| show_words(app, a, &text, from, to));
            if !shown {
                notice(app, anchor, "The sentence could not be translated.");
            }
        }
    }
}

/// The languages of a word-by-word translation of `text`: Arabic goes to the language chosen for translation (English when that is Arabic), and any other
/// text to Arabic, like the sentence translator.
fn word_pair(text: &str, layout: Option<Lang>, translate_to: Lang) -> (Lang, Lang) {
    if text.chars().any(verba_core::is_arabic) {
        (Lang::Ar, if translate_to == Lang::Ar { Lang::En } else { translate_to })
    } else {
        (if layout == Some(Lang::Fr) { Lang::Fr } else { Lang::En }, Lang::Ar)
    }
}

/// Shows `text` translated word by word, with its label; Tab types it over the selection. False when the dictionary knew no word.
fn show_words(app: &mut App, anchor: Anchor, text: &str, from: Lang, to: Lang) -> bool {
    let hint = if from == Lang::Ar { Some(Lang::Ar) } else { Some(from) };
    let Some(reading) = app.engine.translate_words(text, to, hint) else { return false };
    let label = format!("{} \u{2192} {}, word by word", from.code().to_uppercase(), to.code().to_uppercase());
    app.fix = None;
    app.typed_len = 0;
    present(app, anchor, vec![Suggestion { text: reading.text, lang: to }], Some(label));
    // the selection is still there: typing replaces it
    app.translating = Some(0);
    true
}

#[cfg(test)]
mod word_by_word_tests {
    use super::*;

    #[test]
    fn the_languages_of_a_word_by_word_translation_follow_the_text() {
        assert_eq!(word_pair("شكرا لك", None, Lang::En), (Lang::Ar, Lang::En));
        assert_eq!(word_pair("شكرا لك", None, Lang::Fr), (Lang::Ar, Lang::Fr));
        assert_eq!(word_pair("شكرا لك", None, Lang::Ar), (Lang::Ar, Lang::En), "never from Arabic into Arabic");
        assert_eq!(word_pair("thank you", None, Lang::En), (Lang::En, Lang::Ar));
        assert_eq!(word_pair("merci beaucoup", Some(Lang::Fr), Lang::En), (Lang::Fr, Lang::Ar));
    }
}

/// The wrong-keyboard shortcut was pressed: the reader thread read the selected text (or found none). The keys that gave the text are
/// traced back, and the readings that are made of real words are offered; Tab types the highlighted one over the selection.
fn show_fixkeys(app: &mut App, picked: Option<Picked>) {
    let Some(picked) = picked else {
        notice(app, None, "Select the text typed with the wrong keyboard first, then press the shortcut.");
        return;
    };
    let anchor = Some(picked.anchor);
    if picked.text.chars().count() > fixkeys::MAX_CHARS {
        let text = format!("Select at most {} characters at a time.", fixkeys::MAX_CHARS);
        notice(app, anchor, &text);
        return;
    }
    match fixkeys::offer(&picked.text, &verba_win::layouts::installed(), &app.engine, picked.layout) {
        fixkeys::Outcome::NeedTwoLanguages => {
            notice(app, anchor, "Add the other keyboards in Windows (Settings, Time & language, Language), then try again.");
        }
        fixkeys::Outcome::NothingToFix => {
            notice(app, anchor, "No other keyboard turns this text into words.");
        }
        fixkeys::Outcome::Found(readings) => {
            app.fix = None;
            app.typed_len = 0;
            let list: Vec<Suggestion> = readings.iter().map(|r| Suggestion { text: r.text.clone(), lang: r.lang }).collect();
            present(app, picked.anchor, list, readings.first().map(|r| r.label.clone()));
            // present() ran with the old state: this popup is readings of the selection, and typing replaces it
            app.pair_labels = readings.into_iter().map(|r| r.label).collect();
            app.translating = Some(0);
        }
    }
}

/// A short message in the popup's style, that goes away by itself after a moment (it is not a list to choose from: no key cap,
/// and the keys of the popup are not taken).
fn notice(app: &mut App, anchor: Option<Anchor>, text: &str) {
    show_notice(app, anchor, text, true);
}

/// The same, and with `auto_hide` false it stays until something else is shown (a result is on its way).
fn show_notice(app: &mut App, anchor: Option<Anchor>, text: &str, auto_hide: bool) {
    let anchor = anchor.or(app.anchor).unwrap_or_else(|| {
        // no field to point at: near the mouse
        let mut at = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut at);
        }
        Anchor { rect: windows::Win32::Foundation::RECT { left: at.x, top: at.y, right: at.x + 2, bottom: at.y + 18 }, kind: AnchorKind::Caret }
    });
    app.overlay.set_hint("");
    app.overlay.show_with(anchor, &[Suggestion { text: text.to_string(), lang: Lang::En }], 0, look_for(&app.settings), Some("Verba"));
    app.shown.clear();
    app.notice = true;
    app.notice_seq += 1;
    if !auto_hide {
        return;
    }
    let seq = app.notice_seq;
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(2800));
        let controller = CONTROLLER.load(Ordering::SeqCst);
        if controller != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(controller as *mut _)), WM_NOTICE_END, WPARAM(seq), LPARAM(0));
            }
        }
    });
}

/// The directions the sentence translator can do now (empty when it is not installed): for the settings window.
pub(crate) fn sentence_pairs() -> Vec<(Lang, Lang)> {
    APP.with(|a| a.try_borrow().ok().and_then(|b| b.as_ref().map(|app| app.sentences.installed())).unwrap_or_default())
}

/// How many words the dictionary holds (summed over its directions), `None` when it is not installed: for the settings window.
pub(crate) fn dictionary_words() -> Option<Option<usize>> {
    APP.with(|a| {
        a.try_borrow().ok().and_then(|b| {
            b.as_ref().and_then(|app| {
                if !app.engine.has_dictionary() {
                    return None;
                }
                // the dictionary is read at the first translation: until then only the fact that it is there can be said
                Some(app.engine.dictionary_if_read().map(|d| {
                    let langs = [Lang::En, Lang::Fr, Lang::Ar];
                    langs.iter().flat_map(|f| langs.iter().map(move |t| (*f, *t))).filter(|(f, t)| f != t).map(|(f, t)| d.len(f, t)).sum()
                }))
            })
        })
    })
}

/// Shows the candidates. The highlighted one stays when the same words are shown again (the popup only moved).
fn present(app: &mut App, anchor: Anchor, list: Vec<Suggestion>, lead: Option<String>) {
    let same = app.shown.len() == list.len() && app.shown.iter().zip(&list).all(|(a, b)| a.text == b.text);
    if !same {
        app.selected = 0;
    }
    app.selected = app.selected.min(list.len().saturating_sub(1));
    app.anchor = Some(anchor);
    app.lead = lead;
    app.pair_labels.clear();
    app.notice = false;
    app.shown = list;
    redraw(app);
    POPUP_COUNT.store(app.shown.len(), Ordering::SeqCst);
    POPUP_VISIBLE.store(true, Ordering::SeqCst);
}

/// Draws the popup with the current highlight. The key cap at its end names the keys: with several candidates "previous next  accept".
fn redraw(app: &mut App) {
    let Some(anchor) = app.anchor else { return };
    let k = app.settings.keys;
    let hint = if app.shown.len() >= 2 { format!("{} {}  {}", k.prev.label(), k.next.label(), k.accept.label()) } else { k.accept.label() };
    app.overlay.set_hint(&hint);
    // readings of text typed on the wrong keyboard each have their own label; the other lists have one for all
    let lead = app.pair_labels.get(app.selected).map(String::as_str).or(app.lead.as_deref());
    app.overlay.show_with(anchor, &app.shown, app.selected, look_for(&app.settings), lead);
}

/// The "left" or "right" key (-1 or +1): highlight the candidate on that side, round the end. In an Arabic popup the first
/// candidate is at the right, so "left" goes to the next one and "right" to the previous one: the highlight always moves the way
/// the key points.
fn move_highlight(app: &mut App, toward: isize) {
    if app.shown.len() < 2 {
        return;
    }
    let rtl = app.shown.first().is_some_and(|s| s.lang.is_rtl());
    app.selected = highlight_after(app.selected, app.shown.len(), toward, rtl);
    redraw(app);
}

/// Which candidate is highlighted after the left (-1) or right (+1) key. A list that starts on the right (Arabic) runs the other way.
fn highlight_after(selected: usize, count: usize, toward: isize, rtl: bool) -> usize {
    if count == 0 {
        return 0;
    }
    let step = if rtl { -toward } else { toward };
    (selected as isize + step).rem_euclid(count as isize) as usize
}

/// Tab, or a click: replace the typed part of the word with that suggestion, plus a space.
fn accept(app: &mut App, index: usize) {
    let Some(chosen) = app.shown.get(index).cloned() else { return };
    // a spelling fix is remembered, so that the undo shortcut can put the word back
    app.last_fix = match (&app.translating, &app.fix) {
        (None, Some(fix)) => Some(LastFix {
            corrected: format!("{}{}", chosen.text, fix.trailing),
            original: format!("{}{}", fix.original, fix.trailing),
            word: fix.original.clone(),
            at: Instant::now(),
        }),
        _ => None,
    };
    // a completion replaces the part of the word typed and adds a space; a fix replaces the whole word and puts back what followed it
    let (erase, text) = match (&app.translating, &app.fix) {
        // a translation replaces the selected word (typing over it) or the word before the caret, and adds nothing
        (Some(erase), _) => (*erase, chosen.text.clone()),
        (None, Some(fix)) => (fix.erase, format!("{}{}", chosen.text, fix.trailing)),
        (None, None) => (app.typed_len, format!("{} ", chosen.text)),
    };
    hide(app);
    send_replacement(erase, &text, app.settings.line_break_shift, &chosen.text);
    // the next tick sees a fresh word: nothing to remember
    app.dismissed = None;
}

/// Puts the word Verba corrected back as the user wrote it, when it is still right before the caret, and stops flagging it for now.
fn undo_last_fix(app: &mut App) {
    let Some(last) = app.last_fix.take() else {
        notice(app, None, "Nothing to undo.");
        return;
    };
    // only while the corrected word is still the last thing before the caret: anything typed since would be erased otherwise
    if !undo_applies(&app.last_text, &last.corrected, last.at.elapsed()) {
        notice(app, None, "Nothing to undo: the text has changed since.");
        return;
    }
    hide(app);
    send_replacement(last.corrected.chars().count(), &last.original, app.settings.line_break_shift, &last.word);
    app.engine.ignore_word(&last.word);
    app.dismissed = None;
    let text = format!("Put back \u{201C}{}\u{201D}. Verba leaves it alone.", last.word);
    notice(app, None, &text);
}

// ───────────── the quick window ─────────────

/// The names of the languages in the quick window's drop-down, in the order of `quick_translate`'s `choice`.
const QUICK_LANGUAGES: [&str; 4] = ["Automatic", "English", "Fran\u{E7}ais", "\u{627}\u{644}\u{639}\u{631}\u{628}\u{64A}\u{629}"];

/// The window of Ctrl+Alt+Space: type or paste a word, a sentence or a few lines, choose the language, read the translation (and select it, to copy it).
/// It needs nothing selected in another program.
fn quick_window() {
    use verba_win::dialog::{self, Field, Form, Reply};
    if QUICK_OPEN.swap(true, Ordering::SeqCst) {
        return;
    }
    let theme = APP.with(|a| a.try_borrow().ok().and_then(|b| b.as_ref().map(|app| look_for(&app.settings).theme))).unwrap_or(Theme::Dark);
    // (a development build run by a test shows the window without taking the focus, and may start it with a text: VERBA_TEST_QUIET, VERBA_TEST_QUICK_TEXT)
    let quiet = cfg!(debug_assertions) && std::env::var("VERBA_TEST_QUIET").is_ok();
    let prefilled = if cfg!(debug_assertions) { std::env::var("VERBA_TEST_QUICK_TEXT").unwrap_or_default() } else { String::new() };
    let initial = [prefilled.as_str()];
    let form = Form {
        title: "Quick translation",
        intro: "Type or paste a word, a sentence or a few lines, then press Enter. A word shows several translations. The result can be selected and copied.",
        choice: Some(("Translate into", &QUICK_LANGUAGES)),
        fields: &[Field { label: "Text", rtl: None, lines: 3 }],
        initial: if prefilled.is_empty() { &[] } else { &initial },
        ok_label: "Translate",
        result_lines: 7,
        cancel_label: Some("Close"),
        activate: !quiet,
    };
    dialog::ask_with(None, theme, &form, &mut |choice, texts| match quick_translate(&texts[0], choice) {
        Ok(text) => Reply::Show(text),
        Err(why) => Reply::Error(why),
    });
    QUICK_OPEN.store(false, Ordering::SeqCst);
}

/// The translation shown by the quick window: a word (or two or three) from the dictionary with its several translations, a sentence from the
/// sentence translator, and when that is not there or fails, word by word. `choice` is the drop-down: 0 automatic (Arabic goes to the language chosen in the
/// settings, anything else to Arabic), 1 English, 2 French, 3 Arabic. The first line says the direction.
fn quick_translate(text: &str, choice: usize) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Type or paste the text first.".to_string());
    }
    let arabic = text.chars().any(verba_core::is_arabic);
    let (translate_to, translator) = with_app_ret(|app| (app.settings.translate_to, Arc::clone(&app.sentences))).ok_or("Verba is busy; try again.")?;
    let to = match choice {
        1 => Lang::En,
        2 => Lang::Fr,
        3 => Lang::Ar,
        _ if arabic => {
            if translate_to == Lang::Ar { Lang::En } else { translate_to }
        }
        _ => Lang::Ar,
    };
    let name = |l: Lang| match l {
        Lang::En => "English",
        Lang::Fr => "French",
        Lang::Ar => "Arabic",
        Lang::Dz => "Darija",
    };
    // Latin text going to English is taken for French (it is not English), going elsewhere for English
    let hint = if arabic { Some(Lang::Ar) } else if to == Lang::En { Some(Lang::Fr) } else { Some(Lang::En) };
    // a word or two: the dictionary says more than a sentence translator does
    if text.split_whitespace().count() <= 3 && text.chars().count() <= 40 {
        let found = with_app_ret(|app| app.engine.translate_word_limited(text, &[to], hint, 8)).flatten();
        if let Some(t) = found.filter(|t| t.to == to && t.from != t.to) {
            let list: Vec<&str> = t.candidates.iter().map(|c| c.text.as_str()).collect();
            return Ok(format!("{} \u{2192} {}\n{}", name(t.from), name(t.to), list.join("  \u{B7}  ")));
        }
    }
    // a sentence: the translator (Arabic and English), when it has the pair and answers
    let installed = translator.installed();
    // (only when it goes where the person chose: it knows Arabic and English, so a request for French is not answered with English)
    if let Some((from, into)) = verba_local::sentence::pair_for(text, None, to, &installed).filter(|(_, into)| *into == to) {
        if let Ok(out) = translator.translate(from, into, text) {
            if !out.trim().is_empty() {
                return Ok(format!("{} \u{2192} {}\n{}", name(from), name(into), out.trim()));
            }
        }
    }
    // else word by word
    let reading = with_app_ret(|app| app.engine.translate_words(text, to, hint)).flatten().ok_or("Verba found no translation for this text.")?;
    let from = if arabic { Lang::Ar } else { hint.unwrap_or(Lang::En) };
    Ok(format!("{} \u{2192} {}, word by word\n{}", name(from), name(to), reading.text))
}

// ───────────── dictation ─────────────

/// The pack that writes the next dictation down: the one the setting names, or the one of the keyboard that is in use. An error is a sentence for the person.
fn pick_pack(app: &App) -> Result<&'static dictation::Pack, String> {
    use settings::DictationLanguage;
    // the packs that may write it, the best first: Arabic text is written by the Darija pack when Algerian Darija is on in Languages (or chosen here),
    // else by the Arabic pack, and by the Darija pack when that is the only Arabic one installed
    let wanted: &[&str] = match app.settings.dictation_language {
        DictationLanguage::Arabic => &["ar"],
        DictationLanguage::Darija => &["dz"],
        DictationLanguage::English => &["en"],
        DictationLanguage::Auto => match app.last_layout {
            Some(Lang::Ar) if app.settings.darija => &["dz", "ar"],
            Some(Lang::Ar) => &["ar", "dz"],
            Some(Lang::En) => &["en"],
            _ => &[],
        },
    };
    let ready: Vec<&'static dictation::Pack> = dictation::PACKS.iter().filter(|p| dictation::installed(p)).collect();
    match wanted.first() {
        Some(first) => wanted.iter().find_map(|id| ready.iter().copied().find(|p| p.id == *id)).ok_or_else(|| {
            let title = dictation::pack(first).map_or("", |p| p.title);
            format!("The {title} dictation pack is not installed: Settings, Dictation, Download.")
        }),
        // another keyboard (French, German...) or none: the only pack that is installed
        None => ready.first().copied().ok_or_else(|| "Dictation is not installed: Settings, Dictation, Download.".to_string()),
    }
}

/// The shortcut of the dictation: starts listening, or ends the recording and writes it down.
fn toggle_dictation(app: &mut App) {
    if app.recorder.is_some() {
        finish_dictation(app);
        return;
    }
    if app.dictation_busy {
        notice(app, None, "Still writing down the last dictation\u{2026}");
        return;
    }
    let pack = match pick_pack(app) {
        Ok(pack) => pack,
        Err(why) => {
            notice(app, None, &why);
            return;
        }
    };
    if app.sentences.exe().is_none() {
        notice(app, None, "The speech program (verba-translate.exe) is not installed: run the Verba setup again.");
        return;
    }
    match dictation::Recorder::start() {
        Ok(recorder) => {
            app.recorder = Some(recorder);
            app.dictation_pack = Some(pack);
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::SetTimer(Some(HWND(CONTROLLER.load(Ordering::SeqCst) as *mut _)), TIMER_DICTATE, 100, None);
            }
            let text = format!("Listening ({})\u{2026} press the key again when you are done", pack.title);
            show_notice(app, None, &text, false);
        }
        Err(why) => notice(app, None, &why),
    }
}

/// Every 100 ms while recording: takes the sound, and ends the recording when the person stopped talking.
fn listen(app: &mut App) {
    let Some(recorder) = app.recorder.as_mut() else {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(HWND(CONTROLLER.load(Ordering::SeqCst) as *mut _)), TIMER_DICTATE);
        }
        return;
    };
    if recorder.poll() || recorder.vad.dead() {
        finish_dictation(app);
    }
}

/// Ends the recording and has it written down on a thread of its own (a few seconds).
fn finish_dictation(app: &mut App) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::KillTimer(Some(HWND(CONTROLLER.load(Ordering::SeqCst) as *mut _)), TIMER_DICTATE);
    }
    let (Some(recorder), Some(pack)) = (app.recorder.take(), app.dictation_pack.take()) else { return };
    let (pcm, vad) = recorder.finish();
    if vad.dead() {
        notice(app, None, "No sound reached Verba: check that the microphone is allowed in Windows, Settings, Privacy, Microphone.");
        return;
    }
    if !vad.heard_speech() || vad.seconds() < 0.4 {
        notice(app, None, "No speech was heard.");
        return;
    }
    let Some(exe) = app.sentences.exe() else { return };
    app.dictation_busy = true;
    show_notice(app, None, "Writing down\u{2026}", false);
    std::thread::spawn(move || {
        let result = dictation::transcribe(&exe, pack, &pcm);
        if let Ok(mut slot) = DICTATED.lock() {
            *slot = Some(result);
        }
        let controller = CONTROLLER.load(Ordering::SeqCst);
        if controller != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(controller as *mut _)), WM_DICTATED, WPARAM(0), LPARAM(0));
            }
        }
    });
}

/// The words came back: they are typed where the cursor is, followed by a space.
fn type_dictation(app: &mut App, result: Result<String, String>) {
    app.dictation_busy = false;
    hide(app);
    match result {
        Ok(text) if !text.trim().is_empty() => {
            send_replacement(0, &format!("{} ", text.trim()), app.settings.line_break_shift, "dictation");
            app.dismissed = None;
        }
        Ok(_) => notice(app, None, "Nothing was written down."),
        Err(why) => {
            log(&format!("dictation failed: {why}"));
            notice(app, None, "The dictation could not be written down.");
        }
    }
}

/// (Development builds.) The program that writes speech down, for the test thread.
fn app_sentences_exe() -> Option<PathBuf> {
    APP.with(|a| a.try_borrow().ok().and_then(|b| b.as_ref().and_then(|app| app.sentences.exe())))
}

/// (Development builds.) The checks of `VERBA_TEST_DICTATION`, on a thread of their own.
fn test_dictation(what: &str, exe: Option<PathBuf>) {
    let Some(dir) = Settings::dir() else { return };
    let out = dir.join("dictation-test.txt");
    let say = |text: String| {
        let _ = std::fs::OpenOptions::new().create(true).append(true).open(&out).and_then(|mut f| std::io::Write::write_all(&mut f, format!("{text}\n").as_bytes()));
    };
    let mut parts = what.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("download"), Some(id), _) => {
            if let Err(why) = dictation_download(id) {
                say(format!("not started: {why}"));
                return;
            }
            let mut last = String::new();
            loop {
                std::thread::sleep(Duration::from_millis(1000));
                let line = match dictation::progress() {
                    Some(p) => format!("{:?} {} / {} MB ({:.0} %)", p.phase, p.done / 1_000_000, p.total / 1_000_000, p.fraction() * 100.0),
                    None => {
                        say(format!("finished: installed = {}", dictation::pack(id).is_some_and(dictation::installed)));
                        return;
                    }
                };
                if line != last {
                    say(line.clone());
                    last = line;
                }
                if matches!(dictation::progress().map(|p| p.phase), Some(dictation::Phase::Failed(_))) {
                    return;
                }
            }
        }
        (Some("file"), Some(id), Some(wav)) => {
            let (Some(pack), Some(exe)) = (dictation::pack(id), exe) else { return say("no pack or no program".to_string()) };
            match dictation::read_wav_pcm(Path::new(wav)) {
                Ok(pcm) => {
                    let t = Instant::now();
                    let first = dictation::transcribe(&exe, pack, &pcm);
                    say(format!("1 ({:.1}s): {first:?}", t.elapsed().as_secs_f32()));
                    // a second request in the same session: the model is loaded already
                    let t = Instant::now();
                    let second = dictation::transcribe(&exe, pack, &pcm);
                    say(format!("2 ({:.1}s): {second:?}", t.elapsed().as_secs_f32()));
                }
                Err(why) => say(format!("cannot read the file: {why}")),
            }
        }
        (Some("quick"), Some(choice), Some(text)) => {
            let t = Instant::now();
            let answer = quick_translate(text, choice.parse().unwrap_or(0));
            say(format!("({:.1}s) {answer:?}", t.elapsed().as_secs_f32()));
        }
        (Some("mic"), _, _) => match dictation::Recorder::start() {
            Ok(mut r) => {
                for _ in 0..15 {
                    std::thread::sleep(Duration::from_millis(100));
                    r.poll();
                }
                let (pcm, vad) = r.finish();
                say(format!("microphone: {} samples ({:.1} s), speech heard {}, dead {}", pcm.len(), vad.seconds(), vad.heard_speech(), vad.dead()));
            }
            Err(why) => say(format!("microphone: {why}")),
        },
        _ => say(format!("unknown: {what}")),
    }
}

/// Starts the download of a dictation pack; the settings window is drawn again as it moves. An error is a sentence for the person.
pub(crate) fn dictation_download(id: &str) -> Result<(), String> {
    let pack = dictation::pack(id).ok_or("There is no such pack.")?;
    dictation::clear_failure();
    dictation::start_download(pack, || {
        let controller = CONTROLLER.load(Ordering::SeqCst);
        if controller != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(controller as *mut _)), WM_DICTATION, WPARAM(0), LPARAM(0));
            }
        }
    })
}

/// Takes a dictation pack off the disk.
pub(crate) fn dictation_remove(id: &str) {
    if let Some(pack) = dictation::pack(id) {
        dictation::end_session();
        dictation::remove(pack);
    }
}

/// Whether the corrected word may be taken back: it is still the last thing before the caret and the fix is recent. Anything typed
/// since would be erased with it.
fn undo_applies(before_caret: &str, corrected: &str, age: Duration) -> bool {
    age <= Duration::from_secs(120) && !corrected.is_empty() && before_caret.ends_with(corrected)
}

/// Erases `erase` characters before the caret and types `text`, with keys the program recognises as its own.
fn send_replacement(erase: usize, text: &str, shift_enter: bool, what: &str) {
    unsafe {
        let mut inputs: Vec<INPUT> = Vec::new();
        for typed in replace_keys(erase, text) {
            match typed {
                Typed::Backspace => {
                    inputs.push(key(VK_BACK, 0, KEYBD_EVENT_FLAGS(0)));
                    inputs.push(key(VK_BACK, 0, KEYEVENTF_KEYUP));
                }
                Typed::Enter => {
                    for (vk, up) in line_break_keys(shift_enter) {
                        inputs.push(key(VIRTUAL_KEY(vk), 0, if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }));
                    }
                }
                Typed::Unit(unit) => {
                    inputs.push(key(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE));
                    inputs.push(key(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
            }
        }
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        log(&format!("typed '{}': {} keys to send, {} sent, {} characters replaced", what, inputs.len(), sent, erase));
    }
}

// ───────────── the personal dictionary ─────────────

fn stamp_of(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Learns what was collected (one sentence at most) and writes the files.
fn flush_learning(app: &mut App) {
    if app.learn_buf.is_empty() {
        return;
    }
    let text = std::mem::take(&mut app.learn_buf);
    app.learn_words = 0;
    if app.settings.learn {
        app.engine.learn_text(&text);
        save_personal(app);
    }
}

fn save_personal(app: &mut App) {
    let Some((words, learned)) = &app.files else { return };
    if app.engine.personal().is_dirty() {
        let _ = app.engine.personal_mut().save(words, learned);
        app.words_stamp = stamp_of(words);
    }
}

/// A person may edit `personal.txt` (the settings window opens it in Notepad): read it again when it changed.
fn reload_personal_if_edited(app: &mut App) {
    if app.last_check.elapsed() < Duration::from_secs(2) {
        return;
    }
    app.last_check = Instant::now();
    if let Some((words, learned)) = app.files.clone() {
        let stamp = stamp_of(&words);
        if stamp != app.words_stamp && !app.engine.personal().is_dirty() {
            app.engine.set_personal(PersonalDict::load(&words, &learned));
            app.words_stamp = stamp;
        }
    }
    if let Some(path) = app.tr_file.clone() {
        let stamp = stamp_of(&path);
        if stamp != app.tr_stamp {
            app.engine.set_user_translations(UserTranslations::load(&path));
            app.tr_stamp = stamp;
        }
    }
    sync_if_news(app);
}

/// The file of the sync folder.
const SYNC_FILE: &str = "verba-sync.txt";

/// With a sync folder chosen: looks every 20 seconds whether the file there, `personal.txt` or `my-translations.txt` changed since the last sync,
/// and syncs when one did (cheap: three times of files).
fn sync_if_news(app: &mut App) {
    if app.settings.sync_folder.trim().is_empty() || app.sync_checked.elapsed() < Duration::from_secs(20) {
        return;
    }
    app.sync_checked = Instant::now();
    let seen = (stamp_of(&Path::new(app.settings.sync_folder.trim()).join(SYNC_FILE)), app.words_stamp, app.tr_stamp);
    if seen == app.sync_seen {
        return;
    }
    if let Err(why) = sync_with(app) {
        log(&format!("sync: {why}"));
        // looked at: tried again when something changes, not every 20 seconds
        app.sync_seen = seen;
    }
}

/// Syncs now: merges the words and translations of this PC with the file of the sync folder, and writes what changed on each side.
pub(crate) fn sync_now() -> Result<sync::Report, String> {
    with_app_ret(sync_with).ok_or("Verba is busy; try again.")?
}

fn sync_with(app: &mut App) -> Result<sync::Report, String> {
    use sync::State;
    let folder = PathBuf::from(app.settings.sync_folder.trim());
    if folder.as_os_str().is_empty() {
        return Err("No sync folder is chosen.".to_string());
    }
    if !folder.is_dir() {
        return Err(format!("The folder {} is not there (is the drive connected?).", folder.display()));
    }
    let file = folder.join(SYNC_FILE);
    let remote = match std::fs::read_to_string(&file) {
        Ok(text) => State::from_text(&text).ok_or_else(|| format!("{} is not a Verba sync file.", file.display()))?,
        Err(_) => State::default(),
    };
    let remote_time = stamp_of(&file);
    let dir = Settings::dir().ok_or("Verba has no folder for its files.")?;
    let base_path = dir.join("sync-base.txt");
    let base = std::fs::read_to_string(&base_path).ok().and_then(|t| State::from_text(&t)).unwrap_or_default();
    let local = State::from_parts(&app.engine.personal().added_words(), app.engine.user_translations());
    // which side changed last: the file in the folder, or the files here
    let local_time = [app.words_stamp, app.tr_stamp].into_iter().flatten().max();
    let remote_newer = match (remote_time, local_time) {
        (Some(r), Some(l)) => r >= l,
        (Some(_), None) => true,
        _ => false,
    };
    let merged = sync::merge(&base, &local, &remote, remote_newer);
    let report = sync::report(&local, &merged, &remote);

    // here: the words and translations the others added or changed come in, the ones they removed go out
    for (key, form) in &merged.words {
        if !local.words.contains_key(key) {
            app.engine.personal_mut().add_word(form);
        }
    }
    for (key, form) in &local.words {
        if !merged.words.contains_key(key) {
            app.engine.personal_mut().remove_word(form);
        }
    }
    save_personal(app);
    if merged.entries != local.entries {
        save_translations(app, merged.translations())?;
    }
    // there: the file, when this PC had news; and what the two agree on now
    if report.file_written {
        std::fs::write(&file, merged.to_text()).map_err(|e| format!("Could not write {}: {e}", file.display()))?;
    }
    std::fs::create_dir_all(&dir).ok();
    std::fs::write(&base_path, merged.to_text()).map_err(|e| format!("Could not write the memory of the sync: {e}"))?;
    app.sync_seen = (stamp_of(&file), app.words_stamp, app.tr_stamp);
    Ok(report)
}

/// The words the user added by hand, for the settings window.
pub(crate) fn my_words() -> Vec<String> {
    APP.with(|a| a.try_borrow().ok().and_then(|b| b.as_ref().map(|app| app.engine.personal().added_words())).unwrap_or_default())
}

/// Adds a word by hand. Returns whether it was new (a word the person already has, or one that is not a word, is not added).
pub(crate) fn add_my_word(word: &str) -> bool {
    with_app_ret(|app| {
        let added = app.engine.personal_mut().add_word(word);
        if added {
            save_personal(app);
        }
        added
    })
    .unwrap_or(false)
}

/// Removes a word the user added by hand.
pub(crate) fn remove_my_word(word: &str) {
    with_app(|app| {
        app.engine.personal_mut().remove_word(word);
        save_personal(app);
    });
}

/// The translations the user wrote, for the settings window.
pub(crate) fn my_translations() -> Vec<UserEntry> {
    APP.with(|a| a.try_borrow().ok().and_then(|b| b.as_ref().map(|app| app.engine.user_translations().entries().to_vec()))).unwrap_or_default()
}

/// Writes a translation by hand (or replaces the one written before for the same word). The error is a sentence for the person.
pub(crate) fn add_my_translation(from: Lang, to: Lang, word: &str, translations: &str) -> Result<(), String> {
    with_app_ret(|app| {
        let mut all = app.engine.user_translations().clone();
        all.add(from, to, word, translations)?;
        save_translations(app, all)
    })
    .unwrap_or_else(|| Err("Verba is busy; try again.".to_string()))
}

/// Removes the translation at that place of `my_translations()`.
pub(crate) fn remove_my_translation(index: usize) {
    with_app(|app| {
        let mut all = app.engine.user_translations().clone();
        if all.remove(index) {
            let _ = save_translations(app, all);
        }
    });
}

/// Gives the engine the translations and writes the file. The engine has them even when the file could not be written (the error says so).
fn save_translations(app: &mut App, all: UserTranslations) -> Result<(), String> {
    app.engine.set_user_translations(all);
    let Some(path) = app.tr_file.clone() else { return Ok(()) };
    let saved = std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).and_then(|_| app.engine.user_translations().save(&path));
    app.tr_stamp = stamp_of(&path);
    saved.map_err(|e| format!("Could not write the file: {e}"))
}

/// Writes the backup file: the words and translations the user wrote, and the settings. Returns how many words and translations it holds.
pub(crate) fn export_backup(path: &Path) -> Result<(usize, usize), String> {
    let (words, translations, settings) = with_app_ret(|app| (app.engine.personal().added_words(), app.engine.user_translations().clone(), app.settings.render()))
        .ok_or("Verba is busy; try again.")?;
    std::fs::write(path, backup::render(&words, &translations, &settings)).map_err(|e| format!("Could not write the file: {e}"))?;
    Ok((words.len(), translations.len()))
}

/// What reading a backup file changed.
pub(crate) struct Imported {
    pub words: usize,
    pub translations: usize,
    pub settings: bool,
}

/// Reads a backup file: its words and translations are added to the ones the person has (a translation of the same word is replaced) and its
/// settings, when it has them, replace the current ones.
pub(crate) fn import_backup(path: &Path) -> Result<Imported, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read the file: {e}"))?;
    let b = backup::parse(&text).ok_or("This is not a Verba backup file.")?;
    let (words, translations) = with_app_ret(|app| -> Result<(usize, usize), String> {
        let mut added = 0;
        for w in &b.words {
            added += usize::from(app.engine.personal_mut().add_word(w));
        }
        save_personal(app);
        let mut all = app.engine.user_translations().clone();
        let mut kept = 0;
        for e in b.translations.entries() {
            kept += usize::from(all.add(e.from, e.to, &e.word, &e.translations.join("; ")).is_ok());
        }
        save_translations(app, all)?;
        Ok((added, kept))
    })
    .ok_or("Verba is busy; try again.")??;
    let settings = b.settings.map(|text| Settings::parse(&text));
    let changed = settings.is_some();
    if let Some(new) = settings {
        // the sync folder is a path on the PC that made the backup: this PC keeps its own
        update_settings(|current| {
            let folder = std::mem::take(&mut current.sync_folder);
            *current = new;
            current.sync_folder = folder;
        });
        settings_window::refresh();
    }
    Ok(Imported { words, translations, settings: changed })
}

/// The text of the dictionary window for a word: one line per language it can be translated into. `None` when there is nothing to show.
pub(crate) fn look_up_word(word: &str) -> Option<String> {
    let found = with_app_ret(|app| app.engine.lookup_word(word, None))?;
    if found.is_empty() {
        return None;
    }
    let name = |l: Lang| match l {
        Lang::En => "English",
        Lang::Fr => "French",
        Lang::Ar => "Arabic",
        Lang::Dz => "Darija",
    };
    let mut out = format!("{} \u{2192}\n", name(found[0].from));
    for t in &found {
        let words: Vec<&str> = t.candidates.iter().map(|c| c.text.as_str()).collect();
        out.push_str(&format!("{}:  {}\n", name(t.to), words.join("  \u{B7}  ")));
    }
    Some(out)
}

/// Opens `my-translations.txt` in Notepad (made with its explanation when it does not exist yet).
pub(crate) fn open_my_translations() {
    with_app(|app| {
        let Some(path) = app.tr_file.clone() else { return };
        if !path.exists() {
            let _ = save_translations(app, app.engine.user_translations().clone());
        }
        let _ = std::process::Command::new("notepad.exe").arg(&path).spawn();
    });
}

/// Opens `personal.txt` in Notepad (made with its explanation when it does not exist yet).
pub(crate) fn open_my_words() {
    let Some(dir) = Settings::dir() else { return };
    let path = dir.join("personal.txt");
    if !path.exists() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&path, "# Verba personal dictionary: one word per line. You can edit this file.\n# A line that starts with - is a word that must never be learned automatically.\n");
    }
    let _ = std::process::Command::new("notepad.exe").arg(&path).spawn();
}

/// Forgets every word and word pair that was learned. The words the user added by hand stay.
pub(crate) fn forget_learned() {
    with_app(|app| {
        app.learn_buf.clear();
        app.learn_words = 0;
        app.engine.personal_mut().clear_learned();
        save_personal(app);
    });
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: VERBA_KEY_MARK } },
    }
}

/// A copy of the settings, for the settings window.
pub(crate) fn current_settings() -> Settings {
    APP.with(|a| a.try_borrow().ok().and_then(|b| b.as_ref().map(|app| app.settings.clone()))).unwrap_or_default()
}

/// Changes the settings and makes the change take effect everywhere at once: engine, reader thread, popup, tray icon, file.
pub(crate) fn update_settings(change: impl FnOnce(&mut Settings)) {
    with_app(|app| {
        change(&mut app.settings);
        for lang in [Lang::En, Lang::Fr, Lang::Ar, Lang::Dz] {
            app.engine.set_enabled(lang, app.settings.lang_on(lang));
        }
        PAUSED.store(app.settings.paused, Ordering::SeqCst);
        share_keys(&app.settings.keys);
        sync_translate_hotkey(HWND(CONTROLLER.load(Ordering::SeqCst) as *mut _), app.settings.keys.translate);
        sync_fixkeys_hotkey(HWND(CONTROLLER.load(Ordering::SeqCst) as *mut _), app.settings.keys.fixkeys);
        sync_undo_hotkey(HWND(CONTROLLER.load(Ordering::SeqCst) as *mut _), app.settings.keys.undo);
        sync_dictate_hotkey(HWND(CONTROLLER.load(Ordering::SeqCst) as *mut _), app.settings.keys.dictate);
        sync_quick_hotkey(HWND(CONTROLLER.load(Ordering::SeqCst) as *mut _), app.settings.keys.quick);
        if !app.settings.learn {
            app.learn_buf.clear();
            app.learn_words = 0;
        }
        share_rules(&app.settings);
        // what is on the screen may no longer be allowed or in the right language: let the next reading decide
        hide(app);
        app.settings.save();
        let controller = HWND(CONTROLLER.load(Ordering::SeqCst) as *mut _);
        modify_tray(controller, app);
    });
    // the settings window, when it is open, shows the new values
    settings_window::refresh();
}

/// Registers the translate shortcut with Windows (and drops the old one when the user changed it).
fn sync_translate_hotkey(hwnd: HWND, binding: Binding) {
    sync_hotkey(hwnd, HOTKEY_TRANSLATE, &TRANSLATE_KEY, binding, "translate");
}

/// The same for the shortcut that puts right text typed with the wrong keyboard.
fn sync_fixkeys_hotkey(hwnd: HWND, binding: Binding) {
    sync_hotkey(hwnd, HOTKEY_FIXKEYS, &FIXKEYS_KEY, binding, "wrong keyboard");
}

/// The same for the shortcut that opens the quick translation window.
fn sync_quick_hotkey(hwnd: HWND, binding: Binding) {
    sync_hotkey(hwnd, HOTKEY_QUICK, &QUICK_KEY, binding, "quick window");
}

/// The same for the shortcut that starts and ends a dictation.
fn sync_dictate_hotkey(hwnd: HWND, binding: Binding) {
    sync_hotkey(hwnd, HOTKEY_DICTATE, &DICTATE_KEY, binding, "dictation");
}

/// The same for the shortcut that puts back the word Verba corrected.
fn sync_undo_hotkey(hwnd: HWND, binding: Binding) {
    sync_hotkey(hwnd, HOTKEY_UNDO, &UNDO_KEY, binding, "undo");
}

/// Remembers whether Windows accepted a shortcut, for the diagnostics.
fn note_shortcut(what: &str, ok: bool) {
    if let Ok(mut refused) = REFUSED_SHORTCUTS.lock() {
        refused.retain(|w| w != what);
        if !ok {
            refused.push(what.to_string());
        }
    }
}

/// The names of the shortcuts Windows refused.
pub(crate) fn refused_shortcuts() -> Vec<String> {
    REFUSED_SHORTCUTS.lock().map(|r| r.clone()).unwrap_or_default()
}

/// A text value of the registry (the name of this Windows), `None` when it is not there.
fn registry_text(root: windows::Win32::System::Registry::HKEY, key: windows::core::PCWSTR, name: windows::core::PCWSTR) -> Option<String> {
    unsafe {
        let mut bytes = 0u32;
        RegGetValueW(root, key, name, RRF_RT_REG_SZ, None, None, Some(&mut bytes)).ok().ok()?;
        let mut buf = vec![0u16; (bytes as usize).div_ceil(2) + 1];
        RegGetValueW(root, key, name, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut bytes)).ok().ok()?;
        let len = buf.iter().position(|u| *u == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..len]))
    }
}

/// Asks the releases page whether a newer version exists, on another thread; the answer comes back as `WM_UPDATE`. `asked` is whether the
/// person pressed the button (then the answer is always shown).
pub(crate) fn check_for_update(asked: bool) {
    std::thread::spawn(move || {
        let result = update::check(env!("CARGO_PKG_VERSION"));
        if let Ok(mut done) = UPDATE_DONE.lock() {
            *done = Some((asked, result));
        }
        let controller = CONTROLLER.load(Ordering::SeqCst);
        if controller != 0 {
            unsafe {
                let _ = PostMessageW(Some(HWND(controller as *mut _)), WM_UPDATE, WPARAM(0), LPARAM(0));
            }
        }
    });
}

/// With the daily check turned on: asks at start when the last answer is more than a day old.
fn check_for_update_if_due() {
    let Some(stamp_file) = Settings::dir().map(|d| d.join("update-check.txt")) else { return };
    let due = stamp_of(&stamp_file).and_then(|t| t.elapsed().ok()).is_none_or(|age| age > Duration::from_secs(24 * 3600));
    if due {
        let _ = std::fs::create_dir_all(stamp_file.parent().unwrap_or(Path::new(".")));
        let _ = std::fs::write(&stamp_file, "The time of this file is when Verba last asked whether a newer version exists.\n");
        check_for_update(false);
    }
}

/// The facts of the diagnostics report, as they are now.
fn gather_facts() -> diagnostics::Facts {
    use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
    let nt = w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let windows = {
        let name = registry_text(HKEY_LOCAL_MACHINE, nt, w!("ProductName")).unwrap_or_default();
        let display = registry_text(HKEY_LOCAL_MACHINE, nt, w!("DisplayVersion")).unwrap_or_default();
        let build = registry_text(HKEY_LOCAL_MACHINE, nt, w!("CurrentBuild")).unwrap_or_default();
        format!("{name} {display} (build {build})").replace("  ", " ").trim().to_string()
    };
    let layouts = verba_win::layouts::installed().into_iter().map(|l| (l.name, l.lang.map(|g| g.code().to_string()))).collect();
    let refused = refused_shortcuts();
    let last_app = LAST_APP.lock().ok().map(|a| a.clone()).filter(|a| !a.is_empty());
    let mut facts = diagnostics::Facts {
        version: env!("CARGO_PKG_VERSION").to_string(),
        build: if cfg!(debug_assertions) { "development" } else { "release" }.to_string(),
        windows,
        layouts,
        dictionary: dictionary_words(),
        sentence_pairs: sentence_pairs().iter().map(|(f, t)| format!("{} \u{2192} {}", f.code().to_uppercase(), t.code().to_uppercase())).collect(),
        last_app: last_app.clone(),
        ..Default::default()
    };
    with_app(|app| {
        for (lang, name) in [(Lang::En, "English"), (Lang::Fr, "French"), (Lang::Ar, "Arabic"), (Lang::Dz, "Darija")] {
            facts.lists.push((name.to_string(), app.engine.word_count(lang), app.engine.is_enabled(lang)));
        }
        facts.my_words = app.engine.personal().added_words().len();
        facts.my_translations = app.engine.user_translations().len();
        let s = &app.settings;
        let on = |b: bool| if b { "on" } else { "off" };
        facts.settings = vec![
            format!("suggestions: {}", if s.paused { "paused" } else { "on" }),
            format!("fix spelling: {}, next word: {}, learn: {}", on(s.correction), on(s.predict_next), on(s.learn)),
            format!("follow the keyboard language: {}, translate into: {}", on(s.follow_keyboard), s.translate_to.code()),
            format!("apps list: {} app(s), mode {}", s.apps.len(), if s.app_mode == settings::AppMode::OnlyThese { "only these" } else { "all except" }),
        ];
        let k = &s.keys;
        let key = |name: &str, label: String| (name.to_string(), label, !refused.iter().any(|r| r == name));
        facts.shortcuts = vec![
            key("translate", k.translate.label()),
            key("wrong keyboard", k.fixkeys.label()),
            key("undo", k.undo.label()),
            key("quit", "Ctrl+Alt+Shift+Q".to_string()),
            key("pause", "Ctrl+Alt+Shift+V".to_string()),
            key("this app on/off", "Ctrl+Alt+Shift+A".to_string()),
        ];
        facts.last_app_allowed = last_app.as_deref().map(|a| s.allows_app(a));
    });
    facts
}

/// Writes the diagnostics report next to the settings and opens it in Notepad.
pub(crate) fn show_report() {
    let Some(dir) = Settings::dir() else { return };
    let path = dir.join("report.txt");
    let _ = std::fs::create_dir_all(&dir);
    if std::fs::write(&path, diagnostics::render(&gather_facts())).is_ok() {
        // (a development build run by a test, VERBA_TEST_QUIET=1, only writes the file)
        if !(cfg!(debug_assertions) && std::env::var("VERBA_TEST_QUIET").is_ok()) {
            let _ = std::process::Command::new("notepad.exe").arg(&path).spawn();
        }
    }
}

fn sync_hotkey(hwnd: HWND, id: i32, current: &Mutex<Option<Binding>>, binding: Binding, what: &str) {
    let Ok(mut current) = current.lock() else { return };
    if *current == Some(binding) || hwnd.is_invalid() {
        return;
    }
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), id);
        let mut mods = MOD_NOREPEAT;
        if binding.mods & keys::MOD_CTRL != 0 {
            mods |= MOD_CONTROL;
        }
        if binding.mods & keys::MOD_ALT != 0 {
            mods |= MOD_ALT;
        }
        if binding.mods & keys::MOD_SHIFT != 0 {
            mods |= MOD_SHIFT;
        }
        let ok = RegisterHotKey(Some(hwnd), id, mods, binding.vk).is_ok();
        if !ok {
            log(&format!("could not register the {what} shortcut (another program uses it): choose another in the settings"));
        }
        note_shortcut(what, ok);
    }
    *current = Some(binding);
}

fn share_keys(keys: &Keys) {
    if let Ok(mut shared) = KEYS.lock() {
        *shared = *keys;
    }
}

fn share_rules(settings: &Settings) {
    if let Ok(mut rules) = RULES.lock() {
        *rules = Some(settings.clone());
    }
}

/// Switches Verba off in a program, or back on if it was off there. `None`: there is no program to act on.
fn toggle_app(app_name: Option<String>) {
    let Some(name) = app_name else { return };
    update_settings(|s| {
        let allowed = s.allows_app(&name);
        // for a program on the always-off list there is nothing to change
        if allowed || !settings::ALWAYS_OFF.contains(&name.as_str()) {
            s.set_app(&name, !allowed);
        }
    });
}

/// "Why does Verba not work in this field?": after 3 seconds (time to click in the field) reports which steps of reading it worked.
/// The report holds program and class names and step results, never the text.
pub(crate) fn field_check() {
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(3));
        let front = describe_foreground();
        let report = match Locator::new() {
            Ok(locator) => {
                let result = match locator.snapshot(48) {
                    Some(s) => format!("READABLE: {:?} anchor, {} letters read, keyboard {:?}", s.anchor.kind, s.before_caret.chars().count(), s.layout),
                    None => "NOT READABLE by Verba".to_string(),
                };
                format!("{front}

{result}

Steps: {}

This report contains no typed text.", locator.diagnose())
            }
            Err(e) => format!("{front}

UI Automation could not start: {e}"),
        };
        let text: Vec<u16> = report.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            MessageBoxW(None, PCWSTR(text.as_ptr()), w!("Verba: field check"), MB_OK | MB_ICONINFORMATION);
        }
    });
}

fn open_settings() {
    let icon = APP.with(|a| a.try_borrow().ok().and_then(|b| b.as_ref().map(|app| app.icon_on))).unwrap_or_default();
    settings_window::open(icon);
}

// ───────────── the keyboard hook ─────────────

/// Looks at one thing only: Tab or Esc while the popup is open. Everything else passes untouched.
unsafe extern "system" fn keyboard_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if code == 0 {
            let kb = &*(lp.0 as *const KBDLLHOOKSTRUCT);
            let ours = kb.dwExtraInfo == VERBA_KEY_MARK; // keys Verba itself sent
            let msg = wp.0 as u32;
            let ctrl_down = |vk: VIRTUAL_KEY| GetKeyState(vk.0 as i32) < 0;
            let mods = (ctrl_down(VK_CONTROL) as u8) * keys::MOD_CTRL | (ctrl_down(VK_MENU) as u8) * keys::MOD_ALT | (ctrl_down(VK_SHIFT) as u8) * keys::MOD_SHIFT;
            let controller = HWND(CONTROLLER.load(Ordering::SeqCst) as *mut _);
            let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let keys_now = KeyState {
                ours,
                is_down,
                is_up: msg == WM_KEYUP || msg == WM_SYSKEYUP,
                vk: kb.vkCode,
                mods,
                popup_visible: POPUP_VISIBLE.load(Ordering::SeqCst),
                // the previous and next keys act only when there is something to choose between
                several: POPUP_COUNT.load(Ordering::SeqCst) >= 2,
                kept_key: KEPT_KEY.load(Ordering::SeqCst),
                keys: KEYS.lock().map(|k| *k).unwrap_or_default(),
            };
            match key_action(&keys_now) {
                KeyAction::Accept => {
                    KEPT_KEY.store(kb.vkCode, Ordering::SeqCst);
                    let _ = PostMessageW(Some(controller), WM_ACCEPT, WPARAM(ACCEPT_HIGHLIGHTED), LPARAM(0));
                    return LRESULT(1);
                }
                KeyAction::Dismiss => {
                    KEPT_KEY.store(kb.vkCode, Ordering::SeqCst);
                    let _ = PostMessageW(Some(controller), WM_DISMISS, WPARAM(0), LPARAM(0));
                    return LRESULT(1);
                }
                KeyAction::Move(next) => {
                    KEPT_KEY.store(kb.vkCode, Ordering::SeqCst);
                    let _ = PostMessageW(Some(controller), WM_MOVE, WPARAM(next as usize), LPARAM(0));
                    return LRESULT(1);
                }
                KeyAction::Swallow => {
                    // the release of a key that was kept ends it; a repeat of one that is still held changes nothing
                    if keys_now.is_up {
                        KEPT_KEY.store(0, Ordering::SeqCst);
                    }
                    return LRESULT(1);
                }
                KeyAction::Pass => {}
            }
        }
        CallNextHookEx(None::<HHOOK>, code, wp, lp)
    }
}

/// What the keyboard hook does with a key.
#[derive(Debug, PartialEq, Eq)]
enum KeyAction {
    /// Let it through to the app, untouched.
    Pass,
    /// Tab while the popup is open: keep it from the app and accept the first suggestion.
    Accept,
    /// Esc while the popup is open: keep it from the app and close the popup.
    Dismiss,
    /// The left (false) or the right (true) key while there are several candidates: move the highlight that way.
    Move(bool),
    /// Keep the key from the app without doing anything: the release of a key we kept (so the app never sees half a key press),
    /// or the repeat of an accept or close key that is still held.
    Swallow,
}

/// What the hook knows about one key event.
struct KeyState {
    /// A key Verba sent itself.
    ours: bool,
    is_down: bool,
    is_up: bool,
    /// Virtual key code.
    vk: u32,
    /// Which of Ctrl, Alt and Shift are held (`keys::MOD_*`).
    mods: u8,
    popup_visible: bool,
    /// The popup shows two or more candidates.
    several: bool,
    /// The key whose press was kept and whose release is still to come (0 when none).
    kept_key: u32,
    keys: Keys,
}

/// The whole keyboard rule in one place (so it can be tested without a keyboard).
/// Only the keys the user set (Tab, the left and right arrows and Esc unless changed), only while the popup is open,
/// with exactly the modifiers set for them, and never the keys Verba sent itself.
fn key_action(k: &KeyState) -> KeyAction {
    if k.ours {
        return KeyAction::Pass;
    }
    if k.is_down && k.popup_visible {
        let pressed = Binding::new(k.vk, k.mods);
        let held_again = k.kept_key != 0 && k.kept_key == k.vk;
        if pressed == k.keys.accept {
            return if held_again { KeyAction::Swallow } else { KeyAction::Accept };
        }
        if pressed == k.keys.close {
            return if held_again { KeyAction::Swallow } else { KeyAction::Dismiss };
        }
        if k.several && pressed == k.keys.next {
            return KeyAction::Move(true);
        }
        if k.several && pressed == k.keys.prev {
            return KeyAction::Move(false);
        }
    }
    if k.is_up && k.kept_key != 0 && k.vk == k.kept_key {
        return KeyAction::Swallow;
    }
    KeyAction::Pass
}

/// What must be typed to put a suggestion in place of the part of the word already typed.
#[derive(Debug, PartialEq, Eq)]
enum Typed {
    Backspace,
    /// A line break in a translated selection: the Enter key (a character 0x0A typed as text is ignored by many programs).
    Enter,
    Unit(u16),
}

/// The keys of one line break as (virtual key, is a release): Enter, or Shift+Enter for the chat apps where Enter sends the message.
fn line_break_keys(shift: bool) -> Vec<(u16, bool)> {
    const SHIFT: u16 = 0x10;
    const ENTER: u16 = 0x0D;
    if shift { vec![(SHIFT, false), (ENTER, false), (ENTER, true), (SHIFT, true)] } else { vec![(ENTER, false), (ENTER, true)] }
}

/// `erase` backspaces, then `text` (as UTF-16 units, which is what Windows types).
fn replace_keys(erase: usize, text: &str) -> Vec<Typed> {
    let mut keys: Vec<Typed> = (0..erase).map(|_| Typed::Backspace).collect();
    for c in text.chars() {
        match c {
            '\r' => {}
            '\n' => keys.push(Typed::Enter),
            _ => {
                let mut units = [0u16; 2];
                keys.extend(c.encode_utf16(&mut units).iter().copied().map(Typed::Unit));
            }
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fix_is_taken_back_only_while_it_is_the_last_thing_before_the_caret() {
        let fresh = Duration::from_secs(3);
        assert!(undo_applies("I wrote teh ", "the ", fresh) == false, "the original is there, not the fix");
        assert!(undo_applies("I wrote the ", "the ", fresh));
        // something was typed after the fix: erasing would take it too
        assert!(!undo_applies("I wrote the cat", "the ", fresh));
        // too old, or nothing was corrected
        assert!(!undo_applies("I wrote the ", "the ", Duration::from_secs(600)));
        assert!(!undo_applies("I wrote the ", "", fresh));
    }

    const TAB: u32 = 0x09;
    const ESC: u32 = 0x1B;
    const LEFT: u32 = 0x25;
    const RIGHT: u32 = 0x27;
    const ENTER: u32 = 0x0D;

    /// A key going down while the popup is open with three candidates and the default keys.
    fn down(vk: u32, mods: u8) -> KeyState {
        KeyState { ours: false, is_down: true, is_up: false, vk, mods, popup_visible: true, several: true, kept_key: 0, keys: DEFAULT_KEYS }
    }

    #[test]
    fn tab_accepts_only_when_the_popup_is_open_and_no_modifier_is_held() {
        assert_eq!(key_action(&down(TAB, 0)), KeyAction::Accept);
        assert_eq!(key_action(&KeyState { popup_visible: false, ..down(TAB, 0) }), KeyAction::Pass, "popup closed: Tab is the app's");
        assert_eq!(key_action(&down(TAB, keys::MOD_SHIFT)), KeyAction::Pass, "Shift+Tab is the app's");
        assert_eq!(key_action(&down(TAB, keys::MOD_CTRL)), KeyAction::Pass, "Ctrl+Tab is the app's");
    }

    #[test]
    fn esc_closes_the_popup_only_when_it_is_open() {
        assert_eq!(key_action(&down(ESC, 0)), KeyAction::Dismiss);
        assert_eq!(key_action(&KeyState { popup_visible: false, ..down(ESC, 0) }), KeyAction::Pass);
    }

    #[test]
    fn the_arrows_choose_between_candidates_only_when_there_are_several() {
        assert_eq!(key_action(&down(RIGHT, 0)), KeyAction::Move(true));
        assert_eq!(key_action(&down(LEFT, 0)), KeyAction::Move(false));
        assert_eq!(key_action(&KeyState { several: false, ..down(RIGHT, 0) }), KeyAction::Pass, "one candidate: the arrow moves the cursor");
        assert_eq!(key_action(&KeyState { popup_visible: false, ..down(LEFT, 0) }), KeyAction::Pass, "no popup: the arrow is the app's");
        assert_eq!(key_action(&down(LEFT, keys::MOD_SHIFT)), KeyAction::Pass, "Shift+arrow selects text: not ours");
        assert_eq!(key_action(&down(RIGHT, keys::MOD_CTRL)), KeyAction::Pass, "Ctrl+arrow jumps a word: not ours");
    }

    #[test]
    fn the_keys_follow_what_the_user_set() {
        let mut custom = DEFAULT_KEYS;
        custom.accept = Binding::new(ENTER, 0);
        custom.next = Binding::new(0x28, 0); // down
        custom.prev = Binding::new(0x26, 0); // up
        let st = |vk, mods| KeyState { keys: custom, ..down(vk, mods) };
        assert_eq!(key_action(&st(ENTER, 0)), KeyAction::Accept);
        assert_eq!(key_action(&st(TAB, 0)), KeyAction::Pass, "Tab is no longer the accept key");
        assert_eq!(key_action(&st(0x28, 0)), KeyAction::Move(true));
        assert_eq!(key_action(&st(0x26, 0)), KeyAction::Move(false));
        assert_eq!(key_action(&st(RIGHT, 0)), KeyAction::Pass, "the arrows are free again");
        // a key with a modifier
        custom.next = Binding::new(RIGHT, keys::MOD_CTRL);
        assert_eq!(key_action(&KeyState { keys: custom, ..down(RIGHT, keys::MOD_CTRL) }), KeyAction::Move(true));
        assert_eq!(key_action(&KeyState { keys: custom, ..down(RIGHT, 0) }), KeyAction::Pass);
    }

    #[test]
    fn a_kept_key_keeps_its_release_and_nothing_else_is_touched() {
        let up = |vk, kept| KeyState { is_down: false, is_up: true, kept_key: kept, ..down(vk, 0) };
        assert_eq!(key_action(&up(TAB, TAB)), KeyAction::Swallow);
        assert_eq!(key_action(&up(RIGHT, RIGHT)), KeyAction::Swallow);
        assert_eq!(key_action(&up(TAB, 0)), KeyAction::Pass, "no key was kept");
        assert_eq!(key_action(&up(ESC, TAB)), KeyAction::Pass, "another key was kept");
        assert_eq!(key_action(&down(0x41, 0)), KeyAction::Pass, "letters are never touched");
    }

    #[test]
    fn holding_accept_does_not_accept_twice_but_holding_an_arrow_keeps_moving() {
        assert_eq!(key_action(&KeyState { kept_key: TAB, ..down(TAB, 0) }), KeyAction::Swallow);
        assert_eq!(key_action(&KeyState { kept_key: ESC, ..down(ESC, 0) }), KeyAction::Swallow);
        assert_eq!(key_action(&KeyState { kept_key: RIGHT, ..down(RIGHT, 0) }), KeyAction::Move(true));
    }

    #[test]
    fn the_highlight_moves_the_way_the_key_points() {
        // a left-to-right list: right goes to the next candidate, left to the previous one, round the end
        assert_eq!(highlight_after(0, 3, 1, false), 1);
        assert_eq!(highlight_after(2, 3, 1, false), 0, "round the end");
        assert_eq!(highlight_after(0, 3, -1, false), 2);
        // an Arabic list starts on the right: the right key goes back toward the first, the left key on to the next
        assert_eq!(highlight_after(1, 3, 1, true), 0);
        assert_eq!(highlight_after(1, 3, -1, true), 2);
        assert_eq!(highlight_after(0, 3, 1, true), 2, "round the end the other way");
        assert_eq!(highlight_after(0, 0, 1, false), 0);
    }

    #[test]
    fn keys_sent_by_verba_itself_pass_through() {
        assert_eq!(key_action(&KeyState { ours: true, ..down(TAB, 0) }), KeyAction::Pass);
    }

    #[test]
    fn accepting_replaces_the_typed_part_and_adds_a_space() {
        let keys = replace_keys(3, "like ");
        assert_eq!(&keys[..3], [Typed::Backspace, Typed::Backspace, Typed::Backspace]);
        let units: Vec<u16> = keys[3..].iter().map(|k| match k { Typed::Unit(u) => *u, Typed::Backspace | Typed::Enter => panic!("late backspace") }).collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "like ");
    }

    #[test]
    fn a_line_break_is_enter_or_shift_enter_and_shift_is_always_let_go() {
        assert_eq!(line_break_keys(false), [(0x0D, false), (0x0D, true)]);
        assert_eq!(line_break_keys(true), [(0x10, false), (0x0D, false), (0x0D, true), (0x10, true)]);
        // whatever the setting, every key that goes down comes up again
        for shift in [false, true] {
            let keys = line_break_keys(shift);
            for vk in [0x10u16, 0x0D] {
                assert_eq!(keys.iter().filter(|k| k.0 == vk && !k.1).count(), keys.iter().filter(|k| k.0 == vk && k.1).count());
            }
        }
    }

    #[test]
    fn a_translated_selection_with_line_breaks_types_them_as_the_enter_key() {
        let keys = replace_keys(0, "one\r\ntwo\n\nمرحبا");
        assert_eq!(keys.iter().filter(|k| **k == Typed::Enter).count(), 3, "every line break is one Enter, a CR is dropped");
        let units: Vec<u16> = keys.iter().filter_map(|k| if let Typed::Unit(u) = k { Some(*u) } else { None }).collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "onetwoمرحبا");
        // a character outside the basic plane is two units, sent in order
        let emoji: Vec<u16> = replace_keys(0, "a\u{1F600}").iter().filter_map(|k| if let Typed::Unit(u) = k { Some(*u) } else { None }).collect();
        assert_eq!(String::from_utf16(&emoji).unwrap(), "a\u{1F600}");
    }

    #[test]
    fn a_fix_erases_the_word_and_what_followed_and_types_them_back() {
        // `teh ` -> `the `: four characters erased (the word and the space), the corrected word and the same space typed
        let keys = replace_keys("teh ".chars().count(), &format!("{}{}", "the", " "));
        assert_eq!(keys.iter().filter(|k| **k == Typed::Backspace).count(), 4);
        let units: Vec<u16> = keys.iter().filter_map(|k| if let Typed::Unit(u) = k { Some(*u) } else { None }).collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "the ");
    }

    #[test]
    fn arabic_suggestions_type_as_their_own_letters() {
        let keys = replace_keys(2, "مرحبا ");
        assert_eq!(keys.iter().filter(|k| **k == Typed::Backspace).count(), 2);
        let units: Vec<u16> = keys.iter().filter_map(|k| if let Typed::Unit(u) = k { Some(*u) } else { None }).collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "مرحبا ");
    }
}

// ───────────── the tray icon and its menu ─────────────

fn tray_data(hwnd: HWND, icon: HICON, tip: &str) -> NOTIFYICONDATAW {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_ID,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    for (i, u) in tip.encode_utf16().take(nid.szTip.len() - 1).enumerate() {
        nid.szTip[i] = u;
    }
    nid
}

fn tip_for(paused: bool) -> &'static str {
    if paused {
        "Verba: off (click for settings, right-click for the menu)"
    } else {
        "Verba: on (click for settings, right-click for the menu)"
    }
}

/// The dimmed icon says at a glance that suggestions are off.
fn icon_for(app: &App) -> HICON {
    if app.settings.paused { app.icon_off } else { app.icon_on }
}

fn add_tray(hwnd: HWND) {
    with_app(|app| unsafe {
        let nid = tray_data(hwnd, icon_for(app), tip_for(app.settings.paused));
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    });
}

fn modify_tray(hwnd: HWND, app: &App) {
    unsafe {
        let nid = tray_data(hwnd, icon_for(app), tip_for(app.settings.paused));
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

fn remove_tray(hwnd: HWND) {
    unsafe {
        let nid = NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: TRAY_ID, ..Default::default() };
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

fn show_menu(hwnd: HWND) {
    let settings = current_settings();
    // the program the user was working in before they reached for the tray
    let last_app = LAST_APP.lock().ok().map(|a| a.clone()).unwrap_or_default();
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };

        let toggle = if settings.paused { w!("Turn Verba on\tCtrl+Alt+Shift+V") } else { w!("Turn Verba off\tCtrl+Alt+Shift+V") };
        let _ = AppendMenuW(menu, MF_STRING, CMD_PAUSE, toggle);

        // per app: "Turn off in chrome.exe" or "Turn on in chrome.exe"
        let (label, flags) = if last_app.is_empty() {
            ("Turn off in this app\tCtrl+Alt+Shift+A".to_string(), MF_STRING | MF_GRAYED)
        } else if settings::ALWAYS_OFF.contains(&last_app.as_str()) {
            (format!("{last_app} is always off"), MF_STRING | MF_GRAYED)
        } else if settings.allows_app(&last_app) {
            (format!("Turn off in {last_app}\tCtrl+Alt+Shift+A"), MF_STRING)
        } else {
            (format!("Turn on in {last_app}\tCtrl+Alt+Shift+A"), MF_STRING)
        };
        let label: Vec<u16> = label.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = AppendMenuW(menu, flags, CMD_APP, PCWSTR(label.as_ptr()));

        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, CMD_SETTINGS, w!("Settings…"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_CHECK, w!("Check a text field (3 s)…"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("Quit Verba\tCtrl+Alt+Shift+Q"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // the menu closes when you click elsewhere only if its window is in front
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu); // also frees the two submenus
    }
}

/// A round blue icon with a white V, drawn here so the program needs no resource file.
/// With `off` the disc is grey, so the tray shows at a glance that suggestions are off.
fn make_icon(off: bool) -> HICON {
    const N: usize = 32;
    let mut bgra = vec![0u8; N * N * 4];
    let segment_distance = |px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32| {
        let (dx, dy) = (bx - ax, by - ay);
        let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        ((px - (ax + t * dx)).powi(2) + (py - (ay + t * dy)).powi(2)).sqrt()
    };
    for y in 0..N {
        for x in 0..N {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let circle = 15.5 - ((px - 16.0).powi(2) + (py - 16.0).powi(2)).sqrt();
            let alpha = circle.clamp(0.0, 1.0);
            let v = segment_distance(px, py, 9.0, 10.0, 16.0, 23.0).min(segment_distance(px, py, 23.0, 10.0, 16.0, 23.0));
            let white = (2.4 - v).clamp(0.0, 1.0);
            let disc = if off { (140.0, 140.0, 146.0) } else { (0.0, 120.0, 215.0) };
            let (r, g, b) = (disc.0 + (255.0 - disc.0) * white, disc.1 + (255.0 - disc.1) * white, disc.2 + (255.0 - disc.2) * white);
            let i = (y * N + x) * 4;
            // premultiplied by the alpha of the circle's edge
            bgra[i] = (b * alpha) as u8;
            bgra[i + 1] = (g * alpha) as u8;
            bgra[i + 2] = (r * alpha) as u8;
            bgra[i + 3] = (255.0 * alpha) as u8;
        }
    }
    unsafe {
        let color = CreateBitmap(N as i32, N as i32, 1, 32, Some(bgra.as_ptr() as *const _));
        let mask = CreateBitmap(N as i32, N as i32, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}
