// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
using System;
using PaintDotNet;

namespace Tessalith.Dds;

[Serializable]
public sealed class TessalithDdsToken : SaveConfigToken
{
    public PsddsSettings Raw; // raw picks (DC19); the encode resolves again

    public override object Clone() => MemberwiseClone();
}
