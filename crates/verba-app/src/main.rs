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

mod fixkeys;
mod keys;
mod learning;
mod sentences;
mod settings;
mod settings_window;

use keys::{Binding, DEFAULT_KEYS, Keys};
use settings::{Settings, ThemeMode};
use verba_core::{Engine, Lang, Suggestion, current_word, current_word_dz};
use verba_local::{LocalEngine, PersonalDict};
use verba_win::{Anchor, AnchorKind, Locator, Picked, Look, Overlay, Snapshot, Theme, describe_foreground, foreground_app, run_with_splash};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOD_ALT,
    MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, SendInput, UnregisterHotKey, VIRTUAL_KEY, VK_BACK, VK_CONTROL,
    VK_MENU, VK_RETURN, VK_SHIFT,
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
/// wParam of `WM_ACCEPT` that means "the highlighted one" (a click sends the number of the candidate instead).
const ACCEPT_HIGHLIGHTED: usize = usize::MAX;
/// How often the worker thread looks at the app being typed in.
const POLL_MS: u64 = 120;
const HOTKEY_QUIT: i32 = 1;
const HOTKEY_PAUSE: i32 = 2;
const HOTKEY_APP: i32 = 3;
const HOTKEY_TRANSLATE: i32 = 4;
const HOTKEY_FIXKEYS: i32 = 5;
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
    last_check: Instant,
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
            engine.add_dz_seed();
            (engine, files, words_stamp)
        };
        let (mut engine, files, words_stamp) =
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
                Some(App { overlay, engine, settings, icon_on, icon_off, shown: Vec::new(), typed_len: 0, dismissed: None, last_text: String::new(), fix: None, selected: 0, anchor: None, lead: None, translating: None, pair_labels: Vec::new(), sentences: Arc::new(sentences::Translator::find()), sentence_seq: 0, notice: false, notice_seq: 0, learn_buf: String::new(), learn_words: 0, files, words_stamp, last_check: Instant::now() });
        });

        let mods = MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
        if RegisterHotKey(Some(hwnd), HOTKEY_QUIT, mods, 'Q' as u32).is_err() {
            log("could not register Ctrl+Alt+Shift+Q (another program uses it); use the tray menu to quit");
        }
        let _ = RegisterHotKey(Some(hwnd), HOTKEY_PAUSE, mods, 'V' as u32);
        let _ = RegisterHotKey(Some(hwnd), HOTKEY_APP, mods, 'A' as u32);
        with_app(|app| sync_translate_hotkey(hwnd, app.settings.keys.translate));
        with_app(|app| sync_fixkeys_hotkey(hwnd, app.settings.keys.fixkeys));

        with_app(|app| app.overlay.set_click_target(hwnd, WM_ACCEPT));
        add_tray(hwnd);
        spawn_reader();
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), Some(hinst), 0).ok();
        if hook.is_none() {
            log("keyboard hook failed: Tab and Esc will not work");
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
                return engine;
            }
        }
    }
    log("no word lists found: run scripts/fetch-data.ps1");
    LocalEngine::new()
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
                let picked = if allowed { locator.pick_selection(fixkeys::READ_CAP) } else { None };
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
            let snap = if paused || !allowed { None } else { locator.snapshot(48) };
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
    app.translating = None;
    // the selection may be gone: a translation still on its way is for text that is no longer there
    app.sentence_seq += 1;
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
            // two or three selected words that are not in the dictionary as such: the sentence translator may know them
            if picked.selected && words >= 2 && !app.sentences.installed().is_empty() {
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
    if installed.is_empty() {
        notice(app, anchor, "Sentence translation is not installed. Run the Verba setup again and tick it.");
        return;
    }
    let Some((from, to)) = verba_local::sentence::pair_for(&picked.text, picked.layout, app.settings.translate_to, &installed) else {
        notice(app, anchor, "This language pair is not installed (Arabic and English are).");
        return;
    };
    app.sentence_seq += 1;
    let seq = app.sentence_seq;
    app.anchor = Some(picked.anchor);
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
            notice(app, anchor, "The sentence could not be translated.");
        }
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
    match fixkeys::offer(&picked.text, &verba_win::layouts::installed(), &app.engine) {
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
pub(crate) fn dictionary_words() -> Option<usize> {
    APP.with(|a| {
        a.try_borrow().ok().and_then(|b| {
            b.as_ref().and_then(|app| {
                if !app.engine.has_dictionary() {
                    return None;
                }
                let d = app.engine.dictionary();
                let langs = [Lang::En, Lang::Fr, Lang::Ar];
                Some(langs.iter().flat_map(|f| langs.iter().map(move |t| (*f, *t))).filter(|(f, t)| f != t).map(|(f, t)| d.len(f, t)).sum())
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
    // a completion replaces the part of the word typed and adds a space; a fix replaces the whole word and puts back what followed it
    let (erase, text) = match (&app.translating, &app.fix) {
        // a translation replaces the selected word (typing over it) or the word before the caret, and adds nothing
        (Some(erase), _) => (*erase, chosen.text.clone()),
        (None, Some(fix)) => (fix.erase, format!("{}{}", chosen.text, fix.trailing)),
        (None, None) => (app.typed_len, format!("{} ", chosen.text)),
    };
    hide(app);
    unsafe {
        let mut inputs: Vec<INPUT> = Vec::new();
        for typed in replace_keys(erase, &text) {
            match typed {
                Typed::Backspace => {
                    inputs.push(key(VK_BACK, 0, KEYBD_EVENT_FLAGS(0)));
                    inputs.push(key(VK_BACK, 0, KEYEVENTF_KEYUP));
                }
                Typed::Enter => {
                    inputs.push(key(VK_RETURN, 0, KEYBD_EVENT_FLAGS(0)));
                    inputs.push(key(VK_RETURN, 0, KEYEVENTF_KEYUP));
                }
                Typed::Unit(unit) => {
                    inputs.push(key(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE));
                    inputs.push(key(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
            }
        }
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        log(&format!("accepted '{}': {} keys to send, {} sent, {} characters replaced", chosen.text, inputs.len(), sent, erase));
    }
    // the next tick sees a fresh word: nothing to remember
    app.dismissed = None;
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
    let Some((words, learned)) = app.files.clone() else { return };
    let stamp = stamp_of(&words);
    if stamp != app.words_stamp && !app.engine.personal().is_dirty() {
        app.engine.set_personal(PersonalDict::load(&words, &learned));
        app.words_stamp = stamp;
    }
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
        if RegisterHotKey(Some(hwnd), id, mods, binding.vk).is_err() {
            log(&format!("could not register the {what} shortcut (another program uses it): choose another in the settings"));
        }
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
