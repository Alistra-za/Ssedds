//! Mip and resize filters (spec §5.5, DECISIONS S-mips-2).
use crate::config::{Category, MipFilter, Profile};

const NOT_SRGB_COLOUR: &str =
    "tent, Mitchell and Kaiser are for sRGB colour only; this texture uses box (spec §5.5)";
const ALPHA_WEIGHTED: &str =
    "alpha-tested and blended colour uses the alpha-weighted box (spec §5.5)";

/// Why `p.mips.filter` cannot be used for this profile (S-mips-2), or Ok. Box is always allowed. Tent, Mitchell
/// and Kaiser need sRGB colour without alpha weighting; an HDR format (`is_hdr()`) on a colour or HDR sub-type
/// without alpha weighting takes tent only.
pub fn filter_allowed(p: &Profile) -> Result<(), &'static str> {
    use crate::ops::{domain_for, Domain};
    let f = p.mips.filter;
    if f == MipFilter::Box {
        return Ok(());
    }
    let domain = domain_for(p);
    if domain == Domain::Hdr {
        if f != MipFilter::Tent {
            return Err("HDR mips use box or tent (spec §5.5)");
        }
        if !matches!(p.category, Category::Color | Category::Hdr) {
            return Err(NOT_SRGB_COLOUR);
        }
    } else if p.category != Category::Color || domain != Domain::LinearLight {
        return Err(NOT_SRGB_COLOUR);
    }
    if p.alpha_weighted_mips() {
        return Err(ALPHA_WEIGHTED);
    }
    Ok(())
}
