#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Tessalith DDS for GIMP 3: opens and exports DDS files through psdds.dll. Every rule lives in psdds."""
import os
import sys
import traceback

import gi
gi.require_version("Gimp", "3.0")
gi.require_version("Gegl", "0.4")
from gi.repository import Gegl, Gimp, GLib, GObject  # noqa: E402

from psdds_ctypes import (BC1, BC3, BC4, BC5, BC7, BOX, COLOUR, KAISER, LINEAR_MASK, MITCHELL,  # noqa: E402
                          NORMAL, RGBA8, TENT, Psdds, PsddsError, Settings)
from tdds_pixels import band_rows  # noqa: E402

LOAD_PROC = "file-tessalith-dds-load"
EXPORT_PROC = "file-tessalith-dds-export"
# (nick, psdds id, label). All six formats stay valid arguments (DC32); psdds decides which apply.
FORMATS = [("bc1", BC1, "BC1"), ("bc3", BC3, "BC3"), ("bc4", BC4, "BC4"), ("bc5", BC5, "BC5"),
           ("bc7", BC7, "BC7"), ("rgba8", RGBA8, "RGBA8 (uncompressed)")]
TYPES = [("colour", COLOUR, "Colour"), ("normal", NORMAL, "Normal"), ("linear-mask", LINEAR_MASK, "Linear mask")]
FILTERS = [("box", BOX, "Box"), ("tent", TENT, "Tent"), ("mitchell", MITCHELL, "Mitchell"),
           ("kaiser", KAISER, "Kaiser")]
NOT_LOCAL_SAVE = "Tessalith DDS can save only to files on a local disk."
NO_VISIBLE = "There are no visible layers to export."


class ExportError(Exception):
    pass
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


def raw_settings(config):
    """The seven raw arguments, in psdds.h order. psdds resolves them; nothing is adjusted here."""
    return Settings(config.get_choice_id("format"), config.get_choice_id("texture-type"),
                    int(config.get_property("srgb")), config.get_choice_id("mip-filter"),
                    int(config.get_property("mipmaps")), config.get_property("quality"),
                    int(config.get_property("discard-alpha")))


def export_pixels(psdds, img):
    """Reads img (the plug-in's own copy) as sRGB-encoded floats: (w, h, has_alpha, rgba).

    Merges and resizes img's layers, so it must never be the user's image (M7).
    """
    w, h = img.get_width(), img.get_height()
    psdds.check_size(w, h)  # before the merge allocates anything
    layer = img.merge_visible_layers(Gimp.MergeType.CLIP_TO_IMAGE)
    if layer is None:  # e.g. a visible group holding only hidden layers
        raise ExportError(NO_VISIBLE)
    if (layer.get_width(), layer.get_height()) != (w, h) or layer.get_offsets()[1:] != (0, 0):
        layer.resize_to_image_size()
    rgba = bytearray(w * h * 16)
    buf = layer.get_buffer()
    rows = band_rows(w * 16, h)
    # shortcut: always reads sRGB-encoded sRGB floats (babl converts linear and other profiles), like
    # GIMP's 8-bit PNG export; upgrade when an HDR/linear export path exists (NIF team owns it).
    for y in range(0, h, rows):
        n = min(rows, h - y)
        rgba[y * w * 16:(y + n) * w * 16] = buf.get(Gegl.Rectangle.new(0, y, w, n), 1.0, "R'G'B'A float",
                                                    Gegl.AbyssPolicy.CLAMP)
    return w, h, psdds.alpha_used(rgba, w, h), rgba  # DC31: alpha comes from the pixels


def _write_atomic(path, data):
    tmp = path + ".tmpsave"
    try:
        with open(tmp, "wb") as f:
            f.write(data)
            f.flush()
            os.fsync(f.fileno())
        os.replace(tmp, path)
    except OSError:
        if os.path.exists(tmp):
            os.remove(tmp)
        raise


