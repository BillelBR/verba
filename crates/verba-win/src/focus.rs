//! Finds where the user is typing, in any app, without reading what they type.
//!
//! Three ways, from most to least exact:
//! 1. the classic Windows caret (Notepad and older apps),
//! 2. the caret range through UI Automation `TextPattern2` (modern apps),
//! 3. the text field's own rectangle (Chromium-based apps such as VS Code and Slack).

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
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
            // modern fields answer through UI Automation; the classic Windows edit boxes only through their own messages
            let before_caret = text_before_caret(&el, max_chars).or_else(|| classic_edit_before_caret(max_chars))?;
            let anchor = classic_caret().map(|rect| Anchor { rect, kind: AnchorKind::Caret }).or_else(|| uia_anchor(&el))?;
            Some(Snapshot { anchor, before_caret, layout: crate::apps::keyboard_lang(), app: crate::apps::foreground_app().unwrap_or_default() })
        }
    }
}

/// How much of a selection is read: a word, or a few sentences to translate (the caller refuses what is too long for it).
const SELECTION_READ_MAX: usize = 1300;

impl Locator {
    /// The word or the sentences to translate, read right now: the text selected in the focused field, or else the word just before the caret.
    /// Nothing is kept: the text is returned to the caller. Password fields are never read. `None` when there is no readable field
    /// or no word.
    pub fn pick(&self, max_chars: i32) -> Option<Picked> {
        unsafe {
            let el = self.uia.GetFocusedElement().ok()?;
            if is_password(&el) {
                return None;
            }
            let anchor = classic_caret().map(|rect| Anchor { rect, kind: AnchorKind::Caret }).or_else(|| uia_anchor(&el))?;
            let layout = crate::apps::keyboard_lang();
            let app = crate::apps::foreground_app().unwrap_or_default();
            if let Some(text) = uia_selection(&el).or_else(|| classic_edit_selection(SELECTION_READ_MAX)) {
                return Some(Picked { text, anchor, layout, app, selected: true });
            }
            let before = text_before_caret(&el, max_chars).or_else(|| classic_edit_before_caret(max_chars))?;
            let word = verba_core::current_word(&before);
            if word.is_empty() {
                return None;
            }
            Some(Picked { text: word.to_string(), anchor, layout, app, selected: false })
        }
    }
}

impl Locator {
    /// The text selected in the focused field, up to `max_chars` characters (a longer selection comes back cut: the caller compares the
    /// length with its own limit). Unlike `pick`, a word before the caret is not taken instead: without a selection this is `None`.
    /// Nothing is kept. Password fields are never read.
    pub fn pick_selection(&self, max_chars: usize) -> Option<Picked> {
        unsafe {
            let el = self.uia.GetFocusedElement().ok()?;
            if is_password(&el) {
                return None;
            }
            let anchor = classic_caret().map(|rect| Anchor { rect, kind: AnchorKind::Caret }).or_else(|| uia_anchor(&el))?;
            let text = uia_selection_up_to(&el, max_chars).or_else(|| classic_edit_selection(max_chars))?;
            Some(Picked { text, anchor, layout: crate::apps::keyboard_lang(), app: crate::apps::foreground_app().unwrap_or_default(), selected: true })
        }
    }
}

impl Locator {
    /// A step-by-step account of reading the focused field, for finding out why a field cannot be read.
    /// It never contains the text itself, only which steps worked.
    pub fn diagnose(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let step = |out: &mut String, name: &str, r: &windows::core::Result<()>| {
            let _ = match r {
                Ok(()) => write!(out, "{name}: ok; "),
                Err(e) => write!(out, "{name}: FAILED {:08X}; ", e.code().0 as u32),
            };
        };
        unsafe {
            let el = match self.uia.GetFocusedElement() {
                Ok(e) => e,
                Err(e) => return format!("GetFocusedElement FAILED {:08X}", e.code().0 as u32),
            };
            let class = el.CurrentClassName().map(|b| b.to_string()).unwrap_or_default();
            let _ = write!(out, "class '{class}', control type {}; ", el.CurrentControlType().map(|t| t.0).unwrap_or(-1));
            let _ = write!(out, "password: {}; ", is_password(&el));
            let tp = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId);
            let vp = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId);
            let _ = write!(out, "TextPattern: {}; ValuePattern: {}; ", tp.is_ok(), vp.is_ok());
            let Ok(tp) = tp else { return out };
            let sel = match tp.GetSelection() {
                Ok(s) => s,
                Err(e) => return format!("{out}GetSelection FAILED {:08X}", e.code().0 as u32),
            };
            let n = sel.Length().unwrap_or(-1);
            let _ = write!(out, "selections: {n}; ");
            let range = match sel.GetElement(0).and_then(|r| r.Clone()) {
                Ok(r) => r,
                Err(e) => return format!("{out}GetElement/Clone FAILED {:08X}", e.code().0 as u32),
            };
            step(&mut out, "collapse", &range.MoveEndpointByRange(TextPatternRangeEndpoint_End, &range, TextPatternRangeEndpoint_Start));
            let moved = range.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -48);
            let _ = write!(out, "step back: {:?}; ", moved.as_ref().map_err(|e| e.code().0 as u32));
            match range.GetText(48) {
                Ok(t) => {
                    let _ = write!(out, "text length {}", t.len());
                }
                Err(e) => {
                    let _ = write!(out, "GetText FAILED {:08X}", e.code().0 as u32);
                }
            }
        }
        out
    }
}

