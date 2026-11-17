//! The suggestion popup: a small card that floats above the text being typed.
//!
//! It is drawn with Direct2D and DirectWrite into a layered window with per-pixel transparency, so it has
//! smooth rounded corners, a soft shadow and crisp text (Arabic included, right to left). It never takes the
//! keyboard focus, so the app the user is typing in keeps it. A click on a candidate accepts it.

use std::cell::RefCell;

use verba_core::Suggestion;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    D2D1CreateFactory, ID2D1Factory, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL,
    DWRITE_READING_DIRECTION_RIGHT_TO_LEFT, DWRITE_TEXT_METRICS, DWriteCreateFactory, IDWriteFactory, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW, HBITMAP, HDC, HGDIOBJ, MONITOR_DEFAULTTONEAREST, MONITORINFO,
    MonitorFromRect, ReleaseDC, SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, GetDpiForSystem, MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetWindowRect, HWND_TOPMOST, IDC_HAND, KillTimer,
    LoadCursorW, MSG, PM_REMOVE, PeekMessageW, PostMessageW, RegisterClassW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_SHOWWINDOW, SW_HIDE, SetCursor, SetTimer, SetWindowPos, ShowWindow, TranslateMessage, ULW_ALPHA, UpdateLayeredWindow,
    WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_SETCURSOR, WM_TIMER, WNDCLASSW, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::w;
use windows_numerics::Vector2;

use crate::focus::{Anchor, AnchorKind};
pub use crate::look::{Look, Theme};

const FADE_TIMER: usize = 1;
const WM_MOUSELEAVE: u32 = 0x02A3;
const SHADOW_LAYERS: usize = 10;

// ───────────── small geometry helpers ─────────────

#[derive(Clone, Copy, Debug, Default)]
struct Rf {
    l: f32,
    t: f32,
    r: f32,
    b: f32,
}

impl Rf {
    fn d2d(self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.l, top: self.t, right: self.r, bottom: self.b }
    }
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.l && x < self.r && y >= self.t && y < self.b
    }
    fn grow(self, d: f32) -> Rf {
        Rf { l: self.l - d, t: self.t - d, r: self.r + d, b: self.b + d }
    }
    fn shift(self, dx: f32, dy: f32) -> Rf {
        Rf { l: self.l + dx, t: self.t + dy, r: self.r + dx, b: self.b + dy }
    }
}

fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

struct Palette {
    bg: D2D1_COLOR_F,
    border: D2D1_COLOR_F,
    text: D2D1_COLOR_F,
    muted: D2D1_COLOR_F,
    hover: D2D1_COLOR_F,
    accent: D2D1_COLOR_F,
    on_accent: D2D1_COLOR_F,
}

fn palette(look: Look) -> Palette {
    let [ar, ag, ab] = look.accent;
    let (ar, ag, ab) = (ar as f32 / 255.0, ag as f32 / 255.0, ab as f32 / 255.0);
    // dark text on a light accent, white text on a dark one
    let lum = 0.299 * ar + 0.587 * ag + 0.114 * ab;
    let on_accent = if lum > 0.62 { rgba(0.07, 0.07, 0.07, 1.0) } else { rgba(1.0, 1.0, 1.0, 1.0) };
    match look.theme {
        Theme::Light => Palette {
            bg: rgba(0.976, 0.976, 0.980, 0.98),
            border: rgba(0.0, 0.0, 0.0, 0.13),
            text: rgba(0.09, 0.09, 0.10, 1.0),
            muted: rgba(0.40, 0.40, 0.43, 1.0),
            hover: rgba(0.0, 0.0, 0.0, 0.07),
            accent: rgba(ar, ag, ab, 1.0),
            on_accent,
        },
        Theme::Dark => Palette {
            bg: rgba(0.165, 0.165, 0.176, 0.98),
            border: rgba(1.0, 1.0, 1.0, 0.13),
            text: rgba(0.97, 0.97, 0.98, 1.0),
            muted: rgba(0.66, 0.66, 0.70, 1.0),
            hover: rgba(1.0, 1.0, 1.0, 0.10),
            accent: rgba(ar, ag, ab, 1.0),
            on_accent,
        },
    }
}

// ───────────── what is on screen ─────────────

struct Item {
    layout: IDWriteTextLayout,
    rect: Rf,
    tw: f32,
    th: f32,
}

struct Hint {
    layout: IDWriteTextLayout,
    chip: Rf,
    divider_x: f32,
    tw: f32,
    th: f32,
}

/// A small label at the start of the card ("Fix"), which says what the candidates are.
struct Lead {
    layout: IDWriteTextLayout,
    rect: Rf,
    tw: f32,
    th: f32,
}

