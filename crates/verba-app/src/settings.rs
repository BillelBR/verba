//! What the user can change, kept in a small text file (`%APPDATA%\Verba\settings.txt`).
//!
//! One `key=value` per line. Nothing else is stored: no text, no history.

use std::path::PathBuf;

use verba_core::Lang;

use crate::keys::{Action, Binding, Keys};

/// Programs where Verba is always off, whatever the user chose: it must never read what is typed into a vault.
pub const ALWAYS_OFF: &[&str] = &[
    "verba.exe",
    "keepass.exe",
    "keepassxc.exe",
    "1password.exe",
    "bitwarden.exe",
    "lastpass.exe",
    "dashlane.exe",
    "enpass.exe",
    "nordpass.exe",
    "roboform.exe",
];

/// Follow Windows (light or dark), or always one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeMode {
    Auto,
    Light,
    Dark,
}

/// What the list of programs means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppMode {
    /// Verba works everywhere except in the listed programs.
    AllExcept,
    /// Verba works only in the listed programs.
    OnlyThese,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub theme: ThemeMode,
    pub english: bool,
    pub french: bool,
    pub arabic: bool,
    /// Algerian Darija written in Latin letters (off until the user turns it on).
    pub darija: bool,
    pub paused: bool,
    /// Show a spelling fix when a word is finished (off until the user turns it on).
    pub correction: bool,
    /// Offer the likely next word after a finished word and a space (off until the user turns it on).
    pub predict_next: bool,
    /// Learn the words the user types, on this PC only, to offer them later (off until the user turns it on).
    pub learn: bool,
    /// Ask the project's releases page once a day whether a newer Verba exists (off until the user turns it on: it is the only thing that
    /// goes over the network).
    pub check_updates: bool,
    /// A folder (OneDrive, Dropbox...) where my words and my translations are kept in step with the other PCs; empty when not syncing.
    pub sync_folder: String,
    /// The keys that drive the popup: accept, previous, next, close (Tab, left, right and Esc unless the user changed them).
    pub keys: Keys,
    /// The language a word is translated into (a word in that language goes to the next one: English, then French, then Arabic).
    pub translate_to: Lang,
    /// Suggest in the language of the keyboard layout (instead of guessing from the letters typed).
    pub follow_keyboard: bool,
    /// A line break in a text Verba types (a translation, a text put right) is the Enter key, or Shift+Enter, which is a new line in chat
    /// apps where Enter sends the message.
    pub line_break_shift: bool,
    pub app_mode: AppMode,
    /// Program file names, lower case (`chrome.exe`).
    pub apps: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Auto,
            english: true,
            french: true,
            arabic: true,
            darija: false,
            paused: false,
            correction: false,
            predict_next: false,
            learn: false,
            check_updates: false,
            sync_folder: String::new(),
            keys: Keys::default(),
            translate_to: Lang::En,
            follow_keyboard: true,
            line_break_shift: false,
            app_mode: AppMode::AllExcept,
            apps: Vec::new(),
        }
    }
}

impl Settings {
    pub fn lang_on(&self, lang: Lang) -> bool {
        match lang {
            Lang::En => self.english,
            Lang::Fr => self.french,
            Lang::Ar => self.arabic,
            Lang::Dz => self.darija,
        }
    }

    #[cfg(test)]
    pub fn set_lang(&mut self, lang: Lang, on: bool) {
        match lang {
            Lang::En => self.english = on,
            Lang::Fr => self.french = on,
            Lang::Ar => self.arabic = on,
            Lang::Dz => self.darija = on,
        }
    }

    /// Whether Verba may work in this program. An unknown program (empty name) is allowed unless the list is "only these".
    pub fn allows_app(&self, exe: &str) -> bool {
        let exe = exe.to_lowercase();
        if ALWAYS_OFF.contains(&exe.as_str()) {
            return false;
        }
        let listed = self.apps.iter().any(|a| *a == exe);
        match self.app_mode {
            AppMode::AllExcept => !listed,
            AppMode::OnlyThese => listed,
        }
    }

    /// Turns Verba on or off for one program, whatever the mode means for the list.
    pub fn set_app(&mut self, exe: &str, on: bool) {
        let exe = exe.trim().to_lowercase();
        if exe.is_empty() {
            return;
        }
        // in "all except" the list holds the programs that are off; in "only these" it holds the ones that are on
        let on_the_list = match self.app_mode {
            AppMode::AllExcept => !on,
            AppMode::OnlyThese => on,
        };
        self.apps.retain(|a| *a != exe);
        if on_the_list {
            self.apps.push(exe);
            self.apps.sort();
        }
    }

