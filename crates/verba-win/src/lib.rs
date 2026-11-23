//! Verba on Windows.

pub mod apps;
pub mod dialog;
pub mod focus;
pub mod layouts;
pub mod look;
pub mod overlay;
pub mod splash;
pub mod ui;

pub use apps::{IconPixels, InstalledApp, describe_foreground, exe_of_window, foreground_app, icon_pixels, installed_apps, keyboard_lang, path_of_window, running_apps, running_programs};
pub use focus::{Anchor, AnchorKind, Locator, Picked, Snapshot};
pub use look::{Look, Theme, system_accent};
pub use overlay::Overlay;
pub use splash::run_with_splash;
