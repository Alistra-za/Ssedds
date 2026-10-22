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
.pl{fill:var(--bg)}
.nf{fill:var(--mute)}
.df{fill:none;stroke:var(--ink);stroke-width:2}
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
    """Back-to-back bars: speed (log, grows left) | encoder | quality (linear, grows right)."""
    import math
    # name, speed (lo, hi or None), speed label, dRGB, dAlpha, colour, labels; copied from the table
    T = [
        ("bc7e max‡ (Tessalith opt-in)", (0.6, None), "~0.6", 0.32, 0.32, BLUE, "+0.32", "+0.32"),
        ("bc7e u5 (Tessalith default)", (0.22, None), "0.22", 0.01, -0.03, BLUE, "+0.01", M + "0.03"),
        ("bc7e u6 (reference)", (0.28, None), "0.28", 0.0, 0.0, BLUE, "0", "0"),
        ("Compressonator q1.0", (33, None), "~33", -0.16, -1.50, VIOLET, M + "0.16", M + "1.50"),
        ("NVTT Production/Highest", (14, None), "~14", -0.32, -0.37, AQUA, M + "0.32", M + "0.37"),
        ("bc7e u4", (0.12, None), "0.12", -0.39, -0.36, BLUE, M + "0.39", M + "0.36"),
        ("ISPC slow", (0.30, None), "0.30", -0.59, -0.67, ORANGE, M + "0.59", M + "0.67"),
        ("ISPC basic", (0.11, None), "0.11", -0.75, -0.69, ORANGE, M + "0.75", M + "0.69"),
        ("NVTT Normal (GPU)", (0.03, 0.09), "0.03–0.09", -1.4, -0.75, AQUA, M + "1.4", M + "0.75"),
        ("texconv CPU (-bc x)", (16, 20), "~16–20", -1.6, -2.4, MAGENTA, M + "1.6", M + "2.4"),
        ("texconv GPU", (0.03, 0.09), "0.03–0.09", -1.8, -2.8, MAGENTA, M + "1.8", M + "2.8"),
    ]
    assert [r[3] for r in T] == sorted((r[3] for r in T), reverse=True)  # sorted by RGB
    SX0, PX = 290, 62.5          # speed panel: baseline x (0.01 s) and px per decade
    QX0, QX1, QMIN, QMAX = 510, 770, -3.4, 0.9
    RH, top = 34, 112

    def spx(v):                   # x of a speed value, growing left
        return SX0 - (math.log10(v) - math.log10(0.01)) * PX

    def qx(v):
        return QX0 + (v - QMIN) / (QMAX - QMIN) * (QX1 - QX0)

    bot = top + RH * len(T)
    b = text(10, 20, "BC7 encoders: speed against quality", "t")
    b += text(10, 38, "Left: speed, s per megapixel, log scale. Shorter bar = faster. "
              "Right: Δ dB against bc7e u6. Higher is better. Sorted by RGB. "
              "Bold = Tessalith default.", "s")
    b += '<rect x="10" y="48" width="14" height="10" class="mu"/>\n' + text(30, 57, "Speed, or RGB", "s")
    b += '<rect x="130" y="48" width="14" height="10" class="mu lt"/>\n'
    b += text(150, 57, "Range of speed, or real alpha", "s")
    b += text(SX0, 92, "Speed (s/Mpix, log scale)", "b", "end")
    b += text(QX0, 92, "Δ dB against bc7e u6", "b")
    for v in (0.01, 0.1, 1, 10, 100):
        b += '<path class="g" d="M%.1f %dV%d"/>\n' % (spx(v), top, bot)
        b += text(spx(v), bot + 15, str(v), "m", "middle")
    for v in (-3, -2, -1, 0):
        b += '<path class="%s" d="M%.1f %dV%d"/>\n' % ("z" if v == 0 else "g", qx(v), top, bot)
        b += text(qx(v), bot + 15, (M + str(abs(v))) if v < 0 else "0", "m", "middle")
    for i, (name, (lo, hi), slab, d, a, c, dl, al) in enumerate(T):
        y = top + i * RH
        cy = y + RH / 2
        b += text(302, cy + 4, name, "b" if "default" in name else "")
        # speed bar: solid to the faster end, pale from there to the slower end of a range
        near = lo
        far = hi if hi else lo
        b += '<rect x="%.1f" y="%.1f" width="%.1f" height="14" class="f%d"/>\n' % (
            spx(near), cy - 7, SX0 - spx(near), c)
        if hi:
            b += '<rect x="%.1f" y="%.1f" width="%.1f" height="14" class="f%d lt"/>\n' % (
                spx(far), cy - 7, spx(near) - spx(far), c)
        b += text(spx(far) - 5, cy + 4, slab, "s", "end")
        for k, (v, lab) in enumerate(((d, dl), (a, al))):
            by = cy - 11 + k * 12
            xa, xb = sorted((qx(0), qx(v)))
            if xb - xa > 0.5:
                b += '<rect x="%.1f" y="%.1f" width="%.1f" height="10" class="f%d%s"/>\n' % (
                    xa, by, xb - xa, c, " lt" if k else "")
            if v < 0:
                b += text(xa - 5, by + 9, lab, "s", "end")
            else:
                b += text(xb + 5, by + 9, lab, "s")
    b += text(10, bot + 40, "‡ bc7e max is Tessalith's opt-in effort (encoder.effort = \"max\"). "
              "Measured later, on a 150-texture Skyrim sample; its Δ is against u6 on that sample.",
              "s")
    b += text(10, bot + 56, "~ means approximate. Speed bars start at 0.01 s. A pale tail marks a "
              "range. The 0.03–0.09 s range is shared by NVTT Normal (GPU) and texconv GPU.", "s")
    desc = ("Back-to-back bar chart of BC7 encoders. Left bars show speed in seconds per megapixel "
            "on a log scale, shorter is faster. Right bars show quality in dB against bc7e u6. "
            "Tessalith's opt-in bc7e max is best at +0.32 dB RGB and alpha, about 0.6 s per "
            "megapixel. The default, bc7e u5 (in bold), is level with u6 at 0.22 s. NVTT Production, "
            "Compressonator and texconv CPU take 14 to 33 s and do not beat it. GPU encoders take "
            "0.03 to 0.09 s and lose 1.4 to 2.8 dB.")
    return svg(780, bot + 66, "BC7 encoders: speed against quality", desc, b)


