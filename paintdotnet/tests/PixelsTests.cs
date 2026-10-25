namespace Tessalith.Dds.Tests;

public unsafe class PixelsTests
{
    static float[] Convert(byte[] bgra)
    {
        var f = new float[bgra.Length];
        fixed (byte* s = bgra) fixed (float* d = f) Pixels.BgraRowToRgbaFloats(s, bgra.Length / 4, d);
        return f;
    }

    [Fact]
    public void BgraToFloatsKnownPixel()
    {
        var f = Convert(new byte[] { 10, 20, 30, 255 });
        Assert.Equal(new[] { 30 / 255f, 20 / 255f, 10 / 255f, 1f }, f);
    }

    [Fact]
    public void RgbaToBgraSwapsRedBlueKeepsAlpha()
    {
        byte[] rgba = { 1, 2, 3, 4, 5, 6, 7, 8 };
        var bgra = new byte[8];
        fixed (byte* s = rgba) fixed (byte* d = bgra) Pixels.RgbaRowToBgra(s, 2, d);
        Assert.Equal(new byte[] { 3, 2, 1, 4, 7, 6, 5, 8 }, bgra);
    }

    [Fact]
    public void WidthZeroIsNoOp()
    {
        byte b = 9; float f = 7; byte o = 5;
        Pixels.BgraRowToRgbaFloats(&b, 0, &f);
        Pixels.RgbaRowToBgra(&b, 0, &o);
        Assert.Equal(7f, f);
        Assert.Equal(5, o);
    }

    // The 254/255 edge lives in psdds; this pins that the conversion feeds it correctly.
    [Fact]
    public void AlphaRuleOverConvertedRow()
    {
        var row = new byte[16 * 4];
        for (int i = 0; i < 16; i++) { row[i * 4] = 1; row[i * 4 + 3] = 254; }
        Assert.False(Psdds.AlphaUsed(Convert(row)));
        row[5 * 4 + 3] = 253;
        Assert.True(Psdds.AlphaUsed(Convert(row)));
    }
}