# SPDX-License-Identifier: GPL-3.0-or-later
"""ctypes wrapper around psdds.dll (photoshop/psdds.h). numpy + ctypes only; every rule lives in the DLL."""
import ctypes as C
import os
import tempfile
import time
from pathlib import Path

import numpy as np

ABI_VERSION = 3  # must equal PSDDS_ABI_VERSION in photoshop/psdds.h (a test parses it)
# Labels for the header's enum indices; they are the ABI, not a rule.
FORMATS = ("BC1", "BC3", "BC4", "BC5", "BC7", "RGBA8", "BC6H", "RGBA16F")
TYPES = ("Colour", "Normal", "Linear mask")
FILTERS = ("Box", "Tent", "Mitchell", "Kaiser")
ROLES = ("roughness", "metallic", "ao", "specular")
_KEYS = ("format", "texture_type", "srgb", "filter", "mips", "quality", "discard_alpha")
_ERR = 1024


class PsddsError(RuntimeError):
    pass


class _Settings(C.Structure):
    _fields_ = [(k, C.c_uint32) for k in _KEYS]


class _Controls(C.Structure):
    _fields_ = [(k, C.c_uint32) for k in ("srgb_enabled", "filter_enabled", "quality_enabled")]


class _Buf(C.Structure):
    _fields_ = [("data", C.POINTER(C.c_uint8)), ("len", C.c_size_t)]


class PsddsSource(C.Structure):
    _fields_ = [("px", C.POINTER(C.c_float)), ("width", C.c_uint32), ("height", C.c_uint32),
                ("channels", C.c_uint32), ("channel", C.c_uint32)]


class _Job(C.Structure):
    _fields_ = [("source", PsddsSource * 4), ("fill", C.c_int32 * 4), ("quality", C.c_uint32), ("mips", C.c_uint32),
                ("normal", C.POINTER(C.c_float)), ("normal_width", C.c_uint32), ("normal_height", C.c_uint32),
                ("normal_settings", _Settings)]


class _Result(C.Structure):
    _fields_ = [("rmaos", _Buf), ("normal", _Buf), ("defaulted", C.c_uint32), ("baked", C.c_uint32)]


def _load():
    path = Path(os.environ.get("TESSALITH_PSDDS_DLL") or Path(__file__).with_name("psdds.dll"))
    try:
        dll = C.CDLL(str(path))
    except OSError as e:
        raise PsddsError(f"cannot load {path}: {e}") from e
    dll.psdds_abi_version.argtypes, dll.psdds_abi_version.restype = [], C.c_uint32
    got = dll.psdds_abi_version()  # first, so an older DLL lacking later symbols still gets this message
    if got != ABI_VERSION:
        raise PsddsError(f"psdds.dll ABI {got}, these nodes need {ABI_VERSION}")
    p, u32, sz, cp = C.POINTER, C.c_uint32, C.c_size_t, C.c_char_p
    fp, bp, ep = p(C.c_float), p(C.c_uint8), C.c_char_p  # ep: writable error/note buffers
    sig = {
        "psdds_abi_version": ([], u32), "psdds_version": ([], cp),
        "psdds_defaults": ([u32], _Settings),
        "psdds_preset_count": ([], u32), "psdds_preset_name": ([u32], cp),
        "psdds_preset_settings": ([u32, p(_Settings)], C.c_int32),
        "psdds_resolve": ([p(_Settings), u32, p(_Settings), p(_Controls)], C.c_int32),
        "psdds_format_allowed": ([u32, u32], u32), "psdds_format_offered": ([u32, u32, u32], u32),
        "psdds_resolve_depth": ([p(_Settings), u32, u32, p(_Settings), p(_Controls), p(u32)], C.c_int32),
        "psdds_encode_depth": ([fp, u32, u32, u32, u32, p(_Settings), p(_Buf), ep, sz], C.c_int32),
        "psdds_decode_depth": ([bp, sz, p(u32), p(u32), p(u32), p(u32), p(_Buf), ep, sz], C.c_int32),
        "psdds_quality_count": ([u32], u32), "psdds_quality_label": ([u32, u32], cp),
        "psdds_encode": ([fp, u32, u32, u32, p(_Settings), p(_Buf), ep, sz], C.c_int32),
        "psdds_decode": ([bp, sz, p(u32), p(u32), p(u32), p(_Buf), ep, sz], C.c_int32),
        "psdds_check_size": ([u32, u32, ep, sz], C.c_int32),
        "psdds_dds_openable": ([bp, sz], C.c_int32),
        "psdds_opens_with_alpha": ([bp, sz, u32], C.c_int32),
        "psdds_alpha_used": ([fp, sz], C.c_int32),
        "psdds_max_file_bytes": ([], C.c_uint64),
        "psdds_rmaos_encode": ([p(_Job), p(_Result), ep, sz, ep, sz], C.c_int32),
        "psdds_rmaos_default_quality": ([], u32),
        "psdds_free": ([_Buf], None),
    }
    for name, (args, res) in sig.items():
        f = getattr(dll, name)
        f.argtypes, f.restype = args, res
    return dll


