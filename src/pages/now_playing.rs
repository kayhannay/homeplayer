use eframe::egui;
use rodio_player::TitleChanged;

use crate::pages::semi_transparent_fill;

pub fn paint_now_playing(
    ui: &mut egui::Ui,
    current_title: &TitleChanged,
    is_playing: bool,
    is_paused: bool,
    volume: f32,
    cover_texture: Option<&egui::TextureHandle>,
) {
    let art_size = egui::vec2(150.0, 150.0);

    // Estimate the height of the content block so we can vertically center it.
    // The content is a horizontal row whose tallest element is the album art (150px).
    let content_height = art_size.y;

    let available_height = ui.available_height();
    let top_padding = ((available_height - content_height) / 2.0).max(0.0);

    ui.add_space(top_padding);

    ui.horizontal(|ui| {
        let available_width = ui.available_width();
        ui.add_space(available_width / 10.0);

        // Album art or placeholder on the left
        if let Some(texture) = cover_texture {
            let img_size = texture.size_vec2();
            let img_aspect = img_size.x / img_size.y;
            let art_aspect = art_size.x / art_size.y;

            // "Cover" scaling: fill the square while preserving aspect ratio
            let uv_rect = if img_aspect > art_aspect {
                let visible_fraction = art_aspect / img_aspect;
                let offset = (1.0 - visible_fraction) / 2.0;
                egui::Rect::from_min_max(egui::pos2(offset, 0.0), egui::pos2(1.0 - offset, 1.0))
            } else {
                let visible_fraction = img_aspect / art_aspect;
                let offset = (1.0 - visible_fraction) / 2.0;
                egui::Rect::from_min_max(egui::pos2(0.0, offset), egui::pos2(1.0, 1.0 - offset))
            };

            let (rect, _) = ui.allocate_exact_size(art_size, egui::Sense::hover());
            let rounding = egui::CornerRadius::same(12);

            // Paint a semi-transparent background behind the image for consistency
            ui.painter()
                .rect_filled(rect, rounding, semi_transparent_fill(ui));

            // Draw the cover image with rounded corners by using the clip rect
            ui.painter().with_clip_rect(rect).image(
                texture.id(),
                rect,
                uv_rect,
                egui::Color32::WHITE,
            );
        } else {
            // Fallback placeholder
            let (rect, _) = ui.allocate_exact_size(art_size, egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 12.0, semi_transparent_fill(ui));
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "🎵",
                egui::FontId::proportional(64.0),
                ui.visuals().text_color(),
            );
        }

        ui.add_space(16.0);

        // Title, artist, album, and status text on the right, vertically centered
        // relative to the album art
        ui.vertical(|ui| {
            // Estimate text block height to center it within the art_size height
            let title_height = 34.0; // approximate for size 28
            let spacing = 4.0;
            let artist_height = 26.0; // approximate for size 22
            let album_height = if !current_title.album.is_empty() && current_title.album != "-" {
                22.0
            } else {
                0.0
            };
            let text_block_height = title_height + spacing + artist_height + album_height;
            let text_top_padding = ((art_size.y - text_block_height) / 2.0).max(0.0);

            ui.add_space(text_top_padding);

            let title_text = if current_title.title.is_empty() {
                egui_i18n::tr!("no_track_selected")
            } else {
                current_title.title.clone()
            };
            ui.label(egui::RichText::new(title_text).strong().size(28.0));
            ui.add_space(4.0);

            let artist_text = if current_title.artist.is_empty() {
                egui_i18n::tr!("unknown_artist")
            } else {
                current_title.artist.clone()
            };
            ui.label(egui::RichText::new(artist_text).weak().size(22.0));

            let album = if current_title.album.is_empty() || current_title.album == "-" {
                ""
            } else {
                &current_title.album
            };
            if !album.is_empty() {
                ui.label(egui::RichText::new(album).weak().italics().size(18.0));
            }
        });
    });
}
