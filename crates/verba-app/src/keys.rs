//! The keys that drive the suggestion popup, and how they are written in `settings.txt`.
//!
//! Four actions while the popup is open: accept the highlighted suggestion, highlight the previous or the next one, close the popup.
//! Each is one key, with Ctrl, Alt or Shift if the user wants. Nothing here touches Windows: it is plain data, so it is tested.

/// Modifier bits of a binding.
pub const MOD_CTRL: u8 = 1;
pub const MOD_ALT: u8 = 2;
pub const MOD_SHIFT: u8 = 4;

const VK_BACK: u32 = 0x08;
const VK_TAB: u32 = 0x09;
const VK_RETURN: u32 = 0x0D;
const VK_ESCAPE: u32 = 0x1B;
const VK_SPACE: u32 = 0x20;
const VK_LEFT: u32 = 0x25;
const VK_RIGHT: u32 = 0x27;

/// One key with the modifiers that must be held with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    /// Windows virtual key code.
    pub vk: u32,
    pub mods: u8,
}

impl Binding {
    pub const fn new(vk: u32, mods: u8) -> Self {
        Self { vk, mods }
    }

    /// Whether a key can be used. Modifier keys alone cannot. With Ctrl or Alt any other key can. Without them only keys that are
    /// never typed as text can: Tab, Enter, Esc, the arrows, Home, End, Page Up and Down, Insert, Delete and the function keys.
    /// A letter, a digit, Space or Backspace on its own would stop the user from typing while the popup is open.
    pub fn usable(vk: u32, mods: u8) -> bool {
        let modifier_key = matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C | 0xA0..=0xA5 | 0x14);
        if modifier_key || vk == 0 || vk > 254 {
            return false;
        }
        if mods & (MOD_CTRL | MOD_ALT) != 0 {
            return true;
        }
        matches!(vk, VK_TAB | VK_RETURN | VK_ESCAPE | 0x21..=0x28 | 0x2D | 0x2E | 0x70..=0x87)
    }

    /// Whether a key can be a shortcut that works in every app (registered with Windows): it needs Ctrl or Alt with it.
    pub fn usable_hotkey(vk: u32, mods: u8) -> bool {
        mods & (MOD_CTRL | MOD_ALT) != 0 && Self::usable(vk, mods)
    }

    /// `9:0` for Tab, `39:1` for Ctrl+Right.
    pub fn render(self) -> String {
        format!("{}:{}", self.vk, self.mods)
    }

    /// The reverse of `render`; `None` for anything that is not a usable binding.
    pub fn parse(text: &str) -> Option<Self> {
        let (vk, mods) = text.trim().split_once(':')?;
        let (vk, mods) = (vk.trim().parse::<u32>().ok()?, mods.trim().parse::<u8>().ok()?);
        (mods <= 7 && Self::usable(vk, mods)).then_some(Self { vk, mods })
    }

    /// What the key is called on a key cap: `Tab`, `←`, `Ctrl+→`.
    pub fn label(self) -> String {
        let mut out = String::new();
        if self.mods & MOD_CTRL != 0 {
            out.push_str("Ctrl+");
        }
        if self.mods & MOD_ALT != 0 {
            out.push_str("Alt+");
        }
        if self.mods & MOD_SHIFT != 0 {
            out.push_str("Shift+");
        }
        out.push_str(&key_name(self.vk));
        out
    }
}

