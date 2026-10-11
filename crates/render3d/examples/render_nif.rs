//! Renders one NIF with the installed CS to a PNG (smoke tool).
//!
//! `cargo run -p render3d --release --example render_nif -- <Shaders dir> <nif> <out.png> [mods dir] [size]`
//!
//! Textures are resolved as loose files `<mods dir>\<any mod>\<textures/…>`; anything not found uses defaults.
use render3d::scene::Scene;
use std::path::PathBuf;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        return Err("usage: render_nif <Shaders dir> <nif> <out.png> [mods dir] [size]".into());
    }
    let shaders = PathBuf::from(&a[0]);
    let nif = std::fs::read(&a[1]).map_err(|e| format!("{}: {e}", a[1]))?;
    let mods: Vec<PathBuf> = a
        .get(3)
        .and_then(|d| std::fs::read_dir(d).ok())
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let size: u32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(512);
    let fp = render3d::locate::fingerprint(&shaders, None).map_err(|e| e.to_string())?;
    let cache = std::env::temp_dir().join("sdc_render3d_cache");
    let mut r = render3d::Renderer::new(fp, cache)?;
    println!("adapter: {} (warp {})", r.gpu().adapter, r.gpu().warp);
    let resolve = |p: &str| -> Option<Vec<u8>> {
        let rel = p.replace('/', "\\");
        mods.iter().find_map(|m| std::fs::read(m.join(&rel)).ok())
    };
    let t = std::time::Instant::now();
    let rep = r.load(&nif, &resolve);
    println!("load {} ms: {rep:#?}", t.elapsed().as_millis());
    println!("bounds {:?}", r.bounds());
    let mut scene = Scene::neutral();
    // Debug overrides: SDC_AMBIENT=<grey>, SDC_LIGHT=<scale>.
    if let Some(g) = std::env::var("SDC_AMBIENT")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        scene.ambient6 = [[g; 3]; 6];
    }
    if let Some(k) = std::env::var("SDC_LIGHT")
        .ok()
        .and_then(|v| v.parse::<f32>().ok())
    {
        scene.dir_light.color = scene.dir_light.color.map(|c| c * k);
    }
    r.render(&scene, size, size)?; // warm-up
    let t = std::time::Instant::now();
    let px = r.render(&scene, size, size)?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    let lit = px
        .chunks(4)
        .filter(|p| p[0] as u32 + p[1] as u32 + p[2] as u32 > 30)
        .count();
    println!(
        "render {ms:.2} ms, lit {:.2}% ({lit} px)",
        100.0 * lit as f64 / (size * size) as f64
    );
    image::save_buffer(&a[2], &px, size, size, image::ColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    println!("wrote {}", a[2]);
    // Display aid only: the frame is linear working space (Linear Lighting on) with P0's identity readback;
    // also write a linear→sRGB-encoded copy so textures can be judged by eye.
    let enc: Vec<u8> = px
        .chunks(4)
        .flat_map(|p| {
            let e = |v: u8| {
                let l = v as f32 / 255.0;
                let s = if l <= 0.003_130_8 {
                    l * 12.92
                } else {
                    1.055 * l.powf(1.0 / 2.4) - 0.055
                };
                (s * 255.0).round() as u8
            };
            [e(p[0]), e(p[1]), e(p[2]), p[3]]
        })
        .collect();
    let out2 = a[2].replace(".png", "_srgb.png");
    image::save_buffer(&out2, &enc, size, size, image::ColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    println!("wrote {out2}");
    Ok(())
}
