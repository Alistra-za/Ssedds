// Tessalith DDS for Paint.NET. Copyright (c) 2026 Alistraza. GPL-3.0-or-later with the additional permission in paintdotnet/COPYING.
using System;
using System.Collections.Generic;
using System.Linq;
using System.Windows.Forms;
using PaintDotNet;

namespace Tessalith.Dds;

/// <summary>The save options, as photoshop/src/dialog.rc and dialog.cpp (DC19, DC33, DC37, DC43, DC45): every control is
/// always present; greyed controls show psdds_resolve's effective value while raw keeps the user's pick. No colours are
/// set, so the host form's theme applies.</summary>
public sealed class TessalithDdsWidget : SaveConfigWidget<TessalithDdsFileType, TessalithDdsToken>
{
    const string Caption = "Tessalith DDS";
    const string ReadOnlyMessage = "The preset file could not be read, so presets can't be changed.";

    readonly ComboBox _preset = List("Preset"), _type = List("Texture type"), _format = List("Format"),
                      _filter = List("Filter"), _quality = List("Quality");
    readonly CheckBox _srgb = Box("&sRGB"), _discard = Box("Discard t&ransparency"), _mips = Box("&Mipmaps");
    readonly Button _save = new() { Text = "Sa&ve preset...", AutoSize = true }, _delete = new() { Text = "&Delete", AutoSize = true };

    readonly string? _dir = PresetStore.Dir();
    readonly PresetFile _presets;
    readonly PresetSelection _selection = new();
    readonly MeasuredAlpha _alpha = new();
    PsddsSettings _raw = Psdds.Defaults(Psdds.Colour);
    bool _refreshing;

    public TessalithDdsWidget(TessalithDdsFileType fileType) : base(fileType)
    {
        _presets = _dir is null ? new PresetFile([], true, []) : PresetStore.Load(_dir);
        _save.Enabled = _dir is not null; // no %APPDATA%: presets cannot be stored
        BuildLayout();
        foreach (var c in DialogLists.Types) _type.Items.Add(c);
        foreach (var c in DialogLists.Filters) _filter.Items.Add(c);
        FillPresets();

        _preset.SelectedIndexChanged += (_, _) => OnPresetPicked();
        _type.SelectedIndexChanged += (_, _) => OnPick(_type, (ref PsddsSettings r, uint v) => r.TextureType = v);
        _format.SelectedIndexChanged += (_, _) => OnPick(_format, (ref PsddsSettings r, uint v) => r.Format = v);
        _filter.SelectedIndexChanged += (_, _) => OnPick(_filter, (ref PsddsSettings r, uint v) => r.Filter = v);
        _quality.SelectedIndexChanged += (_, _) => OnPick(_quality, (ref PsddsSettings r, uint v) => r.Quality = v);
        _srgb.CheckedChanged += (_, _) => OnPick(_srgb, (ref PsddsSettings r, uint v) => r.Srgb = v);
        _discard.CheckedChanged += (_, _) => OnPick(_discard, (ref PsddsSettings r, uint v) => r.DiscardAlpha = v);
        _mips.CheckedChanged += (_, _) => OnPick(_mips, (ref PsddsSettings r, uint v) => r.Mips = v);
        _save.Click += (_, _) => SavePreset();
        _delete.Click += (_, _) => DeletePreset();

        // Paint.NET makes one TessalithDdsFileType per process (GetFileTypeInstances); the host builds this widget from it
        // and runs the preview save on that same instance, so the instance event reaches the open widget.
        fileType.HasAlphaMeasured += OnMeasured;
    }

    static ComboBox List(string name) => new() { DropDownStyle = ComboBoxStyle.DropDownList, AccessibleName = name, DisplayMember = nameof(Choice.Label), Anchor = AnchorStyles.Left | AnchorStyles.Right };
    static CheckBox Box(string text) => new() { Text = text, AutoSize = true, Anchor = AnchorStyles.Left };
    static Label NewLabel(string text) => new() { Text = text, AutoSize = true, Anchor = AnchorStyles.Left };

