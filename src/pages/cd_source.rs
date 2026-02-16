use eframe::egui;

use crate::UiAction;
use crate::config::Source;
use crate::pages::{semi_transparent_fill, semi_transparent_group_frame};
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
    source: &Source,
    source_idx: usize,
    state: &CdSourceState,
    actions: &mut Vec<UiAction>,
) {
    ui.add_space(8.0);
    ui.heading(format!("💿 {}", source.name));
    ui.separator();

    // Action bar: Refresh / Eject
    ui.horizontal(|ui| {
        if state.loading {
            ui.spinner();
            ui.label(egui::RichText::new("Reading disc…").weak().italics());
        } else {
            if ui
                .button("🔄 Refresh")
                .on_hover_text("Read the CD table of contents")
                .clicked()
            {
                actions.push(UiAction::LoadCdToc { source_idx });
            }
            if ui
                .button("⏏ Eject")
                .on_hover_text("Eject the CD tray")
                .clicked()
            {
                actions.push(UiAction::EjectCd { source_idx });
            }
        }
    });

    ui.add_space(4.0);

    // Device path
    if !source.path.is_empty() {
        ui.label(
            egui::RichText::new(format!("Device: {}", source.path))
                .weak()
                .small(),
        );
        ui.add_space(4.0);
    }

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

    // Disc summary
    let audio_tracks: Vec<&CdTrackInfo> = state.tracks.iter().filter(|t| t.is_audio).collect();
    let total_duration: std::time::Duration = audio_tracks.iter().map(|t| t.duration).sum();
    let total_mins = total_duration.as_secs() / 60;
    let total_secs = total_duration.as_secs() % 60;

    semi_transparent_group_frame(ui).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{} audio tracks  •  {}:{:02} total",
                    audio_tracks.len(),
                    total_mins,
                    total_secs,
                ))
                .size(13.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(egui::RichText::new("▶ Play All").size(13.0))
                    .on_hover_text("Play all audio tracks from the beginning")
                    .clicked()
                {
                    actions.push(UiAction::PlayCd {
                        source_idx,
                        start_track: 0,
                    });
                }
            });
        });
    });

    ui.add_space(4.0);

    // Track listing
    let fill = semi_transparent_fill(ui);
    for track in &state.tracks {
        if !track.is_audio {
            // Show data tracks as disabled
            ui.add_space(2.0);
            ui.add_sized(
                egui::vec2(ui.available_width(), 40.0),
                egui::Button::new(
                    egui::RichText::new(format!(
                        "  {}   Track {:02}   [data track]",
                        "💾", track.number
                    ))
                    .weak()
                    .size(14.0),
                )
                .fill(fill)
                .frame(true),
            );
            continue;
        }

        let label = format!(
            "  🎵   Track {:02}   {}",
            track.number,
            track.duration_display(),
        );

        ui.add_space(2.0);
        let response = ui.add_sized(
            egui::vec2(ui.available_width(), 40.0),
            egui::Button::new(egui::RichText::new(&label).size(15.0))
                .fill(fill)
                .frame(true),
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
