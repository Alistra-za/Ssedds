//! The artist checks (v2, DECISIONS Note CI): one pure function per check turns the engine's cached check stats
//! into `CHK-*` warnings, gated by the `[checks]` switches. Results live only in `AppModel`'s check maps: never in
//! `TextureItem::warnings` or `SetView::set_warnings`, so they never change "ready" (owner answer 1).
use super::sets::{SlotFill, SlotKind};
use super::AppModel;
use crate::pgp::settings::{feature_on, Feature, Settings};
use crate::settings::ChecksSettings;
use engine::analyse::{Analysis, ChannelStats};
use engine::checks::{luma_code, BandCounts, CheckStats, LUMA_BLACK, LUMA_BRIGHT, LUMA_DARK};
use engine::codes::{Code, Warning};
use engine::config::{Category, Subtype};
use engine::policy::ResolvedJob;
use engine::presets::AlphaContent;
use engine::session::Thumbnail;
use engine::shader::Shader;
use serde_json::{json, Value};
use std::collections::HashMap;

/// Albedo: warn when more than this share of texels is darker than 30 or brighter than 240 (luma codes). Tuned on
/// the 10k set (DECISIONS CI8): 0.05 flagged 43 % of PBR albedos.
pub const RANGE_SHARE: f64 = 0.50;
/// Metallic (G of `_rmaos`): warn when more than this share is grey (codes 26..=229). 0.20 flagged 19 % (CI8).
pub const METAL_GREY_SHARE: f64 = 0.40;
/// Roughness (R of `_rmaos`): warn when more than this share is at code 0 or 255.
pub const ROUGH_EXTREME_SHARE: f64 = 0.50;
/// AO (B of `_rmaos`): warn when its mean is below this.
pub const AO_DARK_MEAN: f64 = 0.35;
/// Emissive: warn when fewer than this share of texels are black (luma code `<= LUMA_BLACK`).
pub const EMISSIVE_BLACK_MIN: f64 = 0.25;
/// Dark metal: warn when more than this share of texels are metal (`_rmaos` G >= 128) with albedo luma < 155.
pub const DARK_METAL_SHARE: f64 = 0.50;
/// A metal map needs at least this share of metal texels before dark metal is judged.
pub const DARK_METAL_MIN_METAL: f64 = 0.02;
/// Plastic: a vanilla `_n` whose alpha is at least this share >= 250 (or missing) has no specular map.
pub const PLASTIC_SHARE: f64 = 0.95;
/// Green guess: the flipped curl is under this ratio of the as-stored curl.
pub const GREEN_RATIO: f64 = 0.5;
/// Green guess: below this as-stored curl the map is too flat to judge.
pub const CURL_FLOOR: f64 = 1e-4;
/// Parallax: a share at the clipping codes above this warns.
pub const CLIP_SHARE: f64 = 0.10;
/// Parallax: a share of steep neighbour steps above this warns. 0.01 flagged 7 % of `_p` (CI8).
pub const STEEP_SHARE: f64 = 0.03;
/// Parallax: a height whose max − min is under this many codes uses too little of its range.
pub const RANGE_USE_CODES: f64 = 16.0;
/// Landscape displacement scales further apart than this factor are flagged.
pub const SCALE_FACTOR: f64 = 2.0;

/// The check a `CHK-*` code belongs to; its switch hides it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    Range,
    Plastic,
    LowDetail,
    Green,
    Parallax,
    Uv,
}

/// Every `CHK-*` code's check; `None` for any other code.
pub fn check_of(code: Code) -> Option<Check> {
    Some(match code {
        Code::ChkRange => Check::Range,
        Code::ChkPlastic => Check::Plastic,
        Code::ChkLowDetail => Check::LowDetail,
        Code::ChkGreenGuess | Code::ChkZSign => Check::Green,
        Code::ChkParallax | Code::ChkParallaxScale => Check::Parallax,
        Code::ChkUvGutter => Check::Uv,
        _ => return None,
    })
}

