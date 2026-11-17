//! Shows the suggestion popup above the field you are typing in, for a few seconds.
//!
//!   cargo run -p verba-win --example popup_demo -- --lang ar --delay 4 --seconds 6
//!
//! Click into any text field during the delay. The popup never takes the focus from it.
//! `--fixed` shows it at a fixed place instead (no field needed); `--select N` highlights candidate N; `--hint "text"` sets the key cap.
//! `--text "a whole sentence"` shows that one candidate instead (a translated sentence wraps in a card of its own); `--lead "AR → EN"` adds the label.

use std::time::{Duration, Instant};

use verba_core::{Lang, Suggestion};
use verba_win::{Anchor, AnchorKind, Locator, Look, Overlay, Theme};
use windows::Win32::Foundation::RECT;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let lang = match get("--lang").as_deref() {
        Some("fr") => Lang::Fr,
        Some("ar") => Lang::Ar,
        Some("dz") => Lang::Dz,
        _ => Lang::En,
    };
    let delay: u64 = get("--delay").and_then(|s| s.parse().ok()).unwrap_or(3);
    let seconds: u64 = get("--seconds").and_then(|s| s.parse().ok()).unwrap_or(5);
    let look = if args.iter().any(|a| a == "--dark") { Look::with_theme(Theme::Dark) } else if args.iter().any(|a| a == "--light") { Look::with_theme(Theme::Light) } else { Look::system() };

    let words: &[&str] = match lang {
        Lang::En => &["hello", "help", "held"],
        Lang::Fr => &["bonjour", "bonsoir", "bon"],
        Lang::Ar => &["مرحبا", "مرحبًا بك", "مرحبتين"],
        Lang::Dz => &["wach", "wesh", "wech"],
    };
    let suggestions: Vec<Suggestion> = match get("--text") {
        Some(text) => vec![Suggestion { text, lang }],
        None => words.iter().map(|w| Suggestion { text: (*w).to_string(), lang }).collect(),
    };
    let lead = get("--lead");

    let fixed = args.iter().any(|a| a == "--fixed");
    let select: usize = get("--select").and_then(|s| s.parse().ok()).unwrap_or(0);
    let hint = get("--hint");
    println!("Click into a text field now. The popup appears in {delay}s and stays {seconds}s.");
    let locator = Locator::new().expect("UI Automation");
    let overlay = Overlay::new().expect("popup window");

    if let Some(h) = &hint {
        overlay.set_hint(h);
    }
    std::thread::sleep(Duration::from_secs(delay));
    let located = if fixed {
        Some(Anchor { rect: RECT { left: 600, top: 500, right: 603, bottom: 520 }, kind: AnchorKind::Caret })
    } else {
        locator.locate()
    };
    match located {
        Some(anchor) => {
            println!("anchor: {:?} left={} top={} right={} bottom={}", anchor.kind, anchor.rect.left, anchor.rect.top, anchor.rect.right, anchor.rect.bottom);
            overlay.show_with(anchor, &suggestions, select, look, lead.as_deref());
            let r = overlay.rect();
            println!("popup:  left={} top={} right={} bottom={}", r.left, r.top, r.right, r.bottom);
        }
        None => println!("no text field has the focus: nothing to show"),
    }
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        overlay.pump();
        std::thread::sleep(Duration::from_millis(15));
    }
}
