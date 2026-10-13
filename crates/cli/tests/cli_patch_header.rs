//! M6 Task 10 (DECISIONS S7): `sdc patch-header`.
use image::{ImageBuffer, Rgb};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sdc() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_sdc"));
    c.env(
        "APPDATA",
        std::env::temp_dir().join("sdc-test-appdata-none"),
    );
    c
}
fn code(o: &Output) -> i32 {
    o.status.code().unwrap()
}
fn text(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap()
}

/// `<root>/wall.dds`, a BC7 UNORM diffuse with 7 mips whose header lacks the MIPMAPCOUNT flag and the
/// COMPLEX|MIPMAP caps (`check_header` flags both, so `sdc validate` fails it; `validate_files` passes no
/// `expected_levels`, so a wrong *count* alone would not fail it).
fn broken(root: &Path) -> PathBuf {
    let src = root.join("src");
    std::fs::create_dir_all(&src).unwrap();
    ImageBuffer::from_fn(64, 64, |x, y| Rgb([x as u8 * 4, y as u8 * 4, 90]))
        .save(src.join("wall.png"))
        .unwrap();
    let out = root.join("conv");
    let o = sdc()
        .args([
            "convert",
            "--target",
            "vanilla",
            "--set",
            "subtype=diffuse",
            "-o",
        ])
        .arg(&out)
        .arg(src.join("wall.png"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let mut b = std::fs::read(out.join("wall.dds")).unwrap();
    let flags = u32::from_le_bytes(b[8..12].try_into().unwrap()) & !0x2_0000; // DDSD_MIPMAPCOUNT
    b[8..12].copy_from_slice(&flags.to_le_bytes());
    b[108..112].copy_from_slice(&0x1000u32.to_le_bytes()); // DDSCAPS_TEXTURE only
    let p = root.join("wall.dds");
    std::fs::write(&p, b).unwrap();
    p
}

#[test]
fn out_dir_gets_a_patched_copy_that_validates_and_the_source_is_untouched() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let before = std::fs::read(&p).unwrap();
    let o = sdc().args(["validate"]).arg(&p).output().unwrap();
    assert_eq!(code(&o), 4, "broken first: {}", text(&o.stdout));
    let fixed = d.path().join("fixed");
    let o = sdc()
        .args(["patch-header", "--out-dir"])
        .arg(&fixed)
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}{}", text(&o.stdout), text(&o.stderr));
    assert!(
        text(&o.stdout).contains("DDS-HEADER-PATCHED"),
        "{}",
        text(&o.stdout)
    );
    let after = std::fs::read(fixed.join("wall.dds")).unwrap();
    assert_eq!(&after[148..], &before[148..], "payload byte-identical");
    assert_eq!(std::fs::read(&p).unwrap(), before, "source untouched");
    let o = sdc()
        .args(["validate"])
        .arg(fixed.join("wall.dds"))
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stdout));
}

#[test]
fn in_place_and_dry_run() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let before = std::fs::read(&p).unwrap();
    let o = sdc()
        .args(["patch-header", "--in-place", "--dry-run"])
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0);
    assert_eq!(std::fs::read(&p).unwrap(), before, "dry run writes nothing");
    let o = sdc()
        .args(["patch-header", "--in-place"])
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    let after = std::fs::read(&p).unwrap();
    assert_ne!(
        u32::from_le_bytes(after[8..12].try_into().unwrap()) & 0x2_0000,
        0,
        "MIPMAPCOUNT restored"
    );
    assert_eq!(
        u32::from_le_bytes(after[108..112].try_into().unwrap()),
        0x1000 | 0x8 | 0x40_0000
    );
}

#[test]
fn a_subtype_profile_fixes_the_tag_and_an_impossible_tag_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let fixed = d.path().join("fixed");
    let o = sdc()
        .args([
            "patch-header",
            "--target",
            "cs",
            "--subtype",
            "pbr_albedo",
            "--out-dir",
        ])
        .arg(&fixed)
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stdout));
    assert_eq!(
        u32::from_le_bytes(
            std::fs::read(fixed.join("wall.dds")).unwrap()[128..132]
                .try_into()
                .unwrap()
        ),
        99
    );
    let o = sdc()
        .args([
            "patch-header",
            "--tag",
            "srgb",
            "--format",
            "bc4",
            "--dry-run",
            "--in-place",
        ])
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(code(&o), 1);
    assert!(
        text(&o.stdout).contains("DDS-HEADER-UNPATCHABLE"),
        "{}",
        text(&o.stdout)
    );
}

