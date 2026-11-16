//! Plays the start-up splash for a pretend loading of a few seconds.
//!
//!   cargo run -p verba-win --example splash_demo -- --seconds 2.5 [--dark|--light]

use std::time::Duration;

use verba_win::{Look, Theme, run_with_splash};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seconds: f32 = args.iter().position(|a| a == "--seconds").and_then(|i| args.get(i + 1)).and_then(|s| s.parse().ok()).unwrap_or(2.5);
    let look = if args.iter().any(|a| a == "--dark") {
        Look::with_theme(Theme::Dark)
    } else if args.iter().any(|a| a == "--light") {
        Look::with_theme(Theme::Light)
    } else {
        Look::system()
    };
    let started = std::time::Instant::now();
    let answer = run_with_splash(look, move || {
        std::thread::sleep(Duration::from_secs_f32(seconds));
        42
    });
    println!("work returned {answer}; splash and work took {:.2} s in all", started.elapsed().as_secs_f32());
}