/// The name of a key (without modifiers).
pub fn key_name(vk: u32) -> String {
    match vk {
        VK_BACK => "Backspace".into(),
        VK_TAB => "Tab".into(),
        VK_RETURN => "Enter".into(),
        VK_ESCAPE => "Esc".into(),
        VK_SPACE => "Space".into(),
        0x21 => "Page Up".into(),
        0x22 => "Page Down".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        VK_LEFT => "\u{2190}".into(),
        0x26 => "\u{2191}".into(),
        VK_RIGHT => "\u{2192}".into(),
        0x28 => "\u{2193}".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x30..=0x39 | 0x41..=0x5A => char::from_u32(vk).map(|c| c.to_string()).unwrap_or_default(),
        0x60..=0x69 => format!("Num {}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => format!("Key {vk}"),
    }
}

/// The four things a key can do while the popup is open. `Prev` moves the highlight to the left, `Next` to the right
/// (the names are kept from the settings file).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Accept,
    Prev,
    Next,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keys {
    pub accept: Binding,
    pub prev: Binding,
    pub next: Binding,
    pub close: Binding,
    /// The shortcut that translates the selected word, in every app (not only while the popup is open).
    pub translate: Binding,
}

/// Tab picks, the left and right arrows move, Esc closes.
pub const DEFAULT_KEYS: Keys = Keys {
    accept: Binding::new(VK_TAB, 0),
    prev: Binding::new(VK_LEFT, 0),
    next: Binding::new(VK_RIGHT, 0),
    close: Binding::new(VK_ESCAPE, 0),
    translate: Binding::new(0x54, MOD_CTRL | MOD_ALT), // Ctrl+Alt+T
};

impl Default for Keys {
    fn default() -> Self {
        DEFAULT_KEYS
    }
}

impl Keys {
    pub fn get(&self, action: Action) -> Binding {
        match action {
            Action::Accept => self.accept,
            Action::Prev => self.prev,
            Action::Next => self.next,
            Action::Close => self.close,
        }
    }

    fn slot(&mut self, action: Action) -> &mut Binding {
        match action {
            Action::Accept => &mut self.accept,
            Action::Prev => &mut self.prev,
            Action::Next => &mut self.next,
            Action::Close => &mut self.close,
        }
    }

    /// Gives an action a key. When another action already has that key, the two swap, so no key ever does two things.
    pub fn assign(&mut self, action: Action, binding: Binding) {
        let old = self.get(action);
        for other in [Action::Accept, Action::Prev, Action::Next, Action::Close] {
            if other != action && self.get(other) == binding {
                *self.slot(other) = old;
            }
        }
        *self.slot(action) = binding;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_tab_left_right_and_escape() {
        let k = Keys::default();
        assert_eq!((k.accept.label(), k.prev.label(), k.next.label(), k.close.label()), ("Tab".into(), "\u{2190}".into(), "\u{2192}".into(), "Esc".into()));
    }

    #[test]
    fn a_binding_survives_being_written_and_read() {
        for b in [Binding::new(9, 0), Binding::new(39, MOD_CTRL), Binding::new(0x41, MOD_CTRL | MOD_ALT), Binding::new(0x7B, MOD_SHIFT)] {
            assert_eq!(Binding::parse(&b.render()), Some(b));
        }
    }

    #[test]
    fn keys_that_would_stop_typing_are_refused() {
        // letters, digits, space and backspace alone, or with Shift only, are text
        for vk in [0x41, 0x5A, 0x30, VK_SPACE, VK_BACK, 0xBE] {
            assert!(!Binding::usable(vk, 0), "{vk} alone");
            assert!(!Binding::usable(vk, MOD_SHIFT), "Shift+{vk}");
        }
        // with Ctrl or Alt they are shortcuts, not text
        assert!(Binding::usable(0x41, MOD_CTRL) && Binding::usable(VK_SPACE, MOD_ALT));
        // the keys made for this
        for vk in [VK_TAB, VK_RETURN, VK_ESCAPE, VK_LEFT, VK_RIGHT, 0x26, 0x28, 0x24, 0x23, 0x21, 0x22, 0x2D, 0x2E, 0x70, 0x87] {
            assert!(Binding::usable(vk, 0), "{vk}");
        }
        // a modifier key by itself is not a key
        for vk in [0x10, 0x11, 0x12, 0xA0, 0xA5, 0x5B] {
            assert!(!Binding::usable(vk, MOD_CTRL), "{vk}");
        }
    }

    #[test]
    fn damaged_text_gives_no_binding() {
        for text in ["", "9", "x:0", "9:9", "65:0", "0:0", "300:0", "9:-1"] {
            assert_eq!(Binding::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn labels_say_the_modifiers_first() {
        assert_eq!(Binding::new(VK_RIGHT, MOD_CTRL | MOD_SHIFT).label(), "Ctrl+Shift+\u{2192}");
        assert_eq!(Binding::new(0x70, 0).label(), "F1");
        assert_eq!(Binding::new(0x41, MOD_ALT).label(), "Alt+A");
    }

    #[test]
    fn giving_an_action_a_key_that_another_has_swaps_the_two() {
        let mut k = Keys::default();
        k.assign(Action::Accept, Binding::new(VK_RIGHT, 0));
        assert_eq!(k.accept, Binding::new(VK_RIGHT, 0));
        assert_eq!(k.next, Binding::new(VK_TAB, 0), "the old Tab went to the action that lost its key");
        // no two actions ever share a key
        let all = [k.accept, k.prev, k.next, k.close];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn the_translate_shortcut_needs_ctrl_or_alt_and_is_not_swapped_with_the_popup_keys() {
        assert_eq!(Keys::default().translate.label(), "Ctrl+Alt+T");
        assert!(Binding::usable_hotkey(0x54, MOD_CTRL | MOD_ALT));
        assert!(Binding::usable_hotkey(0x71, MOD_ALT), "Alt+F2");
        assert!(!Binding::usable_hotkey(VK_TAB, 0), "a plain Tab can not be a shortcut for every app");
        assert!(!Binding::usable_hotkey(0x54, MOD_SHIFT), "Shift alone neither");
        let mut k = Keys::default();
        k.assign(Action::Accept, Binding::new(VK_RETURN, 0));
        assert_eq!(k.translate, DEFAULT_KEYS.translate, "assigning a popup key leaves the shortcut alone");
    }

    #[test]
    fn a_new_free_key_just_replaces() {
        let mut k = Keys::default();
        k.assign(Action::Accept, Binding::new(VK_RETURN, 0));
        assert_eq!(k.accept, Binding::new(VK_RETURN, 0));
        assert_eq!((k.prev, k.next, k.close), (DEFAULT_KEYS.prev, DEFAULT_KEYS.next, DEFAULT_KEYS.close));
    }
}
