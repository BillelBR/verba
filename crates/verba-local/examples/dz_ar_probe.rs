//! What the engine suggests for Arabic text typed as the user types it, with Darija on: `dz_ar_probe [--dz-ar] [text before the caret]...`
//! (`--dz-ar` also loads `data/words/dz-ar.txt`). Each argument is the text before the caret; the three suggestions are printed.

use verba_core::Lang;
use verba_local::LocalEngine;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut e = LocalEngine::load_dir(std::path::Path::new("data/words")).expect("data/words");
    if args.iter().any(|a| a == "--dz-ar") {
        e.load_dz(std::path::Path::new("data/words")).expect("dz lists");
    } else {
        e.add_dz_seed();
    }
    e.set_enabled(Lang::Dz, true);
    for text in args.iter().filter(|a| *a != "--dz-ar") {
        let list: Vec<String> = e.suggest_in(Lang::Ar, text, 3).into_iter().map(|s| s.text).collect();
        let next: Vec<String> = if text.ends_with(' ') { e.next_words(Some(Lang::Ar), text, 3).into_iter().map(|s| s.text).collect() } else { Vec::new() };
        println!("{text:>16} | {}  {}", list.join(" "), if next.is_empty() { String::new() } else { format!("[next: {}]", next.join(" ")) });
    }
}
