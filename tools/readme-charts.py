"""Writes the README and wiki bar charts to docs/img/*.svg. Stdlib only.

Run from the repo root:  python -I tools/readme-charts.py
Data below is copied from the README encoder tables and from the local study
"BC7 alpha weighting vs BC3" (2026-10-21, DECISIONS Note CP). None = not measured.
"""
import os
from xml.sax.saxutils import escape

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "docs", "img")
M = "−"  # minus sign, as in the tables

# Colour follows the encoder in every chart. Light / dark hex per slot.
# Order passes the dataviz validator on adjacent pairs, light and dark.
SLOTS = [("#2a78d6", "#3987e5"), ("#eb6834", "#d95926"), ("#1baf7a", "#199e70"),
         ("#6250d6", "#9085e9"), ("#e87ba4", "#d55181")]
BLUE, ORANGE, AQUA, VIOLET, MAGENTA = range(5)

STYLE = """
svg{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif;
--ink:#1f2328;--mute:#59636e;--grid:#d1d9e0;--bg:#ffffff;%s}
@media (prefers-color-scheme: dark){svg{--ink:#e6edf3;--mute:#9198a1;--grid:#30363d;--bg:#0d1117;%s}}
.bg{fill:var(--bg)}
text{fill:var(--ink);font-size:12px;paint-order:stroke;stroke:var(--bg);stroke-width:4px;stroke-linejoin:round}
.m{fill:var(--mute)}
.b{font-weight:600}
.t{font-size:14px;font-weight:600}
.s{font-size:11px;fill:var(--mute)}
.g{stroke:var(--grid);stroke-width:1;fill:none}
.z{stroke:var(--mute);stroke-width:1;fill:none}
.lt{opacity:.5}
.mu{fill:var(--mute)}
""" % (
    "".join("--c%d:%s;" % (i, s[0]) for i, s in enumerate(SLOTS)),
    "".join("--c%d:%s;" % (i, s[1]) for i, s in enumerate(SLOTS)),
) + "".join(".f%d{fill:var(--c%d)}" % (i, i) for i in range(len(SLOTS)))


def svg(w, h, title, desc, body):
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 %d %d" width="%d" height="%d" '
        'role="img" aria-labelledby="t d">\n<title id="t">%s</title>\n<desc id="d">%s</desc>\n'
        "<style>%s</style>\n<rect class=\"bg\" width=\"%d\" height=\"%d\"/>\n%s</svg>\n"
        % (w, h, w, h, escape(title), escape(desc), STYLE, w, h, body)
    )


def text(x, y, s, cls="", anchor="start"):
    return '<text x="%.1f" y="%.1f" class="%s" text-anchor="%s">%s</text>\n' % (
        x, y, cls, anchor, escape(s))


def num(v, signed=False, dec=2):
    s = "%.*f" % (dec, abs(v))
    if v < 0:
        return M + s
    return ("+" + s) if signed and v > 0 else s


def hbars(y0, rows, nbars, x0, x1, vmin, vmax, base, ticks, label_x=10,
          bh=10, gap=12, tick_labels=True, fmt=None, axis_title=None):
    """Horizontal bars from `base`. rows: dict(name, bold, vals[], labels[], colors[],
    light[] (pale bar), right (text in the right column), right_x).
    Returns (svg, y after the last row + axis)."""
    def sx(v):
        return x0 + (v - vmin) / (vmax - vmin) * (x1 - x0)

    rh = nbars * bh + (nbars - 1) * 2 + gap
    top, bot = y0, y0 + rh * len(rows)
    b = ""
    for t in ticks:
        b += '<path class="g" d="M%.1f %.1fV%.1f"/>\n' % (sx(t), top, bot)
        if tick_labels:
            lab = (M + str(abs(t))) if t < 0 else str(t)
            b += text(sx(t), bot + 15, lab, "m", "middle")
    if vmin < base < vmax or base == 0:
        b += '<path class="z" d="M%.1f %.1fV%.1f"/>\n' % (sx(base), top, bot)
    for i, r in enumerate(rows):
        ry = top + i * rh + gap / 2
        b += text(label_x, ry + (nbars * bh) / 2 + 4, r["name"], "b" if r.get("bold") else "")
        for k in range(nbars):
            v, lab = r["vals"][k], r["labels"][k]
            y = ry + k * (bh + 2)
            if v is None:
                b += text(sx(base) + 6, y + bh - 1, lab, "m")
                continue
            cls = "f%d%s" % (r["colors"][k], " lt" if r.get("light", [0] * nbars)[k] else "")
            xa, xb = sorted((sx(base), sx(v)))
            if xb - xa > 0.5:
                b += '<rect x="%.1f" y="%.1f" width="%.1f" height="%d" class="%s"/>\n' % (
                    xa, y, xb - xa, bh, cls)
            if v < base:
                b += text(xa - 5, y + bh - 1, lab, "s" if nbars > 1 else "", "end")
            else:
                b += text(xb + 5, y + bh - 1, lab, "s" if nbars > 1 else "", "start")
        if r.get("right"):
            b += text(r["right_x"], ry + (nbars * bh) / 2 + 4, r["right"], "")
    return b, bot


