using System.Text;

namespace Tessalith.Dds.Tests;

public sealed class PresetsTests : IDisposable
{
    const string Header = "# Tessalith DDS presets: name, format, type, sRGB, filter, mips, quality, discard transparency";
    readonly string _dir = Path.Combine(Path.GetTempPath(), "tessalith-pdn-presets-" + Guid.NewGuid().ToString("N"));
    string FilePath => Path.Combine(_dir, PresetStore.FileName);

    public PresetsTests() => Directory.CreateDirectory(_dir);
    public void Dispose() { try { Directory.Delete(_dir, true); } catch (IOException) { } }

    static List<Preset> Seeds() =>
        Enumerable.Range(0, Psdds.PresetCount()).Select(i => new Preset(Psdds.PresetName(i), Psdds.PresetSettings(i))).ToList();

    static PsddsSettings S(uint format, uint type, uint srgb, uint filter, uint mips, uint quality, uint discard) =>
        new() { Format = format, TextureType = type, Srgb = srgb, Filter = filter, Mips = mips, Quality = quality, DiscardAlpha = discard };

    static string Line(string name, PsddsSettings s) =>
        $"{name}\t{s.Format}\t{s.TextureType}\t{s.Srgb}\t{s.Filter}\t{s.Mips}\t{s.Quality}\t{s.DiscardAlpha}";

    void WriteFile(params string[] lines) => File.WriteAllText(FilePath, string.Join("\r\n", lines) + "\r\n", new UTF8Encoding(false));

    [Fact]
    public void Presets_seed_an_absent_file()
    {
        var f = PresetStore.Load(_dir);
        Assert.True(f.Complete);
        Assert.Equal(5, f.List.Count);
        Assert.Equal(Seeds(), f.List);
        Assert.True(File.Exists(FilePath));
        Assert.Equal(Seeds(), PresetStore.Load(_dir).List);
    }

    [Fact]
    public void Presets_round_trip()
    {
        var list = new List<Preset>
        {
            new("日本語 #1 😀", S(4, 0, 1, 3, 1, 2, 1)),
            new("#hash", S(2, 2, 0, 1, 0, 0, 0)),
        };
        Assert.True(PresetStore.Save(_dir, new PresetFile(list, true, [])));
        var back = PresetStore.Load(_dir);
        Assert.Equal(list, back.List);
        Assert.Empty(back.Extra);
        var first = File.ReadAllLines(FilePath)[0];
        Assert.DoesNotContain('\t', first);
        Assert.Equal(Header, first);
        Assert.Contains("\r\n", File.ReadAllText(FilePath));
    }

    [Fact]
    public void Presets_empty_file_stays_empty()
    {
        Assert.True(PresetStore.Save(_dir, new PresetFile([], true, [])));
        var f = PresetStore.Load(_dir);
        Assert.Empty(f.List);
        Assert.True(f.Complete);
        Assert.Empty(PresetStore.Load(_dir).List);
    }

