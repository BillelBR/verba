//! Prints what Verba sends to the sentence translator for a text: abbreviations written out, spoken Arabic made standard.
//!
//!   cargo run -q -p verba-local --example prepare -- ar "واش راك؟ كلش مليح؟"
//!   cargo run -q -p verba-local --example prepare -- en "gg guys, gn, see u tomorrow"

use verba_core::Lang;
use verba_local::{colloquial, slang};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [lang, texts @ ..] = args.as_slice() else {
        eprintln!("usage: prepare <en|ar> <text>...");
        return;
    };
    for text in texts {
        let out = match lang.as_str() {
            "ar" => colloquial::shipped().expand(&slang::shipped().expand_into(text, Lang::Ar)),
            _ => slang::shipped().expand(text),
        };
        println!("{out}");
    }
}
