// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
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

    /// <summary>psdds_alpha_used of the last save (the preview's); raised on the save's worker thread. An instance event
    /// suffices: this is the one instance per process, and the host builds the widget from it and previews through it.</summary>
    internal event Action<bool, bool>? HasAlphaMeasured;

    // Note FC6.5 (DI9): a missing or wrong-ABI psdds.dll never throws while the options dialog is built: that throw would
    // escape Paint.NET's save catch (DocumentWorkspace.GetSaveConfigToken runs before it) to the crash handler. The dialog
    // opens with its options greyed, and the preview's OnSaveT throws the plain PsddsException (path and expected ABI),
    // which the dialog's preview shows once in Paint.NET's own error box.
    protected override TessalithDdsToken OnCreateDefaultSaveConfigTokenT() => new() { Raw = Psdds.ColourDefaults };

    protected override TessalithDdsWidget OnCreateSaveConfigWidgetT() => new(this);

    protected override object GetSerializablePortionOfSaveConfigToken(TessalithDdsToken token) => DialogModel.ToPortion(token.Raw);

    protected override TessalithDdsToken GetSaveConfigTokenFromSerializablePortionT(object portion) => new() { Raw = DialogModel.FromPortion(portion) };

    protected override void OnSaveT(Document input, Stream output, TessalithDdsToken token, Surface scratchSurface, ProgressEventHandler progressCallback)
    {
        // before the render too: the preview re-saves on every dialog change (DdsSave.Write checks again before allocating)
        if (Psdds.CheckSize((uint)input.Width, (uint)input.Height) is { } why) throw new PsddsException(why);
        // DdsSave reads width x height pixels from row 0 at Stride: a surface of another size would be read out of bounds
        if (scratchSurface.Width != input.Width || scratchSurface.Height != input.Height)
            throw new InvalidOperationException($"the scratch surface is {scratchSurface.Width}x{scratchSurface.Height}, the image {input.Width}x{input.Height}");
        scratchSurface.Clear();
        input.CreateRenderer().Render(scratchSurface);
        DdsSave.Write(output, (byte*)scratchSurface.GetRowPointer(0), scratchSurface.Stride, input.Width, input.Height, token.Raw,
            (m, note) => HasAlphaMeasured?.Invoke(m, note));
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
