//! Shows the suggestion popup above the field you are typing in, for a few seconds.
//!
//!   cargo run -p verba-win --example popup_demo -- --lang ar --delay 4 --seconds 6
//!
//! Click into any text field during the delay. The popup never takes the focus from it.

use std::time::{Duration, Instant};

use verba_core::{Lang, Suggestion};
use verba_win::{Locator, Look, Overlay, Theme};

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
    let suggestions: Vec<Suggestion> = words.iter().map(|w| Suggestion { text: (*w).to_string(), lang }).collect();

    println!("Click into a text field now. The popup appears in {delay}s and stays {seconds}s.");
    let locator = Locator::new().expect("UI Automation");
    let overlay = Overlay::new().expect("popup window");

    std::thread::sleep(Duration::from_secs(delay));
    match locator.locate() {
        Some(anchor) => {
            println!("anchor: {:?} left={} top={} right={} bottom={}", anchor.kind, anchor.rect.left, anchor.rect.top, anchor.rect.right, anchor.rect.bottom);
            overlay.show(anchor, &suggestions, 0, look);
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
