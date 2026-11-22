//! A small form: a title, an optional drop-down choice, one or more text boxes, "Add" and "Cancel".
//!
//! The settings window is drawn by `ui.rs` and has no text box; this is what it opens when the person has to type something (a word to
//! add, a translation to write). The form is modal for its owner: the owner is disabled while the form is open, and `ask` returns when the
//! form is closed. It runs its own message loop, so call it from `Ui::defer` (after the frame), like the menus.
//!
//! The form does not know what the text means: the caller gives `accept`, which gets the choice and the texts when the person presses the
//! button and answers `Ok` (the form closes) or `Err(sentence)` (the sentence is shown in red and the form stays open, with the text kept).
//!
//! How it looks matches the settings window: a heading, rounded boxes, a rounded accent button. The typing itself is done by real Windows
//! text boxes (so every input method and Arabic work), placed inside boxes the form draws; the buttons and the drop-down are drawn by the
//! form (anti-aliased with Direct2D, text with GDI) and open their list in a small window of their own.

use std::cell::RefCell;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE, D2D1_ROUNDED_RECT, D2D1CreateFactory, ID2D1Factory,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DT_CALCRECT, DT_CENTER, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, DeleteObject,
    DrawTextW, EndPaint, FillRect, GetDC, GetStockObject, HBRUSH, HDC, HFONT, HGDIOBJ, InvalidateRect, PAINTSTRUCT, ReleaseDC, SelectObject, SetBkColor,
    SetBkMode, SetTextColor, DEFAULT_GUI_FONT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::DRAWITEMSTRUCT;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetFocus, ReleaseCapture, SetCapture, SetFocus, VK_DOWN, VK_ESCAPE, VK_F4, VK_RETURN, VK_SPACE, VK_UP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWL_EXSTYLE, GetClientRect, GetCursorPos, GetForegroundWindow,
    GetMessageW, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW, IsDialogMessageW, IsWindow, KillTimer,
    LoadCursorW, MSG, PostQuitMessage, RegisterClassW, SW_SHOW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW,
    SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE,
    WM_COMMAND, WM_CTLCOLOREDIT, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_PAINT, WM_SETFONT, WM_TIMER,
    WindowFromPoint, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_SYSMENU,
    WS_TABSTOP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};
use windows_numerics::Vector2;

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
    /// Lines of a text area under the boxes where `accept` can show an answer (`Reply::Show`); 0 for none. The Cancel button then says Close.
    pub result_lines: usize,
    /// The words on the second button: `None` is Cancel (Close with a result area), `Some("")` has no second button (a message).
    pub cancel_label: Option<&'a str>,
    /// `false` shows the form without taking the keyboard focus (to look at it from a test).
    pub activate: bool,
}

const ID_OK: i32 = 1;
const ID_CANCEL: i32 = 2;
const ID_COMBO: i32 = 100;
const ID_EDIT: i32 = 200;
/// Logical sizes (scaled with the screen's dpi).
const WIDTH: i32 = 460;
const MARGIN: i32 = 28;
const FIELD_H: i32 = 38;
const BUTTON_H: i32 = 38;
const ROW_H: i32 = 34;
const EX_RIGHT: isize = 0x0000_1000;
const EX_RTLREADING: isize = 0x0000_2000;
const EX_LEFTSCROLLBAR: isize = 0x0000_4000;
const ES_AUTOHSCROLL: u32 = 0x0080;
const BS_OWNERDRAW: u32 = 0x000B;
const SS_LEFT: u32 = 0x0000;
const EN_SETFOCUS: u32 = 0x0100;
const EN_KILLFOCUS: u32 = 0x0200;
const WM_DRAWITEM: u32 = 0x002B;
const WM_CAPTURECHANGED: u32 = 0x0215;
const WM_MOUSEACTIVATE: u32 = 0x0021;
const TIMER_HOVER: usize = 91;

/// A colour as bytes: red, green, blue.
type Rgb = [u8; 3];

#[derive(Clone, Copy)]
struct Palette {
    dark: bool,
    window: Rgb,
    text: Rgb,
    muted: Rgb,
    field: Rgb,
    field_hover: Rgb,
    border: Rgb,
    error: Rgb,
    accent: Rgb,
    on_accent: Rgb,
}

fn palette(theme: Theme) -> Palette {
    let accent = crate::look::system_accent();
    let lum = 0.299 * accent[0] as f32 + 0.587 * accent[1] as f32 + 0.114 * accent[2] as f32;
    let on_accent = if lum > 158.0 { [18, 18, 18] } else { [255, 255, 255] };
    if theme == Theme::Dark {
        Palette { dark: true, window: [0x20, 0x20, 0x20], text: [0xF2, 0xF2, 0xF2], muted: [0xA8, 0xA8, 0xA8], field: [0x2B, 0x2B, 0x2B], field_hover: [0x33, 0x33, 0x33], border: [0x4A, 0x4A, 0x4A], error: [0xFF, 0x7B, 0x72], accent, on_accent }
    } else {
        Palette { dark: false, window: [0xF3, 0xF3, 0xF3], text: [0x1A, 0x1A, 0x1A], muted: [0x5F, 0x5F, 0x5F], field: [0xFF, 0xFF, 0xFF], field_hover: [0xF7, 0xF7, 0xF7], border: [0xCF, 0xCF, 0xCF], error: [0xC4, 0x2B, 0x1C], accent, on_accent }
    }
}

