# Installing Verba

## For users

**You need:** Windows 10 (version 1809 or later) or Windows 11, 64-bit. Nothing else: no admin rights, no Visual C++ packages, no internet after the download.

1. Go to the [Releases page](https://github.com/BillelBR/verba/releases) and download `Verba-Setup-<version>.exe` from the latest release.
2. Run it and follow the wizard (English or French). It installs for **you only** by default. A first dialog lets you choose "all users" instead (that one asks for administrator rights).
3. Leave "Start Verba when Windows starts" ticked if you want it always on. Finish with "Launch Verba".

Verba then sits in the **system tray** (the `^` arrow by the clock). Open its menu with a click on the icon.

### "Windows protected your PC" (SmartScreen)

The installer is **not code-signed yet** (a certificate costs money and the project is new), so Windows may warn that the publisher is unknown.
Click **More info**, then **Run anyway**.

To be sure the file is the one the project published, compare its SHA-256 with the `.sha256` file next to it on the Releases page:

```powershell
Get-FileHash .\Verba-Setup-0.4.0.exe -Algorithm SHA256
```

The two hashes must be identical. The source code is in this repository, and the Releases are built by GitHub Actions from it.

Some antivirus programs are also wary of any program that watches the keyboard (Verba needs this to read Tab and Esc while its popup is shown). If yours blocks Verba, you can check the code yourself and add an exception for the Verba folder. Never turn SmartScreen or your antivirus off for this.

Why the warning exists and what can remove it: [SIGNING.md](SIGNING.md).

### What the installer does

| What | Where |
|---|---|
| The program, `verba.exe` | `%LOCALAPPDATA%\Programs\Verba` (all users: `C:\Program Files\Verba`) |
| The word lists (English, French, Arabic) | `data\words` inside that folder. CC-BY-SA-4.0, see `WORD-LISTS-LICENSE.txt` |
| A Start Menu shortcut (and a desktop one if you tick it) | Start Menu |
| "Start with Windows" (if ticked) | one value `Verba` in your own `HKCU\...\CurrentVersion\Run` |
| Your choices (theme, languages, programs) | `%APPDATA%\Verba\settings.txt`, written by the program, not the installer |

### Start, quit, uninstall

- **Start:** Start Menu > Verba.
- **Quit:** the tray icon's menu > *Quit Verba*, or press **Ctrl+Alt+Shift+Q**.
- **Pause:** **Ctrl+Alt+Shift+V**, or the tray menu.
- **Update:** run the newer Setup. It closes the running Verba first and keeps your settings.
- **Uninstall:** Settings > Apps > *Verba* > Uninstall. It asks whether to delete your settings too (the default is No).

### Privacy

Verba reads only the few letters before the caret in the text box you are typing in, never in password boxes, and by default it keeps nothing.
It sends nothing over the network: it has no network code.

Two options, both **off** until you turn them on in the settings, keep something on your own PC:

- **Learn the words I use:** Verba counts the words you type and the pairs of neighbouring words, to offer your own words and better next-word suggestions. It never stores a sentence, never three words in a row, no numbers, addresses, e-mails or paths, and nothing from the programs you excluded or from password boxes. The files are plain text in `%APPDATA%\Verba` (`learned.txt`, readable by anyone who can open your user folder). *Forget what it learned* in the settings, or deleting `learned.txt`, erases it all.
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

Other pieces: `scripts/make_icon.py` regenerates `crates/verba-app/assets/verba.ico`; `crates/verba-app/build.rs` puts the icon, version information and manifest into `verba.exe` (it needs `rc.exe` from the Windows SDK, and only warns if it is missing); `.cargo/config.toml` links the C runtime statically.

### Publish a release

1. Change `version` in `[workspace.package]` of the root `Cargo.toml`, commit and push.
2. Tag and push the tag. The tag must be `v` plus that exact version:

   ```
   git tag v0.4.0
   git push origin v0.4.0
   ```

3. The **Release** workflow (`.github/workflows/release.yml`) builds the installer and creates the GitHub Release with the Setup and its `.sha256`. It is marked *pre-release* when the version starts with `0.` or the tag has a hyphen (`v0.4.0-beta1`).

The **CI** workflow builds and tests every push to `main` and every pull request.
