//! A small form made of native Windows controls: a title, an optional drop-down choice, one or more text boxes, "Add" and "Cancel".
//!
//! The settings window is drawn by `ui.rs` and has no text box; this is what it opens when the person has to type something (a word to
//! add, a translation to write). The form is modal for its owner: the owner is disabled while the form is open, and `ask` returns when the
//! form is closed. It runs its own message loop, so call it from `Ui::defer` (after the frame), like the menus.
//!
//! The form does not know what the text means: the caller gives `accept`, which gets the choice and the texts when the person presses the
//! button and answers `Ok` (the form closes) or `Err(sentence)` (the sentence is shown in red and the form stays open, with the text kept).

use std::cell::RefCell;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CreateSolidBrush, DeleteObject, FillRect, GetStockObject, HBRUSH, HDC, HFONT, HGDIOBJ, SetBkColor, SetTextColor, DEFAULT_GUI_FONT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWL_EXSTYLE,
    GetClientRect, GetMessageW, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsDialogMessageW,
    LoadCursorW, MSG, PostQuitMessage, RegisterClassW, SW_SHOW, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE,
    WM_COMMAND, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_ERASEBKGND, WM_QUIT, WM_SETFONT, WNDCLASSW, WS_CAPTION, WS_CHILD,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};
use windows::core::{PCWSTR, w};

use crate::look::Theme;

/// A text box of the form.
pub struct Field<'a> {
    pub label: &'a str,
    /// Writes right to left when this answers true for the chosen option of the drop-down (an Arabic word).
    pub rtl: Option<fn(usize) -> bool>,
}

/// What the form asks.
pub struct Form<'a> {
    pub title: &'a str,
    /// One or two lines of explanation above the controls (may be empty).
    pub intro: &'a str,
    /// A drop-down: its label and its options. The chosen place is given to `accept`.
    pub choice: Option<(&'a str, &'a [&'a str])>,
    pub fields: &'a [Field<'a>],
    /// The text each box starts with (may be shorter than `fields`: the other boxes start empty).
    pub initial: &'a [&'a str],
    pub ok_label: &'a str,
    /// `false` shows the form without taking the keyboard focus (to look at it from a test).
    pub activate: bool,
}

const ID_OK: i32 = 1;
const ID_CANCEL: i32 = 2;
const ID_COMBO: i32 = 100;
const ID_EDIT: i32 = 200;
const WIDTH: i32 = 440;
const MARGIN: i32 = 20;
const EX_RIGHT: isize = 0x0000_1000;
const EX_RTLREADING: isize = 0x0000_2000;
const EX_LEFTSCROLLBAR: isize = 0x0000_4000;
const ES_AUTOHSCROLL: u32 = 0x0080;
const BS_DEFPUSHBUTTON: u32 = 0x0001;
const CBS_DROPDOWNLIST: u32 = 0x0003;
const SS_LEFT: u32 = 0x0000;
const CBN_SELCHANGE: u32 = 1;

struct State {
    combo: Option<HWND>,
    edits: Vec<HWND>,
    rtl: Vec<Option<fn(usize) -> bool>>,
    error: HWND,
    colors: Colors,
    bg: HBRUSH,
    field_bg: HBRUSH,
    /// The person pressed the button, closed the form, or nothing happened yet.
    submit: bool,
    closed: bool,
}