/// A bitmap the popup is drawn into, then handed to Windows with its own transparency.
struct Dib {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

struct Shown {
    items: Vec<Item>,
    hint: Option<Hint>,
    lead: Option<Lead>,
    selected: usize,
    hover: Option<usize>,
    w: i32,
    h: i32,
    pos: POINT,
    card: Rf,
    look: Look,
    scale: f32,
    dib: Dib,
    alpha: u8,
    tracking: bool,
}

struct Gfx {
    dwrite: IDWriteFactory,
    target: windows::Win32::Graphics::Direct2D::ID2D1DCRenderTarget,
}

struct State {
    gfx: Gfx,
    shown: Option<Shown>,
    visible: bool,
    /// Where a click on a candidate is reported: window and message (the candidate's number is the wParam).
    notify: Option<(HWND, u32)>,
    /// The text of the key cap at the end of the card: the keys that choose and accept ("Tab", or "← →  Tab").
    hint: String,
}

thread_local! {
    // the window procedure runs on the thread that made the window, so the state can live here
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

fn with_state<R>(f: impl FnOnce(&mut State, HWND) -> R, hwnd: HWND) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(|st| f(st, hwnd))))
}

// ───────────── the popup ─────────────

pub struct Overlay {
    hwnd: HWND,
}

impl Overlay {
    /// Makes the (hidden) popup window. Call once, on the thread that will pump messages.
    pub fn new() -> windows::core::Result<Self> {
        unsafe {
            // positions we get from other apps are real pixels, so be DPI-aware too
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let hinst = GetModuleHandleW(None)?;
            let class = w!("VerbaSuggestionPopup");
            let wc = WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: class, ..Default::default() };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                class,
                w!("Verba"),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(hinst.into()),
                None,
            )?;

            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                // 96 dpi: one unit is one real pixel, and the sizes are scaled by hand per monitor
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let target = d2d.CreateDCRenderTarget(&props)?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            STATE.with(|s| *s.borrow_mut() = Some(State { gfx: Gfx { dwrite, target }, shown: None, visible: false, notify: None, hint: "Tab".to_string() }));
            Ok(Self { hwnd })
        }
    }

    /// What the key cap at the end of the card says. Takes effect the next time the card is shown.
    pub fn set_hint(&self, text: &str) {
        with_state(|st, _| st.hint = text.to_string(), self.hwnd);
    }

    /// Tells the popup where to report a click on a candidate: `msg` is posted to `target` with the candidate's number as wParam.
    pub fn set_click_target(&self, target: HWND, msg: u32) {
        with_state(|st, _| st.notify = Some((target, msg)), self.hwnd);
    }

    /// Shows the candidates just above `anchor` (or below it when there is no room above).
    /// `selected` is the one Tab will accept.
    pub fn show(&self, anchor: Anchor, suggestions: &[Suggestion], selected: usize, look: Look) {
        self.show_with(anchor, suggestions, selected, look, None);
    }

    /// Like `show`, with a small label before the candidates (for example "Fix" for spelling corrections).
    pub fn show_with(&self, anchor: Anchor, suggestions: &[Suggestion], selected: usize, look: Look, lead: Option<&str>) {
        if suggestions.is_empty() {
            self.hide();
            return;
        }
        let hwnd = self.hwnd;
        let _ = with_state(
            |st, hwnd| {
                if let Err(_e) = build_and_present(st, hwnd, anchor, suggestions, selected, look, lead) {
                    #[cfg(debug_assertions)]
                    eprintln!("verba: popup failed: {_e}");
                }
            },
            hwnd,
        );
    }

    /// Hides the popup (it stays ready to show again).
    pub fn hide(&self) {
        let hwnd = self.hwnd;
        let _ = with_state(
            |st, hwnd| {
                st.visible = false;
                st.shown = None;
                unsafe {
                    let _ = KillTimer(Some(hwnd), FADE_TIMER);
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
            },
            hwnd,
        );
    }

    /// Where the popup (with its shadow margin) is on the screen right now.
    pub fn rect(&self) -> RECT {
        let mut r = RECT::default();
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut r);
        }
        r
    }

    /// Lets Windows deliver the popup's messages. Only needed when no other loop is running.
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
        STATE.with(|s| *s.borrow_mut() = None);
    }
}

// ───────────── layout ─────────────

fn monitor_scale(rect: &RECT) -> (f32, RECT) {
    unsafe {
        let mon = MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let work = if GetMonitorInfoW(mon, &mut mi).as_bool() { mi.rcWork } else { *rect };
        let (mut dx, mut dy) = (0u32, 0u32);
        let dpi = if GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy).is_ok() && dx > 0 { dx } else { GetDpiForSystem().max(96) };
        (dpi as f32 / 96.0, work)
    }
}

