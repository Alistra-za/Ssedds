# SPDX-License-Identifier: GPL-3.0-or-later
from __future__ import annotations

import numpy as np

from api import BaseOutput
from nodes.groups import Condition, if_group
from nodes.properties.inputs import BoolInput, ImageInput

from . import _pbr, _ui, tessalith_group
from ._pbr import RmaosJob


def _channel(i):
    return if_group(Condition.type(i, "Image { channels: 3 } | Image { channels: 4 }"))(_ui.channel_input(i))


def _role(i):
    return [
        ImageInput(_ui.ROLE_LABELS[i]).make_optional().with_id(i),
        _channel(i),
        if_group(~Condition.type(i, "Image"))(_ui.fill_input(i)),
    ]


@tessalith_group.register(
    schema_id="tessalith:dds:rmaos",
    name="RMAOS (Tessalith)",
    description=[
        "Packs a Community Shaders TruePBR `_rmaos` texture: R roughness, G metallic, B AO, A specular. "
        "Each role takes any image and the channel to read from it, so one image can feed several roles. "
        "Roughness is required. "
        "A missing metallic, AO or specular map takes its value (0-255).",
        "Connect to Save Normal (Tessalith), which writes `_rmaos` beside `_n` and bakes the normal into the "
        "roughness mips. On its own this node writes nothing.",
    ],
    icon="MdLayers",
    inputs=[
        ImageInput(_ui.ROLE_LABELS[0]).with_id(0),
        _channel(0),
        *_role(1),
        *_role(2),
        *_role(3),
        _ui.rmaos_quality_input(),
        BoolInput("Mipmaps", default=True).with_id(_ui.RMAOS_MIPS_ID),
    ],
    outputs=[BaseOutput(_pbr.RMAOS_TYPE, "RMAOS", associated_type=RmaosJob)],
    side_effects=False,
)
def rmaos_tessalith_node(
    roughness: np.ndarray,
    roughness_channel: int,
    metallic: np.ndarray | None,
    metallic_channel: int,
    metallic_fill: int,
    ao: np.ndarray | None,
    ao_channel: int,
    ao_fill: int,
    specular: np.ndarray | None,
    specular_channel: int,
    specular_fill: int,
    quality: int,
    mips: bool,
) -> RmaosJob:
    srcs = _pbr.sources(
        (roughness, metallic, ao, specular), (roughness_channel, metallic_channel, ao_channel, specular_channel)
    )
    fills = _pbr.fills(srcs, (0, metallic_fill, ao_fill, specular_fill))
    return RmaosJob(srcs, fills, quality, mips)