/// Whether `c`'s switch is on.
pub fn switch_on(on: &ChecksSettings, c: Check) -> bool {
    match c {
        Check::Range => on.range,
        Check::Plastic => on.plastic,
        Check::LowDetail => on.low_detail,
        Check::Green => on.green,
        Check::Parallax => on.parallax,
        Check::Uv => on.uv,
    }
}

/// What a set's height map is judged against (spec, "Parallax height standards").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeightNeutral {
    /// 0.5: vanilla, CPM and PBR parallax.
    Half,
    /// 1.0: the PBR multilayer inner layer (coat parallax); 0 is the maximum recess.
    White,
    /// Vanilla multilayer: the layer alpha is a thickness, not a height.
    NotHeight,
}

/// `resolved` is the set's settings through `auto_flags::resolved`, so a `_cnr`'s auto multilayer counts (X-S1).
pub fn height_neutral(shader: Option<Shader>, resolved: &Settings) -> HeightNeutral {
    match shader {
        Some(Shader::Pbr)
            if feature_on(resolved, Feature::Multilayer)
                && resolved.get("coat_parallax").and_then(Value::as_bool) == Some(true) =>
        {
            HeightNeutral::White
        }
        Some(Shader::Multilayer) => HeightNeutral::NotHeight,
        _ => HeightNeutral::Half,
    }
}

/// A per-texel value-range rule: the share of texels where `bad` is true is the share the check compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeRule {
    AlbedoLuma,
    MetalGrey,
    RoughExtreme,
    EmissiveBackground,
}

impl RangeRule {
    pub fn bad(self, px: [u8; 4]) -> bool {
        match self {
            RangeRule::AlbedoLuma => {
                let l = luma_code(px[0], px[1], px[2]);
                !(LUMA_DARK..=LUMA_BRIGHT).contains(&l)
            }
            RangeRule::MetalGrey => (26..=229).contains(&px[1]),
            RangeRule::RoughExtreme => px[0] == 0 || px[0] == 255,
            RangeRule::EmissiveBackground => luma_code(px[0], px[1], px[2]) > LUMA_BLACK,
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            RangeRule::AlbedoLuma => "albedo_luma",
            RangeRule::MetalGrey => "metal_grey",
            RangeRule::RoughExtreme => "rough_extreme",
            RangeRule::EmissiveBackground => "emissive_background",
        }
    }
    /// The texels `bad` is true for, from the band counts.
    pub fn count(self, s: &CheckStats) -> u64 {
        match self {
            RangeRule::AlbedoLuma => s.luma_dark + s.luma_bright,
            RangeRule::MetalGrey => s.texels - s.bands[1].low - s.bands[1].high,
            RangeRule::RoughExtreme => s.bands[0].eq0 + s.bands[0].eq255,
            RangeRule::EmissiveBackground => s.texels - s.luma_black,
        }
    }
    /// The share above which the rule warns.
    fn limit(self) -> f64 {
        match self {
            RangeRule::AlbedoLuma => RANGE_SHARE,
            RangeRule::MetalGrey => METAL_GREY_SHARE,
            RangeRule::RoughExtreme => ROUGH_EXTREME_SHARE,
            RangeRule::EmissiveBackground => 1.0 - EMISSIVE_BLACK_MIN,
        }
    }
    fn message(self, share: f64) -> String {
        let p = pct(share);
        match self {
            RangeRule::AlbedoLuma => format!(
                "albedo: {p} % of texels are darker than 30/255 or brighter than 240/255 (PBR guidance 30–240)"
            ),
            RangeRule::MetalGrey => format!(
                "metallic (G): {p} % of texels are grey (26–229/255); metal is usually 0 or 255, grey reads as half metal"
            ),
            RangeRule::RoughExtreme => format!(
                "roughness (R): {p} % of texels are at 0 or 255; few real surfaces are perfectly smooth or rough"
            ),
            RangeRule::EmissiveBackground => format!(
                "emissive: only {} % of texels are black, so most of the surface glows",
                pct(1.0 - share)
            ),
        }
    }
}

