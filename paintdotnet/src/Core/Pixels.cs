namespace Tessalith.Dds;

public static unsafe class Pixels
{
    // One row of straight-alpha BGRA8 (B,G,R,A) to RGBA floats v/255 (DC6).
    // shortcut: scalar loop, about 0.3 s for 16k²; vectorise if saves feel slow.
    public static void BgraRowToRgbaFloats(byte* bgra, int width, float* rgba)
    {
        for (int i = 0; i < width; i++)
        {
            byte* s = bgra + i * 4;
            float* d = rgba + i * 4;
            d[0] = s[2] / 255f;
            d[1] = s[1] / 255f;
            d[2] = s[0] / 255f;
            d[3] = s[3] / 255f;
        }
    }

    // One row of RGBA8 (psdds_decode output) to BGRA8.
    public static void RgbaRowToBgra(byte* rgba, int width, byte* bgra)
    {
        for (int i = 0; i < width; i++)
        {
            byte* s = rgba + i * 4;
            byte* d = bgra + i * 4;
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
    }
}