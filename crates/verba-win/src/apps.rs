//! Which program is in front, and which language its keyboard is set to.
//!
//! Only the program's file name (for example `chrome.exe`) and a language code are read: nothing about what is typed.

use verba_core::Lang;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyboardLayout;
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GW_OWNER, GetClassNameW, GetForegroundWindow, GetWindow, GetWindowLongW, GetWindowTextLengthW,
    GetWindowThreadProcessId, IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PWSTR};

/// The program file name of a window's process, in lower case (`chrome.exe`).
pub fn exe_of_window(hwnd: HWND) -> Option<String> {
    path_of_window(hwnd).and_then(|full| full.rsplit('\\').next().map(|s| s.to_lowercase()))
}

/// The full path of the program of a window's process (`C:\Program Files\Google\Chrome\Application\chrome.exe`).
pub fn path_of_window(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(process);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

fn class_of(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// Windows that belong to the taskbar and its tray: clicking Verba's own tray icon puts these in front,
/// and they are not "the app the user is typing in".
fn is_taskbar(hwnd: HWND) -> bool {
    matches!(
        class_of(hwnd).as_str(),
        "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "NotifyIconOverflowWindow" | "TopLevelWindowForOverflowXamlIsland" | "Windows.UI.Core.CoreWindow"
    )
}

/// The program the user is working in right now. `None` for the taskbar and for Verba itself.
pub fn foreground_app() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() || is_taskbar(hwnd) {
            return None;
        }
        exe_of_window(hwnd).filter(|e| e != "verba.exe")
    }
}

/// The program and the window class in front, for the field check (no title, nothing typed).
pub fn describe_foreground() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return "no window in front".to_string();
        }
        format!("program {}, window class {}", exe_of_window(hwnd).unwrap_or_else(|| "?".into()), class_of(hwnd))
    }
}

/// The language of the keyboard layout in use in the front window, when it is one Verba has a list for.
/// This is what the user's keys mean, which is a better guide than guessing from the letters.
pub fn keyboard_lang() -> Option<Lang> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let thread = GetWindowThreadProcessId(hwnd, None);
        let hkl = GetKeyboardLayout(thread);
        let lang_id = (hkl.0 as usize & 0xFFFF) as u16;
        match lang_id & 0x03FF {
            0x09 => Some(Lang::En),
            0x0C => Some(Lang::Fr),
            0x01 => Some(Lang::Ar),
            _ => None,
        }
    }
}

/// The programs that have a window on the screen right now with their full path, one entry for each program (no Verba): for lists that show icons.
pub fn running_programs() -> Vec<(String, String)> {
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        unsafe {
            let apps = &mut *(lp.0 as *mut Vec<(String, String)>);
            let owned = GetWindow(hwnd, GW_OWNER).map(|o| !o.is_invalid()).unwrap_or(false);
            let tool = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0;
            if IsWindowVisible(hwnd).as_bool() && !owned && !tool && GetWindowTextLengthW(hwnd) > 0 && !is_taskbar(hwnd) {
                if let Some(path) = path_of_window(hwnd) {
                    let exe = path.rsplit('\\').next().unwrap_or("").to_lowercase();
                    if !exe.is_empty() && exe != "verba.exe" && !apps.iter().any(|(e, _)| *e == exe) {
                        apps.push((exe, path));
                    }
                }
            }
        }
        true.into()
    }
    let mut apps: Vec<(String, String)> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut apps as *mut Vec<(String, String)> as isize));
    }
    apps
}

/// The programs that have a window on the screen right now (sorted, without repeats, without Verba): for "choose an app" lists.
pub fn running_apps() -> Vec<String> {
    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        unsafe {
            let apps = &mut *(lp.0 as *mut Vec<String>);
            let owned = GetWindow(hwnd, GW_OWNER).map(|o| !o.is_invalid()).unwrap_or(false);
            let tool = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0;
            if IsWindowVisible(hwnd).as_bool() && !owned && !tool && GetWindowTextLengthW(hwnd) > 0 && !is_taskbar(hwnd) {
                if let Some(exe) = exe_of_window(hwnd) {
                    if exe != "verba.exe" {
                        apps.push(exe);
                    }
                }
            }
        }
        true.into()
    }
    let mut apps: Vec<String> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut apps as *mut Vec<String> as isize));
    }
    apps.sort();
    apps.dedup();
    apps
}

