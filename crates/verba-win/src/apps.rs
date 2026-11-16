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
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        full.rsplit('\\').next().map(|s| s.to_lowercase())
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
