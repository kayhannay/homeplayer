use eframe::egui;

use crate::UiAction;
use crate::config::Source;
use crate::pages::semi_transparent_fill;
use rodio_player::cd_audio::CdTrackInfo;

/// State for the CD source page, stored in the main application.
#[derive(Debug, Clone)]
pub struct CdSourceState {
    /// The loaded track listing, if any.
    pub tracks: Vec<CdTrackInfo>,
    /// Whether a TOC read is currently in progress.
    pub loading: bool,
    /// Status / error message to display.
    pub status: String,
    /// Whether a disc was detected on last check.
    pub disc_present: bool,
}

impl CdSourceState {
    pub fn new() -> Self {
        Self {
            tracks: Vec::new(),
            loading: false,
            status: "Insert a CD and press Refresh.".to_string(),
            disc_present: false,
        }
    }
}

pub fn paint_cd_source(
    ui: &mut egui::Ui,
    _source: &Source,
    source_idx: usize,
    state: &CdSourceState,
    actions: &mut Vec<UiAction>,
) {
    ui.add_space(8.0);

    // Action bar: disc info on the left, Refresh / Eject on the right
    let action_button_size = egui::vec2(120.0, 48.0);
    ui.horizontal(|ui| {
        if state.loading {
            ui.spinner();
            ui.label(egui::RichText::new("Reading disc…").weak().italics());
        } else {
            // Disc summary on the left
            if !state.tracks.is_empty() {
                let audio_tracks: Vec<&CdTrackInfo> =
                    state.tracks.iter().filter(|t| t.is_audio).collect();
                let data_tracks = state.tracks.len() - audio_tracks.len();
                let total_duration: std::time::Duration =
                    audio_tracks.iter().map(|t| t.duration).sum();
                let total_mins = total_duration.as_secs() / 60;
                let total_secs = total_duration.as_secs() % 60;

                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "💿  {} audio track{}  •  {}:{:02}",
                            audio_tracks.len(),
                            if audio_tracks.len() == 1 { "" } else { "s" },
                            total_mins,
                            total_secs,
                        ))
                        .size(14.0),
                    );
                    if data_tracks > 0 {
                        ui.label(
                            egui::RichText::new(format!(
                                "     {} data track{}",
                                data_tracks,
                                if data_tracks == 1 { "" } else { "s" },
                            ))
                            .size(12.0)
                            .weak(),
                        );
                    }
                });
            }

            // Buttons on the right
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_sized(
                        action_button_size,
                        egui::Button::new(egui::RichText::new("⏏ Eject").size(16.0)),
                    )
                    .on_hover_text("Eject the CD tray")
                    .clicked()
                {
                    actions.push(UiAction::EjectCd { source_idx });
                }
                if ui
                    .add_sized(
                        action_button_size,
                        egui::Button::new(egui::RichText::new("🔄 Refresh").size(16.0)),
                    )
                    .on_hover_text("Read the CD table of contents")
                    .clicked()
                {
                    actions.push(UiAction::LoadCdToc { source_idx });
                }
            });
        }
    });

    ui.add_space(4.0);

    // Status message when there are no tracks
    if state.tracks.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("💿").size(64.0));
            ui.add_space(16.0);
            ui.label(egui::RichText::new(&state.status).weak().size(14.0));
        });
        return;
    }

    // Track listing
    let fill = semi_transparent_fill(ui);
    for track in &state.tracks {
        if !track.is_audio {
            // Show data tracks as disabled
            ui.add_space(2.0);
            ui.add(
                egui::Button::new(
                    egui::RichText::new(format!(
                        "  {}   Track {:02}   [data track]",
                        "💾", track.number
                    ))
                    .weak()
                    .size(16.0),
                )
                .fill(fill)
                .frame(true)
                .min_size(egui::vec2(ui.available_width(), 48.0)),
            );
            continue;
        }

        let label = format!(
            "  🎵   Track {:02}   {}",
            track.number,
            track.duration_display(),
        );

        ui.add_space(2.0);
        let response = ui.add(
            egui::Button::new(egui::RichText::new(&label).size(16.0))
                .fill(fill)
                .frame(true)
                .min_size(egui::vec2(ui.available_width(), 48.0)),
        );

        if response.clicked() {
            // Find the index within audio-only tracks for this track
            let audio_index = state
                .tracks
                .iter()
                .filter(|t| t.is_audio)
                .position(|t| t.number == track.number)
                .unwrap_or(0);

            actions.push(UiAction::PlayCd {
                source_idx,
                start_track: audio_index,
            });
        }

        response.on_hover_text(format!(
            "Play from track {} ({} sectors)",
            track.number,
            track.sector_count()
        ));
    }
}
