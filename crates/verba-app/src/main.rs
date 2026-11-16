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
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};

use verba_core::{Engine, Suggestion, current_word};
use verba_local::LocalEngine;
use verba_win::{Locator, Overlay, Theme};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
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
    AppendMenuW, CallNextHookEx, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon,
    DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, HHOOK, HICON, ICONINFO, KBDLLHOOKSTRUCT, KillTimer,
    MF_CHECKED, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, SetTimer, SetWindowsHookExW, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_COMMAND, WM_DESTROY, WM_HOTKEY, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONUP,
    WM_NULL, WM_RBUTTONUP, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW, WS_POPUP,
};
use windows::core::{PCWSTR, w};

const WM_TRAY: u32 = WM_APP + 1;
const WM_ACCEPT: u32 = WM_APP + 2;
const WM_DISMISS: u32 = WM_APP + 3;
const TIMER_TICK: usize = 1;
const TICK_MS: u32 = 150;
const HOTKEY_QUIT: i32 = 1;
const HOTKEY_PAUSE: i32 = 2;
const CMD_PAUSE: usize = 1001;
const CMD_QUIT: usize = 1002;
const TRAY_ID: u32 = 1;
const LLKHF_INJECTED: u32 = 0x10;

// what the keyboard hook needs to know without touching the rest of the program
static POPUP_VISIBLE: AtomicBool = AtomicBool::new(false);
static SWALLOWED_TAB: AtomicBool = AtomicBool::new(false);
static CONTROLLER: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

struct App {
    locator: Locator,
    overlay: Overlay,
    engine: LocalEngine,
    paused: bool,
    icon: HICON,
    /// What the popup shows now.
    shown: Vec<Suggestion>,
    /// How many characters of the word are typed already (they are replaced when a suggestion is accepted).
    typed_len: usize,
    /// The text for which the user pressed Esc: stay closed until it changes.
    dismissed: Option<String>,
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

        let engine = load_engine();
        let locator = Locator::new().expect("UI Automation is not available");
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
            *a.borrow_mut() = Some(App { locator, overlay, engine, paused: false, icon, shown: Vec::new(), typed_len: 0, dismissed: None });
        });

        let mods = MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT;
        if RegisterHotKey(Some(hwnd), HOTKEY_QUIT, mods, 'Q' as u32).is_err() {
            log("could not register Ctrl+Alt+Shift+Q (another program uses it); use the tray menu to quit");
        }
        let _ = RegisterHotKey(Some(hwnd), HOTKEY_PAUSE, mods, 'V' as u32);

        add_tray(hwnd);
        SetTimer(Some(hwnd), TIMER_TICK, TICK_MS, None);
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

fn system_theme() -> Theme {
    unsafe {
        let mut light = 1u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let r = RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut light as *mut u32 as *mut _),
            Some(&mut size),
        );
        if r.is_ok() && light == 0 { Theme::Dark } else { Theme::Light }
    }
}

// ───────────── the controller window ─────────────

