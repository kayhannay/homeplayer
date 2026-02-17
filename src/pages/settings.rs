use eframe::egui;

use crate::UiAction;
use crate::config::{Config, ConfigSourceType, Source, Station};
use crate::pages::{semi_transparent_group_frame, source_type_icon};

/// Mutable state for the settings page editor.
pub struct SettingsState {
    /// Editable copy of the configuration.
    pub config: Config,
    /// Whether the editable config diverges from the saved config.
    pub dirty: bool,
    /// Result message after the last save attempt.
    pub save_message: Option<(String, bool)>,
    /// Controls which source's station list is expanded for editing.
    pub expanded_source: Option<usize>,
    /// Temporary fields for adding a new source.
    pub adding_source: bool,
    pub new_source_name: String,
    pub new_source_path: String,
    pub new_source_type: ConfigSourceType,
    /// Temporary fields for adding a new station (keyed by source index).
    pub adding_station_for: Option<usize>,
    pub new_station_name: String,
    pub new_station_url: String,
    pub new_station_icon: String,
    /// Confirmation dialog for source removal.
    pub confirm_remove_source: Option<usize>,
    /// Confirmation dialog for station removal (source_idx, station_idx).
    pub confirm_remove_station: Option<(usize, usize)>,
}

impl SettingsState {
    pub fn new(config: &Config) -> Self {
        Self {
            config: config.clone(),
            dirty: false,
            save_message: None,
            expanded_source: None,
            adding_source: false,
            new_source_name: String::new(),
            new_source_path: String::new(),
            new_source_type: ConfigSourceType::File,
            adding_station_for: None,
            new_station_name: String::new(),
            new_station_url: String::new(),
            new_station_icon: String::new(),
            confirm_remove_source: None,
            confirm_remove_station: None,
        }
    }

    /// Reset the editable config back to the given (current/saved) config.
    pub fn reset(&mut self, config: &Config) {
        self.config = config.clone();
        self.dirty = false;
        self.save_message = None;
        self.expanded_source = None;
        self.adding_source = false;
        self.new_source_name.clear();
        self.new_source_path.clear();
        self.new_source_type = ConfigSourceType::File;
        self.adding_station_for = None;
        self.new_station_name.clear();
        self.new_station_url.clear();
        self.new_station_icon.clear();
        self.confirm_remove_source = None;
        self.confirm_remove_station = None;
    }
}

fn source_type_label(source_type: &ConfigSourceType) -> &'static str {
    match source_type {
        ConfigSourceType::File => "File",
        ConfigSourceType::Stream => "Stream",
        ConfigSourceType::CD => "CD",
        ConfigSourceType::KidsFile => "KidsFile",
    }
}

const ALL_SOURCE_TYPES: [ConfigSourceType; 4] = [
    ConfigSourceType::File,
    ConfigSourceType::Stream,
    ConfigSourceType::CD,
    ConfigSourceType::KidsFile,
];

