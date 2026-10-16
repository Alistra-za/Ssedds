//! Windows exe resources (icon + version) via rc.exe; no crate needed. Missing rc.exe only warns.
use std::{env, fs, path::PathBuf, process::Command};

const NAME: &str = "sdc";
const DESC: &str = "Tessalith command line (sdc)";
const ICO: &str = "assets/sdc.ico";

fn rc_exe() -> Option<PathBuf> {
    if let Some(p) = env::var_os("RC") {
        return Some(p.into());
    }
    let bin = PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut vers: Vec<_> = fs::read_dir(bin)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    vers.sort(); // equal-width version dirs: the newest sorts last
    vers.into_iter()
        .rev()
        .map(|v| v.join(r"x64\rc.exe"))
        .find(|p| p.exists())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICO}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows")
        || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc")
    {
        return;
    }
    let Some(rc) = rc_exe() else {
        println!("cargo:warning=rc.exe not found; building without exe icon/version resources");
        return;
    };
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let ico = fs::canonicalize(ICO).unwrap();
    let ico = ico
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/");
    let ver = env::var("CARGO_PKG_VERSION").unwrap();
    let mut n: Vec<&str> = ver.split(['.', '-', '+']).take(3).collect();
    n.resize(3, "0");
    let commas = format!("{},0", n.join(","));
    let rc_text = format!(
        r#"1 ICON "{ico}"
1 VERSIONINFO
FILEVERSION {commas}
PRODUCTVERSION {commas}
FILEOS 0x40004
FILETYPE 1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "ProductName", "Tessalith"
      VALUE "FileDescription", "{DESC}"
      VALUE "FileVersion", "{ver}"
      VALUE "ProductVersion", "{ver}"
      VALUE "OriginalFilename", "{NAME}.exe"
      VALUE "InternalName", "{NAME}"
      VALUE "LegalCopyright", "Licensed under GPL-3.0-or-later"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#
    );
    let (rcf, res) = (out.join("tessalith.rc"), out.join("tessalith.res"));
    fs::write(&rcf, rc_text).unwrap();
    match Command::new(&rc)
        .args(["/nologo", "/fo"])
        .arg(&res)
        .arg(&rcf)
        .status()
    {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", res.display()),
        _ => println!("cargo:warning=rc.exe failed; building without exe icon/version resources"),
    }
}