unsafe extern "system" fn controller_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER if wp.0 == TIMER_TICK => {
            with_app(tick);
            LRESULT(0)
        }
        WM_HOTKEY => {
            match wp.0 as i32 {
                HOTKEY_QUIT => unsafe { PostQuitMessage(0) },
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
            match wp.0 & 0xFFFF {
                CMD_PAUSE => with_app(|app| toggle_pause(app, hwnd)),
                CMD_QUIT => unsafe { PostQuitMessage(0) },
                _ => {}
            }
            LRESULT(0)
        }
        WM_ACCEPT => {
            with_app(accept);
            LRESULT(0)
        }
        WM_DISMISS => {
            with_app(|app| {
                app.dismissed = Some(String::new());
                hide(app);
            });
            LRESULT(0)
        }
        WM_DESTROY => {
            remove_tray(hwnd);
            unsafe { PostQuitMessage(0) };
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

/// Every tick: read the few letters before the caret, ask the engine, show or hide the popup.
fn tick(app: &mut App) {
    if app.paused {
        if POPUP_VISIBLE.load(Ordering::SeqCst) {
            hide(app);
        }
        return;
    }
    let Some(snap) = app.locator.snapshot(48) else {
        if POPUP_VISIBLE.load(Ordering::SeqCst) {
            hide(app);
        }
        return;
    };
    match &app.dismissed {
        Some(d) if d.is_empty() => app.dismissed = Some(snap.before_caret.clone()), // Esc: remember what was there
        Some(d) if *d == snap.before_caret => return,                                 // unchanged: stay closed
        Some(_) => app.dismissed = None,                                              // the user typed on: allowed again
        None => {}
    }
    let found = app.engine.suggest(&snap.before_caret, 3);
    if found.is_empty() {
        if POPUP_VISIBLE.load(Ordering::SeqCst) {
            hide(app);
        }
        return;
    }
    app.typed_len = current_word(&snap.before_caret).chars().count();
    app.overlay.show(snap.anchor, &found, 0, system_theme());
    app.shown = found;
    POPUP_VISIBLE.store(true, Ordering::SeqCst);
}

/// Tab: replace the typed part of the word with the first suggestion, plus a space.
fn accept(app: &mut App) {
    let Some(first) = app.shown.first().cloned() else { return };
    hide(app);
    unsafe {
        let mut inputs: Vec<INPUT> = Vec::new();
        for _ in 0..app.typed_len {
            inputs.push(key(VK_BACK, 0, KEYBD_EVENT_FLAGS(0)));
            inputs.push(key(VK_BACK, 0, KEYEVENTF_KEYUP));
        }
        for unit in format!("{} ", first.text).encode_utf16() {
            inputs.push(key(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE));
            inputs.push(key(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
        }
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
    // the next tick sees a fresh word: nothing to remember
    app.dismissed = None;
}

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn toggle_pause(app: &mut App, hwnd: HWND) {
    app.paused = !app.paused;
    if app.paused {
        hide(app);
    }
    modify_tray(hwnd, app);
}

// ───────────── the keyboard hook ─────────────

/// Looks at one thing only: Tab or Esc while the popup is open. Everything else passes untouched.
unsafe extern "system" fn keyboard_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if code == 0 {
            let kb = &*(lp.0 as *const KBDLLHOOKSTRUCT);
            let ours = kb.flags.0 & LLKHF_INJECTED != 0; // keys Verba itself sent
            let msg = wp.0 as u32;
            let ctrl_down = |vk: VIRTUAL_KEY| GetKeyState(vk.0 as i32) < 0;
            let plain = !ctrl_down(VK_SHIFT) && !ctrl_down(VK_CONTROL) && !ctrl_down(VK_MENU);
            if !ours {
                let controller = HWND(CONTROLLER.load(Ordering::SeqCst) as *mut _);
                if (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN) && POPUP_VISIBLE.load(Ordering::SeqCst) && plain {
                    if kb.vkCode == VK_TAB.0 as u32 {
                        SWALLOWED_TAB.store(true, Ordering::SeqCst);
                        let _ = PostMessageW(Some(controller), WM_ACCEPT, WPARAM(0), LPARAM(0));
                        return LRESULT(1);
                    }
                    if kb.vkCode == VK_ESCAPE.0 as u32 {
                        let _ = PostMessageW(Some(controller), WM_DISMISS, WPARAM(0), LPARAM(0));
                        return LRESULT(1);
                    }
                }
                // the Tab we kept from the app: keep its release too
                if msg == WM_KEYUP && kb.vkCode == VK_TAB.0 as u32 && SWALLOWED_TAB.swap(false, Ordering::SeqCst) {
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(None::<HHOOK>, code, wp, lp)
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
        let nid = tray_data(hwnd, app.icon, tip_for(app.paused));
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    });
}

fn modify_tray(hwnd: HWND, app: &App) {
    unsafe {
        let nid = tray_data(hwnd, app.icon, tip_for(app.paused));
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
    let paused = APP.with(|a| a.try_borrow().ok().and_then(|b| b.as_ref().map(|app| app.paused)).unwrap_or(false));
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let on_flags = if paused { MF_STRING } else { MF_STRING | MF_CHECKED };
        let _ = AppendMenuW(menu, on_flags, CMD_PAUSE, w!("Suggestions on\tCtrl+Alt+Shift+V"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("Quit Verba\tCtrl+Alt+Shift+Q"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // the menu closes when you click elsewhere only if its window is in front
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
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

#[allow(dead_code)]
fn destroy_icon(icon: HICON) {
    unsafe {
        let _ = DestroyIcon(icon);
        let _ = KillTimer(None, TIMER_TICK);
    }
}