pub fn paint_settings(ui: &mut egui::Ui, state: &mut SettingsState, actions: &mut Vec<UiAction>) {
    ui.add_space(8.0);

    // ── Appearance ──────────────────────────────────────────────────────
    semi_transparent_group_frame(ui).show(ui, |ui| {
        ui.label(egui::RichText::new("Appearance").strong().size(15.0));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Theme:");
            egui::widgets::global_theme_preference_buttons(ui);
        });
    });

    ui.add_space(8.0);

    // ── Audio ───────────────────────────────────────────────────────────
    semi_transparent_group_frame(ui).show(ui, |ui| {
        ui.label(egui::RichText::new("Audio").strong().size(15.0));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Start volume:");
            let mut vol = state.config.audio.start_volume as i32;
            let slider = egui::Slider::new(&mut vol, 0..=100).suffix("%");
            if ui.add(slider).changed() {
                state.config.audio.start_volume = vol.clamp(0, 100) as u8;
                state.dirty = true;
                state.save_message = None;
            }
        });
    });

    ui.add_space(8.0);

    // ── Sources ─────────────────────────────────────────────────────────
    semi_transparent_group_frame(ui).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Sources").strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("➕ Add Source").clicked() {
                    state.adding_source = true;
                    state.new_source_name.clear();
                    state.new_source_path.clear();
                    state.new_source_type = ConfigSourceType::File;
                }
            });
        });
        ui.add_space(4.0);

        if state.config.sources.is_empty() {
            ui.label(egui::RichText::new("No sources configured").weak());
        }

        // We need indices for removal; iterate by index.
        let num_sources = state.config.sources.len();
        let mut source_to_remove: Option<usize> = None;

        for i in 0..num_sources {
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(2.0);

            let is_expanded = state.expanded_source == Some(i);
            let header_icon = if is_expanded { "▼" } else { "▶" };
            let type_icon = source_type_icon(&state.config.sources[i].source_type);

            // Source header row
            ui.horizontal(|ui| {
                let toggle_text = format!(
                    "{} {} {}",
                    header_icon, type_icon, &state.config.sources[i].name
                );
                if ui
                    .add(egui::Button::new(egui::RichText::new(toggle_text).strong()).frame(false))
                    .clicked()
                {
                    if is_expanded {
                        state.expanded_source = None;
                    } else {
                        state.expanded_source = Some(i);
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Remove button with confirmation
                    if state.confirm_remove_source == Some(i) {
                        ui.label(
                            egui::RichText::new("Remove?")
                                .color(egui::Color32::from_rgb(255, 100, 100)),
                        );
                        if ui.button("Yes").clicked() {
                            source_to_remove = Some(i);
                            state.confirm_remove_source = None;
                        }
                        if ui.button("No").clicked() {
                            state.confirm_remove_source = None;
                        }
                    } else if ui
                        .button(
                            egui::RichText::new("🗑").color(egui::Color32::from_rgb(255, 100, 100)),
                        )
                        .on_hover_text("Remove source")
                        .clicked()
                    {
                        state.confirm_remove_source = Some(i);
                    }

                    // Move down
                    if i < num_sources - 1 {
                        if ui.button("⬇").on_hover_text("Move down").clicked() {
                            state.config.sources.swap(i, i + 1);
                            state.dirty = true;
                            state.save_message = None;
                            if state.expanded_source == Some(i) {
                                state.expanded_source = Some(i + 1);
                            } else if state.expanded_source == Some(i + 1) {
                                state.expanded_source = Some(i);
                            }
                        }
                    }
                    // Move up
                    if i > 0 {
                        if ui.button("⬆").on_hover_text("Move up").clicked() {
                            state.config.sources.swap(i, i - 1);
                            state.dirty = true;
                            state.save_message = None;
                            if state.expanded_source == Some(i) {
                                state.expanded_source = Some(i - 1);
                            } else if state.expanded_source == Some(i - 1) {
                                state.expanded_source = Some(i);
                            }
                        }
                    }
                });
            });

            // Expanded source details
            if is_expanded {
                ui.indent(ui.id().with(("source_detail", i)), |ui| {
                    ui.add_space(4.0);

                    // Source type
                    ui.horizontal(|ui| {
                        ui.label("Type:");
                        let current_label = source_type_label(&state.config.sources[i].source_type);
                        egui::ComboBox::from_id_salt(format!("source_type_{}", i))
                            .selected_text(current_label)
                            .show_ui(ui, |ui| {
                                for st in &ALL_SOURCE_TYPES {
                                    let label = source_type_label(st);
                                    if ui
                                        .selectable_value(
                                            &mut state.config.sources[i].source_type,
                                            st.clone(),
                                            label,
                                        )
                                        .changed()
                                    {
                                        state.dirty = true;
                                        state.save_message = None;
                                    }
                                }
                            });
                    });

                    ui.add_space(2.0);

                    // Source name
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        if ui
                            .text_edit_singleline(&mut state.config.sources[i].name)
                            .changed()
                        {
                            state.dirty = true;
                            state.save_message = None;
                        }
                    });

                    ui.add_space(2.0);

                    // Source path (not relevant for Stream type, but still editable)
                    ui.horizontal(|ui| {
                        ui.label("Path:");
                        if ui
                            .text_edit_singleline(&mut state.config.sources[i].path)
                            .changed()
                        {
                            state.dirty = true;
                            state.save_message = None;
                        }
                    });

                    // Stations (for Stream sources or any source that has them)
                    if matches!(
                        state.config.sources[i].source_type,
                        ConfigSourceType::Stream
                    ) || !state.config.sources[i].stations.is_empty()
                    {
                        ui.add_space(6.0);
                        paint_stations(ui, state, i);
                    }
                });
            }
        }

        // Handle deferred source removal
        if let Some(idx) = source_to_remove {
            state.config.sources.remove(idx);
            state.dirty = true;
            state.save_message = None;
            // Adjust expanded_source
            match state.expanded_source {
                Some(e) if e == idx => state.expanded_source = None,
                Some(e) if e > idx => state.expanded_source = Some(e - 1),
                _ => {}
            }
        }

        // Add-source form
        if state.adding_source {
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);
            ui.label(egui::RichText::new("New Source").strong());
            ui.add_space(2.0);

            ui.horizontal(|ui| {
                ui.label("Type:");
                egui::ComboBox::from_id_salt("new_source_type")
                    .selected_text(source_type_label(&state.new_source_type))
                    .show_ui(ui, |ui| {
                        for st in &ALL_SOURCE_TYPES {
                            ui.selectable_value(
                                &mut state.new_source_type,
                                st.clone(),
                                source_type_label(st),
                            );
                        }
                    });
            });

            ui.horizontal(|ui| {
                ui.label("Name:");
                ui.text_edit_singleline(&mut state.new_source_name);
            });

            ui.horizontal(|ui| {
                ui.label("Path:");
                ui.text_edit_singleline(&mut state.new_source_path);
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let name_ok = !state.new_source_name.trim().is_empty();
                if ui
                    .add_enabled(name_ok, egui::Button::new("✔ Add"))
                    .clicked()
                {
                    state.config.sources.push(Source {
                        source_type: state.new_source_type.clone(),
                        name: state.new_source_name.trim().to_string(),
                        path: state.new_source_path.trim().to_string(),
                        stations: Vec::new(),
                    });
                    state.adding_source = false;
                    state.dirty = true;
                    state.save_message = None;
                    state.new_source_name.clear();
                    state.new_source_path.clear();
                    state.new_source_type = ConfigSourceType::File;
                }
                if ui.button("✖ Cancel").clicked() {
                    state.adding_source = false;
                }
            });
        }
    });

    ui.add_space(8.0);

    // ── Save / Reset ────────────────────────────────────────────────────
    semi_transparent_group_frame(ui).show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(state.dirty, egui::Button::new("💾 Save"))
                .on_hover_text(
                    "Save configuration to disk (restart recommended for source changes)",
                )
                .clicked()
            {
                actions.push(UiAction::SaveConfig {
                    config: state.config.clone(),
                });
            }

            if ui
                .add_enabled(state.dirty, egui::Button::new("↩ Reset"))
                .on_hover_text("Discard changes and revert to saved configuration")
                .clicked()
            {
                actions.push(UiAction::ResetSettings);
            }

            if state.dirty {
                ui.label(
                    egui::RichText::new("  ● unsaved changes")
                        .color(egui::Color32::from_rgb(255, 200, 50))
                        .small(),
                );
            }
        });

        // Show save result message
        if let Some((msg, success)) = &state.save_message {
            ui.add_space(4.0);
            let color = if *success {
                egui::Color32::from_rgb(100, 220, 100)
            } else {
                egui::Color32::from_rgb(255, 100, 100)
            };
            ui.label(egui::RichText::new(msg).color(color).small());
        }

        if !state.dirty {
            if let Some((_, true)) = &state.save_message {
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new("ℹ Restart the application to apply source changes.")
                        .weak()
                        .small(),
                );
            }
        }
    });

    ui.add_space(8.0);

    // ── About ───────────────────────────────────────────────────────────
    semi_transparent_group_frame(ui).show(ui, |ui| {
        ui.label(egui::RichText::new("About").strong().size(15.0));
        ui.add_space(4.0);
        ui.label("Homeplayer v0.1.0");
        ui.label(egui::RichText::new("Built with egui & rodio").weak());
    });

    ui.add_space(40.0);
}

