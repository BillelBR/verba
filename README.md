# Verba

> Working name. Open-source writing assistant for every app on your computer.

Verba helps you write anywhere: it suggests the next word as you type, in a small popup above the text, and (next) fixes mistakes on the spot.
It is built to work for every major language, to run fully on your own device, and to cost nothing.

## The two features

1. **Suggestions** – while you type, Verba offers the word you are likely to write. *(working, v0)*
2. **Instant correction** – spelling and grammar fixes, applied as you type. Optional: the user turns it on. *(planned)*

Translation is **not** part of the first version. It comes later, as a separate feature.

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
| **Tab** | accept the first suggestion (replaces what you typed, adds a space) |
| **Click** | accept the suggestion you click (the popup never takes the keyboard focus) |
| **Esc** | close the popup |
| **Ctrl+Alt+Shift+V** | turn suggestions on / off |
| **Ctrl+Alt+Shift+A** | turn Verba off / on in the app you are in |
| **Ctrl+Alt+Shift+Q** | quit |

**Language follows your keyboard.** With the English keyboard you get English words, with the French keyboard French, with the Arabic keyboard Arabic.
Another keyboard layout (German, say) falls back to guessing from the letters. You can turn this off in Settings.

**Settings** (every change applies at once; saved in `%APPDATA%\Verba\settings.txt`):
suggestions on/off · follow the keyboard language · English / Français / العربية each on its own · look (follow Windows, light or dark) ·
start with Windows (installed build) · **Apps**: work everywhere except the apps you list, or only in the apps you list (pick from the running apps).
Password managers and Verba itself are always off. For an app switched off, Verba does not read anything at all.

**Look:** a rounded card with a soft shadow, drawn with Direct2D/DirectWrite. It follows Windows' light or dark mode and
accent colour, scales per monitor (high-DPI), fades in, highlights the candidate under the mouse, and lays Arabic out from the right.

English, French and Arabic (Modern Standard Arabic) work. Arabic suggestions read from the right.

## Principles

- **Private by default.** Nothing you type leaves your computer unless you choose an online engine (none exists yet).
- **Works offline.** The default engine is local, with no internet and no account.
- **No subscription.** Free and open source. If you want a cloud model later, you bring your own API key.
- **Every major language.** Engines and data are built per language, starting with English, French and Arabic.
- **Engines are plugins.** A local engine or an API engine can be swapped without touching the rest of the app.

### What Verba reads, exactly

- Only the **few letters just before the caret**, from the text field that has the focus, through Windows' accessibility interface (UI Automation).
- **Never password fields.**
- It **keeps nothing**: no log, no file, no network.
- The keyboard hook looks at **one thing**: whether the key is Tab or Esc while the popup is open. Other keys pass untouched.
- Because it uses the accessibility interface, some browsers and apps (Chromium-based ones) may switch on their accessibility mode when Verba runs, which can cost a little speed in them.

## How it is built

A Rust workspace:

| Crate | Role |
|---|---|
| `verba-core` | languages, text helpers, the `Engine` trait |
| `verba-local` | local engine: word completion from frequency lists |
| `verba-win` | Windows: finds where you type, reads the word before the caret, draws the popup |
| `verba-app` | the tray program that ties them together |
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

Next: instant correction, more languages, a proper settings window, Mac and Linux.

## License

Not chosen yet. A permissive license (MIT or Apache-2.0) is the plan.
Word lists keep their own licenses (see docs/DATA.md). Models must allow free use; non-commercial-only models (for example NLLB-200) are not used.