/// The per-texel rules of a sub-type (AO dark is a mean, not a rule).
pub fn rules_for(st: Subtype) -> &'static [RangeRule] {
    match st {
        Subtype::PbrAlbedo => &[RangeRule::AlbedoLuma],
        Subtype::Rmaos => &[RangeRule::RoughExtreme, RangeRule::MetalGrey],
        Subtype::PbrEmissive | Subtype::Glow => &[RangeRule::EmissiveBackground],
        _ => &[],
    }
}

/// `n / d`, 0.0 when `d` is 0.
fn share(n: u64, d: u64) -> f64 {
    if d == 0 {
        0.0
    } else {
        n as f64 / d as f64
    }
}

fn pct(s: f64) -> String {
    format!("{:.0}", s * 100.0)
}

fn range(rule: &str, msg: String) -> Warning {
    Warning::new(Code::ChkRange, msg).with_data(json!({ "rule": rule }))
}

/// Checks 1, 2, 4 and 5 of one texture, from its cached stats. Empty without stats.
pub fn texture_checks(
    job: &ResolvedJob,
    a: &Analysis,
    neutral: HeightNeutral,
    on: &ChecksSettings,
) -> Vec<Warning> {
    let Some(s) = &a.checks else {
        return vec![];
    };
    let st = job.profile.subtype;
    let mut out = vec![];
    if on.range {
        for &r in rules_for(st) {
            let sh = share(r.count(s), s.texels);
            if sh > r.limit() {
                out.push(range(r.id(), r.message(sh)));
            }
        }
        if st == Subtype::Rmaos && s.texels > 0 && a.channels[2].mean < AO_DARK_MEAN {
            out.push(range(
                "ao_dark",
                format!(
                    "AO (B) averages {:.2}; a dark AO darkens the whole surface",
                    a.channels[2].mean
                ),
            ));
        }
    }
    if on.plastic
        && st == Subtype::Normal
        && (a.alpha == AlphaContent::None || share(a.alpha_hist.ge250, s.texels) >= PLASTIC_SHARE)
    {
        out.push(Warning::new(
            Code::ChkPlastic,
            "_n has no specular in its alpha (missing or all white): the surface may look plastic",
        ));
    }
    if on.green && st.category() == Category::Normal {
        if st != Subtype::Msn && !s.two_channel && s.texels > 0 && a.channels[2].mean < 0.5 {
            out.push(Warning::new(
                Code::ChkZSign,
                format!(
                    "normal: blue averages {:.2}, so most normals point into the surface (Z sign)",
                    a.channels[2].mean
                ),
            ));
        }
        if let Some(c) = s.curl {
            if c.as_stored > CURL_FLOOR
                && c.flipped < GREEN_RATIO * c.as_stored
                && !job.profile.channels.flip_green
            {
                out.push(Warning::new(
                    Code::ChkGreenGuess,
                    "normal: the green channel looks flipped (curl test); if the lighting looks inverted, try \
                     \"Treat as OpenGL (flip green)\"",
                ));
            }
        }
    }
    if on.parallax {
        out.extend(parallax_checks(job, a, s, neutral));
    }
    out
}