# ------------------------------------------------- chart 2: other formats, dB
def chart_formats():
    """Groups = formats. One bar per encoder; the stripe pattern shows the encoder."""
    ENC = ["rgbcx", "NVTT 3", "Compressonator", "Intel ISPC", "texconv (CPU)"]
    PAT = [None, "pd", "ph", "px", "pv"]
    NM = "not measured"
    G = [  # format, values in ENC order (copied from the table)
        ("BC1 colour (RGB PSNR)", [39.85, 39.74, 39.05, 38.11, 37.41]),
        ("BC3 colour + alpha (premultiplied PSNR)", [49.20, 49.29, 48.75, 48.02, 48.07]),
        ("BC4 masks (PSNR)", [45.59, 45.05, 45.19, 43.70, 43.94]),
        # bench round 3, normal_real, n = 93, rg_psnr (PSNR of the stored R and G channels)
        ("BC5 normals (RG PSNR, n = 93)", [47.03, 46.08, 46.27, 44.57, 44.83]),
        ("BC6H HDR (multi-exposure PSNR)", [None, 48.00, None, 50.38, 49.62]),
    ]
    x0, x1, vmin, vmax = 30, 660, 35, 51
    bh, step = 14, 18

    def sx(v):
        return x0 + (v - vmin) / (vmax - vmin) * (x1 - x0)

    defs = (
        '<defs>'
        '<pattern id="pd" width="8" height="8" patternUnits="userSpaceOnUse" '
        'patternTransform="rotate(45)"><rect width="3.5" height="8" class="pl"/></pattern>'
        '<pattern id="ph" width="8" height="7" patternUnits="userSpaceOnUse">'
        '<rect width="8" height="3" class="pl"/></pattern>'
        '<pattern id="px" width="8" height="8" patternUnits="userSpaceOnUse">'
        '<rect width="8" height="2.4" class="pl"/><rect width="2.4" height="8" class="pl"/>'
        '</pattern>'
        '<pattern id="pv" width="8" height="8" patternUnits="userSpaceOnUse">'
        '<rect width="3.5" height="8" class="pl"/></pattern></defs>\n'
    )
    b = text(10, 20, "Best encoder per format: PSNR in dB, grouped by format", "t")
    b += text(10, 38, "Higher is better. Bars start at 35 dB, not zero. The stripe shows the encoder. Bold label and outline = Tessalith default.",
              "s")
    lx = 10
    for i, name in enumerate(ENC):
        b += '<rect x="%d" y="48" width="14" height="12" class="nf"/>\n' % lx
        if PAT[i]:
            b += '<rect x="%d" y="48" width="14" height="12" fill="url(#%s)"/>\n' % (lx, PAT[i])
        b += text(lx + 20, 58, name, "s")
        lx += 20 + 7 * len(name) + 18
    top = 78
    gh = len(ENC) * step + 22
    bot = top + gh * len(G)
    for t in (35, 40, 45, 50):
        b += '<path class="g" d="M%.1f %dV%d"/>\n' % (sx(t), top, bot - 4)
        b += text(sx(t), bot + 8, str(t), "m", "middle")
    for gi, (title, vals) in enumerate(G):
        gy = top + gi * gh
        b += text(10, gy + 10, title, "b")
        for k, v in enumerate(vals):
            by = gy + 16 + k * step
            if v is None:
                lab = NM + "†" if (ENC[k] == "Compressonator" and gi == 3) else NM
                b += text(sx(vmin) + 6, by + bh - 2, lab, "s")
                continue
            w = sx(v) - sx(vmin)
            b += '<rect x="%.1f" y="%d" width="%.1f" height="%d" class="nf"/>\n' % (
                sx(vmin), by, w, bh)
            if PAT[k]:
                b += '<rect x="%.1f" y="%d" width="%.1f" height="%d" fill="url(#%s)"/>\n' % (
                    sx(vmin), by, w, bh, PAT[k])
            dflt = (k == 0 and gi < 4) or (k == 3 and gi == 4)  # Tessalith defaults: rgbcx BC1-5, ISPC BC6H
            if dflt:
                b += '<rect x="%.1f" y="%d" width="%.1f" height="%d" class="df"/>\n' % (
                    sx(vmin), by, w, bh)
            b += text(sx(v) + (7 if dflt else 5), by + bh - 2, "%.2f" % v, "b" if dflt else "s")
    b += text(10, bot + 32, "† Original 4 HDRIs only; Compressonator's mPSNR there was 1.2 dB "
              "below ISPC.", "s")
    b += text(10, bot + 48, "BC5 angle error and BC6H highlight share are not dB; the full table "
              "is in the wiki (Research).", "s")
    desc = ("Bar chart of PSNR in dB, grouped by format (BC1 RGB, BC3 premultiplied, BC4, BC5 RG, BC6H "
            "multi-exposure), one bar per encoder, with the encoder shown by a stripe pattern. "
            "rgbcx is best on BC1 (39.85) and BC4 (45.59), and within 0.09 dB of NVTT 3 on BC3 "
            "(49.20 against 49.29), and best on BC5 (47.03). Intel ISPC is best on BC6H (50.38). rgbcx and Compressonator "
            "BC6H values were not measured. texconv CPU is last or near last. The Tessalith default encoder per format "
            "(rgbcx for BC1 to BC5, Intel ISPC for BC6H) has an outline and a bold label.")
    return svg(780, bot + 58, "Encoders compared by format, PSNR", desc, defs + b)


