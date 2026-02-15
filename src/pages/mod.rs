pub mod cd_source;
pub mod file_source;
pub mod now_playing;
pub mod settings;
pub mod stream_source;

use crate::config::ConfigSourceType;
use eframe::egui;

pub use cd_source::paint_cd_source;
pub use file_source::{FileRenderData, paint_file_source};
pub use now_playing::paint_now_playing;
pub use settings::paint_settings;
pub use stream_source::paint_stream_source;

pub fn source_type_icon(source_type: &ConfigSourceType) -> &'static str {
    match source_type {
        ConfigSourceType::File => "📁",
        ConfigSourceType::Stream => "📻",
        ConfigSourceType::CD => "💿",
    }
}

/// Returns a semi-transparent version of the widget inactive background fill color,
/// so the page background image is partly visible behind list items and groups.
pub fn semi_transparent_fill(ui: &egui::Ui) -> egui::Color32 {
    let fill = ui.visuals().widgets.inactive.bg_fill;
    egui::Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), 180)
}

/// Returns a semi-transparent `Frame` suitable for group-style containers,
/// so the page background image is partly visible behind grouped sections.
pub fn semi_transparent_group_frame(ui: &egui::Ui) -> egui::Frame {
    let fill = ui.visuals().widgets.inactive.bg_fill;
    let transparent_fill = egui::Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), 160);
    egui::Frame::group(ui.style()).fill(transparent_fill)
}
