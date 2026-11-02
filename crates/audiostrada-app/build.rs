//! Embeds the icon and version information in the Windows executable.

fn main() {
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows");
    // The resource compiler only comes with Windows builds; cross checks skip it.
    let host_windows = std::env::var("HOST").is_ok_and(|h| h.contains("windows"));
    if target_windows && host_windows {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/icons/audiostrada.ico");
        if let Err(e) = res.compile() {
            println!("cargo::warning=cannot embed the Windows icon: {e}");
        }
    }
    println!("cargo::rerun-if-changed=build.rs");
}
