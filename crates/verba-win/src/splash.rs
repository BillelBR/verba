//! The start-up splash: while Verba loads its word lists on another thread, the logo draws itself on a small card
//! (the disc grows, the "V" is written stroke by stroke, a ring turns), and when the loading is done the ring closes
//! and the card fades away.
//!
//! The picture is Direct2D drawn into a layered window with per-pixel transparency, like the suggestion popup. It never takes
//! the keyboard focus and lets mouse clicks through. How long it stays follows the real loading: the animation plays for at
//! least `MIN_INTRO` seconds so it never flashes, and the ring keeps turning for as long as the loading lasts.
//!
//! The timeline is a pure function (`frame`), so it is tested without a screen.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT,
    D2D_RECT_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_CAP_STYLE_ROUND, D2D1_DASH_STYLE_SOLID, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT, D2D1_GAMMA_2_2, D2D1_LINE_JOIN_ROUND,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT,
    D2D1_RENDER_TARGET_USAGE_NONE, D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
    D2D1CreateFactory, ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_METRICS,
    DWriteCreateFactory, IDWriteFactory, IDWriteTextLayout,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW, MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint, ReleaseDC,
    SelectObject,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, HWND_TOPMOST, MSG, PM_REMOVE, PeekMessageW, RegisterClassW,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SetWindowPos, TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::w;
use windows_numerics::{Matrix3x2, Vector2};

use crate::look::{Look, Theme};

/// The animation plays at least this long, so a fast start does not make the card flash.
const MIN_INTRO: f32 = 0.85;
/// After the loading is done: the ring closes, the card holds for a moment, then fades.
const CLOSE_RING: f32 = 0.25;
const HOLD: f32 = 0.05;
const FADE: f32 = 0.20;

// ───────────── the timeline (pure) ─────────────

/// Everything the drawing needs at one moment. Lengths are in logical pixels of a 96 dpi screen; angles in degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// The card and what it holds.
    pub card_alpha: f32,
    pub card_scale: f32,
    /// The disc with the logo.
    pub disc_alpha: f32,
    pub disc_scale: f32,
    /// The soft light behind the disc.
    pub glow: f32,
    /// How much of each stroke of the "V" is drawn, 0 to 1 (left stroke, then right stroke).
    pub v_left: f32,
    pub v_right: f32,
    /// The ring: start angle, length, opacity.
    pub ring_start: f32,
    pub ring_len: f32,
    pub ring_alpha: f32,
    /// The word "Verba".
    pub word_alpha: f32,
    pub word_dy: f32,
    /// 1 while everything is shown, falling to 0 as the card fades away.
    pub fade: f32,
    /// The animation is over: the window can go.
    pub finished: bool,
}

fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

fn ease_out(p: f32) -> f32 {
    1.0 - (1.0 - clamp01(p)).powi(3)
}

fn ease_in(p: f32) -> f32 {
    clamp01(p).powi(3)
}

fn ease_in_out(p: f32) -> f32 {
    let p = clamp01(p);
    if p < 0.5 { 4.0 * p * p * p } else { 1.0 - (-2.0 * p + 2.0).powi(3) / 2.0 }
}

/// Eases out with a small overshoot: the disc pops in and settles.
fn ease_out_back(p: f32) -> f32 {
    let p = clamp01(p);
    let (c1, c3) = (1.4_f32, 2.4_f32);
    1.0 + c3 * (p - 1.0).powi(3) + c1 * (p - 1.0).powi(2)
}

/// The turning arc while loading: `(start angle, length)` at time `t`. It breathes between short and long while it turns.
fn loading_arc(t: f32) -> (f32, f32) {
    let start = -90.0 + 360.0 * (t * 0.8);
    let breathe = (std::f32::consts::PI * t * 0.7).sin().powi(2);
    (start, 70.0 + 150.0 * breathe)
}

