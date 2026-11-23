//! The programs the Apps page lists: every program of this PC that Verba can find (the Start menu, and the programs that have a window open now),
//! with a switch for Verba in each. Finding them reads the shortcut files, so it is done once, on a thread of its own, and the page says that it is looking.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use verba_win::{InstalledApp, installed_apps, running_apps};

use crate::settings::{ALWAYS_OFF, Settings};

/// One line of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The name shown (`Google Chrome`); for a program that is only known by its file, the file's name made readable (`Chrome`).
    pub name: String,
    /// The file name in lower case: what the settings hold.
    pub exe: String,
    /// The program has a window open now.
    pub running: bool,
}

struct Catalog {
    started: bool,
    loaded: bool,
    installed: Vec<InstalledApp>,
    running: Vec<String>,
    running_at: Option<Instant>,
}

static CATALOG: Mutex<Catalog> = Mutex::new(Catalog { started: false, loaded: false, installed: Vec::new(), running: Vec::new(), running_at: None });

/// Starts looking for the programs, once. `done` is called when the list is ready (the page draws again).
pub fn ensure_loaded(done: impl Fn() + Send + 'static) {
    let Ok(mut c) = CATALOG.lock() else { return };
    if c.started {
        return;
    }
    c.started = true;
    drop(c);
    std::thread::spawn(move || {
        let installed = installed_apps();
        if let Ok(mut c) = CATALOG.lock() {
            c.installed = installed;
            c.loaded = true;
        }
        done();
    });
}

/// Whether the programs are still being looked for.
pub fn loading() -> bool {
    CATALOG.lock().map(|c| !c.loaded).unwrap_or(false)
}

/// A file name made readable: `chrome.exe` is `Chrome`, `winword.exe` is `Winword`.
pub fn pretty(exe: &str) -> String {
    let stem = exe.strip_suffix(".exe").unwrap_or(exe);
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Programs of Windows that have a window but are not programs a person types in or chooses (the host of the Store apps, the search and the
/// input panels): they are not listed when they are only found open.
const SYSTEM_HOSTS: &[&str] = &[
    "applicationframehost.exe",
    "textinputhost.exe",
    "searchhost.exe",
    "searchapp.exe",
    "shellexperiencehost.exe",
    "startmenuexperiencehost.exe",
    "lockapp.exe",
    "systemsettings.exe",
    "widgets.exe",
    "dwm.exe",
    "ctfmon.exe",
];

/// The rows: the programs found, the ones that are open now, and the ones in the user's list that are neither (a program that was removed from
/// the PC, or typed by its file name), sorted by name; `filter` keeps the ones whose name or file contains it.
pub fn merge(installed: &[InstalledApp], running: &[String], listed: &[String], filter: &str) -> Vec<Row> {
    let mut rows: Vec<Row> = installed.iter().map(|a| Row { name: a.name.clone(), exe: a.exe.clone(), running: running.contains(&a.exe) }).collect();
    for exe in running.iter().chain(listed) {
        // a program of Windows that is only open is not offered (one the user listed is shown, so that it can be switched back)
        let hidden = SYSTEM_HOSTS.contains(&exe.as_str()) && !listed.contains(exe);
        if !hidden && !rows.iter().any(|r| r.exe == *exe) {
            rows.push(Row { name: pretty(exe), exe: exe.clone(), running: running.contains(exe) });
        }
    }
    let f = filter.trim().to_lowercase();
    if !f.is_empty() {
        rows.retain(|r| r.name.to_lowercase().contains(&f) || r.exe.contains(&f));
    }
    rows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.exe.cmp(&b.exe)));
    rows
}

/// The rows to show now (the open programs are looked at again every three seconds).
pub fn rows(settings: &Settings, filter: &str) -> Vec<Row> {
    let Ok(mut c) = CATALOG.lock() else { return Vec::new() };
    if c.running_at.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) {
        c.running = running_apps();
        c.running_at = Some(Instant::now());
    }
    merge(&c.installed, &c.running, &settings.apps, filter)
}

/// Whether Verba is always off in this program (a password manager, or Verba itself).
pub fn always_off(exe: &str) -> bool {
    ALWAYS_OFF.contains(&exe)
}

/// A file name typed by the person: `notepad`, `Notepad.exe` and ` notepad.exe ` are the same program. `None` when it is not a file name.
pub fn typed_exe(text: &str) -> Option<String> {
    let t = text.trim().trim_matches('"').to_lowercase();
    let name = t.rsplit(['\\', '/']).next().unwrap_or("").trim().to_string();
    if name.is_empty() || name.len() > 100 || name.chars().any(|c| c.is_control() || "<>:\"|?*".contains(c)) {
        return None;
    }
    Some(if name.ends_with(".exe") { name } else { format!("{name}.exe") })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, exe: &str) -> InstalledApp {
        InstalledApp { name: name.to_string(), exe: exe.to_string() }
    }

    #[test]
    fn the_rows_join_what_is_installed_what_is_open_and_what_the_user_listed() {
        let installed = [app("Notepad++", "notepad++.exe"), app("Google Chrome", "chrome.exe")];
        let running = ["chrome.exe".to_string(), "discord.exe".to_string()];
        let listed = ["oldtool.exe".to_string(), "chrome.exe".to_string()];
        let rows = merge(&installed, &running, &listed, "");
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["Discord", "Google Chrome", "Notepad++", "Oldtool"], "sorted by name, once each");
        // the host of the Store apps is open on every PC and is not a program to switch
        let with_host = merge(&installed, &["applicationframehost.exe".to_string(), "code.exe".to_string()], &[], "");
        assert!(with_host.iter().all(|r| r.exe != "applicationframehost.exe") && with_host.iter().any(|r| r.exe == "code.exe"));
        assert!(rows.iter().find(|r| r.exe == "chrome.exe").unwrap().running);
        assert!(!rows.iter().find(|r| r.exe == "notepad++.exe").unwrap().running);
        // a program only known by its file is named from the file
        assert_eq!(rows.iter().find(|r| r.exe == "discord.exe").unwrap().name, "Discord");
    }

    #[test]
    fn the_filter_looks_at_the_name_and_at_the_file() {
        let installed = [app("Google Chrome", "chrome.exe"), app("Microsoft Word", "winword.exe"), app("Notepad++", "notepad++.exe")];
        assert_eq!(merge(&installed, &[], &[], "word").len(), 1);
        assert_eq!(merge(&installed, &[], &[], "WINWORD").len(), 1, "capitals do not matter, the file counts");
        assert_eq!(merge(&installed, &[], &[], "  ").len(), 3, "an empty filter keeps everything");
        assert!(merge(&installed, &[], &[], "zzz").is_empty());
    }

    #[test]
    fn a_file_name_typed_by_hand_is_made_one_program() {
        assert_eq!(typed_exe("Notepad").as_deref(), Some("notepad.exe"));
        assert_eq!(typed_exe(" NOTEPAD.EXE ").as_deref(), Some("notepad.exe"));
        assert_eq!(typed_exe("\"C:\\Program Files\\App\\app.exe\"").as_deref(), Some("app.exe"), "a path gives its file");
        assert_eq!(typed_exe(""), None);
        assert_eq!(typed_exe("a|b"), None);
        assert_eq!(pretty("chrome.exe"), "Chrome");
        assert!(always_off("keepass.exe") && !always_off("chrome.exe"));
    }
}
