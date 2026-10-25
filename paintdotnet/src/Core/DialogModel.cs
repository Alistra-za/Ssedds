// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
using System.Collections.Generic;
using System.Linq;

namespace Tessalith.Dds;

public readonly record struct Choice(string Label, uint Value);

/// <summary>Display labels only (as photoshop/src/dialog.cpp); values are psdds.h enums. sRGB, Mipmaps and Discard transparency are checkboxes.</summary>
public static class DialogLists
{
    public static readonly Choice[] Formats = [new("BC1", 0), new("BC3", 1), new("BC4", 2), new("BC5", 3), new("BC7", 4), new("RGBA8 (uncompressed)", 5)];
    public static readonly Choice[] Types = [new("Colour", 0), new("Normal", 1), new("Linear mask", 2)];
    public static readonly Choice[] Filters = [new("Box", 0), new("Tent", 1), new("Mitchell", 2), new("Kaiser", 3)];
}

/// <summary>What the dialog shows for a raw pick: effective values, enabled states, and the Format and Quality lists.</summary>
public sealed record DialogState(PsddsSettings Shown, bool SrgbEnabled, bool FilterEnabled, bool QualityEnabled,
                                 IReadOnlyList<Choice> Formats, IReadOnlyList<Choice> Quality);

/// <summary>dialog.cpp's FromResolve and refresh lists, plus the token codec. Every rule is psdds's (DC19, DC32, DC33).</summary>
public static class DialogModel
{
    /// <summary>null when psdds refuses the raw values (unreachable from the controls).</summary>
    public static DialogState? Refresh(in PsddsSettings raw, bool hasAlpha)
    {
        if (!Psdds.TryResolve(raw, hasAlpha, out var shown, out var c)) return null;
        var formats = DialogLists.Formats.Where(f => Psdds.FormatAllowed(shown.TextureType, f.Value)).ToArray();
        var quality = Psdds.QualityLabels(shown.Format).Select((label, i) => new Choice(label, (uint)i)).ToArray();
        return new DialogState(shown, c.SrgbEnabled != 0, c.FilterEnabled != 0, c.QualityEnabled != 0, formats, quality);
    }

    /// <summary>Seven ints, DiscardAlpha last; a plain array BinaryFormatter can store.</summary>
    public static int[] ToPortion(in PsddsSettings raw) =>
        [(int)raw.Format, (int)raw.TextureType, (int)raw.Srgb, (int)raw.Filter, (int)raw.Mips, (int)raw.Quality, (int)raw.DiscardAlpha];

    /// <summary>The raw values of a stored portion (int[7], or int[6] without DiscardAlpha, DC36) that psdds accepts;
    /// anything else gives Colour's defaults without an error (DC19).</summary>
    public static PsddsSettings FromPortion(object? portion)
    {
        if (portion is int[] { Length: 6 or 7 } a && a.All(v => v >= 0))
        {
            var raw = new PsddsSettings
            {
                Format = (uint)a[0], TextureType = (uint)a[1], Srgb = (uint)a[2], Filter = (uint)a[3],
                Mips = (uint)a[4], Quality = (uint)a[5], DiscardAlpha = a.Length == 7 ? (uint)a[6] : 0,
            };
            if (Psdds.TryResolve(raw, false, out _, out _)) return raw;
        }
        return Psdds.Defaults(Psdds.Colour);
    }
}
