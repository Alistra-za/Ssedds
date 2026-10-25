using System;
using System.Collections.Generic;
using System.IO;
using System.Security;
using System.Text;

namespace Tessalith.Dds;

/// <summary>A stored preset. Raw: the seven raw values, as the token keeps them (DC19).</summary>
public sealed record Preset(string Name, PsddsSettings Raw);

/// <summary>What Load read (DC40 PresetFileInfo). Complete false: the file exists but was not read in full and must
/// never be written. Extra: lines that did not parse, written back verbatim after the presets.</summary>
public sealed record PresetFile(List<Preset> List, bool Complete, List<string> Extra);

/// <summary>The dialog's presets in one UTF-8 file, behaviour and format as photoshop/src/presets.cpp (DC37, DC40):
/// a header line without tabs, then name TAB format TAB type TAB srgb TAB filter TAB mips TAB quality TAB discard, CRLF.</summary>
public static class PresetStore
{
    public const string FileName = "paintdotnet.txt"; // not Photoshop's presets.txt (Owner decision 10)
    const string Header = "# Tessalith DDS presets: name, format, type, sRGB, filter, mips, quality, discard transparency";
    const int MaxFileBytes = 1 << 20; // a preset line is under 300 bytes; bounds what a corrupt file can allocate
    static readonly char[] Space = [' ', '\t', '\r', '\n', '\v', '\f', ' ', '　']; // presets.cpp CleanPresetName
    static readonly UTF8Encoding Strict = new(false, true), Lenient = new(false, false);

    /// <summary>TESSALITH_DDS_PRESETS_DIR when set (tests), else %APPDATA%\Tessalith DDS\presets; null when neither is set.</summary>
    public static string? Dir()
    {
        var env = Environment.GetEnvironmentVariable("TESSALITH_DDS_PRESETS_DIR");
        if (!string.IsNullOrEmpty(env)) return env;
        var appdata = Environment.GetEnvironmentVariable("APPDATA");
        return string.IsNullOrEmpty(appdata) ? null : Path.Combine(appdata, "Tessalith DDS", "presets");
    }

    /// <summary>An absent file is created with psdds's seeds, which are returned (Complete). A file that cannot be read
    /// in full (open fails other than not-found, over 1 MiB, short read) gives an empty list, Complete false, and is
    /// left untouched. Lines that do not parse go to Extra; the header and empty lines are dropped.</summary>
    public static PresetFile Load(string dir)
    {
        var file = new PresetFile([], true, []);
        if (string.IsNullOrEmpty(dir)) return file;
        byte[] bytes;
        try
        {
            using var f = new FileStream(Path.Combine(dir, FileName), FileMode.Open, FileAccess.Read, FileShare.Read);
            if (f.Length > MaxFileBytes) return file with { Complete = false };
            bytes = new byte[f.Length];
            f.ReadExactly(bytes);
        }
        catch (Exception e) when (e is FileNotFoundException or DirectoryNotFoundException)
        {
            for (int i = 0; i < Psdds.PresetCount(); i++) file.List.Add(new Preset(Psdds.PresetName(i), Psdds.PresetSettings(i)));
            Save(dir, file); // a failure only means the seeds are offered again next time
            return file;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or SecurityException or NotSupportedException or ArgumentException)
        {
            return file with { Complete = false }; // there but unreadable: never written over (DC40)
        }
        for (int start = 0; start < bytes.Length;)
        {
            int end = Array.IndexOf(bytes, (byte)'\n', start);
            if (end < 0) end = bytes.Length;
            var line = bytes.AsSpan(start, end - start);
            if (line.Length > 0 && line[^1] == '\r') line = line[..^1];
            start = end + 1;
            string? text = null;
            try { text = Strict.GetString(line); } catch (DecoderFallbackException) { }
            if (text != null && ParseLine(text) is { } p) file.List.Add(p);
            // shortcut: a line that is not valid UTF-8 is kept with U+FFFD in place of its bad bytes (Extra holds
            // strings), so it is not byte-for-byte verbatim; keep bytes in Extra if foreign files ever matter.
            else if (line.Length > 0 && text != Header) file.Extra.Add(text ?? Lenient.GetString(line));
        }
        return file;
    }

