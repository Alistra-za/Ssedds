# SPDX-License-Identifier: GPL-3.0-or-later
"""GIMP batch cases, run by batch\\run_batch.ps1 inside gimp-console's python-fu-eval.

Writes %TDDS_OUT%\\results.json ({name: "pass" | "skip: <why>" | "<error>"}) and
%TDDS_OUT%\\expect_log.json ({name: [text the GIMP log must contain]}).
"""
import ctypes
import json
import os
import struct
import sys
import traceback

_repo = os.environ["TDDS_REPO"]  # exec() gives this script no __file__
for _p in ("gimp/tessalith-dds", "gimp/tests"):
    sys.path.insert(0, os.path.join(_repo, _p))

import gi  # noqa: E402
gi.require_version("Gimp", "3.0")
gi.require_version("Gegl", "0.4")
from gi.repository import Gegl, Gimp, Gio, GLib  # noqa: E402
from psdds_ctypes import BC7, COLOUR, RGBA8, Psdds, PsddsError  # noqa: E402

OUT = os.environ["TDDS_OUT"]
TOO_LARGE = "This file is too large to be a DDS this plug-in can open."
psdds = Psdds()
EXPECT_LOG = {}


def _path(name):
    return os.path.join(OUT, name)


def _encode(pixels, fmt):
    """pixels: 16 (r, g, b, a) u8 tuples for a 4x4 image; mips off."""
    s = psdds.defaults(COLOUR)
    s.format, s.mips = fmt, 0
    buf = bytearray(struct.pack(f"<{len(pixels) * 4}f", *(v / 255 for p in pixels for v in p)))
    return psdds.encode(buf, 4, 4, any(p[3] < 255 for p in pixels), s)


def _write(name, data):
    with open(_path(name), "wb") as f:
        f.write(data)
    return _path(name)


def _load(path):
    try:
        return Gimp.file_load(Gimp.RunMode.NONINTERACTIVE, Gio.File.new_for_path(path))
    except GLib.Error:
        return None


def _read(layer, fmt):
    buf = layer.get_buffer()
    return bytes(buf.get(Gegl.Rectangle.new(0, 0, layer.get_width(), layer.get_height()), 1.0, fmt,
                         Gegl.AbyssPolicy.CLAMP))


def _first_layer(img):
    assert img is not None, "load returned no image"
    layers = img.get_layers()
    assert layers, "image has no layers"
    return layers[0]


GRADIENT = [(i * 16, 255 - i * 16, (i * 37) % 256, 255) for i in range(16)]


def load_rgba8_opaque_opens_rgb():
    data = _encode(GRADIENT, BC7)
    _, _, _, rgba = psdds.decode(data)
    img = _load(_write("opaque_bc7.dds", data))
    layer = _first_layer(img)
    assert len(img.get_layers()) == 1, f"{len(img.get_layers())} layers"
    assert layer.has_alpha() is False, "opaque file opened with alpha"
    want = b"".join(rgba[i:i + 3] for i in range(0, len(rgba), 4))
    assert _read(layer, "R'G'B' u8") == want, "pixels differ from psdds decode"
    img.delete()


def load_alpha_opens_rgba():
    px = list(GRADIENT)
    px[5] = px[5][:3] + (100,)
    data = _encode(px, RGBA8)
    _, _, _, rgba = psdds.decode(data)
    img = _load(_write("alpha100.dds", data))
    layer = _first_layer(img)
    assert layer.has_alpha() is True, "file with alpha 100 opened without alpha"
    assert _read(layer, "R'G'B'A u8") == rgba, "pixels differ from psdds decode"
    img.delete()


def load_alpha_254_opens_opaque():
    data = _encode([p[:3] + (254,) for p in GRADIENT], RGBA8)
    layer = _first_layer(img := _load(_write("alpha254.dds", data)))
    assert layer.has_alpha() is False, "alpha 254 everywhere opened with alpha (DC23)"
    img.delete()


