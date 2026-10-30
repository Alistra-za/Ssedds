//! Note FB8 (amendment D): the UI carries no code text. A row or banner shows a sentence that names the action; the
//! code stays on the hover and in the Messages log.
use engine::codes::Code;

/// The action sentence of `code`, one table for every code Problems or a banner can show (warnings, blockers and
/// errors). Empty for the codes with none (Info results never get a row): the caller then shows the engine's message.
pub fn code_sentence(code: Code) -> &'static str {
    use Code::*;
    match code {
        LossAlphaData => {
            "This alpha channel carries data the format would drop — choose what it means"
        }
        LossClampHdr => "Some values are brighter than this format can hold — pick an HDR format or accept the clipping",
        LossReencode => "This DDS would be decoded and encoded again, losing quality — keep its format or use the original image",
        ChanNotGray => "This map should be grey but has colour — replace the file or convert it to grey",
        ChanNormalLength => "This normal map's blue (Z) channel doesn't match its red and green — use Rebuild Z or replace the file",
        ChanAoInverted => "This AO map looks inverted — flip it or replace the file",
        ChanGlossAsRough => "This map looks like gloss, not roughness — invert it or rename it",
        FmtBc5Normal => "BC5 can't store this normal map the way it is set up — choose another format",
        FmtLandscapeBc1 => "Landscape textures can't use BC1 — choose another format",
        FmtBc1Alpha => "BC1 can't keep this texture's alpha — choose a format with alpha or remove the alpha",
        FmtPerceptualData => "Perceptual weighting is wrong for a data map — turn it off or change the format",
        AlphaExactOffM => "This map needs exact alpha but it is switched off — turn it on",
        AlphaParallaxNoAlpha => "Parallax is on but the source has no alpha — add an alpha channel or turn parallax off",
        TagSrgbVanilla => "This vanilla texture is tagged sRGB but the game reads it as linear — change the colour tag",
        TagSrgbNonpbr => "sRGB is not valid for this texture type — change the colour tag",
        TagUnormPbr => "Linear (UNORM) is not valid for this PBR texture — change the colour tag",
        TagSrgbLod => "LOD textures can't be tagged sRGB — change the colour tag",
        TagContentMismatch => "The colour tag doesn't match what the image holds — check the colour setting",
        CfgUnavailable => "A setting needs something that isn't available here — change the setting",
        CfgInvalid => "A setting has a value that isn't allowed — fix it in the settings",
        CfgPresetSkipped => "A preset file could not be loaded and was skipped — fix or remove the file",
        CfgGreenConflict => "The green-channel setting contradicts the normal-map convention — pick one of them",
        DimNpot => "This texture's size isn't a power of two — resize it or acknowledge",
        DimSmall => "This texture is too small for its format — use a bigger image",
        DimFlowSmall => "This flow map is small — use a bigger image or acknowledge",
        DimTooLarge => "This texture is larger than the format allows — resize it",
        SetCompanionDds => "A companion file of this set is a DDS and can't be combined — use its original image",
        SetBakeRatio => "The maps of this set differ a lot in size — use matching sizes",
        SugLowConfidence => "Tessalith isn't sure what this alpha means — choose what it means",
        SugContradiction => "The file name and its contents disagree on the type — check the sub-type",
        ValAlphaGuarantee => "The written alpha doesn't keep its guarantee — check the alpha settings",
        ValCoverage => "Cutout coverage drifted across the mips — adjust the threshold or the coverage setting",
        ValMComplexMargin => "The smallest mip's alpha is too low for complex material — change the alpha settings",
        ValMGrayscale => "This metal map isn't grey — use a grey source",
        ValMHeightClip => "Height values in this map are clipped — lower its contrast",
        ValPgpatcher50 => "PGPatcher may read this file's alpha wrongly — check the alpha values",
        ValBc6hPeak => "Bright peaks lose accuracy in BC6H — check the highlights",
        ValHeader => "The written DDS header isn't valid — export again",
        ValMipError => "A mip level strays from its source — check the filter or the source",
        EncNvttMissing => "NVTT isn't set up — set its path in Advanced or choose another encoder",
        EncNvttVersion => "This NVTT version is untested — acknowledge to use it or install version 3.2.5",
        EncNvttFailed => "NVTT failed on this file — check its setup or choose another encoder",
        EncWeightsIgnored => "The BC7 weighting is ignored here — clear it or change the encoder",
        InDuplicate => "Two files claim the same slot — remove or rename one",
        InPackingInput => "A packing input is missing or unusable — check the files of this set",
        InDecode => "This image could not be read — replace the file",
        InNonfinite => "This image had broken pixel values that were replaced — re-export the source",
        InUnreadable => "A file in the folder could not be read and was skipped — check it",
        InAliasInvalid => "A name alias was refused — fix it in the preset",
        OutUnknownExisting => "An existing output file isn't ours — move or remove it",
        OutSidecarKept => "An old sidecar file could not be removed — delete it by hand",
        PackDefaulted => "A packed channel had no input and got a default — add the missing map",
        PackSizeMismatch => "The packed maps differ in size — use matching sizes",
        PackConflict => "A packed file collides with another output — remove one of them",
        PackUnusedInput => "A packing input isn't used by this set — remove it or change the recipe",
        PackRecipeConflict => "The packing recipe doesn't fit this set — change the recipe",
        PackRecipeUnbuildable => "The packing recipe can't be built from this set — add the maps it needs",
        RnmNoBase => "A normal layer has no base normal map — add the base",
        RnmIgnoredEye => "Eye normal layers are ignored here — remove the layer",
        RnmMaskOrphan => "A layer mask has no layer to apply to — remove it or add the layer",
        RnmLayerShadowed => "A normal layer is hidden by another — remove one of them",
        ShaderUnavailable => "The chosen shader isn't available — pick another shader",
        ShaderExtraMap => "This set has a map its shader doesn't read — remove it or change the shader",
        ShaderMissingMap => "This set lacks a map its shader needs — add it or change the shader",
        DdsHeaderUnpatchable => "This DDS header can't be fixed in place — re-export it from the source image",
        GrassShape => "This grass texture has the wrong layout — use a grass atlas shape",
        GrassTopOpaque => "The tips of this grass texture are fully opaque — check its alpha",
        GrassNormals => "This grass texture's normals look wrong — check or regenerate them",
        GrassDetectSource => "This looks like grass from its source — confirm the grass setting",
        ValGrassDetect => "The grass check failed on the output — check the grass settings",
        PbrGrassDetect => "Community Shaders would read this grass albedo as complex grass — change its alpha or name",
        ChkRange => "Some pixel values are outside the range this map type should use — adjust the map",
        ChkPlastic => "This material may look like plastic — check its roughness and metal values",
        ChkZSign => "This normal map's blue channel points the wrong way — flip it",
        ChkParallax => "The parallax height looks wrong for this map — check the height map",
        ChkParallaxScale => "The parallax scale looks too strong or too weak for this set — adjust it",
        ChkLowDetail => "This texture has little detail for its size — consider half size",
        ChkUvGutter => "UV islands sit too close together for the mip levels — widen the gaps or use fewer mips",
        DeriveDdsSource => "A DDS source can't be converted to vanilla — use the original image",
        DeriveNoAlbedo => "This set has no albedo so it can't be converted — add the albedo",
        DerivePathConflict => "A converted file lands on another file's output — rename one of them",
        AlsoPhaseFailed => "An extra export step stopped with an error — see Messages and export again",
        // Note FC10: the row's `Reset also.json` is the remedy.
        AlsoOwnedUnreadable => "also.json cannot be read — Reset also.json starts a new record",
        AlsoZipFailed => "The zip or its checksum file was not written — check the folder and export again",
        AlsoReportFailed => "The extra export's report was not written — check the folder and export again",
        CubeFaces => "A cubemap's faces are missing or differ in size — fix the six faces",
        CubeNoMips => "A cubemap without mips sparkles on curved surfaces — turn mips on",
        // Note FC3: the Info results that are rows, since the user acts on them.
        GenStale => "This generated map no longer matches its normal map or set — regenerate it",
        DeriveNoEnvmap => "No environment mask was written for this converted set — see the report",
        DeriveNifSuggest => "This converted set needs NIF values set by hand — the report lists them",
        ChkGreenGuess => "This normal map's green channel looks flipped — treat it as OpenGL if its lighting looks inverted",
        // The other Info results: no row, no sentence.
        _ => "",
    }
}