lib = _load()


def _text(buf) -> str:
    return buf.value.decode("utf-8", "replace")


def _raise(buf):
    raise PsddsError(_text(buf))


def _d(s: _Settings) -> dict:
    return {k: getattr(s, k) for k in _KEYS}


def _s(d: dict) -> _Settings:
    return _Settings(*(int(d[k]) for k in _KEYS))


def version() -> str:
    return lib.psdds_version().decode()


def defaults(texture_type: int) -> dict:
    return _d(lib.psdds_defaults(texture_type))


def presets() -> list:
    out = []
    for i in range(lib.psdds_preset_count()):
        s = _Settings()
        if lib.psdds_preset_settings(i, C.byref(s)) != 0:
            raise PsddsError(f"preset {i} unavailable")
        name = lib.psdds_preset_name(i)
        if name is None:
            raise PsddsError(f"preset {i} has no name")
        out.append((name.decode(), _d(s)))
    return out


def resolve(settings: dict, has_alpha: bool) -> tuple:
    eff, ctl = _Settings(), _Controls()
    if lib.psdds_resolve(C.byref(_s(settings)), int(bool(has_alpha)), C.byref(eff), C.byref(ctl)) != 0:
        raise PsddsError("settings out of range")
    return _d(eff), {k: getattr(ctl, k) for k, _ in ctl._fields_}


def resolve_depth(settings: dict, has_alpha: bool, depth: int) -> tuple:
    """-> (effective, controls, discard_enabled); depth is 8, 16 or 32."""
    eff, ctl, de = _Settings(), _Controls(), C.c_uint32()
    if lib.psdds_resolve_depth(C.byref(_s(settings)), int(bool(has_alpha)), depth, C.byref(eff), C.byref(ctl), C.byref(de)) != 0:
        raise PsddsError("settings or depth out of range")
    return _d(eff), {k: getattr(ctl, k) for k, _ in ctl._fields_}, bool(de.value)


def format_offered(texture_type: int, fmt: int, depth: int) -> bool:
    return bool(lib.psdds_format_offered(texture_type, fmt, depth))


def format_allowed(texture_type: int, fmt: int) -> bool:
    return bool(lib.psdds_format_allowed(texture_type, fmt))


def quality_labels(fmt: int) -> list:
    return [lib.psdds_quality_label(fmt, i).decode() for i in range(lib.psdds_quality_count(fmt))]


def rmaos_default_quality() -> int:
    return lib.psdds_rmaos_default_quality()


def check_size(w: int, h: int) -> None:
    err = C.create_string_buffer(_ERR)
    if lib.psdds_check_size(w, h, err, _ERR) != 0:
        _raise(err)


def _f32(a: np.ndarray) -> np.ndarray:
    return np.ascontiguousarray(a, dtype=np.float32)