/// A program found in the Start menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledApp {
    /// The name the Start menu shows (`Google Chrome`).
    pub name: String,
    /// The program's file name in lower case (`chrome.exe`): what Verba's list of programs holds.
    pub exe: String,
    /// The full path of the program (where its icon is read).
    pub path: String,
}

/// The programs installed on this PC that have a shortcut in the Start menu (the ones of every user and the ones of this user), with the name of the shortcut and the
/// file name of what it starts. Uninstallers, help files and the like are left out; programs of the Microsoft Store (they have no shortcut file) are not
/// found until they are open. Sorted by name, one entry for each program. It reads the shortcut files (a second or two on a PC with many programs),
/// so call it from a thread of its own.
pub fn installed_apps() -> Vec<InstalledApp> {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, STGM};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
    use windows::core::{Interface, PCWSTR};

    fn shortcuts(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() && depth < 5 {
                shortcuts(&path, depth + 1, out);
            } else if path.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
                out.push(path);
            }
        }
    }

    let mut dirs: Vec<PathBuf> = Vec::new();
    for var in ["ProgramData", "APPDATA"] {
        if let Some(p) = std::env::var_os(var) {
            dirs.push(PathBuf::from(p).join(r"Microsoft\Windows\Start Menu\Programs"));
        }
    }
    let mut files = Vec::new();
    for d in &dirs {
        shortcuts(d, 0, &mut files);
    }
    let mut found: HashMap<String, (String, String)> = HashMap::new();
    unsafe {
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        for file in &files {
            let name = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if is_not_a_program(&name) {
                continue;
            }
            let Ok(link) = CoCreateInstance::<_, IShellLinkW>(&ShellLink, None, CLSCTX_INPROC_SERVER) else { continue };
            let Ok(persist) = link.cast::<IPersistFile>() else { continue };
            let wide: Vec<u16> = file.as_os_str().to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
            if persist.Load(PCWSTR(wide.as_ptr()), STGM(0)).is_err() {
                continue;
            }
            let mut buf = [0u16; 520];
            let mut data = windows::Win32::Storage::FileSystem::WIN32_FIND_DATAW::default();
            if link.GetPath(&mut buf, &mut data, 0).is_err() {
                continue;
            }
            let len = buf.iter().position(|u| *u == 0).unwrap_or(buf.len());
            let target = String::from_utf16_lossy(&buf[..len]);
            let Some(exe) = target.rsplit('\\').next().map(str::to_lowercase) else { continue };
            // shortcuts of Windows that open a panel through a helper (Administrative Tools starts `control.exe`) are not programs
            if !exe.ends_with(".exe") || is_not_a_program(&exe) || matches!(exe.as_str(), "control.exe" | "mmc.exe" | "rundll32.exe" | "msiexec.exe" | "wscript.exe" | "cscript.exe" | "explorer.exe") {
                continue;
            }
            // the first name found for a program is kept (the files are not in any order, so the shortest is: `Chrome` before `Chrome (2)`)
            found.entry(exe).and_modify(|n| if name.len() < n.0.len() { *n = (name.clone(), target.clone()) }).or_insert((name, target));
        }
        if com {
            CoUninitialize();
        }
    }
    let mut apps: Vec<InstalledApp> = found.into_iter().map(|(exe, (name, path))| InstalledApp { name, exe, path }).filter(|a| a.exe != "verba.exe").collect();
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.exe.cmp(&b.exe)));
    apps
}

