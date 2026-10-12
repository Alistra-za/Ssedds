//! Stub scene: camera, one directional light and a six-colour ambient, written into CS's cbuffers by name.
//!
//! Every value the engine would upload and P0 does not model comes from the forward hair draw of the owner's
//! capture (`tools/rdc-out/p04/Skyrim_AE_1.6.1170_frame11178/state_6544.json`, Linear Lighting on) so a
//! zero-initialised cbuffer never zeroes a multiplier (directional/ambient light, colour clamp …).
use crate::cbuffer::{CbWriter, Field};
use crate::cs_settings::write_feature_data;

/// Row-major 3×4 (`row r = [m_r0, m_r1, m_r2, t_r]`).
pub type Mat3x4 = [[f32; 4]; 3];
/// Row-major 4×4, used as `clip = M · v` (HLSL `mul(M, v)` on a `row_major` matrix).
pub type Mat4 = [[f32; 4]; 4];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Camera {
    /// The orbit at [`AUTO_YAW`], [`AUTO_PITCH`] and `radius × AUTO_DISTANCE` around the bounds centre: in front
    /// of the model (Skyrim actors face +Y; the world is Z-up), a little above, looking at the centre.
    Auto,
    /// Orbit around `target`: yaw about +Z (0 = camera on the −Y side), pitch above the XY plane, both in
    /// radians; `distance` in NIF units.
    Orbit {
        target: [f32; 3],
        yaw: f32,
        pitch: f32,
        distance: f32,
    },
}

/// Yaw of [`Camera::Auto`]: the camera on the +Y side, facing the front of an actor.
pub const AUTO_YAW: f32 = std::f32::consts::PI;
/// Pitch of [`Camera::Auto`]: `atan(0.25)`, slightly above.
pub const AUTO_PITCH: f32 = 0.244_978_66;
/// Distance of [`Camera::Auto`] in bounding radii.
pub const AUTO_DISTANCE: f32 = 2.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirLight {
    /// World-space unit vector **towards** the light (`dot(N, DirLightDirection)` in Lighting.hlsl).
    pub direction: [f32; 3],
    /// Linear colour × intensity, as the engine uploads it (multiplied in-shader by
    /// `linearLightingSettings.directionalLightMult`).
    pub color: [f32; 3],
}

/// What is drawn behind the model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Background {
    /// A flat grey: a **linear** value. With `Renderer::set_encode_srgb(true)` it is encoded with the rest of the
    /// frame at readback; without it (gamma-space output) the renderer writes its sRGB encoding, so the same
    /// value looks the same either way. A perceptual (sRGB) grey from a UI slider goes through [`Background::grey`].
    Solid(f32),
    /// The bundled studio HDRI ([`crate::hdri`]) by view direction. It also replaces `Scene::ambient6`: the
    /// ambient light is taken from the same image ([`Scene::ambient`]).
    Hdri,
}

impl Background {
    /// A solid grey from an sRGB-perceptual value in 0..1 (0 = black, 1 = white).
    pub fn grey(srgb: f32) -> Background {
        Background::Solid(srgb_to_linear(srgb.clamp(0.0, 1.0)))
    }
}

/// The one point light of the preview (white, the vanilla `PointLightPosition` / `PointLightColor` slot 0).
/// Brightness 0 is off: nothing is written, so the frame is the same as without it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLight {
    /// Linear white scale, uploaded as `PointLightColor` (the shader multiplies by `pointLightMult`).
    pub brightness: f32,
    /// Falloff radius in NIF units (`PointLightPosition.w`): the light reaches 0 at this distance.
    pub radius: f32,
    /// World position (Z-up).
    pub position: [f32; 3],
}

impl PointLight {
    pub const OFF: PointLight = PointLight {
        brightness: 0.0,
        radius: 1.0,
        position: [0.0; 3],
    };

