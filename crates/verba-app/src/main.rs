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
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

mod settings;

use settings::{Settings, ThemeMode};
use verba_core::{Engine, Lang, Suggestion, current_word};
use verba_local::LocalEngine;
use verba_win::{Locator, Look, Overlay, Snapshot, Theme};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOD_ALT,
    MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, SendInput, UnregisterHotKey, VIRTUAL_KEY, VK_BACK, VK_CONTROL,
    VK_ESCAPE, VK_MENU, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallNextHookEx, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, HHOOK, HICON, ICONINFO, KBDLLHOOKSTRUCT,
    MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, SetWindowsHookExW, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_COMMAND, WM_DESTROY, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONUP,
    WM_NULL, WM_RBUTTONUP, WM_SYSKEYDOWN, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const WM_TRAY: u32 = WM_APP + 1;
const WM_ACCEPT: u32 = WM_APP + 2;
const WM_DISMISS: u32 = WM_APP + 3;
/// A fresh reading from the worker thread is waiting in `LATEST`.
const WM_SNAPSHOT: u32 = WM_APP + 4;
/// How often the worker thread looks at the app being typed in.
const POLL_MS: u64 = 120;
const HOTKEY_QUIT: i32 = 1;
const HOTKEY_PAUSE: i32 = 2;
const CMD_PAUSE: usize = 1001;
const CMD_QUIT: usize = 1002;
const CMD_LANG_EN: usize = 1011;
const CMD_LANG_FR: usize = 1012;
const CMD_LANG_AR: usize = 1013;
const CMD_THEME_AUTO: usize = 1021;
const CMD_THEME_LIGHT: usize = 1022;
const CMD_THEME_DARK: usize = 1023;
const CMD_AUTOSTART: usize = 1031;
const TRAY_ID: u32 = 1;
/// Stamped on the keys Verba itself sends, so its own keyboard hook can tell them from anyone else's
/// (remote-desktop tools and accessibility programs also send keys, and those must still work).
const VERBA_KEY_MARK: usize = 0x5645_5242; // "VERB"

// what the keyboard hook needs to know without touching the rest of the program
static POPUP_VISIBLE: AtomicBool = AtomicBool::new(false);
static SWALLOWED_TAB: AtomicBool = AtomicBool::new(false);
static CONTROLLER: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static PAUSED: AtomicBool = AtomicBool::new(false);
static QUITTING: AtomicBool = AtomicBool::new(false);
/// What the worker thread last read (outer `Some` = new, not yet taken; inner `None` = no text field has the focus).
static LATEST: Mutex<Option<Option<Snapshot>>> = Mutex::new(None);

struct App {
    overlay: Overlay,
    engine: LocalEngine,
    settings: Settings,
    icon: HICON,
    /// What the popup shows now.
    shown: Vec<Suggestion>,
    /// How many characters of the word are typed already (they are replaced when a suggestion is accepted).
    typed_len: usize,
    /// The text for which the user pressed Esc: stay closed until it changes.
    dismissed: Option<String>,
    /// The text before the caret at the last reading (what Esc remembers).
    last_text: String,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

fn main() {
    unsafe {
        // one copy at a time
        let _mutex = CreateMutexW(None, true, w!("Local\\VerbaSingleInstance"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }

        let settings = Settings::load();
        let mut engine = load_engine();
        for lang in [Lang::En, Lang::Fr, Lang::Ar] {
            engine.set_enabled(lang, settings.lang_on(lang));
        }
        PAUSED.store(settings.paused, Ordering::SeqCst);
        let overlay = Overlay::new().expect("could not make the popup window");
        let icon = make_icon();

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
                Some(App { overlay, engine, settings, icon, shown: Vec::new(), typed_len: 0, dismissed: None, last_text: String::new() });
        });

        let mods = MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
        if RegisterHotKey(Some(hwnd), HOTKEY_QUIT, mods, 'Q' as u32).is_err() {
            log("could not register Ctrl+Alt+Shift+Q (another program uses it); use the tray menu to quit");
        }
        let _ = RegisterHotKey(Some(hwnd), HOTKEY_PAUSE, mods, 'V' as u32);

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

fn autostart_enabled() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, w!("Verba"), RRF_RT_REG_SZ, None, None, None).is_ok() }
}