    [Fact]
    public void Presets_unreadable_file_is_left_alone()
    {
        WriteFile(Header, Line("Mine", S(4, 0, 0, 0, 1, 0, 0)));
        var before = File.ReadAllBytes(FilePath);
        using (new FileStream(FilePath, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
        {
            var f = PresetStore.Load(_dir);
            Assert.Empty(f.List);
            Assert.False(f.Complete);
            Assert.False(PresetStore.Save(_dir, f));
        }
        Assert.Equal(before, File.ReadAllBytes(FilePath));

        var big = Encoding.UTF8.GetBytes(Header + "\r\n" + new string('x', 1 << 20));
        File.WriteAllBytes(FilePath, big);
        var g = PresetStore.Load(_dir);
        Assert.Empty(g.List);
        Assert.False(g.Complete);
        Assert.False(PresetStore.Save(_dir, g));
        Assert.Equal(big, File.ReadAllBytes(FilePath));
    }

    static readonly string Good1 = Line("Good one", S(4, 0, 1, 3, 1, 1, 0));
    static readonly string Good2 = Line("Good two", S(2, 2, 0, 0, 0, 0, 1));
    static readonly string[] Corrupt =
    [
        "seven\t4\t0\t0\t0\t1\t0",
        "nine\t4\t0\t0\t0\t1\t0\t0\t0",
        "hex\t0x4\t0\t0\t0\t1\t0\t0",
        "\t4\t0\t0\t0\t1\t0\t0",
        new string('n', 65) + "\t4\t0\t0\t0\t1\t0\t0",
        "ctl\u0001x\t4\t0\t0\t0\t1\t0\t0",
        " leading\t4\t0\t0\t0\t1\t0\t0",
        "discard2\t4\t0\t0\t0\t1\t0\t2",
        "format99\t99\t0\t0\t0\t1\t0\t0",
    ];

    void WriteCorrupt() => WriteFile([Header, Corrupt[0], Corrupt[1], Good1, Corrupt[2], Corrupt[3], Corrupt[4], Corrupt[5], Good2, Corrupt[6], Corrupt[7], Corrupt[8]]);

    [Fact]
    public void Presets_corrupt_lines_are_skipped()
    {
        WriteCorrupt();
        var f = PresetStore.Load(_dir);
        Assert.True(f.Complete);
        Assert.Equal(["Good one", "Good two"], f.List.Select(p => p.Name));
        Assert.Equal(S(4, 0, 1, 3, 1, 1, 0), f.List[0].Raw);
    }

    [Fact]
    public void Presets_unparsed_lines_survive()
    {
        WriteCorrupt();
        var f = PresetStore.Load(_dir);
        Assert.Equal(Corrupt, f.Extra);
        Assert.True(PresetStore.Save(_dir, f));
        var lines = File.ReadAllLines(FilePath);
        Assert.Equal([Header, Good1, Good2, .. Corrupt], lines);
        var once = File.ReadAllBytes(FilePath);
        Assert.True(PresetStore.Save(_dir, PresetStore.Load(_dir)));
        Assert.Equal(once, File.ReadAllBytes(FilePath));
        Assert.True(PresetStore.Save(_dir, PresetStore.Load(_dir)));
        Assert.Equal(once, File.ReadAllBytes(FilePath));
    }

    [Fact]
    public void Presets_foreign_bytes_survive()
    {
        // A Windows-1252 "café" line, a lone 0xFF, and a line starting with a UTF-16 LE BOM: none parses (presets.cpp
        // keeps them as unparsed lines), and each must be written back as the same bytes.
        var bytes = new List<byte>();
        void Add(params byte[] b) { bytes.AddRange(b); bytes.Add(13); bytes.Add(10); }
        Add(Encoding.UTF8.GetBytes(Header));
        Add(0xFF, 0xFE, (byte)'x');
        Add(Encoding.UTF8.GetBytes(Good1));
        Add(0x63, 0x61, 0x66, 0xE9);
        Add(0xFF);
        File.WriteAllBytes(FilePath, bytes.ToArray());

        var f = PresetStore.Load(_dir);
        Assert.Single(f.List);
        Assert.Equal(3, f.Extra.Count);
        Assert.True(PresetStore.Save(_dir, f));
        var expected = new List<byte>();
        foreach (var part in new[] { Encoding.UTF8.GetBytes(Header), Encoding.UTF8.GetBytes(Good1), [0xFF, 0xFE, (byte)'x'], [0x63, 0x61, 0x66, 0xE9], new byte[] { 0xFF } })
        {
            expected.AddRange(part);
            expected.AddRange("\r\n"u8.ToArray());
        }
        var once = File.ReadAllBytes(FilePath);
        Assert.Equal(expected.ToArray(), once);
        Assert.True(PresetStore.Save(_dir, PresetStore.Load(_dir)));
        Assert.True(PresetStore.Save(_dir, PresetStore.Load(_dir)));
        Assert.Equal(once, File.ReadAllBytes(FilePath));
    }

    [Fact]
    public void Presets_first_run_creates_the_folder()
    {
        var dir = Path.Combine(_dir, "new", "sub");
        var f = PresetStore.Load(dir);
        Assert.True(f.Complete);
        Assert.Equal(Seeds(), f.List);
        Assert.True(File.Exists(Path.Combine(dir, PresetStore.FileName)));
    }

    [Fact]
    public void Presets_size_cap_boundary()
    {
        var head = Encoding.UTF8.GetBytes(Header + "\r\n" + Good1 + "\r\n");
        var exact = new byte[1 << 20];
        head.CopyTo(exact, 0);
        exact.AsSpan(head.Length).Fill((byte)'x');
        File.WriteAllBytes(FilePath, exact);
        var f = PresetStore.Load(_dir);
        Assert.True(f.Complete);
        Assert.Single(f.List);

        File.WriteAllBytes(FilePath, [.. exact, (byte)'x']);
        Assert.False(PresetStore.Load(_dir).Complete);
    }

    [Fact]
    public void Presets_empty_lines_and_header_are_not_extra()
    {
        WriteFile(Header, "", Good1, "", Header);
        var f = PresetStore.Load(_dir);
        Assert.Single(f.List);
        Assert.Empty(f.Extra);
    }

    [Fact]
    public void Presets_save_failure_keeps_the_old_file()
    {
        var parentFile = Path.Combine(_dir, "afile");
        File.WriteAllText(parentFile, "x");
        Assert.False(PresetStore.Save(Path.Combine(parentFile, "sub"), new PresetFile(Seeds(), true, [])));

        WriteFile(Header, Good1);
        var before = File.ReadAllBytes(FilePath);
        using (new FileStream(FilePath, FileMode.Open, FileAccess.ReadWrite, FileShare.None))
            Assert.False(PresetStore.Save(_dir, new PresetFile(Seeds(), true, [])));
        Assert.Equal(before, File.ReadAllBytes(FilePath));
        Assert.Empty(Directory.GetFiles(_dir, "*.tmp"));
    }

    [Fact]
    public void Presets_incomplete_file_is_never_written()
    {
        WriteFile(Header, Good1);
        var before = File.ReadAllBytes(FilePath);
        Assert.False(PresetStore.Save(_dir, new PresetFile(Seeds(), false, [])));
        Assert.Equal(before, File.ReadAllBytes(FilePath));
        File.Delete(FilePath);
        var f = PresetStore.Load(_dir);
        Assert.True(f.Complete);
        Assert.Equal(Seeds(), f.List);
    }

    [Fact]
    public void Preset_names()
    {
        Assert.Equal("abc", PresetStore.CleanName("  abc \t"));
        Assert.Equal("abc", PresetStore.CleanName("\u00A0\u3000abc\u3000\u00A0"));
        Assert.Equal("a\u00A0b", PresetStore.CleanName("a\u00A0b"));
        Assert.Equal("\u2002abc", PresetStore.CleanName("\u2002abc")); // string.Trim would remove it; presets.cpp does not
        Assert.Equal(new string('a', 64), PresetStore.CleanName(" " + new string('a', 64) + " "));
        Assert.Null(PresetStore.CleanName(new string('a', 65)));
        Assert.Null(PresetStore.CleanName(""));
        Assert.Null(PresetStore.CleanName(" \t\u3000 "));
        Assert.Null(PresetStore.CleanName("a\tb"));
        Assert.Null(PresetStore.CleanName("a\nb"));
        Assert.Null(PresetStore.CleanName("a\u007fb"));

        var list = Seeds();
        Assert.Equal(0, PresetStore.Find(list, "colour bc7"));
        Assert.Equal(-1, PresetStore.Find(list, "nope"));
        var raw = S(0, 0, 1, 0, 1, 0, 0);
        Assert.Equal(0, PresetStore.Upsert(list, "COLOUR BC7", raw));
        Assert.Equal(5, list.Count);
        Assert.Equal(new Preset("COLOUR BC7", raw), list[0]);
        Assert.Equal(5, PresetStore.Upsert(list, "New", raw));
        Assert.Equal(6, list.Count);
    }

    [Fact]
    public void Preset_matching()
    {
        var list = PresetStore.Load(_dir).List;
        var raw = list[0].Raw;
        Assert.Equal(0, PresetStore.Match(list, raw, false));
        raw.Srgb = 1;
        Assert.Equal(-1, PresetStore.Match(list, raw, false));
        Assert.Equal(-1, PresetStore.Match(list, raw, true));
        raw.Srgb = 0;
        Assert.Equal(0, PresetStore.Match(list, raw, false));

        var normal = list[2].Raw;
        normal.Srgb = 1;
        Assert.Equal(2, PresetStore.Match(list, normal, false));

        var bc1 = list[1].Raw;
        bc1.DiscardAlpha = 0;
        Assert.Equal(-1, PresetStore.Match(list, bc1, true));
        Assert.Equal(1, PresetStore.Match(list, bc1, false));
        bc1.DiscardAlpha = 1;
        Assert.Equal(1, PresetStore.Match(list, bc1, true));
        Assert.Equal(1, PresetStore.Match(list, bc1, false));

        Assert.Equal(-1, PresetStore.Match([], raw, false));
        list.RemoveAt(0);
        Assert.Equal(-1, PresetStore.Match(list, raw, false));
    }

    [Fact]
    public void Preset_matching_dc40()
    {
        var list = Seeds();
        var raw = list[0].Raw;
        raw.DiscardAlpha = 1;
        Assert.Equal(0, PresetStore.Match(list, raw, false));
        Assert.Equal(-1, PresetStore.Match(list, raw, true));

        list.Add(new Preset("Twin", list[0].Raw));
        Assert.Equal(5, PresetStore.Match(list, list[0].Raw, false, preferred: 5));
        Assert.Equal(0, PresetStore.Match(list, list[0].Raw, false, preferred: -1));
        Assert.Equal(0, PresetStore.Match(list, list[0].Raw, false, preferred: 2)); // 2 no longer matches
    }

    [Fact]
    public void PresetSelection_dc45()
    {
        var list = Seeds();
        var raw = list[0].Raw;
        var sel = new PresetSelection();
        Assert.Equal(0, sel.Shown(list, raw, false));
        Assert.True(sel.CanDelete);

        sel.PickCustom();
        Assert.Equal(-1, sel.Shown(list, raw, false));
        Assert.False(sel.CanDelete);
        raw = list[2].Raw; // an edit that lands on seed 2's values
        Assert.Equal(-1, sel.Shown(list, raw, false));
        Assert.True(sel.Custom);

        sel.Pick(2);
        Assert.Equal(2, sel.Shown(list, raw, false));
        Assert.True(sel.CanDelete);

        sel.PickCustom();
        list.Add(new Preset("Saved", raw));
        sel.Saved(5);
        Assert.Equal(5, sel.Shown(list, raw, false));
        Assert.False(sel.Custom);

        Assert.Equal(2, new PresetSelection().Shown(list, raw, false));
    }

    [Fact]
    public void PresetSelection_reset_rematches_like_an_edit()
    {
        var list = Seeds();
        list.Add(new Preset("Twin", list[0].Raw));
        var sel = new PresetSelection();
        sel.PickCustom();
        sel.Reset(); // the host's Defaults button: a stale Custom does not survive
        Assert.Equal(0, sel.Shown(list, list[0].Raw, false));
        Assert.False(sel.Custom);

        sel.Pick(5);
        sel.Reset(); // nor a stale twin pick
        Assert.Equal(0, sel.Shown(list, list[0].Raw, false));
    }

    [Fact]
    public void PresetSelection_reset_after_delete()
    {
        var list = Seeds();
        list.Add(new Preset("Twin A", list[0].Raw));
        list.Add(new Preset("Twin B", list[0].Raw));
        var raw = list[0].Raw;
        var sel = new PresetSelection();
        sel.Pick(5);
        Assert.Equal(5, sel.Shown(list, raw, false));
        list.RemoveAt(5);
        sel.Deleted();
        Assert.Equal(-1, sel.Picked);
        Assert.Equal(0, sel.Shown(list, raw, false)); // without the reset, index 5 (now Twin B) would stay shown
    }
}

[CollectionDefinition(Name, DisableParallelization = true)]
public sealed class PresetDirEnvCollection { public const string Name = "Preset dir environment"; }

// Tasks 5/6: any test that calls PresetStore.Dir(), loads from the default folder, or sets TESSALITH_DDS_PRESETS_DIR
// or APPDATA joins this collection, so it never runs beside this one. Preset I/O tests pass a temp dir instead.
[Collection(PresetDirEnvCollection.Name)]
public sealed class PresetDirEnvTests
{
    [Fact]
    public void Preset_dir_env()
    {
        var dir = Path.Combine(Path.GetTempPath(), "tessalith-pdn-presets-env");
        var env = Environment.GetEnvironmentVariable("TESSALITH_DDS_PRESETS_DIR");
        var appdata = Environment.GetEnvironmentVariable("APPDATA");
        try
        {
            Environment.SetEnvironmentVariable("TESSALITH_DDS_PRESETS_DIR", dir);
            Assert.Equal(dir, PresetStore.Dir());
            Environment.SetEnvironmentVariable("TESSALITH_DDS_PRESETS_DIR", null);
            Environment.SetEnvironmentVariable("APPDATA", null);
            Assert.Null(PresetStore.Dir());
            Environment.SetEnvironmentVariable("APPDATA", @"C:\x");
            Assert.Equal(@"C:\x\Tessalith DDS\presets", PresetStore.Dir());
        }
        finally
        {
            Environment.SetEnvironmentVariable("TESSALITH_DDS_PRESETS_DIR", env);
            Environment.SetEnvironmentVariable("APPDATA", appdata);
        }
    }
}