fn build_and_present(
    st: &mut State,
    hwnd: HWND,
    anchor: Anchor,
    suggestions: &[Suggestion],
    selected: usize,
    look: Look,
    lead_text: Option<&str>,
) -> windows::core::Result<()> {
    unsafe {
        let (s, work) = monitor_scale(&anchor.rect);
        let font_px = 15.0 * s;
        let small_px = 11.5 * s;
        let pad_x = 11.0 * s;
        let item_h = 30.0 * s;
        let margin = 4.0 * s;
        let shadow = 14.0 * s;
        let gap = 6.0 * s;
        let rtl_layout = suggestions[0].lang.is_rtl();

        // measure every word with the same font the drawing uses
        let text_layout = |text: &str, rtl: bool, px: f32, bold: bool| -> windows::core::Result<(IDWriteTextLayout, f32, f32)> {
            let weight = if bold {
                windows::Win32::Graphics::DirectWrite::DWRITE_FONT_WEIGHT_SEMI_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            };
            let fmt = st.gfx.dwrite.CreateTextFormat(w!("Segoe UI"), None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, px, w!("en-us"))?;
            if rtl {
                fmt.SetReadingDirection(DWRITE_READING_DIRECTION_RIGHT_TO_LEFT)?;
            }
            let wide: Vec<u16> = text.encode_utf16().collect();
            let layout = st.gfx.dwrite.CreateTextLayout(&wide, &fmt, 2000.0, 200.0)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            layout.GetMetrics(&mut m)?;
            // shrink the layout box to the text: a right-to-left text is placed against the right edge of its box
            layout.SetMaxWidth(m.widthIncludingTrailingWhitespace.max(m.width) + 1.0)?;
            Ok((layout, m.width, m.height))
        };

        let mut widths = Vec::new();
        let mut items: Vec<Item> = Vec::new();
        for sug in suggestions {
            let (layout, tw, th) = text_layout(&sug.text, sug.lang.is_rtl(), font_px, false)?;
            let w = (tw + 2.0 * pad_x).max(40.0 * s);
            widths.push(w);
            items.push(Item { layout, rect: Rf::default(), tw, th });
        }
        let (hint_layout, hint_tw, hint_th) = text_layout(st.hint.as_str(), false, small_px, false)?;
        let chip_w = hint_tw + 14.0 * s;
        // an empty hint means no key cap at all (a short notice, not a list to choose from)
        let no_hint = st.hint.is_empty();
        let hint_w = if no_hint { 0.0 } else { gap + chip_w };
        // the label before the candidates, in the UI language (not the language of the words)
        let lead = match lead_text {
            Some(text) => Some(text_layout(text, false, small_px, true)?),
            None => None,
        };
        let lead_w = lead.as_ref().map_or(0.0, |(_, tw, _)| tw + 14.0 * s);

        let card_w = 2.0 * margin + lead_w + widths.iter().sum::<f32>() + hint_w;
        let card_h = item_h + 2.0 * margin;

        // inside the card: the first (best) candidate is on the right for Arabic, on the left otherwise
        let lead_rect = if rtl_layout {
            Rf { l: card_w - margin - lead_w, t: margin, r: card_w - margin, b: margin + item_h }
        } else {
            Rf { l: margin, t: margin, r: margin + lead_w, b: margin + item_h }
        };
        let mut cursor = if rtl_layout { card_w - margin - lead_w } else { margin + lead_w };
        for (it, w) in items.iter_mut().zip(&widths) {
            if rtl_layout {
                it.rect = Rf { l: cursor - w, t: margin, r: cursor, b: margin + item_h };
                cursor -= w;
            } else {
                it.rect = Rf { l: cursor, t: margin, r: cursor + w, b: margin + item_h };
                cursor += w;
            }
        }
        let chip_h = 20.0 * s;
        let chip_top = (card_h - chip_h) / 2.0;
        let (chip, divider_x) = if rtl_layout {
            (Rf { l: cursor - gap - chip_w, t: chip_top, r: cursor - gap, b: chip_top + chip_h }, cursor - gap / 2.0)
        } else {
            (Rf { l: cursor + gap, t: chip_top, r: cursor + gap + chip_w, b: chip_top + chip_h }, cursor + gap / 2.0)
        };

        // on the screen: above the anchor; below it when the top is in the way; never off the screen
        let gap_y = 6.0 * s;
        let mut card_top = anchor.rect.top as f32 - gap_y - card_h;
        if card_top < work.top as f32 {
            card_top = anchor.rect.bottom as f32 + gap_y;
        }
        // an Arabic popup hangs from the caret's right side; with only the field known, it starts at the field's left
        let mut card_left =
            if rtl_layout && anchor.kind == AnchorKind::Caret { anchor.rect.right as f32 - card_w } else { anchor.rect.left as f32 };
        card_left = card_left.min(work.right as f32 - card_w).max(work.left as f32);

        // the window is the card plus room for the shadow all around
        let (w_px, h_px) = ((card_w + 2.0 * shadow).ceil() as i32, (card_h + 2.0 * shadow).ceil() as i32);
        let pos = POINT { x: (card_left - shadow).round() as i32, y: (card_top - shadow).round() as i32 };
        let card = Rf { l: shadow, t: shadow, r: shadow + card_w, b: shadow + card_h };
        for it in &mut items {
            it.rect = it.rect.shift(shadow, shadow);
        }
        let chip = chip.shift(shadow, shadow);
        let divider_x = divider_x + shadow;
        let lead = lead.map(|(layout, tw, th)| Lead { layout, rect: lead_rect.shift(shadow, shadow), tw, th });

        // the picture to draw into
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(Some(screen));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w_px,
                biHeight: -h_px, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(Some(screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0);
        ReleaseDC(None, screen);
        let bmp = bmp?;
        let old = SelectObject(dc, bmp.into());
        let dib = Dib { dc, bmp, old };

        let first_show = !st.visible;
        let keep_hover = st.shown.as_ref().and_then(|p| p.hover).filter(|i| *i < items.len());
        let mut shown = Shown {
            items,
            hint: if no_hint { None } else { Some(Hint { layout: hint_layout, chip, divider_x, tw: hint_tw, th: hint_th }) },
            lead,
            selected: selected.min(suggestions.len() - 1),
            hover: keep_hover,
            w: w_px,
            h: h_px,
            pos,
            card,
            look,
            scale: s,
            dib,
            alpha: if first_show { 0 } else { 255 },
            tracking: false,
        };
        render(&st.gfx, &mut shown)?;
        present(hwnd, &shown)?;
        st.shown = Some(shown);
        st.visible = true;
        if first_show {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            SetTimer(Some(hwnd), FADE_TIMER, 15, None);
        }
        Ok(())
    }
}

// ───────────── drawing ─────────────

fn rounded(r: Rf, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: radius, radiusY: radius }
}

