# SPDX-License-Identifier: GPL-3.0-or-later
"""ctypes wrapper over psdds.dll (photoshop/psdds.h, ABI 2). No gi import; every rule lives in psdds."""
import ctypes
import os

EXPECTED_ABI = 2
BC1, BC3, BC4, BC5, BC7, RGBA8 = range(6)
COLOUR, NORMAL, LINEAR_MASK = range(3)
BOX, TENT, MITCHELL, KAISER = range(4)

_ERR_CAP = 512
_FORMATS = (BC1, BC3, BC4, BC5, BC7, RGBA8)


class Settings(ctypes.Structure):
    _fields_ = [(n, ctypes.c_uint32) for n in
                ("format", "texture_type", "srgb", "filter", "mips", "quality", "discard_alpha")]

    def key(self):
        return tuple(getattr(self, n) for n, _ in self._fields_)


class Controls(ctypes.Structure):
    _fields_ = [(n, ctypes.c_uint32) for n in ("srgb_enabled", "filter_enabled", "quality_enabled")]


class _Buf(ctypes.Structure):
    _fields_ = [("data", ctypes.c_void_p), ("len", ctypes.c_size_t)]


class PsddsError(Exception):
    pass


def _text(buf):
    return buf.value.decode("utf-8", errors="replace")


def match_preset(presets, raw, has_alpha, preferred, resolve):
    """Index of the preset whose effective values equal raw's: preferred if it still does, else the first, else -1."""
    def eff(s):
        s = Settings.from_buffer_copy(s)
        if not has_alpha:
            s.discard_alpha = 0  # DC40: discard is ignored without alpha
        return resolve(s, has_alpha)[0].key()
    want = eff(raw)
    hits = [i for i, (_, p) in enumerate(presets) if eff(p) == want]
    if preferred in hits:
        return preferred
    return hits[0] if hits else -1


class PresetState:
    """DC40 picked seed, DC45 custom. Indices are psdds seed indices (M4); None or -1 = Custom."""

    def __init__(self, resolve, count):
        self._resolve = resolve
        self._count = count
        self.picked = -1
        self.custom = False

    def pick(self, index):
        if index is None or index == -1:
            self.custom = True
        else:
            if not 0 <= index < self._count:
                raise ValueError(f"preset index {index} is out of range")
            self.custom = False
            self.picked = index

    def saved(self):
        self.custom = False

    def shown(self, presets, raw, has_alpha):
        if self.custom:
            return -1
        self.picked = match_preset(presets, raw, has_alpha, self.picked, self._resolve)
        return self.picked


