// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
namespace Tessalith.Dds.Tests;

public unsafe class DdsSaveTests
{
    // RGBA8 without mips so the decode is byte-exact.
    static PsddsSettings Rgba8(uint discard)
    {
        var s = Psdds.Defaults(Psdds.Colour);
        s.Format = 5; s.Mips = 0; s.DiscardAlpha = discard;
        Assert.True(Psdds.FormatAllowed(Psdds.Colour, 5));
        return s;
    }

    // 4x3 BGRA with a padded stride; alpha varies so has_alpha is measured true.
    static byte[] Bgra(int w, int h, int stride)
    {
        var b = new byte[stride * h];
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
            {
                int i = y * stride + x * 4;
                b[i] = (byte)(10 * x); b[i + 1] = (byte)(20 * y); b[i + 2] = (byte)(30 + x + y); b[i + 3] = (byte)(x == 0 ? 0 : 128);
            }
        return b;
    }

    static (byte[] Dds, bool? Measured) Save(byte[] src, int w, int h, int stride, PsddsSettings s, Stream? output = null)
    {
        var ms = output ?? new MemoryStream();
        bool? measured = null;
        fixed (byte* p = src) DdsSave.Write(ms, p, stride, w, h, s, m => measured = m);
        return (((MemoryStream)ms).ToArray(), measured);
    }

    [Fact]
    public void SaveDecodesBackToTheSourcePixels()
    {
        const int w = 4, h = 3, stride = 20;
        var src = Bgra(w, h, stride);
        var (dds, measured) = Save(src, w, h, stride, Rgba8(0));
        Assert.True(measured);
        var (rgba, dw, dh, hasAlpha) = Psdds.Decode(dds);
        Assert.Equal((4u, 3u, true), (dw, dh, hasAlpha));
        // alpha-0 texels may get their colour bled from neighbours by the engine; compare colour where alpha > 0 only
        for (int y = 0; y < h; y++)
            for (int x = 0; x < w; x++)
            {
                int s = y * stride + x * 4, d = (y * w + x) * 4;
                Assert.Equal(src[s + 3], rgba[d + 3]);
                if (src[s + 3] > 0) Assert.Equal(new[] { src[s + 2], src[s + 1], src[s] }, rgba[d..(d + 3)]);
            }
    }

    [Fact]
    public void DiscardMakesTheSaveOpaqueKeepsColourAndStillMeasures()
    {
        const int w = 4, h = 3, stride = 16;
        var src = Bgra(w, h, stride);
        var (dds, measured) = Save(src, w, h, stride, Rgba8(1));
        Assert.True(measured); // the widget needs it for when the box is unticked
        var (rgba, _, _, _) = Psdds.Decode(dds); // RGBA8 always decodes with has_alpha 1 (the format stores it)
        for (int i = 0; i < w * h; i++)
        {
            int s = i * 4;
            Assert.Equal(new byte[] { src[s + 2], src[s + 1], src[s], 255 }, rgba[s..(s + 4)]);
        }
    }

    [Fact]
    public void OpaquePixelsMeasureNoAlpha()
    {
        var src = new byte[2 * 2 * 4];
        Array.Fill(src, (byte)255);
        var (_, measured) = Save(src, 2, 2, 8, Rgba8(0));
        Assert.False(measured);
    }

    [Fact]
    public void EncodeErrorWritesNothing()
    {
        var s = Rgba8(0);
        s.Format = 99;
        var ms = new MemoryStream();
        Assert.Throws<PsddsException>(() => Save(new byte[16], 2, 2, 8, s, ms));
        Assert.Equal(0, ms.Length);
    }

    [Fact]
    public void SizeRefusalWritesNothingAndMeasuresNothing()
    {
        var ms = new MemoryStream();
        bool called = false;
        var e = Assert.Throws<PsddsException>(() =>
        {
            fixed (byte* p = new byte[4]) DdsSave.Write(ms, p, 4, 0, 1, Rgba8(0), _ => called = true);
        });
        Assert.Equal(Psdds.CheckSize(0, 1), e.Message);
        Assert.False(called);
        Assert.Equal(0, ms.Length);
    }
}
