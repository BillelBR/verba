//! The settings window: a plain Windows window with the usual controls. Every change applies at once (no Apply button).
//!
//! It holds no state of its own: it shows the program's settings and sends each change back through `crate::update_settings`.

use std::cell::Cell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering};

use verba_core::Lang;
use verba_win::running_apps;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, CreateFontIndirectW, DeleteObject, GetSysColor, GetSysColorBrush, HBRUSH, HDC, HFONT, SetBkColor};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, SystemParametersInfoForDpi};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GetSystemMetrics, GetWindowRect,
    HICON, HMENU, IsWindow, MF_GRAYED, MF_STRING, NONCLIENTMETRICSW, RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SPI_GETNONCLIENTMETRICS,
    SW_SHOW, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SendMessageW, SetForegroundWindow, SetWindowPos, ShowWindow, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_TOPALIGN, TrackPopupMenu, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_MINIMIZEBOX, WS_OVERLAPPED,
    WS_SYSMENU, WS_VISIBLE, WM_CTLCOLORBTN, WM_CTLCOLORSTATIC,
};
use windows::core::{PCWSTR, w};

use crate::settings::{ALWAYS_OFF, AppMode, Settings, ThemeMode};

/// The window being shown (0 when there is none): the message loop needs it for Tab and arrow keys.
pub static WINDOW: AtomicIsize = AtomicIsize::new(0);

// control numbers
const ID_ON: i32 = 2001;
const ID_FOLLOW: i32 = 2010;
const ID_EN: i32 = 2011;
const ID_FR: i32 = 2012;
const ID_AR: i32 = 2013;
const ID_THEME_AUTO: i32 = 2021;
const ID_THEME_LIGHT: i32 = 2022;
const ID_THEME_DARK: i32 = 2023;
const ID_AUTOSTART: i32 = 2031;
const ID_MODE: i32 = 2040;
const ID_LIST: i32 = 2041;
const ID_ADD: i32 = 2042;
const ID_REMOVE: i32 = 2043;
const ID_CLOSE: i32 = 2099;
const MENU_APP_BASE: usize = 3000;

// message and style numbers (plain values: they never change)
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const LB_ADDSTRING: u32 = 0x0180;
const LB_RESETCONTENT: u32 = 0x0184;
const LB_GETCURSEL: u32 = 0x0188;
const CBN_SELCHANGE: usize = 1;
const BS_PUSHBUTTON: u32 = 0x0;
const BS_DEFPUSHBUTTON: u32 = 0x1;
const BS_AUTOCHECKBOX: u32 = 0x3;
const BS_GROUPBOX: u32 = 0x7;
const BS_AUTORADIOBUTTON: u32 = 0x9;
const CBS_DROPDOWNLIST: u32 = 0x3;
const LBS_NOTIFY: u32 = 0x1;
const WS_TABSTOP_: u32 = 0x0001_0000;
const WS_GROUP_: u32 = 0x0002_0000;
const WS_VSCROLL_: u32 = 0x0020_0000;
const WS_EX_CLIENTEDGE_: u32 = 0x0000_0200;

