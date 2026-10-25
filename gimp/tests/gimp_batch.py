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
from psdds_ctypes import BC4, BC7, COLOUR, RGBA8, Psdds, PsddsError  # noqa: E402

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
    assert psdds.decode(data)[3][3::4] == bytes([254] * 16), "fixture alpha is not 254"
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
    assert (img.get_width(), img.get_height()) == (4, 4), f"cube opened as {img.get_width()}x{img.get_height()}"
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


def load_non_local_error_text():
    # Called directly: gimp-file-load would fetch a remote file to a local copy first.
    proc = Gimp.get_pdb().lookup_procedure("file-tessalith-dds-load")
    cfg = proc.create_config()
    cfg.set_property("run-mode", Gimp.RunMode.NONINTERACTIVE)
    cfg.set_property("file", Gio.File.new_for_uri("http://127.0.0.1:9/remote.dds"))
    res = proc.run(cfg)
    assert res.index(0) == Gimp.PDBStatusType.EXECUTION_ERROR, f"status {res.index(0)}"
    msg = res.index(1) if res.length() > 1 else None
    assert msg == "Tessalith DDS can open only files on a local disk.", f"error text {msg!r}"


def load_procedure_wins_for_dds():
    layer = _first_layer(img := _load(_write("plain.dds", _encode(GRADIENT, RGBA8))))
    assert layer.get_name() == "Background", f"first layer is {layer.get_name()!r}: file-dds handled .dds"
    img.delete()


EXPORT = "file-tessalith-dds-export"


def _image(w, h, px, fmt="R'G'B'A u8", kind=Gimp.ImageType.RGBA_IMAGE, base=Gimp.ImageBaseType.RGB,
           precision=None):
    """A one-layer image holding px (bytes in fmt)."""
    img = (Gimp.Image.new_with_precision(w, h, base, precision) if precision else Gimp.Image.new(w, h, base))
    _add_layer(img, w, h, px, fmt, kind)
    return img


def _add_layer(img, w, h, px, fmt="R'G'B'A u8", kind=Gimp.ImageType.RGBA_IMAGE, x=0, y=0):
    layer = Gimp.Layer.new(img, f"L{len(img.get_layers())}", w, h, kind, 100, Gimp.LayerMode.NORMAL)
    img.insert_layer(layer, None, 0)
    layer.set_offsets(x, y)
    buf = layer.get_buffer()
    buf.set(Gegl.Rectangle.new(0, 0, w, h), fmt, bytes(px))
    buf.flush()
    return layer


def _gradient(w, h, alpha=255):
    return bytes(v for y in range(h) for x in range(w)
                 for v in ((x * 4) % 256, (y * 4) % 256, ((x + y) * 2) % 256, alpha))


def _export_run(img, name, **args):
    """Runs the export procedure NONINTERACTIVE; returns (result values, path)."""
    path = _path(name)
    cfg = Gimp.get_pdb().lookup_procedure(EXPORT).create_config()
    cfg.set_property("run-mode", Gimp.RunMode.NONINTERACTIVE)
    cfg.set_property("image", img)
    cfg.set_property("file", Gio.File.new_for_path(path))
    for k, v in args.items():
        cfg.set_property(k.replace("_", "-"), v)
    return Gimp.get_pdb().lookup_procedure(EXPORT).run(cfg), path


def _export(img, name, **args):
    """Exports and returns the file's bytes."""
    assert Gimp.get_pdb().lookup_procedure(EXPORT) is not None, f"{EXPORT} is not registered"
    res, path = _export_run(img, name, **args)
    assert res.index(0) == Gimp.PDBStatusType.SUCCESS, \
        f"export status {res.index(0)}: {res.index(1) if res.length() > 1 else ''}"
    with open(path, "rb") as f:
        return f.read()


def _direct(img, has_alpha=False, s=None):
    """psdds.encode of the first layer's pixels read as sRGB-encoded floats."""
    layer = img.get_layers()[0]
    rgba = bytearray(_read(layer, "R'G'B'A float"))
    return psdds.encode(rgba, layer.get_width(), layer.get_height(), has_alpha, s or psdds.defaults(COLOUR))


def export_matches_direct_encode():
    img = _image(64, 64, _rgb(_gradient(64, 64)), "R'G'B' u8", Gimp.ImageType.RGB_IMAGE)
    s = psdds.defaults(COLOUR)
    s.format = BC7
    assert _export(img, "direct.dds", format="bc7", texture_type="colour") == _direct(img, False, s), \
        "export differs from psdds.encode of the same pixels"
    img.delete()


def export_alpha_kept_bc3():
    px = bytearray(_gradient(8, 8))
    px[4 * 9 + 3] = 0
    img = _image(8, 8, px)
    _, _, a, rgba = psdds.decode(_export(img, "alpha_bc3.dds", format="bc3"))
    assert a, "BC3 export with a transparent pixel decodes without alpha"
    assert rgba[4 * 9 + 3] < 128, f"transparent pixel decodes with alpha {rgba[4 * 9 + 3]}"
    img.delete()


