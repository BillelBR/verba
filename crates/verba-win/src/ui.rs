//! A small immediate-mode UI toolkit drawn with Direct2D, for Verba's settings window.
//!
//! Every frame the caller's `draw` function runs from top to bottom and says what is on the page ("a toggle row with this title");
//! the toolkit lays it out, draws it, and tells the caller what was clicked. Nothing is kept by the widgets themselves: the caller owns
//! the values. Actions the caller wants to run (saving settings, opening a menu) are handed to `Ui::defer` and run after the frame,
//! outside the toolkit's own state, so they may even run a menu loop of their own.
//!
//! What it gives: a sidebar with pages, cards with rows, toggle switches that slide, buttons, segmented choices, theme tiles with a
//! small preview, a list, light and dark palettes, mouse and keyboard (Tab, Space, Enter, arrows, wheel), a visible focus ring.
//! It does not provide a screen-reader tree: that is the price of drawing everything ourselves.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_SIZE_U, D2D1_ALPHA_MODE_UNKNOWN, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_ANTIALIAS_MODE_ALIASED, D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE,
    D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_GAMMA_2_2, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_LINE_JOIN_ROUND, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1CreateFactory,
    ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush, ID2D1StrokeStyle,
};
use windows::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD,
    DWRITE_TEXT_METRICS, DWriteCreateFactory, IDWriteFactory, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, SetProcessDpiAwarenessContext};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_LEFT,
    VK_MENU, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetSystemMetrics, HICON, IDC_ARROW, IDC_HAND, IsWindow, KillTimer, LoadCursorW,
    RegisterClassW, SM_CXSCREEN, SM_CYSCREEN, SW_SHOW, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetCursor, SetForegroundWindow, SetTimer,
    SetWindowPos, ShowWindow, WINDOW_EX_STYLE, WM_CLOSE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_PAINT, WM_SETCURSOR, WM_SIZE, WM_SYSCHAR, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSW, WS_CAPTION, WS_MINIMIZEBOX, WS_OVERLAPPED, WS_SYSMENU,
};
use windows::core::{BOOL, PCWSTR, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::look::{Look, Theme};

const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_MOUSEWHEEL: u32 = 0x020A;
const WM_DPICHANGED: u32 = 0x02E0;
const TIMER_ANIM: usize = 7;

/// Width of the sidebar and the margins of the page, in logical pixels.
pub const SIDEBAR_W: f32 = 232.0;
const PAD: f32 = 36.0;
const CARD_RADIUS: f32 = 8.0;
const ROW_MIN: f32 = 58.0;
const ROW_PAD_X: f32 = 18.0;

// ───────────── colours ─────────────

type Col = D2D1_COLOR_F;

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Col {
    D2D1_COLOR_F { r, g, b, a }
}

fn hex(v: u32, a: f32) -> Col {
    rgba(((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0, a)
}

fn mix(a: Col, b: Col, t: f32) -> Col {
    let t = t.clamp(0.0, 1.0);
    rgba(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t)
}

fn with_alpha(c: Col, a: f32) -> Col {
    rgba(c.r, c.g, c.b, c.a * a)
}

#[derive(Clone, Copy)]
struct Palette {
    dark: bool,
    bg: Col,
    sidebar: Col,
    card: Col,
    card_hover: Col,
    border: Col,
    text: Col,
    muted: Col,
    hover: Col,
    accent: Col,
    on_accent: Col,
    danger: Col,
    field: Col,
}

fn palette(look: Look) -> Palette {
    let [ar, ag, ab] = look.accent;
    let accent = rgba(ar as f32 / 255.0, ag as f32 / 255.0, ab as f32 / 255.0, 1.0);
    let lum = 0.299 * accent.r + 0.587 * accent.g + 0.114 * accent.b;
    let on_accent = if lum > 0.62 { rgba(0.07, 0.07, 0.07, 1.0) } else { rgba(1.0, 1.0, 1.0, 1.0) };
    match look.theme {
        Theme::Light => Palette {
            dark: false,
            bg: hex(0xF3F3F3, 1.0),
            sidebar: hex(0xEAEAEA, 1.0),
            card: hex(0xFFFFFF, 1.0),
            card_hover: hex(0xFAFAFA, 1.0),
            border: hex(0xE2E2E2, 1.0),
            text: hex(0x1A1A1A, 1.0),
            muted: hex(0x5F5F5F, 1.0),
            hover: rgba(0.0, 0.0, 0.0, 0.055),
            accent,
            on_accent,
            danger: hex(0xC42B1C, 1.0),
            field: hex(0xF1F1F1, 1.0),
        },
        Theme::Dark => Palette {
            dark: true,
            bg: hex(0x202020, 1.0),
            sidebar: hex(0x181818, 1.0),
            card: hex(0x2B2B2B, 1.0),
            card_hover: hex(0x323232, 1.0),
            border: hex(0x3C3C3C, 1.0),
            text: hex(0xFFFFFF, 1.0),
            muted: hex(0xA8A8A8, 1.0),
            hover: rgba(1.0, 1.0, 1.0, 0.07),
            accent,
            on_accent,
            danger: hex(0xFF7B72, 1.0),
            field: hex(0x383838, 1.0),
        },
    }
}

// ───────────── geometry ─────────────

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl R {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
    fn inset(&self, d: f32) -> R {
        R::new(self.x + d, self.y + d, self.w - 2.0 * d, self.h - 2.0 * d)
    }
    fn d2d(&self) -> D2D_RECT_F {
        D2D_RECT_F { left: self.x, top: self.y, right: self.right(), bottom: self.bottom() }
    }
}

// ───────────── what a frame draws ─────────────

enum Cmd {
    Nop,
    Rect { r: R, radius: f32, fill: Option<Col>, stroke: Option<(Col, f32)> },
    Circle { c: (f32, f32), r: f32, fill: Option<Col>, stroke: Option<(Col, f32)> },
    Text { layout: IDWriteTextLayout, x: f32, y: f32, color: Col },
    /// The logo: the blue disc with the white V.
    Logo { c: (f32, f32), r: f32 },
    Clip(R),
    Unclip,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Font {
    Text,
    Semibold,
    Icons,
}

type Layout = (IDWriteTextLayout, f32, f32);

/// One entry of `Ui::entry_list`: an optional tag, a title and an optional detail line.
#[derive(Clone, Debug, Default)]
pub struct ListEntry {
    pub tag: String,
    pub title: String,
    pub detail: String,
}

/// How a button looks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    Primary,
    Secondary,
    Danger,
}

/// What a key row tells back.
#[derive(Clone, Copy, Default)]
pub struct KeyRow {
    /// The user pressed a key (virtual key code, modifiers: 1 Ctrl, 2 Alt, 4 Shift) while the row waited for one.
    pub captured: Option<(u32, u8)>,
    /// The user pressed the button that puts the original key back.
    pub reset: bool,
}

/// What a widget tells back.
#[derive(Clone, Copy, Default)]
pub struct Resp {
    pub hover: bool,
    pub down: bool,
    pub clicked: bool,
    pub focused: bool,
}

/// A hash of a widget's name: what the toolkit remembers things by.
pub fn id_of(name: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in name.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ───────────── input and persistent state ─────────────

#[derive(Default)]
struct Input {
    mouse: Option<(f32, f32)>,
    pressed: bool,
    released: bool,
    down: bool,
    wheel: f32,
    tab: i32,
    activate: bool,
    left: bool,
    right: bool,
    up: bool,
    down_key: bool,
    /// A key pressed while a key row was waiting for one (virtual key code, modifiers: 1 Ctrl, 2 Alt, 4 Shift).
    captured: Option<(u32, u8)>,
    /// Esc was pressed while a key row was waiting: give up.
    capture_cancel: bool,
}

struct UiState {
    page: usize,
    hover: Option<u64>,
    active: Option<u64>,
    focus: Option<u64>,
    focus_visible: bool,
    order: Vec<u64>,
    order_next: Vec<u64>,
    anim: HashMap<u64, f32>,
    animating: bool,
    scroll: f32,
    content_h: f32,
    view_h: f32,
    /// The scroll bar is being dragged: how far below the top of its thumb the pointer took hold.
    bar_drag: Option<f32>,
    /// The pointer is over the scroll bar (or drags it): it is drawn wider.
    bar_hover: bool,
    hand: bool,
    reveal: Option<R>,
    /// The key row waiting for the user to press a key.
    capture: Option<u64>,
    last: Instant,
}

impl UiState {
    fn new() -> Self {
        Self {
            page: 0,
            hover: None,
            active: None,
            focus: None,
            focus_visible: false,
            order: Vec::new(),
            order_next: Vec::new(),
            anim: HashMap::new(),
            animating: false,
            scroll: 0.0,
            content_h: 0.0,
            view_h: 0.0,
            bar_drag: None,
            bar_hover: false,
            hand: false,
            reveal: None,
            capture: None,
            last: Instant::now(),
        }
    }
}

struct Gfx {
    dwrite: IDWriteFactory,
    cache: HashMap<(String, u32, Font, u32), Layout>,
}

impl Gfx {
    fn layout(&mut self, text: &str, size: f32, font: Font, max_w: f32) -> Layout {
        let key = (text.to_string(), (size * 10.0) as u32, font, max_w.min(20000.0) as u32);
        if let Some(l) = self.cache.get(&key) {
            return l.clone();
        }
        let made = unsafe {
            let (family, weight) = match font {
                Font::Text => (w!("Segoe UI"), DWRITE_FONT_WEIGHT_NORMAL),
                Font::Semibold => (w!("Segoe UI"), DWRITE_FONT_WEIGHT_SEMI_BOLD),
                Font::Icons => (w!("Segoe MDL2 Assets"), DWRITE_FONT_WEIGHT_NORMAL),
            };
            let fmt = self.dwrite.CreateTextFormat(family, None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!("en-us")).ok();
            fmt.and_then(|fmt| {
                let wide: Vec<u16> = text.encode_utf16().collect();
                let layout = self.dwrite.CreateTextLayout(&wide, &fmt, max_w, 4000.0).ok()?;
                let mut m = DWRITE_TEXT_METRICS::default();
                layout.GetMetrics(&mut m).ok()?;
                Some((layout, m.width, m.height))
            })
        };
        match made {
            Some(l) => {
                if self.cache.len() > 600 {
                    self.cache.clear();
                }
                self.cache.insert(key, l.clone());
                l
            }
            None => panic!("DirectWrite could not make a text layout"),
        }
    }
}

// ───────────── the frame ─────────────

struct Card {
    first: bool,
    start_y: f32,
    bg: usize,
}

/// One frame's drawing context, handed to the caller's `draw` function.
pub struct Ui<'a> {
    g: &'a mut Gfx,
    s: &'a mut UiState,
    input: &'a Input,
    p: Palette,
    side: Vec<Cmd>,
    main: Vec<Cmd>,
    in_side: bool,
    width: f32,
    height: f32,
    // layout cursor of the page
    cy: f32,
    card: Option<Card>,
    sy: f32,
    deferred: Vec<Box<dyn FnOnce()>>,
    close: bool,
}

impl<'a> Ui<'a> {
    // ----- plumbing -----

    fn push(&mut self, c: Cmd) {
        if self.in_side { self.side.push(c) } else { self.main.push(c) }
    }

    /// Runs `f` after the frame, outside the toolkit's state (so it may open a menu, show a box, save a file).
    pub fn defer(&mut self, f: impl FnOnce() + 'static) {
        self.deferred.push(Box::new(f));
    }

    /// Asks the window to close after this frame.
    pub fn close_window(&mut self) {
        self.close = true;
    }

    /// The page chosen in the sidebar.
    pub fn page(&self) -> usize {
        self.s.page
    }

    pub fn is_dark(&self) -> bool {
        self.p.dark
    }

    fn text(&mut self, x: f32, y: f32, s: &str, size: f32, font: Font, color: Col, max_w: f32) -> (f32, f32) {
        let (layout, w, h) = self.g.layout(s, size, font, max_w);
        self.push(Cmd::Text { layout, x, y, color });
        (w, h)
    }

    fn measure(&mut self, s: &str, size: f32, font: Font, max_w: f32) -> (f32, f32) {
        let (_, w, h) = self.g.layout(s, size, font, max_w);
        (w, h)
    }

    fn glyph(&mut self, rect: R, ch: char, size: f32, color: Col) {
        let (layout, w, h) = self.g.layout(&ch.to_string(), size, Font::Icons, 200.0);
        self.push(Cmd::Text { layout, x: rect.x + (rect.w - w) / 2.0, y: rect.y + (rect.h - h) / 2.0, color });
    }

    fn pointer_inside(&self, r: R) -> bool {
        let Some((mx, my)) = self.input.mouse else { return false };
        if self.in_side {
            r.contains(mx, my)
        } else {
            mx >= SIDEBAR_W && my >= 0.0 && my < self.height && r.contains(mx, my + self.s.scroll)
        }
    }

    /// Eases a remembered number toward `target` (a toggle sliding, a highlight fading).
    fn anim(&mut self, id: u64, target: f32, seconds: f32) -> f32 {
        let dt = self.dt();
        let v = self.s.anim.entry(id).or_insert(target);
        let step = dt / seconds.max(0.01);
        if (*v - target).abs() <= step {
            *v = target;
        } else {
            *v += step * (target - *v).signum();
            self.s.animating = true;
        }
        *v
    }

    fn dt(&self) -> f32 {
        // the time of this frame, shared by every widget (set once per frame by `paint`)
        FRAME_DT.with(|d| d.get())
    }

    /// The common part of every clickable thing.
    fn hit(&mut self, id: u64, r: R, enabled: bool) -> Resp {
        let inside = enabled && self.pointer_inside(r);
        if enabled {
            self.s.order_next.push(id);
        }
        if inside {
            self.s.hover = Some(id);
            self.s.hand = true;
        }
        let mut clicked = false;
        if inside && self.input.pressed {
            self.s.active = Some(id);
            self.s.focus = Some(id);
            self.s.focus_visible = false;
        }
        if self.input.released && self.s.active == Some(id) && inside {
            clicked = true;
        }
        let focused = self.s.focus == Some(id);
        if focused && enabled && self.input.activate {
            clicked = true;
        }
        if focused && self.s.focus_visible && !self.in_side {
            self.s.reveal = Some(r);
        }
        Resp { hover: inside, down: inside && self.s.active == Some(id) && self.input.down, clicked, focused }
    }

    fn focus_ring(&mut self, r: R, radius: f32) {
        self.push(Cmd::Rect { r: r.inset(-3.0), radius: radius + 3.0, fill: None, stroke: Some((self.p.accent, 2.0)) });
    }

    // ----- the sidebar -----

    /// Starts the sidebar: its background, the logo, the name and the version.
    pub fn sidebar_begin(&mut self, name: &str, subtitle: &str) {
        self.in_side = true;
        let (sw, h) = (SIDEBAR_W, self.height);
        let p = self.p;
        self.push(Cmd::Rect { r: R::new(0.0, 0.0, sw, h), radius: 0.0, fill: Some(p.sidebar), stroke: None });
        self.push(Cmd::Rect { r: R::new(sw - 1.0, 0.0, 1.0, h), radius: 0.0, fill: Some(p.border), stroke: None });
        self.push(Cmd::Logo { c: (44.0, 46.0), r: 18.0 });
        self.text(70.0, 28.0, name, 18.0, Font::Semibold, p.text, 140.0);
        self.text(70.0, 51.0, subtitle, 12.0, Font::Text, p.muted, 150.0);
        self.sy = 92.0;
    }

    /// A page of the sidebar: icon and label. Returns true when it was chosen.
    pub fn nav_item(&mut self, name: &str, glyph: char, label: &str, index: usize) -> bool {
        let id = id_of(name);
        let r = R::new(12.0, self.sy, SIDEBAR_W - 24.0, 40.0);
        self.sy += 44.0;
        let resp = self.hit(id, r, true);
        let selected = self.s.page == index;
        let p = self.p;
        let a = self.anim(id, if selected { 1.0 } else if resp.hover { 0.5 } else { 0.0 }, 0.12);
        if a > 0.0 {
            let fill = if selected { with_alpha(p.hover, 1.6) } else { with_alpha(p.hover, a * 2.0) };
            self.push(Cmd::Rect { r, radius: 6.0, fill: Some(fill), stroke: None });
        }
        if selected {
            self.push(Cmd::Rect { r: R::new(r.x, r.y + 11.0, 3.0, 18.0), radius: 1.5, fill: Some(p.accent), stroke: None });
        }
        let icon_color = if selected { p.accent } else { p.text };
        self.glyph(R::new(r.x + 12.0, r.y, 22.0, r.h), glyph, 16.0, icon_color);
        self.text(r.x + 46.0, r.y + 10.0, label, 14.0, if selected { Font::Semibold } else { Font::Text }, p.text, r.w - 52.0);
        if resp.focused && self.s.focus_visible {
            self.focus_ring(r, 6.0);
        }
        if resp.clicked {
            self.s.page = index;
            self.s.scroll = 0.0;
            self.s.reveal = None;
            // the sidebar was drawn before the click was known: draw the frame again with the new page
            self.s.animating = true;
        }
        resp.clicked
    }

    /// The master switch at the foot of the sidebar: a line of text and a toggle.
    pub fn sidebar_switch(&mut self, name: &str, label: &str, status: &str, value: &mut bool) -> bool {
        let id = id_of(name);
        let h = self.height;
        let p = self.p;
        let r = R::new(12.0, h - 78.0, SIDEBAR_W - 24.0, 62.0);
        let resp = self.hit(id, r, true);
        let a = self.anim(id ^ 0x5a5a, if resp.hover { 1.0 } else { 0.0 }, 0.12);
        self.push(Cmd::Rect { r, radius: 8.0, fill: Some(mix(p.card, p.card_hover, a)), stroke: Some((p.border, 1.0)) });
        self.text(r.x + 14.0, r.y + 11.0, label, 14.0, Font::Semibold, p.text, 120.0);
        let status_color = if *value { p.accent } else { p.muted };
        self.text(r.x + 14.0, r.y + 33.0, status, 12.0, Font::Text, status_color, 120.0);
        self.toggle_switch(id, R::new(r.right() - 14.0 - 44.0, r.y + 20.0, 44.0, 22.0), *value, true);
        if resp.focused && self.s.focus_visible {
            self.focus_ring(r, 8.0);
        }
        if resp.clicked {
            *value = !*value;
            return true;
        }
        false
    }

    // ----- the page -----

    /// Starts the page: title and a line under it.
    pub fn page_begin(&mut self, title: &str, subtitle: &str) {
        self.in_side = false;
        let p = self.p;
        self.cy = 30.0;
        let x = SIDEBAR_W + PAD;
        let w = self.width - SIDEBAR_W - 2.0 * PAD;
        let (_, th) = self.text(x, self.cy, title, 26.0, Font::Semibold, p.text, w);
        self.cy += th + 4.0;
        if !subtitle.is_empty() {
            let (_, sh) = self.text(x, self.cy, subtitle, 13.0, Font::Text, p.muted, w);
            self.cy += sh;
        }
        self.cy += 22.0;
    }

    fn content_x(&self) -> f32 {
        SIDEBAR_W + PAD
    }

    fn content_w(&self) -> f32 {
        self.width - SIDEBAR_W - 2.0 * PAD
    }

    /// A small heading above a card.
    pub fn section(&mut self, label: &str) {
        let p = self.p;
        let (x, w) = (self.content_x(), self.content_w());
        let (_, h) = self.text(x + 2.0, self.cy, label, 13.0, Font::Semibold, p.muted, w);
        self.cy += h + 8.0;
    }

    /// Starts a card: a rounded panel that holds rows.
    pub fn card_begin(&mut self) {
        self.main.push(Cmd::Nop);
        self.card = Some(Card { first: true, start_y: self.cy, bg: self.main.len() - 1 });
    }

    pub fn card_end(&mut self) {
        let Some(card) = self.card.take() else { return };
        let r = R::new(self.content_x(), card.start_y, self.content_w(), self.cy - card.start_y);
        let p = self.p;
        self.main[card.bg] = Cmd::Rect { r, radius: CARD_RADIUS, fill: Some(p.card), stroke: Some((p.border, 1.0)) };
        self.cy += 22.0;
    }

    /// Lays out the next row of the card: returns its rectangle and puts the title and description in it.
    fn row(&mut self, title: &str, desc: Option<&str>, ctrl_w: f32, min_h: f32) -> R {
        let p = self.p;
        let (x, w) = (self.content_x(), self.content_w());
        let text_w = (w - 2.0 * ROW_PAD_X - ctrl_w - if ctrl_w > 0.0 { 16.0 } else { 0.0 }).max(120.0);
        let (_, th) = self.measure(title, 14.0, Font::Text, text_w);
        let dh = desc.map_or(0.0, |d| self.measure(d, 12.0, Font::Text, text_w).1 + 3.0);
        let h = (th + dh + 28.0).max(min_h);
        if let Some(card) = &mut self.card {
            if !card.first {
                self.main.push(Cmd::Rect { r: R::new(x + ROW_PAD_X, self.cy, w - 2.0 * ROW_PAD_X, 1.0), radius: 0.0, fill: Some(with_alpha(p.border, 0.8)), stroke: None });
            }
            card.first = false;
        }
        let r = R::new(x, self.cy, w, h);
        let top = r.y + (h - th - dh) / 2.0;
        self.text(x + ROW_PAD_X, top, title, 14.0, Font::Text, p.text, text_w);
        if let Some(d) = desc {
            self.text(x + ROW_PAD_X, top + th + 3.0, d, 12.0, Font::Text, p.muted, text_w);
        }
        self.cy += h;
        r
    }

    fn toggle_switch(&mut self, id: u64, r: R, on: bool, enabled: bool) {
        let p = self.p;
        let a = self.anim(id ^ 0x7007, if on { 1.0 } else { 0.0 }, 0.14);
        let hover = self.s.hover == Some(id) || self.pointer_inside(r.inset(-6.0));
        let alpha = if enabled { 1.0 } else { 0.4 };
        let off_stroke = with_alpha(mix(p.muted, p.text, 0.2), alpha);
        self.push(Cmd::Rect { r, radius: r.h / 2.0, fill: Some(with_alpha(p.accent, a * alpha)), stroke: Some((with_alpha(off_stroke, 1.0 - a), 1.2)) });
        let pad = 4.5;
        let big = if hover && enabled { 1.0 } else { 0.0 };
        let radius = 5.5 + 1.5 * a + big;
        let cx = r.x + pad + radius + (r.w - 2.0 * pad - 2.0 * radius) * a;
        let thumb = mix(off_stroke, p.on_accent, a);
        self.push(Cmd::Circle { c: (cx, r.y + r.h / 2.0), r: radius, fill: Some(thumb), stroke: None });
    }

    /// A row with a title, a description and a toggle switch at the right. Returns true when it was flipped (and `value` changed).
    pub fn toggle_row(&mut self, name: &str, title: &str, desc: Option<&str>, value: &mut bool, enabled: bool) -> bool {
        let id = id_of(name);
        let r = self.row(title, desc, 44.0, ROW_MIN);
        let resp = self.hit(id, r, enabled);
        let p = self.p;
        if resp.hover && enabled {
            let a = self.anim(id ^ 0x11, 1.0, 0.1);
            self.main_background(r, with_alpha(p.hover, 0.6 * a));
        } else {
            self.s.anim.remove(&(id ^ 0x11));
        }
        if !enabled {
            // a disabled row says so: its text is already drawn, dim it with a veil
            self.main_background(r, with_alpha(p.card, 0.45));
        }
        self.toggle_switch(id, R::new(r.right() - ROW_PAD_X - 44.0, r.y + (r.h - 22.0) / 2.0, 44.0, 22.0), *value, enabled);
        if resp.focused && self.s.focus_visible {
            self.focus_ring(r.inset(2.0), 6.0);
        }
        if resp.clicked {
            *value = !*value;
        }
        resp.clicked
    }

    /// Paints over the row area with a translucent fill (hover tint, disabled veil): after the row's own drawing.
    fn main_background(&mut self, r: R, color: Col) {
        self.push(Cmd::Rect { r: r.inset(1.0), radius: 6.0, fill: Some(color), stroke: None });
    }

    fn button_shape(&mut self, id: u64, r: R, label: &str, style: ButtonStyle, enabled: bool) -> Resp {
        let resp = self.hit(id, r, enabled);
        let p = self.p;
        let a = self.anim(id ^ 0x22, if resp.down { 1.0 } else if resp.hover { 0.6 } else { 0.0 }, 0.1);
        let (fill, stroke, text) = match style {
            ButtonStyle::Primary => (mix(p.accent, p.text, a * 0.12), None, p.on_accent),
            ButtonStyle::Secondary => (mix(p.field, mix(p.field, p.text, 0.12), a), Some(p.border), p.text),
            ButtonStyle::Danger => (mix(p.field, mix(p.field, p.danger, 0.18), a), Some(p.border), p.danger),
        };
        let alpha = if enabled { 1.0 } else { 0.45 };
        self.push(Cmd::Rect { r, radius: 6.0, fill: Some(with_alpha(fill, alpha)), stroke: stroke.map(|c| (with_alpha(c, alpha), 1.0)) });
        let (tw, th) = self.measure(label, 13.0, Font::Text, 400.0);
        self.text(r.x + (r.w - tw) / 2.0, r.y + (r.h - th) / 2.0, label, 13.0, Font::Text, with_alpha(text, alpha), 400.0);
        if resp.focused && self.s.focus_visible {
            self.focus_ring(r, 6.0);
        }
        resp
    }

    /// A row with a title, a description and a button at the right. Returns true when the button was pressed.
    pub fn button_row(&mut self, name: &str, title: &str, desc: Option<&str>, label: &str, style: ButtonStyle, enabled: bool) -> bool {
        let (bw, _) = self.measure(label, 13.0, Font::Text, 400.0);
        let btn_w = (bw + 34.0).max(96.0);
        let r = self.row(title, desc, btn_w, ROW_MIN);
        let b = R::new(r.right() - ROW_PAD_X - btn_w, r.y + (r.h - 32.0) / 2.0, btn_w, 32.0);
        self.button_shape(id_of(name), b, label, style, enabled).clicked
    }

    /// A button on its own line inside a card, or at a given place: returns true when pressed.
    pub fn button_at(&mut self, name: &str, r: R, label: &str, style: ButtonStyle) -> bool {
        self.button_shape(id_of(name), r, label, style, true).clicked
    }

    /// A row with a title and a segmented choice below it. `value` is the index chosen. Returns true when it changed.
    pub fn segmented_row(&mut self, name: &str, title: &str, desc: Option<&str>, options: &[&str], value: &mut usize) -> bool {
        let r = self.row(title, desc, 0.0, 0.0);
        let p = self.p;
        let (x, w) = (r.x + ROW_PAD_X, r.w - 2.0 * ROW_PAD_X);
        let seg_h = 36.0;
        let back = R::new(x, self.cy - 2.0, w, seg_h);
        self.push(Cmd::Rect { r: back, radius: 8.0, fill: Some(p.field), stroke: Some((p.border, 1.0)) });
        let n = options.len().max(1);
        let seg_w = (w - 8.0) / n as f32;
        let mut changed = false;
        let group = id_of(name);
        // arrow keys move inside the group when one of its parts has the focus
        let focus_in = (0..n).any(|i| self.s.focus == Some(group.wrapping_add(i as u64 + 1)));
        if focus_in {
            if self.input.right && *value + 1 < n {
                *value += 1;
                changed = true;
                self.s.focus = Some(group.wrapping_add(*value as u64 + 1));
            }
            if self.input.left && *value > 0 {
                *value -= 1;
                changed = true;
                self.s.focus = Some(group.wrapping_add(*value as u64 + 1));
            }
        }
        for (i, label) in options.iter().enumerate() {
            let id = group.wrapping_add(i as u64 + 1);
            let seg = R::new(back.x + 4.0 + seg_w * i as f32, back.y + 4.0, seg_w, seg_h - 8.0);
            let resp = self.hit(id, seg, true);
            let selected = *value == i;
            let a = self.anim(id ^ 0x33, if selected { 1.0 } else if resp.hover { 0.4 } else { 0.0 }, 0.12);
            if a > 0.0 {
                let fill = if selected { mix(p.field, p.card, a) } else { with_alpha(p.hover, a) };
                let stroke = if selected { Some((with_alpha(p.border, a), 1.0)) } else { None };
                self.push(Cmd::Rect { r: seg, radius: 6.0, fill: Some(fill), stroke });
            }
            let (tw, th) = self.measure(label, 13.0, Font::Text, seg_w);
            self.text(seg.x + (seg.w - tw) / 2.0, seg.y + (seg.h - th) / 2.0, label, 13.0, if selected { Font::Semibold } else { Font::Text }, p.text, seg_w);
            if resp.focused && self.s.focus_visible {
                self.focus_ring(seg, 6.0);
            }
            if resp.clicked && !selected {
                *value = i;
                changed = true;
            }
        }
        self.cy += seg_h + 14.0;
        changed
    }

    /// A row of theme tiles (each with a small preview of the popup): follow Windows, light, dark. Returns true when it changed.
    pub fn theme_tiles(&mut self, name: &str, labels: [&str; 3], value: &mut usize) -> bool {
        let p = self.p;
        let (x, w) = (self.content_x() + ROW_PAD_X, self.content_w() - 2.0 * ROW_PAD_X);
        let gap = 14.0;
        let tw = (w - 2.0 * gap) / 3.0;
        let (pv_h, label_h) = (92.0, 30.0);
        let top = self.cy + 18.0;
        let mut changed = false;
        let group = id_of(name);
        let focus_in = (0..3).any(|i| self.s.focus == Some(group.wrapping_add(i as u64 + 1)));
        if focus_in {
            if self.input.right && *value < 2 {
                *value += 1;
                changed = true;
                self.s.focus = Some(group.wrapping_add(*value as u64 + 1));
            }
            if self.input.left && *value > 0 {
                *value -= 1;
                changed = true;
                self.s.focus = Some(group.wrapping_add(*value as u64 + 1));
            }
        }
        for i in 0..3 {
            let id = group.wrapping_add(i as u64 + 1);
            let tile = R::new(x + (tw + gap) * i as f32, top, tw, pv_h + label_h);
            let resp = self.hit(id, tile, true);
            let selected = *value == i;
            let a = self.anim(id ^ 0x44, if selected { 1.0 } else if resp.hover { 0.5 } else { 0.0 }, 0.12);
            let preview = R::new(tile.x, tile.y, tile.w, pv_h);
            self.theme_preview(preview, i);
            let stroke = mix(p.border, p.accent, a);
            self.push(Cmd::Rect { r: preview, radius: 8.0, fill: None, stroke: Some((stroke, 1.0 + a)) });
            // the label with a round mark that fills when chosen
            let ly = tile.y + pv_h + 8.0;
            let mark = (tile.x + 10.0, ly + 8.0);
            self.push(Cmd::Circle { c: mark, r: 7.0, fill: None, stroke: Some((mix(p.muted, p.accent, a), 1.3)) });
            if a > 0.01 {
                self.push(Cmd::Circle { c: mark, r: 3.5 * a, fill: Some(p.accent), stroke: None });
            }
            self.text(tile.x + 26.0, ly, labels[i], 13.0, if selected { Font::Semibold } else { Font::Text }, p.text, tile.w - 30.0);
            if resp.focused && self.s.focus_visible {
                self.focus_ring(preview, 8.0);
            }
            if resp.clicked && !selected {
                *value = i;
                changed = true;
            }
        }
        self.cy = top + pv_h + label_h + 16.0;
        changed
    }

    /// A tiny picture of the popup in a theme (0: split light and dark, 1: light, 2: dark).
    fn theme_preview(&mut self, r: R, which: usize) {
        let draw_one = |ui: &mut Ui, area: R, dark: bool| {
            let (bg, card, text, chip) = if dark {
                (hex(0x1F1F1F, 1.0), hex(0x2D2D30, 1.0), hex(0xF2F2F2, 1.0), hex(0x3A3A3E, 1.0))
            } else {
                (hex(0xEDEDED, 1.0), hex(0xFFFFFF, 1.0), hex(0x222222, 1.0), hex(0xE6E6E9, 1.0))
            };
            let accent = ui.p.accent;
            ui.push(Cmd::Rect { r: area, radius: 8.0, fill: Some(bg), stroke: None });
            let c = R::new(r.x + r.w / 2.0 - 52.0, r.y + 26.0, 104.0, 34.0);
            ui.push(Cmd::Rect { r: c, radius: 7.0, fill: Some(card), stroke: Some((with_alpha(text, 0.14), 1.0)) });
            let chip_w = 28.0;
            ui.push(Cmd::Rect { r: R::new(c.x + 6.0, c.y + 6.0, chip_w, 22.0), radius: 5.0, fill: Some(accent), stroke: None });
            ui.push(Cmd::Rect { r: R::new(c.x + 42.0, c.y + 13.0, 20.0, 8.0), radius: 4.0, fill: Some(with_alpha(text, 0.55)), stroke: None });
            ui.push(Cmd::Rect { r: R::new(c.x + 70.0, c.y + 13.0, 22.0, 8.0), radius: 4.0, fill: Some(with_alpha(text, 0.55)), stroke: None });
            let _ = chip;
            ui.push(Cmd::Rect { r: R::new(r.x + 22.0, r.y + 12.0, 34.0, 6.0), radius: 3.0, fill: Some(with_alpha(text, 0.25)), stroke: None });
        };
        self.push(Cmd::Clip(r));
        match which {
            0 => {
                // the whole tile in light, then the right half again in dark
                draw_one(self, r, false);
                self.push(Cmd::Clip(R::new(r.x + r.w / 2.0, r.y, r.w / 2.0, r.h)));
                draw_one(self, r, true);
                self.push(Cmd::Unclip);
            }
            1 => draw_one(self, r, false),
            _ => draw_one(self, r, true),
        }
        self.push(Cmd::Unclip);
    }

    /// A small note with an icon (an information line inside a card or under it).
    pub fn note(&mut self, glyph: char, text: &str) {
        let p = self.p;
        let (x, w) = (self.content_x(), self.content_w());
        let tw = w - 2.0 * ROW_PAD_X - 30.0;
        let (_, th) = self.measure(text, 12.0, Font::Text, tw);
        let h = (th + 24.0).max(46.0);
        if let Some(card) = &mut self.card {
            if !card.first {
                self.main.push(Cmd::Rect { r: R::new(x + ROW_PAD_X, self.cy, w - 2.0 * ROW_PAD_X, 1.0), radius: 0.0, fill: Some(with_alpha(p.border, 0.8)), stroke: None });
            }
            card.first = false;
        }
        self.glyph(R::new(x + ROW_PAD_X - 2.0, self.cy, 24.0, h), glyph, 16.0, p.muted);
        self.text(x + ROW_PAD_X + 28.0, self.cy + (h - th) / 2.0, text, 12.0, Font::Text, p.muted, tw);
        self.cy += h;
    }

    /// A row of text only (for the about card): a short line and a quieter one under it.
    pub fn info_row(&mut self, title: &str, desc: Option<&str>) {
        self.row(title, desc, 0.0, 52.0);
    }

    /// The about block: the logo, the name, the version and a line.
    pub fn about_block(&mut self, name: &str, version: &str, line: &str) {
        let p = self.p;
        let (x, w) = (self.content_x(), self.content_w());
        let h = 96.0;
        if let Some(card) = &mut self.card {
            card.first = false;
        }
        self.push(Cmd::Logo { c: (x + ROW_PAD_X + 28.0, self.cy + h / 2.0), r: 28.0 });
        self.text(x + ROW_PAD_X + 74.0, self.cy + 20.0, name, 20.0, Font::Semibold, p.text, w - 100.0);
        self.text(x + ROW_PAD_X + 74.0, self.cy + 48.0, version, 13.0, Font::Text, p.muted, w - 100.0);
        let _ = line;
        self.cy += h;
    }

    /// A list of items (program names) with a small remove cross at the right of each. Returns the index whose cross was pressed.
    pub fn removable_list(&mut self, name: &str, items: &[String], empty_text: &str) -> Option<usize> {
        let p = self.p;
        let (x, w) = (self.content_x(), self.content_w());
        let mut removed = None;
        if items.is_empty() {
            let (tw, th) = self.measure(empty_text, 13.0, Font::Text, w - 40.0);
            let h = 74.0;
            self.text(x + (w - tw) / 2.0, self.cy + (h - th) / 2.0, empty_text, 13.0, Font::Text, p.muted, w - 40.0);
            self.cy += h;
            if let Some(card) = &mut self.card {
                card.first = false;
            }
            return None;
        }
        for (i, item) in items.iter().enumerate() {
            let h = 46.0;
            if let Some(card) = &mut self.card {
                if !card.first {
                    self.main.push(Cmd::Rect { r: R::new(x + ROW_PAD_X, self.cy, w - 2.0 * ROW_PAD_X, 1.0), radius: 0.0, fill: Some(with_alpha(p.border, 0.8)), stroke: None });
                }
                card.first = false;
            }
            let row = R::new(x, self.cy, w, h);
            self.glyph(R::new(x + ROW_PAD_X - 2.0, row.y, 24.0, h), '\u{E737}', 15.0, p.muted);
            self.text(x + ROW_PAD_X + 30.0, row.y + 13.0, item, 14.0, Font::Text, p.text, w - 120.0);
            let cross = R::new(row.right() - ROW_PAD_X - 28.0, row.y + 9.0, 28.0, 28.0);
            let id = id_of(&format!("{name}:{i}"));
            let resp = self.hit(id, cross, true);
            let a = self.anim(id ^ 0x55, if resp.hover { 1.0 } else { 0.0 }, 0.1);
            if a > 0.0 {
                self.push(Cmd::Rect { r: cross, radius: 6.0, fill: Some(with_alpha(p.danger, 0.16 * a)), stroke: None });
            }
            self.glyph(cross, '\u{E711}', 11.0, mix(p.muted, p.danger, a));
            if resp.focused && self.s.focus_visible {
                self.focus_ring(cross, 6.0);
            }
            if resp.clicked {
                removed = Some(i);
            }
            self.cy += h;
        }
        removed
    }

    /// A list of entries, each with a small tinted tag (a direction such as "AR to EN"), a title, a quieter detail line under it and a remove
    /// cross at the right; a row is lit under the pointer. Returns the index whose cross was pressed.
    pub fn entry_list(&mut self, name: &str, items: &[ListEntry], empty_text: &str) -> Option<usize> {
        let p = self.p;
        let (x, w) = (self.content_x(), self.content_w());
        if items.is_empty() {
            let (tw, th) = self.measure(empty_text, 13.0, Font::Text, w - 40.0);
            let h = 74.0;
            self.text(x + (w - tw) / 2.0, self.cy + (h - th) / 2.0, empty_text, 13.0, Font::Text, p.muted, w - 40.0);
            self.cy += h;
            if let Some(card) = &mut self.card {
                card.first = false;
            }
            return None;
        }
        let mut removed = None;
        for (i, item) in items.iter().enumerate() {
            let h = if item.detail.is_empty() { 50.0 } else { 64.0 };
            if let Some(card) = &mut self.card {
                if !card.first {
                    self.main.push(Cmd::Rect { r: R::new(x + ROW_PAD_X, self.cy, w - 2.0 * ROW_PAD_X, 1.0), radius: 0.0, fill: Some(with_alpha(p.border, 0.8)), stroke: None });
                }
                card.first = false;
            }
            let row = R::new(x, self.cy, w, h);
            let id = id_of(&format!("{name}:{i}"));
            let lit = self.anim(id ^ 0x77, if self.pointer_inside(row) { 1.0 } else { 0.0 }, 0.1);
            if lit > 0.0 {
                self.push(Cmd::Rect { r: R::new(x + 6.0, row.y + 3.0, w - 12.0, h - 6.0), radius: 6.0, fill: Some(with_alpha(p.hover, lit)), stroke: None });
            }
            let mut tx = x + ROW_PAD_X;
            if !item.tag.is_empty() {
                let (tw, th) = self.measure(&item.tag, 11.0, Font::Semibold, 200.0);
                // one width for every tag, so the titles line up
                let chip = R::new(tx, row.y + (h - 24.0) / 2.0, tw.max(46.0) + 24.0, 24.0);
                self.push(Cmd::Rect { r: chip, radius: 12.0, fill: Some(with_alpha(p.accent, if p.dark { 0.22 } else { 0.14 })), stroke: None });
                self.text(chip.x + (chip.w - tw) / 2.0, chip.y + (chip.h - th) / 2.0, &item.tag, 11.0, Font::Semibold, if p.dark { mix(p.accent, p.text, 0.35) } else { p.accent }, 200.0);
                tx = chip.right() + 14.0;
            }
            let text_w = (row.right() - ROW_PAD_X - 44.0 - tx).max(60.0);
            if item.detail.is_empty() {
                let (_, th) = self.measure(&item.title, 14.0, Font::Semibold, text_w);
                self.text(tx, row.y + (h - th) / 2.0, &item.title, 14.0, Font::Semibold, p.text, text_w);
            } else {
                let (_, t1) = self.measure(&item.title, 14.5, Font::Semibold, text_w);
                let (_, t2) = self.measure(&item.detail, 12.5, Font::Text, text_w);
                let top = row.y + (h - (t1 + 3.0 + t2)) / 2.0;
                self.text(tx, top, &item.title, 14.5, Font::Semibold, p.text, text_w);
                self.text(tx, top + t1 + 3.0, &item.detail, 12.5, Font::Text, p.muted, text_w);
            }
            let cross = R::new(row.right() - ROW_PAD_X - 28.0, row.y + (h - 28.0) / 2.0, 28.0, 28.0);
            let rid = id_of(&format!("{name}:x{i}"));
            let resp = self.hit(rid, cross, true);
            let a = self.anim(rid ^ 0x55, if resp.hover { 1.0 } else { 0.0 }, 0.1);
            if a > 0.0 {
                self.push(Cmd::Rect { r: cross, radius: 6.0, fill: Some(with_alpha(p.danger, 0.16 * a)), stroke: None });
            }
            self.glyph(cross, '\u{E711}', 11.0, mix(p.muted, p.danger, a));
            if resp.focused && self.s.focus_visible {
                self.focus_ring(cross, 6.0);
            }
            if resp.clicked {
                removed = Some(i);
            }
            self.cy += h;
        }
        removed
    }

    /// A row with a title, a description and a key cap: click it, then press the new key. `label` is the current key; `is_default`
    /// says whether it is the original one (the reset button is only offered when it is not).
    pub fn key_row(&mut self, name: &str, title: &str, desc: Option<&str>, label: &str, is_default: bool) -> KeyRow {
        let id = id_of(name);
        let (cap_w, reset_w) = (132.0, 32.0);
        let r = self.row(title, desc, cap_w + 8.0 + reset_w, ROW_MIN);
        let p = self.p;
        let top = r.y + (r.h - 32.0) / 2.0;
        let reset_r = R::new(r.right() - ROW_PAD_X - reset_w, top, reset_w, 32.0);
        let cap = R::new(reset_r.x - 8.0 - cap_w, top, cap_w, 32.0);
        let mut out = KeyRow::default();

        let resp = self.hit(id, cap, true);
        if resp.clicked {
            self.s.capture = Some(id);
            self.s.animating = true;
        } else if self.s.capture == Some(id) {
            if self.input.pressed && !self.pointer_inside(cap) {
                // a click elsewhere gives up
                self.s.capture = None;
            } else if let Some(k) = self.input.captured {
                out.captured = Some(k);
                self.s.capture = None;
                self.s.animating = true;
            } else if self.input.capture_cancel {
                self.s.capture = None;
                self.s.animating = true;
            }
        }
        let waiting = self.s.capture == Some(id);
        let a = self.anim(id ^ 0x66, if waiting { 1.0 } else if resp.hover { 0.5 } else { 0.0 }, 0.12);
        let fill = mix(p.field, mix(p.field, p.text, 0.12), a.min(0.6));
        let stroke = if waiting { (p.accent, 2.0) } else { (p.border, 1.0) };
        self.push(Cmd::Rect { r: cap, radius: 6.0, fill: Some(fill), stroke: Some(stroke) });
        let (text, font, color) = if waiting { ("Press a key\u{2026}", Font::Text, p.muted) } else { (label, Font::Semibold, p.text) };
        let (tw, th) = self.measure(text, 13.0, font, cap_w - 8.0);
        self.text(cap.x + (cap.w - tw) / 2.0, cap.y + (cap.h - th) / 2.0, text, 13.0, font, color, cap_w - 8.0);
        if resp.focused && self.s.focus_visible {
            self.focus_ring(cap, 6.0);
        }

        // the button that puts the original key back
        let rid = id_of(&format!("{name}:reset"));
        let rresp = self.hit(rid, reset_r, !is_default);
        if !is_default {
            let ra = self.anim(rid ^ 0x77, if rresp.hover { 1.0 } else { 0.0 }, 0.1);
            if ra > 0.0 {
                self.push(Cmd::Rect { r: reset_r, radius: 6.0, fill: Some(with_alpha(p.hover, 1.4 * ra)), stroke: None });
            }
            self.glyph(reset_r, '\u{E7A7}', 14.0, p.text);
            if rresp.focused && self.s.focus_visible {
                self.focus_ring(reset_r, 6.0);
            }
            if rresp.clicked {
                out.reset = true;
            }
        }
        out
    }

    /// Adds space under the last thing.
    pub fn space(&mut self, h: f32) {
        self.cy += h;
    }

    /// Where the next row of the card starts and how wide it is: for the caller's own widgets.
    pub fn cursor(&self) -> R {
        R::new(self.content_x(), self.cy, self.content_w(), 0.0)
    }

    pub fn advance(&mut self, h: f32) {
        self.cy += h;
    }
}

thread_local! {
    static FRAME_DT: std::cell::Cell<f32> = const { std::cell::Cell::new(0.016) };
}

// ───────────── the window ─────────────

/// How the window looks and where it is.
pub struct Config {
    pub title: &'static str,
    pub class: PCWSTR,
    /// Client size in logical pixels.
    pub width: i32,
    pub height: i32,
    pub icon: HICON,
    /// The colours to use now (called about once a second).
    pub look: fn() -> Look,
}

struct Ctx {
    hwnd: HWND,
    factory: ID2D1Factory,
    gfx: Gfx,
    target: Option<ID2D1HwndRenderTarget>,
    ui: UiState,
    input: Input,
    draw: Box<dyn FnMut(&mut Ui)>,
    look: fn() -> Look,
    pal: Palette,
    pal_at: Instant,
    dpi: f32,
    tracking: bool,
}

thread_local! {
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
}

fn with_ctx<R>(f: impl FnOnce(&mut Ctx) -> R) -> Option<R> {
    CTX.with(|c| c.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

/// Opens the window (or brings the open one to the front). `draw` runs once per frame, on this thread.
pub fn open(cfg: Config, draw: Box<dyn FnMut(&mut Ui)>) -> Option<HWND> {
    unsafe {
        if let Some(existing) = with_ctx(|c| c.hwnd) {
            if IsWindow(Some(existing)).as_bool() {
                let _ = ShowWindow(existing, SW_SHOW);
                let _ = SetForegroundWindow(existing);
                return Some(existing);
            }
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let hinst = GetModuleHandleW(None).ok()?;
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: cfg.class,
            hIcon: cfg.icon,
            hCursor: LoadCursorW(None, IDC_ARROW).ok().unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let dpi = GetDpiForSystem().max(96);
        let scale = dpi as f32 / 96.0;
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let mut frame = RECT { left: 0, top: 0, right: (cfg.width as f32 * scale).round() as i32, bottom: (cfg.height as f32 * scale).round() as i32 };
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WINDOW_EX_STYLE(0), dpi);
        let (ww, wh) = (frame.right - frame.left, frame.bottom - frame.top);
        let (x, y) = ((GetSystemMetrics(SM_CXSCREEN) - ww) / 2, (GetSystemMetrics(SM_CYSCREEN) - wh) / 3);
        let title: Vec<u16> = cfg.title.encode_utf16().chain(std::iter::once(0)).collect();
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), cfg.class, PCWSTR(title.as_ptr()), style, x, y, ww, wh, None, None, Some(hinst.into()), None).ok()?;

        let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let pal = palette((cfg.look)());
        set_title_bar(hwnd, pal.dark);
        let ctx = Ctx {
            hwnd,
            factory,
            gfx: Gfx { dwrite, cache: HashMap::new() },
            target: None,
            ui: UiState::new(),
            input: Input::default(),
            draw,
            look: cfg.look,
            pal,
            pal_at: Instant::now(),
            dpi: dpi as f32,
            tracking: false,
        };
        CTX.with(|c| *c.borrow_mut() = Some(ctx));
        // (a development build run by a test, VERBA_TEST_QUIET=1, shows the window without taking the focus)
        let quiet = cfg!(debug_assertions) && std::env::var("VERBA_TEST_QUIET").is_ok();
        let _ = ShowWindow(hwnd, if quiet { SW_SHOWNOACTIVATE } else { SW_SHOW });
        // the very first ShowWindow of a program is overridden by how the program was started (hidden, minimised...);
        // this one is not, so the window always appears
        let _ = SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | if quiet { SWP_SHOWWINDOW | SWP_NOACTIVATE } else { SWP_SHOWWINDOW });
        if !quiet {
            let _ = SetForegroundWindow(hwnd);
        }
        Some(hwnd)
    }
}

/// Asks the open window to draw again (the program's settings changed from somewhere else).
pub fn refresh() {
    if let Some(h) = with_ctx(|c| c.hwnd) {
        unsafe {
            let _ = InvalidateRect(Some(h), None, false);
        }
    }
}

fn set_title_bar(hwnd: HWND, dark: bool) {
    unsafe {
        let v = BOOL(dark as i32);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &v as *const BOOL as *const _, std::mem::size_of::<BOOL>() as u32);
    }
}

fn make_target(ctx: &mut Ctx) -> Option<ID2D1HwndRenderTarget> {
    unsafe {
        let mut rc = RECT::default();
        GetClientRect(ctx.hwnd, &mut rc).ok()?;
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_UNKNOWN, alphaMode: D2D1_ALPHA_MODE_UNKNOWN },
            dpiX: ctx.dpi,
            dpiY: ctx.dpi,
            usage: D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
        };
        let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: ctx.hwnd,
            pixelSize: D2D_SIZE_U { width: (rc.right - rc.left).max(1) as u32, height: (rc.bottom - rc.top).max(1) as u32 },
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        ctx.factory.CreateHwndRenderTarget(&props, &hp).ok()
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                let _ = EndPaint(hwnd, &ps);
                paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_SIZE => {
                with_ctx(|c| {
                    if let Some(t) = &c.target {
                        let _ = t.Resize(&D2D_SIZE_U { width: (lp.0 & 0xFFFF).max(1) as u32, height: ((lp.0 >> 16) & 0xFFFF).max(1) as u32 });
                    }
                });
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_DPICHANGED => {
                let new_dpi = (wp.0 & 0xFFFF) as f32;
                let r = *(lp.0 as *const RECT);
                with_ctx(|c| {
                    c.dpi = new_dpi;
                    c.target = None;
                    c.gfx.cache.clear();
                });
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER);
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let (px, py) = ((lp.0 & 0xFFFF) as i16 as f32, ((lp.0 >> 16) & 0xFFFF) as i16 as f32);
                with_ctx(|c| {
                    let s = c.dpi / 96.0;
                    c.input.mouse = Some((px / s, py / s));
                    if !c.tracking {
                        c.tracking = true;
                        let mut tme = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                        let _ = TrackMouseEvent(&mut tme);
                    }
                });
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                with_ctx(|c| {
                    c.input.mouse = None;
                    c.tracking = false;
                });
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let _ = SetCapture(hwnd);
                with_ctx(|c| {
                    let s = c.dpi / 96.0;
                    c.input.mouse = Some((((lp.0 & 0xFFFF) as i16 as f32) / s, (((lp.0 >> 16) & 0xFFFF) as i16 as f32) / s));
                    c.input.pressed = true;
                    c.input.down = true;
                });
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                with_ctx(|c| {
                    let s = c.dpi / 96.0;
                    c.input.mouse = Some((((lp.0 & 0xFFFF) as i16 as f32) / s, (((lp.0 >> 16) & 0xFFFF) as i16 as f32) / s));
                    c.input.released = true;
                    c.input.down = false;
                });
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32 / 120.0;
                with_ctx(|c| c.input.wheel += delta);
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_SYSKEYDOWN => {
                if capture_key(hwnd, wp.0 as u32) {
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_SYSCHAR => {
                if with_ctx(|c| c.ui.capture.is_some()).unwrap_or(false) {
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_KEYDOWN => {
                if capture_key(hwnd, wp.0 as u32) {
                    return LRESULT(0);
                }
                let vk = wp.0 as u16;
                let shift = GetKeyState(VK_SHIFT.0 as i32) < 0;
                let mut handled = true;
                with_ctx(|c| match vk {
                    v if v == VK_TAB.0 => {
                        c.input.tab = if shift { -1 } else { 1 };
                    }
                    v if v == VK_SPACE.0 || v == VK_RETURN.0 => c.input.activate = true,
                    v if v == VK_LEFT.0 => c.input.left = true,
                    v if v == VK_RIGHT.0 => c.input.right = true,
                    v if v == VK_UP.0 => c.input.up = true,
                    v if v == VK_DOWN.0 => c.input.down_key = true,
                    v if v == VK_ESCAPE.0 => {}
                    _ => handled = false,
                });
                if vk == VK_ESCAPE.0 {
                    let _ = DestroyWindow(hwnd);
                    return LRESULT(0);
                }
                if handled {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_SETCURSOR => {
                let hand = with_ctx(|c| c.ui.hand).unwrap_or(false);
                let client = (lp.0 & 0xFFFF) == 1; // HTCLIENT
                if client {
                    SetCursor(LoadCursorW(None, if hand { IDC_HAND } else { IDC_ARROW }).ok());
                    return LRESULT(1);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }
            WM_TIMER => {
                let _ = InvalidateRect(Some(hwnd), None, false);
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                let _ = KillTimer(Some(hwnd), TIMER_ANIM);
                CTX.with(|c| *c.borrow_mut() = None);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

/// While a key row waits for a key, every key is its: Esc gives up, a modifier alone is ignored, anything else is taken.
/// Returns true when the key was used (the window must not act on it).
fn capture_key(hwnd: HWND, vk: u32) -> bool {
    let capturing = with_ctx(|c| c.ui.capture.is_some()).unwrap_or(false);
    if !capturing {
        return false;
    }
    unsafe {
        let held = |k: u16| GetKeyState(k as i32) < 0;
        let mods = (held(VK_CONTROL.0) as u8) | ((held(VK_MENU.0) as u8) << 1) | ((held(VK_SHIFT.0) as u8) << 2);
        let modifier_key = matches!(vk, 0x10 | 0x11 | 0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5);
        if !modifier_key {
            with_ctx(|c| {
                if vk == VK_ESCAPE.0 as u32 {
                    c.input.capture_cancel = true;
                } else {
                    c.input.captured = Some((vk, mods));
                }
            });
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
    true
}

/// How wide the strip at the right edge is where the pointer takes hold of the scroll bar (the bar itself is thinner: easy to hit).
const SCROLL_HIT_W: f32 = 18.0;

/// Where the scroll bar's track starts, how long it is, and how long its thumb is, for a page of `content_h` in a window of `height`.
fn scroll_bar_geometry(height: f32, content_h: f32) -> (f32, f32, f32) {
    let track = (height - 16.0).max(1.0);
    let thumb = (track * height / content_h.max(1.0)).clamp(32.0_f32.min(track), track);
    (8.0, track, thumb)
}

/// One frame: runs the caller's `draw`, draws the result, then runs what the caller deferred.
fn paint(hwnd: HWND) {
    let Some(mut ctx) = CTX.with(|c| c.try_borrow_mut().ok().and_then(|mut b| b.take())) else { return };
    let mut deferred: Vec<Box<dyn FnOnce()>> = Vec::new();
    let mut close = false;
    let mut again = false;
    unsafe {
        if ctx.target.is_none() {
            ctx.target = make_target(&mut ctx);
        }
        // the colours: Windows' own theme may change while the window is open
        if ctx.pal_at.elapsed().as_secs_f32() > 1.0 {
            ctx.pal_at = Instant::now();
            let p = palette((ctx.look)());
            if p.dark != ctx.pal.dark {
                set_title_bar(hwnd, p.dark);
            }
            ctx.pal = p;
        }
        if let Some(target) = ctx.target.clone() {
            let scale = ctx.dpi / 96.0;
            let size = target.GetSize();
            let (width, height) = (size.width / 1.0, size.height / 1.0);
            let now = Instant::now();
            let dt = now.duration_since(ctx.ui.last).as_secs_f32().clamp(0.001, 0.05);
            ctx.ui.last = now;
            FRAME_DT.with(|d| d.set(dt));
            ctx.ui.hand = false;
            ctx.ui.hover = None;
            ctx.ui.animating = false;
            ctx.ui.order_next.clear();

            // keyboard focus moves with Tab through what the last frame offered
            if ctx.input.tab != 0 && !ctx.ui.order.is_empty() {
                let n = ctx.ui.order.len() as i32;
                let at = ctx.ui.focus.and_then(|f| ctx.ui.order.iter().position(|x| *x == f)).map(|i| i as i32);
                let next = match at {
                    Some(i) => (i + ctx.input.tab).rem_euclid(n),
                    None => if ctx.input.tab > 0 { 0 } else { n - 1 },
                };
                ctx.ui.focus = Some(ctx.ui.order[next as usize]);
                ctx.ui.focus_visible = true;
            }
            // the wheel scrolls the page
            if ctx.input.wheel != 0.0 {
                ctx.ui.scroll -= ctx.input.wheel * 56.0;
            }
            let max_scroll = (ctx.ui.content_h - ctx.ui.view_h).max(0.0);
            ctx.ui.scroll = ctx.ui.scroll.clamp(0.0, max_scroll);

            // the scroll bar is a real one: take the thumb and drag it, click the track to go a page up or down
            ctx.ui.bar_hover = false;
            if ctx.ui.content_h > height + 1.0 {
                let (track_top, track, thumb_h) = scroll_bar_geometry(height, ctx.ui.content_h);
                let room = (track - thumb_h).max(1.0);
                let span = (ctx.ui.content_h - height).max(1.0);
                let thumb_top = track_top + room * (ctx.ui.scroll / span);
                let mouse = ctx.input.mouse;
                if let Some(grab) = ctx.ui.bar_drag {
                    // the pointer may leave the window while the thumb is held (the window has the mouse captured)
                    if ctx.input.down {
                        if let Some((_, my)) = mouse {
                            let top = (my - grab).clamp(track_top, track_top + room);
                            ctx.ui.scroll = (top - track_top) / room * span;
                        }
                    } else {
                        ctx.ui.bar_drag = None;
                    }
                    ctx.ui.bar_hover = true;
                } else if let Some((mx, my)) = mouse.filter(|(mx, my)| *mx >= width - SCROLL_HIT_W && *my >= 0.0 && *my <= height) {
                    ctx.ui.bar_hover = true;
                    if ctx.input.pressed {
                        if my >= thumb_top && my <= thumb_top + thumb_h {
                            ctx.ui.bar_drag = Some(my - thumb_top);
                        } else {
                            ctx.ui.scroll += if my < thumb_top { -height * 0.9 } else { height * 0.9 };
                        }
                        // the click was for the bar, not for what is under it
                        ctx.input.pressed = false;
                    }
                    let _ = mx;
                }
                ctx.ui.scroll = ctx.ui.scroll.clamp(0.0, span);
            } else {
                ctx.ui.bar_drag = None;
            }

            let (side, main, content_end, d, c, hand_anim) = {
                let Ctx { gfx, ui, input, draw, pal, .. } = &mut ctx;
                let mut frame = Ui {
                    g: gfx,
                    s: ui,
                    input,
                    p: *pal,
                    side: Vec::new(),
                    main: Vec::new(),
                    in_side: false,
                    width: width / 1.0 * 1.0,
                    height,
                    cy: 0.0,
                    card: None,
                    sy: 0.0,
                    deferred: Vec::new(),
                    close: false,
                };
                // sizes are in logical pixels: the target works in them (its dpi is set)
                frame.width = width;
                frame.height = height;
                draw(&mut frame);
                let end = frame.cy;
                (std::mem::take(&mut frame.side), std::mem::take(&mut frame.main), end, std::mem::take(&mut frame.deferred), frame.close, frame.s.animating)
            };
            deferred = d;
            close = c;
            ctx.ui.content_h = content_end + 30.0;
            ctx.ui.view_h = height;
            // keep the focused widget in view when it was reached with the keyboard
            if let Some(r) = ctx.ui.reveal.take() {
                let (top, bottom) = (r.y - 20.0, r.bottom() + 20.0);
                if top < ctx.ui.scroll {
                    ctx.ui.scroll = top.max(0.0);
                    again = true;
                } else if bottom > ctx.ui.scroll + height {
                    ctx.ui.scroll = bottom - height;
                    again = true;
                }
            }
            let pal = ctx.pal;
            render(&ctx, &target, &side, &main, pal, scale, width, height);
            // the one-shot parts of the input are used up
            let released = ctx.input.released;
            ctx.input.pressed = false;
            ctx.input.released = false;
            ctx.input.wheel = 0.0;
            ctx.input.tab = 0;
            ctx.input.activate = false;
            ctx.input.left = false;
            ctx.input.right = false;
            ctx.input.up = false;
            ctx.input.down_key = false;
            ctx.input.captured = None;
            ctx.input.capture_cancel = false;
            if released {
                ctx.ui.active = None;
            }
            std::mem::swap(&mut ctx.ui.order, &mut ctx.ui.order_next);
            let animating = hand_anim || ctx.ui.animating;
            if animating || again {
                SetTimer(Some(hwnd), TIMER_ANIM, 15, None);
            } else {
                let _ = KillTimer(Some(hwnd), TIMER_ANIM);
            }
        }
    }
    let alive = !close;
    CTX.with(|c| *c.borrow_mut() = Some(ctx));
    let had_actions = !deferred.is_empty();
    for f in deferred {
        f();
    }
    unsafe {
        if close {
            let _ = DestroyWindow(hwnd);
        } else if had_actions || alive && again {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render(ctx: &Ctx, target: &ID2D1HwndRenderTarget, side: &[Cmd], main: &[Cmd], p: Palette, scale: f32, width: f32, height: f32) {
    unsafe {
        let _ = scale;
        target.BeginDraw();
        target.Clear(Some(&p.bg));
        let brush: ID2D1SolidColorBrush = match target.CreateSolidColorBrush(&rgba(0.0, 0.0, 0.0, 1.0), None) {
            Ok(b) => b,
            Err(_) => {
                let _ = target.EndDraw(None, None);
                return;
            }
        };
        let round: Option<ID2D1StrokeStyle> = ctx
            .factory
            .CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES {
                    startCap: D2D1_CAP_STYLE_ROUND,
                    endCap: D2D1_CAP_STYLE_ROUND,
                    dashCap: D2D1_CAP_STYLE_ROUND,
                    lineJoin: D2D1_LINE_JOIN_ROUND,
                    miterLimit: 10.0,
                    dashStyle: D2D1_DASH_STYLE_SOLID,
                    dashOffset: 0.0,
                },
                None,
            )
            .ok();
        // the page (scrolled and clipped to its area), then the sidebar on top
        let view = R::new(SIDEBAR_W, 0.0, width - SIDEBAR_W, height);
        target.PushAxisAlignedClip(&view.d2d(), D2D1_ANTIALIAS_MODE_ALIASED);
        target.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: -ctx.ui.scroll });
        run(target, &brush, round.as_ref(), main);
        target.SetTransform(&Matrix3x2::identity());
        // a thin scroll bar when the page is longer than the window
        if ctx.ui.content_h > height + 1.0 {
            let (track_top, track, thumb_h) = scroll_bar_geometry(height, ctx.ui.content_h);
            let top = track_top + (track - thumb_h) * (ctx.ui.scroll / (ctx.ui.content_h - height).max(1.0));
            let active = ctx.ui.bar_hover || ctx.ui.bar_drag.is_some();
            // wider and darker while the pointer is on it, with a faint track behind
            if active {
                brush.SetColor(&with_alpha(p.muted, 0.14));
                let t = R::new(width - 14.0, track_top, 10.0, track);
                target.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: t.d2d(), radiusX: 5.0, radiusY: 5.0 }, &brush);
            }
            let w = if active { 6.0 } else { 4.0 };
            brush.SetColor(&with_alpha(p.muted, if ctx.ui.bar_drag.is_some() { 0.9 } else if active { 0.7 } else { 0.45 }));
            let r = R::new(width - 9.0 - (w - 4.0) / 2.0 - 0.0, top, w, thumb_h);
            target.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: w / 2.0, radiusY: w / 2.0 }, &brush);
        }
        target.PopAxisAlignedClip();
        run(target, &brush, round.as_ref(), side);
        let _ = target.EndDraw(None, None);
    }
}

fn run(t: &ID2D1HwndRenderTarget, brush: &ID2D1SolidColorBrush, round: Option<&ID2D1StrokeStyle>, cmds: &[Cmd]) {
    unsafe {
        for c in cmds {
            match c {
                Cmd::Nop => {}
                Cmd::Rect { r, radius, fill, stroke } => {
                    let rr = D2D1_ROUNDED_RECT { rect: r.d2d(), radiusX: *radius, radiusY: *radius };
                    if let Some(f) = fill {
                        brush.SetColor(f);
                        t.FillRoundedRectangle(&rr, brush);
                    }
                    if let Some((col, width)) = stroke {
                        brush.SetColor(col);
                        let half = width / 2.0;
                        let inner = D2D1_ROUNDED_RECT {
                            rect: r.inset(half).d2d(),
                            radiusX: (radius - half).max(0.0),
                            radiusY: (radius - half).max(0.0),
                        };
                        t.DrawRoundedRectangle(&inner, brush, *width, None);
                    }
                }
                Cmd::Circle { c, r, fill, stroke } => {
                    let e = D2D1_ELLIPSE { point: Vector2::new(c.0, c.1), radiusX: *r, radiusY: *r };
                    if let Some(f) = fill {
                        brush.SetColor(f);
                        t.FillEllipse(&e, brush);
                    }
                    if let Some((col, width)) = stroke {
                        brush.SetColor(col);
                        t.DrawEllipse(&e, brush, *width, None);
                    }
                }
                Cmd::Text { layout, x, y, color } => {
                    brush.SetColor(color);
                    t.DrawTextLayout(Vector2::new(*x, *y), layout, brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
                }
                Cmd::Logo { c, r } => draw_logo(t, brush, round, *c, *r),
                Cmd::Clip(r) => t.PushAxisAlignedClip(&r.d2d(), D2D1_ANTIALIAS_MODE_ALIASED),
                Cmd::Unclip => t.PopAxisAlignedClip(),
            }
        }
    }
}

/// The logo of the icon: a blue disc (a soft gradient) with a white V of two thick strokes.
fn draw_logo(t: &ID2D1HwndRenderTarget, brush: &ID2D1SolidColorBrush, round: Option<&ID2D1StrokeStyle>, c: (f32, f32), r: f32) {
    unsafe {
        let stops = [
            D2D1_GRADIENT_STOP { position: 0.0, color: rgba(0.17, 0.60, 0.95, 1.0) },
            D2D1_GRADIENT_STOP { position: 1.0, color: rgba(0.0, 0.38, 0.72, 1.0) },
        ];
        let e = D2D1_ELLIPSE { point: Vector2::new(c.0, c.1), radiusX: r, radiusY: r };
        let drawn = (|| -> windows::core::Result<()> {
            let collection = t.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let gradient = t.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: Vector2::new(c.0, c.1 - r), endPoint: Vector2::new(c.0, c.1 + r) },
                None,
                &collection,
            )?;
            t.FillEllipse(&e, &gradient);
            Ok(())
        })();
        if drawn.is_err() {
            brush.SetColor(&rgba(0.0, 0.47, 0.84, 1.0));
            t.FillEllipse(&e, brush);
        }
        brush.SetColor(&rgba(1.0, 1.0, 1.0, 1.0));
        let u = r / 15.5;
        let at = |gx: f32, gy: f32| Vector2::new(c.0 + (gx - 16.0) * u, c.1 + (gy - 16.0) * u);
        t.DrawLine(at(9.0, 10.0), at(16.0, 23.0), brush, 4.8 * u, round);
        t.DrawLine(at(23.0, 10.0), at(16.0, 23.0), brush, 4.8 * u, round);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_differ_and_are_stable() {
        assert_eq!(id_of("a"), id_of("a"));
        assert_ne!(id_of("a"), id_of("b"));
        assert_ne!(id_of("nav:writing"), id_of("nav:languages"));
    }

    #[test]
    fn rectangles_know_what_they_contain() {
        let r = R::new(10.0, 20.0, 30.0, 40.0);
        assert!(r.contains(10.0, 20.0) && r.contains(39.9, 59.9));
        assert!(!r.contains(40.0, 30.0) && !r.contains(9.9, 30.0) && !r.contains(20.0, 60.0));
        assert_eq!(r.right(), 40.0);
        assert_eq!(r.bottom(), 60.0);
        assert_eq!(r.inset(5.0), R::new(15.0, 25.0, 20.0, 30.0));
    }

    #[test]
    fn colours_mix() {
        let c = mix(rgba(0.0, 0.0, 0.0, 1.0), rgba(1.0, 1.0, 1.0, 0.0), 0.5);
        assert!((c.r - 0.5).abs() < 1e-6 && (c.a - 0.5).abs() < 1e-6);
        assert_eq!(hex(0xFF8000, 1.0).g.round(), 1.0 * (128.0_f32 / 255.0).round());
    }

    #[test]
    fn the_scroll_thumb_follows_the_page_and_stays_in_its_track() {
        // a page three times as long as the window: the thumb is a third of the track
        let (top, track, thumb) = scroll_bar_geometry(600.0, 1800.0);
        assert_eq!(top, 8.0);
        assert!((track - 584.0).abs() < 0.01);
        assert!((thumb - 584.0 / 3.0).abs() < 0.5);
        // a very long page: the thumb never gets smaller than something a hand can take hold of
        assert_eq!(scroll_bar_geometry(600.0, 600_000.0).2, 32.0);
        // a page just over the window: the thumb fills almost the whole track and never goes past it
        let (_, track, thumb) = scroll_bar_geometry(600.0, 601.0);
        assert!(thumb <= track && thumb > track * 0.99);
        // a window shorter than the minimum thumb does not make a thumb longer than the track
        let (_, track, thumb) = scroll_bar_geometry(30.0, 3000.0);
        assert!(thumb <= track);
    }
}
