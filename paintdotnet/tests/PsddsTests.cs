// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
using System.Runtime.InteropServices;
using System.Text.RegularExpressions;

namespace Tessalith.Dds.Tests;

public unsafe class PsddsTests
{
    const uint BC1 = 0, BC4 = 2, BC5 = 3, BC7 = 4, RGBA8 = 5;
    const uint Box = 0, Kaiser = 3;

    static PsddsSettings Colour(uint format = BC7, uint srgb = 0, uint mips = 1, uint filter = Box, uint quality = 0, uint discard = 0, uint alphaMode = 0) =>
        new() { Format = format, TextureType = Psdds.Colour, Srgb = srgb, Filter = filter, Mips = mips, Quality = quality, DiscardAlpha = discard, AlphaMode = alphaMode };

    // Gradient image; alpha 1 unless given.
    static float[] Image(int w, int h, Func<int, int, float>? alpha = null)
    {
        var px = new float[w * h * 4];
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
            {
                int i = (y * w + x) * 4;
                px[i] = (x * 255 / Math.Max(1, w - 1)) / 255f;
                px[i + 1] = (y * 255 / Math.Max(1, h - 1)) / 255f;
                px[i + 2] = ((x + y) * 255 / Math.Max(1, w + h - 2)) / 255f;
                px[i + 3] = alpha?.Invoke(x, y) ?? 1f;
            }
        return px;
    }

    static byte[] Enc(float[] px, int w, int h, bool hasAlpha, PsddsSettings s)
    {
        fixed (float* p = px) return Psdds.Encode(p, (uint)w, (uint)h, hasAlpha, s);
    }

    static string FindHeader()
    {
        for (var d = new DirectoryInfo(AppContext.BaseDirectory); d != null; d = d.Parent)
        {
            var f = Path.Combine(d.FullName, "photoshop", "psdds.h");
            if (File.Exists(f)) return f;
        }
        throw new FileNotFoundException("photoshop\\psdds.h not found above " + AppContext.BaseDirectory);
    }

    [Fact]
    public void AbiVersionMatchesHeaderAndDll()
    {
        var m = Regex.Match(File.ReadAllText(FindHeader()), @"#define PSDDS_ABI_VERSION (\d+)u");
        Assert.True(m.Success);
        Assert.Equal(uint.Parse(m.Groups[1].Value), Psdds.ExpectedAbiVersion);
        Psdds.EnsureCompatible();
    }

    [Fact]
    public void AbiMismatchIsRefused()
    {
        var e = Assert.Throws<PsddsException>(() => Psdds.CheckAbi(Psdds.ExpectedAbiVersion + 1));
        Assert.Contains("version", e.Message);
        Psdds.CheckAbi(Psdds.ExpectedAbiVersion);
    }

    [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern nint GetModuleHandleW(string name);

    [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)]
    static extern uint GetModuleFileNameW(nint module, char[] buf, uint size);

    [Fact]
    public void DllIsLoadedFromBesideTheAssembly()
    {
        Psdds.EnsureCompatible();
        var h = GetModuleHandleW("psdds.dll");
        Assert.NotEqual(0, h);
        var buf = new char[1024];
        var n = GetModuleFileNameW(h, buf, (uint)buf.Length);
        var expected = Path.Combine(Path.GetDirectoryName(typeof(Psdds).Assembly.Location)!, "psdds.dll");
        Assert.Equal(Path.GetFullPath(expected), new string(buf, 0, (int)n), ignoreCase: true);
    }

    [Fact]
    public void MissingOrWrongBitnessDllMessage()
    {
        const string msg = "psdds.dll is missing or is not the 64-bit build; reinstall Tessalith DDS next to TessalithDds.dll";
        Assert.Equal(msg, Assert.Throws<PsddsException>(() => Psdds.Check(() => throw new DllNotFoundException())).Message);
        Assert.Equal(msg, Assert.Throws<PsddsException>(() => Psdds.Check(() => throw new BadImageFormatException())).Message);
    }

