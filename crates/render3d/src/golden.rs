//! Golden-capture comparison: diff metrics, per-format tolerances, measured noise floor and fixture index.
//! Pure Rust, no GPU. Fixtures hold game data and never enter the repo.
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Diff {
    pub max_abs: f32,
    pub mean_abs: f32,
    /// Share of masked pixels with any channel over the limit.
    pub over: f32,
    pub pixels: u32,
}

/// Shared walk: `limit(channel, b)` gives the allowed |a-b| for a channel (b = capture value).
fn walk(a: &[f32], b: &[f32], mask: &[bool], limit: impl Fn(usize, f32) -> f32) -> Diff {
    assert!(
        a.len() == 4 * mask.len() && b.len() == a.len(),
        "golden diff: length mismatch"
    );
    let (mut max_abs, mut sum, mut chans, mut pixels, mut over) = (0f32, 0f64, 0u64, 0u32, 0u32);
    for p in (0..mask.len()).filter(|&p| mask[p]) {
        pixels += 1;
        let mut bad = false;
        for i in 4 * p..4 * p + 4 {
            if !a[i].is_finite() || !b[i].is_finite() {
                // fail closed unless the stored bits are identical
                if a[i].to_bits() != b[i].to_bits() {
                    max_abs = f32::INFINITY;
                    bad = true;
                }
                continue;
            }
            let d = (a[i] - b[i]).abs();
            max_abs = max_abs.max(d);
            sum += d as f64;
            chans += 1;
            bad |= d > limit(i % 4, b[i]);
        }
        over += bad as u32;
    }
    if pixels == 0 {
        return Diff {
            max_abs: 0.0,
            mean_abs: 0.0,
            over: 0.0,
            pixels: 0,
        };
    }
    Diff {
        max_abs,
        mean_abs: if chans == 0 {
            0.0
        } else {
            (sum / chans as f64) as f32
        },
        over: over as f32 / pixels as f32,
        pixels,
    }
}

/// `a`, `b`: RGBA f32 per pixel (len = 4 * mask.len()); `over` counts masked pixels with any channel over `threshold`.
pub fn diff(a: &[f32], b: &[f32], mask: &[bool], threshold: f32) -> Diff {
    walk(a, b, mask, |_, _| threshold)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tol {
    Abs(f32),
    Rel([f32; 4]),
}

/// Per-format tolerance (requirements section 7 item 9). Relative limits apply to |b|.
pub fn format_tol(format: &str) -> Option<Tol> {
    Some(match format {
        "R10G10B10A2_UNORM" => Tol::Abs(1.0 / 1023.0),
        "R11G11B10_FLOAT" => Tol::Rel([1.0 / 64.0, 1.0 / 64.0, 1.0 / 32.0, 0.0]),
        "R16G16B16A16_FLOAT" | "R16G16_FLOAT" | "R16_FLOAT" => Tol::Rel([1e-3; 4]),
        "R16_UNORM" => Tol::Abs(1.0 / 65535.0),
        "R8G8_UNORM" | "R8G8B8A8_UNORM" => Tol::Abs(1.0 / 255.0),
        _ => return None,
    })
}

/// Floor so a zero capture value compares absolutely (below half-float resolution).
const REL_FLOOR: f32 = 1e-6;

/// D-P5: a channel passes when |a-b| <= max(format limit for b, noise_max).
pub fn diff_tol(a: &[f32], b: &[f32], mask: &[bool], tol: Tol, noise_max: f32) -> Diff {
    walk(a, b, mask, |c, bv| {
        let f = match tol {
            Tol::Abs(t) => t,
            Tol::Rel(r) => (r[c] * bv.abs()).max(REL_FLOOR),
        };
        f.max(noise_max)
    })
}

/// Measured noise floor (golden_noise.py `tolerances` block): set name -> file -> (p99, max).
pub struct Noise {
    sets: HashMap<String, HashMap<String, (f32, f32)>>,
}

fn read_json(path: &Path) -> Result<Value, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&s).map_err(|e| format!("{}: {e}", path.display()))
}

/// golden_noise.py tag_of on a subject folder name: drop `ev<digits>_`, then a trailing `_<digits>`.
fn tag_of(dir_name: &str) -> &str {
    let name = match dir_name.strip_prefix("ev").and_then(|r| r.split_once('_')) {
        Some((d, rest)) if !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()) => rest,
        _ => dir_name,
    };
    match name.rsplit_once('_') {
        Some((head, tail))
            if !head.is_empty() && !tail.is_empty() && tail.bytes().all(|c| c.is_ascii_digit()) =>
        {
            head
        }
        _ => name,
    }
}