/// [`code_sentence`], or a plain line for a code with none (a place that has no engine message to fall back on).
pub fn code_line(code: Code) -> &'static str {
    match code_sentence(code) {
        "" => "Tessalith reported a problem — see Messages",
        s => s,
    }
}

/// Every code `text` names, as `(start, end, code)`: a whole token (`LOSS-ALPHA-DATA`), never a part of a longer one.
fn code_spans(text: &str) -> Vec<(usize, usize, Code)> {
    let token = |c: char| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-';
    let mut out = vec![];
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let at_start = text[..i]
            .chars()
            .next_back()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '-' || c == '_'));
        let n = rest.find(|c| !token(c)).unwrap_or(rest.len());
        if at_start && n > 0 {
            if let Some(c) = Code::parse(&rest[..n]) {
                out.push((i, i + n, c));
                i += n;
                continue;
            }
        }
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// The first code `text` names.
pub fn first_code(text: &str) -> Option<Code> {
    code_spans(text).first().map(|s| s.2)
}

/// `text` without the codes it names: a leading `CODE: ` prefix (the engine's habit) goes with its colon, a code in
/// the middle of a sentence goes alone (its backticks and the doubled space it leaves with it). The code stays in
/// the Messages log, which gets the text as it was.
pub fn without_codes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    for (s, e, _) in code_spans(text) {
        out.push_str(&text[i..s]);
        i = e;
        if text[..s].ends_with('`') && text[e..].starts_with('`') {
            out.pop();
            i += 1;
        }
        // "CODE: sentence" keeps the sentence.
        if text[i..].starts_with(':') {
            i += 1;
        }
        if out.is_empty() || out.ends_with(' ') {
            i += text[i..].len() - text[i..].trim_start_matches(' ').len();
        }
    }
    out.push_str(&text[i..]);
    out
}

