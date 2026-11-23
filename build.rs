//! Embeds `envedit.manifest` (Common Controls v6, PerMonitorV2 DPI,
//! asInvoker) into the executable using the MSVC linker.

fn main() {
    println!("cargo:rerun-if-changed=envedit.manifest");
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    let dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo");
    let manifest = std::path::Path::new(&dir).join("envedit.manifest");
    println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
    // The manifest carries its own trustInfo; don't let the linker add a second one.
    println!("cargo:rustc-link-arg-bins=/MANIFESTUAC:NO");
}