#[test]
fn a_valid_file_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let fixed = d.path().join("fixed");
    sdc()
        .args(["patch-header", "--in-place"])
        .arg(&p)
        .output()
        .unwrap();
    let o = sdc()
        .args(["patch-header", "--out-dir"])
        .arg(&fixed)
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0);
    assert!(
        text(&o.stdout).contains("nothing to patch"),
        "{}",
        text(&o.stdout)
    );
    assert!(!fixed.join("wall.dds").exists());
}

#[test]
fn an_output_choice_is_required() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let o = sdc().args(["patch-header"]).arg(&p).output().unwrap();
    assert_eq!(code(&o), 2);
}

/// Controller ruling: a failed patch leaves the original byte-identical, whether refused or the write fails.
#[test]
fn a_failed_in_place_patch_leaves_the_original_byte_identical() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let before = std::fs::read(&p).unwrap();
    let o = sdc()
        .args(["patch-header", "--format", "bc4", "--in-place"])
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(code(&o), 1, "{}", text(&o.stdout));
    assert_eq!(
        std::fs::read(&p).unwrap(),
        before,
        "refused patch writes nothing"
    );
}

/// Windows only: a read-only file in a writable folder does not stop `rename` on Unix.
#[cfg(windows)]
#[test]
fn a_failed_write_leaves_the_original_byte_identical_and_is_not_reported_patched() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let before = std::fs::read(&p).unwrap();
    let mut perm = std::fs::metadata(&p).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&p, perm.clone()).unwrap();
    let o = sdc()
        .args(["patch-header", "--in-place"])
        .arg(&p)
        .output()
        .unwrap();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&p, perm).unwrap();
    assert_eq!(
        code(&o),
        3,
        "write onto a read-only file fails: {}",
        text(&o.stderr)
    );
    assert!(
        !text(&o.stdout).contains("DDS-HEADER-PATCHED"),
        "{}",
        text(&o.stdout)
    );
    assert_eq!(
        std::fs::read(&p).unwrap(),
        before,
        "failed write leaves the original"
    );
    let leftovers: Vec<_> = std::fs::read_dir(d.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "no temp left behind");
}

/// DECISIONS S6: `--out-dir` that resolves to the input's own folder is refused, never a silent overwrite.
#[test]
fn out_dir_equal_to_the_input_folder_is_a_usage_error() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let before = std::fs::read(&p).unwrap();
    let o = sdc()
        .args(["patch-header", "--out-dir"])
        .arg(d.path())
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(code(&o), 2, "{}", text(&o.stderr));
    // Another spelling of the same folder.
    let o = sdc()
        .args(["patch-header", "--out-dir"])
        .arg(d.path().join("src").join(".."))
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(code(&o), 2, "{}", text(&o.stderr));
    assert_eq!(std::fs::read(&p).unwrap(), before, "source untouched");
}

#[test]
fn dry_run_with_out_dir_writes_nothing() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let fixed = d.path().join("fixed");
    let o = sdc()
        .args(["patch-header", "--dry-run", "--out-dir"])
        .arg(&fixed)
        .arg(&p)
        .output()
        .unwrap();
    assert_eq!(code(&o), 0, "{}", text(&o.stderr));
    assert!(
        text(&o.stdout).contains("DDS-HEADER-PATCHED"),
        "{}",
        text(&o.stdout)
    );
    assert!(!fixed.exists(), "dry run creates nothing");
}

#[test]
fn two_inputs_with_one_name_are_refused_with_out_dir() {
    let d = tempfile::tempdir().unwrap();
    let p = broken(d.path());
    let other = d.path().join("b");
    std::fs::create_dir_all(&other).unwrap();
    let q = other.join("WALL.dds");
    std::fs::copy(&p, &q).unwrap();
    let fixed = d.path().join("fixed");
    let o = sdc()
        .args(["patch-header", "--out-dir"])
        .arg(&fixed)
        .arg(&p)
        .arg(&q)
        .output()
        .unwrap();
    assert_eq!(code(&o), 2, "{}", text(&o.stderr));
    assert!(
        text(&o.stderr).contains("two inputs are named"),
        "{}",
        text(&o.stderr)
    );
    assert!(!fixed.exists());
}

/// Audit left-out: a folder is a usage error naming it, not "Access is denied".
#[test]
fn patch_header_given_a_folder_is_a_usage_error() {
    let d = tempfile::tempdir().unwrap();
    let o = sdc()
        .arg("patch-header")
        .arg("--dry-run")
        .arg("--in-place")
        .arg(d.path())
        .output()
        .unwrap();
    assert_eq!(code(&o), 2, "{}", text(&o.stderr));
    assert!(
        text(&o.stderr).contains("is a folder"),
        "{}",
        text(&o.stderr)
    );
}
