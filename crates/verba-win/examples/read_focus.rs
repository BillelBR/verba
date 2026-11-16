//! Shows what Verba can read from the text field that has the focus, once every 300 ms.
//! For privacy it prints only numbers and the last letters' code points, never the text itself.
//!
//!   cargo run -p verba-win --example read_focus -- --seconds 5

use std::time::{Duration, Instant};

use verba_win::Locator;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seconds: u64 = args.iter().position(|a| a == "--seconds").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(5);
    let locator = Locator::new().expect("UI Automation");
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        match locator.snapshot(48) {
            Some(s) => {
                let tail: Vec<String> = s.before_caret.chars().rev().take(4).map(|c| format!("{:04X}", c as u32)).collect();
                println!(
                    "anchor {:?} ({},{})-({},{})  chars before caret: {}  last 4 (reversed) code points: {}",
                    s.anchor.kind,
                    s.anchor.rect.left,
                    s.anchor.rect.top,
                    s.anchor.rect.right,
                    s.anchor.rect.bottom,
                    s.before_caret.chars().count(),
                    tail.join(" ")
                );
            }
            None => println!("no readable text field has the focus -> {}", locator.diagnose()),
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}
