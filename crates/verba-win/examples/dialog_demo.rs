//! Shows the form of `verba_win::dialog` without taking the focus, to look at it: `dialog_demo [light|dark] [translation|word]`.
//! The form stays open until the process is stopped; the button always answers with an error sentence, to see how one looks.

use verba_win::Theme;
use verba_win::dialog::{self, Field, Form};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let theme = if args.get(1).map(String::as_str) == Some("dark") { Theme::Dark } else { Theme::Light };
    let options = ["Arabic \u{2192} English", "English \u{2192} Arabic", "French \u{2192} English"];
    let word = args.get(2).map(String::as_str) == Some("word");
    let translation_fields = [Field { label: "Word", rtl: Some(|i| i == 0) }, Field { label: "Translation", rtl: Some(|i| i == 1) }];
    let word_fields = [Field { label: "Word", rtl: None }];
    let form = if word {
        Form { title: "Add a word", intro: "One word, 2 to 40 letters. Verba will not flag it as a typo.", choice: None, fields: &word_fields, initial: &["Snow"], ok_label: "Add", result_lines: 0, activate: false }
    } else {
        Form {
            title: "Write a translation",
            intro: "When you translate this word, your translations come first. Write several separated by ; (at most 5).",
            choice: Some(("Translate", &options)),
            fields: &translation_fields,
            initial: &["\u{062A}\u{0637}\u{0628}\u{064A}\u{0642}", "app; application"],
            ok_label: "Add",
            result_lines: 0,
            activate: false,
        }
    };
    if args.get(2).map(String::as_str) == Some("lookup") {
        let lookup_fields = [Field { label: "Word", rtl: None }];
        let form = Form {
            title: "Look up a word",
            intro: "A word in English, French, Arabic, or Algerian Darija in Latin letters.",
            choice: None,
            fields: &lookup_fields,
            initial: &["cold"],
            ok_label: "Look up",
            result_lines: 6,
            activate: false,
        };
        let text = "English \u{2192}\nFrench:  froid  \u{B7}  rhume  \u{B7}  glacial  \u{B7}  frais\nArabic:  \u{0628}\u{0627}\u{0631}\u{062F}  \u{B7}  \u{0632}\u{0643}\u{0627}\u{0645}\n";
        dialog::ask_with(None, theme, &form, &mut |_, _| dialog::Reply::Show(text.to_string()));
        return;
    }
    dialog::ask(None, theme, &form, &mut |_, _| Err("The translation is written in Arabic letters.".to_string()));
}
