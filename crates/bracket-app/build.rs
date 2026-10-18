//! Windows resources of bracket.exe: the application manifest (DPI awareness, long paths, Windows
//! 10/11), the icon and the version information shown in Explorer's file properties.
//!
//! The icon is drawn by `build/icon.rs`. A 64×64 copy in raw RGBA is also written for the
//! window icon, which eframe takes as pixels.

#[path = "build/icon.rs"]
mod icon;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Sizes in the `.ico`: the ones Windows asks for at 100 % to 300 % scale.
const ICON_SIZES: [u32; 9] = [16, 20, 24, 32, 40, 48, 64, 96, 256];
pub const WINDOW_ICON_SIZE: u32 = 64;

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-changed=build/icon.rs");
    println!("cargo::rerun-if-changed=res/bracket.manifest");

    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo sets it"));

    let ico = out.join("bracket.ico");
    fs::write(&ico, icon::ico(&ICON_SIZES)).expect("writing the icon");
    fs::write(out.join("window-icon.rgba"), icon::rgba(WINDOW_ICON_SIZE))
        .expect("writing the window icon");

    let rc = out.join("bracket.rc");
    let source = resource_script(&ico, &manifest_dir.join("res").join("bracket.manifest"));
    fs::write(&rc, source).expect("writing the resource script");
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_required()
        .expect("compiling the Windows resources (rc.exe from the Windows SDK)");
}

/// The resource script: icon, manifest and version information.
fn resource_script(ico: &Path, manifest: &Path) -> String {
    let version = env::var("CARGO_PKG_VERSION").expect("cargo sets it");
    let number = |part: &str| {
        env::var(format!("CARGO_PKG_VERSION_{part}"))
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(0)
    };
    let (major, minor, patch) = (number("MAJOR"), number("MINOR"), number("PATCH"));
    let path = |p: &Path| p.display().to_string().replace('\\', "\\\\");
    format!(
        r#"#pragma code_page(65001)

1 ICON "{ico}"
1 24 "{manifest}"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3F
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "FileDescription", "Bracket"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "bracket"
            VALUE "LegalCopyright", "MIT OR Apache-2.0"
            VALUE "OriginalFilename", "bracket.exe"
            VALUE "ProductName", "Bracket"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#,
        ico = path(ico),
        manifest = path(manifest),
    )
}
