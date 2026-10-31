// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
using System.Threading;

namespace Tessalith.Dds;

/// <summary>The last psdds_alpha_used result of the preview save, written on its worker thread and read on the UI
/// thread (ruling 7); null until the first preview pass reports.</summary>
public sealed class MeasuredAlpha
{
    int _v = -1; // -1 unknown, 0, 1, 2 = opaque with the alpha 254-255 note (Note FC6)

    /// <summary>true when the value differs from the last one (unknown counts as different).</summary>
    public bool Set(bool used, bool note = false)
    {
        int v = used ? 1 : note ? 2 : 0;
        return Interlocked.Exchange(ref _v, v) != v;
    }

    public bool? Value => Volatile.Read(ref _v) is var v && v < 0 ? null : v == 1;

    /// <summary>Alpha only 254-255 was written opaque by the last preview (Psdds.Alpha254Note).</summary>
    public bool Note => Volatile.Read(ref _v) == 2;
}