def _rgb(px):
    return b"".join(px[i:i + 3] for i in range(0, len(px), 4))


def _opaque(dds):
    return set(psdds.decode(dds)[3][3::4]) == {255}


def export_opaque_alpha_channel_is_opaque():
    px = _gradient(8, 8)
    a, b = _image(8, 8, px), _image(8, 8, _rgb(px), "R'G'B' u8", Gimp.ImageType.RGB_IMAGE)
    assert _export(a, "op_a.dds", format="bc7") == _export(b, "op_b.dds", format="bc7"), \
        "alpha channel at 255 exports differently from no alpha channel (DC31)"
    a.delete()
    b.delete()


def export_alpha_254_is_opaque():
    px = bytearray(_gradient(8, 8, 254))
    a, b = _image(8, 8, px), _image(8, 8, _rgb(px), "R'G'B' u8", Gimp.ImageType.RGB_IMAGE)
    da = _export(a, "a254.dds", format="bc7")
    assert da == _export(b, "a254_rgb.dds", format="bc7"), "alpha 254 everywhere did not export opaque"
    # psdds_decode's has_alpha is "the format stores alpha" (always 1 for BC7), so check the alpha bytes.
    assert _opaque(da), "alpha 254 export decodes with alpha below 255"
    px[4 * 5 + 3] = 253
    c = _image(8, 8, px)
    assert not _opaque(_export(c, "a253.dds", format="bc7")), "a pixel at 253 lost alpha"
    for i in (a, b, c):
        i.delete()


def export_offset_layer_is_canvas_sized():
    img = Gimp.Image.new(64, 64, Gimp.ImageBaseType.RGB)
    layer = _add_layer(img, 16, 16, _gradient(16, 16), x=10, y=10)
    w, h, _, _ = psdds.decode(_export(img, "offset.dds", format="rgba8"))
    assert (w, h) == (64, 64), f"decoded {w}x{h}"
    # M7: the user's layer is untouched.
    assert (layer.get_width(), layer.get_height(), *layer.get_offsets()[1:]) == (16, 16, 10, 10), \
        "the user's layer was resized or moved"
    img.delete()


def export_two_layers_merged_user_image_unchanged():
    img = Gimp.Image.new(8, 8, Gimp.ImageBaseType.RGB)
    _add_layer(img, 8, 8, bytes([255, 0, 0]) * 64, "R'G'B' u8", Gimp.ImageType.RGB_IMAGE)
    top = _add_layer(img, 4, 4, bytes([0, 0, 255]) * 16, "R'G'B' u8", Gimp.ImageType.RGB_IMAGE, 2, 2)
    before = [(lay.get_name(), lay.get_width(), lay.get_height(), tuple(lay.get_offsets()[1:]),
               _read(lay, "R'G'B'A u8")) for lay in img.get_layers()]
    _, _, _, rgba = psdds.decode(_export(img, "two.dds", format="rgba8"))
    assert rgba[0:3] == bytes([255, 0, 0]), f"pixel 0,0 is {tuple(rgba[0:3])}, not the bottom layer's red"
    i = (3 * 8 + 3) * 4
    assert rgba[i:i + 3] == bytes([0, 0, 255]), f"pixel 3,3 is {tuple(rgba[i:i + 3])}, not the top layer's blue"
    after = [(lay.get_name(), lay.get_width(), lay.get_height(), tuple(lay.get_offsets()[1:]),
              _read(lay, "R'G'B'A u8")) for lay in img.get_layers()]
    assert after == before, "the user's image layers changed"
    assert top.is_valid(), "the user's top layer was merged away"
    img.delete()


def _float_image(hi, lo):
    vals = [hi, lo, hi, 1.0] * 8 + [lo, hi, lo, 1.0] * 8
    img = _image(4, 4, struct.pack(f"<{len(vals)}f", *vals), "R'G'B'A float", base=Gimp.ImageBaseType.RGB,
                 precision=Gimp.Precision.FLOAT_NON_LINEAR)
    return img


def export_float_out_of_range_clamps():
    a, b = _float_image(1.5, -0.25), _float_image(1.0, 0.0)
    got = struct.unpack("<4f", _read(a.get_layers()[0], "R'G'B'A float")[:16])
    assert got[0] == 1.5 and got[1] == -0.25, f"fixture does not hold out-of-range values: {got}"
    assert _export(a, "f_out.dds", format="rgba8") == _export(b, "f_in.dds", format="rgba8"), \
        "out-of-range floats export differently from clamped ones"
    a.delete()
    b.delete()


