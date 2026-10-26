// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;

namespace Tessalith.Dds;

[StructLayout(LayoutKind.Sequential)]
public struct PsddsSettings { public uint Format, TextureType, Srgb, Filter, Mips, Quality, DiscardAlpha, AlphaMode; }

[StructLayout(LayoutKind.Sequential)]
public struct PsddsControls { public uint SrgbEnabled, FilterEnabled, QualityEnabled, AlphaModeEnabled; }

[StructLayout(LayoutKind.Sequential)]
internal struct PsddsBuf { public nint Data; public nuint Len; }

/// <summary>Text shown in Paint.NET's error details.</summary>
public sealed class PsddsException(string message) : Exception(message);

/// <summary>Safe wrappers over psdds.dll (photoshop/psdds.h). Holds no rule: every decision is psdds's.</summary>
public static unsafe partial class Psdds
{
    public const uint ExpectedAbiVersion = 4; // = PSDDS_ABI_VERSION in photoshop/psdds.h; a test pins the equality
    public const uint Colour = 0, Normal = 1, LinearMask = 2; // enum values from psdds.h, not rules

    const int ErrCap = 512;
    static volatile bool _compatible;

    [LibraryImport("psdds")] private static partial uint psdds_abi_version();
    [LibraryImport("psdds")] private static partial PsddsSettings psdds_defaults(uint textureType);
    [LibraryImport("psdds")] private static partial uint psdds_preset_count();
    [LibraryImport("psdds")] private static partial nint psdds_preset_name(uint i);
    [LibraryImport("psdds")] private static partial int psdds_preset_settings(uint i, PsddsSettings* output);
    [LibraryImport("psdds")] private static partial int psdds_resolve_depth(PsddsSettings* input, uint hasAlpha, uint depth, PsddsSettings* effective, PsddsControls* controls, uint* discardEnabled);
    [LibraryImport("psdds")] private static partial uint psdds_format_allowed(uint textureType, uint format);
    [LibraryImport("psdds")] private static partial uint psdds_quality_count(uint format);
    [LibraryImport("psdds")] private static partial nint psdds_quality_label(uint format, uint i);
    [LibraryImport("psdds")] private static partial int psdds_encode(float* rgba, uint width, uint height, uint hasAlpha, PsddsSettings* settings, PsddsBuf* output, byte* err, nuint errCap);
    [LibraryImport("psdds")] private static partial int psdds_decode(byte* dds, nuint len, uint* width, uint* height, uint* hasAlpha, PsddsBuf* output, byte* err, nuint errCap);
    [LibraryImport("psdds")] private static partial int psdds_check_size(uint width, uint height, byte* err, nuint errCap);
    [LibraryImport("psdds")] private static partial int psdds_alpha_used(float* rgba, nuint pixels);
    [LibraryImport("psdds")] private static partial void psdds_free(PsddsBuf buf);

    static string? _failure; // the first failure's text, replayed so later calls fail the same way without probing again

    // Load psdds.dll only from beside this assembly, never from PATH or the working directory.
    static Psdds()
    {
        NativeLibrary.SetDllImportResolver(typeof(Psdds).Assembly, (name, asm, _) =>
        {
            if (name != "psdds") return IntPtr.Zero;
            var dir = Path.GetDirectoryName(asm.Location);
            var path = dir == null ? null : Path.Combine(dir, "psdds.dll");
            if (path != null && NativeLibrary.TryLoad(path, out var h)) return h;
            throw new DllNotFoundException("psdds.dll was not found beside " + asm.Location);
        });
    }

    /// <summary>Refuses any psdds.dll whose ABI version is not the one this plugin was written for. Runs once; a failure is remembered.</summary>
    public static void EnsureCompatible()
    {
        if (_compatible) return;
        if (_failure != null) throw new PsddsException(_failure);
        try { Check(psdds_abi_version); }
        catch (PsddsException e) { _failure = e.Message; throw; }
        _compatible = true;
    }

    internal static void Check(Func<uint> abiVersion)
    {
        uint found;
        try { found = abiVersion(); }
        catch (Exception e) when (e is DllNotFoundException or BadImageFormatException or EntryPointNotFoundException)
        {
            throw new PsddsException("psdds.dll is missing or is not the 64-bit build; reinstall Tessalith DDS next to TessalithDds.dll");
        }
        CheckAbi(found);
    }

    internal static void CheckAbi(uint found)
    {
        if (found != ExpectedAbiVersion)
            throw new PsddsException($"psdds.dll is version {found} but this plugin needs version {ExpectedAbiVersion}; reinstall Tessalith DDS");
    }

    static string ErrText(byte* err)
    {
        int n = 0;
        while (n < ErrCap && err[n] != 0) n++;
        return Utf8Text(new ReadOnlySpan<byte>(err, n));
    }

    // Drops a final character cut by the buffer cap, then the single full stop the header allows
    // (the wrapper trims it because Paint.NET's own error text adds its punctuation).
    internal static string Utf8Text(ReadOnlySpan<byte> b)
    {
        int i = b.Length - 1;
        while (i > 0 && (b[i] & 0xC0) == 0x80) i--;
        if (i >= 0 && b[i] >= 0xC0)
        {
            int need = b[i] >= 0xF0 ? 4 : b[i] >= 0xE0 ? 3 : 2;
            if (b.Length - i < need) b = b[..i];
        }
        var s = Encoding.UTF8.GetString(b);
        return s.EndsWith('.') ? s[..^1] : s;
    }

