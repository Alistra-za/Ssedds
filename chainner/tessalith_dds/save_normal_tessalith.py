# SPDX-License-Identifier: GPL-3.0-or-later
from __future__ import annotations

from pathlib import Path

import numpy as np

from api import BaseInput, Lazy
from nodes.groups import Condition, if_group
from nodes.properties.inputs import BoolInput, DirectoryInput, ImageInput, RelativePathInput

from . import _pbr, _ui, tessalith_group
from ._pbr import RmaosJob

_Q = _ui.normal_quality_inputs()
assert len(_Q) == 3, "save_normal_tessalith_node takes three quality inputs; psdds now has another count"


def _rmaos_input():
    i = BaseInput(_pbr.RMAOS_TYPE, "RMAOS", associated_type=RmaosJob)
    i.type_definitions = _pbr.RMAOS_TYPE_DEF  # the only declaration of this type
    return (
        i.make_optional()
        .make_lazy()
        .with_id(_ui.N_RMAOS_ID)
        .with_docs("Optional. Connect RMAOS (Tessalith) to also write `<base>_rmaos.dds` with the normal baked into its roughness mips.")
    )


# Module-level so a test can read the visibility. With RMAOS connected only Discard is hidden: psdds_rmaos_encode's
# normal_settings uses Format, Mipmaps and Quality (over the pbr_normal base) and ignores discard_alpha (R19).
INPUTS = [
    ImageInput("Normal").make_lazy(),
    DirectoryInput(must_exist=False),
    RelativePathInput("Subdirectory Path")
    .make_optional()
    .with_docs("An optional subdirectory path, created if missing. Separate levels with `/`."),
    RelativePathInput("Base Name").with_docs(
        "Writes `<base>_n.dds`, plus `<base>_rmaos.dds` with RMAOS connected. Existing files are overwritten."
    ),
    _ui.normal_format_input(),
    # shown while RMAOS is unconnected; the frontend checks conditions in its built-in scope, so no TessalithRmaos here
    if_group(~Condition.type(_ui.N_RMAOS_ID, "any"))(_ui.discard_input(_ui.N_DISCARD_ID)),
    _ui.mips_input(_ui.N_MIPS_ID, _ui.NORMAL),
    *[if_group(c)(q) for c, q in _Q],
    _rmaos_input(),
    BoolInput("Skip existing files", default=False)
    .with_id(1000)
    .with_docs("If enabled, existing files are not overwritten. With RMAOS connected, skips only when both files exist."),
]


@tessalith_group.register(
    schema_id="tessalith:dds:save_normal",
    name="Save Normal (Tessalith)",
    description=[
        "Save a normal map as `<base>_n.dds` with Tessalith's encoder (the same bytes as Save DDS with type Normal).",
        "With RMAOS (Tessalith) connected, also writes `<base>_rmaos.dds` from one engine call: the `_n` uses the "
        "TruePBR normal profile with this node's Format, Mipmaps and Quality, and the normal is baked into the "
        "roughness mips.",
    ],
    icon="MdSave",
    inputs=INPUTS,
    outputs=[],
    side_effects=True,
)
def save_normal_tessalith_node(
    lazy_normal: Lazy[np.ndarray],
    base_directory: Path,
    relative_path: str | None,
    base_name: str,
    fmt: int,
    discard: bool,
    mips: bool,
    quality_0: int,
    quality_1: int,
    quality_2: int,
    lazy_rmaos: Lazy[RmaosJob | None],
    skip_existing_files: bool,
) -> None:
    if relative_path and relative_path != ".":
        base_directory = base_directory / relative_path
    _, quality = _ui.selected(_ui.NORMAL, {_ui.NORMAL: fmt}, {0: quality_0, 1: quality_1, 2: quality_2})
    settings = _pbr.normal_settings(fmt, mips, quality, discard)
    # an unconnected optional lazy input arrives as Lazy.ready(None); test that without evaluating a connected one
    paired = not (lazy_rmaos.has_value and lazy_rmaos.value is None)
    _pbr.save_normal(
        lambda: lazy_normal.value,
        settings,
        (lambda: lazy_rmaos.value) if paired else None,
        base_directory,
        base_name,
        skip_existing_files,
    )
