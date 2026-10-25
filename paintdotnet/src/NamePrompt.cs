using System.Windows.Forms;

namespace Tessalith.Dds;

/// <summary>The "Save preset" name prompt (photoshop/src/dialog.rc IDD_PRESET_NAME, dialog.cpp name_proc). OK keeps the
/// form open until the name passes PresetStore.CleanName; PresetName is then the clean name.</summary>
sealed class NamePrompt : Form
{
    readonly TextBox _text = new() { MaxLength = 64, Width = 240, TabIndex = 1, AccessibleName = "Name" };

    public string PresetName { get; private set; } = "";

    public NamePrompt(string proposed)
    {
        Text = "Save preset";
        FormBorderStyle = FormBorderStyle.FixedDialog;
        StartPosition = FormStartPosition.CenterParent;
        MinimizeBox = MaximizeBox = ShowInTaskbar = false;
        AutoSize = true;
        AutoSizeMode = AutoSizeMode.GrowAndShrink;
        _text.Text = proposed;
        var ok = new Button { Text = "OK", TabIndex = 2, AutoSize = true };
        var cancel = new Button { Text = "Cancel", TabIndex = 3, AutoSize = true, DialogResult = DialogResult.Cancel };
        ok.Click += (_, _) =>
        {
            if (PresetStore.CleanName(_text.Text) is { } clean)
            {
                PresetName = clean;
                DialogResult = DialogResult.OK;
                return;
            }
            MessageBox.Show(this, "Enter a preset name of 1 to 64 characters.", "Tessalith DDS", MessageBoxButtons.OK, MessageBoxIcon.Warning);
            _text.Focus();
        };
        AcceptButton = ok;
        CancelButton = cancel;
        var buttons = new FlowLayoutPanel { AutoSize = true, Anchor = AnchorStyles.Right, TabIndex = 2, Margin = Padding.Empty };
        buttons.Controls.AddRange([ok, cancel]);
        var table = new TableLayoutPanel { AutoSize = true, ColumnCount = 2, Padding = new Padding(8) };
        table.Controls.Add(new Label { Text = "&Name:", AutoSize = true, Anchor = AnchorStyles.Left, TabIndex = 0 }, 0, 0);
        table.Controls.Add(_text, 1, 0);
        table.Controls.Add(buttons, 0, 1);
        table.SetColumnSpan(buttons, 2);
        Controls.Add(table);
        Shown += (_, _) => { _text.Focus(); _text.SelectAll(); };
    }
}
