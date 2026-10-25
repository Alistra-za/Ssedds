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
    public async Task SetFromAnotherThreadIsSeen()
    {
        var a = new MeasuredAlpha();
        await Task.Run(() => a.Set(true)); // the preview save reports on a worker thread
        Assert.True(a.Value);
    }
}
