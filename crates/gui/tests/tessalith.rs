//! AG1: the GUI is called Tessalith; the CLI stays `sdc`.
mod support;
use sdc_gui::model::AppModel;
use sdc_gui::settings::GuiSettings;

#[test]
fn the_window_title_is_tessalith() {
    assert_eq!(sdc_gui::PRODUCT_NAME, "Tessalith");
    let m = AppModel::new(GuiSettings::default(), None, support::session());
    assert_eq!(sdc_gui::ui::header::window_title(&m), "Tessalith");
}
