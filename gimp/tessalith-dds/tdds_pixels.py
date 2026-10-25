# SPDX-License-Identifier: GPL-3.0-or-later
"""Pixel transfer helpers. They only size the row bands moved between GEGL and psdds."""

BAND_CAP = 64 * 1024 * 1024  # DC17: transfer-size choice per read/write, not a settings rule


def band_rows(row_bytes, height, cap=BAND_CAP):
    """Rows per band: at least 1, at most height."""
    return max(1, min(height, cap // max(1, row_bytes)))