    // dialog.rc's grid: labels | Type and Format lists (one width) | sRGB and Discard transparency; the Preset row's buttons,
    // the Filter row and the Quality list span the two right columns. TabIndex is the visual order; each label sits just
    // before its list so its mnemonic moves focus there.
    void BuildLayout()
    {
        var t = new TableLayoutPanel { AutoSize = true, AutoSizeMode = AutoSizeMode.GrowAndShrink, ColumnCount = 3, RowCount = 5 };
        int tab = 0;
        void Add(Control c, int col, int row, int span = 1)
        {
            c.TabIndex = tab++;
            t.Controls.Add(c, col, row);
            if (span > 1) t.SetColumnSpan(c, span);
        }
        Panel Row(params Control[] cs)
        {
            var p = new FlowLayoutPanel { AutoSize = true, WrapContents = false, Margin = Padding.Empty, Anchor = AnchorStyles.Left | AnchorStyles.Right };
            for (int i = 0; i < cs.Length; i++) { cs[i].TabIndex = i; p.Controls.Add(cs[i]); }
            return p;
        }

        Add(NewLabel("&Preset:"), 0, 0); Add(_preset, 1, 0); Add(Row(_save, _delete), 2, 0);
        Add(NewLabel("&Texture type:"), 0, 1); Add(_type, 1, 1); Add(_srgb, 2, 1);
        Add(NewLabel("&Format:"), 0, 2); Add(_format, 1, 2); Add(_discard, 2, 2);
        Add(_mips, 0, 3);
        var filterRow = new TableLayoutPanel { AutoSize = true, ColumnCount = 2, Margin = Padding.Empty, Anchor = AnchorStyles.Left | AnchorStyles.Right };
        filterRow.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
        filterRow.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
        var filterLabel = NewLabel("F&ilter:");
        filterLabel.TabIndex = 0;
        _filter.TabIndex = 1;
        filterRow.Controls.Add(filterLabel, 0, 0);
        filterRow.Controls.Add(_filter, 1, 0);
        Add(filterRow, 1, 3, 2);
        Add(NewLabel("&Quality:"), 0, 4); Add(_quality, 1, 4, 2);

        AutoSize = true;
        AutoSizeMode = AutoSizeMode.GrowAndShrink;
        Controls.Add(t);
    }