#[derive(Clone, Copy)]
struct Colors {
    window: COLORREF,
    text: COLORREF,
    field: COLORREF,
    error: COLORREF,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Opens the form and returns when it is closed: `true` when `accept` said `Ok`, `false` when the person cancelled.
pub fn ask(owner: Option<HWND>, theme: Theme, form: &Form, accept: &mut dyn FnMut(usize, &[String]) -> Result<(), String>) -> bool {
    unsafe {
        let dark = theme == Theme::Dark;
        let (bg_c, colors) = if dark {
            (rgb(0x20, 0x20, 0x20), Colors { window: rgb(0x20, 0x20, 0x20), text: rgb(0xF0, 0xF0, 0xF0), field: rgb(0x2D, 0x2D, 0x2D), error: rgb(0xFF, 0x7B, 0x72) })
        } else {
            (rgb(0xF3, 0xF3, 0xF3), Colors { window: rgb(0xF3, 0xF3, 0xF3), text: rgb(0x1A, 0x1A, 0x1A), field: rgb(0xFF, 0xFF, 0xFF), error: rgb(0xC4, 0x2B, 0x1C) })
        };
        let Ok(hinst) = GetModuleHandleW(None) else { return false };
        let class = w!("VerbaDialog");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(dialog_proc),
            hInstance: hinst.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).ok().unwrap_or_default(),
            ..Default::default()
        });

        let dpi = owner.map(|o| GetDpiForWindow(o)).filter(|d| *d > 0).unwrap_or_else(|| GetDpiForSystem().max(96)) as i32;
        let s = |v: i32| v * dpi / 96;
        let font: HFONT = CreateFontW(-s(15), 0, 0, 0, 400, 0, 0, 0, Default::default(), Default::default(), Default::default(), Default::default(), 0, w!("Segoe UI"));
        let font_obj = if font.is_invalid() { GetStockObject(DEFAULT_GUI_FONT) } else { HGDIOBJ(font.0) };

        // the height: the controls one under the other
        let inner = WIDTH - 2 * MARGIN;
        let mut y = MARGIN;
        let intro_lines = if form.intro.is_empty() { 0 } else { 2 };
        let intro_y = y;
        y += intro_lines * 21 + if intro_lines > 0 { 12 } else { 0 };
        let combo_y = y;
        if form.choice.is_some() {
            y += 20 + 4 + 26 + 12;
        }
        let mut field_y = Vec::new();
        for _ in form.fields {
            field_y.push(y);
            y += 20 + 4 + 26 + 12;
        }
        let error_y = y;
        y += 36;
        let buttons_y = y;
        y += 30 + MARGIN;
        let (client_w, client_h) = (s(WIDTH), s(y));
        let style = WS_CAPTION | WS_SYSMENU;
        let mut frame = RECT { left: 0, top: 0, right: client_w, bottom: client_h };
        let _ = AdjustWindowRectEx(&mut frame, style, false, WINDOW_EX_STYLE(0));
        let (ww, wh) = (frame.right - frame.left, frame.bottom - frame.top);
        let (mut x0, mut y0) = (200, 200);
        let mut owner_rect = RECT::default();
        if let Some(o) = owner {
            if GetWindowRect(o, &mut owner_rect).is_ok() {
                x0 = owner_rect.left + (owner_rect.right - owner_rect.left - ww) / 2;
                y0 = owner_rect.top + (owner_rect.bottom - owner_rect.top - wh) / 3;
            }
        }

        let title = wide(form.title);
        let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE(0), class, PCWSTR(title.as_ptr()), style, x0, y0, ww, wh, owner, None, Some(hinst.into()), None) else {
            let _ = DeleteObject(font_obj);
            return false;
        };
        let bg = CreateSolidBrush(bg_c);
        let field_bg = CreateSolidBrush(colors.field);

        let control = |class: PCWSTR, text: &str, style: u32, ex: isize, id: i32, x: i32, y: i32, w: i32, h: i32| -> HWND {
            let t = wide(text);
            let h = CreateWindowExW(
                WINDOW_EX_STYLE(ex as u32),
                class,
                PCWSTR(t.as_ptr()),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | style),
                s(x),
                s(y),
                s(w),
                s(h),
                Some(hwnd),
                Some(HMENU(id as usize as *mut _)),
                Some(hinst.into()),
                None,
            )
            .unwrap_or_default();
            SendMessageW(h, WM_SETFONT, Some(WPARAM(font_obj.0 as usize)), Some(LPARAM(1)));
            h
        };
        let label = |text: &str, y: i32| control(w!("STATIC"), text, SS_LEFT, 0, -1, MARGIN, y, inner, 20);

        if intro_lines > 0 {
            control(w!("STATIC"), form.intro, SS_LEFT, 0, -1, MARGIN, intro_y, inner, intro_lines * 21);
        }
        let mut combo = None;
        if let Some((text, options)) = form.choice {
            label(text, combo_y);
            let c = control(w!("COMBOBOX"), "", CBS_DROPDOWNLIST | WS_TABSTOP.0 | WS_VSCROLL.0, 0, ID_COMBO, MARGIN, combo_y + 24, inner, 200);
            for o in options {
                let t = wide(o);
                SendMessageW(c, CB_ADDSTRING, None, Some(LPARAM(t.as_ptr() as isize)));
            }
            SendMessageW(c, CB_SETCURSEL, Some(WPARAM(0)), None);
            combo = Some(c);
        }
        let mut edits = Vec::new();
        for (i, f) in form.fields.iter().enumerate() {
            label(f.label, field_y[i]);
            // a thin border (WS_EX_CLIENTEDGE) keeps the box visible on both backgrounds
            edits.push(control(w!("EDIT"), "", ES_AUTOHSCROLL | WS_TABSTOP.0, 0x200, ID_EDIT + i as i32, MARGIN, field_y[i] + 24, inner, 26));
        }
        let error = control(w!("STATIC"), "", SS_LEFT, 0, -1, MARGIN, error_y, inner, 32);
        let ok_w = 100;
        let ok = control(w!("BUTTON"), form.ok_label, BS_DEFPUSHBUTTON | WS_TABSTOP.0, 0, ID_OK, WIDTH - MARGIN - 2 * ok_w - 8, buttons_y, ok_w, 30);
        let cancel = control(w!("BUTTON"), "Cancel", WS_TABSTOP.0, 0, ID_CANCEL, WIDTH - MARGIN - ok_w, buttons_y, ok_w, 30);
        if dark {
            let explorer = w!("DarkMode_Explorer");
            let cfd = w!("DarkMode_CFD");
            let _ = windows::Win32::UI::Controls::SetWindowTheme(ok, explorer, PCWSTR::null());
            let _ = windows::Win32::UI::Controls::SetWindowTheme(cancel, explorer, PCWSTR::null());
            if let Some(c) = combo {
                let _ = windows::Win32::UI::Controls::SetWindowTheme(c, cfd, PCWSTR::null());
            }
            for e in &edits {
                let _ = windows::Win32::UI::Controls::SetWindowTheme(*e, cfd, PCWSTR::null());
            }
            let v = windows::core::BOOL(1);
            let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                hwnd,
                windows::Win32::Graphics::Dwm::DWMWA_USE_IMMERSIVE_DARK_MODE,
                &v as *const _ as *const _,
                std::mem::size_of::<windows::core::BOOL>() as u32,
            );
        }

        STATE.with(|st| {
            *st.borrow_mut() =
                Some(State { combo, edits: edits.clone(), rtl: form.fields.iter().map(|f| f.rtl).collect(), error, colors, bg, field_bg, submit: false, closed: false })
        });
        apply_direction();
        for (edit, text) in edits.iter().zip(form.initial) {
            let t = wide(text);
            let _ = SetWindowTextW(*edit, PCWSTR(t.as_ptr()));
        }

        if let Some(o) = owner {
            let _ = EnableWindow(o, false);
        }
        if form.activate {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            if let Some(first) = edits.first() {
                let _ = SetFocus(Some(*first));
            }
        } else {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }

        let mut saved = false;
        let mut msg = MSG::default();
        while !with_state(|s| s.closed).unwrap_or(true) {
            let got = GetMessageW(&mut msg, None, 0, 0);
            if got.0 <= 0 {
                if got.0 == 0 {
                    // the program is quitting: keep the quit for its own loop
                    PostQuitMessage(msg.wParam.0 as i32);
                }
                break;
            }
            if !IsDialogMessageW(hwnd, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if with_state(|s| std::mem::take(&mut s.submit)).unwrap_or(false) {
                let choice = with_state(|s| s.combo).flatten().map(|c| SendMessageW(c, CB_GETCURSEL, None, None).0.max(0) as usize).unwrap_or(0);
                let texts: Vec<String> = edits.iter().map(|e| text_of(*e)).collect();
                match accept(choice, &texts) {
                    Ok(()) => {
                        saved = true;
                        break;
                    }
                    Err(sentence) => {
                        let t = wide(&sentence);
                        let _ = SetWindowTextW(error, PCWSTR(t.as_ptr()));
                        // the box the sentence is about is not known here: the person is back in the first one
                        if let (true, Some(first)) = (form.activate, edits.first()) {
                            let _ = SetFocus(Some(*first));
                        }
                    }
                }
            }
        }
        let _ = WM_QUIT; // (the constant documents what `got == 0` means above)

        if let Some(o) = owner {
            let _ = EnableWindow(o, true);
            let _ = SetForegroundWindow(o);
        }
        let _ = DestroyWindow(hwnd);
        STATE.with(|st| *st.borrow_mut() = None);
        let _ = DeleteObject(HGDIOBJ(bg.0));
        let _ = DeleteObject(HGDIOBJ(field_bg.0));
        if !font.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
        saved
    }
}