/// Shortcuts and files that are not programs a person types in: uninstallers, help, license, website links, setup helpers.
fn is_not_a_program(name: &str) -> bool {
    let n = name.to_lowercase();
    ["uninstall", "unins0", "unins1", "uninst", "readme", "read me", "release notes", "license", "help", "website", "web site", "documentation", "setup", "installer", "updater", "crashpad", "report"]
        .iter()
        .any(|w| n.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_not_a_program_is_left_out() {
        for n in ["Uninstall Notepad++", "unins000.exe", "Read me", "Readme", "Visit our website", "Help and support", "License Agreement", "Setup.exe"] {
            assert!(is_not_a_program(n), "{n}");
        }
        for n in ["Google Chrome", "Notepad++", "Microsoft Word", "chrome.exe", "WINWORD.EXE", "Telegram", "Spotify"] {
            assert!(!is_not_a_program(n), "{n}");
        }
    }

    #[test]
    fn the_start_menu_of_this_pc_gives_programs_with_a_name_and_a_file() {
        let apps = installed_apps();
        // every PC with Windows has some (and none is Verba, an uninstaller or a file that is not a program)
        for a in &apps {
            assert!(a.exe.ends_with(".exe") && !a.name.is_empty() && a.exe != "verba.exe", "{a:?}");
        }
        let mut seen = std::collections::HashSet::new();
        assert!(apps.iter().all(|a| seen.insert(a.exe.clone())), "one entry for each program");
        println!("{} programs, the first ones: {:?}", apps.len(), apps.iter().take(5).collect::<Vec<_>>());
    }
}

/// The picture of a program's icon: `width` by `height` pixels, blue, green, red and alpha bytes one after the other (the colours not premultiplied).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconPixels {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// The large icon (32 by 32) of a program file, as pixels. `None` when the file has none or cannot be read. Call it from a thread of its own.
pub fn icon_pixels(path: &str) -> Option<IconPixels> {
    use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
    unsafe {
        // the shell's icon handlers want COM on the thread
        let com = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let icon = icon_pixels_of(path);
        if com {
            CoUninitialize();
        }
        icon
    }
}

unsafe fn icon_pixels_of(path: &str) -> Option<IconPixels> {
    use std::ffi::c_void;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDIBits, GetObjectW, HGDIOBJ,
    };
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, ICONINFO};
    use windows::core::PCWSTR;
    unsafe {
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut info = SHFILEINFOW::default();
        let got = SHGetFileInfoW(PCWSTR(wide.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(0), Some(&mut info), std::mem::size_of::<SHFILEINFOW>() as u32, SHGFI_ICON | SHGFI_LARGEICON);
        if got == 0 || info.hIcon.is_invalid() {
            return None;
        }
        let mut icon = ICONINFO::default();
        let mut result = None;
        if GetIconInfo(info.hIcon, &mut icon).is_ok() {
            let mut bm = BITMAP::default();
            let colour = if icon.hbmColor.is_invalid() { icon.hbmMask } else { icon.hbmColor };
            if GetObjectW(HGDIOBJ(colour.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void)) != 0 && bm.bmWidth > 0 && bm.bmHeight > 0 {
                let (w, h) = (bm.bmWidth as usize, bm.bmHeight as usize);
                // an icon without a colour bitmap has a mask of double height (the mask, then the picture): not handled, the colour bitmap is what is read
                let hdc = CreateCompatibleDC(None);
                let read = |bitmap: windows::Win32::Graphics::Gdi::HBITMAP| -> Option<Vec<u8>> {
                    let mut bmi = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: w as i32,
                            biHeight: -(h as i32),
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    let mut buf = vec![0u8; w * h * 4];
                    let lines = GetDIBits(hdc, bitmap, 0, h as u32, Some(buf.as_mut_ptr() as *mut c_void), &mut bmi, DIB_RGB_COLORS);
                    (lines != 0).then_some(buf)
                };
                if !icon.hbmColor.is_invalid() {
                    if let Some(mut pixels) = read(icon.hbmColor) {
                        // an old icon has no alpha: its mask says what is transparent (a white pixel of the mask)
                        if pixels.chunks_exact(4).all(|p| p[3] == 0) {
                            if let Some(mask) = read(icon.hbmMask) {
                                for (p, m) in pixels.chunks_exact_mut(4).zip(mask.chunks_exact(4)) {
                                    p[3] = if m[0] == 0 { 255 } else { 0 };
                                }
                            }
                        }
                        result = Some(IconPixels { width: w as u32, height: h as u32, bgra: pixels });
                    }
                }
                let _ = DeleteDC(hdc);
            }
            if !icon.hbmColor.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(icon.hbmColor.0));
            }
            if !icon.hbmMask.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(icon.hbmMask.0));
            }
        }
        let _ = DestroyIcon(info.hIcon);
        result
    }
}
