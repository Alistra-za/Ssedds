// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
using System;
using System.IO;
using System.Runtime.InteropServices;

namespace Tessalith.Dds;

public static unsafe class DdsSave
{
    /// <summary>
    /// Straight-alpha BGRA8 rows (first row at <paramref name="bgra"/>, <paramref name="stride"/> bytes apart) to one DDS
    /// written to <paramref name="output"/> with a single Write, only after psdds encoded (DC27). <paramref name="measured"/>
    /// gets psdds_alpha_used's answer before the encode, also with Discard transparency on (DC31, DC36).
    /// </summary>
    public static void Write(Stream output, byte* bgra, long stride, int width, int height, in PsddsSettings raw, Action<bool>? measured)
    {
        uint w = (uint)width, h = (uint)height;
        if (Psdds.CheckSize(w, h) is { } why) throw new PsddsException(why); // before any allocation (DC12)
        nuint pixels = (nuint)w * h;
        float* buf = (float*)NativeMemory.Alloc(pixels * 16); // throws OutOfMemoryException itself
        byte[] bytes;
        try
        {
            for (int y = 0; y < height; y++)
                Pixels.BgraRowToRgbaFloats(bgra + y * stride, width, buf + (nuint)y * w * 4);
            bool m = Psdds.AlphaUsed(buf, pixels);
            measured?.Invoke(m);
            bytes = Psdds.Encode(buf, w, h, raw.DiscardAlpha == 0 && m, raw);
        }
        finally { NativeMemory.Free(buf); }
        output.Write(bytes);
    }
}
