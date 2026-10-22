"""Writes the README charts to docs/img/*.svg. Stdlib only.

Run from the repo root:  python -I tools/readme-charts.py
Data below is copied from the old README tables. None = "not measured".
"""
import math
import os
from xml.sax.saxutils import escape

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "docs", "img")

# Colour follows the encoder, in every chart. Light / dark hex per slot.
SLOTS = [("#2a78d6", "#3987e5"), ("#eb6834", "#d95926"), ("#1baf7a", "#199e70"),
         ("#6250d6", "#9085e9"), ("#e87ba4", "#d55181")]
BLUE, ORANGE, AQUA, VIOLET, MAGENTA = range(5)
ENC_COLOR = {
    "bc7e": BLUE, "rgbcx": BLUE, "ISPC": ORANGE, "NVTT": AQUA,
    "texconv": MAGENTA, "Compressonator": VIOLET,
}

STYLE = """
svg{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif;
--ink:#1f2328;--mute:#59636e;--grid:#d1d9e0;--bg:#ffffff;%s}
@media (prefers-color-scheme: dark){svg{--ink:#e6edf3;--mute:#9198a1;--grid:#30363d;--bg:#0d1117;%s}}
.bg{fill:var(--bg)}
text{fill:var(--ink);font-size:12px;paint-order:stroke;stroke:var(--bg);stroke-width:4px;stroke-linejoin:round}
.m{fill:var(--mute)}
.t{font-size:14px;font-weight:600}
.s{font-size:11px;fill:var(--mute)}
.g{stroke:var(--grid);stroke-width:1;fill:none}
.z{stroke:var(--mute);stroke-width:1;fill:none}
.ring{stroke:var(--bg);stroke-width:2}
""" % (
    "".join("--c%d:%s;" % (i, s[0]) for i, s in enumerate(SLOTS)),
    "".join("--c%d:%s;" % (i, s[1]) for i, s in enumerate(SLOTS)),
) + "".join(".f%d{fill:var(--c%d)}.k%d{stroke:var(--c%d);fill:none}" % (i, i, i, i)
            for i in range(len(SLOTS)))


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


def nice_ticks(lo, hi, n=4):
    raw = (hi - lo) / n
    mag = 10 ** math.floor(math.log10(raw))
    step = min((m * mag for m in (1, 2, 2.5, 5, 10) if m * mag >= raw), default=raw)
    t = math.ceil(lo / step - 1e-9) * step
    out = []
    while t <= hi + 1e-9:
        out.append(round(t, 10))
        t += step
    return out, step


def fmt_tick(v, step):
    d = 0 if step >= 1 else (1 if step >= 0.1 else 2)
    s = "%.*f" % (d, v)
    return s.replace("-", "−") if v < 0 else s


# ---------------------------------------------------------------- chart 1
def scatter_panel(y0, title, sub, pts, ymin, ymax, h=290):
    X0, W = 70, 670
    LO, HI = math.log10(0.02), math.log10(50)
    top, bot = y0 + 40, y0 + 40 + h

    def px(v):
        return X0 + (math.log10(v) - LO) / (HI - LO) * W

    def py(v):
        return top + (ymax - v) / (ymax - ymin) * h

    b = text(X0 - 60, y0 + 14, title, "t") + text(X0 - 60, y0 + 30, sub, "s")
    for v in range(math.ceil(ymin), math.floor(ymax) + 1):
        cls = "z" if v == 0 else "g"
        b += '<path class="%s" d="M%d %.1fH%d"/>\n' % (cls, X0, py(v), X0 + W)
        lab = ("+%d" % v if v > 0 else "%d" % v).replace("-", "−")
        b += text(X0 - 8, py(v) + 4, lab, "m", "end")
    for v, lab in [(0.03, "0.03"), (0.1, "0.1"), (0.3, "0.3"), (1, "1"), (3, "3"),
                   (10, "10"), (30, "30")]:
        b += '<path class="g" d="M%.1f %.1fV%.1f"/>\n' % (px(v), top, top + h)
        b += text(px(v), top + h + 16, lab, "m", "middle")
    for name, lo, hi, y, label, dx, dy, anchor, col in pts:
        c = ENC_COLOR[col]
        if hi is None:
            b += '<circle cx="%.1f" cy="%.1f" r="5" class="f%d ring"/>\n' % (px(lo), py(y), c)
            lx = px(lo)
        else:
            b += ('<path d="M%.1f %.1fH%.1f" class="k%d" stroke-width="10" '
                  'stroke-linecap="round"/>\n' % (px(lo), py(y), px(hi), c))
            lx = px(hi)
        if anchor == "start_lo":
            lx, anchor = px(lo), "start"
        elif anchor == "end" and hi is not None:
            lx = px(lo)
        b += text(lx + dx, py(y) + dy, label, "", anchor)
    return b


