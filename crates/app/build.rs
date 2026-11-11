// Generates the .ico from the procedural logo and embeds it as the .exe icon (Windows only).
#[path = "src/logo.rs"]
#[allow(dead_code)]
mod logo;

fn main() {
    println!("cargo:rerun-if-changed=src/logo.rs");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("psnippet.ico");
    std::fs::write(&out, logo::ico(&[16, 24, 32, 48, 64, 128, 256])).expect("write ico");
    let mut res = winresource::WindowsResource::new();
    res.set_icon(out.to_str().unwrap());
    res.set("ProductName", "PSnippet");
    res.set("FileDescription", "PSnippet - text expander");
    if let Err(e) = res.compile() {
        // A missing resource compiler must not break the build; the exe just has the default icon.
        println!("cargo:warning=could not embed icon: {e}");
    }
}