# ------------------------------------------------------- chart 1: BC7 encoders
def chart_bc7():
    # name, speed (s/Mpix), dRGB (real), dAlpha (real), encoder colour; copied from the table
    T = [
        ("bc7e max‡ (Tessalith opt-in)", "~0.6", 0.32, 0.32, BLUE, "+0.32", "+0.32"),
        ("bc7e u5 (Tessalith default)", "0.22", 0.01, -0.03, BLUE, "+0.01", M + "0.03"),
        ("bc7e u6 (reference)", "0.28", 0.0, 0.0, BLUE, "0", "0"),
        ("Compressonator q1.0", "~33", -0.16, -1.50, VIOLET, M + "0.16", M + "1.50"),
        ("NVTT Production/Highest", "~14", -0.32, -0.37, AQUA, M + "0.32", M + "0.37"),
        ("bc7e u4", "0.12", -0.39, -0.36, BLUE, M + "0.39", M + "0.36"),
        ("ISPC slow", "0.30", -0.59, -0.67, ORANGE, M + "0.59", M + "0.67"),
        ("ISPC basic", "0.11", -0.75, -0.69, ORANGE, M + "0.75", M + "0.69"),
        ("NVTT Normal (GPU)", "0.03–0.09", -1.4, -0.75, AQUA, M + "1.4", M + "0.75"),
        ("texconv CPU (-bc x)", "~16–20", -1.6, -2.4, MAGENTA, M + "1.6", M + "2.4"),
        ("texconv GPU", "0.03–0.09", -1.8, -2.8, MAGENTA, M + "1.8", M + "2.8"),
    ]
    assert [r[2] for r in T] == sorted((r[2] for r in T), reverse=True)  # sorted by RGB
    rows = [dict(name=n, bold=n.startswith("bc7e max"), vals=[d, a], labels=[dl, al],
                 colors=[c, c], light=[0, 1], right=sp, right_x=650)
            for n, sp, d, a, c, dl, al in T]
    head = 96
    b = text(10, 20, "BC7 encoders: quality against bc7e u6, with speed", "t")
    b += text(10, 38, "Δ in dB against bc7e u6 (higher is better), sorted by RGB. "
              "Speed is in seconds per megapixel.", "s")
    b += '<rect x="10" y="50" width="14" height="10" class="mu"/>\n'
    b += text(30, 59, "RGB, real colour textures", "s")
    b += '<rect x="196" y="50" width="14" height="10" class="mu lt"/>\n'
    b += text(216, 59, "Alpha, real alpha textures", "s")
    b += text(650, 82, "Speed (s/Mpix)", "s")
    body, bot = hbars(head, rows, 2, 235, 585, -3.4, 0.6, 0, [-3, -2, -1, 0], tick_labels=True)
    b += body
    b += text(10, bot + 40, "‡ bc7e max is Tessalith's opt-in effort (encoder.effort = \"max\"). "
              "Measured later, on a 150-texture Skyrim sample; its Δ is against u6 on that sample.",
              "s")
    b += text(10, bot + 56, "~ means approximate. The 0.03–0.09 s range is shared by "
              "NVTT Normal (GPU) and texconv GPU.", "s")
    desc = ("Horizontal bars of BC7 encoder quality against bc7e u6, in dB, sorted by RGB. "
            "Tessalith's opt-in bc7e max is best at +0.32 dB RGB and +0.32 dB alpha, about 0.6 s "
            "per megapixel. bc7e u5, the default, is level with u6 at 0.22 s. Slow encoders "
            "(NVTT Production about 14 s, Compressonator about 33 s, texconv CPU 16 to 20 s) "
            "do not beat it. GPU encoders are fastest, 0.03 to 0.09 s, but lose 1.4 to 2.8 dB.")
    return svg(780, bot + 66, "BC7 encoders compared with bc7e u6", desc, b)