fn colorref(c: Rgb) -> COLORREF {
    COLORREF(c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16)
}

/// A colour moved toward white (`k` > 0) or black (`k` < 0).
fn shade(c: Rgb, k: f32) -> Rgb {
    let to = if k >= 0.0 { 255.0 } else { 0.0 };
    let k = k.abs();
    [0, 1, 2].map(|i| (c[i] as f32 + (to - c[i] as f32) * k).round().clamp(0.0, 255.0) as u8)
}

struct State {
    hwnd: HWND,
    factory: Option<ID2D1Factory>,
    pal: Palette,
    scale: f32,
    combo: Option<HWND>,
    options: Vec<String>,
    /// The option chosen in the drop-down.
    sel: usize,
    /// The list of the drop-down while it is open, and the row under the pointer or the keys.
    list: Option<HWND>,
    hot_row: usize,
    edits: Vec<HWND>,
    rtl: Vec<Option<fn(usize) -> bool>>,
    /// The frame drawn behind each text box (device pixels).
    frames: Vec<RECT>,
    /// The card behind the result text, if there is one.
    card: Option<RECT>,
    ok: HWND,
    cancel: HWND,
    error: HWND,
    result: Option<HWND>,
    /// Texts drawn in the muted colour (the explanation, the labels).
    muted_texts: Vec<HWND>,
    bg: HBRUSH,
    field_bg: HBRUSH,
    font_body: HFONT,
    font_button: HFONT,
    /// The pointer is over: the button, Cancel, the drop-down.
    hot: [bool; 3],
    /// The form has been in front at least once (a form shown only to be looked at never is, and its list must not close for that).
    seen_front: bool,
    /// The person pressed the button, closed the form, or nothing happened yet.
    submit: bool,
    closed: bool,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// What `accept` answers when the person presses the button.
pub enum Reply {
    /// Done: the form closes.
    Close,
    /// A sentence shown in red; the form stays open with the text kept.
    Error(String),
    /// A text shown in the result area (`Form::result_lines`); the form stays open.
    Show(String),
}

// ───────────── drawing ─────────────

/// One shape of the form, in device pixels relative to the area it is drawn in.
enum Shape {
    Round { r: [f32; 4], radius: f32, fill: Option<Rgb>, stroke: Option<(Rgb, f32)> },
    Line { a: (f32, f32), b: (f32, f32), color: Rgb, width: f32 },
}

fn d2d_color(c: Rgb) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c[0] as f32 / 255.0, g: c[1] as f32 / 255.0, b: c[2] as f32 / 255.0, a: 1.0 }
}

/// Draws shapes with Direct2D (smooth edges) on a device context, inside `area` of it.
fn draw_shapes(factory: &ID2D1Factory, hdc: HDC, area: RECT, shapes: &[Shape]) {
    unsafe {
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
            dpiX: 96.0,
            dpiY: 96.0,
            usage: D2D1_RENDER_TARGET_USAGE_GDI_COMPATIBLE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let Ok(rt) = factory.CreateDCRenderTarget(&props) else { return };
        if rt.BindDC(hdc, &area).is_err() {
            return;
        }
        let Ok(brush) = rt.CreateSolidColorBrush(&d2d_color([0, 0, 0]), None) else { return };
        rt.BeginDraw();
        for shape in shapes {
            match shape {
                Shape::Round { r, radius, fill, stroke } => {
                    let rr = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: r[0], top: r[1], right: r[2], bottom: r[3] }, radiusX: *radius, radiusY: *radius };
                    if let Some(c) = fill {
                        brush.SetColor(&d2d_color(*c));
                        rt.FillRoundedRectangle(&rr, &brush);
                    }
                    if let Some((c, width)) = stroke {
                        brush.SetColor(&d2d_color(*c));
                        // the stroke is centred on the edge: the shape is drawn half a line inside, so it keeps its size
                        let h = width / 2.0;
                        let inner = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: r[0] + h, top: r[1] + h, right: r[2] - h, bottom: r[3] - h }, radiusX: (radius - h).max(0.0), radiusY: (radius - h).max(0.0) };
                        rt.DrawRoundedRectangle(&inner, &brush, *width, None);
                    }
                }
                Shape::Line { a, b, color, width } => {
                    brush.SetColor(&d2d_color(*color));
                    rt.DrawLine(Vector2 { X: a.0, Y: a.1 }, Vector2 { X: b.0, Y: b.1 }, &brush, *width, None);
                }
            }
        }
        let _ = rt.EndDraw(None, None);
    }
}

/// Writes `text` inside `rc` with the font and colour given (GDI: the same text rendering as the text boxes).
fn draw_text(hdc: HDC, font: HFONT, color: Rgb, text: &str, rc: &mut RECT, flags: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT) {
    unsafe {
        let old = SelectObject(hdc, HGDIOBJ(font.0));
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, colorref(color));
        let mut buf: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(hdc, &mut buf, rc, flags | DT_NOPREFIX);
        SelectObject(hdc, old);
    }
}