impl Noise {
    pub fn load(path: &Path) -> Result<Noise, String> {
        let err = |m: &str| format!("{}: {m}", path.display());
        let v = read_json(path)?;
        let tols = v
            .get("tolerances")
            .and_then(Value::as_object)
            .ok_or_else(|| err("no tolerances block"))?;
        let mut sets = HashMap::new();
        for (set, body) in tols {
            let files = body
                .get("files")
                .and_then(Value::as_object)
                .ok_or_else(|| err("tolerance set without files"))?;
            let mut m = HashMap::new();
            for (f, l) in files {
                let g = |k: &str| {
                    l.get(k)
                        .and_then(Value::as_f64)
                        .map(|x| x as f32)
                        .filter(|x| x.is_finite() && *x >= 0.0)
                        .ok_or_else(|| err(&format!("bad {k} for {set}/{f}")))
                };
                m.insert(f.clone(), (g("p99")?, g("max")?));
            }
            sets.insert(set.clone(), m);
        }
        Ok(Noise { sets })
    }

    /// A set named after the subject's tag wins, else "static". Returns (p99, max) for `file`.
    pub fn limit(&self, subject_dir_name: &str, file: &str) -> Option<(f32, f32)> {
        let set = self
            .sets
            .get(tag_of(subject_dir_name))
            .or_else(|| self.sets.get("static"))?;
        set.get(file).copied()
    }
}

/// One subject folder of the golden root.
pub struct Subject {
    pub dir: PathBuf,
    pub capture: String,
    pub profile: String,
    pub event: u32,
    pub shape: String,
}

fn subdirs(p: &Path) -> Result<Vec<PathBuf>, String> {
    let e = |e: std::io::Error| format!("{}: {e}", p.display());
    let mut out = Vec::new();
    for ent in std::fs::read_dir(p).map_err(e)? {
        let ent = ent.map_err(e)?;
        if ent.file_type().map_err(e)?.is_dir() {
            out.push(ent.path());
        }
    }
    Ok(out)
}

/// Every subject under `root` whose capture folder's manifest.json "profile" is in `profiles`; sorted by (capture, event).
pub fn subjects(root: &Path, profiles: &[&str]) -> Result<Vec<Subject>, String> {
    let mut out = Vec::new();
    for cap in subdirs(root)? {
        let capture = cap.file_name().unwrap().to_string_lossy().into_owned();
        let mpath = cap.join("manifest.json");
        let profile = read_json(&mpath)?
            .get("profile")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("{}: no profile", mpath.display()))?
            .to_string();
        if !profiles.contains(&profile.as_str()) {
            continue;
        }
        for dir in subdirs(&cap)? {
            let name = dir.file_name().unwrap().to_string_lossy().into_owned();
            let Some((ev, shape)) = name.strip_prefix("ev").and_then(|r| r.split_once('_')) else {
                continue;
            };
            let Ok(event) = ev.parse::<u32>() else {
                continue;
            };
            out.push(Subject {
                dir,
                capture: capture.clone(),
                profile: profile.clone(),
                event,
                shape: shape.to_string(),
            });
        }
    }
    out.sort_by(|a, b| (&a.capture, a.event).cmp(&(&b.capture, b.event)));
    Ok(out)
}

/// mrt.json essentials: crop [x,y,w,h] and each RT's index and format (files are `mrt<index>.f32`).
pub struct MrtInfo {
    pub crop: [u32; 4],
    pub rts: Vec<(u32, String)>,
}

pub fn read_mrt_info(subject: &Path) -> Result<MrtInfo, String> {
    let p = subject.join("mrt.json");
    let err = |m: &str| format!("{}: {m}", p.display());
    let v = read_json(&p)?;
    let c = v
        .get("crop")
        .and_then(Value::as_array)
        .filter(|c| c.len() == 4)
        .ok_or_else(|| err("bad crop"))?;
    let mut crop = [0u32; 4];
    for (o, x) in crop.iter_mut().zip(c) {
        *o = x
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| err("bad crop"))?;
    }
    let mut rts = Vec::new();
    for r in v
        .get("rts")
        .and_then(Value::as_array)
        .ok_or_else(|| err("no rts"))?
    {
        let i = r
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok());
        let f = r.get("format").and_then(Value::as_str);
        rts.push((
            i.ok_or_else(|| err("bad rt index"))?,
            f.ok_or_else(|| err("bad rt format"))?.to_string(),
        ));
    }
    Ok(MrtInfo { crop, rts })
}

fn read_bytes(path: &Path, expected: usize) -> Result<Vec<u8>, String> {
    let b = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if b.len() != expected {
        return Err(format!(
            "{}: {} bytes, expected {expected}",
            path.display(),
            b.len()
        ));
    }
    Ok(b)
}

/// Little-endian f32 file; `expected_len` is the number of floats.
pub fn read_f32(path: &Path, expected_len: usize) -> Result<Vec<f32>, String> {
    let n = expected_len
        .checked_mul(4)
        .ok_or_else(|| format!("{}: length overflow", path.display()))?;
    let b = read_bytes(path, n)?;
    Ok(b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect())
}

/// uint8 per pixel, != 0.
pub fn read_mask(path: &Path, expected_len: usize) -> Result<Vec<bool>, String> {
    Ok(read_bytes(path, expected_len)?
        .into_iter()
        .map(|b| b != 0)
        .collect())
}
