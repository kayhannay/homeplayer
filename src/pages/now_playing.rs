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
    ui.add_space(8.0);
    ui.heading("Now Playing");
    ui.separator();
    ui.add_space(20.0);

    // Album art or placeholder
    ui.vertical_centered(|ui| {
        let art_size = egui::vec2(200.0, 200.0);

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
    });

    ui.add_space(20.0);

    ui.vertical_centered(|ui| {
        let title = if current_title.title.is_empty() {
            "No track selected"
        } else {
            &current_title.title
        };
        ui.label(egui::RichText::new(title).strong().size(20.0));
        ui.add_space(4.0);

        let artist = if current_title.artist.is_empty() {
            "Unknown Artist"
        } else {
            &current_title.artist
        };
        ui.label(egui::RichText::new(artist).weak().size(15.0));

        let album = if current_title.album.is_empty() || current_title.album == "-" {
            ""
        } else {
            &current_title.album
        };
        if !album.is_empty() {
            ui.label(egui::RichText::new(album).weak().italics());
        }
    });

    ui.add_space(20.0);

    ui.vertical_centered(|ui| {
        let status = if is_playing && !is_paused {
            "▶ Playing"
        } else if is_paused {
            "⏸ Paused"
        } else {
            "⏹ Stopped"
        };
        ui.label(egui::RichText::new(status).size(14.0));
        ui.label(
            egui::RichText::new(format!("Volume: {:.0}%", volume * 100.0))
                .weak()
                .small(),
        );
    });
}