class Psdds:
    def __init__(self, path=None):
        path = path or os.environ.get("TESSALITH_PSDDS_DLL") or os.path.join(
            os.path.dirname(os.path.abspath(__file__)), "psdds.dll")
        try:
            d = self._dll = ctypes.CDLL(path)
        except OSError as e:
            raise PsddsError(f"Cannot load psdds.dll: {e}") from e
        # Check the ABI before touching anything else.
        try:
            d.psdds_abi_version.argtypes, d.psdds_abi_version.restype = [], ctypes.c_uint32
            got = d.psdds_abi_version()
        except AttributeError as e:
            raise PsddsError(f"psdds.dll has no ABI version, this plug-in needs {EXPECTED_ABI}") from e
        if got != EXPECTED_ABI:
            raise PsddsError(f"psdds.dll has ABI version {got}, this plug-in needs {EXPECTED_ABI}")
        u32, vp, sz, cp = ctypes.c_uint32, ctypes.c_void_p, ctypes.c_size_t, ctypes.c_char_p
        S, C, B = ctypes.POINTER(Settings), ctypes.POINTER(Controls), ctypes.POINTER(_Buf)
        pu32, eb = ctypes.POINTER(u32), ctypes.c_char_p
        i32 = ctypes.c_int32
        for name, args, res in [
            ("psdds_defaults", [u32], Settings),
            ("psdds_preset_count", [], u32),
            ("psdds_preset_name", [u32], cp),
            ("psdds_preset_settings", [u32, S], i32),
            ("psdds_resolve", [S, u32, S, C], i32),
            ("psdds_format_allowed", [u32, u32], u32),
            ("psdds_quality_count", [u32], u32),
            ("psdds_quality_label", [u32, u32], cp),
            ("psdds_encode", [vp, u32, u32, u32, S, B, eb, sz], i32),
            ("psdds_decode", [cp, sz, pu32, pu32, pu32, B, eb, sz], i32),
            ("psdds_check_size", [u32, u32, eb, sz], i32),
            ("psdds_dds_openable", [cp, sz], i32),
            ("psdds_opens_with_alpha", [cp, sz, u32], i32),
            ("psdds_alpha_used", [vp, sz], i32),
            ("psdds_max_file_bytes", [], ctypes.c_uint64),
            ("psdds_free", [_Buf], None),
        ]:
            f = getattr(d, name)
            f.argtypes, f.restype = args, res

    def defaults(self, texture_type):
        return self._dll.psdds_defaults(texture_type)

    def presets(self):
        out = []
        for i in range(self._dll.psdds_preset_count()):
            s = Settings()
            if self._dll.psdds_preset_settings(i, ctypes.byref(s)) != 0:
                raise PsddsError(f"psdds preset {i} is unreadable")
            out.append((self._dll.psdds_preset_name(i).decode("utf-8"), s))
        return out

    def match_preset(self, presets, raw, has_alpha, preferred=-1):
        return match_preset(presets, raw, has_alpha, preferred, self.resolve)

    def resolve(self, raw, has_alpha):
        eff, ctl = Settings(), Controls()
        if self._dll.psdds_resolve(ctypes.byref(raw), int(bool(has_alpha)), ctypes.byref(eff), ctypes.byref(ctl)) != 0:
            raise ValueError("psdds rejected the settings (a value is out of range)")
        return eff, ctl

    def format_allowed(self, texture_type, fmt):
        return bool(self._dll.psdds_format_allowed(texture_type, fmt))

    def allowed_formats(self, texture_type):
        return [f for f in _FORMATS if self.format_allowed(texture_type, f)]

    def quality_labels(self, fmt):
        d = self._dll
        return [d.psdds_quality_label(fmt, i).decode("utf-8") for i in range(d.psdds_quality_count(fmt))]

    def check_size(self, w, h):
        err = ctypes.create_string_buffer(_ERR_CAP)
        if self._dll.psdds_check_size(w, h, err, _ERR_CAP) != 0:
            raise PsddsError(_text(err))

    def openable(self, head):
        # shortcut: 148 = magic + header + DX10 header, the "up to 148" of psdds.h's psdds_dds_openable; revisit if that changes.
        head = bytes(head[:148])
        return bool(self._dll.psdds_dds_openable(head, len(head)))

    def opens_with_alpha(self, rgba8, has_alpha):
        rgba8 = bytes(rgba8)
        return bool(self._dll.psdds_opens_with_alpha(rgba8, len(rgba8) // 4, int(bool(has_alpha))))

    @staticmethod
    def _floats(rgba, w, h):
        # psdds cannot check the length itself, so a short buffer would be read past its end.
        if len(rgba) != w * h * 16:
            raise ValueError("pixel buffer is not width*height*16 bytes")
        return (ctypes.c_float * (w * h * 4)).from_buffer(rgba)

    def alpha_used(self, rgba, w, h):
        return bool(self._dll.psdds_alpha_used(self._floats(rgba, w, h), w * h))

    def max_file_bytes(self):
        return self._dll.psdds_max_file_bytes()

    def encode(self, rgba, w, h, has_alpha, s):
        """rgba must be a writable buffer (bytearray), not bytes: ctypes from_buffer needs it."""
        arr = self._floats(rgba, w, h)
        out, err = _Buf(), ctypes.create_string_buffer(_ERR_CAP)
        rc = self._dll.psdds_encode(arr, w, h, int(bool(has_alpha)), ctypes.byref(s), ctypes.byref(out), err, _ERR_CAP)
        try:
            if rc != 0:
                raise PsddsError(_text(err))
            return ctypes.string_at(out.data, out.len)
        finally:
            self._dll.psdds_free(out)

    def decode(self, data):
        data = bytes(data)
        w, h, a = ctypes.c_uint32(), ctypes.c_uint32(), ctypes.c_uint32()
        out, err = _Buf(), ctypes.create_string_buffer(_ERR_CAP)
        rc = self._dll.psdds_decode(data, len(data), ctypes.byref(w), ctypes.byref(h), ctypes.byref(a),
                                    ctypes.byref(out), err, _ERR_CAP)
        try:
            if rc != 0:
                raise PsddsError(_text(err))
            return w.value, h.value, bool(a.value), ctypes.string_at(out.data, out.len)
        finally:
            self._dll.psdds_free(out)