# ------------------------------------- chart 3: BC3 against BC7 alpha weights
def chart_alpha_weights():
    # Names use the GUI dropdown labels where the setting is offered (Note CP1).
    NAMES = ["BC3 (rgbcx L18 -r)", "Linear (w1, default)", "Alpha ×2 (w2)",
             "w4 (study only)", "Alpha ×8 (w8)", "Alpha ×16 (w16)", "w32 (study only)",
             "w64 (study only)", "Perceptual"]
    # (RGB PSNR, alpha PSNR) means, copied from the study's Results tables (w32 and w64 from
    # Follow-up 3), plus note lines drawn under the class
    CLASSES = [
        ("Colour with smooth alpha", "alpha_real, 96 textures",
         [(41.00, 47.24), (48.40, 47.93), (47.74, 49.39), (47.13, 50.56),
          (46.60, 51.40), (46.17, 51.93), (45.84, 52.23), (45.58, 52.38), (46.54, 51.17)], []),
        ("Normal map with specular in alpha", "n_spec, 30 textures. RGB is the stored normal.",
         [(33.47, 46.06), (37.54, 37.24), (36.79, 39.28), (36.00, 41.39),
          (35.22, 43.36), (34.53, 44.96), (33.95, 46.02), (33.49, 46.64)],
         ["At w64 BC7 passes BC3 on mean specular, but its normals are now worse than BC3's "
          "(2.85° against 2.81° mean error)."]),
        ("Packed rmaos with specular in alpha", "rmaos_spec, 24 textures",
         [(38.64, 58.98), (45.27, 49.92), (44.95, 51.79), (44.62, 53.36),
          (44.25, 54.74), (43.82, 56.07), (43.32, 57.31), (42.77, 58.51)],
         ["Why BC3 leads on specular here: BC3 gives alpha its own half of the block (two 8-bit "
          "ends, 8 levels per block).",
          "BC7 shares one block across RGBA: its alpha either shares the colour indices (mode 6) "
          "or gets 4 levels (mode 5).",
          "These RGB channels are three unrelated maps (roughness, metal, AO), so colour needs "
          "most of BC7's bits, while the specular",
          "is smooth and near 255. BC3 keeps 91% of specular-255 texels exact against BC7's 65 "
          "to 74%, and pays 4 to 7 dB of RGB for it."]),
    ]
    b = text(10, 20, "BC3 against BC7 alpha weights: mean PSNR in dB", "t")
    b += text(10, 38, "Higher is better. Bars start at 30 dB, not zero. "
              "Perceptual was measured on the first class only. Bold = Tessalith default.", "s")
    b += '<rect x="10" y="48" width="14" height="10" class="f0"/>\n' + text(30, 57, "RGB", "s")
    b += '<rect x="76" y="48" width="14" height="10" class="f1"/>\n' + text(96, 57, "Alpha", "s")
    y = 76
    x0, x1, vmin, vmax = 235, 655, 30, 60
    for title, sub, data, notes in CLASSES:
        b += text(10, y + 10, title, "t") + text(10 + 8 + len(title) * 7.3, y + 10, sub, "s")
        rows = [dict(name=NAMES[i], bold=(i == 1), vals=[r, a], labels=["%.2f" % r, "%.2f" % a],
                     colors=[BLUE, ORANGE]) for i, (r, a) in enumerate(data)]
        body, bot = hbars(y + 20, rows, 2, x0, x1, vmin, vmax, vmin, [30, 40, 50, 60])
        b += body
        for k, line in enumerate(notes):
            b += text(10, bot + 34 + k * 15, line, "s")
        y = bot + 40 + len(notes) * 15 + (8 if notes else 0)
    b += text(10, y - 8, "Local study, 2026-10-21 and 2026-10-22 (DECISIONS Note CP). Alpha ×8 "
              "is the BC3 replacement the study found for smooth alpha.", "s")
    b += text(10, y + 7, "No weight up to ×64 beats BC3 on alpha on every texture in any class; "
              "every doubling costs 0.25 to 0.55 dB RGB.", "s")
    y += 15
    desc = ("Grouped bars of mean RGB and alpha PSNR for BC3 and BC7 with alpha weights 1 to 64, "
            "in three texture classes. On colour with smooth alpha, Alpha x8 gives 51.40 dB alpha "
            "against 47.24 for BC3 and keeps 46.60 dB RGB against 41.00. On a normal map with "
            "specular, w64 passes BC3 on alpha (46.64 against 46.06) but its normals fall behind. "
            "On packed rmaos with specular, BC3 keeps the highest alpha, 58.98 dB against 58.51 "
            "at w64, because BC3 gives alpha its own half of the block.")
    return svg(780, y + 6, "BC3 against BC7 alpha weights", desc, b)