thread_local! {
    static FONT: Cell<isize> = const { Cell::new(0) };
    static SCALE: Cell<f32> = const { Cell::new(1.0) };
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn px(v: i32) -> i32 {
    (v as f32 * SCALE.with(|s| s.get())).round() as i32
}

/// Shows the window (or brings it to the front when it is already open).
pub fn open(icon: HICON) {
    unsafe {
        let existing = HWND(WINDOW.load(Ordering::SeqCst) as *mut c_void);
        if !existing.is_invalid() && IsWindow(Some(existing)).as_bool() {
            let _ = ShowWindow(existing, SW_SHOW);
            let _ = SetForegroundWindow(existing);
            return;
        }
        let Ok(module) = GetModuleHandleW(None) else { return };
        let hinst: HINSTANCE = module.into();
        let class = w!("VerbaSettings");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: hinst,
            lpszClassName: class,
            hIcon: icon,
            hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as usize as *mut c_void),
            ..Default::default()
        };
        RegisterClassW(&wc);

        let dpi = GetDpiForSystem().max(96);
        SCALE.with(|s| s.set(dpi as f32 / 96.0));
        let (client_w, client_h) = (px(480), px(578));
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        // the window is bigger than its inside by the title bar and the borders
        let mut frame = RECT { left: 0, top: 0, right: client_w, bottom: client_h };
        let _ = windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi(&mut frame, style, false, WINDOW_EX_STYLE(0), dpi);
        let (win_w, win_h) = (frame.right - frame.left, frame.bottom - frame.top);
        let (x, y) = ((GetSystemMetrics(SM_CXSCREEN) - win_w) / 2, (GetSystemMetrics(SM_CYSCREEN) - win_h) / 3);
        let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("Verba settings"), style, x, y, win_w, win_h, None, None, Some(hinst), None) else {
            return;
        };
        WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);
        make_font(dpi);
        build_controls(hwnd, hinst);
        refresh(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOW);
        // the very first ShowWindow of a program is overridden by how the program was started (hidden, minimised...);
        // this one is not, so the window always appears
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_SHOWWINDOW);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// The font the system uses for its own windows (Segoe UI), at this screen's scale.
fn make_font(dpi: u32) {
    unsafe {
        let mut metrics = NONCLIENTMETRICSW { cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
        if SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS.0, metrics.cbSize, Some(&mut metrics as *mut _ as *mut c_void), 0, dpi).is_ok() {
            let font = CreateFontIndirectW(&metrics.lfMessageFont);
            FONT.with(|f| f.set(font.0 as isize));
        }
    }
}

fn child(parent: HWND, class: PCWSTR, text: &str, style: u32, ex: u32, (x, y, w, h): (i32, i32, i32, i32), id: i32) -> HWND {
    unsafe {
        let text = wide(text);
        let hinst: HINSTANCE = GetModuleHandleW(None).map(Into::into).unwrap_or_default();
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(ex),
            class,
            PCWSTR(text.as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | style),
            px(x),
            px(y),
            px(w),
            px(h),
            Some(parent),
            Some(HMENU(id as usize as *mut c_void)),
            Some(hinst),
            None,
        )
        .unwrap_or_default();
        let font = FONT.with(|f| f.get());
        if font != 0 {
            SendMessageW(hwnd, WM_SETFONT, Some(WPARAM(font as usize)), Some(LPARAM(1)));
        }
        hwnd
    }
}

fn build_controls(win: HWND, _hinst: HINSTANCE) {
    let button = w!("BUTTON");
    let label = w!("STATIC");
    let tab = WS_TABSTOP_;
    // Suggestions on
    child(win, button, "Suggestions on", BS_AUTOCHECKBOX | tab, 0, (20, 14, 440, 24), ID_ON);

    // Language
    child(win, button, "Language", BS_GROUPBOX, 0, (20, 48, 440, 126), 0);
    child(win, button, "Follow the keyboard language", BS_AUTOCHECKBOX | tab, 0, (36, 70, 408, 22), ID_FOLLOW);
    child(win, button, "English", BS_AUTOCHECKBOX | tab, 0, (36, 96, 124, 22), ID_EN);
    child(win, button, "Français", BS_AUTOCHECKBOX | tab, 0, (166, 96, 124, 22), ID_FR);
    child(win, button, "العربية", BS_AUTOCHECKBOX | tab, 0, (296, 96, 148, 22), ID_AR);
    child(win, label, "Verba suggests in the language of the keyboard you are typing with. Turn this off to guess it from the letters instead.", 0, 0, (36, 122, 408, 44), 0);

    // Look
    child(win, button, "Look", BS_GROUPBOX, 0, (20, 184, 440, 58), 0);
    child(win, button, "Follow Windows", BS_AUTORADIOBUTTON | WS_GROUP_ | tab, 0, (36, 208, 140, 22), ID_THEME_AUTO);
    child(win, button, "Light", BS_AUTORADIOBUTTON, 0, (190, 208, 100, 22), ID_THEME_LIGHT);
    child(win, button, "Dark", BS_AUTORADIOBUTTON, 0, (300, 208, 100, 22), ID_THEME_DARK);

    child(win, button, "Start Verba when Windows starts", BS_AUTOCHECKBOX | tab, 0, (20, 254, 440, 24), ID_AUTOSTART);

    // Apps
    child(win, button, "Apps", BS_GROUPBOX, 0, (20, 290, 440, 238), 0);
    let combo = child(win, w!("COMBOBOX"), "", CBS_DROPDOWNLIST | tab | WS_VSCROLL_, 0, (36, 314, 408, 120), ID_MODE);
    unsafe {
        for item in ["Work in every app except the ones listed", "Work only in the apps listed"] {
            let text = wide(item);
            SendMessageW(combo, CB_ADDSTRING, Some(WPARAM(0)), Some(LPARAM(text.as_ptr() as isize)));
        }
    }
    child(win, w!("LISTBOX"), "", LBS_NOTIFY | WS_VSCROLL_ | tab, WS_EX_CLIENTEDGE_, (36, 348, 262, 128), ID_LIST);
    child(win, button, "Add a running app…", BS_PUSHBUTTON | tab, 0, (308, 348, 136, 30), ID_ADD);
    child(win, button, "Remove", BS_PUSHBUTTON | tab, 0, (308, 386, 136, 30), ID_REMOVE);
    child(win, label, "Password managers and Verba itself are always off, whatever you choose.", 0, 0, (36, 484, 408, 36), 0);

    child(win, button, "Close", BS_DEFPUSHBUTTON | tab, 0, (360, 540, 100, 28), ID_CLOSE);
}