    [Fact]
    public void ErrorTextCutsWholeCharactersAndOneFullStop()
    {
        Assert.Equal("ab", Psdds.Utf8Text("abé"u8[..3]));
        Assert.Equal("ab€", Psdds.Utf8Text("ab€"u8));
        Assert.Equal("a.", Psdds.Utf8Text("a.."u8));
        Assert.Equal("a", Psdds.Utf8Text("a."u8));
    }

    [Fact]
    public void DefaultsAndLayout()
    {
        // psdds.h ABI 4 (Note DH4): 8 fields, alpha_mode at 28; controls 4 fields, alpha_mode_enabled at 12
        Assert.Equal(32, Marshal.SizeOf<PsddsSettings>());
        Assert.Equal(28, (int)Marshal.OffsetOf<PsddsSettings>(nameof(PsddsSettings.AlphaMode)));
        Assert.Equal(16, Marshal.SizeOf<PsddsControls>());
        Assert.Equal(12, (int)Marshal.OffsetOf<PsddsControls>(nameof(PsddsControls.AlphaModeEnabled)));
        foreach (var t in new[] { Psdds.Colour, Psdds.Normal, Psdds.LinearMask })
        {
            var d = Psdds.Defaults(t);
            Assert.Equal(BC7, d.Format);
            Assert.Equal(0u, d.Srgb);
            Assert.Equal(1u, d.Mips);
            Assert.Equal(0u, d.DiscardAlpha);
            Assert.Equal(0u, d.AlphaMode);
            Assert.Equal(t, d.TextureType);
        }
    }

    [Fact]
    public void Presets()
    {
        Assert.Equal(5, Psdds.PresetCount());
        var names = Enumerable.Range(0, 5).Select(Psdds.PresetName).ToArray();
        Assert.Equal(new[] { "Colour BC7", "Colour BC1 (small, opaque)", "Normal BC7", "Mask/data BC7", "Greyscale mask BC4" }, names);
        for (int i = 0; i < 5; i++)
        {
            var s = Psdds.PresetSettings(i);
            Assert.Equal(0u, s.Srgb);
            Assert.Equal(i is 1 or 4 ? 1u : 0u, s.DiscardAlpha); // BC1 and BC4 store no alpha (Note DF2)
            Assert.Equal(0u, s.AlphaMode);
            Assert.True(Psdds.TryResolve(s, false, out var shown, out _));
            Assert.Equal(s, shown);
            Assert.True(Psdds.TryResolve(s, true, out var shownA, out _));
            Assert.Equal(s with { Filter = shownA.Filter }, shownA);
        }
        Assert.Throws<PsddsException>(() => Psdds.PresetSettings(5));
        Assert.Throws<PsddsException>(() => Psdds.PresetName(5));
    }

    [Fact]
    public void NormalForcesSrgbOff()
    {
        var s = Psdds.Defaults(Psdds.Normal) with { Srgb = 1 };
        Assert.True(Psdds.TryResolve(s, false, out var shown, out var c));
        Assert.Equal(0u, shown.Srgb);
        Assert.Equal(0u, c.SrgbEnabled);
    }

    [Fact]
    public void FormatsPerType()
    {
        Assert.False(Psdds.FormatAllowed(Psdds.Colour, BC4));
        Assert.False(Psdds.FormatAllowed(Psdds.Colour, BC5));
        Assert.True(Psdds.FormatAllowed(Psdds.Normal, BC5));
        Assert.False(Psdds.FormatAllowed(Psdds.Normal, BC4));
        for (uint f = 0; f < 6; f++) Assert.True(Psdds.FormatAllowed(Psdds.LinearMask, f));
        Assert.True(Psdds.TryResolve(Colour(BC4), false, out var shown, out _));
        Assert.Equal(Psdds.Defaults(Psdds.Colour).Format, shown.Format);
    }

