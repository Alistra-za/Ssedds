#!/usr/bin/env python3
"""Build the alpha-weighting classes of the bc7e tuning study from the Poly Haven (CC0) corpus assets.

Reads the existing corpus (`fetch_polyhaven.py` output, checked against MANIFEST.tsv), fetches the extra maps
each class needs through the Poly Haven API, and writes under <root>:

  n_spec/<id>.png        the 35 `normal` assets: RGB = nor_dx 1k, alpha = 255 - Rough (a vanilla `_n` with
                         specular in alpha, as 1 - roughness)
  n_spec_mip2/<id>.png   the same images at mip 2 (two 2x2 box passes on all four channels, RGB renormalised
                         to unit length as Tessalith's normal mips are)
  m_height/<id>.png      the 35 `diffuse` assets: a plausible Community Shaders complex-material `_m`:
                         R = AO (env-mask stand-in), G = 255 - Rough (glossiness), B = Metal (0 when the asset
                         has no metal map), alpha = Displacement stretched so its 1st and 99th percentiles land
                         on 0 and 255 and clipped, so real 0 and 255 regions occur (parallax height)

and <root>/MANIFEST-alpha.tsv: class, id, source maps, their URLs and Poly Haven MD5s, SHA-256 of each
download and of the PNG written. Every source is CC0 (https://polyhaven.com/license).

usage: make_alpha_classes.py <corpus root> [--manifest docs/research/bc7e-tuning/MANIFEST.tsv]
"""
import csv
import hashlib
import io
import json
import os
import sys
import time
import urllib.request

import numpy as np
from PIL import Image

API = "https://api.polyhaven.com"


def get(url, retries=4):
    last = None
    for i in range(retries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "tessalith-bc7e-study/1.0"})
            with urllib.request.urlopen(req, timeout=120) as r:
                return r.read()
        except Exception as e:  # noqa: BLE001
            last = e
            time.sleep(2 * (i + 1))
    raise last


def sha256(b):
    return hashlib.sha256(b).hexdigest()


def grey(data):
    """A single-channel map as float 0..255. Poly Haven's 1k PNGs are 8- or 16-bit; PIL's "L" conversion clips
    16-bit values, so 16-bit maps are scaled down explicitly."""
    img = Image.open(io.BytesIO(data))
    arr = np.asarray(img)
    if arr.ndim == 3:
        arr = arr[..., 0]
    if arr.dtype == np.uint16:
        return arr.astype(np.float64) / 65535.0 * 255.0
    if arr.dtype != np.uint8:
        arr = np.asarray(img.convert("I"))
        return arr.astype(np.float64) / arr.max() * 255.0 if arr.max() > 255 else arr.astype(np.float64)
    return arr.astype(np.float64)


def to_u8(f):
    return np.clip(f + 0.5, 0, 255).astype(np.uint8)


def entry(files, key, size="1k"):
    m = files.get(key)
    if not m or size not in m:
        return None
    return m[size].get("png") or m[size].get("jpg")


def mip2(rgba):
    """Two 2x2 box passes on all four channels; RGB renormalised as a unit normal."""
    f = rgba.astype(np.float64)
    for _ in range(2):
        f = (f[0::2, 0::2] + f[1::2, 0::2] + f[0::2, 1::2] + f[1::2, 1::2]) / 4.0
    v = f[..., :3] / 255.0 * 2.0 - 1.0
    n = np.linalg.norm(v, axis=-1, keepdims=True)
    n[n < 1e-9] = 1.0
    f[..., :3] = (v / n * 0.5 + 0.5) * 255.0
    return np.clip(f + 0.5, 0, 255).astype(np.uint8)


def write(path, rgba):
    Image.fromarray(rgba, "RGBA").save(path, compress_level=6)
    with open(path, "rb") as f:
        return sha256(f.read())