/// The picture at `t` seconds after the start. `done_at` is when the loading finished (`None` while it is still going).
pub fn frame(t: f32, done_at: Option<f32>) -> Frame {
    let t = t.max(0.0);
    // the intro
    let card_p = ease_out(t / 0.30);
    let disc_t = (t - 0.10) / 0.40;
    let word_p = ease_out((t - 0.52) / 0.30);
    let ring_in = ease_out((t - 0.40) / 0.25);

    // when the ending starts: the loading is done and the intro is over
    let end_start = done_at.map(|d| d.max(MIN_INTRO));
    let (mut ring_start, mut ring_len) = loading_arc(end_start.map_or(t, |e| t.min(e)));
    let mut disc_pulse = 1.0;
    let mut fade = 1.0;
    let mut finished = false;
    if let Some(e) = end_start {
        let q = (t - e) / CLOSE_RING;
        if q > 0.0 {
            ring_len += (360.0 - ring_len) * ease_out(q);
            disc_pulse += 0.05 * (std::f32::consts::PI * clamp01(q)).sin();
        }
        let r = (t - (e + CLOSE_RING + HOLD)) / FADE;
        if r > 0.0 {
            fade = 1.0 - ease_in(r);
            finished = r >= 1.0;
        }
    }
    if end_start.is_none() {
        // nothing to close yet
        ring_start = ring_start.rem_euclid(360.0);
    }
    let out_scale = 1.0 + 0.05 * (1.0 - fade);
    Frame {
        card_alpha: card_p,
        card_scale: (0.94 + 0.06 * card_p) * out_scale,
        disc_alpha: ease_out((t - 0.10) / 0.25),
        disc_scale: (0.55 + 0.45 * ease_out_back(disc_t)) * disc_pulse,
        glow: 0.5 * ease_out((t - 0.30) / 0.50),
        v_left: ease_in_out((t - 0.35) / 0.25),
        v_right: ease_in_out((t - 0.52) / 0.25),
        ring_start,
        ring_len,
        ring_alpha: ring_in,
        word_alpha: word_p,
        word_dy: (1.0 - word_p) * 10.0,
        fade,
        finished,
    }
}

// ───────────── the window ─────────────

fn rgba(r: f32, g: f32, b: f32, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r, g, b, a }
}

/// The logo's blue, the one of the icon (#0078D7), and its two ends for a gentle gradient.
const BLUE: [f32; 3] = [0.0, 0.47, 0.84];
const BLUE_TOP: [f32; 3] = [0.17, 0.60, 0.95];
const BLUE_BOTTOM: [f32; 3] = [0.0, 0.38, 0.72];

struct Colors {
    card: D2D1_COLOR_F,
    border: D2D1_COLOR_F,
    text: D2D1_COLOR_F,
    track: D2D1_COLOR_F,
    shadow: f32,
}

fn colors(theme: Theme) -> Colors {
    match theme {
        Theme::Light => Colors {
            card: rgba(0.976, 0.976, 0.980, 0.99),
            border: rgba(0.0, 0.0, 0.0, 0.10),
            text: rgba(0.09, 0.09, 0.10, 1.0),
            track: rgba(0.0, 0.0, 0.0, 0.07),
            shadow: 0.025,
        },
        Theme::Dark => Colors {
            card: rgba(0.125, 0.125, 0.137, 0.99),
            border: rgba(1.0, 1.0, 1.0, 0.10),
            text: rgba(0.97, 0.97, 0.98, 1.0),
            track: rgba(1.0, 1.0, 1.0, 0.08),
            shadow: 0.05,
        },
    }
}

/// Logical sizes of the layout (96 dpi pixels).
const CARD_W: f32 = 300.0;
const CARD_H: f32 = 320.0;
const MARGIN: f32 = 40.0;
const DISC_R: f32 = 56.0;
const DISC_CY: f32 = 130.0;
const RING_R: f32 = 70.0;

struct Splash {
    hwnd: HWND,
    factory: ID2D1Factory,
    target: ID2D1DCRenderTarget,
    word: IDWriteTextLayout,
    word_w: f32,
    word_h: f32,
    look: Look,
    scale: f32,
    pos: POINT,
    w: i32,
    h: i32,
    dc: windows::Win32::Graphics::Gdi::HDC,
    bmp: windows::Win32::Graphics::Gdi::HBITMAP,
    old: windows::Win32::Graphics::Gdi::HGDIOBJ,
}

impl Drop for Splash {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.dc);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

