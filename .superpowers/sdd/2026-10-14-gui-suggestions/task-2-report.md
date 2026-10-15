# Task 2 report

Commits: cec2b7d (carry-over: Tessalith wording, Esc cancels hex box, stale buffer, tests), then the items 2/4/23/24 commit.

## RED
Carry-over: against the old color.rs, esc_cancels_a_hex_edit failed ('nothing committed') and a_stale_edit_buffer_is_dropped_when_the_row_goes_away failed ('no stale text').

Items 4/23 (dialogs.rs and sets_column.rs at HEAD, rest new): 5 failed, 7 passed:
```
test undo_clears_the_encoding_note_and_keeps_the_export_cancelled_note ... ok
test ctrl_z_and_ctrl_y_work_in_the_meshes_view ... ok
test a_dialog_ignores_enter_on_the_frame_it_opens ... FAILED
test the_buttons_enable_with_history_and_their_hover_text_carries_the_label ... ok
test undo_and_redo_are_disabled_with_no_history ... ok
test the_unlabelled_controls_have_hover_text ... ok
test esc_closes_the_dialogs_that_use_the_shared_modal ... FAILED
test the_status_line_says_what_was_undone_and_redone ... ok
test enter_on_the_external_change_dialog_never_overwrites ... FAILED
test enter_never_decides_a_decision_dialog ... ok
test a_sets_menu_groups_the_three_renames_in_a_submenu_with_hover_text ... FAILED
test delete_set_asks_cancel_keeps_it_and_delete_is_one_undo_step ... FAILED
test result: FAILED. 7 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
error: test failed, to rerun pass `-p sdc-gui --test dialog_keys`
```

Item 2 and 24 (model/header/app/columns at HEAD): compile failure, so every test in the file is RED:
```
error[E0425]: cannot find function `undo_redo_keys` in module `sdc_gui::ui::header`
  --> crates\gui\tests\dialog_keys.rs:96:57
   |
96 |             |ui, m: &mut AppModel| sdc_gui::ui::header::undo_redo_keys(ui.ctx(), m),
--
error: could not compile `sdc-gui` (test "dialog_keys") due to 1 previous error
warning: build failed, waiting for other jobs to finish...
```

enter_never_decides_a_decision_dialog and the dialog-opening-frame guard are regression guards (pass before and after, except the guard, which fails without modal_keys).

## Gates
fmt, clippy (--all-targets and plain), test --workspace: 2415 passed, 0 failed.

## Rulings: DECISIONS AG10-AG14. AG5 outcome: Ctrl+Z enabled in Meshes; app.rs documented no reason.

## Not covered by tests
Click-outside behaviour; hover text on Mode combo and the preview toolbar (needs a GPU preview); hover on filters is tested for one toggle.