    [Fact]
    public void FilterGreying()
    {
        Assert.True(Psdds.TryResolve(Colour(mips: 0), false, out _, out var c0));
        Assert.Equal(0u, c0.FilterEnabled);
        foreach (uint srgb in new[] { 0u, 1u })
        {
            Assert.True(Psdds.TryResolve(Colour(srgb: srgb, filter: Kaiser), false, out var shown, out var c));
            Assert.Equal(1u, c.FilterEnabled);
            Assert.Equal(Kaiser, shown.Filter);
            // alpha kept as data (alpha_mode 0): the filter stays editable (Note DH6); as transparency it greys at Box
            Assert.True(Psdds.TryResolve(Colour(srgb: srgb, filter: Kaiser), true, out var shownD, out var cD));
            Assert.Equal(1u, cD.FilterEnabled);
            Assert.Equal(Kaiser, shownD.Filter);
            Assert.True(Psdds.TryResolve(Colour(srgb: srgb, filter: Kaiser, alphaMode: 1), true, out var shownA, out var cA));
            Assert.Equal(0u, cA.FilterEnabled);
            Assert.Equal(Box, shownA.Filter);
        }
    }

    [Fact]
    public void DiscardGreying()
    {
        foreach (uint f in new[] { BC1, BC4, BC5 })
        {
            var s = Psdds.Defaults(Psdds.LinearMask) with { Format = f };
            Assert.True(Psdds.TryResolve(s, true, out var shown, out _, out bool enabled));
            Assert.False(enabled);
            Assert.Equal(1u, shown.DiscardAlpha); // shown ticked whatever the raw pick
        }
        foreach (uint f in new[] { 1u /* BC3 */, BC7, RGBA8 })
            foreach (uint d in new[] { 0u, 1u })
            {
                Assert.True(Psdds.TryResolve(Colour(f, discard: d), true, out var shown, out _, out bool enabled));
                Assert.True(enabled);
                Assert.Equal(d, shown.DiscardAlpha);
            }
    }

    [Fact]
    public void AlphaModeGreying()
    {
        foreach (uint mode in new[] { 0u, 1u })
        {
            Assert.True(Psdds.TryResolve(Colour(alphaMode: mode), true, out var kept, out var c));
            Assert.Equal(1u, c.AlphaModeEnabled);
            Assert.Equal(mode, kept.AlphaMode);
            // not kept: no document alpha, Discard ticked, or a format without alpha; the effective value is then 0 (DH3)
            foreach (var (s, a) in new[] { (Colour(alphaMode: mode), false), (Colour(discard: 1, alphaMode: mode), true), (Colour(BC1, alphaMode: mode), true) })
            {
                Assert.True(Psdds.TryResolve(s, a, out var shown, out var g));
                Assert.Equal(0u, g.AlphaModeEnabled);
                Assert.Equal(0u, shown.AlphaMode);
            }
        }
        Assert.False(Psdds.TryResolve(Colour(alphaMode: 2), true, out _, out _));
    }

    [Fact]
    public void SrgbOnlyChangesTheTag()
    {
        var px = Image(8, 8);
        var on = Enc(px, 8, 8, false, Colour(srgb: 1));
        var off = Enc(px, 8, 8, false, Colour(srgb: 0));
        Assert.NotEqual(on[128], off[128]);
        Assert.Equal(on.AsSpan(148).ToArray(), off.AsSpan(148).ToArray());
    }

    [Fact]
    public void QualityFallbackAndRejects()
    {
        var s = Psdds.Defaults(Psdds.LinearMask) with { Format = BC4, Quality = 4 };
        Assert.True(Psdds.TryResolve(s, false, out var shown, out _));
        Assert.True(shown.Quality < Psdds.QualityLabels(BC4).Count);
        Assert.False(Psdds.TryResolve(Colour(99), false, out _, out _));
        Assert.False(Psdds.TryResolve(Colour(discard: 2), false, out _, out _));
        Assert.Empty(Psdds.QualityLabels(RGBA8));
        Assert.NotEmpty(Psdds.QualityLabels(BC7));
    }

