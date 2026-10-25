using System.Threading;

namespace Tessalith.Dds;

/// <summary>The last psdds_alpha_used result of the preview save, written on its worker thread and read on the UI
/// thread (ruling 7); null until the first preview pass reports.</summary>
public sealed class MeasuredAlpha
{
    int _v = -1; // -1 unknown, 0, 1

    public void Set(bool used) => Volatile.Write(ref _v, used ? 1 : 0);

    public bool? Value => Volatile.Read(ref _v) is var v && v < 0 ? null : v == 1;
}
