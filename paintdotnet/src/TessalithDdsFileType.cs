using System;
using System.IO;
using PaintDotNet;
using PaintDotNet.Rendering;

namespace Tessalith.Dds;

public sealed class TessalithDdsFactory : IFileTypeFactory2
{
    public FileType[] GetFileTypeInstances(IFileTypeHost host) => [new TessalithDdsFileType()];
}

public sealed class TessalithDdsSupportInfo : IPluginSupportInfo
{
    public string DisplayName => "Tessalith DDS";
    public string Author => "Alistraza";
    public string Copyright => "Copyright (c) 2026 Alistraza. GPL-3.0-or-later with a Paint.NET exception";
    public Version Version => typeof(TessalithDdsSupportInfo).Assembly.GetName().Version ?? new Version();
    public Uri WebsiteUri => new("https://github.com/Alistra-za/Tessalith");
}

// Never rename the class or namespace: the host keys the remembered settings on "<namespace>.<type>.BinaryFormatter".
[PluginSupportInfo(typeof(TessalithDdsSupportInfo))]
public sealed unsafe class TessalithDdsFileType : FileType<TessalithDdsToken, TessalithDdsWidget>
{
    public TessalithDdsFileType() : base("Tessalith DDS", new FileTypeOptions
    {
        LoadExtensions = [".dds"], // preview decode only; Open always goes to the built-in DDS type (Owner decision 1)
        SaveExtensions = [".dds"],
    }) { }

    /// <summary>psdds_alpha_used of the last save (the preview's); raised on the save's worker thread (Task 6).</summary>
    internal event Action<bool>? HasAlphaMeasured;

    protected override TessalithDdsToken OnCreateDefaultSaveConfigTokenT() => new() { Raw = Psdds.Defaults(Psdds.Colour) };

    protected override TessalithDdsWidget OnCreateSaveConfigWidgetT() => new(this);

    protected override object GetSerializablePortionOfSaveConfigToken(TessalithDdsToken token) => DialogModel.ToPortion(token.Raw);

    protected override TessalithDdsToken GetSaveConfigTokenFromSerializablePortionT(object portion) => new() { Raw = DialogModel.FromPortion(portion) };

    protected override void OnSaveT(Document input, Stream output, TessalithDdsToken token, Surface scratchSurface, ProgressEventHandler progressCallback)
    {
        scratchSurface.Clear();
        input.CreateRenderer().Render(scratchSurface);
        DdsSave.Write(output, (byte*)scratchSurface.GetRowPointer(0), scratchSurface.Stride, input.Width, input.Height, token.Raw,
            m => HasAlphaMeasured?.Invoke(m));
    }

    protected override Document OnLoad(Stream input)
    {
        if (input.CanSeek && input.Length - input.Position > Array.MaxLength)
            throw new PsddsException("this file is too large to be a DDS this plugin can open");
        var ms = new MemoryStream();
        input.CopyTo(ms); // shortcut: a non-seekable stream past Array.MaxLength fails with MemoryStream's IOException; the preview's is the encoder's own output
        var (rgba, w, h, _) = Psdds.Decode(ms.GetBuffer().AsSpan(0, (int)ms.Length));
        var doc = new Document((int)w, (int)h);
        var layer = Layer.CreateBackgroundLayer((int)w, (int)h);
        fixed (byte* src = rgba)
            for (int y = 0; y < (int)h; y++)
                Pixels.RgbaRowToBgra(src + (long)y * w * 4, (int)w, (byte*)layer.Surface.GetRowPointer(y));
        doc.Layers.Add(layer);
        return doc;
    }
}
