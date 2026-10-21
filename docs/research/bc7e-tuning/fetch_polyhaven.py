#!/usr/bin/env python3
"""Build the bc7e tuning corpus from Poly Haven (CC0) 1k textures.

Layout written under <root>:
  diffuse/<id>.png   Diffuse 1k, alpha 255
  normal/<id>.png    nor_dx 1k (DirectX green, as Skyrim stores it), alpha 255
  cutout/<id>.png    Diffuse RGB + alpha from the asset's Alpha/Opacity map when it has one, else from its
                     own Displacement or AO map thresholded at its median and blurred one texel (anti-aliased)
  smooth/<id>.png    Diffuse RGB + alpha = the asset's Displacement or AO map (a smooth 0..255 ramp)
and <root>/MANIFEST.tsv: class, id, source map, source URL, md5 (Poly Haven's), sha256 of the downloaded
file, sha256 of the PNG written. Every source is CC0 (https://polyhaven.com/license).

usage: fetch_polyhaven.py <root> [--diffuse 35 --normal 35 --cutout 15 --smooth 15] [--size 1k]
"""
import hashlib
import io
import json
import os
import sys
import time
import urllib.request

import numpy as np
from PIL import Image, ImageFilter

API = "https://api.polyhaven.com"


def get(url, retries=4):
    last = None
    for i in range(retries):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "tessalith-bc7e-study/1.0"}), timeout=120) as r:
                return r.read()
        except Exception as e:  # noqa: BLE001
            last = e
            time.sleep(2 * (i + 1))
    raise last


def sha256(b):
    return hashlib.sha256(b).hexdigest()


def pick(files, key, size, fmt="png"):
    m = files.get(key)
    if not m or size not in m:
        return None
    f = m[size].get(fmt) or m[size].get("jpg")
    return f


def to_rgba8(img):
    img = img.convert("RGBA")
    return np.asarray(img, dtype=np.uint8).copy()


def main():
    args = sys.argv[1:]
    root = args[0]
    want = {"diffuse": 35, "normal": 35, "cutout": 15, "smooth": 15}
    size = "1k"
    i = 1
    while i < len(args):
        a = args[i]
        if a == "--size":
            size = args[i + 1]
            i += 2
            continue
        k = a.lstrip("-")
        if k in want:
            want[k] = int(args[i + 1])
            i += 2
            continue
        i += 1
    for c in want:
        os.makedirs(os.path.join(root, c), exist_ok=True)
    assets = json.loads(get(f"{API}/assets?type=textures"))
    ids = sorted(assets)
    print(f"{len(ids)} texture assets listed")
    manifest = []
    counts = {c: 0 for c in want}
    families = set()

    def family(i):
        # One asset per material family (strip trailing _0N / _N).
        parts = i.rsplit("_", 1)
        return parts[0] if len(parts) == 2 and parts[1].isdigit() else i

    cache = {}

    def fetch(entry):
        url = entry["url"]
        if url not in cache:
            data = get(url)
            cache[url] = data
        return cache[url]

    for aid in ids:
        if all(counts[c] >= want[c] for c in want):
            break
        fam = family(aid)
        if fam in families:
            continue
        try:
            files = json.loads(get(f"{API}/files/{aid}"))
        except Exception as e:  # noqa: BLE001
            print(f"skip {aid}: {e}")
            continue
        diff = pick(files, "Diffuse", size)
        nor = pick(files, "nor_dx", size)
        disp = pick(files, "Displacement", size) or pick(files, "AO", size)
        alpha_map = None
        for key in ("Alpha", "Opacity", "alpha", "opacity"):
            alpha_map = pick(files, key, size)
            if alpha_map:
                alpha_key = key
                break
        wrote = False
        # Decide this asset's role: fill the class that still needs the most.
        order = sorted(want, key=lambda c: counts[c] / want[c])
        for cls in order:
            if counts[cls] >= want[cls]:
                continue
            if cls == "diffuse" and diff:
                data = fetch(diff)
                rgba = to_rgba8(Image.open(io.BytesIO(data)))
                rgba[..., 3] = 255
                src = ("Diffuse", diff)
            elif cls == "normal" and nor:
                data = fetch(nor)
                rgba = to_rgba8(Image.open(io.BytesIO(data)))
                rgba[..., 3] = 255
                src = ("nor_dx", nor)
            elif cls in ("cutout", "smooth") and diff and (disp or alpha_map):
                data = fetch(diff)
                rgba = to_rgba8(Image.open(io.BytesIO(data)))
                if alpha_map and cls == "cutout":
                    adata = fetch(alpha_map)
                    a = np.asarray(Image.open(io.BytesIO(adata)).convert("L"), dtype=np.uint8)
                    src = (alpha_key, alpha_map)
                    data = data + adata
                else:
                    adata = fetch(disp)
                    g = Image.open(io.BytesIO(adata)).convert("L")
                    if cls == "cutout":
                        arr = np.asarray(g, dtype=np.float32)
                        mask = Image.fromarray(((arr > np.median(arr)) * 255).astype(np.uint8))
                        a = np.asarray(mask.filter(ImageFilter.GaussianBlur(0.8)), dtype=np.uint8)
                    else:
                        a = np.asarray(g, dtype=np.uint8)
                    src = (("Displacement" if pick(files, "Displacement", size) else "AO") + (" thresholded+blurred" if cls == "cutout" else ""), disp)
                    data = data + adata
                if a.shape != rgba.shape[:2]:
                    continue
                rgba[..., 3] = a
            else:
                continue
            if rgba.shape[0] % 4 or rgba.shape[1] % 4:
                continue
            out = os.path.join(root, cls, f"{aid}.png")
            Image.fromarray(rgba, "RGBA").save(out, compress_level=6)
            with open(out, "rb") as f:
                outsha = sha256(f.read())
            manifest.append((cls, aid, src[0], src[1]["url"], src[1].get("md5", ""), sha256(data), outsha))
            counts[cls] += 1
            families.add(fam)
            wrote = True
            print(f"{cls:8} {aid:32} {rgba.shape[1]}x{rgba.shape[0]}  {counts}")
            break
        if not wrote:
            continue
    with open(os.path.join(root, "MANIFEST.tsv"), "w") as f:
        f.write("class\tid\tsource_map\turl\tpolyhaven_md5\tsha256_download\tsha256_png\n")
        for row in manifest:
            f.write("\t".join(row) + "\n")
    print("done", counts)


if __name__ == "__main__":
    main()
