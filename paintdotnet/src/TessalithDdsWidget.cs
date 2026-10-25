using PaintDotNet;

namespace Tessalith.Dds;

// shortcut: keeps the token only, no controls; Task 6 replaces it with the DC33/DC37/DC43 dialog.
public sealed class TessalithDdsWidget : SaveConfigWidget<TessalithDdsFileType, TessalithDdsToken>
{
    PsddsSettings _raw;
    internal bool? HasAlpha; // last psdds_alpha_used from the preview save (ruling 7); Task 6 applies it once the handle exists

    public TessalithDdsWidget(TessalithDdsFileType fileType) : base(fileType)
    {
        fileType.HasAlphaMeasured += OnMeasured; // the FileType lives for the process: unsubscribe on Dispose
    }

    void OnMeasured(bool m) => HasAlpha = m;

    protected override void Dispose(bool disposing)
    {
        if (disposing) FileType.HasAlphaMeasured -= OnMeasured;
        base.Dispose(disposing);
    }

    protected override void InitWidgetFromToken(TessalithDdsToken token) => _raw = token.Raw;

    protected override TessalithDdsToken CreateTokenFromWidget() => new() { Raw = _raw };
}
