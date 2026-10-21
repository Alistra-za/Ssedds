//! The image of a derived job (DECISIONS Note CN11): the recipe maths (`recipe`) per texel on the set's albedo or
//! normal and its scalars. The scalars (rough, metal, AO, `_p` R) and the `_curves`/cavity pair are built by
//! `assemble_channels`, each from only the files its four channels name, so a colour map is never decoded twice.
use super::recipe::{self, DiffuseFlavour};
use super::{DeriveJob, DeriveKind};
use crate::image::{Rgba32F, SourceInfo};
use crate::load::{load_image, LoadError};
use crate::pack::assemble::{assemble_channels, check_sizes, constant, Assembled, PackError};
use crate::pack::{ChannelSource, PackSource};
use std::path::PathBuf;

/// The image whose R, G, B, A are `channels`, read from the files they name; a `w`×`h` image of constants when they
/// name none (`assemble_channels` sizes its canvas from its sources).
fn channels_image(
    channels: [ChannelSource; 4],
    gray: crate::config::GraySource,
    (w, h): (u32, u32),
) -> Result<Assembled, PackError> {
    let mut sources: Vec<PackSource> = vec![];
    for c in &channels {
        if let ChannelSource::Source { path, role, .. } = c {
            if !sources.iter().any(|s| s.path == *path) {
                sources.push(PackSource {
                    path: path.clone(),
                    role: *role,
                    stamp: None,
                });
            }
        }
    }
    if sources.is_empty() {
        return Ok(Assembled {
            image: Rgba32F::new(
                w,
                h,
                channels.each_ref().map(|c| constant(c).unwrap_or(0.0)),
            ),
            info: SourceInfo {
                bit_depth: 8,
                channels: 4,
                has_alpha: false,
                is_float: false,
            },
            non_gray: vec![],
        });
    }
    assemble_channels(&channels, &sources, gray)
}

/// Builds the image of `job`. Every source shares one size (`PackError::SizeMismatch`, as packing; TA22).
pub fn assemble_derived(job: &DeriveJob) -> Result<Assembled, PackError> {
    assemble_derived_rough(job).map(|(a, _)| a)
}

