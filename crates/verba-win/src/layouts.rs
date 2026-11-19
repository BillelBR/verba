//! The keyboard layouts installed in Windows, as tables of what every key gives.
//!
//! Windows computes the answer itself: for a key (its scan code), the modifiers held and a layout, `ToUnicodeEx` says which characters
//! come out. So no table is written by hand and any layout the user installed works. Nothing is typed and no key is pressed: the
//! calls only read the layout (the flag `4` of `ToUnicodeEx` keeps it from changing the state of the keyboard, dead keys included).

use verba_core::Lang;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayoutList, HKL, MAPVK_VSC_TO_VK_EX, MapVirtualKeyExW, ToUnicodeEx};
use windows::Win32::Globalization::{GetLocaleInfoW, LOCALE_SENGLISHLANGUAGENAME};

/// Shift held with the key (the same number as `verba_local::wrongkeys::SHIFT`).
pub const SHIFT: u8 = 1;
/// AltGr (Ctrl+Alt) held with the key (the same number as `verba_local::wrongkeys::ALTGR`).
pub const ALTGR: u8 = 2;

/// One installed layout and what its keys give.
#[derive(Clone, Debug)]
pub struct InstalledLayout {
    /// The language of the layout in English (`Arabic`, `French`), with its identifier (`00020401`) so that two layouts of one language differ.
    pub name: String,
    /// The language Verba has a list for, `None` for any other (German, say).
    pub lang: Option<Lang>,
    /// Every key that gives text: (scan code, modifiers, the text).
    pub keys: Vec<(u16, u8, String)>,
}

/// The scan codes of the keys that type text on the main block: the number row, the three letter rows, the key left of Z on a 102-key
/// keyboard (0x56) and the space bar. They are the same on every layout, which is what makes "the same key" mean something.
fn scan_codes() -> Vec<u16> {
    let mut v: Vec<u16> = (0x02..=0x0D).chain(0x10..=0x1B).chain(0x1E..=0x29).chain(0x2B..=0x35).collect();
    v.push(0x56);
    v.push(0x39);
    v
}

/// The language of a layout handle when Verba has a list for it.
pub fn lang_of(hkl: HKL) -> Option<Lang> {
    match (hkl.0 as usize & 0x3FF) as u16 {
        0x09 => Some(Lang::En),
        0x0C => Some(Lang::Fr),
        0x01 => Some(Lang::Ar),
        _ => None,
    }
}

fn name_of(hkl: HKL) -> String {
    let lang_id = (hkl.0 as usize & 0xFFFF) as u32;
    // the layout identifier is in the high word when it is not the default one of the language (Arabic 102 AZERTY is 0002)
    let layout_id = (hkl.0 as usize >> 16) & 0xFFFF;
    let mut buf = [0u16; 80];
    let n = unsafe { GetLocaleInfoW(lang_id, LOCALE_SENGLISHLANGUAGENAME, Some(&mut buf)) };
    let language = if n > 1 { String::from_utf16_lossy(&buf[..n as usize - 1]) } else { format!("{lang_id:04X}") };
    format!("{language} {layout_id:04X}{lang_id:04X}")
}

/// What the keys of one layout give: the plain key, with Shift, with AltGr, with both. A dead key (the French circumflex) gives its own
/// character. A key that gives a control character (Ctrl+Alt on a layout without AltGr) or nothing is left out.
pub fn keys_of(hkl: HKL) -> Vec<(u16, u8, String)> {
    let mut out = Vec::new();
    for mods in [0, SHIFT, ALTGR, SHIFT | ALTGR] {
        for scan in scan_codes() {
            let vk = unsafe { MapVirtualKeyExW(scan as u32, MAPVK_VSC_TO_VK_EX, Some(hkl)) };
            if vk == 0 {
                continue;
            }
            let mut state = [0u8; 256];
            if mods & SHIFT != 0 {
                state[0x10] = 0x80; // VK_SHIFT
            }
            if mods & ALTGR != 0 {
                state[0x11] = 0x80; // VK_CONTROL
                state[0x12] = 0x80; // VK_MENU
            }
            let mut buf = [0u16; 8];
            let n = unsafe { ToUnicodeEx(vk, scan as u32, &state, &mut buf, 4, Some(hkl)) };
            // n < 0: a dead key, whose own character is in the buffer
            let len = if n < 0 { 1 } else { n as usize }.min(buf.len());
            if len == 0 {
                continue;
            }
            let text = String::from_utf16_lossy(&buf[..len]);
            if !text.chars().any(char::is_control) {
                out.push((scan, mods, text));
            }
        }
    }
    out
}

/// The layouts installed now (the list the language bar switches between), in the order Windows gives them.
pub fn installed() -> Vec<InstalledLayout> {
    unsafe {
        let count = GetKeyboardLayoutList(None);
        if count <= 0 {
            return Vec::new();
        }
        let mut list = vec![HKL::default(); count as usize];
        let got = GetKeyboardLayoutList(Some(&mut list)).max(0) as usize;
        list.truncate(got);
        list.into_iter().map(|hkl| InstalledLayout { name: name_of(hkl), lang: lang_of(hkl), keys: keys_of(hkl) }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(layout: &InstalledLayout, scan: u16, mods: u8) -> Option<&str> {
        layout.keys.iter().find(|(s, m, _)| *s == scan && *m == mods).map(|(_, _, t)| t.as_str())
    }

    #[test]
    fn the_installed_layouts_give_letters_for_the_keys_of_the_letter_rows() {
        let layouts = installed();
        assert!(!layouts.is_empty(), "Windows always has one keyboard layout");
        for l in &layouts {
            assert!(l.keys.iter().any(|(s, m, _)| *s == 0x39 && *m == 0), "{}: the space bar gives a space", l.name);
            assert!(l.keys.len() > 30, "{}: only {} keys", l.name, l.keys.len());
        }
    }

    /// Needs the English (US), French and Arabic (102 AZERTY) keyboards of the author's PC; it only reads the layouts:
    ///   cargo test -p verba-win layouts -- --ignored --nocapture
    #[test]
    #[ignore = "needs the US English, French and Arabic keyboards installed"]
    fn the_real_layouts_of_this_pc() {
        let layouts = installed();
        for l in &layouts {
            eprintln!("{} ({:?}): {} keys; q-row: {:?}", l.name, l.lang, l.keys.len(), (0x10..0x1Au16).map(|s| text_of(l, s, 0).unwrap_or("-")).collect::<Vec<_>>());
        }
        let us = layouts.iter().find(|l| l.lang == Some(Lang::En)).expect("an English layout");
        assert_eq!(text_of(us, 0x10, 0), Some("q"));
        assert_eq!(text_of(us, 0x10, SHIFT), Some("Q"));
        let fr = layouts.iter().find(|l| l.lang == Some(Lang::Fr)).expect("a French layout");
        assert_eq!(text_of(fr, 0x10, 0), Some("a"), "the key of q is a on an AZERTY keyboard");
        assert_eq!(text_of(fr, 0x32, 0), Some(","), "and m is next to the l, on the row of the Z");
        assert!(text_of(fr, 0x03, ALTGR).is_some(), "AltGr+é is ~ on the French keyboard");
        let ar = layouts.iter().find(|l| l.lang == Some(Lang::Ar)).expect("an Arabic layout");
        assert!(ar.keys.iter().filter(|(_, m, _)| *m == 0).any(|(_, _, t)| t.chars().all(verba_core::is_arabic)), "the Arabic layout gives Arabic letters");
    }
}
