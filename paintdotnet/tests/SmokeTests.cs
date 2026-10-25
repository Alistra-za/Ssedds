// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
namespace Tessalith.Dds.Tests;

public class SmokeTests
{
    [Fact]
    public void PsddsDllIsNextToTests() =>
        Assert.True(File.Exists(Path.Combine(AppContext.BaseDirectory, "psdds.dll")), "psdds.dll missing: run crates\\psdds\\build.ps1, or pass -p:PsddsDll=");
}