# SPDX-License-Identifier: GPL-3.0-or-later
"""Input builders and visibility rules for Save DDS, derived at import by probing psdds (no rule lives here)."""
from nodes.groups import Condition
from nodes.properties.inputs import BoolInput, DropDownInput, NumberInput

from . import _psdds as P
from ._pbr import FILL_DEFAULTS

PRESET_ID, TYPE_ID, SRGB_ID = 4, 5, 6
FMT_ID = {0: 7, 1: 8, 2: 9}
DISCARD_ID, MIPS_ID, FILTER_ID = 10, 11, 12
QUALITY_ID0 = 13  # one input per quality group: 13, 14, 15, 16 (BC6H)
HDR_ID = 17  # "Linear HDR input" (depth 32 when on, 16 when off)
ALPHA_MODE_ID = 18  # "Alpha is transparency" (Note DH)
DEPTHS = (16, 32)  # chaiNNer images are float32: 16 = sRGB-coded 0..1, 32 = linear light

NTYPES, NFMT = len(P.TYPES), len(P.FORMATS)
_CONTROL = {"srgb": "srgb_enabled", "filter": "filter_enabled", "quality": "quality_enabled", "discard": None,  # None: discard_enabled
            "alpha_mode": "alpha_mode_enabled"}
# Note DH: Alpha is transparency is enabled only where alpha is kept, so it is probed with has_alpha 1 (and Discard off);
# every other control with has_alpha 0 (DE10).
_PROBE_ALPHA = {"alpha_mode": True}
SEEDS = P.presets()


def allowed(t):
    out = []
    for f in range(NFMT):
        o = {P.format_offered(t, f, d) for d in DEPTHS}
        if len(o) > 1:
            raise RuntimeError(f"psdds offers format {f} for type {t} at only one of depths {DEPTHS}; chaiNNer cannot tell them apart")
        if o == {True}:
            out.append(f)
    return out


def visibility_table():
    """{control: {(type, fmt, srgb, mips, depth)}} probed with has_alpha = 0 (pixels are unknown when visibility is evaluated),
    except _PROBE_ALPHA controls, probed with has_alpha = 1."""
    out = {k: set() for k in _CONTROL}
    for t in range(NTYPES):
        for f in allowed(t):
            for srgb in (0, 1):
                for mips in (0, 1):
                    for depth in DEPTHS:
                        s = {**P.defaults(t), "format": f, "srgb": srgb, "mips": mips}
                        probe = {a: P.resolve_depth(s, a, depth) for a in (False, True)}
                        for k, c in _CONTROL.items():
                            _, ctl, discard = probe[_PROBE_ALPHA.get(k, False)]
                            if discard if c is None else ctl[c]:
                                out[k].add((t, f, srgb, mips, depth))
    return out


TABLE = visibility_table()

# formats grouped by identical quality label lists: [(formats, labels)], label-less formats (RGBA8) have none
QUALITY_GROUPS = []
for _f in range(NFMT):
    _l = P.quality_labels(_f)
    if _l:
        for _g in QUALITY_GROUPS:
            if _g[1] == _l:
                _g[0].append(_f)
                break
        else:
            QUALITY_GROUPS.append(([_f], _l))
QUALITY_GROUPS = [(tuple(f), l) for f, l in QUALITY_GROUPS]
_GROUP_OF = {f: i for i, (fs, _) in enumerate(QUALITY_GROUPS) for f in fs}


def custom():
    return Condition.enum(PRESET_ID, 0)  # a seed preset hides every setting input


def preset_options():
    return [{"option": "Custom", "value": 0}] + [{"option": n, "value": i + 1} for i, (n, _) in enumerate(SEEDS)]


def preset_input():
    # input_type/associated_type pattern: TileSizeDropdown (nodes/properties/inputs/generic_inputs.py), int option values
    return DropDownInput("number", "Preset", preset_options(), default_value=0).with_id(PRESET_ID)


def settings_for(preset, raw):
    return raw if preset == 0 else dict(SEEDS[preset - 1][1])


def _dd(label, labels, default, id_, values=None):
    values = list(range(len(labels))) if values is None else values
    opts = [{"option": n, "value": v} for n, v in zip(labels, values)]
    return DropDownInput("number", label, opts, default_value=default).with_id(id_)


def type_input():
    return _dd("Texture type", P.TYPES, P.defaults(0)["texture_type"], TYPE_ID)


def format_inputs():
    out = []
    for t in range(NTYPES):
        fs = allowed(t)
        out.append(_dd("Format", [P.FORMATS[f] for f in fs], P.defaults(t)["format"], FMT_ID[t], fs))
    return out


def filter_input():
    return _dd("Mip filter", P.FILTERS, P.defaults(0)["filter"], FILTER_ID)


def srgb_input():
    return BoolInput("sRGB", default=bool(P.defaults(0)["srgb"])).with_id(SRGB_ID)


def discard_input(id_=DISCARD_ID):
    return BoolInput("Discard transparency", default=bool(P.defaults(0)["discard_alpha"])).with_id(id_)


def alpha_mode_input():
    return BoolInput("Alpha is transparency", default=bool(P.defaults(0)["alpha_mode"])).with_id(ALPHA_MODE_ID).with_docs(
        "Off: alpha is data (a mask), and the colour under fully transparent pixels is saved as it is. On: alpha is "
        "see-through, so that colour is replaced by the nearest visible colour and the mips are alpha-weighted. Shown only "
        "where alpha can be kept; it applies only when the image has transparent pixels."
    )