fn control(win: HWND, id: i32) -> HWND {
    unsafe { windows::Win32::UI::WindowsAndMessaging::GetDlgItem(Some(win), id).unwrap_or_default() }
}

fn set_check(win: HWND, id: i32, on: bool) {
    unsafe {
        SendMessageW(control(win, id), BM_SETCHECK, Some(WPARAM(on as usize)), Some(LPARAM(0)));
    }
}

fn is_checked(win: HWND, id: i32) -> bool {
    unsafe { SendMessageW(control(win, id), BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))).0 == 1 }
}

/// Shows the program's current settings in the controls.
fn refresh(win: HWND) {
    let s = crate::current_settings();
    set_check(win, ID_ON, !s.paused);
    set_check(win, ID_FOLLOW, s.follow_keyboard);
    set_check(win, ID_EN, s.english);
    set_check(win, ID_FR, s.french);
    set_check(win, ID_AR, s.arabic);
    set_check(win, ID_THEME_AUTO, s.theme == ThemeMode::Auto);
    set_check(win, ID_THEME_LIGHT, s.theme == ThemeMode::Light);
    set_check(win, ID_THEME_DARK, s.theme == ThemeMode::Dark);
    set_check(win, ID_AUTOSTART, crate::autostart_enabled());
    unsafe {
        // starting with Windows points at this program's file: only the real (release) build offers it
        let _ = windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow(control(win, ID_AUTOSTART), !cfg!(debug_assertions));
        let mode = if s.app_mode == AppMode::OnlyThese { 1 } else { 0 };
        SendMessageW(control(win, ID_MODE), CB_SETCURSEL, Some(WPARAM(mode)), Some(LPARAM(0)));
        let list = control(win, ID_LIST);
        SendMessageW(list, LB_RESETCONTENT, Some(WPARAM(0)), Some(LPARAM(0)));
        for app in &s.apps {
            let text = wide(app);
            SendMessageW(list, LB_ADDSTRING, Some(WPARAM(0)), Some(LPARAM(text.as_ptr() as isize)));
        }
    }
}