    [Fact]
    public void CheckSize()
    {
        Assert.Null(Psdds.CheckSize(64, 64));
        var t = Psdds.CheckSize(uint.MaxValue / 2, 1);
        Assert.NotNull(t);
        Assert.False(t!.EndsWith('.'));
        Assert.Contains("larger than", t);
    }

    [Fact]
    public void AlphaUsed()
    {
        static bool Used(params float[] a)
        {
            var px = new float[a.Length * 4];
            for (int i = 0; i < a.Length; i++) px[i * 4 + 3] = a[i];
            fixed (float* p = px) return Psdds.AlphaUsed(p, (nuint)a.Length);
        }
        Assert.False(Used(1, 1, 1, 254 / 255f));
        Assert.True(Used(1, 1, 1, 253 / 255f));
        Assert.False(Used(1, 1, 1, 1));
        Assert.True(Used(1, 1, 1, 0));
    }

    [Fact]
    public void RoundTrips()
    {
        var px = Image(8, 8);
        var bc7 = Psdds.Decode(Enc(px, 8, 8, true, Colour()));
        Assert.Equal((8u, 8u, true), (bc7.Width, bc7.Height, bc7.HasAlpha));
        for (int i = 3; i < bc7.Rgba.Length; i += 4) Assert.True(bc7.Rgba[i] >= 254);

        var r8 = Psdds.Decode(Enc(px, 8, 8, true, Colour(RGBA8)));
        Assert.True(r8.HasAlpha);
        for (int i = 0; i < px.Length; i++)
            Assert.Equal((byte)Math.Round(px[i] * 255), r8.Rgba[i]);

        Assert.False(Psdds.Decode(Enc(px, 8, 8, false, Colour(BC1))).HasAlpha);
    }

    [Fact]
    public void DiscardTransparency()
    {
        var clear = Image(8, 8, (x, y) => (x + y) % 3 == 0 ? 0.25f : 1f);
        var opaque = Image(8, 8);
        var discarded = Enc(clear, 8, 8, true, Colour(discard: 1));
        Assert.Equal(Enc(opaque, 8, 8, false, Colour()), discarded);
        Assert.NotEqual(Enc(clear, 8, 8, true, Colour(discard: 0)), discarded);
    }

    [Fact]
    public void OversizeReportsCheckSizeText()
    {
        var px = new float[20000 * 4];
        var e = Assert.Throws<PsddsException>(() => Enc(px, 20000, 1, true, Colour()));
        Assert.Equal(Psdds.CheckSize(20000, 1), e.Message);
    }

    [Fact]
    public void SpanOverloadChecksLength()
    {
        Assert.Throws<ArgumentException>(() => Psdds.Encode(new float[8 * 8 * 4 - 1], 8, 8, false, Colour()));
        Assert.Equal(Enc(Image(8, 8), 8, 8, false, Colour()), Psdds.Encode(Image(8, 8), 8, 8, false, Colour()));
    }

    [Fact]
    public void DecodeGarbageThrows()
    {
        Assert.Throws<PsddsException>(() => Psdds.Decode(new byte[10]));
        Assert.Throws<PsddsException>(() => Psdds.Decode(ReadOnlySpan<byte>.Empty));
    }

    [Fact]
    public void AlphaUsedSpanChecksShape()
    {
        Assert.Throws<ArgumentException>(() => Psdds.AlphaUsed(new float[5]));
        Assert.True(Psdds.AlphaUsed(new float[] { 1, 1, 1, 0 }));
        Assert.False(Psdds.AlphaUsed(new float[] { 1, 1, 1, 1 }));
    }

    [Fact]
    public void ConcurrentEncodesMatchSequential()
    {
        var px = Image(256, 256);
        var seq = Enc(px, 256, 256, false, Colour());
        var tasks = Enumerable.Range(0, 2).Select(_ => Task.Run(() => Enc(px, 256, 256, false, Colour()))).ToArray();
        Task.WaitAll(tasks);
        foreach (var t in tasks) Assert.Equal(seq, t.Result);
    }
}