fn render(gfx: &Gfx, sh: &mut Shown) -> windows::core::Result<()> {
    unsafe {
        let t = &gfx.target;
        let p = palette(sh.look);
        let s = sh.scale;
        t.BindDC(sh.dib.dc, &RECT { left: 0, top: 0, right: sh.w, bottom: sh.h })?;
        t.BeginDraw();
        t.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));
        let brush: ID2D1SolidColorBrush = t.CreateSolidColorBrush(&rgba(0.0, 0.0, 0.0, 1.0), None)?;
        let radius = 9.0 * s;

        // a soft shadow: stacked, nearly transparent rounded rectangles, a little lower than the card
        let shadow_alpha = if sh.look.theme == Theme::Dark { 0.04 } else { 0.02 };
        for i in 0..SHADOW_LAYERS {
            let grow = (SHADOW_LAYERS - i) as f32 * 1.25 * s;
            brush.SetColor(&rgba(0.0, 0.0, 0.0, shadow_alpha));
            t.FillRoundedRectangle(&rounded(sh.card.grow(grow).shift(0.0, 3.0 * s), radius + grow), &brush);
        }

        // the card and its hairline border
        brush.SetColor(&p.bg);
        t.FillRoundedRectangle(&rounded(sh.card, radius), &brush);
        brush.SetColor(&p.border);
        t.DrawRoundedRectangle(&rounded(sh.card.grow(-0.5), radius - 0.5), &brush, 1.0, None);

        // the candidates
        for (i, it) in sh.items.iter().enumerate() {
            let is_sel = i == sh.selected;
            if is_sel {
                brush.SetColor(&p.accent);
                t.FillRoundedRectangle(&rounded(it.rect, 6.5 * s), &brush);
            } else if sh.hover == Some(i) {
                brush.SetColor(&p.hover);
                t.FillRoundedRectangle(&rounded(it.rect, 6.5 * s), &brush);
            }
            brush.SetColor(if is_sel { &p.on_accent } else { &p.text });
            let x = it.rect.l + (it.rect.r - it.rect.l - it.tw) / 2.0;
            let y = it.rect.t + (it.rect.b - it.rect.t - it.th) / 2.0;
            t.DrawTextLayout(Vector2::new(x, y), &it.layout, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
        }

        // the label ("Fix") in the accent colour, so a correction does not look like a completion
        if let Some(l) = &sh.lead {
            brush.SetColor(&p.accent);
            let x = l.rect.l + (l.rect.r - l.rect.l - l.tw) / 2.0;
            let y = l.rect.t + (l.rect.b - l.rect.t - l.th) / 2.0;
            t.DrawTextLayout(Vector2::new(x, y), &l.layout, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
        }

        // the "Tab" hint: a thin divider and a small key cap
        if let Some(h) = &sh.hint {
            let mut line = p.muted;
            line.a = 0.35;
            brush.SetColor(&line);
            let (top, bottom) = (sh.card.t + 9.0 * s, sh.card.b - 9.0 * s);
            t.FillRectangle(&D2D_RECT_F { left: h.divider_x - 0.5, top, right: h.divider_x + 0.5, bottom }, &brush);
            line.a = 0.55;
            brush.SetColor(&line);
            t.DrawRoundedRectangle(&rounded(h.chip.grow(-0.5), 5.0 * s), &brush, 1.0, None);
            brush.SetColor(&p.muted);
            let x = h.chip.l + (h.chip.r - h.chip.l - h.tw) / 2.0;
            let y = h.chip.t + (h.chip.b - h.chip.t - h.th) / 2.0;
            t.DrawTextLayout(Vector2::new(x, y), &h.layout, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
        }
        t.EndDraw(None, None)
    }
}

/// Hands the drawn picture to Windows at the right place, with its transparency and the current fade level.
fn present(hwnd: HWND, sh: &Shown) -> windows::core::Result<()> {
    unsafe {
        let size = SIZE { cx: sh.w, cy: sh.h };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: sh.alpha, AlphaFormat: AC_SRC_ALPHA as u8 };
        UpdateLayeredWindow(hwnd, None, Some(&sh.pos), Some(&size), Some(sh.dib.dc), Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA)
    }
}

