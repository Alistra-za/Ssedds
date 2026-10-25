# SPDX-License-Identifier: GPL-3.0-or-later
"""Input builders and visibility rules for Save DDS, derived at import by probing psdds (no rule lives here)."""
from nodes.groups import Condition
from nodes.properties.inputs import BoolInput, DropDownInput, NumberInput

from . import _psdds as P

PRESET_ID, TYPE_ID, SRGB_ID = 4, 5, 6
FMT_ID = {0: 7, 1: 8, 2: 9}
DISCARD_ID, MIPS_ID, FILTER_ID = 10, 11, 12
QUALITY_ID0 = 13  # one input per quality group: 13, 14, 15

NTYPES, NFMT = len(P.TYPES), len(P.FORMATS)
_CONTROL = {"srgb": "srgb_enabled", "filter": "filter_enabled", "quality": "quality_enabled"}
SEEDS = P.presets()


def allowed(t):
    return [f for f in range(NFMT) if P.format_allowed(t, f)]


def visibility_table():
    """{control: {(type, fmt, srgb, mips)}} probed with has_alpha = 0 (pixels are unknown when visibility is evaluated)."""
    out = {k: set() for k in _CONTROL}
    for t in range(NTYPES):
        for f in allowed(t):
            for srgb in (0, 1):
                for mips in (0, 1):
                    _, ctl = P.resolve({**P.defaults(t), "format": f, "srgb": srgb, "mips": mips}, False)
                    for k, c in _CONTROL.items():
                        if ctl[c]:
                            out[k].add((t, f, srgb, mips))
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


def mips_input(id_=MIPS_ID, texture_type=0):
    return BoolInput("Mipmaps", default=bool(P.defaults(texture_type)["mips"])).with_id(id_)


def quality_inputs():
    out = []
    for g, (fs, labels) in enumerate(QUALITY_GROUPS):
        t = next(t for t in range(NTYPES) if any(P.format_allowed(t, f) for f in fs))
        f = next(f for f in fs if P.format_allowed(t, f))
        eff, _ = P.resolve({**P.defaults(t), "format": f}, False)
        out.append(_dd("Quality", labels, eff["quality"], QUALITY_ID0 + g))
    return out


def quality_condition(g):
    fs = QUALITY_GROUPS[g][0]
    terms = []
    for t in range(NTYPES):
        ok = [f for f in fs if P.format_allowed(t, f)]
        if ok:
            terms.append(Condition.enum(TYPE_ID, t) & Condition.enum(FMT_ID[t], ok))
    c = Condition.const(False)
    for x in terms:
        c = c | x
    return custom() & c


def visible(control):
    """Table collapsed to enum sets per (type, that type's format, srgb) with the mips values it holds."""
    groups = {}
    for t, f, srgb, mips in sorted(TABLE[control]):
        groups.setdefault((t, f, srgb), []).append(mips)
    c = Condition.const(False)
    for (t, f, srgb), mips in groups.items():
        c = c | (Condition.enum(TYPE_ID, t) & Condition.enum(FMT_ID[t], f) & Condition.enum(SRGB_ID, srgb) & Condition.enum(MIPS_ID, mips))
    return custom() & c


# --- Save Normal (Tessalith) and RMAOS (Tessalith) (Task 3) ---
NORMAL = P.TYPES.index("Normal")
N_FMT_ID, N_DISCARD_ID, N_MIPS_ID, N_QUALITY_ID0, N_RMAOS_ID = 4, 5, 6, 7, 10  # quality 7, 8, 9
ROLE_LABELS = ("Roughness", "Metallic", "AO", "Specular")  # psdds role order (P.ROLES)
CHANNEL_ID0, FILL_MODE_ID0, FILL_ID0 = 10, 20, 30  # + role index
RMAOS_QUALITY_ID, RMAOS_MIPS_ID = 40, 41


def normal_format_input():
    fs = allowed(NORMAL)
    return _dd("Format", [P.FORMATS[f] for f in fs], P.defaults(NORMAL)["format"], N_FMT_ID, fs)


def normal_quality_inputs():
    """[(condition, input)] per quality group, shown while the Normal format is one of that group's allowed formats."""
    out = []
    for g, (fs, labels) in enumerate(QUALITY_GROUPS):
        ok = [f for f in fs if P.format_allowed(NORMAL, f)]
        default = P.resolve({**P.defaults(NORMAL), "format": ok[0]}, False)[0]["quality"] if ok else 0
        out.append((Condition.enum(N_FMT_ID, ok), _dd("Quality", labels, default, N_QUALITY_ID0 + g)))
    return out


def channel_input(role):
    return _dd(f"{ROLE_LABELS[role]} channel", ("R", "G", "B", "A"), 0, CHANNEL_ID0 + role)


def fill_mode_input(role):
    return _dd(f"{ROLE_LABELS[role]} fill", ("Default", "Custom"), 0, FILL_MODE_ID0 + role).with_docs(
        "Default uses the engine's neutral value for this channel; Custom uses the number below (0-255)."
    )


def fill_input(role):
    return NumberInput(f"{ROLE_LABELS[role]} value", min=0, max=255, default=0).with_id(FILL_ID0 + role)


def rmaos_quality_input():
    labels = P.quality_labels(P.FORMATS.index("BC7"))  # the rmaos profile is BC7 (label lookup only)
    return _dd("Quality", labels, P.rmaos_default_quality(), RMAOS_QUALITY_ID)


def selected(texture_type, formats_by_type, qualities_by_group):
    """(format of the shown type's dropdown, quality of the shown group); hidden values are ignored."""
    fmt = formats_by_type[texture_type]
    g = _GROUP_OF.get(fmt)
    return fmt, (P.defaults(texture_type)["quality"] if g is None else qualities_by_group[g])