def alpha_mode_visible():
    """Where psdds can keep alpha (probed with has_alpha 1) and Discard transparency is off. Discard's own condition never
    names this input (DE20)."""
    return visible("alpha_mode") & Condition.bool(DISCARD_ID, False)


def hdr_input():
    return BoolInput("Linear HDR input", default=False).with_id(HDR_ID).with_docs(
        "Off: pixels are sRGB-coded 0..1 (a PNG), clamped to 0..1. On: pixels are linear light and values above 1 are kept "
        "for BC6H and RGBA16F (an EXR); 8-bit formats clip and convert."
    )


def depth_for(hdr):
    return DEPTHS[1] if hdr else DEPTHS[0]


def mips_input(id_=MIPS_ID, texture_type=0):
    return BoolInput("Mipmaps", default=bool(P.defaults(texture_type)["mips"])).with_id(id_)


def quality_inputs():
    out = []
    for g, (fs, labels) in enumerate(QUALITY_GROUPS):
        t = next(t for t in range(NTYPES) if any(f in allowed(t) for f in fs))
        f = next(f for f in fs if f in allowed(t))
        eff, _, _ = P.resolve_depth({**P.defaults(t), "format": f}, False, DEPTHS[0])
        out.append(_dd("Quality", labels, eff["quality"], QUALITY_ID0 + g))
    return out


def quality_condition(g):
    fs = QUALITY_GROUPS[g][0]
    terms = []
    for t in range(NTYPES):
        ok = [f for f in fs if f in allowed(t)]
        if ok:
            terms.append(Condition.enum(TYPE_ID, t) & Condition.enum(FMT_ID[t], ok))
    c = Condition.const(False)
    for x in terms:
        c = c | x
    return custom() & c


def visible(control):
    """Table collapsed per (type, that type's format), naming Mipmaps and the HDR switch only where they matter. sRGB is never
    named: chaiNNer evaluates a dropdown condition together with that dropdown's own condition, so an input naming itself
    recursed forever and crashed the editor (DE20); none of the controls here names itself either.
    psdds's rules make no control depend on sRGB; if that changes, fail at import."""
    groups = {}
    for t, f, srgb, mips, depth in TABLE[control]:
        groups.setdefault((t, f), set()).add((srgb, mips, depth))
    c = Condition.const(False)
    for (t, f), held in sorted(groups.items()):
        mips = sorted({m for _, m, _ in held})
        depths = sorted({d for _, _, d in held})
        if held != {(s, m, d) for s in (0, 1) for m in mips for d in depths}:
            raise RuntimeError(f"{control} visibility depends on sRGB or couples Mipmaps and depth (type {t}, format {f}); chaiNNer cannot express that")
        term = Condition.enum(TYPE_ID, t) & Condition.enum(FMT_ID[t], f)
        if mips != [0, 1]:
            term = term & Condition.enum(MIPS_ID, mips)
        if len(depths) == 1:
            term = term & Condition.bool(HDR_ID, depths[0] == 32)
        c = c | term
    return custom() & c


# --- Save Normal (Tessalith) and RMAOS (Tessalith) (Task 3) ---
NORMAL = P.TYPES.index("Normal")
N_FMT_ID, N_DISCARD_ID, N_MIPS_ID, N_QUALITY_ID0, N_RMAOS_ID = 4, 5, 6, 7, 10  # quality 7, 8, 9
ROLE_LABELS = ("Roughness", "Metallic", "AO", "Specular")  # psdds role order (P.ROLES)
CHANNEL_ID0, FILL_ID0 = 10, 30  # + role index; 20-23 were the Default/Custom fill dropdowns (retired)
RMAOS_QUALITY_ID, RMAOS_MIPS_ID = 40, 41


def normal_format_input():
    fs = allowed(NORMAL)
    return _dd("Format", [P.FORMATS[f] for f in fs], P.defaults(NORMAL)["format"], N_FMT_ID, fs)


def normal_quality_inputs():
    """[(condition, input)] per quality group, shown while the Normal format is one of that group's allowed formats."""
    out = []
    for g, (fs, labels) in enumerate(QUALITY_GROUPS):
        ok = [f for f in fs if f in allowed(NORMAL)]
        if not ok:
            continue  # BC6H: Normal does not offer it, and its id would collide with the RMAOS input
        default = P.resolve({**P.defaults(NORMAL), "format": ok[0]}, False)[0]["quality"]
        out.append((Condition.enum(N_FMT_ID, ok), _dd("Quality", labels, default, N_QUALITY_ID0 + g)))
    return out


def channel_input(role):
    return _dd(f"{ROLE_LABELS[role]} channel", ("R", "G", "B", "A"), 0, CHANNEL_ID0 + role)


def fill_input(role):
    return NumberInput(f"{ROLE_LABELS[role]} value", min=0, max=255, default=FILL_DEFAULTS[role]).with_id(
        FILL_ID0 + role
    ).with_docs("The value (0-255) for this channel when no image is connected.")


def rmaos_quality_input():
    labels = P.quality_labels(P.FORMATS.index("BC7"))  # the rmaos profile is BC7 (label lookup only)
    return _dd("Quality", labels, P.rmaos_default_quality(), RMAOS_QUALITY_ID)


def selected(texture_type, formats_by_type, qualities_by_group):
    """(format of the shown type's dropdown, quality of the shown group); hidden values are ignored."""
    fmt = formats_by_type[texture_type]
    g = _GROUP_OF.get(fmt)
    return fmt, (P.defaults(texture_type)["quality"] if g is None else qualities_by_group[g])
