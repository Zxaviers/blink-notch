//! Puts Blink's icon and its version information into `blink.exe` (what Explorer, the taskbar, Installed apps and the Start Menu
//! show). The version comes from Cargo, so there is one place to change it.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/icon/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let mut numbers: Vec<u32> = version.split(['.', '-', '+']).map(|p| p.parse().unwrap_or(0)).take(3).collect();
    numbers.resize(3, 0);
    let (major, minor, patch) = (numbers[0], numbers[1], numbers[2]);
    let icon = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("../../assets/icon/icon.ico");
    let icon = icon.to_string_lossy().replace('\\', "/");
    let rc = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Zxaviers"
      VALUE "FileDescription", "Blink"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "blink"
      VALUE "OriginalFilename", "blink.exe"
      VALUE "ProductName", "Blink"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap_or_default()).join("blink.rc");
    std::fs::write(&out, rc).expect("write the resource script");
    embed_resource::compile(&out, embed_resource::NONE).manifest_required().expect("embed the icon and version information");
}