def export_dds(procedure, run_mode, image, file, options, metadata, config, data):
    path = file.get_path()
    if path is None:
        return error(procedure, NOT_LOCAL_SAVE)
    if not any(lay.get_visible() for lay in image.get_layers()):
        return error(procedure, NO_VISIBLE)
    res, img = options.get_image(image)
    if res != Gimp.ExportReturn.EXPORT:
        img = img.duplicate()  # never merge or resize the user's image (M7)
    try:
        psdds = Psdds()
        w, h, has_alpha, rgba = export_pixels(psdds, img)
        s = raw_settings(config)
        out = psdds.encode(rgba, w, h, has_alpha and not s.discard_alpha, s)  # DC36
        del rgba
        _write_atomic(path, out)
    except (PsddsError, OSError, ExportError) as e:
        return error(procedure, str(e))
    except Exception as e:  # noqa: BLE001 - the user gets a message, the traceback goes to stderr
        traceback.print_exc()
        return error(procedure, f"Tessalith DDS export failed ({type(e).__name__}): {e}")
    finally:
        img.delete()
    return procedure.new_return_values(Gimp.PDBStatusType.SUCCESS, None)


def _choice(items):
    c = Gimp.Choice.new()
    for nick, cid, label in items:
        c.add(nick, cid, label, "")
    return c


def _nick(items, cid):
    return next(n for n, i, _ in items if i == cid)


class TessalithDds(Gimp.PlugIn):
    def do_query_procedures(self):
        try:
            Psdds()  # the export's defaults and quality range come from psdds
        except PsddsError as e:
            # GIMP caches this list until the .py file's mtime changes, so the user must reinstall.
            print(f"Tessalith DDS: export is not registered: {e}. Fix psdds.dll, then reinstall the plug-in.",
                  file=sys.stderr)
            return [LOAD_PROC]
        return [LOAD_PROC, EXPORT_PROC]

    def do_set_i18n(self, name):
        return False

    def do_create_procedure(self, name):
        if name == EXPORT_PROC:
            return self._export_procedure(name)
        if name != LOAD_PROC:
            return None
        proc = Gimp.LoadProcedure.new(self, name, Gimp.PDBProcType.PLUGIN, load_dds, None)
        self._file_info(proc)
        return proc

    def _export_procedure(self, name):
        psdds = Psdds()
        d = psdds.defaults(COLOUR)
        top_q = max(len(psdds.quality_labels(f)) for _, f, _ in FORMATS) - 1
        proc = Gimp.ExportProcedure.new(self, name, Gimp.PDBProcType.PLUGIN, False, export_dds, None)
        proc.set_image_types("*")
        proc.set_capabilities(Gimp.ExportCapabilities.CAN_HANDLE_RGB | Gimp.ExportCapabilities.CAN_HANDLE_ALPHA,
                              None, None)
        self._file_info(proc)
        rw = GObject.ParamFlags.READWRITE
        proc.add_choice_argument("format", "Format", "DDS format", _choice(FORMATS), _nick(FORMATS, d.format), rw)
        proc.add_choice_argument("texture-type", "Texture type", "Texture type", _choice(TYPES),
                                 _nick(TYPES, d.texture_type), rw)
        proc.add_boolean_argument("srgb", "sRGB", "Tag the file as sRGB", bool(d.srgb), rw)
        proc.add_choice_argument("mip-filter", "Filter", "Mip filter", _choice(FILTERS), _nick(FILTERS, d.filter), rw)
        proc.add_boolean_argument("mipmaps", "Mipmaps", "Write mipmaps", bool(d.mips), rw)
        proc.add_boolean_argument("discard-alpha", "Discard transparency", "Save opaque", bool(d.discard_alpha), rw)
        proc.add_int_argument("quality", "Quality", "Quality index", 0, top_q, d.quality, rw)
        return proc

    @staticmethod
    def _file_info(proc):
        proc.set_menu_label("Tessalith DDS")
        proc.set_format_name("Tessalith DDS")
        proc.set_extensions("dds")
        proc.set_mime_types("image/dds")
        # Keep the magic: a magicless handler would override file-dds even when not preferred.
        proc.set_magics("0,string,DDS")
        proc.set_priority(-1)
        proc.set_attribution("Alistraza", "Alistraza", "2026")
        # Never persistent_enable (DC25): one process per call keeps the DLL mapped while its pool lives.


Gimp.main(TessalithDds.__gtype__, sys.argv)