def chart1():
    rgb = [
        ("gpu1", 0.03, 0.09, -1.4, "NVTT Normal (GPU) −1.4", 14, 4, "start", "NVTT"),
        ("gpu2", 0.03, 0.09, -1.8, "texconv GPU −1.8", 14, 4, "start", "texconv"),
        ("ispcb", 0.11, None, -0.75, "ISPC basic −0.75", -9, 4, "end", "ISPC"),
        ("u4", 0.12, None, -0.39, "bc7e u4 −0.39", -9, 4, "end", "bc7e"),
        ("u5", 0.22, None, 0.01, "bc7e u5 +0.01", -9, 4, "end", "bc7e"),
        ("u6", 0.28, None, 0.0, "bc7e u6 0 (reference)", 10, 16, "start", "bc7e"),
        ("max", 0.6, None, 0.32, "bc7e max‡ +0.32", 10, 4, "start", "bc7e"),
        ("ispcs", 0.30, None, -0.59, "ISPC slow −0.59", 10, 4, "start", "ISPC"),
        ("nvtt", 14, None, -0.32, "NVTT Production/Highest −0.32", 0, 20, "middle", "NVTT"),
        ("tcpu", 16, 20, -1.6, "texconv CPU (-bc x) −1.6", 0, -14, "middle", "texconv"),
        ("comp", 33, None, -0.16, "Compressonator q1.0 −0.16", 6, -12, "end", "Compressonator"),
    ]
    alpha = [
        ("gpu1", 0.03, 0.09, -0.75, "NVTT Normal (GPU) −0.75", -5, 22, "start_lo", "NVTT"),
        ("gpu2", 0.03, 0.09, -2.8, "texconv GPU −2.8", 14, 4, "start", "texconv"),
        ("ispcb", 0.11, None, -0.69, "ISPC basic −0.69", -2, -13, "end", "ISPC"),
        ("u4", 0.12, None, -0.36, "bc7e u4 −0.36", -9, 4, "end", "bc7e"),
        ("u5", 0.22, None, -0.03, "bc7e u5 −0.03", -9, 4, "end", "bc7e"),
        ("u6", 0.28, None, 0.0, "bc7e u6 0 (reference)", 10, 16, "start", "bc7e"),
        ("max", 0.6, None, 0.32, "bc7e max‡ +0.32", 10, 4, "start", "bc7e"),
        ("ispcs", 0.30, None, -0.67, "ISPC slow −0.67", 10, 4, "start", "ISPC"),
        ("nvtt", 14, None, -0.37, "NVTT Production/Highest −0.37", 0, 20, "middle", "NVTT"),
        ("tcpu", 16, 20, -2.4, "texconv CPU (-bc x) −2.4", 0, -14, "middle", "texconv"),
        ("comp", 33, None, -1.50, "Compressonator q1.0 −1.50", 6, 20, "end", "Compressonator"),
    ]
    sub = "Δ vs bc7e u6 in dB (higher is better). Further right = slower."
    b = scatter_panel(0, "BC7: colour (RGB, real textures)", sub, rgb, -3, 0.5)
    b += scatter_panel(350, "BC7: real alpha", sub, alpha, -3, 0.5)
    b += text(405, 716, "Time per megapixel in seconds (log scale)", "m", "middle")
    b += text(10, 742,"‡ bc7e max is opt-in (encoder.effort = \"max\"), measured later on a "
              "150-texture sample, about 0.6 s/Mpix; its Δ is against u6 on that sample.", "s")
    b += text(10, 758,"“~” times are approximate. Ranges are drawn as bars. "
              "NVTT Normal and texconv GPU share the 0.03–0.09 range.", "s")
    desc = ("Scatter of BC7 encoders, speed against quality. The bc7e encoder is on the "
            "fast, high-quality side: bc7e u5 and u6 take about 0.2 to 0.3 s per megapixel "
            "and set the reference. Slow encoders (NVTT Production about 14, texconv CPU "
            "16 to 20, Compressonator about 33) are no better than bc7e u6 on real colour "
            "or alpha. GPU encoders are fastest but lose 1.4 to 2.8 dB.")
    return svg(780, 772, "BC7 encoders: speed against quality", desc, b)