/// Paint the station list editor for a given source.
fn paint_stations(ui: &mut egui::Ui, state: &mut SettingsState, source_idx: usize) {
    let stations_len = state.config.sources[source_idx].stations.len();

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("Stations ({})", stations_len))
                .strong()
                .size(13.0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("➕ Add Station").clicked() {
                state.adding_station_for = Some(source_idx);
                state.new_station_name.clear();
                state.new_station_url.clear();
                state.new_station_icon.clear();
            }
        });
    });

    let mut station_to_remove: Option<usize> = None;

    for j in 0..stations_len {
        ui.add_space(2.0);
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("#{}", j + 1)).weak().small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Remove station
                    if state.confirm_remove_station == Some((source_idx, j)) {
                        ui.label(
                            egui::RichText::new("Remove?")
                                .color(egui::Color32::from_rgb(255, 100, 100))
                                .small(),
                        );
                        if ui.small_button("Yes").clicked() {
                            station_to_remove = Some(j);
                            state.confirm_remove_station = None;
                        }
                        if ui.small_button("No").clicked() {
                            state.confirm_remove_station = None;
                        }
                    } else if ui
                        .small_button(
                            egui::RichText::new("🗑").color(egui::Color32::from_rgb(255, 100, 100)),
                        )
                        .on_hover_text("Remove station")
                        .clicked()
                    {
                        state.confirm_remove_station = Some((source_idx, j));
                    }

                    // Move down
                    if j < stations_len - 1 {
                        if ui.small_button("⬇").on_hover_text("Move down").clicked() {
                            state.config.sources[source_idx].stations.swap(j, j + 1);
                            state.dirty = true;
                            state.save_message = None;
                        }
                    }
                    // Move up
                    if j > 0 {
                        if ui.small_button("⬆").on_hover_text("Move up").clicked() {
                            state.config.sources[source_idx].stations.swap(j, j - 1);
                            state.dirty = true;
                            state.save_message = None;
                        }
                    }
                });
            });

            egui::Grid::new(format!("station_grid_{}_{}", source_idx, j))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Name:");
                    if ui
                        .add(
                            egui::TextEdit::singleline(
                                &mut state.config.sources[source_idx].stations[j].name,
                            )
                            .desired_width(ui.available_width() - 8.0),
                        )
                        .changed()
                    {
                        state.dirty = true;
                        state.save_message = None;
                    }
                    ui.end_row();

                    ui.label("URL:");
                    if ui
                        .add(
                            egui::TextEdit::singleline(
                                &mut state.config.sources[source_idx].stations[j].url,
                            )
                            .desired_width(ui.available_width() - 8.0),
                        )
                        .changed()
                    {
                        state.dirty = true;
                        state.save_message = None;
                    }
                    ui.end_row();

                    ui.label("Icon:");
                    if ui
                        .add(
                            egui::TextEdit::singleline(
                                &mut state.config.sources[source_idx].stations[j].icon,
                            )
                            .desired_width(ui.available_width() - 8.0),
                        )
                        .changed()
                    {
                        state.dirty = true;
                        state.save_message = None;
                    }
                    ui.end_row();
                });
        });
    }

    // Handle deferred station removal
    if let Some(idx) = station_to_remove {
        state.config.sources[source_idx].stations.remove(idx);
        state.dirty = true;
        state.save_message = None;
    }

    // Add-station form
    if state.adding_station_for == Some(source_idx) {
        ui.add_space(4.0);
        ui.group(|ui| {
            ui.label(egui::RichText::new("New Station").strong().small());

            egui::Grid::new(format!("new_station_grid_{}", source_idx))
                .num_columns(2)
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Name:");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.new_station_name)
                            .desired_width(ui.available_width() - 8.0),
                    );
                    ui.end_row();

                    ui.label("URL:");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.new_station_url)
                            .desired_width(ui.available_width() - 8.0),
                    );
                    ui.end_row();

                    ui.label("Icon:");
                    ui.add(
                        egui::TextEdit::singleline(&mut state.new_station_icon)
                            .desired_width(ui.available_width() - 8.0),
                    );
                    ui.end_row();
                });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let valid = !state.new_station_name.trim().is_empty()
                    && !state.new_station_url.trim().is_empty();
                if ui.add_enabled(valid, egui::Button::new("✔ Add")).clicked() {
                    state.config.sources[source_idx].stations.push(Station {
                        name: state.new_station_name.trim().to_string(),
                        url: state.new_station_url.trim().to_string(),
                        icon: state.new_station_icon.trim().to_string(),
                    });
                    state.adding_station_for = None;
                    state.dirty = true;
                    state.save_message = None;
                    state.new_station_name.clear();
                    state.new_station_url.clear();
                    state.new_station_icon.clear();
                }
                if ui.button("✖ Cancel").clicked() {
                    state.adding_station_for = None;
                }
            });
        });
    }
}
