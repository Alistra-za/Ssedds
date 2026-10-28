//! A cube job's stages up to quantise (cubemap support v1, DECISIONS DS10, DS11): load, decode to linear light,
//! cut the six faces, size them, colour filters, cone blur, then one mip chain built in direction space across faces.
use super::{filter, import_dir, layout, sample, Cube, CubeJob};
use crate::api::CancelToken;
use crate::config::{ColorContent, CubeLayout, Profile, ResizeMode};
use crate::events::Stage;
use crate::image::Rgba32F;
use crate::load::load_image_as;
use crate::ops::{
    game_gamma_to_linear, linear_to_game_gamma, preprocess_with, SourceTransfer, Work,
};
use crate::pipeline::{FileError, FileTracker, Keep, Prepared};
use crate::policy::ResolvedJob;
use crate::quantize::to_encoder_input;

/// The cube's `Prepared`: `inputs` and `dims` face-major (+X..-Z, each face's whole chain) and `faces = 6` for an
/// encode or a measure; the +X face alone (`faces = 1`) for a preview.
pub(crate) fn prepare(
    job: &ResolvedJob,
    cj: &CubeJob,
    keep: Keep,
    cancel: &CancelToken,
    tracker: &FileTracker,
) -> Result<Prepared, FileError> {
    let p = &job.profile;
    let stop = || {
        if cancel.is_cancelled() {
            Err(FileError::Cancelled)
        } else {
            Ok(())
        }
    };
    let failed = |st: Stage| move |e: String| FileError::Failed(st, e);
    let files = match cj.layout {
        CubeLayout::Faces => 6,
        _ => 1,
    };
    if cj.sources.len() != files {
        return Err(failed(Stage::Load)(format!(
            "a {:?} cube reads {files} source file(s), the job has {}",
            cj.layout,
            cj.sources.len()
        )));
    }
    // The cube is sized by `cubemap.face_size`, never by `resize`. DECISIONS DS11: `srgb` content on a cube is the
    // game's 1.6 gamma, decoded and re-encoded here, so preprocess and quantise see stored values (`content linear`).
    let mut q = Profile {
        resize: ResizeMode::None,
        ..p.clone()
    };
    q.color.content = ColorContent::Linear;
    let gamma = p.color.content == ColorContent::Srgb;
    let mut works: Vec<Work> = Vec::with_capacity(files);
    for s in &cj.sources {
        stop()?;
        tracker.stage.set(Stage::Load);
        let (img, info) = load_image_as(&s.path, p.alpha.exr_straight)
            .map_err(|e| failed(Stage::Load)(format!("{}: {e}", s.path.display())))?;
        tracker.stage.set(Stage::Preprocess);
        let mut w = preprocess_with(img, &q, SourceTransfer::of(&info));
        // A float source is linear already.
        if gamma && !info.is_float {
            for px in &mut w.img.px {
                for c in &mut px[..3] {
                    *c = game_gamma_to_linear(*c);
                }
            }
        }
        works.push(w);
    }
    stop()?;
    let src = (works[0].img.width, works[0].img.height);
    let n = layout::face_size(p, cj.layout, src);
    let direct: Option<fn(&Rgba32F, u32) -> Cube> = match cj.layout {
        CubeLayout::Equirect => Some(import_dir::equirect),
        CubeLayout::MirrorBall => Some(import_dir::mirror_ball),
        CubeLayout::Angular => Some(import_dir::angular),
        CubeLayout::Photo => {
            Some(|img, n| import_dir::equirect(&import_dir::photo_panorama(img), n))
        }
        _ => None,
    };
    let faces: [Rgba32F; 6] = if let Some(import) = direct {
        // Any loaded size reads as a panorama (DS14, DS15), sampled straight at the face size, so the resize below
        // is a no-op.
        let Cube { faces, .. } = import(&works[0].img, n);
        let mut k = 0;
        faces.map(|f| {
            k += 1;
            layout::orient(f, k - 1, &p.cubemap)
        })
    } else if cj.layout == CubeLayout::Faces {
        if works
            .iter()
            .any(|w| (w.img.width, w.img.height) != (src.0, src.0))
        {
            return Err(failed(Stage::Load)(
                "the six faces are not equal squares; re-run".into(),
            ));
        }
        std::array::from_fn(|k| {
            let img = std::mem::replace(&mut works[k].img, Rgba32F::new(0, 0, [0.0; 4]));
            layout::orient(img, k, &p.cubemap)
        })
    } else {
        layout::import_raw(
            &works[0].img,
            cj.layout,
            &p.cubemap,
            p.cubemap.layout.is_none(),
        )
        .map_err(failed(Stage::Preprocess))?
    };
    // Only `alpha_present` and `domain` are read at quantise.
    let mut meta = works.swap_remove(0);
    drop(works);
    meta.img = Rgba32F::new(0, 0, [0.0; 4]);
    let cell = faces[0].width;
    let mut cube = Cube { size: cell, faces };
    if n < cell {
        for f in &mut cube.faces {
            *f = crate::mips::area_resample(f, (n, n), false);
        }
        cube.size = n;
    } else if n > cell {
        cube = sample::resample(&cube, n, |d| d);
    }
    // Run order (DECISIONS DS14): import, face rotate/flip, yaw, mirror, colour filters, ground fill, blur.
    if p.cubemap.yaw != 0.0 {
        cube = filter::yaw(&cube, p.cubemap.yaw);
    }
    if p.cubemap.mirror {
        cube = filter::mirror(&cube);
    }
    filter::colour(&mut cube, &p.cubemap);
    if let Some(g) = &p.cubemap.ground_fill {
        filter::ground_fill(&mut cube, g, p.cubemap.horizon);
    }
    stop()?;

    tracker.stage.set(Stage::Mipmaps);
    // DS24: no copy at blur 0, and the pre-blur cube is freed before the mip chain.
    let blur = p.cubemap.blur.unwrap_or(0.0);
    let cube = if blur > 0.0 {
        let blurred = sample::blur(&cube, blur);
        drop(cube);
        blurred
    } else {
        cube
    };
    let levels = crate::mips::level_count(n, n, &p.mips.levels) as usize;
    let mut chain = sample::mip_chain(cube, levels);
    if p.drop_top_mip && chain.len() > 1 {
        chain.remove(0);
    }
    if gamma {
        for px in chain
            .iter_mut()
            .flat_map(|c| c.faces.iter_mut())
            .flat_map(|f| f.px.iter_mut())
        {
            for c in &mut px[..3] {
                *c = linear_to_game_gamma(*c);
            }
        }
    }
    stop()?;
    let one: Vec<(u32, u32)> = chain.iter().map(|c| (c.size, c.size)).collect();
    let mut prep = Prepared {
        first_level: 0,
        dims: vec![],
        chain: vec![],
        domain: meta.domain,
        inputs: vec![],
        base_coverage: None,
        source_peak: None,
        detail_ratio: None,
        faces: 6,
    };
    match keep {
        Keep::Encode | Keep::Measure => {
            prep.dims = (0..6).flat_map(|_| one.iter().copied()).collect();
            if keep == Keep::Encode {
                prep.inputs = (0..6)
                    .flat_map(|f| chain.iter().map(move |c| (f, c)))
                    .map(|(f, c)| to_encoder_input(&c.faces[f], &meta, &q))
                    .collect();
            }
        }
        Keep::Preview { max_size } | Keep::SourceOnly { max_size } => {
            // DECISIONS DT2: a preview encodes all six faces (face-major, each chain from first_level); the source side shows +X.
            prep.faces = if matches!(keep, Keep::Preview { .. }) {
                6
            } else {
                1
            };
            prep.first_level = crate::session::first_level_within(&one, max_size);
            prep.dims = one;
            let mut all: Vec<Vec<Rgba32F>> = vec![vec![]; 6];
            for c in chain.into_iter().skip(prep.first_level) {
                for (f, face) in c.faces.into_iter().enumerate() {
                    all[f].push(face);
                }
            }
            if matches!(keep, Keep::Preview { .. }) {
                prep.inputs = all
                    .iter()
                    .flatten()
                    .map(|l| to_encoder_input(l, &meta, &q))
                    .collect();
            }
            prep.chain = all.swap_remove(0);
        }
    }
    Ok(prep)
}
