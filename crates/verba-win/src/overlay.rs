//! The suggestion popup: a small window that floats above the text being typed.
//!
//! It never takes the keyboard focus (so the app the user is typing in keeps it), stays on top,
//! and supports right-to-left words (Arabic) next to left-to-right ones.

use std::sync::Mutex;

use verba_core::Suggestion;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DEFAULT_CHARSET,
    DT_CENTER, DT_NOPREFIX, DT_RTLREADING, DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FillRect,
    FrameRect, GetDC, GetMonitorInfoW, InvalidateRect, SetWindowRgn, GetTextExtentPoint32W, HFONT, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromRect, OUT_DEFAULT_PRECIS, PAINTSTRUCT, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, SetProcessDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, HWND_TOPMOST, PM_REMOVE, PeekMessageW,
    RegisterClassW, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SetWindowPos, ShowWindow, TranslateMessage,
    WM_ERASEBKGND, WM_MOUSEACTIVATE, WM_PAINT, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, MSG,
};
use windows::core::w;

use crate::focus::Anchor;

/// Light or dark, to match the app the user is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

struct Colors {
    bg: COLORREF,
    border: COLORREF,
    text: COLORREF,
    highlight: COLORREF,
    highlight_text: COLORREF,
}

impl Theme {
    fn colors(self) -> Colors {
        // COLORREF is 0x00BBGGRR
        match self {
            Theme::Light => Colors {
                bg: COLORREF(0x00F9F9F9),
                border: COLORREF(0x00CFCFCF),
                text: COLORREF(0x00202020),
                highlight: COLORREF(0x00D77800), // Windows blue
                highlight_text: COLORREF(0x00FFFFFF),
            },
            Theme::Dark => Colors {
                bg: COLORREF(0x002B2B2B),
                border: COLORREF(0x00505050),
                text: COLORREF(0x00F0F0F0),
                highlight: COLORREF(0x00D77800),
                highlight_text: COLORREF(0x00FFFFFF),
            },
        }
    }
}

struct Item {
    text: Vec<u16>,
    rtl: bool,
    rect: RECT,
}

struct State {
    items: Vec<Item>,
    selected: usize,
    width: i32,
    height: i32,
    px: i32,
    theme: Theme,
}

// the window procedure is called by Windows, so the state it draws from lives here
static STATE: Mutex<Option<State>> = Mutex::new(None);

pub struct Overlay {
    hwnd: HWND,
    px: i32,
}