/// Check 5 on a `_p` (R) or a CPM `_m` with parallax on (A). An R8 / BC4 `_p` decodes with A = 255, so A is never
/// read for a `_p`.
fn parallax_checks(
    job: &ResolvedJob,
    a: &Analysis,
    s: &CheckStats,
    neutral: HeightNeutral,
) -> Vec<Warning> {
    let cpm = job.profile.subtype == Subtype::ComplexMaterial;
    let (c, b, what): (&ChannelStats, &BandCounts, &str) = match job.profile.subtype {
        Subtype::Height => (&a.channels[0], &s.bands[0], "_p"),
        // CI1 (review M2): without source alpha A reads 1.0 everywhere, so its bands say nothing; the engine's
        // ALPHA-PARALLAX-NO-ALPHA already names that case.
        Subtype::ComplexMaterial
            if job.profile.alpha.parallax == Some(true) && a.alpha != AlphaContent::None =>
        {
            (&a.channels[3], &s.bands[3], "_m alpha")
        }
        _ => return vec![],
    };
    if neutral == HeightNeutral::NotHeight || s.texels == 0 {
        return vec![];
    }
    let n = s.texels;
    let mut out = vec![];
    let mut warn = |m: String| out.push(Warning::new(Code::ChkParallax, m));
    let used = f64::from(c.max - c.min);
    if used < RANGE_USE_CODES / 255.0 {
        warn(format!(
            "{what} uses only {:.0}/255 of its range: the parallax will look flat",
            used * 255.0
        ));
    }
    match neutral {
        HeightNeutral::Half if share(b.eq0 + b.eq255, n) > CLIP_SHARE => warn(format!(
            "{} % of {what} is clipped at 0 or 255",
            pct(share(b.eq0 + b.eq255, n))
        )),
        HeightNeutral::White if share(b.eq0, n) > CLIP_SHARE => warn(format!(
            "{} % of {what} is clipped at 0, the maximum recess",
            pct(share(b.eq0, n))
        )),
        _ => {}
    }
    if cpm && share(b.le4 + b.ge251, n) > CLIP_SHARE {
        warn(format!(
            "{} % of {what} is at or below 4/255 or at or above 251/255: parallax is off there",
            pct(share(b.le4 + b.ge251, n))
        ));
    }
    if neutral == HeightNeutral::Half && c.mean > 0.75 {
        warn(format!(
            "{what} is mostly white: with a 0.5 neutral it reads as raised everywhere"
        ));
    }
    if let Some(st) = s.steep.filter(|&st| st > STEEP_SHARE) {
        warn(format!(
            "{} % of {what}'s neighbour steps jump by 32/255 or more: expect stretched texels at the edges",
            pct(st)
        ));
    }
    out
}

/// The share of texels that are metal (`rmaos` G >= 128) with albedo luma < 155. `None` when the sizes differ or
/// fewer than 2 % of texels are metal.
pub fn dark_metal_share(albedo: &Thumbnail, rmaos: &Thumbnail) -> Option<f64> {
    if (albedo.width, albedo.height) != (rmaos.width, rmaos.height)
        || albedo.rgba.len() != rmaos.rgba.len()
    {
        return None;
    }
    let (mut metal, mut dark) = (0u64, 0u64);
    let (pa, pr) = (
        albedo.rgba.as_chunks::<4>().0,
        rmaos.rgba.as_chunks::<4>().0,
    );
    for (a, r) in pa.iter().zip(pr) {
        if r[1] >= 128 {
            metal += 1;
            dark += u64::from(luma_code(a[0], a[1], a[2]) < 155);
        }
    }
    let texels = (albedo.rgba.len() / 4) as u64;
    (texels > 0 && share(metal, texels) >= DARK_METAL_MIN_METAL).then(|| share(dark, texels))
}

/// The keys whose scale is more than `SCALE_FACTOR` from the rest: both of a pair, else against the lower median.
/// Scales that are not finite or `<= 0` are left out.
pub fn scale_outliers(scales: &[(String, f64)]) -> Vec<String> {
    let ok: Vec<&(String, f64)> = scales
        .iter()
        .filter(|(_, s)| s.is_finite() && *s > 0.0)
        .collect();
    if ok.len() < 2 {
        return vec![];
    }
    if ok.len() == 2 {
        let (a, b) = (ok[0].1, ok[1].1);
        return if a.max(b) / a.min(b) > SCALE_FACTOR {
            ok.iter().map(|(k, _)| k.clone()).collect()
        } else {
            vec![]
        };
    }
    let mut v: Vec<f64> = ok.iter().map(|(_, s)| *s).collect();
    v.sort_by(f64::total_cmp);
    let m = v[(v.len() - 1) / 2];
    ok.iter()
        .filter(|(_, s)| (s / m).max(m / s) > SCALE_FACTOR)
        .map(|(k, _)| k.clone())
        .collect()
}

/// The check results by item key and by set key, and what they were computed from.
#[derive(Default)]
pub(crate) struct CheckState {
    items: HashMap<String, Vec<Warning>>,
    sets: HashMap<String, Vec<Warning>>,
    /// (plans applied, sets document revision, switches, UV cache revision) of the last refresh (K1).
    key: Option<(u64, u64, ChecksSettings, u64)>,
    /// Bumped whenever `items` is replaced by a plan or cleared by a new folder.
    pub plans_applied: u64,
}

