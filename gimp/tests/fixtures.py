# SPDX-License-Identifier: GPL-3.0-or-later
"""Generated test inputs; no image or DDS file lives in the repo."""
import array
import struct


def dds_header(fourcc=0, caps2=0, dxgi=None, array_size=1):
    """DDS magic + 124-byte header (+ 20-byte DX10 header when dxgi is given). fourcc: int or 4 bytes."""
    if dxgi is not None:
        fourcc = b"DX10"
    if isinstance(fourcc, bytes):
        fourcc = struct.unpack("<I", fourcc)[0]
    pf = struct.pack("<IIIIIIII", 32, 4 if fourcc else 0x41, fourcc, 0, 0, 0, 0, 0)
    head = b"DDS " + struct.pack("<IIIIIII", 124, 0x1007, 4, 4, 0, 0, 1) + b"\0" * 44 + pf
    head += struct.pack("<IIIII", 0x1000, caps2, 0, 0, 0)
    if dxgi is not None:
        head += struct.pack("<IIIII", dxgi, 3, 0, array_size, 0)
    return head


def rgba_floats(pixels):
    """bytearray of float32 RGBA from a list of (r, g, b, a)."""
    return bytearray(array.array("f", [v for p in pixels for v in p]).tobytes())
