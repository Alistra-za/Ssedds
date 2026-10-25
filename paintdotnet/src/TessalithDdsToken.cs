using System;
using PaintDotNet;

namespace Tessalith.Dds;

[Serializable]
public sealed class TessalithDdsToken : SaveConfigToken
{
    public PsddsSettings Raw; // raw picks (DC19); the encode resolves again

    public override object Clone() => MemberwiseClone();
}