def to_rgba(img: np.ndarray) -> np.ndarray:
    h, w = img.shape[:2]
    check_size(w, h)
    c = 1 if img.ndim == 2 else img.shape[2]
    out = np.empty((h, w, 4), np.float32)
    out[..., 3] = 1
    if c == 1:
        out[..., :3] = img.reshape(h, w, 1)
    elif c in (3, 4):
        out[..., :3] = img[..., 2::-1]  # chaiNNer is BGR(A)
        if c == 4:
            out[..., 3] = img[..., 3]
    else:
        raise PsddsError(f"unsupported channel count {c}")
    return out


def _need(a: np.ndarray, dtype) -> np.ndarray:
    if a.dtype != dtype or a.ndim != 3 or a.shape[2] != 4 or not a.flags.c_contiguous:
        raise ValueError(f"expected a C-contiguous {np.dtype(dtype).name} array of shape (h, w, 4)")
    return a


def alpha_used(rgba: np.ndarray) -> bool:
    a = _need(rgba, np.float32)
    return bool(lib.psdds_alpha_used(a.ctypes.data_as(C.POINTER(C.c_float)), a.shape[0] * a.shape[1]))


def encode(img: np.ndarray, settings: dict, depth: int = 8) -> bytes:
    """depth 8 is psdds_encode; 16 (sRGB-coded 0..1) and 32 (linear light, HDR kept) use psdds_encode_depth."""
    rgba = to_rgba(img)
    has_alpha = 0 if settings["discard_alpha"] == 1 else int(alpha_used(rgba))
    h, w = rgba.shape[:2]
    out, err = _Buf(), C.create_string_buffer(_ERR)
    try:
        if lib.psdds_encode_depth(rgba.ctypes.data_as(C.POINTER(C.c_float)), w, h, has_alpha, depth, C.byref(_s(settings)),
                                  C.byref(out), err, _ERR) != 0:
            _raise(err)
        return C.string_at(out.data, out.len)
    finally:
        lib.psdds_free(out)


def decode(data: bytes) -> tuple:
    buf = (C.c_uint8 * len(data)).from_buffer_copy(data)
    w, h, ha = C.c_uint32(), C.c_uint32(), C.c_uint32()
    out, err = _Buf(), C.create_string_buffer(_ERR)
    try:
        if lib.psdds_decode(buf, len(data), C.byref(w), C.byref(h), C.byref(ha), C.byref(out), err, _ERR) != 0:
            _raise(err)
        px = np.frombuffer(C.string_at(out.data, out.len), np.uint8).reshape(h.value, w.value, 4)
    finally:
        lib.psdds_free(out)
    return px[..., [2, 1, 0, 3]].astype(np.float32) / 255, bool(ha.value)


def decode_depth32(data: bytes) -> np.ndarray:
    """Top mip of an HDR DDS as float32 RGBA (h, w, 4), top row first."""
    buf = (C.c_uint8 * len(data)).from_buffer_copy(data)
    w, h, ha, d = C.c_uint32(), C.c_uint32(), C.c_uint32(), C.c_uint32()
    out, err = _Buf(), C.create_string_buffer(_ERR)
    try:
        if lib.psdds_decode_depth(buf, len(data), C.byref(w), C.byref(h), C.byref(ha), C.byref(d), C.byref(out), err, _ERR) != 0:
            _raise(err)
        if d.value != 32:
            raise PsddsError(f"expected a depth 32 decode, got {d.value}")
        return np.frombuffer(C.string_at(out.data, out.len), np.float32).reshape(h.value, w.value, 4).copy()
    finally:
        lib.psdds_free(out)


def opens_with_alpha(rgba8: np.ndarray, has_alpha: bool = True) -> bool:
    a = _need(rgba8, np.uint8)
    return bool(lib.psdds_opens_with_alpha(a.ctypes.data_as(C.POINTER(C.c_uint8)), a.shape[0] * a.shape[1], int(has_alpha)))


