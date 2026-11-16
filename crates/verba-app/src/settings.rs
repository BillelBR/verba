//! What the user can change from the tray menu, kept in a small text file (`%APPDATA%\Verba\settings.txt`).
//!
//! One `key=value` per line. Nothing else is stored: no text, no history.

use std::path::PathBuf;

use verba_core::Lang;

/// Follow Windows (light or dark), or always one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Auto,
    Light,
    Dark,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub theme: ThemeMode,
    pub english: bool,
    pub french: bool,
    pub arabic: bool,
    pub paused: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { theme: ThemeMode::Auto, english: true, french: true, arabic: true, paused: false }
    }
}

impl Settings {
    pub fn lang_on(&self, lang: Lang) -> bool {
        match lang {
            Lang::En => self.english,
            Lang::Fr => self.french,
            Lang::Ar => self.arabic,
        }
    }

    pub fn set_lang(&mut self, lang: Lang, on: bool) {
        match lang {
            Lang::En => self.english = on,
            Lang::Fr => self.french = on,
            Lang::Ar => self.arabic = on,
        }
    }

    fn path() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Verba").join("settings.txt"))
    }

    /// Reads the file; anything missing or unreadable keeps its default.
    pub fn load() -> Self {
        let mut s = Self::default();
        let Some(text) = Self::path().and_then(|p| std::fs::read_to_string(p).ok()) else { return s };
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let on = value.trim() == "1";
            match key.trim() {
                "theme" => {
                    s.theme = match value.trim() {
                        "light" => ThemeMode::Light,
                        "dark" => ThemeMode::Dark,
                        _ => ThemeMode::Auto,
                    }
                }
                "english" => s.english = on,
                "french" => s.french = on,
                "arabic" => s.arabic = on,
                "paused" => s.paused = on,
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let theme = match self.theme {
            ThemeMode::Auto => "auto",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        };
        let flag = |b: bool| if b { "1" } else { "0" };
        let text = format!(
            "theme={theme}\nenglish={}\nfrench={}\narabic={}\npaused={}\n",
            flag(self.english),
            flag(self.french),
            flag(self.arabic),
            flag(self.paused)
        );
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_switch_one_by_one() {
        let mut s = Settings::default();
        assert!(s.lang_on(Lang::Ar));
        s.set_lang(Lang::Ar, false);
        assert!(!s.lang_on(Lang::Ar) && s.lang_on(Lang::En) && s.lang_on(Lang::Fr));
    }
}
