//! caret-probe: a read-only test for Verba.
//!
//! Every 300 ms it looks at the app you are typing in and prints what Windows lets us know:
//! which program, what kind of field, and whether the caret position is available (two ways).
//! It never reads what you type: only the program name, the kind of field and numbers.
//!
//! Usage:  caret-probe [--seconds N] [--log FILE]
//! Then switch between apps (Notepad, Chrome, Word, VS Code, Slack...) and type a little in each.

use std::collections::HashSet;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationTextPattern2, IUIAutomationValuePattern,
    UIA_TextPattern2Id, UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GUITHREADINFO, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
};
use windows::core::{BOOL, PWSTR};

/// Program file name (for example `chrome.exe`) of the window's process.
fn exe_name(hwnd: HWND) -> String {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut buf = [0u16; 520];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
            let _ = CloseHandle(h);
            if ok.is_ok() {
                let full = String::from_utf16_lossy(&buf[..len as usize]);
                return full.rsplit('\\').next().unwrap_or("?").to_string();
            }
        }
        "?".to_string()
    }
}

/// Way 1: the classic Windows caret (works in Notepad, many old apps; not in most modern ones).
fn classic_caret() -> Option<RECT> {
    unsafe {
        let mut gti = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        GetGUIThreadInfo(0, &mut gti).ok()?;
        if gti.hwndCaret.is_invalid() {
            return None;
        }
        let mut tl = POINT { x: gti.rcCaret.left, y: gti.rcCaret.top };
        let mut br = POINT { x: gti.rcCaret.right, y: gti.rcCaret.bottom };
        let _ = ClientToScreen(gti.hwndCaret, &mut tl);
        let _ = ClientToScreen(gti.hwndCaret, &mut br);
        Some(RECT { left: tl.x, top: tl.y, right: br.x, bottom: br.y })
    }
}

/// What UI Automation says about the focused field.
struct Uia {
    control_type: i32,
    class: String,
    text_pattern: bool,
    value_pattern: bool,
    /// Way 2: the caret range through TextPattern2 (modern apps, browsers with accessibility on).
    caret_range: bool,
    caret_rect: Option<[f64; 4]>,
    field: RECT,
}

fn uia_probe(uia: &IUIAutomation) -> Option<Uia> {
    unsafe {
        let el = uia.GetFocusedElement().ok()?;
        let control_type = el.CurrentControlType().ok()?.0;
        let class = el.CurrentClassName().map(|b| b.to_string()).unwrap_or_default();
        let field = el.CurrentBoundingRectangle().unwrap_or_default();
        let text_pattern = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId).is_ok();
        let value_pattern = el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId).is_ok();
        let mut caret_range = false;
        let mut caret_rect = None;
        if let Ok(tp2) = el.GetCurrentPatternAs::<IUIAutomationTextPattern2>(UIA_TextPattern2Id) {
            let mut active = BOOL(0);
            if let Ok(range) = tp2.GetCaretRange(&mut active) {
                caret_range = true;
                if let Ok(sa) = range.GetBoundingRectangles() {
                    let v = read_doubles(sa);
                    if v.len() >= 4 {
                        caret_rect = Some([v[0], v[1], v[2], v[3]]);
                    }
                }
            }
        }
        Some(Uia { control_type, class, text_pattern, value_pattern, caret_range, caret_rect, field })
    }
}

fn read_doubles(sa: *mut windows::Win32::System::Com::SAFEARRAY) -> Vec<f64> {
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

fn rect_str(r: &RECT) -> String {
    format!("({},{})-({},{})", r.left, r.top, r.right, r.bottom)
}

fn main() {
    let mut seconds = 60u64;
    let mut log_path = String::from("probe.log");
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--seconds" => { seconds = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(60); i += 1; }
            "--log" => { if let Some(p) = args.get(i + 1) { log_path = p.clone(); } i += 1; }
            _ => {}
        }
        i += 1;
    }

    unsafe { let _ = CoInitializeEx(None, COINIT_MULTITHREADED); }
    let uia: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.expect("UI Automation not available");
    let mut log = OpenOptions::new().create(true).append(true).open(&log_path).ok();

    println!("caret-probe: {seconds}s. Switch between your apps and type a little in each. Nothing you type is read.");
    println!("{:<18} {:<26} {:>4} {:<5} {:<5} {:<5} {:<22} {}", "program", "field class", "type", "text", "value", "caret", "caret position", "how");

    let start = Instant::now();
    let mut seen: HashSet<String> = HashSet::new();
    while start.elapsed() < Duration::from_secs(seconds) {
        let fg = unsafe { GetForegroundWindow() };
        if !fg.is_invalid() {
            let exe = exe_name(fg);
            let classic = classic_caret();
            let u = uia_probe(&uia);
            let (ctype, class, tp, vp, cr, how_pos) = match &u {
                Some(u) => {
                    let (pos, how) = if let Some(r) = &classic {
                        (rect_str(r), "classic")
                    } else if let Some(c) = u.caret_rect {
                        (format!("({:.0},{:.0}) {:.0}x{:.0}", c[0], c[1], c[2], c[3]), "uia-caret")
                    } else if u.text_pattern || u.value_pattern {
                        (rect_str(&u.field), "field-only")
                    } else {
                        ("-".to_string(), "none")
                    };
                    (u.control_type, u.class.clone(), u.text_pattern, u.value_pattern, u.caret_range, (pos, how))
                }
                None => (0, "?".into(), false, false, false, ("-".to_string(), "no-focus")),
            };
            let key = format!("{exe}|{class}|{ctype}|{tp}|{vp}|{cr}|{}", how_pos.1);
            if seen.insert(key) {
                let line = format!(
                    "{:<18} {:<26} {:>4} {:<5} {:<5} {:<5} {:<22} {}",
                    exe, class.chars().take(26).collect::<String>(), ctype, tp, vp, cr, how_pos.0, how_pos.1
                );
                println!("{line}");
                if let Some(f) = log.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
            }
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    println!("done. A line is printed once per program/field kind. Log: {log_path}");
}
