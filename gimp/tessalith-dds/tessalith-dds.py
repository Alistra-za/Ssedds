#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Tessalith DDS for GIMP 3: opens DDS files through psdds.dll. Every rule lives in psdds."""
import os
import sys

import gi
gi.require_version("Gimp", "3.0")
gi.require_version("Gegl", "0.4")
from gi.repository import Gegl, Gimp, GLib, GObject  # noqa: E402

from psdds_ctypes import Psdds, PsddsError  # noqa: E402
from tdds_pixels import band_rows  # noqa: E402

LOAD_PROC = "file-tessalith-dds-load"
TOO_LARGE = "This file is too large to be a DDS this plug-in can open."
NOT_LOCAL = "Tessalith DDS can open only files on a local disk."
NO_FILE_DDS = ("GIMP's own DDS plug-in is needed for this file (HDR, cube map or texture array), "
               "and it is not installed.")


def error(procedure, msg):
    return procedure.new_return_values(Gimp.PDBStatusType.EXECUTION_ERROR,
                                       GLib.Error.new_literal(Gimp.PlugIn.error_quark(), msg, 0))


def _delegate(procedure, run_mode, file):
    """Files psdds does not open (HDR, cube maps, arrays) go to GIMP's file-dds."""
    other = Gimp.get_pdb().lookup_procedure("file-dds-load")
    if other is None:
        return error(procedure, NO_FILE_DDS)
    config = other.create_config()
    config.set_property("run-mode", run_mode)
    config.set_property("file", file)
    return other.run(config)


def _load(procedure, run_mode, file):
    try:
        psdds = Psdds()
        path = file.peek_path()
        if path is None:
            return error(procedure, NOT_LOCAL)
        if os.path.getsize(path) > psdds.max_file_bytes():
            return error(procedure, TOO_LARGE)
        with open(path, "rb") as f:
            head = f.read(148)
            if not psdds.openable(head):
                return _delegate(procedure, run_mode, file)
            f.seek(0)
            w, h, has_alpha, rgba = psdds.decode(f.read())  # the file bytes die with this call
    except (PsddsError, OSError) as e:
        return error(procedure, str(e))
    with_alpha = psdds.opens_with_alpha(rgba, has_alpha)
    img = Gimp.Image.new(w, h, Gimp.ImageBaseType.RGB)
    layer = Gimp.Layer.new(img, "Background", w, h,
                           Gimp.ImageType.RGBA_IMAGE if with_alpha else Gimp.ImageType.RGB_IMAGE,
                           100, Gimp.LayerMode.NORMAL)
    img.insert_layer(layer, None, 0)
    buf = layer.get_buffer()
    rows = band_rows(w * 4, h)
    for y in range(0, h, rows):
        n = min(rows, h - y)
        # On an RGB layer babl drops the alpha (measured).
        buf.set(Gegl.Rectangle.new(0, y, w, n), "R'G'B'A u8", rgba[y * w * 4:(y + n) * w * 4])
    buf.flush()
    return Gimp.ValueArray.new_from_values([
        GObject.Value(Gimp.PDBStatusType, Gimp.PDBStatusType.SUCCESS),
        GObject.Value(Gimp.Image, img),
    ])


def load_dds(procedure, run_mode, file, metadata, flags, config, data):
    return _load(procedure, run_mode, file), flags


class TessalithDds(Gimp.PlugIn):
    # Export (file-tessalith-dds-export) is registered with its procedure in Task 4.
    def do_query_procedures(self):
        return [LOAD_PROC]

    def do_set_i18n(self, name):
        return False

    def do_create_procedure(self, name):
        if name != LOAD_PROC:
            return None
        proc = Gimp.LoadProcedure.new(self, name, Gimp.PDBProcType.PLUGIN, load_dds, None)
        proc.set_menu_label("Tessalith DDS")
        proc.set_format_name("Tessalith DDS")
        proc.set_extensions("dds")
        proc.set_mime_types("image/dds")
        # Keep the magic: a magicless handler would override file-dds even when not preferred.
        proc.set_magics("0,string,DDS")
        proc.set_priority(-1)
        proc.set_attribution("Alistraza", "Alistraza", "2026")
        # Never persistent_enable (DC25): one process per call keeps the DLL mapped while its pool lives.
        return proc


Gimp.main(TessalithDds.__gtype__, sys.argv)
