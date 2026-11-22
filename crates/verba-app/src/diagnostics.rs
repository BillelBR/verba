//! The report the settings can show when something does not work: what Verba is, what it found on this PC and which of its shortcuts Windows
//! refused. It holds no text the person wrote (no word, no translation, no sentence): only counts, names of languages and keyboards, and the
//! name of the last program in front. A pure function from the facts to the text; `main.rs` gathers the facts.

/// What the report says.
#[derive(Debug, Default, PartialEq)]
pub struct Facts {
    pub version: String,
    pub build: String,
    pub windows: String,
    /// `(name, language Verba has a list for)` of each installed keyboard layout.
    pub layouts: Vec<(String, Option<String>)>,
    /// `(language, words in its list, on or off)`.
    pub lists: Vec<(String, usize, bool)>,
    /// `Some(entries)` once read, `Some(None)` installed but not read yet, `None` not installed.
    pub dictionary: Option<Option<usize>>,
    pub sentence_pairs: Vec<String>,
    pub my_words: usize,
    pub my_translations: usize,
    /// The settings that decide what Verba does, as `name: value` lines.
    pub settings: Vec<String>,
    /// The shortcuts and what Windows said: `(name, key, registered)`.
    pub shortcuts: Vec<(String, String, bool)>,
    pub last_app: Option<String>,
    pub last_app_allowed: Option<bool>,
}

pub fn render(f: &Facts) -> String {
    let mut out = format!("Verba {} ({})\nWindows: {}\n", f.version, f.build, if f.windows.is_empty() { "unknown" } else { &f.windows });
    out.push_str("\nKeyboards installed in Windows:\n");
    if f.layouts.is_empty() {
        out.push_str("  none found\n");
    }
    for (name, lang) in &f.layouts {
        out.push_str(&format!("  {name}: {}\n", lang.as_deref().map_or("no word list for this language".to_string(), |l| format!("{l} (has a word list)"))));
    }
    out.push_str("\nWord lists:\n");
    for (lang, words, on) in &f.lists {
        out.push_str(&format!("  {lang}: {words} words, {}\n", if *on { "on" } else { "off" }));
    }
    out.push_str(&match f.dictionary {
        None => "Dictionary: NOT installed (word translation does not work)\n".to_string(),
        Some(None) => "Dictionary: installed, not read yet (it is read at the first translation)\n".to_string(),
        Some(Some(n)) => format!("Dictionary: {n} entries\n"),
    });
    out.push_str(&if f.sentence_pairs.is_empty() {
        "Sentence translation: NOT installed\n".to_string()
    } else {
        format!("Sentence translation: {}\n", f.sentence_pairs.join(", "))
    });
    out.push_str(&format!("\nMy words: {}, my translations: {}\n", f.my_words, f.my_translations));
    out.push_str("\nSettings:\n");
    for s in &f.settings {
        out.push_str(&format!("  {s}\n"));
    }
    out.push_str("\nShortcuts:\n");
    for (name, key, ok) in &f.shortcuts {
        out.push_str(&format!("  {name}: {key}{}\n", if *ok { "" } else { "  <- Windows refused it: another program uses this key; choose another in the settings" }));
    }
    match (&f.last_app, f.last_app_allowed) {
        (Some(app), Some(allowed)) => out.push_str(&format!("\nLast program in front: {app} ({})\n", if allowed { "Verba works there" } else { "Verba is OFF there: see the Apps page" })),
        (Some(app), None) => out.push_str(&format!("\nLast program in front: {app}\n")),
        _ => out.push_str("\nNo program has been in front yet.\n"),
    }
    out.push_str("\nThis report has no text you wrote: only counts, names of languages and keyboards, and the last program's file name.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            version: "0.10.9".into(),
            build: "release".into(),
            windows: "Windows 10 Pro 22H2 (build 19045)".into(),
            layouts: vec![("English 00000409".into(), Some("en".into())), ("German 00000407".into(), None)],
            lists: vec![("English".into(), 150_000, true), ("Arabic".into(), 300_000, false)],
            dictionary: Some(None),
            sentence_pairs: vec!["AR \u{2192} EN".into()],
            my_words: 3,
            my_translations: 2,
            settings: vec!["theme: auto".into()],
            shortcuts: vec![("translate".into(), "Ctrl+Alt+T".into(), true), ("undo".into(), "Ctrl+Alt+Z".into(), false)],
            last_app: Some("chrome.exe".into()),
            last_app_allowed: Some(false),
        }
    }

    #[test]
    fn the_report_says_what_does_not_work() {
        let f = facts();
        let text = render(&f);
        assert!(text.contains("Verba 0.10.9 (release)"));
        assert!(text.contains("German 00000407: no word list"));
        assert!(text.contains("Arabic: 300000 words, off"));
        assert!(text.contains("installed, not read yet"));
        assert!(text.contains("undo: Ctrl+Alt+Z  <- Windows refused it"));
        assert!(text.contains("chrome.exe (Verba is OFF there"));
    }

    #[test]
    fn a_missing_dictionary_and_translator_are_said_plainly() {
        let text = render(&Facts { dictionary: None, ..Facts::default() });
        assert!(text.contains("Dictionary: NOT installed"));
        assert!(text.contains("Sentence translation: NOT installed"));
        assert!(text.contains("No program has been in front yet."));
        assert!(text.contains("Windows: unknown"));
    }

    #[test]
    fn the_report_has_no_word_the_person_wrote() {
        // the facts hold counts, never the words: nothing of `my_words` can reach the text
        let text = render(&facts());
        assert!(text.contains("My words: 3, my translations: 2"));
    }
}
