//! Verba on Windows.

pub mod apps;
pub mod dialog;
pub mod focus;
pub mod layouts;
pub mod look;
pub mod overlay;
pub mod splash;
pub mod ui;

pub use apps::{InstalledApp, describe_foreground, exe_of_window, foreground_app, installed_apps, keyboard_lang, running_apps};
pub use focus::{Anchor, AnchorKind, Locator, Picked, Snapshot};
pub use look::{Look, Theme, system_accent};
pub use overlay::Overlay;
pub use splash::run_with_splash;
