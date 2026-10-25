using System.Collections.Generic;

namespace Tessalith.Dds;

/// <summary>Which preset list entry the dialog shows (dialog.cpp's Context.picked and Context.custom): the picked
/// preset wins over an earlier twin while it still matches (DC40); Custom stays shown until a preset is picked or
/// saved (DC45). One per dialog; a new dialog starts fresh.</summary>
public sealed class PresetSelection
{
    public int Picked { get; private set; } = -1;
    public bool Custom { get; private set; }

    /// <summary>Delete preset is enabled: a preset (not Custom) is shown.</summary>
    public bool CanDelete => !Custom && Picked >= 0;

    public void Pick(int index) { Picked = index; Custom = false; }
    public void PickCustom() => Custom = true;
    public void Saved(int index) { Picked = index; Custom = false; }

    /// <summary>After the shown preset was deleted: indexes moved, so match afresh (dialog.cpp delete_preset).</summary>
    public void Deleted() => Picked = -1;

    /// <summary>The list index to show, -1 = Custom; remembered as Picked (dialog.cpp refresh).</summary>
    public int Shown(IReadOnlyList<Preset> list, in PsddsSettings raw, bool hasAlpha) =>
        Picked = Custom ? -1 : PresetStore.Match(list, raw, hasAlpha, Picked);
}