def max_file_bytes() -> int:
    return lib.psdds_max_file_bytes()


def dds_openable(head: bytes) -> bool:
    buf = (C.c_uint8 * len(head)).from_buffer_copy(head)
    return bool(lib.psdds_dds_openable(buf, len(head)))


def rmaos_encode(sources, fills, quality, mips, normal=None, normal_settings=None):
    """sources: 4 x (array, channel index in that array as given) or None. -> (rmaos, normal|None, defaulted, baked, note)."""
    job = _Job()
    job.fill[:] = [-1] * 4
    if fills is None:
        fills = [None] * 4
    if len(fills) != 4:
        raise ValueError("fills must have exactly 4 entries")
    keep = []  # arrays must outlive the call
    for i, s in enumerate(sources):
        if s is None:
            continue
        arr, ch = s
        a = _f32(arr)
        keep.append(a)
        job.source[i] = PsddsSource(a.ctypes.data_as(C.POINTER(C.c_float)), a.shape[1], a.shape[0],
                                    1 if a.ndim == 2 else a.shape[2], ch)
    for i, f in enumerate(fills):
        job.fill[i] = -1 if f is None else int(f)
    job.quality, job.mips = quality, int(mips)
    if normal is not None:
        n = to_rgba(normal)
        keep.append(n)
        job.normal = n.ctypes.data_as(C.POINTER(C.c_float))
        job.normal_height, job.normal_width = n.shape[:2]
        job.normal_settings = _s(normal_settings or defaults(1))
    res, err, note = _Result(), C.create_string_buffer(_ERR), C.create_string_buffer(_ERR)
    try:
        if lib.psdds_rmaos_encode(C.byref(job), C.byref(res), note, _ERR, err, _ERR) != 0:
            _raise(err)
        rm = C.string_at(res.rmaos.data, res.rmaos.len)
        nm = C.string_at(res.normal.data, res.normal.len) if res.normal.data else None
        defaulted = [c for i, c in enumerate("RGBA") if res.defaulted >> i & 1]
        return rm, nm, defaulted, bool(res.baked), _text(note)
    finally:
        lib.psdds_free(res.rmaos)
        lib.psdds_free(res.normal)


def write_files(pairs) -> None:
    """Write every pair to its own unique temp file beside the target (flushed + fsynced), then os.replace each.
    Unique names keep concurrent saves to one target apart. A failure removes the temps and, if an earlier replace
    already happened, restores that target's previous bytes (or removes a file that was new)."""
    pairs = [(Path(p), d) for p, d in pairs]
    tmps = []
    done = []  # (path, previous bytes or None)
    try:
        for path, data in pairs:
            fd, name = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.", suffix=".tmp")
            tmps.append(Path(name))
            with os.fdopen(fd, "wb") as f:
                f.write(data)
                f.flush()
                os.fsync(f.fileno())
        for i, (tmp, (path, _)) in enumerate(zip(tmps, pairs)):
            # the last replace never needs a restore, and skipping its read avoids racing a concurrent writer
            old = path.read_bytes() if i < len(pairs) - 1 and path.is_file() else None
            # Windows refuses a replace while another replace/open of the target is in flight.
            # shortcut: about 2 s of retries; a lock held longer still raises, so retry for longer if users hit it.
            for attempt in range(20):
                try:
                    os.replace(tmp, path)
                    break
                except PermissionError:
                    if attempt == 19:
                        raise
                    time.sleep(0.01 * (attempt + 1))
            done.append((path, old))
    except BaseException:
        for tmp in tmps:
            tmp.unlink(missing_ok=True)
        for path, old in reversed(done):
            if old is None:
                path.unlink(missing_ok=True)
            else:
                path.write_bytes(old)  # shortcut: best-effort restore, not itself atomic
        raise

if __name__ == "__main__":
    print(version(), lib.psdds_abi_version())