impl Splash {
    fn new(look: Look) -> windows::core::Result<Self> {
        unsafe {
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            // the primary screen: its work area and its scale
            let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
            let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
            let _ = GetMonitorInfoW(monitor, &mut info);
            let work: RECT = info.rcWork;
            let (mut dpi_x, mut dpi_y) = (96u32, 96u32);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
            let scale = dpi_x.max(96) as f32 / 96.0;

            let (w, h) = (((CARD_W + 2.0 * MARGIN) * scale).ceil() as i32, ((CARD_H + 2.0 * MARGIN) * scale).ceil() as i32);
            let pos = POINT { x: work.left + ((work.right - work.left) - w) / 2, y: work.top + ((work.bottom - work.top) - h) * 2 / 5 };

            let hinst = GetModuleHandleW(None)?;
            let class = w!("VerbaSplash");
            RegisterClassW(&WNDCLASSW { lpfnWndProc: Some(wndproc), hInstance: hinst.into(), lpszClassName: class, ..Default::default() });
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT,
                class,
                w!("Verba"),
                WS_POPUP,
                pos.x,
                pos.y,
                w,
                h,
                None,
                None,
                Some(hinst.into()),
                None,
            )?;

            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let target = factory.CreateDCRenderTarget(&props)?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

            // the word "Verba", measured once
            let fmt = dwrite.CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                30.0 * scale,
                w!("en-us"),
            )?;
            let text: Vec<u16> = "Verba".encode_utf16().collect();
            let word = dwrite.CreateTextLayout(&text, &fmt, 1000.0, 200.0)?;
            let mut m = DWRITE_TEXT_METRICS::default();
            word.GetMetrics(&mut m)?;

            // the picture to draw into
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
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