/// The Windows dialog that asks for a file to save (`save`) or to open: its path, or `None` when the person cancelled. `filter_name` and `ext`
/// (`"Verba backup"`, `"verba-backup"`) make the file type shown; "All files" is always there.
pub fn choose_file(owner: Option<HWND>, save: bool, title: &str, default_name: &str, filter_name: &str, ext: &str) -> Option<PathBuf> {
    use windows::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    unsafe {
        let mut buf = [0u16; 1024];
        for (slot, unit) in buf.iter_mut().zip(default_name.encode_utf16().take(1000)) {
            *slot = unit;
        }
        let filter: Vec<u16> = format!("{filter_name} (*.{ext})\0*.{ext}\0All files (*.*)\0*.*\0\0").encode_utf16().collect();
        let title = wide(title);
        let ext = wide(ext);
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: owner.unwrap_or_default(),
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: windows::core::PWSTR(buf.as_mut_ptr()),
            nMaxFile: buf.len() as u32,
            lpstrTitle: PCWSTR(title.as_ptr()),
            lpstrDefExt: PCWSTR(ext.as_ptr()),
            Flags: if save { OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_EXPLORER } else { OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_EXPLORER },
            ..Default::default()
        };
        let ok = if save { GetSaveFileNameW(&mut ofn) } else { GetOpenFileNameW(&mut ofn) };
        if !ok.as_bool() {
            return None;
        }
        let len = buf.iter().position(|u| *u == 0).unwrap_or(buf.len());
        Some(PathBuf::from(std::ffi::OsString::from_wide(&buf[..len])))
    }
}

