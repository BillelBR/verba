# Verba

> Working name. Writing assistant for every app on your computer. Source-available, free for noncommercial use.

Verba helps you write anywhere: it suggests the next word as you type, in a small popup above the text, and (next) fixes mistakes on the spot.
It is built to work for every major language, to run fully on your own device, and to cost nothing.

## The two features

1. **Suggestions** – while you type, Verba offers the word you are likely to write. *(working, v0)*
2. **Instant correction** – when you finish a word that looks misspelled (you type a space or punctuation), a small **Fix** popup offers the right spelling; **Tab** applies it and keeps your space or punctuation. It is **off by default**: turn it on in Settings. It never changes your text by itself.

3. **Word translation** – select a word (or put the cursor after it) and press **Ctrl+Alt+T**: its translations between English, French and Arabic appear above it; the arrows choose, **Tab** puts one in place. It works offline from a dictionary built from Wiktionary (CC BY-SA 4.0). **Sentences** work too, between Arabic and English: select a sentence or a short text (up to 1200 characters) and press the same shortcut; the translation appears above it and **Tab** types it over the selection, with the line breaks kept. Translation models (OPUS-MT, int8: a big one of 245 MB for Arabic into English, a base one of 78 MB for English into Arabic) runs on your PC, offline, in a separate program (`verba-translate.exe`) that starts at the first sentence and ends itself after ten minutes without work. Modern Standard Arabic is translated well; Algerian Darija is **not** (the model invents words), see [docs/ACCURACY.md](docs/ACCURACY.md#sentence-translation-arabic--english). Game and chat abbreviations (`gg`, `gn`, `brb`, `afk`, `see u`, `ez`, `wp`, `noob`, PUBG's `wwcd` and about 160 more) are explained in English, French and Arabic, and written out inside a sentence before it is translated (`gn guys` becomes "Good night guys"; in an Arabic sentence `gg يا شباب` becomes "Good game, guys"). Spoken Arabic (Algerian Darija and the commonest Levantine, Gulf and Egyptian words, about 240 spellings) is made standard before a sentence goes from Arabic to English, so that "نتا فين راك دروك؟" gives "Hey, where are you now?" instead of "Nata Vin Rack Drake?". Both lists are written from general knowledge and not reviewed by a native speaker or a player yet. Algerian Darija is translated only for its commonest words, in Arabic and in Latin letters (a small glossary, not checked by an Algerian yet).

## Try it (Windows)

You need [Rust](https://rustup.rs). Then, in this folder:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\fetch-data.ps1   # downloads the word lists (once)
cargo run -p verba-app
```

Verba lives in the **system tray** (the `^` arrow at the bottom right, by the clock): a blue **V**, grey when suggestions are off.

- **Click the icon:** opens the **Settings** window.
- **Right-click the icon:** *Turn Verba on/off*, *Turn off in `<the app you were in>`* (or *on*), *Settings…*, *Quit*.

Type in any app: when you have typed two letters of a word, a popup appears above it.

| Key | What it does |
|---|---|
| **Tab** | accept the first suggestion (a completion replaces what you typed and adds a space; a **Fix** replaces the misspelled word) |
| **Click** | accept the suggestion you click (the popup never takes the keyboard focus) |
| **Esc** | close the popup (a dismissed fix is not offered again for that word during this session) |
| **Ctrl+Alt+Shift+V** | turn suggestions on / off |
| **Ctrl+Alt+Shift+A** | turn Verba off / on in the app you are in |
| **Ctrl+Alt+Shift+Q** | quit |

**Language follows your keyboard.** With the English keyboard you get English words, with the French keyboard French, with the Arabic keyboard Arabic.
Another keyboard layout (German, say) falls back to guessing from the letters. You can turn this off in Settings.

**Settings** (every change applies at once; saved in `%APPDATA%\Verba\settings.txt`):
suggestions on/off · **fix spelling** on/off · follow the keyboard language · English / Français / العربية each on its own · look (follow Windows, light or dark) ·
start with Windows (installed build) · **Apps**: work everywhere except the apps you list, or only in the apps you list (pick from the running apps).
Password managers and Verba itself are always off. For an app switched off, Verba does not read anything at all.

**Look:** a rounded card with a soft shadow, drawn with Direct2D/DirectWrite. It follows Windows' light or dark mode and
accent colour, scales per monitor (high-DPI), fades in, highlights the candidate under the mouse, and lays Arabic out from the right.

English, French and Arabic (Modern Standard Arabic) work. Arabic suggestions read from the right.

Algerian Darija, written in Latin letters ("Arabizi": `wach`, `kifach`, `sa7a`, `3lach`) or in Arabic letters (واش، كيفاش، بزاف), is the engine's `Lang::Dz`:
one switch, off until it is switched on, with built-in word lists for both scripts. See [docs/ACCURACY.md](docs/ACCURACY.md).

## Principles

- **Private by default.** Nothing you type leaves your computer unless you choose an online engine (none exists yet).
- **Works offline.** The default engine is local, with no internet and no account.
- **No subscription.** Free for personal and other noncommercial use, and the source is public. Using it for work needs a commercial license ([COMMERCIAL.md](COMMERCIAL.md)). If you want a cloud model later, you bring your own API key.
- **Every major language.** Engines and data are built per language, starting with English, French and Arabic.
- **Engines are plugins.** A local engine or an API engine can be swapped without touching the rest of the app.

### What Verba reads, exactly

- Only the **few letters just before the caret**, from the text field that has the focus, through Windows' accessibility interface (UI Automation).
- **Never password fields.**
- It **keeps nothing**: no log, no file, no network.
- The keyboard hook looks at **one thing**: whether the key is one of your popup keys (Tab, the arrows and Esc by default) while the popup is open. Other keys pass untouched.
- Because it uses the accessibility interface, some browsers and apps (Chromium-based ones) may switch on their accessibility mode when Verba runs, which can cost a little speed in them.

## How it is built

A Rust workspace:

| Crate | Role |
|---|---|
| `verba-core` | languages, text helpers, the `Engine` trait |
| `verba-local` | local engine: word completion from frequency lists |
| `verba-win` | Windows: finds where you type, reads the word before the caret, draws the popup |
| `verba-app` | the tray program that ties them together |
| `verba-translate` | the sentence translator (CTranslate2 and OPUS-MT models), a program of its own; not built by a plain `cargo build` (it compiles C++ for about 7 minutes): use `scripts/build-translate.ps1` |
| `spike/` | throw-away experiments (`caret-probe` measures what each app lets us know) |

Useful tools: `cargo run -p verba-win --example read_focus` prints what Verba can read from the field you are in (numbers only, never the text);
`cargo run --release -p verba-local --example accuracy` measures suggestion accuracy (see [docs/ACCURACY.md](docs/ACCURACY.md)).

See [docs/PLAN.md](docs/PLAN.md) for decisions and next steps, and [docs/DATA.md](docs/DATA.md) for the word lists and their licenses.

## Status

Early. Windows only. Verba reads the word before the caret in two ways:

- **Modern fields** (Chromium and Electron apps such as VS Code and Slack, Office, many others) through UI Automation's text pattern.
- **Classic Windows edit boxes** (Notepad, WinForms apps, many dialogs) through the control's own messages.

Fields that give neither show nothing. Popup position is exact when the app tells us where the caret is, otherwise it sits above the field.

Checked so far: 46 automated tests (engine accuracy, key rules, word cleaning, settings), the popup's look in light/dark and Arabic, and
the tray commands. **Not yet checked by an automatic end-to-end test**: pressing Tab in a real app. Keystroke injection on a desktop in use
is unreliable (focus moves, the keyboard layout changes the letters), so that last check is manual: open Notepad, type `hel`, press Tab.

Also available (off until you turn them on in the settings): spelling fixes when you finish a word, the likely next word after a space, and a personal dictionary that can learn the words you use, on your PC only (see Privacy in [docs/INSTALL.md](docs/INSTALL.md)).

Next: Algerian Darija in Latin letters, more languages, Mac and Linux.

## License

Verba's code is under the **[PolyForm Noncommercial License 1.0.0](LICENSE)**.
You can read, run and change it for personal use, study, hobby projects, research, and for charities, schools and public institutions.
Using it in a business or to earn money needs a commercial license: see [COMMERCIAL.md](COMMERCIAL.md).
This is a *source-available* license, not an OSI "open source" one.

To contribute code, read [CONTRIBUTING.md](CONTRIBUTING.md) first (it explains the contribution terms).

The word lists Verba uses are separate data with their own license (CC-BY-SA-4.0) and are downloaded, not stored here;
see [docs/DATA.md](docs/DATA.md) and [THIRD-PARTY.md](THIRD-PARTY.md). Models must allow free use:
non-commercial-only models (for example NLLB-200) are not used.
