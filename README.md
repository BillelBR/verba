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

Verba lives in the **system tray** (the `^` arrow at the bottom right, by the clock). Click its icon for the menu.
Type in any app: when you have typed two letters of a word, a popup appears above it.

| Key | What it does |
|---|---|
| **Tab** | accept the first suggestion (replaces what you typed, adds a space) |
| **Esc** | close the popup |
| **Ctrl+Alt+Shift+V** | pause / resume |
| **Ctrl+Alt+Shift+Q** | quit |

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

See [docs/PLAN.md](docs/PLAN.md) for decisions and next steps, and [docs/DATA.md](docs/DATA.md) for the word lists and their licenses.

## Status

Early. Windows only. Suggestions work in apps that expose their text through UI Automation (Notepad, many modern apps);
others show nothing. Instant correction, more languages, settings and Mac/Linux come next.

## License

Not chosen yet. A permissive license (MIT or Apache-2.0) is the plan.
Word lists keep their own licenses (see docs/DATA.md). Models must allow free use; non-commercial-only models (for example NLLB-200) are not used.
