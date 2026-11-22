//! Backup: the words and translations the user wrote, and the settings, in one plain text file.
//!
//! ```text
//! # Verba backup 1
//! @@ words
//! Snow
//! @@ translations
//! ar>en<TAB>تطبيق<TAB>app; application
//! @@ settings
//! theme=dark
//! ```
//!
//! What Verba learned from the user's writing (`learned.txt`) is not in it: it describes what the person writes, and it grows back. The file
//! is for keeping, and for moving to another PC. Pure functions, no files: `main.rs` reads and writes them.

use verba_local::UserTranslations;

/// The file extension Verba proposes.
pub const EXTENSION: &str = "verba-backup";
const HEADER: &str = "# Verba backup 1";

/// What a backup file holds.
#[derive(Debug, PartialEq)]
pub struct Backup {
    pub words: Vec<String>,
    pub translations: UserTranslations,
    /// The text of `settings.txt`, when the file has it.
    pub settings: Option<String>,
}

/// The text of a backup file.
pub fn render(words: &[String], translations: &UserTranslations, settings: &str) -> String {
    let mut out = format!("{HEADER}\n# Made by Verba. Keep it, or open it from the settings of Verba on another PC (General > Backup > Import).\n");
    out.push_str("@@ words\n");
    for w in words {
        out.push_str(w);
        out.push('\n');
    }
    out.push_str("@@ translations\n");
    out.push_str(&translations.to_dictionary_text());
    out.push_str("@@ settings\n");
    out.push_str(settings);
    if !settings.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Reads a backup file. `None` when it is not one (the first line is not the header).
pub fn parse(text: &str) -> Option<Backup> {
    let text = text.trim_start_matches('\u{feff}');
    if text.lines().find(|l| !l.trim().is_empty()).map(str::trim) != Some(HEADER) {
        return None;
    }
    let (mut words, mut translations, mut settings) = (Vec::new(), String::new(), None::<String>);
    let mut section = "";
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("@@ ") {
            section = match name.trim() {
                "words" => "words",
                "translations" => "translations",
                "settings" => {
                    settings.get_or_insert_with(String::new);
                    "settings"
                }
                _ => "",
            };
            continue;
        }
        match section {
            "words" => {
                let w = line.trim();
                if !w.is_empty() && !w.starts_with('#') {
                    words.push(w.to_string());
                }
            }
            "translations" => {
                translations.push_str(line);
                translations.push('\n');
            }
            "settings" => {
                if let Some(s) = &mut settings {
                    s.push_str(line);
                    s.push('\n');
                }
            }
            _ => {}
        }
    }
    Some(Backup { words, translations: UserTranslations::parse(&translations), settings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use verba_core::Lang;

    fn sample() -> (Vec<String>, UserTranslations) {
        let mut t = UserTranslations::new();
        t.add(Lang::Ar, Lang::En, "تطبيق", "app; application").unwrap();
        t.add(Lang::En, Lang::Ar, "scroll", "تمرير").unwrap();
        (vec!["Snow".to_string(), "iPhone".to_string()], t)
    }

    #[test]
    fn a_backup_comes_back_as_it_went() {
        let (words, tr) = sample();
        let text = render(&words, &tr, "theme=dark\nlearn=1\n");
        let back = parse(&text).expect("a backup");
        assert_eq!(back.words, words);
        assert_eq!(back.translations, tr);
        assert_eq!(back.settings.as_deref(), Some("theme=dark\nlearn=1\n"));
    }

    #[test]
    fn a_file_that_is_not_a_backup_is_refused() {
        assert!(parse("").is_none());
        assert!(parse("theme=dark\n").is_none(), "a settings file is not a backup");
        assert!(parse("# Verba backup 2\n@@ words\nx\n").is_none(), "another version is not read as this one");
    }

    #[test]
    fn a_backup_without_some_parts_is_read_with_what_it_has() {
        let b = parse("\u{feff}# Verba backup 1\n@@ words\n\n# a note\nSnow\n  Verba \n").expect("a backup");
        assert_eq!(b.words, ["Snow", "Verba"]);
        assert!(b.translations.is_empty());
        assert_eq!(b.settings, None, "no settings section: the settings are left alone");
        // a part Verba does not know is skipped
        let b = parse("# Verba backup 1\n@@ future\nwhatever\n@@ words\nSnow\n").expect("a backup");
        assert_eq!(b.words, ["Snow"]);
    }
}