fn rect_f(rc: &RECT) -> [f32; 4] {
    [0.0, 0.0, (rc.right - rc.left) as f32, (rc.bottom - rc.top) as f32]
}

/// The little arrow of the drop-down, at the right of its box.
fn chevron(w: f32, h: f32, k: f32, color: Rgb) -> [Shape; 2] {
    let (cx, cy, s) = (w - 22.0 * k, h / 2.0, 4.5 * k);
    [Shape::Line { a: (cx - s, cy - s / 2.0), b: (cx, cy + s / 2.0), color, width: 1.8 * k }, Shape::Line { a: (cx, cy + s / 2.0), b: (cx + s, cy - s / 2.0), color, width: 1.8 * k }]
}

/// Draws a button of the form (the owner-drawn ones: OK, Cancel and the drop-down).
fn draw_button(dis: &DRAWITEMSTRUCT) {
    let Some((pal, factory, k, hot, bg, font_button, font_body, text, open)) = with_state(|s| {
        let id = dis.CtlID as i32;
        let hot = match id {
            ID_OK => s.hot[0],
            ID_CANCEL => s.hot[1],
            _ => s.hot[2],
        };
        let text = match id {
            ID_COMBO => s.options.get(s.sel).cloned().unwrap_or_default(),
            _ => String::new(),
        };
        (s.pal, s.factory.clone(), s.scale, hot, s.bg, s.font_button, s.font_body, text, s.list.is_some())
    }) else {
        return;
    };
    unsafe {
        let hdc = dis.hDC;
        let mut rc = dis.rcItem;
        FillRect(hdc, &rc, bg);
        let (w, h) = ((rc.right - rc.left) as f32, (rc.bottom - rc.top) as f32);
        let id = dis.CtlID as i32;
        let pressed = (dis.itemState.0 & 0x0001) != 0; // ODS_SELECTED
        let focused = (dis.itemState.0 & 0x0010) != 0; // ODS_FOCUS
        let radius = 8.0 * k;
        let mut shapes = Vec::new();
        let (label_color, font) = match id {
            ID_OK => {
                let fill = if pressed { shade(pal.accent, -0.14) } else if hot { shade(pal.accent, if pal.dark { 0.12 } else { -0.08 }) } else { pal.accent };
                shapes.push(Shape::Round { r: rect_f(&rc), radius, fill: Some(fill), stroke: None });
                if focused {
                    // a ring inside, so it shows on the accent colour
                    let i = 3.0 * k;
                    shapes.push(Shape::Round { r: [i, i, w - i, h - i], radius: radius - 2.0 * k, fill: None, stroke: Some((pal.on_accent, 1.5 * k)) });
                }
                (pal.on_accent, font_button)
            }
            ID_CANCEL => {
                let fill = if pressed { Some(pal.field_hover) } else if hot { Some(pal.field) } else { None };
                shapes.push(Shape::Round { r: rect_f(&rc), radius, fill, stroke: Some(if focused { (pal.accent, 2.0 * k) } else { (pal.border, 1.0 * k) }) });
                (pal.text, font_button)
            }
            _ => {
                let fill = if hot || open { pal.field_hover } else { pal.field };
                shapes.push(Shape::Round { r: rect_f(&rc), radius, fill: Some(fill), stroke: Some(if focused || open { (pal.accent, 2.0 * k) } else { (pal.border, 1.0 * k) }) });
                shapes.extend(chevron(w, h, k, pal.muted));
                (pal.text, font_body)
            }
        };
        if let Some(f) = &factory {
            draw_shapes(f, hdc, rc, &shapes);
        }
        if id == ID_COMBO {
            let mut r = RECT { left: rc.left + (12.0 * k) as i32, top: rc.top, right: rc.right - (40.0 * k) as i32, bottom: rc.bottom };
            draw_text(hdc, font, label_color, &text, &mut r, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
        } else {
            let label = button_label(dis.hwndItem);
            draw_text(hdc, font, label_color, &label, &mut rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
    }
}

fn button_label(h: HWND) -> String {
    text_of(h)
}

/// Draws what is behind the text boxes: their frames (the accent colour on the one that has the keyboard) and the card of the result text.
fn paint_form(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let data = with_state(|s| (s.factory.clone(), s.pal, s.scale, s.frames.clone(), s.edits.clone(), s.card));
        if let Some((Some(factory), pal, k, frames, edits, card)) = data {
            let focus = GetFocus();
            let mut client = RECT::default();
            let _ = GetClientRect(hwnd, &mut client);
            let mut shapes = Vec::new();
            for (frame, edit) in frames.iter().zip(&edits) {
                let focused = focus == *edit;
                shapes.push(Shape::Round { r: [frame.left as f32, frame.top as f32, frame.right as f32, frame.bottom as f32], radius: 8.0 * k, fill: Some(pal.field), stroke: Some(if focused { (pal.accent, 2.0 * k) } else { (pal.border, 1.0 * k) }) });
            }
            if let Some(c) = card {
                shapes.push(Shape::Round { r: [c.left as f32, c.top as f32, c.right as f32, c.bottom as f32], radius: 8.0 * k, fill: Some(pal.field), stroke: Some((pal.border, 1.0 * k)) });
            }
            draw_shapes(&factory, hdc, client, &shapes);
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

fn create_font(height: i32, weight: i32) -> HFONT {
    unsafe { CreateFontW(-height, 0, 0, 0, weight, 0, 0, 0, Default::default(), Default::default(), Default::default(), Default::default(), 0, w!("Segoe UI")) }
}

/// The height a text takes when wrapped at `width` (device pixels).
fn text_height(font: HFONT, text: &str, width: i32) -> i32 {
    unsafe {
        let hdc = GetDC(None);
        let old = SelectObject(hdc, HGDIOBJ(font.0));
        let mut rc = RECT { left: 0, top: 0, right: width, bottom: 0 };
        let mut buf: Vec<u16> = text.encode_utf16().collect();
        DrawTextW(hdc, &mut buf, &mut rc, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
        SelectObject(hdc, old);
        ReleaseDC(None, hdc);
        rc.bottom - rc.top
    }
}

// ───────────── the form ─────────────

/// Opens the form and returns when it is closed: `true` when `accept` said `Ok`, `false` when the person cancelled.
pub fn ask(owner: Option<HWND>, theme: Theme, form: &Form, accept: &mut dyn FnMut(usize, &[String]) -> Result<(), String>) -> bool {
    ask_with(owner, theme, form, &mut |choice, texts| match accept(choice, texts) {
        Ok(()) => Reply::Close,
        Err(sentence) => Reply::Error(sentence),
    })
}

/// A message in the look of the settings window: a heading, a text and one button (`cancel_label` `None`), or two (`Some("Later")`): `true` when the
/// first one was pressed. It replaces the plain Windows message box.
pub fn message(owner: Option<HWND>, theme: Theme, heading: &str, text: &str, ok_label: &str, cancel_label: Option<&str>) -> bool {
    let form = Form {
        title: heading,
        intro: text,
        choice: None,
        fields: &[],
        initial: &[],
        ok_label,
        result_lines: 0,
        cancel_label: Some(cancel_label.unwrap_or("")),
        activate: true,
    };
    ask_with(owner, theme, &form, &mut |_, _| Reply::Close)
}

/// Like `ask`, with an answer that may also show a text in the form without closing it.
pub fn ask_with(owner: Option<HWND>, theme: Theme, form: &Form, accept: &mut dyn FnMut(usize, &[String]) -> Reply) -> bool {
    unsafe {
        let pal = palette(theme);
        let Ok(hinst) = GetModuleHandleW(None) else { return false };
        let class = w!("VerbaDialog");
        let bg = CreateSolidBrush(colorref(pal.window));
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(dialog_proc),
            hInstance: hinst.into(),
            lpszClassName: class,
            hCursor: LoadCursorW(None, IDC_ARROW).ok().unwrap_or_default(),
            ..Default::default()
        });
        let list_class = w!("VerbaDropList");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(list_proc),
            hInstance: hinst.into(),
            lpszClassName: list_class,
            hCursor: LoadCursorW(None, IDC_ARROW).ok().unwrap_or_default(),
            ..Default::default()
        });

        let dpi = owner.map(|o| GetDpiForWindow(o)).filter(|d| *d > 0).unwrap_or_else(|| GetDpiForSystem().max(96)) as i32;
        let k = dpi as f32 / 96.0;
        let s = |v: i32| (v as f32 * k).round() as i32;
        let font_head = create_font(s(20), 600);
        let font_intro = create_font(s(13), 400);
        let font_label = create_font(s(12), 600);
        let font_body = create_font(s(14), 400);
        let font_button = create_font(s(14), 600);
        let fallback = GetStockObject(DEFAULT_GUI_FONT);
        let pick = |f: HFONT| if f.is_invalid() { fallback } else { HGDIOBJ(f.0) };

        // the height: the controls one under the other
        let inner = WIDTH - 2 * MARGIN;
        let mut y = 26;
        let head_y = y;
        y += 28 + 6;
        let intro_y = y;
        let intro_h = if form.intro.is_empty() { 0 } else { (text_height(font_intro, form.intro, s(inner)) as f32 / k).ceil() as i32 + 2 };
        y += if intro_h > 0 { intro_h + 20 } else { 8 };
        let combo_y = y;
        if form.choice.is_some() {
            y += 22 + FIELD_H + 16;
        }
        let mut field_y = Vec::new();
        for _ in form.fields {
            field_y.push(y);
            y += 22 + FIELD_H + 16;
        }
        let result_y = y;
        let result_h = form.result_lines as i32 * 20 + 24;
        if form.result_lines > 0 {
            y += result_h + 14;
        }
        let error_y = y;
        // a message (no box to type in, no answer to show) has no line for an error
        let bare = form.fields.is_empty() && form.choice.is_none() && form.result_lines == 0;
        y += if bare { 4 } else { 40 };
        let buttons_y = y;
        y += BUTTON_H + 26;
        let (client_w, client_h) = (s(WIDTH), s(y));
        let style = WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
        let mut frame = RECT { left: 0, top: 0, right: client_w, bottom: client_h };
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WINDOW_EX_STYLE(0), dpi as u32);
        let (ww, wh) = (frame.right - frame.left, frame.bottom - frame.top);
        let (mut x0, mut y0) = (200, 200);
        let mut owner_rect = RECT::default();
        if let Some(o) = owner {
            if GetWindowRect(o, &mut owner_rect).is_ok() {
                x0 = owner_rect.left + (owner_rect.right - owner_rect.left - ww) / 2;
                y0 = owner_rect.top + (owner_rect.bottom - owner_rect.top - wh) / 3;
            }
        }

        let title = wide("Verba");
        let Ok(hwnd) = CreateWindowExW(WINDOW_EX_STYLE(0), class, PCWSTR(title.as_ptr()), style, x0, y0, ww, wh, owner, None, Some(hinst.into()), None) else {
            for f in [font_head, font_intro, font_label, font_body, font_button] {
                let _ = DeleteObject(HGDIOBJ(f.0));
            }
            let _ = DeleteObject(HGDIOBJ(bg.0));
            return false;
        };
        let field_bg = CreateSolidBrush(colorref(pal.field));

        let control = |class: PCWSTR, text: &str, style: u32, ex: isize, id: i32, font: HGDIOBJ, x: i32, y: i32, w: i32, h: i32| -> HWND {
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
            SendMessageW(h, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            h
        };
        let mut muted_texts = Vec::new();
        control(w!("STATIC"), form.title, SS_LEFT, 0, -1, pick(font_head), MARGIN, head_y, inner, 28);
        if intro_h > 0 {
            muted_texts.push(control(w!("STATIC"), form.intro, SS_LEFT, 0, -1, pick(font_intro), MARGIN, intro_y, inner, intro_h));
        }
        let mut label = |text: &str, y: i32| muted_texts.push(control(w!("STATIC"), text, SS_LEFT, 0, -1, pick(font_label), MARGIN, y, inner, 18));
        let mut options = Vec::new();
        let mut combo = None;
        if let Some((text, opts)) = form.choice {
            label(text, combo_y);
            options = opts.iter().map(|o| o.to_string()).collect();
            combo = Some(control(w!("BUTTON"), "", BS_OWNERDRAW | WS_TABSTOP.0, 0, ID_COMBO, pick(font_body), MARGIN, combo_y + 22, inner, FIELD_H));
        }
        let mut edits = Vec::new();
        let mut frames = Vec::new();
        for (i, f) in form.fields.iter().enumerate() {
            label(f.label, field_y[i]);
            let top = field_y[i] + 22;
            frames.push(RECT { left: s(MARGIN), top: s(top), right: s(MARGIN + inner), bottom: s(top + FIELD_H) });
            // the text box sits inside its frame (no border of its own: the frame is drawn behind it)
            edits.push(control(w!("EDIT"), "", ES_AUTOHSCROLL | WS_TABSTOP.0, 0, ID_EDIT + i as i32, pick(font_body), MARGIN + 12, top + 9, inner - 24, 20));
        }
        let mut card = None;
        let result = (form.result_lines > 0).then(|| {
            card = Some(RECT { left: s(MARGIN), top: s(result_y), right: s(MARGIN + inner), bottom: s(result_y + result_h) });
            control(w!("STATIC"), "", SS_LEFT, 0, -1, pick(font_body), MARGIN + 14, result_y + 12, inner - 28, form.result_lines as i32 * 20)
        });
        let error = control(w!("STATIC"), "", SS_LEFT, 0, -1, pick(font_intro), MARGIN, error_y, inner, 36);
        let ok_w = 112;
        let one_button = form.cancel_label == Some("");
        let ok_x = if one_button { WIDTH - MARGIN - ok_w } else { WIDTH - MARGIN - 2 * ok_w - 10 };
        let ok = control(w!("BUTTON"), form.ok_label, BS_OWNERDRAW | WS_TABSTOP.0, 0, ID_OK, pick(font_button), ok_x, buttons_y, ok_w, BUTTON_H);
        let cancel_text = form.cancel_label.filter(|l| !l.is_empty()).unwrap_or(if form.result_lines > 0 { "Close" } else { "Cancel" });
        let cancel = control(w!("BUTTON"), cancel_text, BS_OWNERDRAW | WS_TABSTOP.0, 0, ID_CANCEL, pick(font_button), WIDTH - MARGIN - ok_w, buttons_y, ok_w, BUTTON_H);
        if one_button {
            // a message: one button; Esc and the close box still answer "no"
            let _ = ShowWindow(cancel, SW_HIDE);
        }
        if pal.dark {
            let v = windows::core::BOOL(1);
            let _ = windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                hwnd,
                windows::Win32::Graphics::Dwm::DWMWA_USE_IMMERSIVE_DARK_MODE,
                &v as *const _ as *const _,
                std::mem::size_of::<windows::core::BOOL>() as u32,
            );
            // (the text boxes keep the dark selection and scroll look of Windows' dark controls)
            for e in &edits {
                let _ = windows::Win32::UI::Controls::SetWindowTheme(*e, w!("DarkMode_CFD"), PCWSTR::null());
            }
        }

        let factory: Option<ID2D1Factory> = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok();
        STATE.with(|st| {
            *st.borrow_mut() = Some(State {
                hwnd,
                factory,
                pal,
                scale: k,
                combo,
                options,
                sel: 0,
                list: None,
                hot_row: 0,
                edits: edits.clone(),
                rtl: form.fields.iter().map(|f| f.rtl).collect(),
                frames,
                card,
                ok,
                cancel,
                error,
                result,
                muted_texts,
                bg,
                field_bg,
                font_body,
                font_button,
                hot: [false; 3],
                seen_front: false,
                submit: false,
                closed: false,
            })
        });
        apply_direction();
        for (edit, text) in edits.iter().zip(form.initial) {
            let t = wide(text);
            let _ = SetWindowTextW(*edit, PCWSTR(t.as_ptr()));
        }
        let _ = SetTimer(Some(hwnd), TIMER_HOVER, 50, None);

        if let Some(o) = owner {
            let _ = EnableWindow(o, false);
        }
        if form.activate {
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(edits.first().copied().or(combo).unwrap_or(ok)));
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
            if msg.message == WM_KEYDOWN && form_key(msg.wParam.0 as u16) {
                continue;
            }
            if !IsDialogMessageW(hwnd, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if with_state(|s| std::mem::take(&mut s.submit)).unwrap_or(false) {
                let choice = with_state(|s| s.sel).unwrap_or(0);
                let texts: Vec<String> = edits.iter().map(|e| text_of(*e)).collect();
                match accept(choice, &texts) {
                    Reply::Close => {
                        saved = true;
                        break;
                    }
                    Reply::Show(text) => {
                        if let Some(r) = result {
                            let t = wide(&text);
                            let _ = SetWindowTextW(r, PCWSTR(t.as_ptr()));
                        }
                        let _ = SetWindowTextW(error, PCWSTR(wide("").as_ptr()));
                    }
                    Reply::Error(sentence) => {
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

        if let Some(o) = owner {
            let _ = EnableWindow(o, true);
            let _ = SetForegroundWindow(o);
        }
        let _ = KillTimer(Some(hwnd), TIMER_HOVER);
        close_list();
        let _ = DestroyWindow(hwnd);
        STATE.with(|st| *st.borrow_mut() = None);
        let _ = DeleteObject(HGDIOBJ(bg.0));
        let _ = DeleteObject(HGDIOBJ(field_bg.0));
        for f in [font_head, font_intro, font_label, font_body, font_button] {
            if !f.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(f.0));
            }
        }
        saved
    }
}

/// The keys the form handles itself before Windows' dialog keys: Enter sends the form, the arrows and Enter steer the open list or change the
/// drop-down's choice. Returns `true` when the key was used.
fn form_key(vk: u16) -> bool {
    let Some((list_open, combo, ok, cancel)) = with_state(|s| (s.list.is_some(), s.combo, s.ok, s.cancel)) else { return false };
    let focus = unsafe { GetFocus() };
    if list_open {
        let n = with_state(|s| s.options.len()).unwrap_or(0).max(1);
        match vk {
            v if v == VK_UP.0 => with_state(|s| s.hot_row = (s.hot_row + n - 1) % n),
            v if v == VK_DOWN.0 => with_state(|s| s.hot_row = (s.hot_row + 1) % n),
            v if v == VK_RETURN.0 || v == VK_SPACE.0 => {
                choose_row(with_state(|s| s.hot_row).unwrap_or(0));
                return true;
            }
            v if v == VK_ESCAPE.0 => {
                close_list();
                return true;
            }
            // any other key (Tab) leaves the drop-down: the list closes and the key does its work
            _ => {
                close_list();
                return false;
            }
        };
        refresh_list();
        return true;
    }
    if Some(focus) == combo {
        let n = with_state(|s| s.options.len()).unwrap_or(0).max(1);
        if vk == VK_UP.0 || vk == VK_DOWN.0 {
            let up = vk == VK_UP.0;
            choose_row(with_state(|s| if up { (s.sel + n - 1) % n } else { (s.sel + 1) % n }).unwrap_or(0));
            return true;
        }
        if vk == VK_F4.0 {
            open_list();
            return true;
        }
    }
    if vk == VK_RETURN.0 {
        if focus == cancel {
            with_state(|s| s.closed = true);
        } else if focus == ok || focus == combo.unwrap_or_default() || with_state(|s| s.edits.contains(&focus)).unwrap_or(false) {
            with_state(|s| s.submit = true);
        } else {
            return false;
        }
        return true;
    }
    false
}

// ───────────── the drop-down's list ─────────────

fn open_list() {
    let Some((hwnd, combo, count, sel, pal, k)) = with_state(|s| (s.hwnd, s.combo, s.options.len(), s.sel, s.pal, s.scale)) else { return };
    let Some(combo) = combo else { return };
    if count == 0 || with_state(|s| s.list.is_some()).unwrap_or(true) {
        close_list();
        return;
    }
    unsafe {
        let mut r = RECT::default();
        if GetWindowRect(combo, &mut r).is_err() {
            return;
        }
        let _ = pal;
        let (w, h) = (r.right - r.left, (count as f32 * ROW_H as f32 * k).round() as i32 + (8.0 * k) as i32);
        let Ok(hinst) = GetModuleHandleW(None) else { return };
        let Ok(list) = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            w!("VerbaDropList"),
            PCWSTR::null(),
            WS_POPUP,
            r.left,
            r.bottom + (4.0 * k) as i32,
            w,
            h,
            Some(hwnd),
            None,
            Some(hinst.into()),
            None,
        ) else {
            return;
        };
        with_state(|s| {
            s.list = Some(list);
            s.hot_row = sel;
        });
        let _ = ShowWindow(list, SW_SHOWNOACTIVATE);
        let _ = SetCapture(list);
        let _ = InvalidateRect(Some(combo), None, false);
    }
}

fn close_list() {
    let Some((list, combo)) = with_state(|s| (s.list.take(), s.combo)) else { return };
    unsafe {
        if let Some(l) = list {
            let _ = ReleaseCapture();
            if IsWindow(Some(l)).as_bool() {
                let _ = DestroyWindow(l);
            }
        }
        if let Some(c) = combo {
            let _ = InvalidateRect(Some(c), None, false);
        }
    }
}

fn refresh_list() {
    if let Some(Some(l)) = with_state(|s| s.list) {
        unsafe {
            let _ = InvalidateRect(Some(l), None, false);
        }
    }
}

/// Takes the option `row` as the choice (and closes the list).
fn choose_row(row: usize) {
    with_state(|s| s.sel = row.min(s.options.len().saturating_sub(1)));
    close_list();
    apply_direction();
}

unsafe extern "system" fn list_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let data = with_state(|s| (s.factory.clone(), s.pal, s.scale, s.options.clone(), s.sel, s.hot_row, s.font_body, s.bg));
                if let Some((factory, pal, k, options, sel, hot, font, bg)) = data {
                    let mut client = RECT::default();
                    let _ = GetClientRect(hwnd, &mut client);
                    FillRect(hdc, &client, bg);
                    let (w, h) = (client.right as f32, client.bottom as f32);
                    let mut shapes = vec![Shape::Round { r: [0.0, 0.0, w, h], radius: 8.0 * k, fill: Some(pal.field), stroke: Some((pal.border, 1.0 * k)) }];
                    let row_h = ROW_H as f32 * k;
                    let pad = 4.0 * k;
                    if hot < options.len() {
                        let top = pad + hot as f32 * row_h;
                        shapes.push(Shape::Round { r: [pad, top + 1.0 * k, w - pad, top + row_h - 1.0 * k], radius: 6.0 * k, fill: Some(pal.field_hover), stroke: None });
                        if pal.dark {
                            shapes.last_mut().map(|s| if let Shape::Round { fill, .. } = s { *fill = Some(shade(pal.field, 0.1)) });
                        }
                    }
                    if sel < options.len() {
                        let top = pad + sel as f32 * row_h;
                        shapes.push(Shape::Round { r: [pad + 3.0 * k, top + 9.0 * k, pad + 6.0 * k, top + row_h - 9.0 * k], radius: 1.5 * k, fill: Some(pal.accent), stroke: None });
                    }
                    if let Some(f) = &factory {
                        draw_shapes(f, hdc, client, &shapes);
                    }
                    for (i, name) in options.iter().enumerate() {
                        let top = (pad + i as f32 * row_h) as i32;
                        let mut r = RECT { left: (pad + 16.0 * k) as i32, top, right: client.right - (pad + 10.0 * k) as i32, bottom: top + row_h as i32 };
                        draw_text(hdc, font, if i == sel { pal.accent_text() } else { pal.text }, name, &mut r, DT_LEFT | DT_VCENTER | DT_SINGLELINE);
                    }
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE
            WM_MOUSEMOVE => {
                let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                let (k, n) = with_state(|s| (s.scale, s.options.len())).unwrap_or((1.0, 0));
                let row = ((y as f32 - 4.0 * k) / (ROW_H as f32 * k)).floor();
                if row >= 0.0 && (row as usize) < n && with_state(|s| std::mem::replace(&mut s.hot_row, row as usize) != row as usize).unwrap_or(false) {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let (x, y) = ((lparam.0 & 0xFFFF) as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as i16 as i32);
                let mut client = RECT::default();
                let _ = GetClientRect(hwnd, &mut client);
                let (k, n) = with_state(|s| (s.scale, s.options.len())).unwrap_or((1.0, 0));
                let inside = x >= 0 && y >= 0 && x < client.right && y < client.bottom;
                let row = ((y as f32 - 4.0 * k) / (ROW_H as f32 * k)).floor();
                if inside && row >= 0.0 && (row as usize) < n {
                    choose_row(row as usize);
                } else {
                    close_list();
                }
                LRESULT(0)
            }
            // another window took the mouse: the list has no reason to stay
            WM_CAPTURECHANGED => {
                if with_state(|s| s.list == Some(hwnd)).unwrap_or(false) && GetForegroundWindow() != hwnd {
                    // (closing is done by the code that took the capture, or by the timer when the form loses the front)
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

impl Palette {
    /// The accent colour made readable on the field colour (a very light accent on a white box is darkened, a very dark one on a dark box is lightened).
    fn accent_text(&self) -> Rgb {
        let lum = |c: Rgb| 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;
        let (a, f) = (lum(self.accent), lum(self.field));
        if (a - f).abs() >= 90.0 {
            self.accent
        } else if self.dark {
            shade(self.accent, 0.45)
        } else {
            shade(self.accent, -0.45)
        }
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
    let Some((sel, edits, rtl)) = with_state(|s| (s.sel, s.edits.clone(), s.rtl.clone())) else { return };
    for (edit, rtl) in edits.iter().zip(rtl) {
        let Some(rtl) = rtl else { continue };
        unsafe {
            let ex = GetWindowLongPtrW(*edit, GWL_EXSTYLE);
            let bits = EX_RIGHT | EX_RTLREADING | EX_LEFTSCROLLBAR;
            let new = if rtl(sel) { ex | bits } else { ex & !bits };
            if new != ex {
                SetWindowLongPtrW(*edit, GWL_EXSTYLE, new);
                let _ = SetWindowPos(*edit, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
            }
        }
    }
}

/// The pointer is over a button of the form (the three owner-drawn ones): redraw the ones whose state changed.
fn track_hover() {
    let Some((ok, cancel, combo, hot, list, hwnd)) = with_state(|s| (s.ok, s.cancel, s.combo, s.hot, s.list, s.hwnd)) else { return };
    unsafe {
        let mut p = POINT::default();
        if GetCursorPos(&mut p).is_err() {
            return;
        }
        let over = WindowFromPoint(p);
        let now = [over == ok, over == cancel, combo.map(|c| over == c).unwrap_or(false)];
        if now != hot {
            with_state(|s| s.hot = now);
            for (i, h) in [Some(ok), Some(cancel), combo].into_iter().enumerate() {
                if let (Some(h), true) = (h, now[i] != hot[i]) {
                    let _ = InvalidateRect(Some(h), None, false);
                }
            }
        }
        // the list of the drop-down follows the form: when the form is no longer in front (another program), the list goes
        let front = GetForegroundWindow();
        if front == hwnd {
            with_state(|s| s.seen_front = true);
        }
        if let Some(l) = list {
            if front != hwnd && front != l && with_state(|s| s.seen_front).unwrap_or(false) {
                close_list();
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
                        if with_state(|s| s.list.is_some()).unwrap_or(false) {
                            close_list();
                        } else {
                            with_state(|s| s.closed = true);
                        }
                    }
                    ID_COMBO => open_list(),
                    // a text box gets or loses the keyboard: its frame changes colour
                    _ if (code == EN_SETFOCUS || code == EN_KILLFOCUS) && id >= ID_EDIT => {
                        let _ = InvalidateRect(Some(hwnd), None, false);
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                with_state(|s| s.closed = true);
                LRESULT(0)
            }
            WM_TIMER if wparam.0 == TIMER_HOVER => {
                track_hover();
                LRESULT(0)
            }
            WM_DRAWITEM => {
                let dis = &*(lparam.0 as *const DRAWITEMSTRUCT);
                draw_button(dis);
                LRESULT(1)
            }
            WM_PAINT => {
                paint_form(hwnd);
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
                let Some((pal, bg, field_bg, error, result, muted)) = with_state(|s| (s.pal, s.bg, s.field_bg, s.error, s.result, s.muted_texts.contains(&HWND(lparam.0 as *mut _)))) else {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                };
                // a text box and the result text sit on the colour of their frame; the other texts on the colour of the window
                let on_field = msg == WM_CTLCOLOREDIT || msg == WM_CTLCOLORLISTBOX || Some(control) == result;
                let color = if control == error { pal.error } else if muted { pal.muted } else { pal.text };
                SetTextColor(hdc, colorref(color));
                SetBkColor(hdc, colorref(if on_field { pal.field } else { pal.window }));
                LRESULT((if on_field { field_bg } else { bg }).0 as isize)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shading_moves_a_colour_toward_white_or_black_and_stays_in_range() {
        assert_eq!(shade([100, 100, 100], 0.0), [100, 100, 100]);
        assert_eq!(shade([100, 100, 100], 1.0), [255, 255, 255]);
        assert_eq!(shade([100, 100, 100], -1.0), [0, 0, 0]);
        assert_eq!(shade([0, 128, 255], 0.5), [128, 192, 255]);
        assert_eq!(shade([10, 10, 10], 5.0), [255, 255, 255]);
    }

    #[test]
    fn the_accent_text_stays_readable_on_the_box_it_is_written_on() {
        let lum = |c: Rgb| 0.299 * c[0] as f32 + 0.587 * c[1] as f32 + 0.114 * c[2] as f32;
        for dark in [true, false] {
            let field = if dark { [0x2B, 0x2B, 0x2B] } else { [0xFF, 0xFF, 0xFF] };
            for accent in [[0, 120, 212], [255, 255, 255], [0, 0, 0], [250, 240, 120], [20, 20, 40]] {
                let p = Palette { dark, window: field, text: [0; 3], muted: [0; 3], field, field_hover: field, border: field, error: field, accent, on_accent: [255; 3] };
                let shown = p.accent_text();
                // a colour that was already far enough from the box is kept; otherwise it is moved until it is
                assert!((lum(shown) - lum(field)).abs() >= 60.0 || shown == accent, "accent {accent:?} on dark={dark} gave {shown:?}");
            }
        }
    }

    #[test]
    fn colours_are_written_as_windows_wants_them() {
        assert_eq!(colorref([0x11, 0x22, 0x33]).0, 0x0033_2211);
    }
}