    static string Utf8(nint p) => Marshal.PtrToStringUTF8(p) ?? "";

    public static PsddsSettings Defaults(uint textureType)
    {
        EnsureCompatible();
        return psdds_defaults(textureType);
    }

    public static bool TryResolve(in PsddsSettings raw, bool hasAlpha, out PsddsSettings shown, out PsddsControls controls) =>
        TryResolve(raw, hasAlpha, out shown, out controls, out _);

    /// <summary>discardEnabled false: Discard transparency is ticked and greyed (the format stores no alpha).</summary>
    public static bool TryResolve(in PsddsSettings raw, bool hasAlpha, out PsddsSettings shown, out PsddsControls controls, out bool discardEnabled)
    {
        EnsureCompatible();
        var input = raw;
        PsddsSettings s = default;
        PsddsControls c = default;
        uint d = 0;
        // Paint.NET documents are 8-bit; at depth 8 this is psdds_resolve plus discard_enabled
        bool ok = psdds_resolve_depth(&input, hasAlpha ? 1u : 0u, 8, &s, &c, &d) == 0;
        shown = ok ? s : default;
        controls = ok ? c : default;
        discardEnabled = ok && d != 0;
        return ok;
    }

    public static bool FormatAllowed(uint textureType, uint format)
    {
        EnsureCompatible();
        return psdds_format_allowed(textureType, format) != 0;
    }

    public static int PresetCount()
    {
        EnsureCompatible();
        return (int)psdds_preset_count();
    }

    public static string PresetName(int i)
    {
        EnsureCompatible();
        var p = i < 0 ? 0 : psdds_preset_name((uint)i);
        return p != 0 ? Utf8(p) : throw new PsddsException($"There is no preset {i}");
    }

    public static PsddsSettings PresetSettings(int i)
    {
        EnsureCompatible();
        PsddsSettings s = default;
        if (i < 0 || psdds_preset_settings((uint)i, &s) != 0) throw new PsddsException($"There is no preset {i}");
        return s;
    }

    public static IReadOnlyList<string> QualityLabels(uint format)
    {
        EnsureCompatible();
        uint n = psdds_quality_count(format);
        var list = new List<string>((int)n);
        for (uint i = 0; i < n; i++) list.Add(Utf8(psdds_quality_label(format, i)));
        return list;
    }

    /// <summary>null when the size can be encoded, else psdds's text (trailing full stop trimmed).</summary>
    public static string? CheckSize(uint width, uint height)
    {
        EnsureCompatible();
        byte* err = stackalloc byte[ErrCap];
        err[0] = 0;
        return psdds_check_size(width, height, err, ErrCap) == 0 ? null : ErrText(err);
    }

    internal static bool AlphaUsed(float* rgba, nuint pixels)
    {
        EnsureCompatible();
        return psdds_alpha_used(rgba, pixels) != 0;
    }

    public static bool AlphaUsed(ReadOnlySpan<float> rgba)
    {
        if (rgba.Length % 4 != 0) throw new ArgumentException("RGBA floats come in fours", nameof(rgba));
        fixed (float* p = rgba) return AlphaUsed(p, (nuint)(rgba.Length / 4));
    }

    /// <summary>rgba must hold exactly width*height*4 floats (psdds cannot check); the public span overload checks.</summary>
    internal static byte[] Encode(float* rgba, uint width, uint height, bool hasAlpha, in PsddsSettings raw)
    {
        EnsureCompatible();
        var settings = raw;
        PsddsBuf buf = default;
        byte* err = stackalloc byte[ErrCap];
        err[0] = 0;
        try
        {
            if (psdds_encode(rgba, width, height, hasAlpha ? 1u : 0u, &settings, &buf, err, ErrCap) != 0)
                throw new PsddsException(ErrText(err));
            return Copy(buf);
        }
        finally { if (buf.Data != 0) psdds_free(buf); }
    }

    public static byte[] Encode(ReadOnlySpan<float> rgba, uint width, uint height, bool hasAlpha, in PsddsSettings raw)
    {
        if ((ulong)rgba.Length != (ulong)width * height * 4)
            throw new ArgumentException($"Expected {(ulong)width * height * 4} floats, got {rgba.Length}", nameof(rgba));
        fixed (float* p = rgba) return Encode(p, width, height, hasAlpha, raw);
    }

    public static (byte[] Rgba, uint Width, uint Height, bool HasAlpha) Decode(ReadOnlySpan<byte> dds)
    {
        EnsureCompatible();
        if (dds.IsEmpty) throw new PsddsException("The file is empty");
        PsddsBuf buf = default;
        uint w = 0, h = 0, a = 0;
        byte* err = stackalloc byte[ErrCap];
        err[0] = 0;
        try
        {
            fixed (byte* p = dds)
            {
                if (psdds_decode(p, (nuint)dds.Length, &w, &h, &a, &buf, err, ErrCap) != 0)
                    throw new PsddsException(ErrText(err));
            }
            return (Copy(buf), w, h, a != 0);
        }
        finally { if (buf.Data != 0) psdds_free(buf); }
    }

    static byte[] Copy(PsddsBuf buf)
    {
        if (buf.Len > int.MaxValue) throw new PsddsException("The DDS data is too large to hold in memory");
        return new ReadOnlySpan<byte>((void*)buf.Data, (int)buf.Len).ToArray();
    }
}