    protected override void OnLoad(EventArgs e)
    {
        base.OnLoad(e);
        // Type and Format share one width, wide enough for the longest label in either (dialog.rc); the Preset list matches.
        int w = DialogLists.Formats.Concat(DialogLists.Types).Max(c => TextRenderer.MeasureText(c.Label, _format.Font).Width)
                + SystemInformation.VerticalScrollBarWidth + LogicalToDeviceUnits(12);
        _preset.Width = _type.Width = _format.Width = w;
        FillPresets(); // again, now that the box width is known (DropDownWidth)
        Sync(null); // also applies a measurement that arrived before the handle existed (ruling 7)
        _preset.Select(); // initial focus: Preset (dialog.rc's first tab stop)
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing) FileType.HasAlphaMeasured -= OnMeasured;
        base.Dispose(disposing);
    }

    // Raised on the preview save's worker thread; must never throw into the save.
    void OnMeasured(bool used)
    {
        if (!_alpha.Set(used)) return; // every preview reports: refresh only on a change
        try
        {
            // has_alpha changes only the filter and the preset match, never type or format (psdds resolve), so no list
            // refill: Sync(_preset) keeps an open Format or Quality dropdown open.
            if (!IsDisposed && IsHandleCreated) BeginInvoke(() => { if (!IsDisposed) Sync(_preset); });
        }
        catch (Exception e) when (e is InvalidOperationException or ObjectDisposedException) { } // disposed meanwhile: nothing to show
    }

    protected override void InitWidgetFromToken(TessalithDdsToken token)
    {
        _raw = token.Raw;
        _selection.Reset(); // also the host's Defaults button: no stale Custom or twin pick survives
        Sync(null);
    }

    protected override TessalithDdsToken CreateTokenFromWidget() => new() { Raw = _raw }; // raw picks, never shown values

    delegate void Setter(ref PsddsSettings raw, uint value);

    void OnPick(Control c, Setter set)
    {
        if (_refreshing) return;
        uint v = c is CheckBox b ? (b.Checked ? 1u : 0u) : ((ComboBox)c).SelectedItem is Choice ch ? ch.Value : 0;
        set(ref _raw, v);
        Sync(c);
        UpdateToken();
    }

    void OnPresetPicked()
    {
        if (_refreshing) return;
        int i = _preset.SelectedIndex - 1; // item 0 is Custom; never look up by name (a preset may be called "Custom")
        if (i < 0)
        {
            _selection.PickCustom(); // keeps the settings: no token change (DC45)
            Sync(_preset);
            return;
        }
        _selection.Pick(i);
        _raw = _presets.List[i].Raw;
        Sync(null); // type and format may both change: full refresh (ruling 8, dialog.cpp:245)
        UpdateToken();
    }

    void FillPresets()
    {
        bool was = _refreshing;
        _refreshing = true;
        _preset.Items.Clear();
        _preset.Items.Add("Custom");
        foreach (var p in _presets.List) _preset.Items.Add(p.Name);
        // the open list shows long names (up to 64 characters) in full; the closed box truncates, as Win32 does
        int widest = _presets.List.Select(p => TextRenderer.MeasureText(p.Name, _preset.Font).Width).DefaultIfEmpty(0).Max()
                     + SystemInformation.VerticalScrollBarWidth + LogicalToDeviceUnits(12);
        _preset.DropDownWidth = Math.Max(_preset.Width, Math.Min(widest, LogicalToDeviceUnits(400)));
        _refreshing = was;
    }

    /// <summary>dialog.cpp refresh(): changed null = start or a preset pick (rebuild Format and Quality);
    /// the Type list rebuilds Format; Type or Format refills Quality. Named Sync: Control.Refresh() repaints.</summary>
    void Sync(Control? changed)
    {
        bool hasAlpha = _alpha.Value ?? false; // unknown until the first preview pass: shown as opaque, filter not greyed for alpha
        if (DialogModel.Refresh(_raw, hasAlpha) is not { } s) return; // unreachable: the controls offer only valid values
        _refreshing = true;
        try
        {
            if (changed is null || changed == _type) Refill(_format, s.Formats);
            if (changed is null || changed == _type || changed == _format) Refill(_quality, s.Quality);
            int match = _selection.Shown(_presets.List, _raw, hasAlpha);
            _preset.SelectedIndex = match + 1;
            Select(_format, s.Shown.Format);
            Select(_type, s.Shown.TextureType);
            Select(_filter, s.Shown.Filter);
            Select(_quality, s.Shown.Quality);
            _srgb.Checked = s.Shown.Srgb != 0;
            _discard.Checked = s.Shown.DiscardAlpha != 0; // always enabled (DC36)
            _mips.Checked = s.Shown.Mips != 0;
            _srgb.Enabled = s.SrgbEnabled;
            _filter.Enabled = s.FilterEnabled;
            _quality.Enabled = s.QualityEnabled;
            _delete.Enabled = _selection.CanDelete;
        }
        finally { _refreshing = false; }
    }

    static void Refill(ComboBox c, IReadOnlyList<Choice> items)
    {
        c.BeginUpdate();
        c.Items.Clear();
        foreach (var i in items) c.Items.Add(i);
        c.EndUpdate();
    }

    static void Select(ComboBox c, uint value)
    {
        int pick = -1;
        for (int i = 0; i < c.Items.Count; i++)
            if (((Choice)c.Items[i]!).Value == value) pick = i;
        c.SelectedIndex = pick;
    }

    IWin32Window Owner => (IWin32Window?)FindForm() ?? this;

    void Message(string text) => MessageBox.Show(Owner, text, Caption, MessageBoxButtons.OK, MessageBoxIcon.Warning);

    bool Ask(string text) =>
        MessageBox.Show(Owner, text, Caption, MessageBoxButtons.YesNo, MessageBoxIcon.Question, MessageBoxDefaultButton.Button2) == DialogResult.Yes;

    // dialog.cpp save_preset. Writes at once, also if the host dialog is then cancelled (DC37); the token is unchanged.
    void SavePreset()
    {
        if (!_presets.Complete) { Message(ReadOnlyMessage); return; }
        if (_dir is null) return;
        string proposed = _selection.Picked < 0 ? "" : _presets.List[_selection.Picked].Name; // Picked is the last Shown
        string name;
        using (var prompt = new NamePrompt(proposed))
        {
            if (prompt.ShowDialog(Owner) != DialogResult.OK) return;
            name = prompt.PresetName;
        }
        var next = new List<Preset>(_presets.List);
        int at = PresetStore.Find(next, name);
        if (at >= 0 && !Ask($"A preset named \"{next[at].Name}\" already exists. Replace it?")) return;
        at = PresetStore.Upsert(next, name, _raw);
        if (!PresetStore.Save(_dir, _presets with { List = next }))
        {
            Message("The preset could not be saved.");
            return;
        }
        Commit(next);
        _selection.Saved(at);
        FillPresets();
        Sync(_preset);
    }

    // dialog.cpp delete_preset.
    void DeletePreset()
    {
        if (!_presets.Complete) { Message(ReadOnlyMessage); return; }
        if (_dir is null || !_selection.CanDelete) return;
        int match = _selection.Picked;
        if (!Ask($"Delete the preset \"{_presets.List[match].Name}\"?")) return;
        var next = new List<Preset>(_presets.List);
        next.RemoveAt(match);
        if (!PresetStore.Save(_dir, _presets with { List = next }))
        {
            Message("The preset could not be deleted.");
            return;
        }
        Commit(next);
        _selection.Deleted(); // before the next Shown: indexes moved
        FillPresets();
        Sync(_preset);
    }

    void Commit(List<Preset> next)
    {
        _presets.List.Clear();
        _presets.List.AddRange(next);
    }
}
