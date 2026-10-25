# SPDX-License-Identifier: GPL-3.0-or-later
from sanic.log import logger

from .. import image_category
from . import _psdds

tessalith_group = image_category.add_node_group("Tessalith DDS")
logger.info(f"Tessalith DDS: psdds {_psdds.version()}")
