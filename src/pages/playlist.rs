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
                    .size(24.0),
            );
            ui.add_space(8.0);
            ui.label(egui::RichText::new(egui_i18n::tr!("playlist_empty_hint")).weak());
        });
        return;
    }

    // Currently playing index (the item the playback thread is on right now)
    let playing_idx = current_index.saturating_sub(1);

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(egui_i18n::tr!("playlist_n_tracks", {count: queue.len()})).weak(),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clear_btn =
                egui::Button::new(egui::RichText::new(egui_i18n::tr!("playlist_clear_button")))
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

        // Split the row into a main (play) area and a remove-button area,
        // exactly as file_source does, so the two hit-rects never overlap.
        let row_height = 48.0;
        let remove_btn_width = 48.0;
        let gap = ui.spacing().item_spacing.x;
        let total_width = ui.available_width();
        let main_width = (total_width - remove_btn_width - gap).max(0.0);

        let (row_rect, _) =
            ui.allocate_exact_size(egui::vec2(total_width, row_height), egui::Sense::hover());

        let main_rect = egui::Rect::from_min_size(row_rect.min, egui::vec2(main_width, row_height));
        let remove_rect = egui::Rect::from_min_size(
            egui::pos2(
                row_rect.max.x - remove_btn_width,
                row_rect.min.y + (row_height - remove_btn_width) / 2.0,
            ),
            egui::vec2(remove_btn_width, remove_btn_width),
        );

        // Build the label for the main button, same style as file_source rows.
        let num_text = format!("{:>3}.", i + 1);
        let label_text = if meta_part.is_empty() {
            format!("{}  {}  {}", num_text, track_icon, title_part)
        } else {
            format!(
                "{}  {}  {} — {}",
                num_text, track_icon, title_part, meta_part
            )
        };
        let label = if is_playing {
            egui::RichText::new(label_text).strong().size(15.0)
        } else if is_past {
            egui::RichText::new(label_text).weak().size(15.0)
        } else {
            egui::RichText::new(label_text).size(15.0)
        };

        // Main area: plain Button exactly like row_with_add_button in file_source.
        let main_clicked = ui
            .new_child(
                egui::UiBuilder::new()
                    .max_rect(main_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            )
            .add(
                egui::Button::new(label)
                    .fill(row_fill)
                    .frame(true)
                    .min_size(main_rect.size()),
            )
            .clicked();

        if main_clicked {
            actions.push(UiAction::PlaylistPlayFrom { index: i });
        }

        // Remove button in its own non-overlapping rect.
        let remove_clicked = ui
            .put(
                remove_rect,
                egui::Button::new(egui::RichText::new("🗑").size(14.0)),
            )
            .on_hover_text(egui_i18n::tr!("playlist_remove_hover"))
            .clicked();

        if remove_clicked {
            actions.push(UiAction::PlaylistRemove { index: i });
        }
    }
}