# ------------------------------------------------- chart 2: other formats, dB
def chart_formats():
    E = [("rgbcx", BLUE), ("NVTT 3", AQUA), ("Compressonator", VIOLET),
         ("Intel ISPC", ORANGE), ("texconv (CPU)", MAGENTA)]
    NM, NMD = "not measured", "not measured†"
    G = [  # title, values in encoder order, labels (copied from the table)
        ("BC1 colour (RGB PSNR)", [39.85, 39.74, 39.05, 38.11, 37.41]),
        ("BC3 colour + alpha (premultiplied PSNR)", [49.20, 49.29, 48.75, 48.02, 48.07]),
        ("BC4 masks (PSNR)", [45.59, 45.05, 45.19, 43.70, 43.94]),
        ("BC6H HDR (multi-exposure PSNR)", [None, 48.00, None, 50.38, 49.62]),
    ]
    b = text(10, 20, "Best encoder per format, PSNR in dB", "t")
    b += text(10, 38, "Higher is better. Bars start at 35 dB, not zero.", "s")
    y = 56
    x0, x1, vmin, vmax = 150, 560, 35, 51
    sections = ""
    start = y
    for title, vals in G:
        sections += text(10, y + 10, title, "b")
        rows = []
        for (n, c), v in zip(E, vals):
            lab = NMD if (v is None and n == "Compressonator") else NM if v is None else "%.2f" % v
            rows.append(dict(name=n, vals=[v], labels=[lab], colors=[c]))
        body, bot = hbars(y + 18, rows, 1, x0, x1, vmin, vmax, vmin, [35, 40, 45, 50],
                          bh=14, gap=6, tick_labels=False)
        sections += body
        y = bot + 18
    # gridlines come from each group; one tick row under the last group
    for t in (35, 40, 45, 50):
        sections += text(x0 + (t - vmin) / (vmax - vmin) * (x1 - x0), y - 4, str(t), "m", "middle")
    b += sections
    b += text(10, y + 20, "† Original 4 HDRIs only; Compressonator's mPSNR there was 1.2 dB "
              "below ISPC.", "s")
    b += text(10, y + 36, "BC5 normals and BC6H highlight share are not dB; the full table "
              "is in the wiki (Research).", "s")
    desc = ("Grouped horizontal bars of PSNR in dB for BC1, BC3, BC4 and BC6H, one bar per "
            "encoder. rgbcx is best on BC1 (39.85) and BC4 (45.59), and within 0.09 dB of NVTT 3 "
            "on BC3 (49.20 against 49.29). Intel ISPC is best on BC6H (50.38). rgbcx and "
            "Compressonator BC6H values were not measured. texconv CPU is last or near last.")
    return svg(780, y + 46, "Encoders compared by format, PSNR", desc, b)


# ------------------------------------- chart 3: BC3 against BC7 alpha weights
def chart_alpha_weights():
    # Names use the GUI dropdown labels where the setting is offered (Note CP1).
    NAMES = ["BC3 (rgbcx L18 -r)", "Linear (w1, default)", "Alpha ×2 (w2)",
             "w4 (study only)", "Alpha ×8 (w8)", "Alpha ×16 (w16)", "Perceptual"]
    # (RGB PSNR, alpha PSNR) means, copied from the study's Results tables
    CLASSES = [
        ("Colour with smooth alpha", "alpha_real, 96 textures",
         [(41.00, 47.24), (48.40, 47.93), (47.74, 49.39), (47.13, 50.56),
          (46.60, 51.40), (46.17, 51.93), (46.54, 51.17)]),
        ("Normal map with specular in alpha", "n_spec, 30 textures. RGB is the stored normal.",
         [(33.47, 46.06), (37.54, 37.24), (36.79, 39.28), (36.00, 41.39),
          (35.22, 43.36), (34.53, 44.96)]),
        ("Packed rmaos with specular in alpha", "rmaos_spec, 24 textures",
         [(38.64, 58.98), (45.27, 49.92), (44.95, 51.79), (44.62, 53.36),
          (44.25, 54.74), (43.82, 56.07)]),
    ]
    b = text(10, 20, "BC3 against BC7 alpha weights: mean PSNR in dB", "t")
    b += text(10, 38, "Higher is better. Bars start at 30 dB, not zero. "
              "Perceptual was measured on the first class only.", "s")
    b += '<rect x="10" y="48" width="14" height="10" class="f0"/>\n' + text(30, 57, "RGB", "s")
    b += '<rect x="76" y="48" width="14" height="10" class="f1"/>\n' + text(96, 57, "Alpha", "s")
    y = 76
    x0, x1, vmin, vmax = 235, 655, 30, 60
    for title, sub, data in CLASSES:
        b += text(10, y + 10, title, "t") + text(10 + 8 + len(title) * 7.3, y + 10, sub, "s")
        rows = [dict(name=NAMES[i], bold=(i == 0), vals=[r, a], labels=["%.2f" % r, "%.2f" % a],
                     colors=[BLUE, ORANGE]) for i, (r, a) in enumerate(data)]
        body, bot = hbars(y + 20, rows, 2, x0, x1, vmin, vmax, vmin, [30, 40, 50, 60])
        b += body
        y = bot + 40
    b += text(10, y - 8, "Local study, 2026-10-21 (DECISIONS Note CP). Alpha ×8 is the "
              "BC3 replacement the study found for smooth alpha.", "s")
    desc = ("Grouped bars of mean RGB and alpha PSNR for BC3 and BC7 with alpha weights, in three "
            "texture classes. On colour with smooth alpha, Alpha x8 gives 51.40 dB alpha against "
            "47.24 for BC3 and keeps 46.60 dB RGB against 41.00. On a normal map or packed rmaos "
            "with specular in alpha, BC3 keeps the highest alpha, 46.06 and 58.98 dB, and BC7 "
            "alpha rises with the weight.")
    return svg(780, y + 6, "BC3 against BC7 alpha weights", desc, b)


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for name, fn in [("encoder-bc7", chart_bc7), ("encoder-formats", chart_formats),
                     ("bc7-alpha-weights", chart_alpha_weights)]:
        path = os.path.join(OUT, name + ".svg")
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(fn())
        print("wrote", os.path.normpath(path))
