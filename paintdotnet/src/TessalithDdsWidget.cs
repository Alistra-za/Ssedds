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
    readonly CheckBox _srgb = Box("&sRGB"), _discard = Box("Discard t&ransparency"), _alphaMode = Box("&Alpha is transparency"),
                      _mips = Box("&Mipmaps");
    readonly Button _save = new() { Text = "Sa&ve preset...", AutoSize = true }, _delete = new() { Text = "&Delete", AutoSize = true };
    // Note FC6: the status line under the options, shown while the preview wrote alpha 254-255 opaque
    readonly Label _note = new() { Text = Psdds.Alpha254Note, AutoSize = true, Anchor = AnchorStyles.Left, Visible = false, UseMnemonic = false };

    readonly string? _dir = PresetStore.Dir();
    readonly PresetFile _presets;
    readonly PresetSelection _selection = new();
    readonly MeasuredAlpha _alpha = new();
    // Never use the FileType property: the SaveConfigWidget(FileType) base ctor does not store it, so it is always null
    // and its getter throws (Paint.NET 5.1 IL; Note DI36).
    readonly TessalithDdsFileType _fileType;
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
        _alphaMode.CheckedChanged += (_, _) => OnPick(_alphaMode, (ref PsddsSettings r, uint v) => r.AlphaMode = v);
        _mips.CheckedChanged += (_, _) => OnPick(_mips, (ref PsddsSettings r, uint v) => r.Mips = v);
        _save.Click += (_, _) => SavePreset();
        _delete.Click += (_, _) => DeletePreset();

        // Paint.NET makes one TessalithDdsFileType per process (GetFileTypeInstances); the host builds this widget from it
        // and runs the preview save on that same instance, so the instance event reaches the open widget.
        _fileType = fileType;
        _fileType.HasAlphaMeasured += OnMeasured;
    }

    static ComboBox List(string name) => new() { DropDownStyle = ComboBoxStyle.DropDownList, AccessibleName = name, DisplayMember = nameof(Choice.Label), Anchor = AnchorStyles.Left | AnchorStyles.Right };
    static CheckBox Box(string text) => new() { Text = text, AutoSize = true, Anchor = AnchorStyles.Left };
    static Label NewLabel(string text) => new() { Text = text, AutoSize = true, Anchor = AnchorStyles.Left };

    // One column in dialog.rc's row order (DC33): Paint.NET gives the options a fixed 200-DIP column and sets this widget's
    // width to it (Note DI37), so the lists fill that width and every label sits above its list. TabIndex is the visual
    // order; each label sits just before its list so its mnemonic moves focus there.
    readonly TableLayoutPanel _table = new() { AutoSize = true, AutoSizeMode = AutoSizeMode.GrowAndShrink, ColumnCount = 1, Dock = DockStyle.Top };

    void BuildLayout()
    {
        _table.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
        int tab = 0;
        foreach (var c in new Control[]
                 {
                     NewLabel("&Preset:"), _preset, Row(_save, _delete),
                     NewLabel("&Texture type:"), _type, _srgb,
                     NewLabel("&Format:"), _format, _discard, _alphaMode,
                     _mips,
                     NewLabel("F&ilter:"), _filter,
                     NewLabel("&Quality:"), _quality,
                     _note,
                 })
        {
            c.TabIndex = tab++;
            _table.Controls.Add(c);
        }
        Controls.Add(_table);
        _table.SizeChanged += (_, _) => { if (Height != _table.Height) Height = _table.Height; }; // the host reads Height after layout

        // the buttons wrap: Delete moves under Save preset... when the column is too narrow for both
        static Panel Row(params Control[] cs)
        {
            var p = new FlowLayoutPanel { AutoSize = true, Margin = Padding.Empty, Anchor = AnchorStyles.Left | AnchorStyles.Right };
            for (int i = 0; i < cs.Length; i++) { cs[i].TabIndex = i; p.Controls.Add(cs[i]); }
            return p;
        }
    }

    protected override void OnLoad(EventArgs e)
    {
        base.OnLoad(e);
        // the lists fill the column and truncate when closed; the open Type and Format lists show the longest label in full
        _type.DropDownWidth = _format.DropDownWidth = DialogLists.Formats.Concat(DialogLists.Types)
            .Max(c => TextRenderer.MeasureText(c.Label, _format.Font).Width) + SystemInformation.VerticalScrollBarWidth + LogicalToDeviceUnits(12);
        _note.MaximumSize = new System.Drawing.Size(Width, 0); // wraps inside the column
        FillPresets(); // again, now that the font and DPI are the host's (DropDownWidth)
        Sync(null); // also applies a measurement that arrived before the handle existed (ruling 7)
        _preset.Select(); // initial focus: Preset (dialog.rc's first tab stop)
    }

    protected override void Dispose(bool disposing)
    {
        // the dialog cancels the preview without waiting, so OnMeasured may still run after this; it checks IsDisposed
        if (disposing) _fileType.HasAlphaMeasured -= OnMeasured;
        base.Dispose(disposing);
    }

    // Raised on the preview save's worker thread; must never throw into the save.
    void OnMeasured(bool used, bool note)
    {
        if (!_alpha.Set(used, note)) return; // every preview reports: refresh only on a change
        try
        {
            // has_alpha changes only the filter, Alpha is transparency and the preset match, never type or format (psdds resolve), so no list
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
        _preset.DropDownWidth = Math.Min(widest, LogicalToDeviceUnits(400)); // Win32 never drops narrower than the box
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
            _discard.Checked = s.Shown.DiscardAlpha != 0; // ticked and greyed where the format stores no alpha (Note DF2)
            _alphaMode.Checked = _raw.AlphaMode != 0; // the raw pick, also when greyed, as dialog.cpp (Note DH3)
            _mips.Checked = s.Shown.Mips != 0;
            _srgb.Enabled = s.SrgbEnabled;
            _discard.Enabled = s.DiscardEnabled;
            _alphaMode.Enabled = s.AlphaModeEnabled;
            _filter.Enabled = s.FilterEnabled;
            _quality.Enabled = s.QualityEnabled;
            _delete.Enabled = _selection.CanDelete;
            _note.Visible = _alpha.Note;
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