# ------------------------------------- chart 4: SSIM and VIF, BC7 opaque colour
def chart_ssim_vif():
    # name, colour, dSSIMx1000 label, dVIFx100 label: copied from the study's section 4.1
    T = [
        ("bc7e u6 perceptual", BLUE, 1.63, 3.10, "+1.63", "+3.10"),
        ("bc7e u5", BLUE, -0.058, -0.10, M + "0.058", M + "0.10"),
        ("Compressonator q1.0 (n = 7)", VIOLET, -0.12, -0.16, M + "0.12", M + "0.16"),
        ("NVTT Production", AQUA, -0.180, -0.46, M + "0.180", M + "0.46"),
        ("ISPC slow", ORANGE, -0.28, -0.49, M + "0.28", M + "0.49"),
        ("bc7e u4", BLUE, -0.27, -0.54, M + "0.27", M + "0.54"),
        ("ISPC basic", ORANGE, -0.32, -0.62, M + "0.32", M + "0.62"),
        ("NVTT Normal (GPU)", AQUA, -0.80, -1.74, M + "0.80", M + "1.74"),
        ("texconv -bc x", MAGENTA, -0.98, -2.05, M + "0.98", M + "2.05"),
        ("texconv GPU", MAGENTA, -1.23, -2.40, M + "1.23", M + "2.40"),
    ]
    assert [r[3] for r in T] == sorted((r[3] for r in T), reverse=True)  # sorted by VIF
    b = text(10, 20, "BC7 opaque colour: SSIM and VIF against bc7e u6", "t")
    b += text(10, 38, "Mean difference over 99 images, on luma. Higher is better; 0 is bc7e u6.", "s")
    b += text(180, 66, "ΔSSIM × 10³", "b")
    b += text(520, 66, "ΔVIF × 10²", "b")
    r1 = [dict(name=n, bold=(n == "bc7e u5"), vals=[s], labels=[sl], colors=[c]) for n, c, s, v, sl, vl in T]
    r2 = [dict(name="", vals=[v], labels=[vl], colors=[c]) for n, c, s, v, sl, vl in T]
    kw = dict(bh=14, gap=8)
    p1, bot = hbars(76, r1, 1, 180, 430, -1.9, 2.0, 0, [-1, 0, 1, 2], **kw)
    p2, _ = hbars(76, r2, 1, 520, 770, -3.0, 4.2, 0, [-2, 0, 2, 4], **kw)
    b += p1 + p2
    b += text(10, bot + 40, "Bars below zero are worse than bc7e u6 (Compressonator, n = 7, is a tie). Bold = Tessalith default. "
              "The full tables are in the wiki (Research).", "s")
    desc = ("Two bar charts of the mean SSIM and VIF difference against bc7e u6 on 99 real colour "
            "textures. bc7e perceptual is the only encoder above zero, at plus 1.63 SSIM and plus "
            "3.10 VIF. bc7e u5, Compressonator, NVTT Production, ISPC and bc7e u4 are slightly "
            "below u6, and the GPU encoders and texconv are furthest below, down to minus 2.40 "
            "VIF for texconv GPU.")
    return svg(780, bot + 50, "SSIM and VIF of BC7 encoders against bc7e u6", desc, b)


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for name, fn in [("encoder-bc7", chart_bc7), ("encoder-formats", chart_formats),
                     ("bc7-alpha-weights", chart_alpha_weights),
                     ("encoder-ssim-vif", chart_ssim_vif)]:
        path = os.path.join(OUT, name + ".svg")
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(fn())
        print("wrote", os.path.normpath(path))
