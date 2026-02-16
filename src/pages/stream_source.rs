use eframe::egui;

use crate::UiAction;
use crate::config::Source;
use crate::pages::semi_transparent_fill;

pub fn paint_stream_source(ui: &mut egui::Ui, source: &Source, actions: &mut Vec<UiAction>) {
    ui.add_space(8.0);

    if source.stations.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("No stations configured")
                    .weak()
                    .size(16.0),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Add stations in the config file.")
                    .weak()
                    .small(),
            );
        });
        return;
    }

    ui.label(
        egui::RichText::new(format!("{} stations", source.stations.len()))
            .weak()
            .small(),
    );
    ui.add_space(4.0);

    let fill = semi_transparent_fill(ui);
    for station in &source.stations {
        ui.add_space(2.0);
        let response = ui.add(
            egui::Button::new(egui::RichText::new(format!("📻  {}", station.name)).size(16.0))
                .fill(fill)
                .frame(true)
                .min_size(egui::vec2(ui.available_width(), 48.0)),
        );

        if response.clicked() {
            actions.push(UiAction::PlayStream {
                url: station.url.clone(),
                icon: station.icon.clone(),
            });
        }

        // Show URL as hover tooltip
        response.on_hover_text(&station.url);
    }
}
