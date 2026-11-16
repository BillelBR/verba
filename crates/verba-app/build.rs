//! Puts the icon, the version information and the application manifest into verba.exe (Windows only).
//!
//! If the resource compiler (rc.exe, part of the Windows SDK) cannot be found, the build still succeeds:
//! the program then simply has no icon, version information or manifest, and a warning says so.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/verba.ico");
    println!("cargo:rerun-if-changed=assets/verba.manifest");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/verba.ico");
    res.set_manifest_file("assets/verba.manifest");
    res.set("ProductName", "Verba");
    res.set("FileDescription", "Verba writing assistant");
    res.set("OriginalFilename", "verba.exe");
    res.set("InternalName", "verba");
    res.set("LegalCopyright", "Verba contributors");
    res.set("CompanyName", "Verba contributors");
    res.set("FileVersion", &version);
    res.set("ProductVersion", &version);

    if let Err(e) = res.compile() {
        println!("cargo:warning=could not embed the icon, version information and manifest ({e}); verba.exe is built without them. Install the Windows SDK (rc.exe) to get them.");
    }
}