/// What a row or line shows for `code` and its engine `text` (Note FB8): the code's sentence, else the engine's
/// text without its code.
pub fn shown_for(code: Option<Code>, text: &str) -> std::borrow::Cow<'_, str> {
    match code.map(code_sentence) {
        Some(s) if !s.is_empty() => s.into(),
        _ => without_codes(text).into(),
    }
}

/// A banner line (Note FB8): the first `CODE:` in it (`CFG-INVALID: preset 'x': …`, often behind a file path) says
/// the code's sentence and keeps the engine's detail after it; any other code goes. The log keeps the line whole.
pub fn banner_line(line: &str) -> String {
    let lead = code_spans(line)
        .into_iter()
        .find(|&(_, e, c)| !code_sentence(c).is_empty() && line[e..].starts_with(':'));
    match lead {
        Some((s, e, c)) => {
            without_codes(&format!("{}{}{}", &line[..s], code_sentence(c), &line[e..]))
        }
        None => without_codes(line),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::codes::Severity;

    #[test]
    fn every_code_a_row_can_show_has_a_sentence_without_a_code_in_it() {
        for &c in Code::ALL {
            let s = code_sentence(c);
            if c.severity() != Severity::Info {
                assert!(!s.is_empty(), "{} has no sentence", c.as_str());
            }
            assert!(
                Code::ALL.iter().all(|k| !s.contains(k.as_str())),
                "{s} names a code"
            );
        }
    }

    #[test]
    fn without_codes_drops_the_code_and_its_colon_only() {
        assert_eq!(
            without_codes("CFG-INVALID: preset 'x': alias is bad"),
            "preset 'x': alias is bad"
        );
        assert_eq!(
            without_codes("acknowledge ENC-NVTT-VERSION to use it"),
            "acknowledge to use it"
        );
        // No empty backticks, and no doubled space, where a quoted code goes.
        assert_eq!(
            without_codes("raises `CHK-RANGE` and `DIM-NPOT`."),
            "raises and ."
        );
        assert_eq!(
            first_code("see CHK-RANGE and DIM-NPOT"),
            Some(Code::ChkRange)
        );
        // A banner line that leads with a code says its sentence, then the detail.
        assert_eq!(
            banner_line("CFG-INVALID: preset 'x': bad — sdc.toml is ignored"),
            format!(
                "{}: preset 'x': bad — sdc.toml is ignored",
                code_sentence(Code::CfgInvalid)
            )
        );
        assert_eq!(banner_line("plain line"), "plain line");
        // Not a code: kept; a longer token that merely starts with a code: kept.
        assert_eq!(without_codes("NO-SUCH-CODE stays"), "NO-SUCH-CODE stays");
        assert_eq!(
            without_codes("CHK-PARALLAX-SCALE-X stays"),
            "CHK-PARALLAX-SCALE-X stays"
        );
        assert_eq!(
            without_codes("plain text — ünïcode"),
            "plain text — ünïcode"
        );
    }
}
