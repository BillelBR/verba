# Plan

## Scope of the first version

- A small desktop app for Windows (Mac and Linux later) that sits in the tray.
- Two features, each with its own switch: **Suggestions** and **Instant correction**.
- Each feature picks an engine: **Local** (default) or **API** (user's own key).
- Languages: start with a short list of major languages (English, French, Arabic, Spanish, German, Portuguese), then grow.

## Decisions from the owner

- **Where suggestions appear:** in a small popup **above the text being typed**, the way Windows' own text suggestions do. If the app does not tell us where the caret is, the popup sits just above the top edge of the text field instead.
- **First languages:** English, French and Arabic (Modern Standard Arabic, فصحى). Right-to-left text must work in the popup and when inserting a suggestion.
- **Two features, each on its own switch:** suggestions and instant correction.
- **Engines:** local by default, or an API with the user's own key.
- **Data and licenses:** word lists and dictionaries are downloaded on demand, one pack per language, so each pack keeps its own license (open dictionaries for English, French and Arabic exist under different licenses: check each one before shipping).

## Stack decision

- **Core in Rust.** Talks to the operating system (keyboard, caret position, focused app) and runs the engines. Windows first (`windows-rs`: hooks, UI Automation), then macOS (Accessibility API) and Linux (AT-SPI / X11). One native binary, small, fast start, no runtime to install.
- **UI in Tauri 2 with TypeScript** (tray, settings, popup). Uses the WebView2 already on Windows.
- **Engines behind one Rust trait.** In-process: spell-check with Hunspell dictionaries (`spellbook` crate), n-gram frequencies, small local models through ONNX Runtime (`ort`) or llama.cpp. Out-of-process: any local server the user already runs (for example Ollama) or an API with the user's own key.
- **Why not the others:** Electron/Node cannot hook the system well and is heavy; Python is slow to start and hard to ship to ordinary users; C#/.NET is excellent on Windows but would make Mac and Linux a rewrite.
- Cost of this choice: Rust is slower to write at first. Worth it for a tool that sits in every app and must stay light.

## Open questions (decide before the first release)

1. **Name.** "Verba" is a working name. Check GitHub, crates.io, npm and the domain before keeping it.
2. **How it reaches other apps.** Two ways, in order of difficulty:
   - *Hotkey on selected text* (simple, reliable): select, press a shortcut, get the fix, press to replace.
   - *Live while typing* (the real goal): needs a keyboard hook and a caret position. Some apps do not expose the caret, and antivirus tools may flag keyboard hooks. Needs a spike to see what works in Chrome, Word, Slack, VS Code and Notepad.
3. **Local models and licenses.** Pick models whose license allows free use and redistribution.
4. **Code signing.** Without a certificate Windows SmartScreen warns on download.

## Steps (status in README)

1. **Spike (a few days):** a tiny program that reads what is typed in another app and shows a popup near the caret. Test in 5 apps. Decide the approach from the result.
2. **Engine interface:** one small contract (`suggest(text, lang)`, `correct(text, lang)`) that both local and API engines implement.
3. **Local engine v0:** dictionary spell-check and word frequency for English and French.
4. **API engine v0:** one provider, key stored on the user's machine.
5. **App shell:** tray icon, settings (two switches, engine, language), start with Windows.
6. **Open-source setup:** LICENSE, CONTRIBUTING, CI that builds Windows, Mac and Linux releases.

## Tools on this machine (checked 2026-11-16)

- Node 24, npm 11, Git, Python 3.14: installed.
- WebView2: installed.
- Rust / cargo: **not installed** (needed if we use Tauri).