            Ok(Self { hwnd, factory, target, word, word_w: m.width, word_h: m.height, look, scale, pos, w, h, dc, bmp, old })
        }
    }

    /// Draws one frame and hands it to Windows.
    fn show(&self, f: &Frame, first: bool) -> windows::core::Result<()> {
        self.draw(f)?;
        unsafe {
            let size = SIZE { cx: self.w, cy: self.h };
            let src = POINT { x: 0, y: 0 };
            let alpha = (f.fade * 255.0).round().clamp(0.0, 255.0) as u8;
            let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: AC_SRC_ALPHA as u8 };
            UpdateLayeredWindow(self.hwnd, None, Some(&self.pos), Some(&size), Some(self.dc), Some(&src), COLORREF(0), Some(&blend), ULW_ALPHA)?;
            if first {
                let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
            }
        }
        Ok(())
    }

    fn draw(&self, f: &Frame) -> windows::core::Result<()> {
        unsafe {
            let t = &self.target;
            let s = self.scale;
            let c = colors(self.look.theme);
            t.BindDC(self.dc, &RECT { left: 0, top: 0, right: self.w, bottom: self.h })?;
            t.BeginDraw();
            t.Clear(Some(&rgba(0.0, 0.0, 0.0, 0.0)));

            // the whole card grows in from slightly smaller, and swells a little as it fades
            let (cx, cy) = (self.w as f32 / 2.0, self.h as f32 / 2.0);
            let k = f.card_scale;
            t.SetTransform(&Matrix3x2 { M11: k, M12: 0.0, M21: 0.0, M22: k, M31: cx * (1.0 - k), M32: cy * (1.0 - k) });

            let brush: ID2D1SolidColorBrush = t.CreateSolidColorBrush(&rgba(0.0, 0.0, 0.0, 1.0), None)?;
            let card = D2D_RECT_F { left: MARGIN * s, top: MARGIN * s, right: (MARGIN + CARD_W) * s, bottom: (MARGIN + CARD_H) * s };
            let radius = 26.0 * s;
            let a = f.card_alpha;

            // a soft shadow: stacked, nearly transparent rounded rectangles, a little lower than the card
            for i in 0..14 {
                let grow = (14 - i) as f32 * 1.6 * s;
                brush.SetColor(&rgba(0.0, 0.0, 0.0, c.shadow * a));
                t.FillRoundedRectangle(
                    &D2D1_ROUNDED_RECT {
                        rect: D2D_RECT_F { left: card.left - grow, top: card.top - grow + 6.0 * s, right: card.right + grow, bottom: card.bottom + grow + 6.0 * s },
                        radiusX: radius + grow,
                        radiusY: radius + grow,
                    },
                    &brush,
                );
            }
            // the card and its hairline border
            let mut col = c.card;
            col.a *= a;
            brush.SetColor(&col);
            t.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: card, radiusX: radius, radiusY: radius }, &brush);
            let mut border = c.border;
            border.a *= a;
            brush.SetColor(&border);
            t.DrawRoundedRectangle(
                &D2D1_ROUNDED_RECT {
                    rect: D2D_RECT_F { left: card.left + 0.5, top: card.top + 0.5, right: card.right - 0.5, bottom: card.bottom - 0.5 },
                    radiusX: radius - 0.5,
                    radiusY: radius - 0.5,
                },
                &brush,
                1.0,
                None,
            );

            // the centre of the disc
            let (dx, dy) = ((MARGIN + CARD_W / 2.0) * s, (MARGIN + DISC_CY) * s);
            let r = DISC_R * s * f.disc_scale.max(0.0);
            let circle = |x: f32, y: f32, r: f32| D2D1_ELLIPSE { point: Vector2::new(x, y), radiusX: r, radiusY: r };

            // a soft light behind the disc
            for i in 0..8 {
                let grow = (8 - i) as f32 * 3.0 * s;
                brush.SetColor(&rgba(BLUE[0], BLUE[1], BLUE[2], 0.035 * f.glow * f.disc_alpha));
                t.FillEllipse(&circle(dx, dy + 3.0 * s, r + grow), &brush);
            }

            // the disc, a gentle gradient from top to bottom
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: rgba(BLUE_TOP[0], BLUE_TOP[1], BLUE_TOP[2], f.disc_alpha) },
                D2D1_GRADIENT_STOP { position: 1.0, color: rgba(BLUE_BOTTOM[0], BLUE_BOTTOM[1], BLUE_BOTTOM[2], f.disc_alpha) },
            ];
            let collection = t.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            let gradient = t.CreateLinearGradientBrush(
                &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: Vector2::new(dx, dy - r), endPoint: Vector2::new(dx, dy + r) },
                None,
                &collection,
            )?;
            t.FillEllipse(&circle(dx, dy, r), &gradient);

            // the "V" of the logo, written in two strokes (the geometry of the icon: a 32 unit grid, the disc has radius 15.5)
            let u = r / 15.5;
            let round = self.factory.CreateStrokeStyle(
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
            )?;
            brush.SetColor(&rgba(1.0, 1.0, 1.0, f.disc_alpha));
            let at = |gx: f32, gy: f32| Vector2::new(dx + (gx - 16.0) * u, dy + (gy - 16.0) * u);
            let stroke = |from: (f32, f32), to: (f32, f32), p: f32| {
                if p > 0.001 {
                    let end = (from.0 + (to.0 - from.0) * p, from.1 + (to.1 - from.1) * p);
                    t.DrawLine(at(from.0, from.1), at(end.0, end.1), &brush, 4.8 * u, Some(&round));
                }
            };
            stroke((9.0, 10.0), (16.0, 23.0), f.v_left);
            stroke((23.0, 10.0), (16.0, 23.0), f.v_right);

            // the ring: a faint full track, and the arc that turns while loading and closes when it is done
            let ring_r = RING_R * s;
            let mut track = c.track;
            track.a *= f.ring_alpha;
            brush.SetColor(&track);
            t.DrawEllipse(&circle(dx, dy, ring_r), &brush, 3.0 * s, None);
            if f.ring_alpha > 0.01 && f.ring_len > 0.5 {
                let geometry = self.factory.CreatePathGeometry()?;
                let sink = geometry.Open()?;
                let steps = ((f.ring_len / 2.0).ceil() as usize).max(2);
                let point = |deg: f32| {
                    let rad = deg.to_radians();
                    Vector2::new(dx + ring_r * rad.cos(), dy + ring_r * rad.sin())
                };
                sink.BeginFigure(point(f.ring_start), D2D1_FIGURE_BEGIN_HOLLOW);
                let pts: Vec<Vector2> = (1..=steps).map(|i| point(f.ring_start + f.ring_len * i as f32 / steps as f32)).collect();
                sink.AddLines(&pts);
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.Close()?;
                brush.SetColor(&rgba(BLUE[0], BLUE[1], BLUE[2], f.ring_alpha));
                t.DrawGeometry(&geometry, &brush, 3.2 * s, Some(&round));
            }

            // the word
            let mut text = c.text;
            text.a *= f.word_alpha;
            brush.SetColor(&text);
            let wx = (MARGIN + CARD_W / 2.0) * s - self.word_w / 2.0;
            let wy = (MARGIN + 232.0) * s + f.word_dy * s;
            t.DrawTextLayout(Vector2::new(wx, wy - self.word_h / 2.0 + 14.0 * s), &self.word, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);

            t.SetTransform(&Matrix3x2::identity());
            t.EndDraw(None, None)
        }
    }
}