// ───────────── messages ─────────────

fn item_at(sh: &Shown, x: f32, y: f32) -> Option<usize> {
    sh.items.iter().position(|it| it.rect.contains(x, y))
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        // clicking the popup must not take the focus from the app being typed in
        WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE
        WM_SETCURSOR => {
            unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
            LRESULT(1)
        }
        WM_TIMER if wp.0 == FADE_TIMER => {
            with_state(
                |st, hwnd| {
                    if let Some(sh) = st.shown.as_mut() {
                        sh.alpha = sh.alpha.saturating_add(46);
                        let _ = present(hwnd, sh);
                        if sh.alpha == 255 {
                            unsafe {
                                let _ = KillTimer(Some(hwnd), FADE_TIMER);
                            }
                        }
                    }
                },
                hwnd,
            );
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = ((lp.0 & 0xFFFF) as i16 as f32, ((lp.0 >> 16) & 0xFFFF) as i16 as f32);
            with_state(
                |st, hwnd| {
                    let State { gfx, shown, .. } = st;
                    if let Some(sh) = shown.as_mut() {
                        if !sh.tracking {
                            sh.tracking = true;
                            let mut tme = TRACKMOUSEEVENT {
                                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                                dwFlags: TME_LEAVE,
                                hwndTrack: hwnd,
                                dwHoverTime: 0,
                            };
                            unsafe {
                                let _ = TrackMouseEvent(&mut tme);
                            }
                        }
                        let over = item_at(sh, x, y);
                        if over != sh.hover {
                            sh.hover = over;
                            let _ = render(gfx, sh);
                            let _ = present(hwnd, sh);
                        }
                    }
                },
                hwnd,
            );
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_state(
                |st, hwnd| {
                    let State { gfx, shown, .. } = st;
                    if let Some(sh) = shown.as_mut() {
                        sh.tracking = false;
                        if sh.hover.is_some() {
                            sh.hover = None;
                            let _ = render(gfx, sh);
                            let _ = present(hwnd, sh);
                        }
                    }
                },
                hwnd,
            );
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = ((lp.0 & 0xFFFF) as i16 as f32, ((lp.0 >> 16) & 0xFFFF) as i16 as f32);
            let hit = with_state(|st, _| st.shown.as_ref().and_then(|sh| item_at(sh, x, y)).zip(st.notify), hwnd).flatten();
            if let Some((index, (target, message))) = hit {
                unsafe {
                    let _ = PostMessageW(Some(target), message, WPARAM(index), LPARAM(0));
                }
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}