# ------------------------------------------------------------ dot panels
def dot_panels(w, panels, cols, caption_lines, title, desc, extra_cell=None):
    PW, PH, ROWH = w // cols, 0, 24
    b = ""
    y = 8
    row_h = []
    for i in range(0, len(panels), cols):
        row_h.append(max(len(p["rows"]) for p in panels[i:i + cols]) * ROWH + 74)
    for i, p in enumerate(panels):
        r, c = divmod(i, cols)
        oy = 8 + sum(row_h[:r])
        ox = c * PW
        b += dot_panel(ox + 10, oy, PW - 20, p)
    total = 8 + sum(row_h)
    for k, ln in enumerate(caption_lines):
        b += text(10, total + 14 + k * 16, ln, "s")
    return svg(w, total + 14 + len(caption_lines) * 16, title, desc, b)


def dot_panel(x, y, w, p):
    """p: title, sub, rows [(name, value|None, label, enc)], log, zero (draw 0 line, stems)."""
    LAB, ROWH = 128, 24
    n = len(p["rows"])
    px0, pw = x + LAB, w - LAB - 62
    vals = [r[1] for r in p["rows"] if r[1] is not None]
    log = p.get("log")
    if log:
        lo, hi = math.log10(0.1), math.log10(100)
        ticks, step = [0.1, 1, 10, 100], None

        def sx(v):
            return px0 + (math.log10(v) - lo) / (hi - lo) * pw
    else:
        lo, hi = min(vals), max(vals)
        if p.get("zero"):
            lo, hi = min(lo, 0), max(hi, 0)
        if p.get("base0"):
            lo = 0
        pad = (hi - lo) * (0.3 if p.get("zero") else 0.12) or 1
        lo = lo if p.get("base0") else lo - pad
        hi = hi + pad
        ticks, step = nice_ticks(lo, hi, 3)

        def sx(v):
            return px0 + (v - lo) / (hi - lo) * pw
    b = text(x, y + 14, p["title"], "t") + text(x, y + 30, p["sub"], "s")
    top = y + 42
    bot = top + n * ROWH
    for t in ticks:
        lab = ("%g" % t) if log else fmt_tick(t, step)
        b += '<path class="g" d="M%.1f %.1fV%.1f"/>\n' % (sx(t), top, bot)
        b += text(sx(t), bot + 14, lab, "m", "middle")
    if p.get("zero"):
        b += '<path class="z" d="M%.1f %.1fV%.1f"/>\n' % (sx(0), top, bot)
    for i, (name, v, label, enc) in enumerate(p["rows"]):
        cy = top + i * ROWH + ROWH / 2
        b += text(x, cy + 4, name, "", "start")
        c = ENC_COLOR[enc]
        if v is None:
            b += text(px0 + 6, cy + 4, label, "m", "start")
            continue
        if p.get("zero"):
            b += '<path d="M%.1f %.1fH%.1f" class="k%d" stroke-width="2"/>\n' % (
                sx(0), cy, sx(v), c)
        b += '<circle cx="%.1f" cy="%.1f" r="5" class="f%d ring"/>\n' % (sx(v), cy, c)
        if p.get("zero") and v < 0:
            b += text(sx(v) - 10, cy + 4, label, "", "end")
        else:
            b += text(sx(v) + 10, cy + 4, label, "", "start")
    return b


M = "−"  # minus sign, as in the old tables