    /// A light placed relative to the loaded model's bounds `(centre, radius)`: up, in front of and to the right
    /// of the default camera (which sits on +Y looking towards −Y, so its right is −X), at `radius_scale`
    /// bounding radii of falloff.
    pub fn around(bounds: ([f32; 3], f32), brightness: f32, radius_scale: f32) -> PointLight {
        let (c, r) = bounds;
        PointLight {
            brightness,
            radius: radius_scale * r,
            position: [c[0] - 1.2 * r, c[1] + 1.6 * r, c[2] + 1.4 * r],
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub camera: Camera,
    pub background: Background,
    /// Multiplier on `dir_light.color` (1 = as is).
    pub sun_brightness: f32,
    pub point_light: PointLight,
    pub dir_light: DirLight,
    /// Ambient (DALC) colours seen from +X, −X, +Y, −Y, +Z, −Z, world space.
    pub ambient6: [[f32; 3]; 6],
}

fn norm(v: [f32; 3]) -> [f32; 3] {
    let l = dot(v, v).sqrt();
    if l > 0.0 {
        [v[0] / l, v[1] / l, v[2] / l]
    } else {
        v
    }
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

impl Scene {
    /// The six ambient colours the shader gets: `ambient6`, or the HDRI's when it is the background.
    /// `dir_light.color` with the sun brightness applied.
    pub fn sun_color(&self) -> [f32; 3] {
        self.dir_light.color.map(|c| c * self.sun_brightness)
    }

    pub fn ambient(&self) -> [[f32; 3]; 6] {
        match self.background {
            Background::Solid(_) => self.ambient6,
            Background::Hdri => crate::hdri::studio().ambient6,
        }
    }

    /// Interior-like neutral preview:
    /// - one directional light from the upper front-left of the auto camera (camera on +Y looking towards −Y, so its
    ///   left is +X),
    ///   warm white. Non-black on purpose: the captures only had the black `(−1,0,0)` placeholder (Q3);
    /// - ambient from six grey levels (04 §3.2): brighter from above (+Z 0.30), darker from below (−Z 0.12),
    ///   sides 0.20–0.23. Linear values (Linear Lighting on); the captured uniform interior ambient was
    ///   ~0.09/0.045/0.012 (warm, dim), so this is a little brighter and neutral for inspecting materials.
    pub fn neutral() -> Scene {
        Scene {
            camera: Camera::Auto,
            background: Background::Solid(0.0),
            sun_brightness: 1.0,
            point_light: PointLight::OFF,
            dir_light: DirLight {
                direction: norm([0.5, 0.8, 0.9]),
                color: [2.0, 1.94, 1.84],
            },
            ambient6: [
                [0.22; 3], // +X
                [0.20; 3], // −X
                [0.21; 3], // +Y
                [0.23; 3], // −Y
                [0.30; 3], // +Z
                [0.12; 3], // −Z
            ],
        }
    }
}

/// `PerGeometry.DirectionalAmbient` from six colours (04 §3.2, verified in answers.md Q2): row `ch` (colour
/// channel) = `[(c+X − c−X)/2, (c+Y − c−Y)/2, (c+Z − c−Z)/2, mean of the six]`, so
/// `mul(M, float4(n, 1))` returns c±axis for n = ±axis. World space, not rotated per object.
pub fn dalc(six: &[[f32; 3]; 6]) -> Mat3x4 {
    let mut m = [[0.0; 4]; 3];
    for (ch, row) in m.iter_mut().enumerate() {
        for axis in 0..3 {
            row[axis] = (six[2 * axis][ch] - six[2 * axis + 1][ch]) / 2.0;
        }
        row[3] = six.iter().map(|c| c[ch]).sum::<f32>() / 6.0;
    }
    m
}

/// `SharedData.AmbientSHR/G/B` from the same six colours: CS `DALCToSH` (src/Utils/SphericalHarmonics.cpp),
/// each colour weighted 4π/6 on the SH2 basis `(Y0, −Y1·y, Y1·z, −Y1·x)` (answers.md Q2; reproduces the
/// captured values to 3e-8, `tools/rdc-scripts/p04_q2_dalc.py`).
pub fn dalc_to_sh(six: &[[f32; 3]; 6]) -> [[f32; 4]; 3] {
    const Y0: f64 = 0.282_094_791_773_878_14;
    const Y1: f64 = 0.488_602_511_902_919_9;
    let w = 4.0 * std::f64::consts::PI / 6.0;
    let dirs: [[f64; 3]; 6] = [
        [1.0, 0.0, 0.0],
        [-1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, -1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
    ];
    let mut sh = [[0.0f64; 4]; 3];
    for (c, d) in six.iter().zip(dirs) {
        let b = [Y0, -Y1 * d[1], Y1 * d[2], -Y1 * d[0]];
        for ch in 0..3 {
            for k in 0..4 {
                sh[ch][k] += b[k] * c[ch] as f64 * w;
            }
        }
    }
    sh.map(|r| r.map(|v| v as f32))
}

/// IEC 61966-2-1 linear → sRGB.
pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// IEC 61966-2-1 sRGB → linear.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Camera state for one frame. The world is drawn **camera-relative** (the engine's convention:
/// Lighting.hlsl takes `viewDirection = −normalize(WorldPosition)`), so `view_proj` has no translation and
/// every World / bone pivot subtracts `eye`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub eye: [f32; 3],
    pub view: Mat4,
    pub proj: Mat4,
    pub view_proj: Mat4,
    pub near: f32,
    pub far: f32,
    pub width: u32,
    pub height: u32,
}

pub fn mul4(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut m = [[0.0; 4]; 4];
    for (r, row) in m.iter_mut().enumerate() {
        for (c, v) in row.iter_mut().enumerate() {
            *v = (0..4).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    m
}

/// General 4×4 inverse (cofactors); the zero matrix when singular.
pub fn inverse4(m: &Mat4) -> Mat4 {
    let a: Vec<f64> = m.iter().flatten().map(|&v| v as f64).collect();
    let mut inv = [0.0f64; 16];
    for (i, out) in inv.iter_mut().enumerate() {
        let (r, c) = (i / 4, i % 4);
        // cofactor C[c][r] (transpose = adjugate)
        let minor: Vec<f64> = (0..4)
            .filter(|&rr| rr != c)
            .flat_map(|rr| (0..4).filter(move |&cc| cc != r).map(move |cc| (rr, cc)))
            .map(|(rr, cc)| a[rr * 4 + cc])
            .collect();
        let det3 = minor[0] * (minor[4] * minor[8] - minor[5] * minor[7])
            - minor[1] * (minor[3] * minor[8] - minor[5] * minor[6])
            + minor[2] * (minor[3] * minor[7] - minor[4] * minor[6]);
        *out = if (r + c) % 2 == 0 { det3 } else { -det3 };
    }
    let det: f64 = (0..4).map(|k| a[k] * inv[k * 4]).sum();
    let mut out = [[0.0f32; 4]; 4];
    if det.abs() > 1e-30 {
        for i in 0..16 {
            out[i / 4][i % 4] = (inv[i] / det) as f32;
        }
    }
    out
}

impl View {
    /// `bounds` = (centre, radius) of the loaded model; `None` (nothing loaded) looks at the origin.
    pub fn new(camera: &Camera, bounds: Option<([f32; 3], f32)>, width: u32, height: u32) -> View {
        let (centre, radius) = bounds.unwrap_or(([0.0; 3], 1.0));
        let radius = radius.max(1e-3);
        let orbit = match *camera {
            Camera::Auto => (centre, AUTO_YAW, AUTO_PITCH, radius * AUTO_DISTANCE),
            Camera::Orbit {
                target,
                yaw,
                pitch,
                distance,
            } => (target, yaw, pitch, distance),
        };
        let (target, yaw, pitch, distance) = orbit;
        let d = [
            pitch.cos() * yaw.sin(),
            -pitch.cos() * yaw.cos(),
            pitch.sin(),
        ];
        let eye = [
            target[0] + d[0] * distance,
            target[1] + d[1] * distance,
            target[2] + d[2] * distance,
        ];
        let f = norm(sub(target, eye));
        let up = if f[2].abs() > 0.999 {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        // Same basis as the captured FrameBuffer::CameraView (state_6544): rows right = f × up, up, forward.
        let r = norm(cross(f, up));
        let u = cross(r, f);
        let view = [
            [r[0], r[1], r[2], 0.0],
            [u[0], u[1], u[2], 0.0],
            [f[0], f[1], f[2], 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let dist = dot(sub(eye, centre), sub(eye, centre)).sqrt();
        let near = (dist - 2.0 * radius).max(dist * 0.01).max(0.01);
        let far = (dist + 2.0 * radius).max(near * 2.0);
        // Left-handed D3D projection, depth 0 at near, 1 at far (captured CameraProj rows 2–3).
        let ys = 1.0 / (50f32.to_radians() / 2.0).tan();
        let xs = ys * height.max(1) as f32 / width.max(1) as f32;
        let q = far / (far - near);
        let proj = [
            [xs, 0.0, 0.0, 0.0],
            [0.0, ys, 0.0, 0.0],
            [0.0, 0.0, q, -q * near],
            [0.0, 0.0, 1.0, 0.0],
        ];
        View {
            eye,
            view,
            proj,
            view_proj: mul4(&proj, &view),
            near,
            far,
            width,
            height,
        }
    }

    /// `Mesh.transform` (row-major 3×4, nifly convention) → camera-relative World rows. HLSL declares
    /// `row_major float3x4 World` and computes `mul(World, float4(p, 1))`, i.e. row · p, so the rows go into
    /// c0..c2 unchanged; only the translation column is shifted by −eye.
    pub fn world(&self, model: &Mat3x4) -> Mat3x4 {
        let mut w = *model;
        for (r, row) in w.iter_mut().enumerate() {
            row[3] -= self.eye[r];
        }
        w
    }
}

/// Flattens rows for `CbWriter::set` (row_major: each row is one 16-byte register).
pub fn rows<const R: usize>(m: &[[f32; 4]; R]) -> Vec<f32> {
    m.iter().flatten().copied().collect()
}

/// Feature settings the engine uploads in `SharedData::FeatureData` (b6) that the Lighting PS reads without
/// any feature define (linear-lighting multipliers in `Color::DirectionalLight/Ambient`, vanilla Fresnel,
/// TruePBR micro-shadows) plus the CS Skin / Hair blocks for when those features are compiled. Values from
/// `state_6544.json` (frame11178).
///
/// CS 1.9 values observed in capture 11178: the fallback for every setting the user's files lack (C7,
/// [`crate::cs_settings::feature_data`]).
pub const FEATURE_FLOATS: &[(&str, &[f32])] = &[
    ("linearLightingSettings.diffuseGamma", &[2.2]),
    ("linearLightingSettings.directionalLightMult", &[1.0]),
    (
        "linearLightingSettings.pointLightMult",
        &[std::f32::consts::PI],
    ),
    ("linearLightingSettings.ambientMult", &[1.0]),
    ("linearLightingSettings.glowmapMult", &[0.66]),
    ("linearLightingSettings.effectLightingMult", &[0.32]),
    ("linearLightingSettings.membraneEffectMult", &[1.0]),
    ("linearLightingSettings.bloodEffectMult", &[1.0]),
    ("linearLightingSettings.projectedEffectMult", &[1.0]),
    ("linearLightingSettings.deferredEffectMult", &[1.0]),
    ("linearLightingSettings.otherEffectMult", &[1.0]),
    ("linearLightingSettings.particleEffectMult", &[1.0]),
    ("linearLightingSettings.lightSpriteEffectMult", &[1.0]),
    ("linearLightingSettings.fireEffectMult", &[1.0]),
    ("linearLightingSettings.fireEffectCurve", &[1.0]),
    ("linearLightingSettings.diffuseCurve", &[1.448_227_6]),
    ("linearLightingSettings.diffuseWhiteReflectance", &[1.0]),
    ("vanillaFresnelSettings.RoughnessMultiplier", &[1.0]),
    ("vanillaFresnelSettings.SpecularRoughnessBlend", &[1.0]),
    ("vanillaFresnelSettings.BaseF0Multiplier", &[0.32]),
    ("vanillaFresnelSettings.MinF0", &[0.02]),
    ("vanillaFresnelSettings.CubemapToF0Multiplier", &[1.0]),
    ("vanillaFresnelSettings.ComplexMaterialF0Multiplier", &[1.0]),
    ("truePBRSettings.VertexAOStrength", &[1.0]),
    ("truePBRSettings.MicroShadowStrength", &[1.0]),
    ("hairSpecularSettings.HairGlossiness", &[70.0]),
    ("hairSpecularSettings.SpecularMult", &[1.0]),
    ("hairSpecularSettings.DiffuseMult", &[1.0]),
    ("hairSpecularSettings.PrimaryTangentShift", &[0.5]),
    ("hairSpecularSettings.SecondaryTangentShift", &[-0.25]),
    ("hairSpecularSettings.HairSaturation", &[1.0]),
    ("hairSpecularSettings.SpecularIndirectMult", &[1.0]),
    ("hairSpecularSettings.DiffuseIndirectMult", &[1.0]),
    ("hairSpecularSettings.BaseColorMult", &[1.5]),
    ("hairSpecularSettings.Transmission", &[1.0]),
    ("hairSpecularSettings.SelfShadowStrength", &[1.0]),
    ("hairSpecularSettings.SelfShadowExponent", &[0.1]),
    ("hairSpecularSettings.SelfShadowScale", &[2.5]),
    ("skinData.skinParams", &[0.7, 0.35, 1.0, 1.0]),
    ("skinData.skinParams2", &[0.15, 0.0, 0.0278, 1.0]),
    ("skinData.skinDetailParams", &[20.0, 2.0, 0.25, 0.0]),
    ("skinData.sssParams", &[0.1, 0.2, 1.0, 1.0]),
    ("skinData.fuzzParams", &[1.0, 0.35, 0.045, 0.25]),
    ("skinData.physicalParams", &[1.3, 0.75, 1.0, 1.0]),
    ("skinData.wetParams", &[512.0, 0.7, 10.0, 4.0]),
];
/// CS 1.9 values observed in capture 11178 (see [`FEATURE_FLOATS`]).
pub const FEATURE_UINTS: &[(&str, u32)] = &[
    ("vanillaFresnelSettings.Enable", 1),
    ("vanillaFresnelSettings.EnableEyeSpecialHandling", 1),
    ("truePBRSettings.EnableMicroShadows", 1),
    ("hairSpecularSettings.Enabled", 1),
    ("hairSpecularSettings.EnableTangentShift", 1),
    ("hairSpecularSettings.EnableSelfShadow", 1),
    ("hairSpecularSettings.HairMode", 1),
];

/// Names the brief requires `Scene::write` to land somewhere in a draw's cbuffers; a name no writer of a
/// draw accepted is logged once per build (CS version drift, 03 §6). `TexcoordOffset` is written by the
/// draw from the NIF's UV offset/scale.
pub const REQUIRED: &[&str] = &[
    "World",
    "ViewProj",
    "EyePosition",
    "DirLightDirection",
    "DirLightColor",
    "DirectionalAmbient",
    "TexcoordOffset",
];

impl Scene {
    /// Writes the scene and camera into one cbuffer (any stage, any slot); fields the layout lacks are
    /// skipped. `model` is the shape's `Mesh.transform`; `feature_values` the FeatureData values
    /// ([`crate::cs_settings::feature_data`]). Returns the names this writer accepted.
    pub fn write(
        &self,
        w: &mut CbWriter,
        v: &View,
        model: &Mat3x4,
        feature_values: &[(String, Vec<f64>)],
    ) -> Vec<&'static str> {
        let mut ok = vec![];
        let mut put = |w: &mut CbWriter, name: &'static str, data: &[f32]| {
            if w.set(name, data) {
                ok.push(name);
            }
        };
        let world = rows(&v.world(model));
        put(w, "World", &world);
        put(w, "PreviousWorld", &world);
        // Camera-relative: the eye is the origin.
        put(w, "EyePosition", &[0.0, 0.0, 0.0, 0.0]);
        put(w, "ViewProj", &rows(&v.view_proj));
        // Skinned draws: identity bones, the camera position as the pivot (Skinned.hlsli subtracts it).
        put(w, "BonesPivot", &v.eye);
        put(w, "PreviousBonesPivot", &v.eye);
        // Vertex fog off: FogParam.w (max fog) = 0, and log2 of 0 → exp2(−inf) = 0, no NaN.
        put(w, "FogParam", &[0.0, 0.0, 1.0, 0.0]);
        let l = self.dir_light;
        put(
            w,
            "DirLightDirection",
            &[l.direction[0], l.direction[1], l.direction[2], 0.0],
        );
        let sun = self.sun_color();
        put(w, "DirLightColor", &[sun[0], sun[1], sun[2], 0.0]);
        let ambient = self.ambient();
        put(w, "DirectionalAmbient", &rows(&dalc(&ambient)));
        let sh = dalc_to_sh(&ambient);
        put(w, "AmbientSHR", &sh[0]);
        put(w, "AmbientSHG", &sh[1]);
        put(w, "AmbientSHB", &sh[2]);
        // b0 PS: colour clamps (captured 1,1,1).
        put(w, "ColourOutputClamp", &[1.0, 1.0, 1.0, 0.0]);
        put(
            w,
            "AmbientSpecularTintAndFresnelPower",
            &[0.0, 0.0, 0.0, 1.0],
        );
        put(w, "SSRParams", &[0.1, 0.1, 1.0, 1.0]);
        let (fw, fh) = (v.width.max(1) as f32, v.height.max(1) as f32);
        put(w, "BufferDim", &[fw, fh, 1.0 / fw, 1.0 / fh]);
        // (far, near, far − near, far · near), as captured.
        put(
            w,
            "CameraData",
            &[v.far, v.near, v.far - v.near, v.far * v.near],
        );
        put(w, "Timer", &[1.0]);
        put(w, "WaterSystemHeight", &[f32::MIN]);
        put(w, "WaterData", &[0.58, 0.58, 0.58, f32::MIN].repeat(25));
        for (n, vals) in POINT_NAMES.iter().zip(PointValues::new(self, v).values) {
            w.set(n, &vals);
        }
        put(w, "CameraView", &rows(&v.view));
        put(w, "CameraProj", &rows(&v.proj));
        put(w, "CameraProjUnjittered", &rows(&v.proj));
        for n in [
            "CameraViewProj",
            "CameraViewProjUnjittered",
            "CameraPreviousViewProjUnjittered",
        ] {
            put(w, n, &rows(&v.view_proj));
        }
        put(w, "CameraViewInverse", &rows(&inverse4(&v.view)));
        put(w, "CameraProjInverse", &rows(&inverse4(&v.proj)));
        put(w, "CameraProjUnjitteredInverse", &rows(&inverse4(&v.proj)));
        put(w, "CameraViewProjInverse", &rows(&inverse4(&v.view_proj)));
        put(w, "CameraPosAdjust", &[v.eye[0], v.eye[1], v.eye[2], 0.0]);
        put(
            w,
            "CameraPreviousPosAdjust",
            &[v.eye[0], v.eye[1], v.eye[2], 0.0],
        );
        // x = 1/gamma as captured; y/z = 0 keeps the forward fog lerp off (iLH `FrameParams.y && .z`).
        put(w, "FrameParams", &[1.0, 0.0, 0.0, 0.0]);
        put(w, "DynamicResolutionParams1", &[1.0; 4]);
        put(w, "DynamicResolutionParams2", &[1.0; 4]);
        if w.set_u32("InInterior", &[1]) {
            ok.push("InInterior");
        }
        write_feature_data(w, feature_values);
        ok
    }
}

/// Camera fields that change with the eye or the target size, in [`ViewValues`] order. `World` and
/// `PreviousWorld` depend on the shape too and are passed to [`SceneFields::write_view`] separately.
pub const VIEW_NAMES: [&str; 17] = [
    "ViewProj",
    "BonesPivot",
    "PreviousBonesPivot",
    "BufferDim",
    "CameraData",
    "CameraView",
    "CameraProj",
    "CameraProjUnjittered",
    "CameraViewProj",
    "CameraViewProjUnjittered",
    "CameraPreviousViewProjUnjittered",
    "CameraViewInverse",
    "CameraProjInverse",
    "CameraProjUnjitteredInverse",
    "CameraViewProjInverse",
    "CameraPosAdjust",
    "CameraPreviousPosAdjust",
];

/// Fields that change with the scene's lights, in [`LightValues`] order.
pub const LIGHT_NAMES: [&str; 6] = [
    "DirLightDirection",
    "DirLightColor",
    "DirectionalAmbient",
    "AmbientSHR",
    "AmbientSHG",
    "AmbientSHB",
];

/// The values of [`VIEW_NAMES`] for one frame, computed once per frame (the four inverses included) instead of
/// once per cbuffer (03 §7). Same values as [`Scene::write`].
pub struct ViewValues {
    pub values: Vec<Vec<f32>>,
}

impl ViewValues {
    pub fn new(v: &View) -> ViewValues {
        let (fw, fh) = (v.width.max(1) as f32, v.height.max(1) as f32);
        let vp = rows(&v.view_proj);
        let eye4 = vec![v.eye[0], v.eye[1], v.eye[2], 0.0];
        ViewValues {
            values: vec![
                vp.clone(),
                v.eye.to_vec(),
                v.eye.to_vec(),
                vec![fw, fh, 1.0 / fw, 1.0 / fh],
                vec![v.far, v.near, v.far - v.near, v.far * v.near],
                rows(&v.view),
                rows(&v.proj),
                rows(&v.proj),
                vp.clone(),
                vp.clone(),
                vp,
                rows(&inverse4(&v.view)),
                rows(&inverse4(&v.proj)),
                rows(&inverse4(&v.proj)),
                rows(&inverse4(&v.view_proj)),
                eye4.clone(),
                eye4,
            ],
        }
    }
}

/// The values of [`LIGHT_NAMES`] for one scene. Same values as [`Scene::write`].
pub struct LightValues {
    pub values: Vec<Vec<f32>>,
}

impl LightValues {
    pub fn new(s: &Scene) -> LightValues {
        let l = s.dir_light;
        let ambient = s.ambient();
        let sun = s.sun_color();
        let sh = dalc_to_sh(&ambient);
        LightValues {
            values: vec![
                vec![l.direction[0], l.direction[1], l.direction[2], 0.0],
                vec![sun[0], sun[1], sun[2], 0.0],
                rows(&dalc(&ambient)),
                sh[0].to_vec(),
                sh[1].to_vec(),
                sh[2].to_vec(),
            ],
        }
    }
}

/// The vanilla point-light fields of `PerGeometry` (`Lighting.hlsl` ~l.579-581) that the forward permutation
/// without `LIGHT_LIMIT_FIX` reads: `float4 PointLightPosition[7]` (w = falloff radius), `float4
/// PointLightColor[7]` and `float2 NumLightNumShadowLight` (x = lights, y = shadow lights). Positions are
/// camera-relative like `World`, so they depend on the eye as well as the scene.
pub const POINT_NAMES: [&str; 3] = [
    "PointLightPosition",
    "PointLightColor",
    "NumLightNumShadowLight",
];

/// The values of [`POINT_NAMES`] for one scene and view: light 0 is the scene's point light, 1-6 are zero.
pub struct PointValues {
    pub values: Vec<Vec<f32>>,
}

impl PointValues {
    pub fn new(s: &Scene, v: &View) -> PointValues {
        let p = s.point_light;
        let mut pos = vec![0.0; 28];
        let mut col = vec![0.0; 28];
        let mut num = 0.0;
        if p.brightness > 0.0 {
            let e = v.eye;
            pos[..4].copy_from_slice(&[
                p.position[0] - e[0],
                p.position[1] - e[1],
                p.position[2] - e[2],
                p.radius,
            ]);
            col[..4].copy_from_slice(&[p.brightness, p.brightness, p.brightness, 0.0]);
            num = 1.0;
        }
        PointValues {
            values: vec![pos, col, vec![num, 0.0]],
        }
    }
}

/// Where one cbuffer keeps the per-frame and light fields, resolved once when the draw is built.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SceneFields {
    pub world: Option<Field>,
    pub previous_world: Option<Field>,
    pub view: Vec<Option<Field>>,
    pub lights: Vec<Option<Field>>,
    pub point: Vec<Option<Field>>,
}

impl SceneFields {
    pub fn resolve(w: &CbWriter) -> SceneFields {
        SceneFields {
            world: w.resolve("World"),
            previous_world: w.resolve("PreviousWorld"),
            view: VIEW_NAMES.iter().map(|n| w.resolve(n)).collect(),
            lights: LIGHT_NAMES.iter().map(|n| w.resolve(n)).collect(),
            point: POINT_NAMES.iter().map(|n| w.resolve(n)).collect(),
        }
    }

    /// Writes `World` / `PreviousWorld` (`world` = `rows(&view.world(model))`) and the camera fields. True if
    /// this cbuffer has any of them (it must then be uploaded).
    pub fn write_view(&self, w: &mut CbWriter, v: &ViewValues, world: &[f32]) -> bool {
        let mut any = false;
        for f in [self.world, self.previous_world].into_iter().flatten() {
            w.set_at(f, world);
            any = true;
        }
        for (f, vals) in self.view.iter().zip(&v.values) {
            if let Some(f) = f {
                w.set_at(*f, vals);
                any = true;
            }
        }
        any
    }

    /// Writes the point-light fields. True if this cbuffer has any of them.
    pub fn write_point(&self, w: &mut CbWriter, p: &PointValues) -> bool {
        let mut any = false;
        for (f, vals) in self.point.iter().zip(&p.values) {
            if let Some(f) = f {
                w.set_at(*f, vals);
                any = true;
            }
        }
        any
    }

    /// Writes the light fields. True if this cbuffer has any of them.
    pub fn write_lights(&self, w: &mut CbWriter, l: &LightValues) -> bool {
        let mut any = false;
        for (f, vals) in self.lights.iter().zip(&l.values) {
            if let Some(f) = f {
                w.set_at(*f, vals);
                any = true;
            }
        }
        any
    }
}

/// Every field [`Scene::write`] writes that depends on neither the camera nor the lights: constants and the
/// FeatureData block (03 §7 "static"). Written when a draw is built and after every material rewrite (the
/// rewrite zeroes the whole writer).
pub fn write_static(w: &mut CbWriter, feature_values: &[(String, Vec<f64>)]) {
    // Camera-relative: the eye is the origin.
    w.set("EyePosition", &[0.0, 0.0, 0.0, 0.0]);
    // Vertex fog off: FogParam.w (max fog) = 0, and log2 of 0 → exp2(−inf) = 0, no NaN.
    w.set("FogParam", &[0.0, 0.0, 1.0, 0.0]);
    w.set("ColourOutputClamp", &[1.0, 1.0, 1.0, 0.0]);
    w.set("AmbientSpecularTintAndFresnelPower", &[0.0, 0.0, 0.0, 1.0]);
    w.set("SSRParams", &[0.1, 0.1, 1.0, 1.0]);
    w.set("Timer", &[1.0]);
    w.set("WaterSystemHeight", &[f32::MIN]);
    w.set("WaterData", &[0.58, 0.58, 0.58, f32::MIN].repeat(25));
    w.set("FrameParams", &[1.0, 0.0, 0.0, 0.0]);
    w.set("DynamicResolutionParams1", &[1.0; 4]);
    w.set("DynamicResolutionParams2", &[1.0; 4]);
    w.set_u32("InInterior", &[1]);
    write_feature_data(w, feature_values);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dalc_and_sh_match_capture_11178() {
        // answers.md Q2, frame11178: uniform (0.090842, 0.045186, 0.011612).
        let c = [0.090_841_71, 0.045_186_2, 0.011_612_247];
        let six = [c; 6];
        let m = dalc(&six);
        assert_eq!(m[0][..3], [0.0; 3]);
        assert!((m[0][3] - 0.090_841_71).abs() < 1e-6);
        let sh = dalc_to_sh(&six);
        assert!((sh[0][0] - 0.322_025_5).abs() < 1e-6, "{:?}", sh[0]);
        assert!((sh[1][0] - 0.160_180_9).abs() < 1e-6);
        assert!((sh[2][0] - 0.041_164_3).abs() < 1e-6);
        assert_eq!(sh[0][1..], [0.0; 3]);
    }

    #[test]
    fn dalc_x_gradient_lands_in_w_like_4869() {
        // frame4869: +X 0.000379, −X 0.001518, others 0.000949 → SH only in c0 and .w.
        let mut six = [[0.000_949; 3]; 6];
        six[0] = [0.000_380; 3]; // t + m with the table's rounded m = −0.000569, t = 0.000949
        six[1] = [0.001_518; 3];
        let m = dalc(&six);
        assert!((m[0][0] - (-0.000_569)).abs() < 1e-7);
        assert!((m[0][3] - 0.000_949).abs() < 1e-6);
        let sh = dalc_to_sh(&six);
        assert!((sh[0][0] - 0.003_362_4).abs() < 5e-6, "{:?}", sh[0]); // inputs rounded to 3 digits
        assert!(sh[0][1].abs() < 1e-9 && sh[0][2].abs() < 1e-9);
        assert!((sh[0][3] - 0.001_164_8).abs() < 5e-6, "{:?}", sh[0]);
    }

    #[test]
    fn world_rows_pack_row_major_camera_relative() {
        use crate::compile::{CbLayout, CbVar, VarKind};
        let var = |name: &str, offset| CbVar {
            name: name.into(),
            offset,
            size: 48,
            kind: VarKind::Float,
        };
        let layout = CbLayout {
            name: "PerGeometry".into(),
            slot: 2,
            size: 96,
            vars: vec![var("World", 0), var("PreviousWorld", 48)],
        };
        let mut w = CbWriter::new(&layout);
        let mut v = View::new(&Camera::Auto, None, 4, 4);
        v.eye = [1.0, 2.0, 3.0];
        let model = [
            [2.0, 0.0, 0.0, 10.0],
            [0.0, 2.0, 0.0, 20.0],
            [0.0, 0.0, 2.0, 30.0],
        ];
        Scene::neutral().write(&mut w, &v, &model, &[]);
        let f = |o: usize| f32::from_le_bytes(w.bytes()[o..o + 4].try_into().unwrap());
        // Row r of the nifly transform is register c<r>: [m_r0, m_r1, m_r2, t_r − eye_r].
        assert_eq!([f(0), f(4), f(8), f(12)], [2.0, 0.0, 0.0, 9.0]);
        assert_eq!([f(16), f(20), f(24), f(28)], [0.0, 2.0, 0.0, 18.0]);
        assert_eq!([f(32), f(36), f(40), f(44)], [0.0, 0.0, 2.0, 27.0]);
    }

    #[test]
    fn auto_camera_projects_centre_to_screen_centre() {
        let v = View::new(&Camera::Auto, Some(([5.0, 6.0, 7.0], 2.0)), 200, 100);
        let c = [5.0 - v.eye[0], 6.0 - v.eye[1], 7.0 - v.eye[2], 1.0];
        let clip: Vec<f32> = (0..4)
            .map(|r| (0..4).map(|k| v.view_proj[r][k] * c[k]).sum())
            .collect();
        assert!(clip[0].abs() < 1e-4 && clip[1].abs() < 1e-4);
        let z = clip[2] / clip[3];
        assert!(z > 0.0 && z < 1.0, "{z}");
        let id = mul4(&v.view_proj, &inverse4(&v.view_proj));
        for (r, row) in id.iter().enumerate() {
            for (c, x) in row.iter().enumerate() {
                assert!((x - if r == c { 1.0 } else { 0.0 }).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn auto_camera_faces_the_front_from_above_and_light_hits_that_side() {
        let c = [5.0, 6.0, 7.0];
        let v = View::new(&Camera::Auto, Some((c, 2.0)), 64, 64);
        // Skyrim actors face +Y: the default view looks at them from +Y, a little above.
        assert!(v.eye[1] > c[1] + 3.0, "{:?}", v.eye);
        assert!(v.eye[2] > c[2], "{:?}", v.eye);
        assert!((v.eye[0] - c[0]).abs() < 1e-4, "{:?}", v.eye);
        let to_eye = sub(v.eye, c);
        assert!(dot(Scene::neutral().dir_light.direction, to_eye) > 0.0);
        // The orbit at the auto angles is the same camera, so a first drag does not jump.
        let o = Camera::Orbit {
            target: c,
            yaw: AUTO_YAW,
            pitch: AUTO_PITCH,
            distance: 2.0 * AUTO_DISTANCE,
        };
        let w = View::new(&o, Some((c, 2.0)), 64, 64);
        for k in 0..3 {
            assert!((w.eye[k] - v.eye[k]).abs() < 1e-4);
        }
    }

    #[test]
    fn srgb_curve() {
        assert!((srgb_to_linear(0.8745) - 0.7379).abs() < 1e-3);
        assert!((srgb_to_linear(0.7961) - 0.5972).abs() < 1e-3);
    }
}
