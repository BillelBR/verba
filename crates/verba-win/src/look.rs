//! How the popup looks: light or dark, and the accent colour. Both follow Windows unless the user picks.

use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::core::{PCWSTR, w};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    /// What Windows is set to for apps (Settings > Personalization > Colors).
    pub fn system() -> Theme {
        match read_dword(w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"), w!("AppsUseLightTheme")) {
            Some(0) => Theme::Dark,
            _ => Theme::Light,
        }
    }
}

/// Theme plus accent colour, as the popup needs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Look {
    pub theme: Theme,
    /// Accent as red, green, blue.
    pub accent: [u8; 3],
}

impl Look {
    /// Windows' own theme and accent colour.
    pub fn system() -> Self {
        Self { theme: Theme::system(), accent: system_accent() }
    }

    /// A fixed theme with Windows' accent colour.
    pub fn with_theme(theme: Theme) -> Self {
        Self { theme, accent: system_accent() }
    }
}

/// The accent colour chosen in Windows settings (blue when it cannot be read).
pub fn system_accent() -> [u8; 3] {
    // stored as 0xAABBGGRR
    match read_dword(w!("Software\\Microsoft\\Windows\\DWM"), w!("AccentColor")) {
        Some(v) => [(v & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, ((v >> 16) & 0xFF) as u8],
        None => [0, 120, 212],
    }
}

fn read_dword(key: PCWSTR, value: PCWSTR) -> Option<u32> {
    unsafe {
        let mut data = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let r = RegGetValueW(HKEY_CURRENT_USER, key, value, RRF_RT_REG_DWORD, None, Some(&mut data as *mut u32 as *mut _), Some(&mut size));
        r.is_ok().then_some(data)
    }
}