#[cfg(test)]
impl CheckState {
    /// Add one result to `key` without a refresh (unit tests).
    pub(crate) fn push_item_warning(&mut self, key: &str, w: Warning) {
        self.items.entry(key.to_string()).or_default().push(w);
    }
}

fn filled(slots: &std::collections::BTreeMap<SlotKind, SlotFill>, k: SlotKind) -> bool {
    !matches!(slots.get(&k), None | Some(SlotFill::Empty))
}

impl AppModel {
    /// The check results of item `item_key`, Info included.
    pub fn check_warnings(&self, item_key: &str) -> &[Warning] {
        self.check_state
            .items
            .get(item_key)
            .map_or(&[], Vec::as_slice)
    }

    /// The set-level check results of engine set `set_key` (CHK-PARALLAX-SCALE).
    pub fn set_check_warnings(&self, set_key: &str) -> &[Warning] {
        self.check_state
            .sets
            .get(set_key)
            .map_or(&[], Vec::as_slice)
    }

    /// Change the switches and save them; the next `poll` recomputes the results (no re-plan).
    pub fn set_checks(&mut self, c: ChecksSettings) {
        self.settings.checks = c;
        self.save_settings();
    }

    /// Recompute the check maps when a plan landed, the sets document changed or a switch moved; then every
    /// view-keyed cache rebuilds. Its key never includes `items_rev`, so its own `invalidate_views` cannot re-run it.
    pub(super) fn refresh_checks(&mut self) -> bool {
        let doc = self
            .sets_doc
            .as_ref()
            .map_or(0, crate::sets_doc::SetsDoc::rev);
        let key = (
            self.check_state.plans_applied,
            doc,
            self.settings.checks,
            self.uv.revision(),
        );
        if self.check_state.key == Some(key) {
            return false;
        }
        self.check_state.key = Some(key);
        let on = self.settings.checks;
        let views = super::sets::views(self);
        let mut neutral = HashMap::new();
        let mut scales = vec![];
        let mut items: HashMap<String, Vec<Warning>> = HashMap::new();
        for v in views.iter().filter(|v| !v.key.is_empty()) {
            let shader = v.shader_choice.or(v.shader.as_ref().map(|p| p.shader));
            let resolved = super::auto_flags::resolved(v.pbr, &v.slots, &v.settings);
            neutral.insert(v.key.as_str(), height_neutral(shader, &resolved));
            if matches!(shader, Some(Shader::Landscape | Shader::PbrLandscape))
                && filled(&v.slots, SlotKind::Height)
            {
                // The texture-set block's displacementScale is already overlaid into the set's settings.
                let s = resolved
                    .get("displacement_scale")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0);
                scales.push((v.key.clone(), s));
            }
        }
        for i in &self.items {
            let (Some(job), Some(a)) = (&i.job, &i.analysis) else {
                continue;
            };
            let n = i
                .set_key
                .as_deref()
                .and_then(|k| neutral.get(k))
                .copied()
                .unwrap_or(HeightNeutral::Half);
            let ws = texture_checks(job, a, n, &on);
            if !ws.is_empty() {
                items.insert(i.key.clone(), ws);
            }
        }
        // UV coverage: only what a click read (owner: on click only), from the session cache.
        let mut any_colour = false;
        for i in &self.items {
            if !i
                .job
                .as_ref()
                .is_some_and(|j| super::uv_coverage::colour(j.profile.subtype))
            {
                continue;
            }
            any_colour = true;
            let g = self
                .uv_tex(&i.key)
                .and_then(|t| self.uv.get(&t))
                .and_then(|c| c.gutter)
                .filter(|&g| on.uv && g > super::uv_coverage::GUTTER_SHARE);
            if let Some(g) = g {
                items.entry(i.key.clone()).or_default().push(Warning::new(
                    Code::ChkUvGutter,
                    format!(
                        "UV islands sit closer than 8 texels for {:.1} % of used texels: colours bleed across them at distance (mip 3+)",
                        g * 100.0
                    ),
                ));
            }
        }
        self.uv.any_colour = any_colour;
        if on.range {
            for v in views.iter() {
                let member = |st: Subtype| {
                    v.members
                        .iter()
                        .filter_map(|k| self.item(k))
                        .find(|i| i.job.as_ref().is_some_and(|j| j.profile.subtype == st))
                };
                let (Some(al), Some(rm)) = (member(Subtype::PbrAlbedo), member(Subtype::Rmaos))
                else {
                    continue;
                };
                let (Some(ta), Some(tr)) = (&al.thumb, &rm.thumb) else {
                    continue;
                };
                if let Some(sh) = dark_metal_share(ta, tr).filter(|&s| s > DARK_METAL_SHARE) {
                    items.entry(al.key.clone()).or_default().push(range(
                        "dark_metal",
                        format!(
                            "albedo: {} % of texels are metal (from the _rmaos) with luma under 155; metal albedo is usually bright",
                            pct(sh)
                        ),
                    ));
                }
            }
        }
        let mut sets: HashMap<String, Vec<Warning>> = HashMap::new();
        if on.parallax {
            for k in scale_outliers(&scales) {
                let s = scales.iter().find(|(x, _)| *x == k).map_or(1.0, |p| p.1);
                sets.entry(k).or_default().push(Warning::new(
                    Code::ChkParallaxScale,
                    format!(
                        "displacement scale {s} is more than {SCALE_FACTOR}× from the other landscape sets': heights will not match where they meet"
                    ),
                ));
            }
        }
        self.check_state.items = items;
        self.check_state.sets = sets;
        self.invalidate_views();
        true
    }
}

