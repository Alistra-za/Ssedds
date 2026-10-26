# SPDX-License-Identifier: GPL-3.0-or-later
from __future__ import annotations

from pathlib import Path

import numpy as np
from sanic.log import logger

from api import KeyInfo, Lazy
from nodes.groups import Condition, if_group
from nodes.properties.inputs import BoolInput, DirectoryInput, ImageInput, RelativePathInput

from . import _psdds, _ui, tessalith_group

assert len(_ui.QUALITY_GROUPS) == 4, "save_dds_tessalith_node takes four quality inputs; psdds now has another count"
assert _ui.NTYPES == 3, "save_dds_tessalith_node takes one format input per texture type (3)"
_FMT = _ui.format_inputs()
_Q = _ui.quality_inputs()


@tessalith_group.register(
    schema_id="tessalith:dds:save",
    name="Save DDS (Tessalith)",
    description="Save an image as a DDS texture with Tessalith's encoder, using the same settings and bytes as Tessalith itself.",
    icon="MdSave",
    inputs=[
        ImageInput().make_lazy(),
        DirectoryInput(must_exist=False),
        RelativePathInput("Subdirectory Path")
        .make_optional()
        .with_docs("An optional subdirectory path, created if missing. Separate levels with `/`."),
        RelativePathInput("Image Name").with_docs("The file name **without** the extension. Existing files are overwritten."),
        _ui.preset_input().with_docs(
            "Custom shows every setting. A preset hides them and uses that preset's settings."
        ),
        if_group(_ui.custom())(_ui.type_input()),
        if_group(_ui.visible("srgb"))(_ui.srgb_input()),
        *[if_group(Condition.enum(_ui.TYPE_ID, t) & _ui.custom())(_FMT[t]) for t in range(len(_FMT))],
        if_group(_ui.visible("discard"))(_ui.discard_input()),
        if_group(_ui.alpha_mode_visible())(_ui.alpha_mode_input()),
        if_group(_ui.custom())(_ui.mips_input()),
        if_group(Condition.bool(_ui.MIPS_ID, True) & _ui.visible("filter"))(_ui.filter_input()),
        *[if_group(_ui.quality_condition(g))(_Q[g]) for g in range(len(_Q))],
        _ui.hdr_input(),
        BoolInput("Skip existing files", default=False).with_id(1000).with_docs("If enabled, existing files are not overwritten."),
    ],
    outputs=[],
    key_info=KeyInfo.enum(_ui.TYPE_ID),
    side_effects=True,
)
def save_dds_tessalith_node(
    lazy_image: Lazy[np.ndarray],
    base_directory: Path,
    relative_path: str | None,
    filename: str,
    preset: int,
    texture_type: int,
    srgb: bool,
    fmt_colour: int,
    fmt_normal: int,
    fmt_mask: int,
    discard: bool,
    alpha_is_transparency: bool,
    mips: bool,
    mip_filter: int,
    quality_0: int,
    quality_1: int,
    quality_2: int,
    quality_3: int,
    linear_hdr_input: bool,
    skip_existing_files: bool,
) -> None:
    if relative_path and relative_path != ".":
        base_directory = base_directory / relative_path
    full_path = (base_directory / f"{filename}.dds").resolve()

    if full_path.exists():
        if skip_existing_files:
            logger.debug(f"Skipping existing file: {full_path}")
            return

    img = lazy_image.value
    fmt, quality = _ui.selected(
        texture_type, {0: fmt_colour, 1: fmt_normal, 2: fmt_mask}, {0: quality_0, 1: quality_1, 2: quality_2, 3: quality_3}
    )
    raw = {
        **_psdds.defaults(texture_type),
        "format": fmt,
        "texture_type": texture_type,
        "srgb": int(srgb),
        "filter": mip_filter,
        "mips": int(mips),
        "quality": quality,
        "discard_alpha": int(discard),
        "alpha_mode": int(alpha_is_transparency),
    }
    settings = _ui.settings_for(preset, raw)
    depth = _ui.depth_for(linear_hdr_input)
    data = _psdds.encode(img, settings, depth)  # PsddsError (a RuntimeError) carries psdds's text untrimmed
    if logger.isEnabledFor(10):  # DEBUG
        has_alpha = not settings["discard_alpha"] and _psdds.alpha_used(_psdds.to_rgba(img))
        eff, _, _ = _psdds.resolve_depth(settings, has_alpha, depth)
        logger.debug(f"Tessalith DDS effective settings (has_alpha {has_alpha}): {eff}")
    full_path.parent.mkdir(parents=True, exist_ok=True)
    _psdds.write_files([(full_path, data)])
