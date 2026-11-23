//! The settings window: five pages in a sidebar (Writing help, Languages, Apps, Appearance, General), drawn by `verba_win::ui`.
//!
//! This file only says what is on each page and what a click does. It holds no state of its own: it reads the program's settings
//! every frame and, when something changed, sends the whole new settings back through `crate::update_settings`. Every change applies
//! at once (there is no Apply button).

use std::cell::Cell;
use std::sync::atomic::{AtomicIsize, Ordering};

use verba_core::Lang;
use verba_win::dialog::{self, Field, Form};
use verba_win::ui::{self, ButtonStyle, Config, ListEntry, Ui};
use verba_win::{Look, Theme};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{HICON, SW_SHOW};
use windows::core::{PCWSTR, w};

use crate::keys::{Action, Binding, Keys};
use crate::settings::{AppMode, Settings, ThemeMode};

/// The window being shown (0 when there is none): the forms and messages it opens need it as their owner.
static WINDOW: AtomicIsize = AtomicIsize::new(0);

thread_local! {
    /// Why the last key the user pressed was not taken (shown on the Keyboard page until the next good key).
    static KEY_PROBLEM: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// The same for the translate shortcut on the Translate page.
    static TRANSLATE_PROBLEM: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// The same for the wrong-keyboard shortcut.
    static FIXKEYS_PROBLEM: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// The same for the undo shortcut on the Writing help page.
    static UNDO_PROBLEM: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// The same for the dictation shortcut.
    static DICTATE_PROBLEM: Cell<Option<&'static str>> = const { Cell::new(None) };
    /// The same for the quick window's shortcut.
    static QUICK_PROBLEM: Cell<Option<&'static str>> = const { Cell::new(None) };
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
    ui.nav_item("nav:translate", '\u{E8C1}', "Translate", 2);
    ui.nav_item("nav:dictation", '\u{E720}', "Dictation", 3);
    ui.nav_item("nav:keys", '\u{E765}', "Keyboard", 4);
    ui.nav_item("nav:apps", '\u{E71D}', "Apps", 5);
    ui.nav_item("nav:look", '\u{E790}', "Appearance", 6);
    ui.nav_item("nav:general", '\u{E713}', "General", 7);
    let mut on = !s.paused;
    if ui.sidebar_switch("master", "Suggestions", if on { "On" } else { "Paused" }, &mut on) {
        s.paused = !on;
    }

    match ui.page() {
        0 => writing(ui, &mut s),
        1 => languages(ui, &mut s),
        2 => translate(ui, &mut s),
        3 => dictation_page(ui, &mut s),
        4 => keyboard(ui, &mut s),
        5 => apps(ui, &mut s),
        6 => appearance(ui, &mut s),
        _ => general(ui, &mut s),
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
        Some("A small “Fix” popup offers the right spelling. Apply it with your accept key (Tab by default), or press Esc to keep your word."),
        &mut s.correction,
        true,
    );
    ui.toggle_row(
        "next",
        "Suggest the next word",
        Some("After a word and a space, the likely next word appears. Accept it with your accept key (Tab by default)."),
        &mut s.predict_next,
        true,
    );
    ui.card_end();

    ui.section("Undo");
    ui.card_begin();
    let current = s.keys.undo;
    let default = Keys::default().undo;
    let row = ui.key_row(
        "key:undo",
        "Put back the word Verba corrected",
        Some("Press it right after a fix and your own word comes back; Verba then leaves that word alone. It only works while the corrected word is still just before the cursor."),
        &current.label(),
        current == default,
    );
    if let Some((vk, mods)) = row.captured {
        if !Binding::usable_hotkey(vk, mods) {
            UNDO_PROBLEM.with(|p| p.set(Some("A shortcut that works in every app needs Ctrl or Alt together with another key.")));
        } else if Binding::new(vk, mods) == s.keys.translate || Binding::new(vk, mods) == s.keys.fixkeys {
            UNDO_PROBLEM.with(|p| p.set(Some("That key already has another job: choose another one.")));
        } else {
            s.keys.undo = Binding::new(vk, mods);
            UNDO_PROBLEM.with(|p| p.set(None));
        }
    }
    if row.reset {
        s.keys.undo = default;
        UNDO_PROBLEM.with(|p| p.set(None));
    }
    ui.card_end();
    if let Some(problem) = UNDO_PROBLEM.with(|p| p.get()) {
        ui.card_begin();
        ui.note('\u{E783}', problem);
        ui.card_end();
    }

    ui.section("My words");
    ui.card_begin();
    ui.toggle_row(
        "learn",
        "Learn the words I use",
        Some("Verba remembers how often you write each word, on this PC only, and offers your own words first. Password boxes and apps you excluded are never read."),
        &mut s.learn,
        true,
    );
    ui.card_end();

    ui.card_begin();
    let words = crate::my_words();
    let entries: Vec<ListEntry> = words.iter().map(|w| ListEntry { title: w.clone(), ..Default::default() }).collect();
    if let Some(i) = ui.entry_list("myword", &entries, "No word added yet. Names, brands and words of your work go here.") {
        let word = words[i].clone();
        ui.defer(move || crate::remove_my_word(&word));
    }
    if ui.button_row(
        "addword",
        "Add a word",
        Some("A word Verba should never flag as a typo and should always offer: a name, a brand, a term of your work."),
        "Add a word…",
        ButtonStyle::Primary,
        true,
    ) {
        ui.defer(add_word);
    }
    if ui.button_row(
        "mywords",
        "Edit as a text file",
        Some("The same words in a plain text file, one per line. Verba reads it again when you save it."),
        "Open my words…",
        ButtonStyle::Secondary,
        true,
    ) {
        ui.defer(crate::open_my_words);
    }
    ui.card_end();

    ui.card_begin();
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

fn translate(ui: &mut Ui, s: &mut Settings) {
    ui.page_begin("Translate", "Select a word or a sentence, press the shortcut, and the translation appears. It works offline.");
    ui.section("Translate into");
    ui.card_begin();
    let mut into = match s.translate_to {
        Lang::Fr => 1,
        Lang::Ar => 2,
        _ => 0,
    };
    if ui.segmented_row(
        "translate-to",
        "Translate into",
        Some("A word that is already in this language goes to English, then French, then Arabic."),
        &["English", "Français", "العربية"],
        &mut into,
    ) {
        s.translate_to = match into {
            1 => Lang::Fr,
            2 => Lang::Ar,
            _ => Lang::En,
        };
    }
    ui.card_end();

    ui.section("My translations");
    ui.card_begin();
    let mine = crate::my_translations();
    let items: Vec<ListEntry> = mine.iter().map(translation_entry).collect();
    if let Some(i) = ui.entry_list("mytr", &items, "No translation written yet. Add the ones the dictionary gets wrong or does not have.") {
        ui.defer(move || crate::remove_my_translation(i));
    }
    if ui.button_row(
        "addtr",
        "Write a translation",
        Some("For a word you select and translate: yours come first, before the dictionary's. A sentence is translated by the neural translator, which does not read this list."),
        "Add a translation…",
        ButtonStyle::Primary,
        true,
    ) {
        ui.defer(add_translation);
    }
    if ui.button_row(
        "mytrfile",
        "Edit as a text file",
        Some("One line per word in a plain text file: direction, the word and its translations. Verba reads it again when you save it."),
        "Open my translations…",
        ButtonStyle::Secondary,
        true,
    ) {
        ui.defer(crate::open_my_translations);
    }
    ui.card_end();

    ui.section("Shortcut");
    ui.card_begin();
    let current = s.keys.translate;
    let default = Keys::default().translate;
    let row = ui.key_row(
        "key:translate",
        "Translate the selection",
        Some("Works in every app. With nothing selected, it translates the word just before the cursor."),
        &current.label(),
        current == default,
    );
    if let Some((vk, mods)) = row.captured {
        if Binding::usable_hotkey(vk, mods) && (Binding::new(vk, mods) == s.keys.fixkeys || Binding::new(vk, mods) == s.keys.undo) {
            TRANSLATE_PROBLEM.with(|p| p.set(Some("That key already has another job: choose another one.")));
        } else if Binding::usable_hotkey(vk, mods) {
            s.keys.translate = Binding::new(vk, mods);
            TRANSLATE_PROBLEM.with(|p| p.set(None));
        } else {
            TRANSLATE_PROBLEM.with(|p| p.set(Some("A shortcut that works in every app needs Ctrl or Alt together with another key.")));
        }
    }
    if row.reset {
        s.keys.translate = default;
        TRANSLATE_PROBLEM.with(|p| p.set(None));
    }
    ui.note('\u{E946}', "Press the shortcut: the translations appear above the text. Choose with the arrow keys, put one in with Tab, close with Esc.");
    ui.card_end();
    if let Some(problem) = TRANSLATE_PROBLEM.with(|p| p.get()) {
        ui.card_begin();
        ui.note('\u{E783}', problem);
        ui.card_end();
    }

    ui.section("Quick window");
    ui.card_begin();
    let current = s.keys.quick;
    let default = Keys::default().quick;
    let row = ui.key_row(
        "key:quick",
        "Open the quick translation window",
        Some("A small window in every app: type or paste a word, a sentence or a few lines, choose the language and read the translation. Nothing has to be selected, and the result can be copied."),
        &current.label(),
        current == default,
    );
    if let Some((vk, mods)) = row.captured {
        let b = Binding::new(vk, mods);
        if !Binding::usable_hotkey(vk, mods) {
            QUICK_PROBLEM.with(|p| p.set(Some("A shortcut that works in every app needs Ctrl or Alt together with another key.")));
        } else if b == s.keys.translate || b == s.keys.fixkeys || b == s.keys.undo || b == s.keys.dictate {
            QUICK_PROBLEM.with(|p| p.set(Some("That key already has another job: choose another one.")));
        } else {
            s.keys.quick = b;
            QUICK_PROBLEM.with(|p| p.set(None));
        }
    }
    if row.reset {
        s.keys.quick = default;
        QUICK_PROBLEM.with(|p| p.set(None));
    }
    ui.card_end();
    if let Some(problem) = QUICK_PROBLEM.with(|p| p.get()) {
        ui.card_begin();
        ui.note('\u{E783}', problem);
        ui.card_end();
    }

    ui.section("Typed with the wrong keyboard");
    ui.card_begin();
    let current = s.keys.fixkeys;
    let default = Keys::default().fixkeys;
    let row = ui.key_row(
        "key:fixkeys",
        "Put the selected text right",
        Some("Typed a paragraph with the English keyboard when you meant Arabic (or French)? Select it, press the shortcut, and Verba finds which keyboard it came from. It also turns Arabic written with Latin letters and digits (wach rak, 3lach, sa7a) into Arabic letters."),
        &current.label(),
        current == default,
    );
    if let Some((vk, mods)) = row.captured {
        if !Binding::usable_hotkey(vk, mods) {
            FIXKEYS_PROBLEM.with(|p| p.set(Some("A shortcut that works in every app needs Ctrl or Alt together with another key.")));
        } else if Binding::new(vk, mods) == s.keys.translate || Binding::new(vk, mods) == s.keys.undo {
            FIXKEYS_PROBLEM.with(|p| p.set(Some("That key already has another job: choose another one.")));
        } else {
            s.keys.fixkeys = Binding::new(vk, mods);
            FIXKEYS_PROBLEM.with(|p| p.set(None));
        }
    }
    if row.reset {
        s.keys.fixkeys = default;
        FIXKEYS_PROBLEM.with(|p| p.set(None));
    }
    ui.note('\u{E946}', "It works with the keyboards installed in Windows. Choose the reading with the arrow keys, put it in with Tab, close with Esc.");
    ui.card_end();
    if let Some(problem) = FIXKEYS_PROBLEM.with(|p| p.get()) {
        ui.card_begin();
        ui.note('\u{E783}', problem);
        ui.card_end();
    }

    ui.section("Line breaks");
    ui.card_begin();
    let mut shifted = usize::from(s.line_break_shift);
    if ui.segmented_row(
        "line-break",
        "A line break in a text Verba types",
        Some("Enter is a new line in most apps. In chat apps (WhatsApp Web, Slack, Teams, Discord) Enter sends the message and Shift+Enter is the new line: choose Shift+Enter there."),
        &["Enter", "Shift+Enter"],
        &mut shifted,
    ) {
        s.line_break_shift = shifted == 1;
    }
    ui.card_end();

    ui.section("Sentences");
    ui.card_begin();
    let pairs = crate::sentence_pairs();
    if pairs.is_empty() {
        ui.info_row("Sentence translation is not installed.", Some("Run the Verba setup again and tick \"Sentence translation\". It needs about 330 MB."));
    } else {
        let names: Vec<String> = pairs.iter().map(|(f, t)| format!("{} \u{2192} {}", f.code().to_uppercase(), t.code().to_uppercase())).collect();
        ui.info_row(&format!("Installed: {}", names.join(", ")), Some("Select a sentence or a short text (up to 1200 characters), then press the shortcut. Modern Standard Arabic works well; Algerian Darija does not."));
    }
    ui.card_end();

    ui.section("Dictionary");
    ui.card_begin();
    if ui.button_row(
        "lookup",
        "Look up a word",
        Some("Type a word in English, French, Arabic or Darija (Latin letters) and see its translations into the other languages, more of them than the popup shows. Yours come first."),
        "Look up…",
        ButtonStyle::Primary,
        true,
    ) {
        ui.defer(look_up);
    }
    match crate::dictionary_words() {
        Some(Some(n)) => ui.info_row(&format!("English, French and Arabic: {} entries", group_thousands(n)), Some("Built from Wiktionary (CC BY-SA 4.0). It never goes online.")),
        Some(None) => ui.info_row(
            "English, French and Arabic: installed.",
            Some("It is read the first time you translate a word (about a second), and the number of entries is shown then. Built from Wiktionary (CC BY-SA 4.0). It never goes online."),
        ),
        None => ui.info_row("The dictionary is not installed.", Some("The Setup installs it next to the program, in data\\words\\dict.txt.")),
    }
    ui.card_end();
}

/// 12345 -> "12,345".
fn group_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The Dictation page: one pack per language to download, and how to dictate.
fn dictation_page(ui: &mut Ui, s: &mut Settings) {
    use crate::dictation::{self, Phase};
    ui.page_begin("Dictation", "Speak and Verba writes what you say where the cursor is. It works on this PC, offline: the sound is never sent anywhere, and is never kept.");
    let progress = dictation::progress();
    let busy = progress.as_ref().is_some_and(|p| !matches!(p.phase, Phase::Failed(_)));
    for pack in dictation::PACKS {
        ui.section(&format!("{} dictation", pack.title));
        ui.card_begin();
        let size = dictation::megabytes(pack.total_bytes());
        let mine = progress.as_ref().filter(|p| p.pack == pack.id);
        if dictation::installed(pack) {
            ui.info_row(&format!("{} dictation is installed.", pack.title), Some(&format!("It uses {size} on the disk. Press the shortcut below and speak.")));
            if ui.button_row(&format!("dict-remove-{}", pack.id), "Remove the pack", Some("Frees the disk. You can download it again."), "Remove", ButtonStyle::Secondary, !busy) {
                let id = pack.id;
                ui.defer(move || {
                    crate::dictation_remove(id);
                    refresh();
                });
            }
        } else if let Some(p) = mine {
            match &p.phase {
                Phase::Downloading | Phase::Verifying => {
                    let done = dictation::megabytes(p.done);
                    let text = if p.phase == Phase::Verifying { "Checking the files\u{2026}".to_string() } else { format!("{:.0} %  \u{00B7}  {done} of {size}", p.fraction() * 100.0) };
                    if ui.progress_row(&format!("dict-progress-{}", pack.id), &format!("Downloading the {} pack\u{2026}", pack.title), Some(&text), p.fraction(), "Cancel", ButtonStyle::Secondary) {
                        ui.defer(dictation::cancel_download);
                    }
                }
                Phase::Failed(why) => {
                    ui.info_row("The download did not finish.", Some(why));
                    let id = pack.id;
                    if ui.button_row(&format!("dict-retry-{}", pack.id), "Try again", Some("What was downloaded is kept: it goes on from there."), "Try again", ButtonStyle::Primary, true) {
                        ui.defer(move || download_pack(id));
                    }
                }
            }
        } else {
            let desc = match pack.id {
                "ar" => "Writes Modern Standard Arabic well enough to correct (about one word in four needs a fix on clear speech); Algerian Darija is not understood. Needs a microphone.",
                "dz" => "Experimental. Understands Algerian spoken Arabic much better than the Arabic pack, but on real conversations about three words in four still need a fix (Darija has no standard spelling). Needs a microphone.",
                _ => "Writes English well (about one word in seventeen needs a fix on clear speech). Needs a microphone.",
            };
            if ui.button_row(&format!("dict-get-{}", pack.id), &format!("Download the {} pack", pack.title), Some(&format!("{desc} {size} to download, once.")), &format!("Download ({size})"), ButtonStyle::Primary, !busy) {
                let id = pack.id;
                ui.defer(move || download_pack(id));
            }
        }
        ui.card_end();
    }

    ui.section("How to dictate");
    ui.card_begin();
    let current = s.keys.dictate;
    let default = Keys::default().dictate;
    let row = ui.key_row(
        "key:dictate",
        "Start and end a dictation",
        Some("Click in a text field, press it, speak, then press it again (or stop talking for two seconds). The words are typed at the cursor after a few seconds."),
        &current.label(),
        current == default,
    );
    if let Some((vk, mods)) = row.captured {
        let b = Binding::new(vk, mods);
        if !Binding::usable_hotkey(vk, mods) {
            DICTATE_PROBLEM.with(|p| p.set(Some("A shortcut that works in every app needs Ctrl or Alt together with another key.")));
        } else if b == s.keys.translate || b == s.keys.fixkeys || b == s.keys.undo {
            DICTATE_PROBLEM.with(|p| p.set(Some("That key already has another job: choose another one.")));
        } else {
            s.keys.dictate = b;
            DICTATE_PROBLEM.with(|p| p.set(None));
        }
    }
    if row.reset {
        s.keys.dictate = default;
        DICTATE_PROBLEM.with(|p| p.set(None));
    }
    let mut which = match s.dictation_language {
        crate::settings::DictationLanguage::Auto => 0,
        crate::settings::DictationLanguage::Arabic => 1,
        crate::settings::DictationLanguage::Darija => 2,
        crate::settings::DictationLanguage::English => 3,
    };
    if ui.segmented_row(
        "dict-language",
        "Language to write",
        Some("\u{201C}Keyboard\u{201D} writes in the language of the keyboard you are typing with (English, or Arabic: with the Darija pack when Algerian Darija is on in Languages, else the Arabic pack); the others always use their pack."),
        &["Keyboard", "Arabic", "Darija", "English"],
        &mut which,
    ) {
        s.dictation_language = match which {
            1 => crate::settings::DictationLanguage::Arabic,
            2 => crate::settings::DictationLanguage::Darija,
            3 => crate::settings::DictationLanguage::English,
            _ => crate::settings::DictationLanguage::Auto,
        };
    }
    ui.card_end();
    if let Some(problem) = DICTATE_PROBLEM.with(|p| p.get()) {
        ui.card_begin();
        ui.note('\u{E783}', problem);
        ui.card_end();
    }
    ui.card_begin();
    ui.note('\u{E946}', "Measured on people reading aloud in a quiet room (docs/ACCURACY.md); a noisy room, a cheap microphone or fast speech make more mistakes, and Algerian Darija is not measured.");
    ui.card_end();
}

/// Starts the download of a pack; a reason it could not start is shown in a message.
fn download_pack(id: &'static str) {
    if let Err(why) = crate::dictation_download(id) {
        say(&why, windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING);
    }
    refresh();
}

fn keyboard(ui: &mut Ui, s: &mut Settings) {
    ui.page_begin("Keyboard", "The keys that drive the suggestions, so you never need the mouse.");
    ui.section("While suggestions are shown");
    ui.card_begin();
    let rows = [
        ("key:accept", Action::Accept, "Accept the highlighted suggestion", "It replaces the word you are typing."),
        ("key:prev", Action::Prev, "Move the highlight to the left", "Works when two or more suggestions are shown. In an Arabic list, which starts on the right, it follows the key too."),
        ("key:next", Action::Next, "Move the highlight to the right", "Works when two or more suggestions are shown."),
        ("key:close", Action::Close, "Close the suggestions", "Until you type again, so the arrow keys move the cursor as usual."),
    ];
    let defaults = Keys::default();
    for (name, action, title, desc) in rows {
        let current = s.keys.get(action);
        let row = ui.key_row(name, title, Some(desc), &current.label(), current == defaults.get(action));
        if let Some((vk, mods)) = row.captured {
            if Binding::usable(vk, mods) {
                // when another action had that key, the two swap
                s.keys.assign(action, Binding::new(vk, mods));
                KEY_PROBLEM.with(|p| p.set(None));
            } else {
                KEY_PROBLEM.with(|p| p.set(Some("That key cannot be used: letters, digits and Space on their own are what you type. Add Ctrl or Alt, or pick another key.")));
            }
        }
        if row.reset {
            s.keys.assign(action, defaults.get(action));
            KEY_PROBLEM.with(|p| p.set(None));
        }
    }
    ui.card_end();

    if let Some(problem) = KEY_PROBLEM.with(|p| p.get()) {
        ui.card_begin();
        ui.note('\u{E783}', problem);
        ui.card_end();
    }

    ui.card_begin();
    if ui.button_row("keys-reset", "Original keys", Some("Tab accepts, the left and right arrows move, Esc closes."), "Reset all", ButtonStyle::Secondary, s.keys != defaults) {
        s.keys = defaults;
        KEY_PROBLEM.with(|p| p.set(None));
    }
    ui.note('\u{E946}', "Click a key, then press the new one. Press Esc to cancel. If you give an action a key another one uses, the two swap.");
    ui.card_end();
}

fn apps(ui: &mut Ui, s: &mut Settings) {
    use crate::app_list;
    ui.page_begin("Apps", "Verba works in every program unless you switch it off here. Every program of this PC that Verba can find is listed, with a switch.");
    app_list::ensure_loaded(crate::refresh_settings_soon);
    let filter = APP_FILTER.with(|f| f.borrow().clone());

    // finding a program, and adding one that is not in the list
    ui.card_begin();
    if filter.is_empty() {
        if ui.button_row("app-find", "Find a program", Some("Type a part of its name."), "Find\u{2026}", ButtonStyle::Secondary, true) {
            ui.defer(find_program);
        }
    } else if ui.button_row("app-clear", &format!("Showing the programs with \u{201C}{filter}\u{201D}"), Some("Only these are listed below."), "Show all", ButtonStyle::Secondary, true) {
        ui.defer(|| APP_FILTER.with(|f| f.borrow_mut().clear()));
    }
    if ui.button_row("app-add", "A program that is not in the list", Some("Type the name of its file, for example notepad.exe: Verba then asks for its switch here."), "Add by name\u{2026}", ButtonStyle::Secondary, true) {
        ui.defer(add_program_by_name);
    }
    ui.card_end();

    let only = s.app_mode == AppMode::OnlyThese;
    let rows = app_list::rows(s, &filter);
    let mut flipped: Option<(String, bool)> = None;
    let show = |ui: &mut Ui, row: &app_list::Row, s: &Settings, flipped: &mut Option<(String, bool)>| {
        let locked = app_list::always_off(&row.exe);
        let mut on = s.allows_app(&row.exe);
        let mut note = row.exe.clone();
        if row.running {
            note.push_str("   \u{00B7}   open now");
        }
        if locked {
            note.push_str("   \u{00B7}   always off: it protects your passwords");
        }
        if ui.app_row(&format!("app:{}", row.exe), &row.name, &note, &mut on, !locked) {
            *flipped = Some((row.exe.clone(), on));
        }
    };

    if app_list::loading() && rows.is_empty() {
        ui.card_begin();
        ui.info_row("Looking for the programs of this PC\u{2026}", Some("It takes a second or two."));
        ui.card_end();
    }
    // the programs the user changed come first: the ones that are off (or, in "only these", the ones that are on)
    let changed: Vec<&app_list::Row> = rows.iter().filter(|r| s.apps.contains(&r.exe)).collect();
    if !changed.is_empty() {
        ui.section(if only { "Switched on by you" } else { "Switched off by you" });
        ui.card_begin();
        for row in &changed {
            show(ui, row, s, &mut flipped);
        }
        ui.card_end();
    }
    let open: Vec<&app_list::Row> = rows.iter().filter(|r| r.running && !s.apps.contains(&r.exe)).collect();
    if filter.is_empty() && !open.is_empty() {
        ui.section("Open now");
        ui.card_begin();
        for row in &open {
            show(ui, row, s, &mut flipped);
        }
        ui.card_end();
    }
    let rest: Vec<&app_list::Row> = rows.iter().filter(|r| !s.apps.contains(&r.exe) && (!filter.is_empty() || !r.running)).collect();
    if !rest.is_empty() {
        ui.section(if filter.is_empty() { "All programs" } else { "Results" });
        ui.card_begin();
        for row in &rest {
            show(ui, row, s, &mut flipped);
        }
        ui.card_end();
    } else if !filter.is_empty() && changed.is_empty() {
        ui.card_begin();
        ui.info_row("No program has this in its name.", Some("A program that is not in the list can be added by the name of its file."));
        ui.card_end();
    }
    if let Some((exe, on)) = flipped {
        s.set_app(&exe, on);
    }

    ui.section("How the list works");
    ui.card_begin();
    let mut mode = if only { 1 } else { 0 };
    if ui.segmented_row(
        "mode",
        "Verba works",
        Some("\u{201C}Everywhere except\u{201D}: on by default, you switch programs off. \u{201C}Only in\u{201D}: off by default, you switch programs on. Changing it starts a new list."),
        &["Everywhere except the ones I switch off", "Only in the ones I switch on"],
        &mut mode,
    ) {
        // switching the meaning of the list starts a new list: the old one meant the opposite
        s.app_mode = if mode == 1 { AppMode::OnlyThese } else { AppMode::AllExcept };
        s.apps.clear();
    }
    ui.note('\u{E72E}', "Password managers and Verba itself are always off, whatever you choose. Programs of the Microsoft Store appear here when they are open.");
    ui.card_end();
}

thread_local! {
    /// What the person typed to find a program in the Apps page (empty: all of them).
    static APP_FILTER: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// "Find a program": asks for a part of its name.
fn find_program() {
    let current = APP_FILTER.with(|f| f.borrow().clone());
    let form = Form {
        title: "Find a program",
        intro: "Type a part of the name of the program (or of its file).",
        choice: None,
        fields: &[Field { label: "Name", rtl: None, lines: 1 }],
        initial: &[current.as_str()],
        ok_label: "Find",
        result_lines: 0,
        cancel_label: None,
        activate: true,
    };
    dialog::ask(owner(), theme(), &form, &mut |_, texts| {
        APP_FILTER.with(|f| *f.borrow_mut() = texts[0].trim().to_string());
        Ok(())
    });
}

/// "A program that is not in the list": asks for the name of its file and adds it with Verba switched off (or on, in "only these").
fn add_program_by_name() {
    let form = Form {
        title: "Add a program",
        intro: "The name of the program's file, for example notepad.exe. It is added with Verba switched off in it (switched on, when the list is \u{201C}only in\u{201D}).",
        choice: None,
        fields: &[Field { label: "File name", rtl: None, lines: 1 }],
        initial: &[],
        ok_label: "Add",
        result_lines: 0,
        cancel_label: None,
        activate: true,
    };
    dialog::ask(owner(), theme(), &form, &mut |_, texts| match crate::app_list::typed_exe(&texts[0]) {
        Some(exe) => {
            crate::update_settings(|s| s.set_app(&exe, s.app_mode == AppMode::OnlyThese));
            Ok(())
        }
        None => Err("This is not the name of a program file.".to_string()),
    });
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

fn general(ui: &mut Ui, s: &mut Settings) {
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

    ui.section("Sync between PCs");
    ui.card_begin();
    let folder = s.sync_folder.trim().to_string();
    ui.info_row(
        &if folder.is_empty() { "Not syncing.".to_string() } else { format!("Syncing with {folder}") },
        Some("My words and my translations are kept in one file in a folder that OneDrive, Dropbox or Google Drive already copies to your other PCs. Choose the same kind of folder on each PC. Verba talks to no server."),
    );
    if ui.button_row("sync-choose", "Choose the folder", Some("A folder of your cloud drive, for example OneDrive\\Verba."), "Choose…", ButtonStyle::Primary, true) {
        ui.defer(choose_sync_folder);
    }
    if ui.button_row("sync-now", "Sync now", Some("Verba also looks every 20 seconds, and syncs when something changed."), "Sync now", ButtonStyle::Secondary, !folder.is_empty()) {
        ui.defer(sync_and_say);
    }
    if ui.button_row("sync-stop", "Stop syncing", Some("The words and translations stay on this PC; the file in the folder is not deleted."), "Stop", ButtonStyle::Secondary, !folder.is_empty()) {
        ui.defer(|| crate::update_settings(|s| s.sync_folder.clear()));
    }
    ui.card_end();

    ui.section("Updates");
    ui.card_begin();
    ui.toggle_row(
        "updates",
        "Check for a new version once a day",
        Some("Verba asks the project's releases page on GitHub which version is the latest, and says so when there is one. It sends nothing else and downloads nothing. It is the only thing Verba does on the network, so it is off until you turn it on."),
        &mut s.check_updates,
        true,
    );
    if ui.button_row(
        "update-now",
        "Check now",
        Some("Asks once, and tells you the answer."),
        "Check now",
        ButtonStyle::Secondary,
        true,
    ) {
        ui.defer(|| crate::check_for_update(true));
    }
    ui.card_end();

    ui.section("If something does not work");
    ui.card_begin();
    let refused = crate::refused_shortcuts();
    if refused.is_empty() {
        ui.info_row("Every Verba shortcut is accepted by Windows.", Some("If a shortcut does nothing in one app, that app may use the same keys, or Verba is switched off there (Apps page)."));
    } else {
        ui.info_row(
            &format!("Windows refused: {}.", refused.join(", ")),
            Some("Another program uses these keys. Choose other ones on the Translate and Writing help pages."),
        );
    }
    if ui.button_row(
        "report",
        "Show a report",
        Some("What Verba found on this PC: keyboards, word lists, dictionary, shortcuts. It has no text you wrote, so you can send it to get help."),
        "Show report",
        ButtonStyle::Secondary,
        true,
    ) {
        ui.defer(crate::show_report);
    }
    ui.card_end();

    ui.section("Backup");
    ui.card_begin();
    if ui.button_row(
        "export",
        "Save a backup",
        Some("My words, my translations and the settings in one file you can keep, or open on another PC. What Verba learned from your writing is not in it."),
        "Save…",
        ButtonStyle::Secondary,
        true,
    ) {
        ui.defer(export_backup);
    }
    if ui.button_row(
        "import",
        "Open a backup",
        Some("Adds the words and translations of a backup file to yours and uses its settings."),
        "Open…",
        ButtonStyle::Secondary,
        true,
    ) {
        ui.defer(import_backup);
    }
    ui.card_end();

    ui.section("About");
    ui.card_begin();
    ui.about_block("Verba", concat!("Version ", env!("CARGO_PKG_VERSION")), "");
    ui.info_row(
        "The idea is BillelBR\u{2019}s, and Verba was built by BillelBR together with Claude.",
        Some("BillelBR owns the idea and every decision about what Verba does. Claude, the AI assistant made by Anthropic, wrote the code with them, step by step."),
    );
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

/// The theme of the window, for the forms it opens.
fn theme() -> Theme {
    look().theme
}

/// A translation of the list: the direction as a tag, the word, and its translations under it.
fn translation_entry(e: &verba_local::UserEntry) -> ListEntry {
    ListEntry {
        tag: format!("{} \u{2192} {}", e.from.code().to_uppercase(), e.to.code().to_uppercase()),
        title: e.word.clone(),
        detail: e.translations.join("  \u{B7}  "),
    }
}

/// The form that asks for a word to keep.
fn add_word() {
    let form = Form {
        title: "Add a word",
        intro: "One word, 2 to 40 letters. Verba will not flag it as a typo and will offer it when you type its first letters.",
        choice: None,
        fields: &[Field { label: "Word", rtl: None, lines: 1 }],
        initial: &[],
        ok_label: "Add",
        result_lines: 0,
        cancel_label: None,
        activate: true,
    };
    dialog::ask(owner(), theme(), &form, &mut |_, texts| {
        if crate::add_my_word(&texts[0]) {
            Ok(())
        } else {
            Err("This is not a single word of 2 to 40 letters, or you already have it.".to_string())
        }
    });
}

/// The directions of the form that asks for a translation: what is written, and from which language to which.
const DIRECTIONS: [(&str, Lang, Lang); 6] = [
    ("Arabic \u{2192} English", Lang::Ar, Lang::En),
    ("English \u{2192} Arabic", Lang::En, Lang::Ar),
    ("French \u{2192} English", Lang::Fr, Lang::En),
    ("English \u{2192} French", Lang::En, Lang::Fr),
    ("Arabic \u{2192} French", Lang::Ar, Lang::Fr),
    ("French \u{2192} Arabic", Lang::Fr, Lang::Ar),
];

/// The form that asks for a word and what it should be translated to.
fn add_translation() {
    let names: Vec<&str> = DIRECTIONS.iter().map(|d| d.0).collect();
    let form = Form {
        title: "Write a translation",
        intro: "When you translate this word, your translations come first. Write several separated by ; (at most 5).",
        choice: Some(("Translate", &names)),
        fields: &[
            Field { label: "Word", rtl: Some(|i| DIRECTIONS[i].1 == Lang::Ar), lines: 1 },
            Field { label: "Translation", rtl: Some(|i| DIRECTIONS[i].2 == Lang::Ar), lines: 1 },
        ],
        initial: &[],
        ok_label: "Add",
        result_lines: 0,
        cancel_label: None,
        activate: true,
    };
    dialog::ask(owner(), theme(), &form, &mut |choice, texts| {
        let (_, from, to) = DIRECTIONS[choice.min(DIRECTIONS.len() - 1)];
        crate::add_my_translation(from, to, &texts[0], &texts[1])
    });
}

/// The dictionary window: a word is typed, its translations into the other languages are shown under it, and the box stays for the next word.
fn look_up() {
    let form = Form {
        title: "Look up a word",
        intro: "A word in English, French, Arabic, or Algerian Darija in Latin letters.",
        choice: None,
        fields: &[Field { label: "Word", rtl: None, lines: 1 }],
        initial: &[],
        ok_label: "Look up",
        result_lines: 6,
        cancel_label: None,
        activate: true,
    };
    dialog::ask_with(owner(), theme(), &form, &mut |_, texts| {
        let word = texts[0].trim();
        if word.is_empty() {
            return dialog::Reply::Error("Type a word.".to_string());
        }
        match crate::look_up_word(word) {
            Some(text) => dialog::Reply::Show(text),
            None => dialog::Reply::Error(format!("The dictionary has no translation for \u{201C}{word}\u{201D}.")),
        }
    });
}

/// The Windows dialog that asks for a folder.
fn pick_folder() -> Option<std::path::PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW, SHGetPathFromIDListW};
    let title = wide("Choose the folder of your cloud drive where Verba keeps its sync file");
    unsafe {
        let info = BROWSEINFOW { hwndOwner: owner().unwrap_or_default(), lpszTitle: PCWSTR(title.as_ptr()), ulFlags: BIF_RETURNONLYFSDIRS, ..Default::default() };
        let item = SHBrowseForFolderW(&info);
        if item.is_null() {
            return None;
        }
        let mut buf = [0u16; 260];
        let ok = SHGetPathFromIDListW(item, &mut buf).as_bool();
        CoTaskMemFree(Some(item as *const _));
        if !ok {
            return None;
        }
        let len = buf.iter().position(|u| *u == 0).unwrap_or(buf.len());
        Some(std::path::PathBuf::from(String::from_utf16_lossy(&buf[..len])))
    }
}

/// "Choose the folder": asks for it, then syncs and says what happened.
fn choose_sync_folder() {
    let Some(path) = pick_folder() else { return };
    let text = path.display().to_string();
    crate::update_settings(|s| s.sync_folder = text);
    sync_and_say();
}

/// Syncs and tells the person what came in and what went out.
fn sync_and_say() {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_ICONWARNING};
    match crate::sync_now() {
        Ok(r) => say(
            &format!(
                "Synced. From the folder: {} new words, {} words removed, {} translations new or changed.{}",
                r.words_added,
                r.words_removed,
                r.translations_changed,
                if r.file_written { " The file in the folder was updated with what this PC has." } else { "" }
            ),
            MB_ICONINFORMATION,
        ),
        Err(e) => say(&e, MB_ICONWARNING),
    }
}

/// What a check for a newer version found, said to the person who asked: with a button to open the page of the new version.
pub fn show_update_result(result: Result<Option<String>, String>) {
    let current = env!("CARGO_PKG_VERSION");
    match result {
        Ok(Some(tag)) => {
            let text = format!("Verba {tag} is available. You have {current}.\n\nThe page of the new version has the installer.");
            if dialog::message(owner(), theme(), "A new version is available", &text, "Open the page", Some("Later")) {
                let page = wide(&crate::update::release_page(&tag));
                unsafe {
                    ShellExecuteW(None, w!("open"), PCWSTR(page.as_ptr()), None, None, SW_SHOW);
                }
            }
        }
        Ok(None) => {
            dialog::message(owner(), theme(), "You are up to date", &format!("Verba {current} is the latest version."), "OK", None);
        }
        Err(e) => {
            dialog::message(owner(), theme(), "Could not check for updates", &e, "OK", None);
        }
    }
}

/// A message box with a title and an icon, owned by the settings window.
fn say(text: &str, icon: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) {
    let warning = icon == windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING;
    dialog::message(owner(), theme(), if warning { "That did not work" } else { "Done" }, text, "OK", None);
}

/// "Save a backup": asks for a file and writes it.
fn export_backup() {
    let name = "Verba backup.verba-backup";
    let Some(path) = dialog::choose_file(owner(), true, "Save a Verba backup", name, "Verba backup", crate::backup::EXTENSION) else { return };
    match crate::export_backup(&path) {
        Ok((words, translations)) => say(&format!("Saved {words} words and {translations} translations, with the settings, in:\n{}", path.display()), windows::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION),
        Err(e) => say(&e, windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING),
    }
}

/// "Open a backup": asks for a file and adds what it holds.
fn import_backup() {
    let Some(path) = dialog::choose_file(owner(), false, "Open a Verba backup", "", "Verba backup", crate::backup::EXTENSION) else { return };
    match crate::import_backup(&path) {
        Ok(done) => say(
            &format!(
                "Added {} new words and {} translations.{}",
                done.words,
                done.translations,
                if done.settings { " The settings of the backup are in use." } else { "" }
            ),
            windows::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION,
        ),
        Err(e) => say(&e, windows::Win32::UI::WindowsAndMessaging::MB_ICONWARNING),
    }
}

fn confirm_forget() {
    let text = "Verba will forget every word and word pair it learned from what you typed. The words you added yourself stay.";
    if dialog::message(owner(), theme(), "Forget what Verba learned?", text, "Forget", Some("Cancel")) {
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

