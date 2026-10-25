// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
namespace Tessalith.Dds.Tests;

public class DialogModelTests
{
    const uint BC1 = 0, BC3 = 1, BC4 = 2, BC5 = 3, BC7 = 4, RGBA8 = 5;

    static PsddsSettings Raw(uint type, uint format = BC7, uint srgb = 0, uint discard = 0, uint mips = 1)
    {
        var s = Psdds.Defaults(type);
        s.Format = format; s.Srgb = srgb; s.DiscardAlpha = discard; s.Mips = mips;
        return s;
    }

    static DialogState R(in PsddsSettings raw, bool hasAlpha) => DialogModel.Refresh(raw, hasAlpha) ?? throw new Exception("psdds refused");

    static uint[] Values(IReadOnlyList<Choice> list) => list.Select(c => c.Value).ToArray();

    [Fact]
    public void Defaults_dc34_dc39_srgb_off_keeps_filter_editable()
    {
        foreach (var raw in new[] { Psdds.Defaults(Psdds.Colour), DialogModel.FromPortion(null) })
        {
            var off = R(raw, false);
            Assert.Equal(0u, off.Shown.Srgb);
            Assert.True(off.FilterEnabled);
            var on = raw; on.Srgb = 1;
            var s = R(on, false);
            Assert.Equal(off.FilterEnabled, s.FilterEnabled);
            Assert.Equal(off.Shown.Filter, s.Shown.Filter);
        }
    }

    [Fact]
    public void Discard_dc36_ungreys_the_filter()
    {
        var keep = R(Raw(Psdds.Colour, BC7, srgb: 1, discard: 0), true);
        Assert.False(keep.FilterEnabled);
        var discard = R(Raw(Psdds.Colour, BC7, srgb: 1, discard: 1), true);
        Assert.True(discard.FilterEnabled);
        Assert.Equal(1u, discard.Shown.DiscardAlpha);
    }

    [Fact]
    public void Raw_srgb_kept_dc19()
    {
        var raw = Raw(Psdds.Colour, srgb: 1);
        raw.TextureType = Psdds.Normal;
        var n = R(raw, false);
        Assert.Equal(0u, n.Shown.Srgb);
        Assert.False(n.SrgbEnabled);
        raw.TextureType = Psdds.Colour;
        var c = R(raw, false);
        Assert.Equal(1u, c.Shown.Srgb);
        Assert.True(c.SrgbEnabled);
    }

    [Fact]
    public void Alpha_greys_the_filter_dc12_dc39()
    {
        foreach (uint srgb in new uint[] { 0, 1 })
        {
            var raw = Raw(Psdds.Colour, BC7, srgb);
            raw.Filter = 3;
            var a = R(raw, true);
            Assert.False(a.FilterEnabled);
            Assert.Equal(0u, a.Shown.Filter);
            Assert.True(R(raw, false).FilterEnabled);
        }
        Assert.True(R(Raw(Psdds.Colour, BC1), true).FilterEnabled);
    }

    [Fact]
    public void Mips_off_greys_the_filter_dc32_dc33()
    {
        var raw = Raw(Psdds.Colour, mips: 0);
        raw.Filter = 2;
        var s = R(raw, false);
        Assert.False(s.FilterEnabled);
        Assert.Equal(2u, s.Shown.Filter);
        raw.Mips = 1;
        Assert.True(R(raw, false).FilterEnabled);
    }

    [Fact]
    public void Formats_per_type_dc32()
    {
        Assert.Equal(new[] { BC1, BC3, BC7, RGBA8 }, Values(R(Raw(Psdds.Colour), false).Formats));
        Assert.Equal(new[] { BC1, BC3, BC5, BC7, RGBA8 }, Values(R(Raw(Psdds.Normal), false).Formats));
        Assert.Equal(new[] { BC1, BC3, BC4, BC5, BC7, RGBA8 }, Values(R(Raw(Psdds.LinearMask), false).Formats));
        Assert.Equal(new[] { "BC1", "BC3", "BC7", "RGBA8 (uncompressed)" }, R(Raw(Psdds.Colour), false).Formats.Select(c => c.Label));
    }

    [Fact]
    public void Raw_format_kept_dc33()
    {
        var raw = Raw(Psdds.Normal, BC5);
        raw.TextureType = Psdds.Colour;
        var c = R(raw, false);
        Assert.Equal(BC7, c.Shown.Format);
        Assert.DoesNotContain(BC5, Values(c.Formats));
        raw.TextureType = Psdds.Normal;
        Assert.Equal(BC5, R(raw, false).Shown.Format);
    }

    [Fact]
    public void Quality_items_follow_the_format()
    {
        foreach (var f in new[] { BC1, BC3, BC7 })
        {
            var s = R(Raw(Psdds.Colour, f), false);
            Assert.Equal(Psdds.QualityLabels(s.Shown.Format), s.Quality.Select(c => c.Label));
            Assert.Equal(Enumerable.Range(0, s.Quality.Count).Select(i => (uint)i), Values(s.Quality));
        }
        var rgba = R(Raw(Psdds.Colour, RGBA8), false);
        Assert.Empty(rgba.Quality);
        Assert.False(rgba.QualityEnabled);
    }

    [Fact]
    public void Codec_round_trip_and_rejects()
    {
        var raw = Raw(Psdds.Colour, BC7, srgb: 1, discard: 1);
        raw.Filter = 3; raw.Quality = 1;
        var p = DialogModel.ToPortion(raw);
        Assert.Equal(7, p.Length);
        Assert.Equal(1, p[6]);
        Assert.Equal(raw, DialogModel.FromPortion(p));

        var six = DialogModel.FromPortion(p[..6]);
        Assert.Equal(raw with { DiscardAlpha = 0 }, six);

        var def = Psdds.Defaults(Psdds.Colour);
        foreach (var bad in new object?[] { null, "x", new int[5], new int[8], new[] { 99, 0, 0, 0, 0, 0, 0 },
                                            new[] { 4, 0, 0, 0, 1, 0, 2 }, new[] { -1, 0, 0, 0, 1, 0, 0 } })
            Assert.Equal(def, DialogModel.FromPortion(bad));

        var mask = Raw(Psdds.LinearMask, BC4);
        mask.Quality = 6;
        Assert.Equal(mask, DialogModel.FromPortion(DialogModel.ToPortion(mask)));

        var cbc5 = Raw(Psdds.Colour, BC5);
        var back = DialogModel.FromPortion(DialogModel.ToPortion(cbc5));
        Assert.Equal(cbc5, back);
        Assert.Equal(BC7, R(back, false).Shown.Format);
    }
}