/// Runs `work` on another thread while the splash plays on this one, and returns what it returned.
/// The animation lasts as long as the work does (never less than `MIN_INTRO` seconds, plus the short ending). If the splash cannot
/// be made for any reason, the work simply runs here without it.
pub fn run_with_splash<T: Send + 'static>(look: Look, work: impl FnOnce() -> T + Send + 'static) -> T {
    let splash = match Splash::new(look) {
        Ok(s) => s,
        Err(_) => return work(),
    };
    let handle = std::thread::spawn(work);
    let start = Instant::now();
    let mut done_at: Option<f32> = None;
    let mut first = true;
    loop {
        // the window has nothing to say, but keep Windows happy
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        let t = start.elapsed().as_secs_f32();
        if done_at.is_none() && handle.is_finished() {
            done_at = Some(t);
        }
        let f = frame(t, done_at);
        if splash.show(&f, first).is_err() && first {
            // the first picture could not be shown: give up on the splash, not on starting
            break;
        }
        first = false;
        if f.finished {
            break;
        }
        std::thread::sleep(Duration::from_millis(8));
    }
    drop(splash);
    match handle.join() {
        Ok(v) => v,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_starts_empty() {
        let f = frame(0.0, None);
        assert_eq!(f.card_alpha, 0.0);
        assert_eq!(f.disc_alpha, 0.0);
        assert_eq!((f.v_left, f.v_right), (0.0, 0.0));
        assert!(!f.finished);
    }

    #[test]
    fn the_logo_is_whole_after_the_intro() {
        let f = frame(MIN_INTRO, None);
        assert!((f.disc_scale - 1.0).abs() < 0.02, "the disc has settled: {}", f.disc_scale);
        assert_eq!((f.v_left, f.v_right), (1.0, 1.0));
        assert_eq!(f.word_alpha, 1.0);
        assert_eq!(f.fade, 1.0);
    }

    #[test]
    fn the_v_is_written_left_stroke_first() {
        let f = frame(0.50, None);
        assert!(f.v_left > f.v_right, "left {} right {}", f.v_left, f.v_right);
    }

    #[test]
    fn it_never_ends_while_the_loading_goes_on() {
        for t in [1.0, 5.0, 60.0] {
            let f = frame(t, None);
            assert!(!f.finished && f.fade == 1.0, "still loading at {t}s");
            assert!(f.ring_len < 360.0 && f.ring_len >= 70.0);
        }
    }

    #[test]
    fn a_fast_load_still_plays_the_intro() {
        // the loading was done at 0.1 s: nothing ends before the intro has played
        assert!(!frame(MIN_INTRO - 0.01, Some(0.1)).finished);
        assert_eq!(frame(MIN_INTRO - 0.01, Some(0.1)).fade, 1.0);
        // and it is over soon after: intro + ring + hold + fade
        let end = MIN_INTRO + CLOSE_RING + HOLD + FADE;
        assert!(frame(end + 0.01, Some(0.1)).finished);
    }

    #[test]
    fn a_slow_load_ends_after_it_is_done() {
        let done = 3.0;
        assert!(!frame(done - 0.1, Some(done)).finished);
        let f = frame(done + CLOSE_RING + HOLD + 0.01, Some(done));
        assert!(f.fade > 0.95 && !f.finished, "the card only starts to fade now: {}", f.fade);
        assert!(frame(done + CLOSE_RING + HOLD + FADE + 0.01, Some(done)).finished);
    }

    #[test]
    fn the_ring_closes_when_the_loading_is_done() {
        let done = 2.0;
        assert!(frame(done, Some(done)).ring_len < 360.0);
        let f = frame(done + CLOSE_RING, Some(done));
        assert!((f.ring_len - 360.0).abs() < 0.01, "closed: {}", f.ring_len);
        // the arc does not jump when the loading ends
        let before = frame(done - 0.001, Some(done)).ring_len;
        let after = frame(done + 0.001, Some(done)).ring_len;
        assert!((after - before).abs() < 3.0, "{before} -> {after}");
    }

    #[test]
    fn values_stay_in_range_all_the_way() {
        let mut t = 0.0;
        while t < 4.0 {
            let f = frame(t, Some(1.7));
            for v in [f.card_alpha, f.disc_alpha, f.v_left, f.v_right, f.ring_alpha, f.word_alpha, f.fade] {
                assert!((0.0..=1.0).contains(&v), "{v} out of range at {t}");
            }
            assert!(f.ring_len > 0.0 && f.ring_len <= 360.0 + 0.01);
            t += 0.01;
        }
    }
}
