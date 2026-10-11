//! DECISIONS O1 exit test "texconv and nvddsinfo read every output". Environment-gated (skipped otherwise, so the
//! suite carries no absolute path): `SDC_TEXCONV` = texconv.exe, `SDC_NVDDSINFO` = nvddsinfo.exe. Both only read
//! the tempdir outputs; texconv writes its PNGs into another tempdir.
use engine::api::*;
use engine::config::Target;
use engine::events::*;
use engine::overrides::parse_set;
use engine::policy::AcceptSuggestions;
use image::{DynamicImage, ImageBuffer, Rgb};
use std::sync::Arc;

#[test]
fn texconv_and_nvddsinfo_read_every_uncompressed_output() {
    let tex = std::env::var_os("SDC_TEXCONV");
    let info = std::env::var_os("SDC_NVDDSINFO");
    if tex.is_none() && info.is_none() {
        eprintln!("skipped: set SDC_TEXCONV and/or SDC_NVDDSINFO");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let src = d.path().join("in/textures/a");
    std::fs::create_dir_all(&src).unwrap();
    let rgb = DynamicImage::ImageRgb8(ImageBuffer::from_fn(64, 64, |x, y| {
        Rgb([(x * 4) as u8, (y * 4) as u8, 90])
    }));
    for n in ["c8.png", "s8.png", "r.png", "rg.png"] {
        rgb.save(src.join(n)).unwrap();
    }
    DynamicImage::ImageRgb32F(ImageBuffer::from_fn(64, 64, |x, _| {
        Rgb([x as f32 / 16.0, 0.5, 0.25])
    }))
    .save(d.path().join("in/textures/a/h_g.tif"))
    .unwrap();
    let mut req = BatchRequest::new(vec![d.path().join("in")], d.path().join("out"), Target::Cs);
    req.recursive = true;
    req.accept = AcceptSuggestions::High;
    req.on_existing = OnExisting::Overwrite;
    req.overrides = [
        "file:**/c8.png.subtype=diffuse",
        "file:**/c8.png.format=rgba8",
        "file:**/s8.png.subtype=pbr_albedo",
        "file:**/s8.png.format=rgba8",
        "file:**/r.png.subtype=height",
        "file:**/r.png.format=r8",
        "file:**/rg.png.subtype=generic_two_channel",
        "file:**/rg.png.format=rg8",
        "file:**/h_g.tif.subtype=pbr_emissive",
        "file:**/h_g.tif.format=rgba16f",
    ]
    .iter()
    .map(|s| parse_set(s).unwrap())
    .collect();
    let e = Engine::new().unwrap();
    let rep = Reporter::new(Arc::new(NullSink), "interop");
    let plan = e.plan(req, &rep, &CancelToken::default()).unwrap();
    let s = e.run(plan, &rep, &CancelToken::default()).unwrap();
    assert_eq!(s.converted, 5);
    let pngs = tempfile::tempdir().unwrap();
    for n in ["c8", "s8", "r", "rg", "h_g"] {
        let f = d.path().join(format!("out/textures/a/{n}.dds"));
        if let Some(t) = &tex {
            let o = std::process::Command::new(t)
                .args(["-nologo", "-y", "-ft", "png", "-o"])
                .arg(pngs.path())
                .arg(&f)
                .output()
                .unwrap();
            assert!(
                o.status.success(),
                "texconv {n}: {}",
                String::from_utf8_lossy(&o.stdout)
            );
        }
        if let Some(i) = &info {
            let o = std::process::Command::new(i).arg(&f).output().unwrap();
            let out = String::from_utf8_lossy(&o.stdout);
            assert!(
                o.status.success() && out.contains("64"),
                "nvddsinfo {n}: {out}"
            );
        }
    }
}
