// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
using System;
using System.IO;
using System.Windows.Forms;
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

    // Note FC6 (DI9): a missing or wrong-ABI psdds.dll shows one plain message box with the path and the expected ABI
    // when the options dialog would open, instead of the host's raw exception; the dialog then does not open.
    protected override TessalithDdsToken OnCreateDefaultSaveConfigTokenT() =>
        new() { Raw = Compatible() ? Psdds.Defaults(Psdds.Colour) : default }; // the widget says why

    protected override TessalithDdsWidget OnCreateSaveConfigWidgetT()
    {
        try { Psdds.EnsureCompatible(); }
        catch (PsddsException e)
        {
            MessageBox.Show(e.Message + ".", "Tessalith DDS", MessageBoxButtons.OK, MessageBoxIcon.Error);
            throw;
        }
        return new(this);
    }

    static bool Compatible()
    {
        try { Psdds.EnsureCompatible(); return true; }
        catch (PsddsException) { return false; }
    }

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