impl Overlay {
    /// Makes the (hidden) popup window. Call once, on the thread that will pump messages.
    pub fn new() -> windows::core::Result<Self> {
        unsafe {
            // the screen positions we get from other apps are real pixels, so be DPI-aware too
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let hinst = GetModuleHandleW(None)?;
            let class = w!("VerbaSuggestionPopup");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinst.into(),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                class,
                w!("Verba"),
                WS_POPUP,
                0,
                0,
                10,
                10,
                None,
                None,
                Some(hinst.into()),
                None,
            )?;
            let dpi = GetDpiForSystem().max(96) as i32;
            Ok(Self { hwnd, px: 18 * dpi / 96 })
        }
    }

    /// Shows the candidates just above `anchor` (or below it when there is no room above).
    /// `selected` is the one Tab will accept.
    pub fn show(&self, anchor: Anchor, suggestions: &[Suggestion], selected: usize, theme: Theme) {
        if suggestions.is_empty() {
            self.hide();
            return;
        }
        unsafe {
            let pad_x = self.px * 7 / 10;
            let pad_y = self.px / 3;
            let margin = self.px / 5;

            // measure each word with the same font the painting uses
            let hdc = GetDC(None);
            let font = make_font(self.px);
            let old = SelectObject(hdc, font.into());
            let mut items = Vec::new();
            let mut line_h = 0;
            for s in suggestions {
                let text: Vec<u16> = s.text.encode_utf16().collect();
                let mut size = Default::default();
                let _ = GetTextExtentPoint32W(hdc, &text, &mut size);
                line_h = line_h.max(size.cy);
                let w = size.cx + 2 * pad_x;
                // the place is set below, once the total width is known
                items.push(Item { text, rtl: s.lang.is_rtl(), rect: RECT { left: 0, top: margin, right: w, bottom: margin } });
            }
            SelectObject(hdc, old);
            let _ = DeleteObject(font.into());
            ReleaseDC(None, hdc);

            let item_h = line_h + 2 * pad_y;
            let width = items.iter().map(|i| i.rect.right).sum::<i32>() + 2 * margin;
            let height = item_h + 2 * margin;

            // Arabic reads from the right: the first (best) candidate is the rightmost one
            let rtl_layout = suggestions[0].lang.is_rtl();
            let mut x = if rtl_layout { width - margin } else { margin };
            for it in &mut items {
                let w = it.rect.right;
                it.rect.bottom = it.rect.top + item_h;
                if rtl_layout {
                    it.rect.right = x;
                    it.rect.left = x - w;
                    x -= w;
                } else {
                    it.rect.left = x;
                    it.rect.right = x + w;
                    x += w;
                }
            }

            // above the anchor; below it when the top of the screen is in the way; never off the screen
            let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let mon = MonitorFromRect(&anchor.rect, MONITOR_DEFAULTTONEAREST);
            let work = if GetMonitorInfoW(mon, &mut mi).as_bool() { mi.rcWork } else { anchor.rect };
            let gap = self.px / 4;
            let mut y = anchor.rect.top - height - gap;
            if y < work.top {
                y = anchor.rect.bottom + gap;
            }
            // an Arabic popup hangs from the caret's right side; with only the field known, it starts at the field's left
            let mut left = if rtl_layout && anchor.kind == crate::focus::AnchorKind::Caret { anchor.rect.right - width } else { anchor.rect.left };
            if left + width > work.right {
                left = work.right - width;
            }
            left = left.max(work.left);

            *STATE.lock().unwrap() =
                Some(State { items, selected: selected.min(suggestions.len() - 1), width, height, px: self.px, theme });

            let radius = self.px / 2;
            let rgn = CreateRoundRectRgn(0, 0, width + 1, height + 1, radius, radius);
            SetWindowRgn(self.hwnd, Some(rgn), true);
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), left, y, width, height, SWP_NOACTIVATE | SWP_SHOWWINDOW);
            let _ = InvalidateRect(Some(self.hwnd), None, true);
        }
    }

    /// Hides the popup (it stays ready to show again).
    pub fn hide(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// Where the popup is on the screen right now.
    pub fn rect(&self) -> RECT {
        let mut r = RECT::default();
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd, &mut r);
        }
        r
    }

    /// Lets Windows deliver the popup's paint messages. Call it regularly from the main loop.
    pub fn pump(&self) {
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn make_font(px: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -px,
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI"),
        )
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // clicking the popup must not take the focus from the app being typed in
        WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        if let Some(st) = STATE.lock().unwrap().as_ref() {
            let c = st.theme.colors();
            let full = RECT { left: 0, top: 0, right: st.width, bottom: st.height };

            let bg = CreateSolidBrush(c.bg);
            FillRect(hdc, &full, bg);
            let _ = DeleteObject(bg.into());
            let line = CreateSolidBrush(c.border);
            FrameRect(hdc, &full, line);
            let _ = DeleteObject(line.into());

            let font = make_font(st.px);
            let old = SelectObject(hdc, font.into());
            SetBkMode(hdc, TRANSPARENT);
            for (i, it) in st.items.iter().enumerate() {
                let mut r = it.rect;
                if i == st.selected {
                    let hl = CreateSolidBrush(c.highlight);
                    FillRect(hdc, &r, hl);
                    let _ = DeleteObject(hl.into());
                    SetTextColor(hdc, c.highlight_text);
                } else {
                    SetTextColor(hdc, c.text);
                }
                let mut text = it.text.clone();
                let mut fmt = DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX;
                if it.rtl {
                    fmt |= DT_RTLREADING;
                }
                DrawTextW(hdc, &mut text, &mut r, fmt);
            }
            SelectObject(hdc, old);
            let _ = DeleteObject(font.into());
        }
        let _ = EndPaint(hwnd, &ps);
    }
}
