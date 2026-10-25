//! Windows builds get the Railhead icon and version details compiled into
//! the .exe. macOS and Linux get their icons from the app bundle / .desktop\
//! file instead, which `cargo xtask dist` assembles from `assets/icons`.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=assets/icons/railhead.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let icon = root
        .join("assets/icons/railhead.ico")
        .to_string_lossy()
        .replace('\\', "/");
    let ver = std::env::var("CARGO_PKG_VERSION").unwrap();
    let nums: Vec<u32> = ver
        .split(['.', '-'])
        .filter_map(|s| s.parse().ok())
        .chain([0, 0, 0])
        .take(3)
        .collect();
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {a},{b},{c},0
PRODUCTVERSION {a},{b},{c},0
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "FileDescription", "Railhead"
            VALUE "ProductName", "Railhead"
            VALUE "FileVersion", "{ver}"
            VALUE "ProductVersion", "{ver}"
            VALUE "OriginalFilename", "railhead.exe"
            VALUE "InternalName", "railhead"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#,
        a = nums[0],
        b = nums[1],
        c = nums[2]
    );
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("railhead.rc");
    std::fs::write(&out, rc).unwrap();
    // Without a resource compiler the game still builds, just without its icon.
    if let Err(e) = embed_resource::compile(&out, embed_resource::NONE).manifest_optional() {
        println!("cargo:warning=no icon in the .exe: {e}");
    }
}
