use eframe::egui;

use crate::config::Config;
use crate::pages::{semi_transparent_group_frame, source_type_icon};

pub fn paint_settings(ui: &mut egui::Ui, config: &Config) {
    ui.add_space(8.0);
    ui.heading("Settings");
    ui.separator();
    ui.add_space(8.0);

    semi_transparent_group_frame(ui).show(ui, |ui| {
        ui.label(egui::RichText::new("Appearance").strong().size(15.0));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Theme:");
            egui::widgets::global_theme_preference_buttons(ui);
        });
    });

    ui.add_space(8.0);

    semi_transparent_group_frame(ui).show(ui, |ui| {
        ui.label(egui::RichText::new("Audio").strong().size(15.0));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Start volume:");
            ui.label(egui::RichText::new(format!("{}%", config.audio.start_volume)).weak());
        });
    });

    ui.add_space(8.0);

    semi_transparent_group_frame(ui).show(ui, |ui| {
        ui.label(egui::RichText::new("Sources").strong().size(15.0));
        ui.add_space(4.0);

        if config.sources.is_empty() {
            ui.label(egui::RichText::new("No sources configured").weak());
        } else {
            for source in &config.sources {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(source_type_icon(&source.source_type));
                    ui.label(egui::RichText::new(&source.name).strong());
                });
                if !source.path.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.label(
                            egui::RichText::new(format!("Path: {}", &source.path))
                                .weak()
                                .small(),
                        );
                    });
                }
                if !source.stations.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(24.0);
                        ui.label(
                            egui::RichText::new(format!("{} station(s)", source.stations.len()))
                                .weak()
                                .small(),
                        );
                    });
                }
                ui.add_space(2.0);
            }
        }
    });

    ui.add_space(8.0);

    semi_transparent_group_frame(ui).show(ui, |ui| {
        ui.label(egui::RichText::new("About").strong().size(15.0));
        ui.add_space(4.0);
        ui.label("Homeplayer v0.1.0");
        ui.label(egui::RichText::new("Built with egui & rodio").weak());
    });

    ui.add_space(40.0);
}