/// What `Locator::pick` found: the word to translate.
pub struct Picked {
    pub text: String,
    pub anchor: Anchor,
    /// The language of the keyboard in use.
    pub layout: Option<verba_core::Lang>,
    /// The program in front, as its file name.
    pub app: String,
    /// The text was selected (typing replaces it); otherwise it is the word before the caret (it must be erased to be replaced).
    pub selected: bool,
}

/// What `Locator::snapshot` found.
pub struct Snapshot {
    pub anchor: Anchor,
    /// The last few characters before the caret.
    pub before_caret: String,
    /// The language of the keyboard in use (English, French or Arabic), `None` for any other layout.
    pub layout: Option<verba_core::Lang>,
    /// The program in front, as its file name (`chrome.exe`); empty when unknown.
    pub app: String,
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

/// Up to `max_chars` characters just before the caret in a classic Windows edit box (Notepad, WinForms, dialogs),
/// which answers to messages instead of a text pattern. Every message has a short timeout, so a stuck program cannot stall Verba.
/// Password boxes are skipped.
fn classic_edit_before_caret(max_chars: i32) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GWL_STYLE, GetClassNameW, GetWindowLongW, SMTO_ABORTIFHUNG, SendMessageTimeoutW};
    const EM_GETSEL: u32 = 0x00B0;
    const EM_LINEINDEX: u32 = 0x00BB;
    const EM_LINELENGTH: u32 = 0x00C1;
    const EM_GETLINE: u32 = 0x00C4;
    const EM_LINEFROMCHAR: u32 = 0x00C9;
    const ES_PASSWORD: u32 = 0x0020;
    unsafe {
        let mut gti = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        GetGUIThreadInfo(0, &mut gti).ok()?;
        let hwnd = gti.hwndFocus;
        if hwnd.is_invalid() {
            return None;
        }
        let mut name = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut name);
        let class = String::from_utf16_lossy(&name[..n.max(0) as usize]).to_ascii_lowercase();
        if !(class == "edit" || class.contains(".edit.") || class.starts_with("richedit")) {
            return None;
        }
        if GetWindowLongW(hwnd, GWL_STYLE) as u32 & ES_PASSWORD != 0 {
            return None;
        }
        let ask = |msg: u32, wp: usize, lp: isize| -> Option<isize> {
            let mut result = 0usize;
            let r = SendMessageTimeoutW(hwnd, msg, WPARAM(wp), LPARAM(lp), SMTO_ABORTIFHUNG, 150, Some(&mut result));
            (r.0 != 0).then_some(result as isize)
        };
        // where the caret is (the start of the selection)
        let (mut start, mut end) = (0u32, 0u32);
        ask(EM_GETSEL, &mut start as *mut u32 as usize, &mut end as *mut u32 as isize)?;
        let line = ask(EM_LINEFROMCHAR, start as usize, 0)?;
        let line_start = ask(EM_LINEINDEX, line as usize, 0)?;
        if line_start < 0 || (start as isize) < line_start {
            return None;
        }
        let line_len = ask(EM_LINELENGTH, line_start as usize, 0)?.clamp(0, 4000) as usize;
        if line_len == 0 {
            return Some(String::new());
        }
        // EM_GETLINE: the first u16 of the buffer says how many characters fit
        let mut buf = vec![0u16; line_len + 2];
        buf[0] = (line_len + 1) as u16;
        let got = ask(EM_GETLINE, line as usize, buf.as_mut_ptr() as isize)?.clamp(0, line_len as isize) as usize;
        let before = ((start as isize - line_start) as usize).min(got);
        let text = String::from_utf16_lossy(&buf[..before]);
        let skip = text.chars().count().saturating_sub(max_chars.max(0) as usize);
        Some(text.chars().skip(skip).collect())
    }
}

/// The text selected in the field, through its text pattern (empty selection: `None`).
unsafe fn uia_selection(el: &IUIAutomationElement) -> Option<String> {
    unsafe { uia_selection_up_to(el, SELECTION_READ_MAX) }
}

/// The text selected in a modern field, at most `max` characters.
unsafe fn uia_selection_up_to(el: &IUIAutomationElement, max: usize) -> Option<String> {
    unsafe {
        let tp = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).ok()?;
        let selection = tp.GetSelection().ok()?;
        if selection.Length().ok()? < 1 {
            return None;
        }
        let range = selection.GetElement(0).ok()?;
        let text = range.GetText(max as i32).ok()?.to_string();
        (!text.trim().is_empty()).then_some(text)
    }
}