fn text_of(h: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(h).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let n = GetWindowTextW(h, &mut buf).max(0) as usize;
        String::from_utf16_lossy(&buf[..n])
    }
}

/// Puts each text box right to left or left to right, as the chosen option asks.
fn apply_direction() {
    let Some((combo, edits, rtl)) = with_state(|s| (s.combo, s.edits.clone(), s.rtl.clone())) else { return };
    let choice = combo.map(|c| unsafe { SendMessageW(c, CB_GETCURSEL, None, None).0.max(0) as usize }).unwrap_or(0);
    for (edit, rtl) in edits.iter().zip(rtl) {
        let Some(rtl) = rtl else { continue };
        unsafe {
            let ex = GetWindowLongPtrW(*edit, GWL_EXSTYLE);
            let bits = EX_RIGHT | EX_RTLREADING | EX_LEFTSCROLLBAR;
            let new = if rtl(choice) { ex | bits } else { ex & !bits };
            if new != ex {
                SetWindowLongPtrW(*edit, GWL_EXSTYLE, new);
                let _ = SetWindowPos(*edit, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
            }
        }
    }
}

unsafe extern "system" fn dialog_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let id = (wparam.0 & 0xFFFF) as i32;
                let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
                match id {
                    ID_OK => {
                        with_state(|s| s.submit = true);
                    }
                    ID_CANCEL => {
                        with_state(|s| s.closed = true);
                    }
                    ID_COMBO if code == CBN_SELCHANGE => apply_direction(),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                with_state(|s| s.closed = true);
                LRESULT(0)
            }
            WM_ERASEBKGND => {
                let Some(bg) = with_state(|s| s.bg) else { return DefWindowProcW(hwnd, msg, wparam, lparam) };
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                FillRect(HDC(wparam.0 as *mut _), &rc, bg);
                LRESULT(1)
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
                let hdc = HDC(wparam.0 as *mut _);
                let control = HWND(lparam.0 as *mut _);
                let Some((colors, bg, field_bg, error)) = with_state(|s| (s.colors, s.bg, s.field_bg, s.error)) else {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                };
                let is_field = msg != WM_CTLCOLORSTATIC;
                SetTextColor(hdc, if control == error { colors.error } else { colors.text });
                SetBkColor(hdc, if is_field { colors.field } else { colors.window });
                LRESULT((if is_field { field_bg } else { bg }).0 as isize)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}