    static Preset? ParseLine(string line)
    {
        var parts = line.Split('\t');
        if (parts.Length != 8 || CleanName(parts[0]) is not { } name || name != parts[0]) return null;
        var v = new uint[7];
        for (int i = 0; i < 7; i++)
            if (!ParseU32(parts[i + 1], out v[i])) return null;
        var raw = new PsddsSettings { Format = v[0], TextureType = v[1], Srgb = v[2], Filter = v[3], Mips = v[4], Quality = v[5], DiscardAlpha = v[6] };
        return Psdds.TryResolve(raw, false, out _, out _) ? new Preset(name, raw) : null;
    }

    // A decimal number below 2^32, nothing else (no sign, no spaces).
    static bool ParseU32(string t, out uint value)
    {
        value = 0;
        if (t.Length is 0 or > 10) return false;
        ulong v = 0;
        foreach (char c in t)
        {
            if (c is < '0' or > '9') return false;
            v = v * 10 + (uint)(c - '0');
        }
        if (v > uint.MaxValue) return false;
        value = (uint)v;
        return true;
    }

    /// <summary>Writes the presets, then Extra verbatim, through a temp file that is flushed and then moved over the old
    /// one. false (old file unchanged) when the file is not Complete, a name is invalid, or any step fails.</summary>
    public static bool Save(string dir, PresetFile file)
    {
        if (!file.Complete || string.IsNullOrEmpty(dir)) return false;
        var text = new StringBuilder(Header).Append("\r\n");
        foreach (var p in file.List)
        {
            if (CleanName(p.Name) != p.Name) return false;
            var s = p.Raw;
            text.Append(p.Name).Append($"\t{s.Format}\t{s.TextureType}\t{s.Srgb}\t{s.Filter}\t{s.Mips}\t{s.Quality}\t{s.DiscardAlpha}\r\n");
        }
        foreach (var line in file.Extra) text.Append(line).Append("\r\n");
        var path = Path.Combine(dir, FileName);
        var tmp = path + ".tmp";
        try
        {
            Directory.CreateDirectory(dir);
            using (var f = new FileStream(tmp, FileMode.Create, FileAccess.Write, FileShare.None))
            {
                f.Write(Strict.GetBytes(text.ToString()));
                f.Flush(flushToDisk: true);
            }
            File.Move(tmp, path, overwrite: true);
            return true;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or SecurityException or NotSupportedException or ArgumentException or EncoderFallbackException)
        {
            try { File.Delete(tmp); } catch (Exception d) when (d is IOException or UnauthorizedAccessException) { }
            return false;
        }
    }

    /// <summary>The name trimmed of presets.cpp's white space set; null when it is then empty, over 64 UTF-16 units, or
    /// holds a control character (below 0x20, or 0x7f).</summary>
    public static string? CleanName(string name)
    {
        var t = name.Trim(Space);
        if (t.Length is 0 or > 64) return null;
        foreach (char c in t)
            if (c < 0x20 || c == 0x7f) return null;
        return t;
    }

    /// <summary>The index of the preset called `name`, ignoring case (ordinal), or -1.</summary>
    public static int Find(IReadOnlyList<Preset> list, string name)
    {
        for (int i = 0; i < list.Count; i++)
            if (string.Equals(list[i].Name, name, StringComparison.OrdinalIgnoreCase)) return i;
        return -1;
    }

    /// <summary>Replaces the preset of that name (any case) in place, or appends; returns its index.</summary>
    public static int Upsert(List<Preset> list, string name, in PsddsSettings raw)
    {
        int at = Find(list, name);
        if (at < 0) { list.Add(new Preset(name, raw)); return list.Count - 1; }
        list[at] = new Preset(name, raw);
        return at;
    }

    // The effective settings; Discard transparency has no effect without alpha, so it is zeroed there (DC40).
    static bool Effective(in PsddsSettings raw, bool hasAlpha, out PsddsSettings eff)
    {
        if (!Psdds.TryResolve(raw, hasAlpha, out eff, out _)) return false;
        if (!hasAlpha) eff.DiscardAlpha = 0;
        return true;
    }

    /// <summary>`preferred` when its effective values still equal raw's, else the first preset whose effective values
    /// do, else -1 (Custom). DC37, DC40.</summary>
    public static int Match(IReadOnlyList<Preset> list, in PsddsSettings raw, bool hasAlpha, int preferred = -1)
    {
        if (!Effective(raw, hasAlpha, out var want)) return -1;
        if (preferred >= 0 && preferred < list.Count && Effective(list[preferred].Raw, hasAlpha, out var got) && got.Equals(want))
            return preferred;
        for (int i = 0; i < list.Count; i++)
            if (Effective(list[i].Raw, hasAlpha, out var e) && e.Equals(want)) return i;
        return -1;
    }
}
