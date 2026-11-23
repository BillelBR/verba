//! The programs the Apps page lists: every program of this PC that Verba can find (the Start menu, and the programs that have a window open now),
//! with a switch for Verba in each. Finding them reads the shortcut files, so it is done once, on a thread of its own, and the page says that it is looking.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use verba_win::{IconPixels, InstalledApp, icon_pixels, installed_apps, running_programs};

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
    /// The program's own icon, once it has been read (a worker thread reads them one after the other; until then the page shows a letter).
    pub icon: Option<Arc<IconPixels>>,
}

struct Catalog {
    started: bool,
    loaded: bool,
    installed: Vec<InstalledApp>,
    running: Vec<String>,
    running_at: Option<Instant>,
    /// The icons read so far, by the program's file name.
    icons: BTreeMap<String, Arc<IconPixels>>,
    /// The programs whose icon was asked for (read, or waiting to be).
    asked: BTreeSet<String>,
    /// The icons waiting to be read: file name and full path.
    queue: Vec<(String, String)>,
    /// A thread is reading the icons of the queue.
    worker: bool,
    /// What to call when something new can be drawn.
    done: Option<fn()>,
}

static CATALOG: Mutex<Catalog> = Mutex::new(Catalog {
    started: false,
    loaded: false,
    installed: Vec::new(),
    running: Vec::new(),
    running_at: None,
    icons: BTreeMap::new(),
    asked: BTreeSet::new(),
    queue: Vec::new(),
    worker: false,
    done: None,
});

/// Starts looking for the programs, once. `done` is called when the list is ready and when more icons are (the page draws again).
pub fn ensure_loaded(done: fn()) {
    let Ok(mut c) = CATALOG.lock() else { return };
    if c.started {
        return;
    }
    c.started = true;
    c.done = Some(done);
    drop(c);
    std::thread::spawn(move || {
        let installed = installed_apps();
        if let Ok(mut c) = CATALOG.lock() {
            for a in &installed {
                ask_icon(&mut c, &a.exe, &a.path);
            }
            c.installed = installed;
            c.loaded = true;
            start_worker(&mut c);
        }
        done();
    });
}

/// Puts a program's icon in the queue, once.
fn ask_icon(c: &mut Catalog, exe: &str, path: &str) {
    if !path.is_empty() && c.asked.insert(exe.to_string()) {
        c.queue.push((exe.to_string(), path.to_string()));
    }
}

/// Starts the thread that reads the icons of the queue one after the other (when there is none yet), and tells the page every few icons.
fn start_worker(c: &mut MutexGuard<Catalog>) {
    if c.worker || c.queue.is_empty() {
        return;
    }
    c.worker = true;
    std::thread::spawn(|| {
        let mut since_told = 0;
        loop {
            let job = {
                let Ok(mut c) = CATALOG.lock() else { return };
                if c.queue.is_empty() {
                    c.worker = false;
                    let done = c.done;
                    drop(c);
                    if since_told > 0 {
                        done.map(|f| f());
                    }
                    return;
                }
                c.queue.remove(0)
            };
            let icon = icon_pixels(&job.1);
            let mut tell = None;
            if let Ok(mut c) = CATALOG.lock() {
                if let Some(px) = icon {
                    c.icons.insert(job.0, Arc::new(px));
                    since_told += 1;
                    if since_told >= 8 {
                        since_told = 0;
                        tell = c.done;
                    }
                }
            }
            if let Some(f) = tell {
                f();
            }
        }
    });
}

/// Whether the programs are still being looked for.
pub fn loading() -> bool {
    CATALOG.lock().map(|c| !c.loaded).unwrap_or(false)
}

/// A file name made readable: `chrome.exe` is `Chrome`, `winword.exe` is `Winword`.
pub fn pretty(exe: &str) -> String {
    // a few programs of Windows whose file name is not their name
    if exe == "explorer.exe" {
        return "File Explorer".to_string();
    }
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
    let mut rows: Vec<Row> = installed.iter().map(|a| Row { name: a.name.clone(), exe: a.exe.clone(), running: running.contains(&a.exe), icon: None }).collect();
    for exe in running.iter().chain(listed) {
        // a program of Windows that is only open is not offered (one the user listed is shown, so that it can be switched back)
        let hidden = SYSTEM_HOSTS.contains(&exe.as_str()) && !listed.contains(exe);
        if !hidden && !rows.iter().any(|r| r.exe == *exe) {
            rows.push(Row { name: pretty(exe), exe: exe.clone(), running: running.contains(exe), icon: None });
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
        let programs = running_programs();
        for (exe, path) in &programs {
            ask_icon(&mut c, exe, path);
        }
        start_worker(&mut c);
        c.running = programs.into_iter().map(|(exe, _)| exe).collect();
        c.running_at = Some(Instant::now());
    }
    let mut rows = merge(&c.installed, &c.running, &settings.apps, filter);
    for row in &mut rows {
        row.icon = c.icons.get(&row.exe).cloned();
    }
    rows
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
        InstalledApp { name: name.to_string(), exe: exe.to_string(), path: String::new() }
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
        assert_eq!(pretty("explorer.exe"), "File Explorer");
        assert!(always_off("keepass.exe") && !always_off("chrome.exe"));
    }
}
