//! The settings window: five pages in a sidebar (Writing help, Languages, Apps, Appearance, General), drawn by `verba_win::ui`.
//!
//! This file only says what is on each page and what a click does. It holds no state of its own: it reads the program's settings
//! every frame and, when something changed, sends the whole new settings back through `crate::update_settings`. Every change applies
//! at once (there is no Apply button).

use std::cell::Cell;
use std::sync::atomic::{AtomicIsize, Ordering};

use verba_win::running_apps;
use verba_win::ui::{self, ButtonStyle, Config, Ui};
use verba_win::{Look, Theme};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, HICON, IDYES, MB_ICONQUESTION, MB_YESNO, MF_GRAYED, MF_STRING, MessageBoxW, SW_SHOW,
    TPM_LEFTALIGN, TPM_RETURNCMD, TPM_TOPALIGN, TrackPopupMenu,
};
use windows::core::{PCWSTR, w};

use crate::settings::{ALWAYS_OFF, AppMode, Settings, ThemeMode};

/// The window being shown (0 when there is none): the menu of running programs needs it as its owner.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
const MENU_APP_BASE: usize = 3000;

thread_local! {
    /// Whether "Start with Windows" is on: asked of the registry once, not at every frame.
    static AUTOSTART: Cell<Option<bool>> = const { Cell::new(None) };
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// The colours of the window: the theme the user chose in the settings (Windows' own when "Follow Windows").
fn look() -> Look {
    match crate::current_settings().theme {
        ThemeMode::Auto => Look::system(),
        ThemeMode::Light => Look::with_theme(Theme::Light),
        ThemeMode::Dark => Look::with_theme(Theme::Dark),
    }
}

/// Shows the window (or brings it to the front when it is already open).
pub fn open(icon: HICON) {
    AUTOSTART.with(|a| a.set(None));
    let cfg = Config { title: "Verba settings", class: w!("VerbaSettings"), width: 900, height: 640, icon, look };
    if let Some(hwnd) = ui::open(cfg, Box::new(draw)) {
        WINDOW.store(hwnd.0 as isize, Ordering::SeqCst);
    }
}

/// The settings changed somewhere else (the tray menu): draw again.
pub fn refresh() {
    ui::refresh();
}

fn owner() -> Option<HWND> {
    let h = WINDOW.load(Ordering::SeqCst);
    (h != 0).then_some(HWND(h as *mut _))
}

// ───────────── one frame ─────────────

fn draw(ui: &mut Ui) {
    let before = crate::current_settings();
    let mut s = before.clone();

    ui.sidebar_begin("Verba", concat!("Version ", env!("CARGO_PKG_VERSION")));
    ui.nav_item("nav:writing", '\u{E70F}', "Writing help", 0);
    ui.nav_item("nav:languages", '\u{E774}', "Languages", 1);
    ui.nav_item("nav:apps", '\u{E71D}', "Apps", 2);
    ui.nav_item("nav:look", '\u{E790}', "Appearance", 3);
    ui.nav_item("nav:general", '\u{E713}', "General", 4);
    let mut on = !s.paused;
    if ui.sidebar_switch("master", "Suggestions", if on { "On" } else { "Paused" }, &mut on) {
        s.paused = !on;
    }

    match ui.page() {
        0 => writing(ui, &mut s),
        1 => languages(ui, &mut s),
        2 => apps(ui, &mut s),
        3 => appearance(ui, &mut s),
        _ => general(ui),
    }

    if s != before {
        ui.defer(move || crate::update_settings(|current| *current = s));
    }
}

fn writing(ui: &mut Ui, s: &mut Settings) {
    ui.page_begin("Writing help", "Choose what Verba does while you type. Everything here stays on your PC.");
    ui.section("While you type");
    ui.card_begin();
    ui.toggle_row(
        "fix",
        "Fix spelling when I finish a word",
        Some("A small “Fix” popup offers the right spelling. Press Tab to apply it, or Esc to keep your word."),
        &mut s.correction,
        true,
    );
    ui.toggle_row(
        "next",
        "Suggest the next word",
        Some("After a word and a space, the likely next word appears. Press Tab to accept it."),
        &mut s.predict_next,
        true,
    );
    ui.card_end();

    ui.section("My words");
    ui.card_begin();
    ui.toggle_row(
        "learn",
        "Learn the words I use",
        Some("Verba remembers how often you write each word, on this PC only, and offers your own words first. Password boxes and apps you excluded are never read."),
        &mut s.learn,
        true,
    );
    if ui.button_row(
        "mywords",
        "My words",
        Some("Words Verba should never flag as typos and should always offer. They are kept in a plain text file you can edit."),
        "Open my words…",
        ButtonStyle::Secondary,
        true,
    ) {
        ui.defer(crate::open_my_words);
    }
    if ui.button_row(
        "forget",
        "Forget what it learned",
        Some("Removes every learned word and word pair. The words you added yourself stay."),
        "Forget…",
        ButtonStyle::Danger,
        true,
    ) {
        ui.defer(confirm_forget);
    }
    ui.card_end();
}

fn languages(ui: &mut Ui, s: &mut Settings) {
    ui.page_begin("Languages", "Verba suggests words in the language you are typing.");
    ui.card_begin();
    ui.toggle_row(
        "follow",
        "Follow the keyboard language",
        Some("Suggest in the language of the keyboard layout you are typing with. Turn this off to guess the language from the letters instead."),
        &mut s.follow_keyboard,
        true,
    );
    ui.card_end();

    ui.section("Suggest in");
    ui.card_begin();
    ui.toggle_row("en", "English", None, &mut s.english, true);
    ui.toggle_row("fr", "Français", None, &mut s.french, true);
    ui.toggle_row("ar", "العربية", Some("Modern Standard Arabic"), &mut s.arabic, true);
    ui.toggle_row(
        "dz",
        "Algerian Darija",
        Some("In Latin letters (wach, kifach, 3lach…) and in Arabic letters (واش، كيفاش، علاش…)"),
        &mut s.darija,
        true,
    );
    ui.card_end();
}

fn apps(ui: &mut Ui, s: &mut Settings) {
    ui.page_begin("Apps", "Choose the programs where Verba works.");
    ui.section("Where Verba works");
    ui.card_begin();
    let mut mode = if s.app_mode == AppMode::OnlyThese { 1 } else { 0 };
    if ui.segmented_row(
        "mode",
        "Verba works",
        Some("The list below holds the apps this rule applies to. Switching starts a new list."),
        &["Everywhere except the listed apps", "Only in the listed apps"],
        &mut mode,
    ) {
        // switching the meaning of the list starts a new list: the old one meant the opposite
        s.app_mode = if mode == 1 { AppMode::OnlyThese } else { AppMode::AllExcept };
        s.apps.clear();
    }
    ui.card_end();

    ui.section(if s.app_mode == AppMode::OnlyThese { "Apps where Verba works" } else { "Apps where Verba is off" });
    ui.card_begin();
    let items = s.apps.clone();
    if let Some(i) = ui.removable_list("apps", &items, "No app in the list yet.") {
        let app = items[i].clone();
        // "all except": the app leaves the list of the ones that are off; "only these": it leaves the ones that are on
        s.set_app(&app, s.app_mode == AppMode::AllExcept);
    }
    if ui.button_row("add", "Add an app", Some("Choose one of the programs that are open now."), "Add a running app…", ButtonStyle::Primary, true) {
        ui.defer(add_app);
    }
    ui.card_end();

    ui.card_begin();
    ui.note('\u{E72E}', "Password managers and Verba itself are always off, whatever you choose.");
    ui.card_end();
}

fn appearance(ui: &mut Ui, s: &mut Settings) {
    ui.page_begin("Appearance", "The look of Verba's popup and of this window.");
    ui.section("Theme");
    ui.card_begin();
    let mut theme = match s.theme {
        ThemeMode::Auto => 0,
        ThemeMode::Light => 1,
        ThemeMode::Dark => 2,
    };
    if ui.theme_tiles("theme", ["Follow Windows", "Light", "Dark"], &mut theme) {
        s.theme = match theme {
            1 => ThemeMode::Light,
            2 => ThemeMode::Dark,
            _ => ThemeMode::Auto,
        };
    }
    ui.note('\u{E946}', "Follow Windows uses your Windows light or dark mode and your accent colour.");
    ui.card_end();
}

fn general(ui: &mut Ui) {
    ui.page_begin("General", "Starting Verba, checking a text field, and about this program.");
    ui.section("Start");
    ui.card_begin();
    let mut auto = AUTOSTART.with(|a| match a.get() {
        Some(v) => v,
        None => {
            let v = crate::autostart_enabled();
            a.set(Some(v));
            v
        }
    });
    // starting with Windows points at this program's file: only the real (release) build offers it
    if ui.toggle_row(
        "autostart",
        "Start Verba when Windows starts",
        Some("Verba then starts quietly in the tray, without the opening animation."),
        &mut auto,
        !cfg!(debug_assertions),
    ) {
        AUTOSTART.with(|a| a.set(Some(auto)));
        ui.defer(move || crate::set_autostart(auto));
    }
    if ui.button_row(
        "check",
        "Check a text field",
        Some("Press “Run check”, then click in a text field of any app. After 3 seconds a report says whether Verba can read it."),
        "Run check",
        ButtonStyle::Secondary,
        true,
    ) {
        ui.defer(crate::field_check);
    }
    ui.card_end();

    ui.section("About");
    ui.card_begin();
    ui.about_block("Verba", concat!("Version ", env!("CARGO_PKG_VERSION")), "");
    ui.info_row("Free for personal and other noncommercial use.", Some("Using Verba for work needs a commercial license."));
    if ui.button_row("license", "License", Some("The terms Verba is released under (PolyForm Noncommercial 1.0.0)."), "Open", ButtonStyle::Secondary, true) {
        ui.defer(|| open_doc("LICENSE"));
    }
    if ui.button_row("commercial", "Commercial use", Some("How a company or a freelancer gets a license."), "Open", ButtonStyle::Secondary, true) {
        ui.defer(|| open_doc("COMMERCIAL.md"));
    }
    ui.card_end();
}

// ───────────── what the buttons do ─────────────

fn confirm_forget() {
    let ask = wide("Forget every word Verba learned from what you typed? The words you added yourself stay.");
    let title = wide("Verba");
    let answer = unsafe { MessageBoxW(owner(), PCWSTR(ask.as_ptr()), PCWSTR(title.as_ptr()), MB_YESNO | MB_ICONQUESTION) };
    if answer == IDYES {
        crate::forget_learned();
    }
}

/// Opens one of the documents installed next to the program (or, in a development build, in the project folder).
fn open_doc(name: &str) {
    let Ok(exe) = std::env::current_exe() else { return };
    for dir in exe.ancestors().skip(1).take(4) {
        let path = dir.join(name);
        if path.exists() {
            let file = wide(&path.display().to_string());
            unsafe {
                ShellExecuteW(None, w!("open"), PCWSTR(file.as_ptr()), None, None, SW_SHOW);
            }
            return;
        }
    }
}

/// The running programs in a pop-up list at the pointer; the one chosen goes on the list.
fn add_app() {
    let settings = crate::current_settings();
    let picked = unsafe {
        let apps = running_apps();
        let Ok(menu) = CreatePopupMenu() else { return };
        for (i, app) in apps.iter().enumerate() {
            let listed = settings.apps.contains(app);
            let always_off = ALWAYS_OFF.contains(&app.as_str());
            let flags = if listed || always_off { MF_STRING | MF_GRAYED } else { MF_STRING };
            let text = wide(app);
            let _ = AppendMenuW(menu, flags, MENU_APP_BASE + i, PCWSTR(text.as_ptr()));
        }
        if apps.is_empty() {
            let _ = AppendMenuW(menu, MF_STRING | MF_GRAYED, MENU_APP_BASE, w!("(no app with a window)"));
        }
        let mut at = POINT::default();
        let _ = GetCursorPos(&mut at);
        let chosen = TrackPopupMenu(menu, TPM_LEFTALIGN | TPM_TOPALIGN | TPM_RETURNCMD, at.x, at.y, None, owner().unwrap_or_default(), None);
        let _ = DestroyMenu(menu);
        (chosen.0 as usize).checked_sub(MENU_APP_BASE).and_then(|i| apps.get(i).cloned())
    };
    if let Some(app) = picked {
        // "all except": the app goes on the list of the ones that are off; "only these": on the list of the ones that are on
        crate::update_settings(|s| s.set_app(&app, s.app_mode == AppMode::OnlyThese));
    }
}