    /// The Verba folder in the user's AppData: where the settings, the personal words and what was learned are kept.
    pub fn dir() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Verba"))
    }

    fn path() -> Option<PathBuf> {
        Self::dir().map(|d| d.join("settings.txt"))
    }

    /// Reads the file; anything missing or unreadable keeps its default.
    pub fn load() -> Self {
        Self::parse(&Self::path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default())
    }

    pub(crate) fn parse(text: &str) -> Self {
        let mut s = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (key.trim(), value.trim());
            let on = value == "1";
            match key {
                "theme" => {
                    s.theme = match value {
                        "light" => ThemeMode::Light,
                        "dark" => ThemeMode::Dark,
                        _ => ThemeMode::Auto,
                    }
                }
                "english" => s.english = on,
                "french" => s.french = on,
                "arabic" => s.arabic = on,
                "darija" => s.darija = on,
                "paused" => s.paused = on,
                "correction" => s.correction = on,
                "predict_next" => s.predict_next = on,
                "learn" => s.learn = on,
                "check_updates" => s.check_updates = on,
                "sync_folder" => s.sync_folder = value.trim().to_string(),
                "key_accept" => s.keys.accept = Binding::parse(value).unwrap_or(Keys::default().accept),
                "key_prev" => s.keys.prev = Binding::parse(value).unwrap_or(Keys::default().prev),
                "key_next" => s.keys.next = Binding::parse(value).unwrap_or(Keys::default().next),
                "key_translate" => {
                    s.keys.translate = Binding::parse(value).filter(|b| Binding::usable_hotkey(b.vk, b.mods)).unwrap_or(Keys::default().translate)
                }
                "key_fixkeys" => {
                    s.keys.fixkeys = Binding::parse(value).filter(|b| Binding::usable_hotkey(b.vk, b.mods)).unwrap_or(Keys::default().fixkeys)
                }
                "key_undo" => {
                    s.keys.undo = Binding::parse(value).filter(|b| Binding::usable_hotkey(b.vk, b.mods)).unwrap_or(Keys::default().undo)
                }
                "translate_to" => {
                    s.translate_to = match value {
                        "fr" => Lang::Fr,
                        "ar" => Lang::Ar,
                        _ => Lang::En,
                    }
                }
                "key_close" => s.keys.close = Binding::parse(value).unwrap_or(Keys::default().close),
                "follow_keyboard" => s.follow_keyboard = on,
                "line_break" => s.line_break_shift = value == "shift",
                "app_mode" => s.app_mode = if value == "only" { AppMode::OnlyThese } else { AppMode::AllExcept },
                "apps" => {
                    s.apps = value.split(';').map(|a| a.trim().to_lowercase()).filter(|a| !a.is_empty()).collect();
                    s.apps.sort();
                    s.apps.dedup();
                }
                _ => {}
            }
        }
        s
    }

    pub(crate) fn render(&self) -> String {
        let theme = match self.theme {
            ThemeMode::Auto => "auto",
            ThemeMode::Light => "light",
            ThemeMode::Dark => "dark",
        };
        let flag = |b: bool| if b { "1" } else { "0" };
        let mode = if self.app_mode == AppMode::OnlyThese { "only" } else { "except" };
        format!(
            "theme={theme}\nenglish={}\nfrench={}\narabic={}\ndarija={}\npaused={}\ncorrection={}\npredict_next={}\nlearn={}\ncheck_updates={}\nsync_folder={}\nkey_accept={}\nkey_prev={}\nkey_next={}\nkey_close={}\nkey_translate={}\nkey_fixkeys={}\nkey_undo={}\ntranslate_to={}\nfollow_keyboard={}\nline_break={}\napp_mode={mode}\napps={}\n",
            flag(self.english),
            flag(self.french),
            flag(self.arabic),
            flag(self.darija),
            flag(self.paused),
            flag(self.correction),
            flag(self.predict_next),
            flag(self.learn),
            flag(self.check_updates),
            self.sync_folder.replace(['\r', '\n'], ""),
            self.keys.get(Action::Accept).render(),
            self.keys.get(Action::Prev).render(),
            self.keys.get(Action::Next).render(),
            self.keys.get(Action::Close).render(),
            self.keys.translate.render(),
            self.keys.fixkeys.render(),
            self.keys.undo.render(),
            self.translate_to.code(),
            flag(self.follow_keyboard),
            if self.line_break_shift { "shift" } else { "enter" },
            self.apps.join(";")
        )
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, self.render());
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

    #[test]
    fn by_default_every_app_is_allowed_except_password_managers_and_verba() {
        let s = Settings::default();
        assert!(s.allows_app("chrome.exe") && s.allows_app("notepad.exe") && s.allows_app(""));
        assert!(!s.allows_app("KeePass.exe"), "a password manager is never read, and names are not case sensitive");
        assert!(!s.allows_app("verba.exe"));
    }

    #[test]
    fn turning_an_app_off_and_on_in_all_except_mode() {
        let mut s = Settings::default();
        s.set_app("Chrome.exe", false);
        assert!(!s.allows_app("chrome.exe") && s.allows_app("code.exe"));
        assert_eq!(s.apps, ["chrome.exe"]);
        s.set_app("chrome.exe", true);
        assert!(s.allows_app("chrome.exe") && s.apps.is_empty());
    }

    #[test]
    fn only_these_mode_allows_just_the_listed_apps() {
        let mut s = Settings { app_mode: AppMode::OnlyThese, ..Settings::default() };
        assert!(!s.allows_app("chrome.exe"));
        s.set_app("chrome.exe", true);
        assert!(s.allows_app("chrome.exe") && !s.allows_app("code.exe"));
        s.set_app("chrome.exe", false);
        assert!(!s.allows_app("chrome.exe"));
        // the always-off programs stay off even when listed
        s.set_app("keepass.exe", true);
        assert!(!s.allows_app("keepass.exe"));
    }

    #[test]
    fn settings_survive_a_save_and_a_load() {
        let mut s = Settings { theme: ThemeMode::Dark, follow_keyboard: false, correction: true, predict_next: true, learn: true, app_mode: AppMode::OnlyThese, ..Settings::default() };
        s.set_lang(Lang::Fr, false);
        s.set_app("code.exe", true);
        s.set_app("slack.exe", true);
        assert_eq!(Settings::parse(&s.render()), s);
    }

    #[test]
    fn correction_is_off_until_the_user_turns_it_on() {
        assert!(!Settings::default().correction);
        assert!(Settings::parse("correction=1").correction);
    }

    #[test]
    fn next_word_and_learning_are_off_until_the_user_turns_them_on() {
        let d = Settings::default();
        assert!(!d.predict_next && !d.learn);
        let s = Settings::parse("predict_next=1\nlearn=1");
        assert!(s.predict_next && s.learn);
    }

    #[test]
    fn the_update_check_is_off_until_the_user_turns_it_on_and_survives_a_save() {
        assert!(!Settings::default().check_updates);
        let mut s = Settings::default();
        s.check_updates = true;
        assert!(Settings::parse(&s.render()).check_updates);
    }

    #[test]
    fn the_sync_folder_is_off_until_chosen_and_survives_a_save_with_spaces() {
        assert!(Settings::default().sync_folder.is_empty());
        let mut s = Settings::default();
        s.sync_folder = r"C:\Users\Some One\OneDrive\Verba sync".to_string();
        assert_eq!(Settings::parse(&s.render()).sync_folder, s.sync_folder);
    }

    #[test]
    fn darija_is_off_until_the_user_turns_it_on() {
        assert!(!Settings::default().darija && !Settings::default().lang_on(Lang::Dz));
        let mut s = Settings::parse("darija=1");
        assert!(s.lang_on(Lang::Dz));
        s.set_lang(Lang::Dz, false);
        assert!(!s.darija);
        assert_eq!(Settings::parse(&Settings { darija: true, ..Settings::default() }.render()).darija, true);
    }

    #[test]
    fn the_keys_default_to_tab_left_right_esc_and_survive_a_save() {
        let d = Settings::default();
        assert_eq!(d.keys, Keys::default());
        let mut s = Settings::default();
        s.keys.assign(Action::Accept, Binding::new(0x0D, 0));
        s.keys.assign(Action::Next, Binding::new(0x28, 0));
        s.keys.assign(Action::Prev, Binding::new(0x26, 0));
        let back = Settings::parse(&s.render());
        assert_eq!(back.keys, s.keys);
        assert_eq!(back, s);
    }

    #[test]
    fn the_undo_shortcut_survives_a_save_and_a_bad_one_falls_back() {
        let mut s = Settings::default();
        s.keys.undo = Binding::new(0x55, crate::keys::MOD_CTRL | crate::keys::MOD_ALT);
        assert_eq!(Settings::parse(&s.render()).keys.undo, s.keys.undo);
        // a plain key cannot be a shortcut for every app
        assert_eq!(Settings::parse("key_undo=Z\n").keys.undo, Keys::default().undo);
    }

    #[test]
    fn the_translate_settings_default_and_survive_a_save() {
        let d = Settings::default();
        assert_eq!((d.translate_to, d.keys.translate), (Lang::En, Keys::default().translate));
        let mut s = Settings::default();
        s.translate_to = Lang::Ar;
        s.keys.translate = Binding::new(0x44, crate::keys::MOD_CTRL | crate::keys::MOD_ALT);
        assert_eq!(Settings::parse(&s.render()), s);
        // a shortcut that is not a shortcut (no Ctrl or Alt) falls back to the default
        assert_eq!(Settings::parse("key_translate=9:0").keys.translate, Keys::default().translate);
    }

    #[test]
    fn a_bad_key_in_the_file_falls_back_to_its_default() {
        let s = Settings::parse("key_accept=65:0\nkey_next=oops\nkey_prev=37:0");
        assert_eq!(s.keys, Keys::default(), "a letter alone cannot be a key, text that is not a key is ignored");
    }

    #[test]
    fn a_damaged_file_falls_back_to_the_defaults() {
        assert_eq!(Settings::parse("nonsense\n=\ntheme\napps"), Settings::default());
        assert!(Settings::parse("follow_keyboard=0").follow_keyboard == false);
        // the key of a line break: Enter unless the file says shift, and it survives a save
        assert!(!Settings::default().line_break_shift);
        assert!(!Settings::parse("line_break=whatever").line_break_shift);
        let shifted = Settings { line_break_shift: true, ..Settings::default() };
        assert!(Settings::parse(&shifted.render()).line_break_shift);
    }
}