/// The running programs in a pop-up list under the button; returns the one chosen.
fn choose_running_app(win: HWND, settings: &Settings) -> Option<String> {
    unsafe {
        let apps = running_apps();
        let menu = CreatePopupMenu().ok()?;
        for (i, app) in apps.iter().enumerate() {
            let listed = settings.apps.contains(app);
            let always_off = ALWAYS_OFF.contains(&app.as_str());
            let flags = if listed || always_off { MF_STRING | MF_GRAYED } else { MF_STRING };
            let text = wide(app);
            let _ = AppendMenuW(menu, flags, MENU_APP_BASE + i, PCWSTR(text.as_ptr()));
        }
        if apps.is_empty() {
            let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, MENU_APP_BASE, w!("(no app with a window)"));
        }
        let mut r = RECT::default();
        let _ = GetWindowRect(control(win, ID_ADD), &mut r);
        let pt = POINT { x: r.left, y: r.bottom };
        let picked = TrackPopupMenu(menu, TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RETURNCMD, pt.x, pt.y, None, win, None);
        let _ = DestroyMenu(menu);
        let index = (picked.0 as usize).checked_sub(MENU_APP_BASE)?;
        apps.get(index).cloned()
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let id = (wp.0 & 0xFFFF) as i32;
            let note = (wp.0 >> 16) & 0xFFFF;
            match id {
                ID_ON => crate::update_settings(|s| s.paused = !is_checked(hwnd, ID_ON)),
                ID_FOLLOW => crate::update_settings(|s| s.follow_keyboard = is_checked(hwnd, ID_FOLLOW)),
                ID_EN => crate::update_settings(|s| s.set_lang(Lang::En, is_checked(hwnd, ID_EN))),
                ID_FR => crate::update_settings(|s| s.set_lang(Lang::Fr, is_checked(hwnd, ID_FR))),
                ID_AR => crate::update_settings(|s| s.set_lang(Lang::Ar, is_checked(hwnd, ID_AR))),
                ID_THEME_AUTO => crate::update_settings(|s| s.theme = ThemeMode::Auto),
                ID_THEME_LIGHT => crate::update_settings(|s| s.theme = ThemeMode::Light),
                ID_THEME_DARK => crate::update_settings(|s| s.theme = ThemeMode::Dark),
                ID_AUTOSTART => crate::set_autostart(is_checked(hwnd, ID_AUTOSTART)),
                ID_MODE if note == CBN_SELCHANGE => {
                    let chosen = unsafe { SendMessageW(control(hwnd, ID_MODE), CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0 };
                    // switching the meaning of the list starts a new list: the old one meant the opposite
                    crate::update_settings(|s| {
                        let mode = if chosen == 1 { AppMode::OnlyThese } else { AppMode::AllExcept };
                        if s.app_mode != mode {
                            s.app_mode = mode;
                            s.apps.clear();
                        }
                    });
                    refresh(hwnd);
                }
                ID_ADD => {
                    let settings = crate::current_settings();
                    if let Some(app) = choose_running_app(hwnd, &settings) {
                        // "all except": the app goes on the list of the ones that are off; "only these": on the list of the ones that are on
                        crate::update_settings(|s| s.set_app(&app, s.app_mode == AppMode::OnlyThese));
                        refresh(hwnd);
                    }
                }
                ID_REMOVE => {
                    let at = unsafe { SendMessageW(control(hwnd, ID_LIST), LB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0 };
                    let settings = crate::current_settings();
                    if let Some(app) = usize::try_from(at).ok().and_then(|i| settings.apps.get(i)) {
                        let app = app.clone();
                        crate::update_settings(|s| s.set_app(&app, s.app_mode == AppMode::AllExcept));
                        refresh(hwnd);
                    }
                }
                ID_CLOSE => unsafe {
                    let _ = DestroyWindow(hwnd);
                },
                _ => {}
            }
            LRESULT(0)
        }
        // labels and check boxes paint their own background: make it the window's, not the grey of a dialog
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => unsafe {
            let hdc = HDC(wp.0 as *mut c_void);
            SetBkColor(hdc, COLORREF(GetSysColor(COLOR_WINDOW)));
            LRESULT(GetSysColorBrush(COLOR_WINDOW).0 as isize)
        },
        WM_CLOSE => {
            unsafe {
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            WINDOW.store(0, Ordering::SeqCst);
            FONT.with(|f| {
                let font = f.replace(0);
                if font != 0 {
                    unsafe {
                        let _ = DeleteObject(HFONT(font as *mut c_void).into());
                    }
                }
            });
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
