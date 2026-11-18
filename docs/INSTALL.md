# Installing Verba

## For users

**You need:** Windows 10 (version 1809 or later) or Windows 11, 64-bit. Nothing else: no admin rights, no Visual C++ packages, no internet after the download.

1. Go to the [Releases page](https://github.com/BillelBR/verba/releases) and download `Verba-Setup-<version>.exe` from the latest release.
2. Run it and follow the wizard (English or French). It installs for **you only** by default. A first dialog lets you choose "all users" instead (that one asks for administrator rights).
3. Leave "Start Verba when Windows starts" ticked if you want it always on. Finish with "Launch Verba".

Verba then sits in the **system tray** (the `^` arrow by the clock). Open its menu with a click on the icon.

### Using it without the mouse

While the suggestion popup is open: **← and →** move the highlight (when there are two or more suggestions), **Tab** puts the highlighted one in, **Esc** closes the popup (the arrow keys then move the cursor as usual until you type again). In an Arabic list, which starts on the right, the highlight still moves the way the arrow points.
Every one of these keys can be changed in Settings > Keyboard: click a key, then press the new one. A letter, a digit or Space on its own is refused (you would no longer be able to type); add Ctrl or Alt, or pick another key.

### Translating a word

Select a word in any app and press **Ctrl+Alt+T** (changeable in Settings > Translate). With nothing selected, the word just before the cursor is used. The translations appear above the word: the arrows choose, **Tab** replaces the word with the chosen one, **Esc** closes. Choose the language to translate into in Settings > Translate; a word that is already in that language goes to the next one. French and Arabic are linked through English when there is no direct entry, which is less exact. Words with attached particles in Arabic (`والكتاب`, `سيارتك`) are found too. The dictionary is Modern Standard Arabic with a few Maghrebi words; it knows no Darija spellings. Everything is offline, and the selected word is read only when you press the shortcut.

### Game and chat abbreviations

Select `gg`, `GN`, `brb`, `afk`, `see u`, `ez`, `wp`, `noob` or a similar abbreviation (about 170 spellings, in Latin and in Arabic letters, with some PUBG words such as `wwcd`, `hot drop`, `blue zone`) and press the shortcut: the popup shows what it means. With "Translate into" set to English it gives the abbreviation written out (the label says **Slang**: `gg` gives "good game"); with French or Arabic it gives the meaning in that language. The same list is used inside a sentence translated from English: `gn guys, see u tomorrow` is written out ("Good night guys, see you tomorrow") before it goes to the translation network, which has never seen these abbreviations and drops or garbles them. The list (`crates/verba-local/seed/chat-slang.txt`) was written from general knowledge and **has not been read by an Algerian or Arab player**: the French and Arabic lines are the ones to check. It does not cover every game word with an ordinary meaning ("carry" stays "carry": a sentence translation of "thanks for the carry" still gives الحمل in Arabic ("the load", or "the pregnancy")).

### Spoken Arabic in a sentence

The network that translates sentences was trained on written Modern Standard Arabic. A sentence in Algerian Darija, or with Levantine, Gulf or Egyptian words, used to come out as names and noise ("دروك" became "Drake") and the Levantine "زلمة" (man) as a slur. Before an Arabic sentence is translated, Verba replaces the words of a list (`crates/verba-local/seed/ar-colloquial.txt`, about 240 spellings) by their standard form: `بغيت نروح للدار دروك` is sent as `أردت أذهب للدار الآن` and comes out as "I wanted to go to the house now." It also writes chat abbreviations out in Arabic (`gg يا شباب` is sent as `لعبة جيدة يا شباب`). Words that are not in the list stay as they are, so many Darija sentences are still translated badly; the list is not reviewed by a native speaker, and you can add a line to it.

### Translating a sentence

Select a sentence (or a few lines, up to 1200 characters) in any app and press the same shortcut, **Ctrl+Alt+T**. Verba says "Translating..." and shows the translation above the selection a moment later (about a second the first time, while the model loads; a fraction of a second after that). **Tab** types it over the selection, **Esc** closes. Spaces and line breaks stay where they were. Arabic goes to English, and text in Latin letters goes to Arabic. It needs the "Sentence translation" option of the setup (about 170 MB on disk) and works offline.

It is Modern Standard Arabic, the Arabic of news and writing. **Algerian Darija is not understood**: the model writes words that do not exist. Check a translation before you send it; a translation program makes mistakes, especially with idioms and names.

### "Windows protected your PC" (SmartScreen)

The installer is **not code-signed yet** (a certificate costs money and the project is new), so Windows may warn that the publisher is unknown.
Click **More info**, then **Run anyway**.

To be sure the file is the one the project published, compare its SHA-256 with the `.sha256` file next to it on the Releases page:

```powershell
Get-FileHash .\Verba-Setup-0.8.0.exe -Algorithm SHA256
```

The two hashes must be identical. The source code is in this repository, and the Releases are built by GitHub Actions from it.

Some antivirus programs are also wary of any program that watches the keyboard (Verba needs this to read Tab and Esc while its popup is shown). If yours blocks Verba, you can check the code yourself and add an exception for the Verba folder. Never turn SmartScreen or your antivirus off for this.

Why the warning exists and what can remove it: [SIGNING.md](SIGNING.md).

### What the installer does

| What | Where |
|---|---|
| The program, `verba.exe` | `%LOCALAPPDATA%\Programs\Verba` (all users: `C:\Program Files\Verba`) |
| The word lists (English, French, Arabic) | `data\words` inside that folder. CC-BY-SA-4.0, see `WORD-LISTS-LICENSE.txt` |
| Sentence translation (if ticked): `verba-translate.exe`, `vcomp140.dll` and two models | the same folder and `models\ar-en`, `models\en-ar`. Apache-2.0 and MIT, see `THIRD-PARTY.md` |
| A Start Menu shortcut (and a desktop one if you tick it) | Start Menu |
| "Start with Windows" (if ticked) | one value `Verba` in your own `HKCU\...\CurrentVersion\Run` |
| Your choices (theme, languages, programs) | `%APPDATA%\Verba\settings.txt`, written by the program, not the installer |

### Start, quit, uninstall

- **Start:** Start Menu > Verba. A short animation of the logo plays while the word lists load. When Windows starts Verba at sign-in (`--startup`), there is no animation: it just appears in the tray.
- **Quit:** the tray icon's menu > *Quit Verba*, or press **Ctrl+Alt+Shift+Q**.
- **Pause:** **Ctrl+Alt+Shift+V**, or the tray menu.
- **Not working in one program?** Settings > General > *Run check* (or the tray menu > *Check a text field (3 s)…*), then click in that field. A report says which step of reading the field worked (program and class names only, never the text). Windows' own Start menu and search box may never show the popup: Windows draws them above every normal window, and only a signed program with special rights can go over them.
- **Update:** run the newer Setup. It closes the running Verba first and keeps your settings.
- **Uninstall:** Settings > Apps > *Verba* > Uninstall. It asks whether to delete your settings too (the default is No).

### Privacy

Verba reads only the few letters before the caret in the text box you are typing in, never in password boxes, and by default it keeps nothing.
It sends nothing over the network: it has no network code.

Two options, both **off** until you turn them on in the settings, keep something on your own PC:

- **Learn the words I use:** Verba keeps three lists of counts, so that it can offer your own words, rank the words you write most often higher, and suggest better next words:
  1. the words you type that are in no dictionary (a name, a term of your trade), with how often; after 3 times they become your words;
  2. **the ordinary words you type too, common ones included** (`the`, `agency`), with how often. The count fades slowly as you keep writing, so old habits fade, and it is used only to put the words you write more often higher in the suggestions;
  3. pairs of neighbouring words (two words, never more), with how often.

  It never stores a sentence, never three words in a row, no dates or times, no numbers, addresses, e-mails or paths, and nothing from the programs you excluded or from password boxes. At most 5 000 words of each kind and 20 000 pairs are kept. Someone who opens the file sees which words and pairs you write most, not what you wrote. The files are plain text in `%APPDATA%\Verba` (`learned.txt`, readable by anyone who can open your user folder). *Forget what it learned* in the settings, or deleting `learned.txt`, erases it all.
- **My words:** `personal.txt` in the same folder, one word per line. You can edit it (the settings open it in Notepad) to add words Verba must never flag as typos and should offer as suggestions.

## For maintainers

### Build the installer on your PC

You need Rust and [Inno Setup 6](https://jrsoftware.org/isdl.php). Then:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
```

It builds the release program, downloads the word lists if they are missing, runs Inno Setup, and writes `dist\Verba-Setup-<version>.exe` and `dist\Verba-Setup-<version>.exe.sha256`.
If Inno Setup is not found, set the `ISCC` environment variable to the full path of `ISCC.exe`.
The version comes from `[workspace.package]` in the root `Cargo.toml`.

The same script builds the sentence translator (`scripts\build-translate.ps1`: CTranslate2 and oneDNN compiled with CMake, about 7 minutes and 7 GB of build files in `target\mt`, needs CMake and the Visual C++ tools) and prepares the two models (`scripts\fetch-models.ps1`: downloads Helsinki-NLP/opus-mt-ar-en and -en-ar and converts them to int8 with a temporary Python 3.10 to 3.13 environment, about 2 minutes each). `-NoSentences` leaves both out for a small installer (`-PythonExe` names the Python to use). `cargo build` and `cargo test` leave `verba-translate` out on purpose (`default-members` in the root `Cargo.toml`). The test that runs the real translator is marked `--ignored`: `cargo test -p verba-app sentences -- --ignored --nocapture` (set `VERBA_TRANSLATE_EXE` and `VERBA_MODELS` if the files are not where the program looks: next to it, then up to six folders above, `target\mt\release` and `models`).

Other pieces: `scripts/make_icon.py` regenerates `crates/verba-app/assets/verba.ico`; `crates/verba-app/build.rs` puts the icon, version information and manifest into `verba.exe` (it needs `rc.exe` from the Windows SDK, and only warns if it is missing); `.cargo/config.toml` links the C runtime statically.

### Publish a release

1. Change `version` in `[workspace.package]` of the root `Cargo.toml`, commit and push.
2. Tag and push the tag. The tag must be `v` plus that exact version:

   ```
   git tag v0.8.0
   git push origin v0.8.0
   ```

3. The **Release** workflow (`.github/workflows/release.yml`) builds the installer and creates the GitHub Release with the Setup and its `.sha256`. It is marked *pre-release* when the version starts with `0.` or the tag has a hyphen (`v0.8.0-beta1`).

The **CI** workflow builds and tests every push to `main` and every pull request.
