use eframe::egui;
use rodio_player::SoundItem;

use crate::UiAction;
use crate::pages::semi_transparent_fill;

/// Render the playlist page.
///
/// `queue` is the full list of items in the sound queue.
/// `current_index` is the *next* index the playback thread will pick up, so
/// the currently-playing item sits at `current_index.saturating_sub(1)`.
pub fn paint_playlist(
    ui: &mut egui::Ui,
    queue: &[SoundItem],
    current_index: usize,
    actions: &mut Vec<UiAction>,
) {
    ui.add_space(8.0);

    if queue.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(
                egui::RichText::new(egui_i18n::tr!("playlist_empty"))
                    .weak()
                    .size(18.0),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(egui_i18n::tr!("playlist_empty_hint"))
                    .weak()
                    .small(),
            );
        });
        return;
    }

    // Currently playing index (the item the playback thread is on right now)
    let playing_idx = current_index.saturating_sub(1);

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(egui_i18n::tr!("playlist_n_tracks", {count: queue.len()}))
                .weak()
                .small(),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clear_btn = egui::Button::new(
                egui::RichText::new(egui_i18n::tr!("playlist_clear_button")).small(),
            )
            .fill(egui::Color32::TRANSPARENT);

            if ui
                .add(clear_btn)
                .on_hover_text(egui_i18n::tr!("playlist_clear_hover"))
                .clicked()
            {
                actions.push(UiAction::PlaylistClear);
            }
        });
    });
    ui.add_space(4.0);

    let fill = semi_transparent_fill(ui);

    for (i, item) in queue.iter().enumerate() {
        let is_playing = i == playing_idx && current_index > 0;
        let is_past = current_index > 0 && i < playing_idx;

        // Build the label text
        let track_icon = if is_playing {
            "▶"
        } else if is_past {
            "✓"
        } else {
            "🎵"
        };

        let title_part = if item.title.is_empty() {
            egui_i18n::tr!("no_track_selected")
        } else {
            item.title.clone()
        };

        let meta_part = if !item.artist.is_empty() && !item.album.is_empty() {
            format!("{}  —  {}", item.artist, item.album)
        } else if !item.artist.is_empty() {
            item.artist.clone()
        } else if !item.album.is_empty() {
            item.album.clone()
        } else {
            String::new()
        };

        // Row frame
        let row_fill = if is_playing {
            // Slightly tinted highlight for the currently-playing track
            let sel = ui.visuals().selection.bg_fill;
            egui::Color32::from_rgba_unmultiplied(sel.r(), sel.g(), sel.b(), 120)
        } else {
            fill
        };

        egui::Frame::new()
            .fill(row_fill)
            .corner_radius(egui::CornerRadius::same(4))
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());

                ui.horizontal(|ui| {
                    // Track number badge
                    let num_text = format!("{:>3}.", i + 1);
                    ui.label(egui::RichText::new(num_text).weak().monospace().size(13.0));

                    ui.label(egui::RichText::new(track_icon).size(14.0));

                    // Title + metadata (takes all remaining space)
                    ui.vertical(|ui| {
                        let title_style = if is_playing {
                            egui::RichText::new(&title_part).strong().size(15.0)
                        } else if is_past {
                            egui::RichText::new(&title_part).weak().size(15.0)
                        } else {
                            egui::RichText::new(&title_part).size(15.0)
                        };
                        ui.label(title_style);

                        if !meta_part.is_empty() {
                            ui.label(egui::RichText::new(&meta_part).weak().italics().size(12.0));
                        }
                    });

                    // Remove button pinned to the right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let remove_btn = egui::Button::new(egui::RichText::new("🗑").size(14.0))
                            .fill(egui::Color32::TRANSPARENT)
                            .min_size(egui::vec2(36.0, 36.0));

                        if ui
                            .add(remove_btn)
                            .on_hover_text(egui_i18n::tr!("playlist_remove_hover"))
                            .clicked()
                        {
                            actions.push(UiAction::PlaylistRemove { index: i });
                        }
                    });
                });
            });

        ui.add_space(2.0);
    }
}