def _cube():
    """Uncompressed RGBA8 cube map: psdds's own 4x4 file with caps2 = all faces and five more faces."""
    one = _encode(GRADIENT, RGBA8)
    head_len = len(one) - 64
    head = bytearray(one[:head_len])
    caps, = struct.unpack_from("<I", head, 108)
    struct.pack_into("<II", head, 108, caps | 0x8, 0xFE00)  # COMPLEX; caps2 = CUBEMAP | all six faces
    return bytes(head) + one[head_len:] * 6


def load_cube_delegates():
    data = _cube()
    assert not psdds.openable(data), "fixture is openable by psdds, so it does not test delegation"
    layer = _first_layer(img := _load(_write("cube.dds", data)))
    assert layer.get_name() != "Background", "cube map opened by Tessalith, not file-dds"
    img.delete()


def load_garbage_error_text():
    data = b"DDS " + b"\0" * 196
    try:
        psdds.decode(data)
        raise AssertionError("psdds decoded the garbage fixture")
    except PsddsError as e:
        EXPECT_LOG["load_garbage_error_text"] = [str(e)]
    assert _load(_write("garbage.dds", data)) is None, "garbage file loaded"


_FSCTL_SET_SPARSE = 0x900C4
_FILE_ATTRIBUTE_SPARSE_FILE = 0x200


def _make_sparse(path, size):
    """Writes "DDS " + a hole up to size bytes. False (file removed) if it cannot be made sparse."""
    import msvcrt
    with open(path, "wb") as f:
        f.write(b"DDS ")
        f.flush()
        k32 = ctypes.WinDLL("kernel32", use_last_error=True)
        k32.DeviceIoControl.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_void_p, ctypes.c_uint32,
                                        ctypes.c_void_p, ctypes.c_uint32, ctypes.POINTER(ctypes.c_uint32),
                                        ctypes.c_void_p]
        got = ctypes.c_uint32()
        k32.DeviceIoControl(msvcrt.get_osfhandle(f.fileno()), _FSCTL_SET_SPARSE, None, 0, None, 0,
                            ctypes.byref(got), None)
        # Never seek+write unless the flag took: on a dense file that allocates the whole size.
        if not os.stat(path).st_file_attributes & _FILE_ATTRIBUTE_SPARSE_FILE:
            sparse = False
        else:
            sparse = True
            f.seek(size - 1)
            f.write(b"\0")
    if not sparse:
        os.remove(path)
    return sparse


def load_too_large_file_refused():
    path = _path("too_large.dds")
    size = psdds.max_file_bytes() + 1
    if not _make_sparse(path, size):
        return f"skip: cannot make a sparse file in {OUT}; the {size}-byte file was not created"
    try:
        assert os.path.getsize(path) == size
        EXPECT_LOG["load_too_large_file_refused"] = [TOO_LARGE]
        assert _load(path) is None, "oversize file loaded"
    finally:
        os.remove(path)


def load_procedure_wins_for_dds():
    layer = _first_layer(img := _load(_write("plain.dds", _encode(GRADIENT, RGBA8))))
    assert layer.get_name() == "Background", f"first layer is {layer.get_name()!r}: file-dds handled .dds"
    img.delete()


CASES = {f.__name__: f for f in (
    load_rgba8_opaque_opens_rgb,
    load_alpha_opens_rgba,
    load_alpha_254_opens_opaque,
    load_cube_delegates,
    load_garbage_error_text,
    load_too_large_file_refused,
    load_procedure_wins_for_dds,
)}

_results = {}
for _name, _fn in CASES.items():
    try:
        _results[_name] = _fn() or "pass"
    except Exception as _e:  # noqa: BLE001 - every failure is reported, the next case still runs
        _results[_name] = f"{type(_e).__name__}: {_e}"
        traceback.print_exc()
with open(_path("results.json"), "w", encoding="utf-8") as _f:
    json.dump(_results, _f, indent=1)
with open(_path("expect_log.json"), "w", encoding="utf-8") as _f:
    json.dump(EXPECT_LOG, _f, indent=1)
