use eframe::egui;

use crate::config::Source;

pub fn paint_cd_source(ui: &mut egui::Ui, source: &Source) {
    ui.add_space(8.0);
    ui.heading(format!("💿 {}", source.name));
    ui.separator();
    ui.add_space(40.0);

    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new("💿").size(64.0));
        ui.add_space(16.0);
        ui.label(egui::RichText::new("CD Playback").strong().size(20.0));
        ui.add_space(8.0);
        ui.label(egui::RichText::new("Coming soon...").weak().size(14.0));
        if !source.path.is_empty() {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!("Device: {}", source.path))
                    .weak()
                    .small(),
            );
        }
    });
}