/// "Checks: 3 warnings on 1 texture and 1 set; see Problems" (preflight K19).
pub fn summary_text(n: usize, textures: usize, sets: usize) -> String {
    let pl = |c: usize, w: &str| format!("{c} {w}{}", if c == 1 { "" } else { "s" });
    let on = match (textures, sets) {
        (t, 0) => pl(t, "texture"),
        (0, s) => pl(s, "set"),
        (t, s) => format!("{} and {}", pl(t, "texture"), pl(s, "set")),
    };
    format!("Checks: {} on {on}; see Problems", pl(n, "warning"))
}

/// The end-of-export line: the Warning-level check rows in Problems (item, set and export-row results; Info is
/// never a row). `None` when there are none. It covers what this export wrote: once the export has rows, only the
/// textures (and sets with a member) that have one are counted; with no rows it covers the whole project. A texture
/// counts once, whether its rows are keyed by item or by job (CI19).
pub fn checks_summary(m: &AppModel) -> Option<String> {
    use super::bottom::{problems, At, Level};
    use std::collections::HashSet;
    let exported: HashSet<&str> = m.export.rows.iter().map(|r| r.job_key.as_str()).collect();
    let all = exported.is_empty();
    let item_of_job = |j: &str| {
        m.items
            .iter()
            .find(|i| i.job_key == j)
            .map(|i| i.key.clone())
    };
    let wrote = |item: &str| {
        all || m
            .item(item)
            .is_some_and(|i| exported.contains(i.job_key.as_str()))
    };
    let views = super::sets::views(m);
    let (mut textures, mut sets) = (HashSet::new(), HashSet::new());
    let mut n = 0;
    for p in problems(m).iter().filter(|p| p.level == Level::Check) {
        match &p.at {
            At::Texture(k) if wrote(k) => {
                textures.insert(k.clone());
            }
            At::File(j) => {
                textures.insert(item_of_job(j).unwrap_or_else(|| j.clone()));
            }
            At::Set { key, .. }
                if views
                    .iter()
                    .any(|v| &v.key == key && v.members.iter().any(|k| wrote(k))) =>
            {
                sets.insert(key.clone());
            }
            _ => continue,
        }
        n += 1;
    }
    (n > 0).then(|| summary_text(n, textures.len(), sets.len()))
}
