//! Embeds assets/icon.ico into the Windows executable so it shows the dragon
//! in Explorer, the taskbar and shortcuts. Cross-compile friendly via the
//! `winresource` crate (no windres needed). No-op on other targets.

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        println!("cargo:rerun-if-changed=assets/icon.ico");
        winresource::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("failed to embed assets/icon.ico into the Windows executable");
    }
}