/// Start with Windows: one value in the user's Run key (no admin rights, nothing installed).
fn set_autostart(on: bool) {
    unsafe {
        if on {
            let Ok(exe) = std::env::current_exe() else { return };
            let value: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain(std::iter::once(0)).collect();
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
                HOTKEY_PAUSE => with_app(|app| toggle_pause(app, hwnd)),
                _ => {}
            }
            LRESULT(0)
        }
        WM_TRAY => {
            let event = (lp.0 as u32) & 0xFFFF;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                show_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let cmd = wp.0 & 0xFFFF;
            match cmd {
                CMD_PAUSE => with_app(|app| toggle_pause(app, hwnd)),
                CMD_QUIT => quit(),
                CMD_LANG_EN | CMD_LANG_FR | CMD_LANG_AR => with_app(|app| {
                    let lang = match cmd {
                        CMD_LANG_EN => Lang::En,
                        CMD_LANG_FR => Lang::Fr,
                        _ => Lang::Ar,
                    };
                    let on = !app.settings.lang_on(lang);
                    app.settings.set_lang(lang, on);
                    app.engine.set_enabled(lang, on);
                    app.settings.save();
                    hide(app);
                }),
                CMD_THEME_AUTO | CMD_THEME_LIGHT | CMD_THEME_DARK => with_app(|app| {
                    app.settings.theme = match cmd {
                        CMD_THEME_LIGHT => ThemeMode::Light,
                        CMD_THEME_DARK => ThemeMode::Dark,
                        _ => ThemeMode::Auto,
                    };
                    app.settings.save();
                    hide(app);
                }),
                CMD_AUTOSTART => set_autostart(!autostart_enabled()),
                _ => {}
            }
            LRESULT(0)
        }
        // a key (Tab) or a click on a candidate: wParam is the candidate's number
        WM_ACCEPT => {
            with_app(|app| accept(app, wp.0));
            LRESULT(0)
        }
        WM_DISMISS => {
            with_app(|app| {
                app.dismissed = Some(app.last_text.clone());
                hide(app);
            });
            LRESULT(0)
        }
        WM_DESTROY => {
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
        let mut last: Option<(i32, i32, i32, i32, String)> = None;
        let mut was_paused = false;
        while !QUITTING.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
            let paused = PAUSED.load(Ordering::SeqCst);
            let snap = if paused { None } else { locator.snapshot(48) };
            // wake the main thread only when something changed
            let key = snap.as_ref().map(|s| (s.anchor.rect.left, s.anchor.rect.top, s.anchor.rect.right, s.anchor.rect.bottom, s.before_caret.clone()));
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
    let Some(snap) = snap else {
        app.last_text.clear();
        if POPUP_VISIBLE.load(Ordering::SeqCst) {
            hide(app);
        }
        return;
    };
    app.last_text = snap.before_caret.clone();
    match &app.dismissed {
        Some(d) if *d == snap.before_caret => return, // Esc was pressed for this text: stay closed
        Some(_) => app.dismissed = None,              // the user typed on: allowed again
        None => {}
    }
    let found = app.engine.suggest(&snap.before_caret, 3);
    log(&format!("engine: {} suggestions", found.len()));
    if found.is_empty() {
        if POPUP_VISIBLE.load(Ordering::SeqCst) {
            hide(app);
        }
        return;
    }
    app.typed_len = current_word(&snap.before_caret).chars().count();
    app.overlay.show(snap.anchor, &found, 0, look_for(&app.settings));
    app.shown = found;
    POPUP_VISIBLE.store(true, Ordering::SeqCst);
}

/// Tab, or a click: replace the typed part of the word with that suggestion, plus a space.
fn accept(app: &mut App, index: usize) {
    let Some(first) = app.shown.get(index).cloned() else { return };
    hide(app);
    unsafe {
        let mut inputs: Vec<INPUT> = Vec::new();
        for typed in accept_keys(app.typed_len, &first.text) {
            match typed {
                Typed::Backspace => {
                    inputs.push(key(VK_BACK, 0, KEYBD_EVENT_FLAGS(0)));
                    inputs.push(key(VK_BACK, 0, KEYEVENTF_KEYUP));
                }
                Typed::Unit(unit) => {
                    inputs.push(key(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE));
                    inputs.push(key(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
            }
        }
        let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        log(&format!("accepted '{}': {} keys to send, {} sent, {} typed letters replaced", first.text, inputs.len(), sent, app.typed_len));
    }
    // the next tick sees a fresh word: nothing to remember
    app.dismissed = None;
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: VERBA_KEY_MARK } },
    }
}

fn toggle_pause(app: &mut App, hwnd: HWND) {
    app.settings.paused = !app.settings.paused;
    PAUSED.store(app.settings.paused, Ordering::SeqCst);
    if app.settings.paused {
        hide(app);
    }
    app.settings.save();
    modify_tray(hwnd, app);
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
            let plain = !ctrl_down(VK_SHIFT) && !ctrl_down(VK_CONTROL) && !ctrl_down(VK_MENU);
            let controller = HWND(CONTROLLER.load(Ordering::SeqCst) as *mut _);
            let is_down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let action = key_action(ours, is_down, msg == WM_KEYUP, kb.vkCode, POPUP_VISIBLE.load(Ordering::SeqCst), plain, SWALLOWED_TAB.load(Ordering::SeqCst));
            match action {
                KeyAction::Accept => {
                    SWALLOWED_TAB.store(true, Ordering::SeqCst);
                    let _ = PostMessageW(Some(controller), WM_ACCEPT, WPARAM(0), LPARAM(0));
                    return LRESULT(1);
                }
                KeyAction::Dismiss => {
                    let _ = PostMessageW(Some(controller), WM_DISMISS, WPARAM(0), LPARAM(0));
                    return LRESULT(1);
                }
                KeyAction::SwallowTabRelease => {
                    SWALLOWED_TAB.store(false, Ordering::SeqCst);
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
    /// The release of a Tab we kept: keep that too, so the app never sees half a key press.
    SwallowTabRelease,
}

/// The whole keyboard rule in one place (so it can be tested without a keyboard).
/// Only plain Tab and Esc, only while the popup is open, and never the keys Verba sent itself.
fn key_action(ours: bool, is_down: bool, is_up: bool, vk: u32, popup_visible: bool, plain: bool, tab_was_kept: bool) -> KeyAction {
    if ours {
        return KeyAction::Pass;
    }
    if is_down && popup_visible && plain {
        if vk == VK_TAB.0 as u32 {
            return KeyAction::Accept;
        }
        if vk == VK_ESCAPE.0 as u32 {
            return KeyAction::Dismiss;
        }
    }
    if is_up && vk == VK_TAB.0 as u32 && tab_was_kept {
        return KeyAction::SwallowTabRelease;
    }
    KeyAction::Pass
}

/// What must be typed to put a suggestion in place of the part of the word already typed.
#[derive(Debug, PartialEq, Eq)]
enum Typed {
    Backspace,
    Unit(u16),
}

/// `typed_len` backspaces, then the suggestion and a space (as UTF-16 units, which is what Windows types).
fn accept_keys(typed_len: usize, suggestion: &str) -> Vec<Typed> {
    let mut keys: Vec<Typed> = (0..typed_len).map(|_| Typed::Backspace).collect();
    keys.extend(format!("{suggestion} ").encode_utf16().map(Typed::Unit));
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAB: u32 = 0x09;
    const ESC: u32 = 0x1B;

    #[test]
    fn tab_accepts_only_when_the_popup_is_open_and_no_modifier_is_held() {
        assert_eq!(key_action(false, true, false, TAB, true, true, false), KeyAction::Accept);
        assert_eq!(key_action(false, true, false, TAB, false, true, false), KeyAction::Pass, "popup closed: Tab is the app's");
        assert_eq!(key_action(false, true, false, TAB, true, false, false), KeyAction::Pass, "Shift/Ctrl/Alt+Tab is the app's");
    }

    #[test]
    fn esc_closes_the_popup_only_when_it_is_open() {
        assert_eq!(key_action(false, true, false, ESC, true, true, false), KeyAction::Dismiss);
        assert_eq!(key_action(false, true, false, ESC, false, true, false), KeyAction::Pass);
    }

    #[test]
    fn a_kept_tab_keeps_its_release_and_nothing_else_is_touched() {
        assert_eq!(key_action(false, false, true, TAB, false, true, true), KeyAction::SwallowTabRelease);
        assert_eq!(key_action(false, false, true, TAB, false, true, false), KeyAction::Pass, "no Tab was kept");
        assert_eq!(key_action(false, true, false, 0x41, true, true, false), KeyAction::Pass, "letters are never touched");
        assert_eq!(key_action(false, false, true, ESC, true, true, true), KeyAction::Pass);
    }

    #[test]
    fn keys_sent_by_verba_itself_pass_through() {
        assert_eq!(key_action(true, true, false, TAB, true, true, false), KeyAction::Pass);
    }

    #[test]
    fn accepting_replaces_the_typed_part_and_adds_a_space() {
        let keys = accept_keys(3, "like");
        assert_eq!(&keys[..3], [Typed::Backspace, Typed::Backspace, Typed::Backspace]);
        let units: Vec<u16> = keys[3..].iter().map(|k| match k { Typed::Unit(u) => *u, Typed::Backspace => panic!("late backspace") }).collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "like ");
    }

    #[test]
    fn arabic_suggestions_type_as_their_own_letters() {
        let keys = accept_keys(2, "مرحبا");
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
        "Verba: paused (Ctrl+Alt+Shift+V to resume)"
    } else {
        "Verba: on (Ctrl+Alt+Shift+Q to quit)"
    }
}

fn add_tray(hwnd: HWND) {
    with_app(|app| unsafe {
        let nid = tray_data(hwnd, app.icon, tip_for(app.settings.paused));
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    });
}

fn modify_tray(hwnd: HWND, app: &App) {
    unsafe {
        let nid = tray_data(hwnd, app.icon, tip_for(app.settings.paused));
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
    let Some(settings) = APP.with(|a| a.try_borrow().ok().and_then(|b| b.as_ref().map(|app| app.settings.clone()))) else { return };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let check = |on: bool| if on { MF_STRING | MF_CHECKED } else { MF_STRING };

        let _ = AppendMenuW(menu, check(!settings.paused), CMD_PAUSE, w!("Suggestions on\tCtrl+Alt+Shift+V"));

        if let Ok(langs) = CreatePopupMenu() {
            let _ = AppendMenuW(langs, check(settings.english), CMD_LANG_EN, w!("English"));
            let _ = AppendMenuW(langs, check(settings.french), CMD_LANG_FR, w!("Français"));
            let _ = AppendMenuW(langs, check(settings.arabic), CMD_LANG_AR, w!("العربية  (Arabic)"));
            let _ = AppendMenuW(menu, MF_POPUP, langs.0 as usize, w!("Languages"));
        }
        if let Ok(theme) = CreatePopupMenu() {
            let _ = AppendMenuW(theme, check(settings.theme == ThemeMode::Auto), CMD_THEME_AUTO, w!("Follow Windows"));
            let _ = AppendMenuW(theme, check(settings.theme == ThemeMode::Light), CMD_THEME_LIGHT, w!("Light"));
            let _ = AppendMenuW(theme, check(settings.theme == ThemeMode::Dark), CMD_THEME_DARK, w!("Dark"));
            let _ = AppendMenuW(menu, MF_POPUP, theme.0 as usize, w!("Theme"));
        }
        // starting with Windows points at this program's file: only for the real (release) build, not a test build
        let auto_flags = if cfg!(debug_assertions) { MF_STRING | MF_GRAYED } else { check(autostart_enabled()) };
        let _ = AppendMenuW(menu, auto_flags, CMD_AUTOSTART, w!("Start with Windows"));

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
fn make_icon() -> HICON {
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
            let (r, g, b) = (0.0 + 255.0 * white, 120.0 + 135.0 * white, 215.0 + 40.0 * white);
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