/// The text selected in a classic Windows edit box (Notepad, WinForms, dialogs), read through its own messages.
/// At most `max_chars` characters; a longer selection gives `None`. Password boxes are skipped.
fn classic_edit_selection(max_chars: usize) -> Option<String> {
    unsafe {
        let mut gti = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        GetGUIThreadInfo(0, &mut gti).ok()?;
        classic_edit_selection_of(gti.hwndFocus, max_chars)
    }
}

/// The same for one given window (the one with the focus is found by the caller).
fn classic_edit_selection_of(hwnd: HWND, max_chars: usize) -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{GWL_STYLE, GetClassNameW, GetWindowLongW, SMTO_ABORTIFHUNG, SendMessageTimeoutW};
    const EM_GETSEL: u32 = 0x00B0;
    const WM_GETTEXT: u32 = 0x000D;
    const WM_GETTEXTLENGTH: u32 = 0x000E;
    const ES_PASSWORD: u32 = 0x0020;
    unsafe {
        if hwnd.is_invalid() {
            return None;
        }
        let mut name = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut name);
        let class = String::from_utf16_lossy(&name[..n.max(0) as usize]).to_ascii_lowercase();
        if !(class == "edit" || class.contains(".edit.") || class.starts_with("richedit")) {
            return None;
        }
        if GetWindowLongW(hwnd, GWL_STYLE) as u32 & ES_PASSWORD != 0 {
            return None;
        }
        let ask = |msg: u32, wp: usize, lp: isize| -> Option<isize> {
            let mut result = 0usize;
            let r = SendMessageTimeoutW(hwnd, msg, WPARAM(wp), LPARAM(lp), SMTO_ABORTIFHUNG, 150, Some(&mut result));
            (r.0 != 0).then_some(result as isize)
        };
        let (mut start, mut end) = (0u32, 0u32);
        ask(EM_GETSEL, &mut start as *mut u32 as usize, &mut end as *mut u32 as isize)?;
        if end <= start || (end - start) as usize > max_chars {
            return None;
        }
        let len = ask(WM_GETTEXTLENGTH, 0, 0)?.clamp(0, 200_000) as usize;
        if len == 0 || end as usize > len {
            return None;
        }
        let mut buf = vec![0u16; len + 1];
        let got = ask(WM_GETTEXT, buf.len(), buf.as_mut_ptr() as isize)?.clamp(0, len as isize) as usize;
        if (end as usize) > got {
            return None;
        }
        let text = String::from_utf16_lossy(&buf[start as usize..end as usize]);
        (!text.trim().is_empty()).then_some(text)
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

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::core::w;
    use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, SendMessageW, SetWindowTextW, WINDOW_EX_STYLE, WINDOW_STYLE, WS_POPUP};

    /// A real, hidden edit box with some text: the way the classic selection reader sees Notepad. No focus and no keys are involved.
    fn edit_box(text: &str, style: u32) -> HWND {
        unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), w!("EDIT"), w!(""), WINDOW_STYLE(WS_POPUP.0 | style), 0, 0, 300, 40, None, None, Some(hinst.into()), None).unwrap();
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr())).unwrap();
            hwnd
        }
    }

    fn select(hwnd: HWND, start: usize, end: usize) {
        const EM_SETSEL: u32 = 0x00B1;
        unsafe {
            SendMessageW(hwnd, EM_SETSEL, Some(WPARAM(start)), Some(LPARAM(end as isize)));
        }
    }

    #[test]
    fn the_selected_text_of_an_edit_box_is_read() {
        let hwnd = edit_box("please say hello again", 0);
        select(hwnd, 11, 16);
        assert_eq!(classic_edit_selection_of(hwnd, 400).as_deref(), Some("hello"));
        // an Arabic word, counted in UTF-16 units like Windows does
        let ar = edit_box("\u{0645}\u{0631}\u{062D}\u{0628}\u{0627} \u{0628}\u{0643}", 0);
        select(ar, 0, 5);
        assert_eq!(classic_edit_selection_of(ar, 400).as_deref(), Some("\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}"));
        unsafe {
            let _ = DestroyWindow(hwnd);
            let _ = DestroyWindow(ar);
        }
    }

    #[test]
    fn nothing_selected_or_too_much_selected_gives_nothing() {
        let hwnd = edit_box("one two three", 0);
        select(hwnd, 4, 4);
        assert_eq!(classic_edit_selection_of(hwnd, 400), None, "only a caret");
        select(hwnd, 0, 13);
        assert_eq!(classic_edit_selection_of(hwnd, 5), None, "longer than the limit");
        assert_eq!(classic_edit_selection_of(hwnd, 400).as_deref(), Some("one two three"));
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }

    #[test]
    fn a_password_box_is_never_read() {
        const ES_PASSWORD: u32 = 0x0020;
        let hwnd = edit_box("secret", ES_PASSWORD);
        select(hwnd, 0, 6);
        assert_eq!(classic_edit_selection_of(hwnd, 400), None);
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}