def chart2():
    E = [("bc7e -u5", "bc7e"), ("NVTT 3 Production", "NVTT"),
         ("Compressonator q1.0", "Compressonator"), ("Intel ISPC slow", "ISPC"),
         ("texconv -bc x", "texconv")]

    def rows(vals, labels):
        return [(E[i][0], vals[i], labels[i], E[i][1]) for i in range(5)]

    NM = "not measured†"
    d = "Δ vs bc7e u6, dB. Higher is better."
    panels = [
        dict(title="Real colour textures", sub=d, zero=True, rows=rows(
            [0.03, -0.27, -0.16, -0.44, -1.47],
            ["+0.03", M + "0.27", M + "0.16†", M + "0.44", M + "1.47"])),
        dict(title="Smooth gradients (synthetic)", sub=d, zero=True, rows=rows(
            [0.00, 3.86, 3.21, 0.73, -6.89],
            ["0.00", "+3.86", "+3.21", "+0.73", M + "6.89"])),
        dict(title="Real decals, smooth alpha", sub=d, zero=True, rows=rows(
            [-0.02, -0.06, -1.50, -0.58, -2.04],
            [M + "0.02", M + "0.06", M + "1.50†", M + "0.58", M + "2.04"])),
        dict(title="Packed data maps", sub="RGB PSNR, dB. Higher is better.", rows=rows(
            [41.22, 41.58, None, 41.20, 40.38],
            ["41.22", "41.58", NM, "41.20", "40.38"])),
        dict(title="Normal maps", sub="Mean angle error, degrees. Lower is better.", rows=rows(
            [1.77, 1.71, None, 1.79, 2.19],
            ["1.77°", "1.71°", NM, "1.79°", "2.19°"])),
        dict(title="Opaque input decoded with alpha 254",
             sub="Share of texels. Lower is better.", base0=True, rows=rows(
                 [28, 0, 1.1, 15, 2.4],
                 ["28%*", "0%", "1.1%", "15%", "2.4%"])),
        dict(title="Time per megapixel", sub="Seconds, log scale. Lower is faster.", log=True,
             rows=rows([0.22, 14, 33, 0.30, 20],
                       ["0.22 s", "~14 s", "~33 s", "0.30 s", "~20 s"])),
    ]
    cap = [
        "† Compressonator q1.0 was not re-measured on the wider set: its values come from the "
        "original 4–7 images.",
        "  There it was 0.15 dB behind NVTT on packed data and 0.27° behind on normals. "
        "The alpha-254 and time rows are also from the original run.",
        "* The opt-in mode 6 p-bit override (exact opaque alpha) is for when this is not acceptable.",
    ]
    desc = ("BC7 encoders at maximum settings, one panel per measure. bc7e u5 matches the best "
            "quality on real colour and decals and takes 0.22 s per megapixel. NVTT 3 Production "
            "and Compressonator score higher on synthetic smooth gradients, but take about 14 "
            "and 33 s per megapixel. texconv CPU is last on every quality measure. bc7e u5 "
            "turns 28 percent of opaque input into alpha 254, against 0 percent for NVTT. "
            "Compressonator packed-data and normal-map values were not measured.")
    return dot_panels(780, panels, 2, cap, "BC7 encoders compared by measure", desc)


def chart3():
    E = [("rgbcx", "rgbcx"), ("NVTT 3", "NVTT"), ("Compressonator", "Compressonator"),
         ("Intel ISPC", "ISPC"), ("texconv (CPU)", "texconv")]

    def rows(vals, labels):
        return [(E[i][0], vals[i], labels[i], E[i][1]) for i in range(5)]

    NM = "not measured"
    NMD = "not measured†"
    panels = [
        dict(title="BC1 colour", sub="RGB PSNR, dB. Higher is better.", rows=rows(
            [39.85, 39.74, 39.05, 38.11, 37.41], ["39.85", "39.74", "39.05", "38.11", "37.41"])),
        dict(title="BC3 colour + alpha", sub="Premultiplied PSNR, dB. Higher is better.",
             rows=rows([49.20, 49.29, 48.75, 48.02, 48.07],
                       ["49.20", "49.29", "48.75", "48.02", "48.07"])),
        dict(title="BC4 masks", sub="PSNR, dB. Higher is better.", rows=rows(
            [45.59, 45.05, 45.19, 43.70, 43.94], ["45.59", "45.05", "45.19", "43.70", "43.94"])),
        dict(title="BC5 normals", sub="Mean angle error, degrees. Lower is better.", rows=rows(
            [0.41, 0.47, 0.45, 0.55, 0.56],
            ["0.41°", "0.47°", "0.45°", "0.55°", "0.56°"])),
        dict(title="BC6H HDR", sub="Multi-exposure PSNR, dB. Higher is better.", rows=rows(
            [None, 48.00, None, 50.38, 49.62], [NM, "48.00", NMD, "50.38", "49.62"])),
        dict(title="BC6H brightest highlight kept",
             sub="Worst image, share kept. Higher is better.", base0=True, rows=rows(
                 [None, 94, 29, 22, 29], [NM, "94%", "29%†", "22%", "29%"])),
    ]
    cap = ["† Original 4 HDRIs only; Compressonator mPSNR there was 1.2 dB below ISPC."]
    desc = ("Best-effort encoders per format, one panel per measure. rgbcx is best on BC1, BC4 "
            "and BC5 and within 0.09 dB of NVTT 3 on BC3. Intel ISPC is best on BC6H "
            "multi-exposure PSNR, 50.38 dB, but NVTT 3 keeps the brightest highlight best, 94 "
            "percent against 22 percent for ISPC. texconv CPU is last or near last on BC1, "
            "BC3 and BC4. rgbcx does not do BC6H.")
    return dot_panels(780, panels, 2, cap, "Encoders compared by format", desc)


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    for name, fn in [("encoder-speed-quality", chart1), ("encoder-bc7-measures", chart2),
                     ("encoder-formats", chart3)]:
        path = os.path.join(OUT, name + ".svg")
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(fn())
        print("wrote", os.path.normpath(path))
