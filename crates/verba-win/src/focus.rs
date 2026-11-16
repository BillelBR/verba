//! Finds where the user is typing, in any app, without reading what they type.
//!
//! Three ways, from most to least exact:
//! 1. the classic Windows caret (Notepad and older apps),
//! 2. the caret range through UI Automation `TextPattern2` (modern apps),
//! 3. the text field's own rectangle (Chromium-based apps such as VS Code and Slack).

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, SAFEARRAY,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, IUIAutomationTextPattern2,
    IUIAutomationValuePattern, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Character,
    UIA_TextPattern2Id, UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{GUITHREADINFO, GetGUIThreadInfo};
use windows::core::BOOL;

/// How exact the position is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorKind {
    /// The rectangle of the caret itself.
    Caret,
    /// The rectangle of the whole text field.
    Field,
}

/// Where to put the popup, in screen pixels.
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    pub rect: RECT,
    pub kind: AnchorKind,
}

/// Looks at the app that has the keyboard focus.
pub struct Locator {
    uia: IUIAutomation,
}

impl Locator {
    pub fn new() -> windows::core::Result<Self> {
        // already initialised on this thread is fine
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)?;
            Ok(Self { uia })
        }
    }

    /// The best position available right now, or `None` when the focus is not in a (non-password) text field.
    pub fn locate(&self) -> Option<Anchor> {
        unsafe {
            let el = self.uia.GetFocusedElement().ok()?;
            if is_password(&el) {
                return None;
            }
            classic_caret().map(|rect| Anchor { rect, kind: AnchorKind::Caret }).or_else(|| uia_anchor(&el))
        }
    }

    /// Where the user is typing and the few letters just before the caret, read in one go.
    /// Nothing is kept: the text is returned to the caller and the caller drops it.
    /// Password fields are never read. `None` when the field gives no caret to read from.
    pub fn snapshot(&self, max_chars: i32) -> Option<Snapshot> {
        unsafe {
            let el = self.uia.GetFocusedElement().ok()?;
            if is_password(&el) {
                return None;
            }
            let before_caret = text_before_caret(&el, max_chars)?;
            let anchor = classic_caret().map(|rect| Anchor { rect, kind: AnchorKind::Caret }).or_else(|| uia_anchor(&el))?;
            Some(Snapshot { anchor, before_caret })
        }
    }
}

/// What `Locator::snapshot` found.
pub struct Snapshot {
    pub anchor: Anchor,
    /// The last few characters before the caret.
    pub before_caret: String,
}

unsafe fn is_password(el: &IUIAutomationElement) -> bool {
    unsafe { el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(false) }
}

/// The caret through UI Automation, else the whole text field.
unsafe fn uia_anchor(el: &IUIAutomationElement) -> Option<Anchor> {
    unsafe {
        if let Ok(tp2) = el.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id) {
            let mut active = BOOL(0);
            if let Ok(range) = tp2.GetCaretRange(&mut active) {
                if let Ok(sa) = range.GetBoundingRectangles() {
                    let v = read_doubles(sa);
                    if v.len() >= 4 && v[2] >= 0.0 && v[3] > 0.0 {
                        let rect = RECT {
                            left: v[0] as i32,
                            top: v[1] as i32,
                            right: (v[0] + v[2]) as i32,
                            bottom: (v[1] + v[3]) as i32,
                        };
                        return Some(Anchor { rect, kind: AnchorKind::Caret });
                    }
                }
            }
        }
        let is_text = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).is_ok()
            || el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId).is_ok();
        if !is_text {
            return None;
        }
        let rect = el.CurrentBoundingRectangle().ok()?;
        if rect.right <= rect.left || rect.bottom <= rect.top {
            return None;
        }
        Some(Anchor { rect, kind: AnchorKind::Field })
    }
}

/// Up to `max_chars` characters just before the caret, through the field's text pattern.
unsafe fn text_before_caret(el: &IUIAutomationElement, max_chars: i32) -> Option<String> {
    unsafe {
        let tp = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).ok()?;
        let selection = tp.GetSelection().ok()?;
        if selection.Length().ok()? < 1 {
            return None;
        }
        let range = selection.GetElement(0).ok()?.Clone().ok()?;
        // the caret is where the selection starts: collapse to that point, then step back a few characters
        range.MoveEndpointByRange(TextPatternRangeEndpoint_End, &range, TextPatternRangeEndpoint_Start).ok()?;
        range.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -max_chars).ok()?;
        let text = range.GetText(max_chars).ok()?;
        Some(text.to_string())
    }
}

fn classic_caret() -> Option<RECT> {
    unsafe {
        let mut gti = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        GetGUIThreadInfo(0, &mut gti).ok()?;
        let hwnd: HWND = gti.hwndCaret;
        if hwnd.is_invalid() {
            return None;
        }
        let mut tl = POINT { x: gti.rcCaret.left, y: gti.rcCaret.top };
        let mut br = POINT { x: gti.rcCaret.right, y: gti.rcCaret.bottom };
        let _ = ClientToScreen(hwnd, &mut tl);
        let _ = ClientToScreen(hwnd, &mut br);
        if br.y <= tl.y {
            return None;
        }
        Some(RECT { left: tl.x, top: tl.y, right: br.x, bottom: br.y })
    }
}

fn read_doubles(sa: *mut SAFEARRAY) -> Vec<f64> {
    use windows::Win32::System::Ole::{SafeArrayDestroy, SafeArrayGetElement, SafeArrayGetLBound, SafeArrayGetUBound};
    let mut out = Vec::new();
    unsafe {
        if sa.is_null() {
            return out;
        }
        let lb = SafeArrayGetLBound(sa, 1).unwrap_or(0);
        let ub = SafeArrayGetUBound(sa, 1).unwrap_or(-1);
        for i in lb..=ub {
            let mut v = 0f64;
            if SafeArrayGetElement(sa, &i, &mut v as *mut f64 as *mut _).is_ok() {
                out.push(v);
            }
        }
        let _ = SafeArrayDestroy(sa);
    }
    out
}