def main():
    root = sys.argv[1]
    manifest = "docs/research/bc7e-tuning/MANIFEST.tsv"
    if "--manifest" in sys.argv:
        manifest = sys.argv[sys.argv.index("--manifest") + 1]
    rows = list(csv.DictReader(open(manifest), delimiter="\t"))
    for c in ("n_spec", "n_spec_mip2", "m_height"):
        os.makedirs(os.path.join(root, c), exist_ok=True)
    out = []
    for r in rows:
        if r["class"] not in ("normal", "diffuse"):
            continue
        aid = r["id"]
        src = os.path.join(root, r["class"], f"{aid}.png")
        base = np.asarray(Image.open(src).convert("RGBA"), dtype=np.uint8).copy()
        with open(src, "rb") as f:
            assert sha256(f.read()) == r["sha256_png"], f"{src} does not match MANIFEST.tsv"
        files = json.loads(get(f"{API}/files/{aid}"))
        rough = entry(files, "Rough")
        if rough is None:
            print(f"skip {aid}: no Rough map")
            continue
        rough_data = get(rough["url"])
        rough_img = grey(rough_data)
        if rough_img.shape != base.shape[:2]:
            print(f"skip {aid}: Rough is {rough_img.shape}")
            continue
        maps = [("Rough", rough, rough_data)]
        if r["class"] == "normal":
            cls = "n_spec"
            rgba = base.copy()
            rgba[..., 3] = to_u8(255.0 - rough_img)
            sha = write(os.path.join(root, cls, f"{aid}.png"), rgba)
            sha_m2 = write(os.path.join(root, "n_spec_mip2", f"{aid}.png"), mip2(rgba))
            out.append((cls, aid, "nor_dx (corpus normal/) + Rough", maps, sha))
            out.append(("n_spec_mip2", aid, "n_spec at mip 2 (box, renormalised)", [], sha_m2))
            print(f"n_spec   {aid}")
        else:
            cls = "m_height"
            disp = entry(files, "Displacement")
            ao = entry(files, "AO")
            metal = entry(files, "Metal")
            if disp is None or ao is None:
                print(f"skip {aid}: needs Displacement and AO")
                continue
            disp_data = get(disp["url"])
            ao_data = get(ao["url"])
            d = grey(disp_data)
            a = grey(ao_data)
            if d.shape != base.shape[:2] or a.shape != base.shape[:2]:
                print(f"skip {aid}: map sizes differ")
                continue
            maps += [("Displacement", disp, disp_data), ("AO", ao, ao_data)]
            rgba = np.zeros_like(base)
            rgba[..., 0] = to_u8(a)
            rgba[..., 1] = to_u8(255.0 - rough_img)
            if metal is not None:
                metal_data = get(metal["url"])
                m = grey(metal_data)
                if m.shape == base.shape[:2]:
                    rgba[..., 2] = to_u8(m)
                    maps.append(("Metal", metal, metal_data))
            lo, hi = np.percentile(d, 1.0), np.percentile(d, 99.0)
            if hi <= lo:
                print(f"skip {aid}: flat displacement")
                continue
            rgba[..., 3] = to_u8((d - lo) / (hi - lo) * 255.0)
            sha = write(os.path.join(root, cls, f"{aid}.png"), rgba)
            out.append((cls, aid, "R=AO G=255-Rough B=Metal|0 A=Displacement stretched 1..99%", maps, sha))
            print(f"m_height {aid}  zeros {np.mean(rgba[..., 3] == 0):.1%} full {np.mean(rgba[..., 3] == 255):.1%}")
    with open(os.path.join(root, "MANIFEST-alpha.tsv"), "w") as f:
        f.write("class\tid\tconstruction\tsource_maps\turls\tpolyhaven_md5\tsha256_downloads\tsha256_png\n")
        for cls, aid, how, maps, sha in out:
            f.write("\t".join([
                cls, aid, how,
                " ".join(k for k, _, _ in maps),
                " ".join(e["url"] for _, e, _ in maps),
                " ".join(e.get("md5", "") for _, e, _ in maps),
                " ".join(sha256(d) for _, _, d in maps),
                sha,
            ]) + "\n")
    print("done", len(out), "rows")


if __name__ == "__main__":
    main()