/// [`assemble_derived`] and the plain mean of the roughness slot (TA16, `DERIVE-NIF-SUGGEST`), read from the
/// scalars it builds anyway, so planning decodes nothing twice.
pub fn assemble_derived_rough(job: &DeriveJob) -> Result<(Assembled, f64), PackError> {
    use DeriveKind::*;
    let paths: Vec<PathBuf> = job.sources.iter().map(|s| s.path.clone()).collect();
    // A malformed job (the planner never builds one) fails rather than panicking a worker.
    let first = paths.first().cloned().ok_or_else(|| LoadError::Decode {
        path: PathBuf::new(),
        msg: format!("a {} job has no sources", job.kind.id()),
    })?;
    let (w, h) = check_sizes(&paths)?;
    let colour = |slot: &Option<PathBuf>, what: &str| {
        let p = slot.as_ref().ok_or_else(|| LoadError::Decode {
            path: first.clone(),
            msg: format!("a {} job has no {what}", job.kind.id()),
        })?;
        let (img, info) = load_image(p)?;
        // `check_sizes` read the headers; a file replaced since then is caught here.
        if (img.width, img.height) != (w, h) {
            return Err(PackError::SizeMismatch(vec![
                (first.clone(), w, h),
                (p.clone(), img.width, img.height),
            ]));
        }
        Ok((img, info))
    };
    let k = job.kind;
    let albedo = matches!(k, VanillaDiffuse | CmDiffuse | GrassStack)
        .then(|| colour(&job.albedo, "albedo"))
        .transpose()?;
    let normal = matches!(k, LegacyN | GrassStack)
        .then(|| colour(&job.normal, "normal"))
        .transpose()?;
    let scalars = channels_image(
        [
            job.rough.clone(),
            job.metal.clone(),
            job.ao.clone(),
            job.height.clone().unwrap_or(ChannelSource::Opaque),
        ],
        job.gray,
        (w, h),
    )?;
    let detail = (job.curves.is_some() || job.cavity.is_some())
        .then(|| {
            channels_image(
                [
                    job.curves
                        .clone()
                        .unwrap_or(ChannelSource::Default { value: 0.5 }),
                    job.cavity
                        .clone()
                        .unwrap_or(ChannelSource::Default { value: 1.0 }),
                    ChannelSource::Opaque,
                    ChannelSource::Opaque,
                ],
                job.gray,
                (w, h),
            )
        })
        .transpose()?;

    let (p, land, has_p) = (&job.params, job.landscape, job.height.is_some());
    // Loaded above whenever the kind reads it.
    let albedo_px = |i: usize| albedo.as_ref().map_or([1.0; 4], |a| a.0.px[i]);
    let normal_px = |i: usize| normal.as_ref().map_or([0.5, 0.5, 1.0, 1.0], |n| n.0.px[i]);
    let diffuse = |i: usize, keep_metal: bool| {
        let (a, s) = (albedo_px(i), scalars.image.px[i]);
        let d = detail.as_ref().map(|d| d.image.px[i]);
        let flavour = if land {
            DiffuseFlavour::Landscape
        } else if keep_metal {
            DiffuseFlavour::CmKeepMetal
        } else {
            DiffuseFlavour::Vanilla
        };
        recipe::diffuse(
            [a[0], a[1], a[2]],
            s[2],
            s[1],
            d.filter(|_| job.curves.is_some()).map(|d| d[0]),
            d.filter(|_| job.cavity.is_some()).map(|d| d[1]),
            p,
            flavour,
        )
    };
    let spec = |i: usize| {
        let s = scalars.image.px[i];
        recipe::spec_alpha(s[0], s[2], s[1], p, land)
    };

    let n = (w as usize) * (h as usize);
    let out_h = if k == GrassStack { 2 * h } else { h };
    let mut image = Rgba32F::new(w, out_h, [0.0, 0.0, 0.0, 1.0]);
    for i in 0..n {
        let s = scalars.image.px[i];
        let v = match k {
            VanillaDiffuse | CmDiffuse => {
                let c = diffuse(i, k == CmDiffuse);
                // A landscape diffuse alpha is the `_p` R (parallax), opaque without one (TA5).
                let a = match (land, has_p) {
                    (false, _) => albedo_px(i)[3],
                    (true, true) => s[3],
                    (true, false) => 1.0,
                };
                [c[0], c[1], c[2], a]
            }
            LegacyN => {
                let q = normal_px(i);
                [q[0], q[1], q[2], spec(i)]
            }
            CmM => recipe::cm_m(s[0], s[1], has_p.then_some(s[3]), p, land),
            EnvM => {
                let e = recipe::env_m(s[0], s[1]);
                [e, e, e, 1.0]
            }
            GrassStack => {
                // With Keep metal brightness off, CM reuses the vanilla diffuse (spec Goal 5).
                let c = diffuse(i, p.keep_metal_brightness);
                let q = normal_px(i);
                image.px[n + i] = [q[0], q[1], q[2], spec(i)];
                [c[0], c[1], c[2], albedo_px(i)[3]]
            }
        };
        image.px[i] = v;
    }

    let infos = [
        Some(scalars.info),
        detail.as_ref().map(|d| d.info),
        albedo.as_ref().map(|a| a.1),
        normal.as_ref().map(|n| n.1),
    ];
    let infos = infos.iter().flatten();
    let info = SourceInfo {
        bit_depth: infos.clone().map(|i| i.bit_depth).max().unwrap_or(8),
        channels: 4,
        has_alpha: !(k == EnvM || (matches!(k, VanillaDiffuse | CmDiffuse) && land && !has_p)),
        // The RGB is in the colour map's coding; the scalars are [0, 1] data whatever their file type, so a float
        // `_p` or roughness must not pick the linear source transfer (review I1). `false` for `cm_m`/`env_m`.
        // shortcut: a float (EXR) colour map is taken as stored values; the recipe was calibrated on coded 8-bit
        // maps, so convert or refuse it in planning if such sets turn up.
        is_float: albedo
            .as_ref()
            .or(normal.as_ref())
            .is_some_and(|c| c.1.is_float),
    };
    let mean_rough = scalars
        .image
        .px
        .iter()
        .map(|q| f64::from(q[0]))
        .sum::<f64>()
        / n.max(1) as f64;
    let mut non_gray = scalars.non_gray;
    non_gray.extend(detail.into_iter().flat_map(|d| d.non_gray));
    Ok((
        Assembled {
            image,
            info,
            non_gray,
        },
        mean_rough,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::derive::recipe::LegacyParams;
    use crate::derive::DeriveSource;
    use image::{ImageBuffer, Rgba};

    /// A set with no scalar file (all slot defaults) still gets an image of the colour map's size.
    #[test]
    fn a_job_with_no_scalar_file_uses_the_defaults_at_the_colour_size() {
        let d = tempfile::tempdir().unwrap();
        let n = d.path().join("x_n.png");
        ImageBuffer::from_pixel(8, 4, Rgba([128u8, 128, 255, 255]))
            .save(&n)
            .unwrap();
        let job = DeriveJob {
            kind: DeriveKind::LegacyN,
            landscape: false,
            albedo: None,
            normal: Some(n.clone()),
            rough: ChannelSource::Default { value: 0.5 },
            metal: ChannelSource::Default { value: 0.0 },
            ao: ChannelSource::Default { value: 1.0 },
            height: None,
            curves: None,
            cavity: None,
            gray: crate::config::GraySource::R,
            params: LegacyParams::default(),
            sources: vec![DeriveSource {
                path: n,
                role: "normal".into(),
                stamp: None,
            }],
        };
        let a = assemble_derived(&job).unwrap();
        assert_eq!((a.image.width, a.image.height), (8, 4));
        // 0.66 · (1 − 0.5) · 1 · 1 = 0.33.
        assert!(a.image.px.iter().all(|q| (q[3] - 0.33).abs() < 1e-5));
        assert!(a.info.has_alpha);

        let env = DeriveJob {
            kind: DeriveKind::EnvM,
            metal: ChannelSource::Default { value: 1.0 },
            ..job
        };
        let a = assemble_derived(&env).unwrap();
        assert!(a.image.px.iter().all(|q| (q[0] - 0.5).abs() < 1e-6));
        assert!(!a.info.has_alpha);
    }

    /// A `VanillaDiffuse` of an 8×8 PNG albedo (alpha 77) with default scalars.
    fn diffuse_job(dir: &std::path::Path) -> DeriveJob {
        let a = dir.join("x.png");
        ImageBuffer::from_pixel(8, 8, Rgba([200u8, 200, 200, 77]))
            .save(&a)
            .unwrap();
        DeriveJob {
            kind: DeriveKind::VanillaDiffuse,
            landscape: false,
            albedo: Some(a.clone()),
            normal: None,
            rough: ChannelSource::Default { value: 0.5 },
            metal: ChannelSource::Default { value: 0.0 },
            ao: ChannelSource::Default { value: 1.0 },
            height: None,
            curves: None,
            cavity: None,
            gray: crate::config::GraySource::R,
            params: LegacyParams::default(),
            sources: vec![DeriveSource {
                path: a,
                role: "albedo".into(),
                stamp: None,
            }],
        }
    }

    /// TA5: a landscape diffuse's alpha is the `_p` R, opaque (and alpha-free) without a `_p`; an object keeps the
    /// albedo's alpha.
    #[test]
    fn a_landscape_diffuse_alpha_is_the_p_r_or_opaque() {
        let d = tempfile::tempdir().unwrap();
        let object = diffuse_job(d.path());
        let a = assemble_derived(&object).unwrap();
        assert!(a
            .image
            .px
            .iter()
            .all(|q| (q[3] - 77.0 / 255.0).abs() < 1e-6));
        assert!(a.info.has_alpha);
        let bare = DeriveJob {
            landscape: true,
            ..object.clone()
        };
        let a = assemble_derived(&bare).unwrap();
        assert!(a.image.px.iter().all(|q| q[3] == 1.0));
        assert!(!a.info.has_alpha);
        let with_p = DeriveJob {
            height: Some(ChannelSource::Default { value: 0.4 }),
            ..bare
        };
        let a = assemble_derived(&with_p).unwrap();
        assert!(a.image.px.iter().all(|q| (q[3] - 0.4).abs() < 1e-6));
        assert!(a.info.has_alpha);
    }

    /// Review M1: a malformed job (no sources) fails, it does not panic.
    #[test]
    fn a_job_without_sources_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let job = DeriveJob {
            albedo: None,
            sources: vec![],
            ..diffuse_job(d.path())
        };
        let e = assemble_derived(&job).err().expect("an error");
        assert!(e.to_string().contains("no sources"), "{e}");
    }

    /// Review I1: a float scalar map (EXR roughness) leaves the image in the colour map's coding (`is_float` picks
    /// the source transfer), for the diffuse and the grass stack.
    #[test]
    fn a_float_scalar_map_does_not_make_the_image_float() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("x_rough.exr");
        image::DynamicImage::ImageRgb32F(ImageBuffer::from_pixel(8, 8, image::Rgb([0.5f32; 3])))
            .save(&r)
            .unwrap();
        let n = d.path().join("x_n.png");
        ImageBuffer::from_pixel(8, 8, Rgba([128u8, 128, 255, 255]))
            .save(&n)
            .unwrap();
        let mut job = diffuse_job(d.path());
        job.rough = ChannelSource::Source {
            path: r.clone(),
            role: crate::config::RawMap::Roughness,
            invert: false,
            channel: None,
        };
        job.sources.push(DeriveSource {
            path: r,
            role: "rough".into(),
            stamp: None,
        });
        assert!(!assemble_derived(&job).unwrap().info.is_float);
        job.kind = DeriveKind::GrassStack;
        job.normal = Some(n.clone());
        job.sources.insert(
            1,
            DeriveSource {
                path: n,
                role: "normal".into(),
                stamp: None,
            },
        );
        assert!(!assemble_derived(&job).unwrap().info.is_float);
    }
}