def export_gray_image():
    img = _image(4, 4, bytes(range(0, 256, 16)), "Y' u8", Gimp.ImageType.GRAY_IMAGE, Gimp.ImageBaseType.GRAY)
    _, _, _, rgba = psdds.decode(_export(img, "gray.dds", format="rgba8"))
    assert all(rgba[i] == rgba[i + 1] == rgba[i + 2] for i in range(0, len(rgba), 4)), "gray export has R != G or B"
    img.delete()


def export_failure_keeps_old_file():
    try:
        psdds.check_size(17000, 4)
        raise AssertionError("psdds accepts 17000x4, so this case tests nothing")
    except PsddsError as e:
        want = str(e)
    old = b"\xab" * 20000
    path = _write("fail.dds", old)
    img = Gimp.Image.new(17000, 4, Gimp.ImageBaseType.RGB)
    lay = Gimp.Layer.new(img, "L", 17000, 4, Gimp.ImageType.RGB_IMAGE, 100, Gimp.LayerMode.NORMAL)
    img.insert_layer(lay, None, 0)
    res, _ = _export_run(img, "fail.dds")
    assert res.index(0) == Gimp.PDBStatusType.EXECUTION_ERROR, f"status {res.index(0)}"
    msg = res.index(1) if res.length() > 1 else None
    assert msg == want, f"error text {msg!r}, psdds says {want!r}"
    with open(path, "rb") as f:
        assert f.read() == old, "the old file changed"
    assert not os.path.exists(path + ".tmpsave"), ".tmpsave left behind"
    img.delete()


def export_discard_transparency_saves_opaque():
    px = bytearray(_gradient(8, 8))
    px[4 * 9 + 3] = 64
    opaque = bytearray(px)
    opaque[4 * 9 + 3] = 255
    a, b = _image(8, 8, px), _image(8, 8, opaque)
    d = _export(a, "disc.dds", format="bc7", discard_alpha=True)
    assert d == _export(b, "disc_op.dds", format="bc7"), "discard export differs from the opaque export"
    assert d != _export(a, "disc_no.dds", format="bc7", discard_alpha=False), "discard made no difference"
    assert _opaque(_export(a, "disc3.dds", format="bc3", discard_alpha=True)), \
        "BC3 with discard decodes with alpha below 255"
    a.delete()
    b.delete()


def export_defaults_have_srgb_off():
    cfg = Gimp.get_pdb().lookup_procedure(EXPORT).create_config()
    assert cfg.get_property("srgb") is False, "srgb defaults on (DC34)"
    img = _image(8, 8, _rgb(_gradient(8, 8)), "R'G'B' u8", Gimp.ImageType.RGB_IMAGE)
    assert _export(img, "defaults.dds") == _direct(img), "default export differs from psdds defaults(COLOUR)"
    img.delete()


def export_disallowed_format_saves_default():
    img = _image(8, 8, _rgb(_gradient(8, 8)), "R'G'B' u8", Gimp.ImageType.RGB_IMAGE)
    assert not psdds.format_allowed(COLOUR, BC4), "psdds allows BC4 for Colour, so this case tests nothing"
    assert _export(img, "c_bc4.dds", texture_type="colour", format="bc4") == \
        _export(img, "c_bc7.dds", texture_type="colour", format="bc7"), "colour + bc4 did not save as the default"
    assert _export(img, "c_rgba8.dds", format="rgba8") != _export(img, "c_bc7b.dds", format="bc7"), \
        "the format argument changes nothing, so this case tests nothing"
    img.delete()


def export_procedure_wins_for_dds():
    img = _image(8, 8, _rgb(_gradient(8, 8)), "R'G'B' u8", Gimp.ImageType.RGB_IMAGE)
    path = _path("filesave.dds")
    Gimp.file_save(Gimp.RunMode.NONINTERACTIVE, img, Gio.File.new_for_path(path), None)
    with open(path, "rb") as f:
        assert f.read() == _direct(img), "file_save of .dds was not written by Tessalith"
    img.delete()


CASES = {f.__name__: f for f in (
    load_rgba8_opaque_opens_rgb,
    load_alpha_opens_rgba,
    load_alpha_254_opens_opaque,
    load_cube_delegates,
    load_garbage_error_text,
    load_too_large_file_refused,
    load_non_local_error_text,
    load_procedure_wins_for_dds,
    export_matches_direct_encode,
    export_alpha_kept_bc3,
    export_opaque_alpha_channel_is_opaque,
    export_alpha_254_is_opaque,
    export_offset_layer_is_canvas_sized,
    export_two_layers_merged_user_image_unchanged,
    export_float_out_of_range_clamps,
    export_gray_image,
    export_failure_keeps_old_file,
    export_discard_transparency_saves_opaque,
    export_defaults_have_srgb_off,
    export_disallowed_format_saves_default,
    export_procedure_wins_for_dds,
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
