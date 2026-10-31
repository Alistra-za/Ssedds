// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
namespace Tessalith.Dds.Tests;

public class MeasuredAlphaTests
{
    [Fact]
    public void UnknownUntilSetThenLastValueWins()
    {
        var a = new MeasuredAlpha();
        Assert.Null(a.Value); // the widget then shows the filter as if opaque
        a.Set(true);
        Assert.True(a.Value);
        a.Set(false);
        Assert.False(a.Value);
    }

    [Fact]
    public void SetReportsWhetherTheValueChanged()
    {
        var a = new MeasuredAlpha();
        Assert.True(a.Set(false)); // unknown -> known
        Assert.False(a.Set(false)); // same value: the widget skips its refresh
        Assert.True(a.Set(true));
    }

    [Fact]
    public void Alpha254NoteIsItsOwnState() // Note FC6: the dialog's status line
    {
        var a = new MeasuredAlpha();
        Assert.False(a.Note);
        Assert.True(a.Set(false, note: true));
        Assert.True(a.Note);
        Assert.False(a.Value); // still opaque for the greying
        Assert.True(a.Set(false)); // Discard ticked: the note goes
        Assert.False(a.Note);
        Assert.False(a.Set(false));
    }

    [Fact]
    public async Task SetFromAnotherThreadIsSeen()
    {
        var a = new MeasuredAlpha();
        await Task.Run(() => a.Set(true)); // the preview save reports on a worker thread
        Assert.True(a.Value);
    }
}
