# SPDX-License-Identifier: GPL-3.0-or-later
"""Save Normal body and the RMAOS job (no chaiNNer imports; every rule lives in psdds)."""
from __future__ import annotations

import logging
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Optional

import numpy as np

from . import _psdds

logger = logging.getLogger("sanic.root")  # the object chaiNNer's `sanic.log.logger` is

RMAOS_TYPE = "TessalithRmaos"
RMAOS_TYPE_DEF = "struct TessalithRmaos;"
NORMAL = _psdds.TYPES.index("Normal")
CHANNEL = (2, 1, 0, 3)  # dropdown R, G, B, A -> index in chaiNNer's BGR(A) array (layout fact)
# fill shown for an unconnected role: the engine's neutral defaults (pack::default_for), so the bytes match a -1 fill.
# Roughness is required, so it has none.
FILL_DEFAULTS = (None, 0, 255, 255)


@dataclass(frozen=True)
class RmaosJob:
    sources: tuple  # 4 x (array as chaiNNer gives it, channel index) or None; roles in psdds order
    fills: tuple  # 4 x int, -1 = engine default
    quality: int
    mips: bool


def sources(images, picks) -> tuple:
    """A grey image (HxW or HxWx1) has only channel 0 (its dropdown is hidden); no copy, no BGR->RGB for sources."""
    return tuple(
        None if img is None else (img, 0 if img.ndim == 2 or img.shape[2] == 1 else CHANNEL[pick])
        for img, pick in zip(images, picks)
    )


def fills(srcs, numbers) -> tuple:
    """A connected role's fill input is hidden, so ignored."""
    return tuple(-1 if s is not None else int(n) for s, n in zip(srcs, numbers))


def normal_settings(fmt: int, mips: bool, quality: int, discard: bool) -> dict:
    return {**_psdds.defaults(NORMAL), "format": fmt, "srgb": 0, "mips": int(mips), "quality": quality,
            "discard_alpha": int(discard)}


def save_normal(
    normal: Callable[[], np.ndarray],
    settings: dict,
    rmaos: Optional[Callable[[], RmaosJob]],
    directory: Path,
    name: str,
    skip_existing: bool,
) -> list:
    """Writes <name>_n.dds, plus <name>_rmaos.dds when rmaos is given. normal/rmaos are thunks so a skip evaluates
    nothing. Returns the paths written ([] when skipped)."""
    n_path = (directory / f"{name}_n.dds").resolve()
    rm_path = (directory / f"{name}_rmaos.dds").resolve()
    targets = [n_path] if rmaos is None else [rm_path, n_path]
    if skip_existing and all(p.exists() for p in targets):
        logger.debug(f"Skipping existing file(s): {', '.join(map(str, targets))}")
        return []

    img = normal()
    if rmaos is None:
        data = _psdds.encode(img, settings)
        n_path.parent.mkdir(parents=True, exist_ok=True)  # only after a successful encode
        _psdds.write_files([(n_path, data)])
        return [n_path]

    job = rmaos()
    # the pair's _n is the pbr_normal profile and alpha-free; discard has no effect there (shared R19)
    rm, nm, defaulted, baked, note = _psdds.rmaos_encode(
        list(job.sources), list(job.fills), job.quality, job.mips, normal=img,
        normal_settings={**settings, "discard_alpha": 0},
    )
    n_path.parent.mkdir(parents=True, exist_ok=True)
    _psdds.write_files([(rm_path, rm), (n_path, nm)])
    logger.info(f"Tessalith RMAOS {rm_path.name}: baked {baked}")
    if note:
        logger.info(f"Tessalith RMAOS {rm_path.name}: {note}")
    if defaulted:
        logger.info(f"Tessalith RMAOS {rm_path.name}: PACK-DEFAULTED {''.join(defaulted)}")
    return [rm_path, n_path]
