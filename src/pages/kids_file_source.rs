use std::collections::HashMap;

use eframe::egui;

use crate::UiAction;
use crate::config::Source;
use crate::music_store::KidsAlbumItem;

// ---------------------------------------------------------------------------
// Render data (cloned for the closure)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct KidsFileRenderData {
    pub source_id: Option<i32>,
    pub albums: Vec<KidsAlbumItem>,
}

// ---------------------------------------------------------------------------
// Kids file source page – album grid with cover art
// ---------------------------------------------------------------------------

pub fn paint_kids_file_source(
    ui: &mut egui::Ui,
    _source: &Source,
    source_idx: usize,
    data: &KidsFileRenderData,
    cover_textures: &HashMap<String, egui::TextureHandle>,
    is_scanning: bool,
    actions: &mut Vec<UiAction>,
) {
    ui.add_space(8.0);

    // Scan button row
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if is_scanning {
                ui.label(
                    egui::RichText::new(egui_i18n::tr!("scanning"))
                        .weak()
                        .italics(),
                );
                ui.spinner();
            } else if ui
                .add_sized(
                    egui::vec2(120.0, 48.0),
                    egui::Button::new(
                        egui::RichText::new(egui_i18n::tr!("scan_button")).size(16.0),
                    ),
                )
                .on_hover_text(egui_i18n::tr!("scan_hover"))
                .clicked()
            {
                actions.push(UiAction::ScanSource { source_idx });
            }
        });
    });

    ui.add_space(4.0);

    if data.source_id.is_none() && !is_scanning {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(egui_i18n::tr!("no_music_indexed"))
                    .weak()
                    .size(16.0),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(egui_i18n::tr!("click_scan_hint"))
                    .weak()
                    .small(),
            );
        });
        return;
    }

    if data.albums.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(egui_i18n::tr!("no_albums_found"))
                    .weak()
                    .size(16.0),
            );
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(egui_i18n::tr!("try_scanning_hint"))
                    .weak()
                    .small(),
            );
        });
        return;
    }

    ui.label(
        egui::RichText::new(egui_i18n::tr!("n_albums", {count: data.albums.len()}))
            .weak()
            .small(),
    );
    ui.add_space(4.0);

    // Album grid
    paint_album_grid(ui, source_idx, data, cover_textures, actions);
}

// ---------------------------------------------------------------------------
// Album grid with cover art
// ---------------------------------------------------------------------------

fn paint_album_grid(
    ui: &mut egui::Ui,
    source_idx: usize,
    data: &KidsFileRenderData,
    cover_textures: &HashMap<String, egui::TextureHandle>,
    actions: &mut Vec<UiAction>,
) {
    let available_width = ui.available_width();

    // Each tile: cover image + two lines of text below.
    // Target tile width ~160px, but adapt to fill the width evenly.
    let min_tile_width: f32 = 140.0;
    let spacing: f32 = 12.0;
    let columns = ((available_width + spacing) / (min_tile_width + spacing))
        .floor()
        .max(1.0) as usize;
    let tile_width = (available_width - spacing * (columns as f32 - 1.0)) / columns as f32;
    let cover_size = tile_width;
    let text_height: f32 = 40.0; // room for artist + album labels
    let tile_height = cover_size + text_height;

    // No inner ScrollArea here — the SwipeView already wraps every page in a
    // vertical ScrollArea that has the drag-jump fix applied.  Adding another
    // nested ScrollArea would shadow the outer one and re-introduce the bug.
    let albums = &data.albums;
    let num_rows = albums.len().div_ceil(columns);

    for row in 0..num_rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = spacing;

            for col in 0..columns {
                let idx = row * columns + col;
                if idx >= albums.len() {
                    // Empty cell – reserve space to keep the grid aligned
                    ui.allocate_space(egui::vec2(tile_width, tile_height));
                    continue;
                }
                let album = &albums[idx];

                // Wrap each tile in a vertical group so we can detect
                // the click on the whole area.
                let (rect, response) = ui
                    .allocate_exact_size(egui::vec2(tile_width, tile_height), egui::Sense::click());

                if ui.is_rect_visible(rect) {
                    let painter = ui.painter_at(rect);

                    // Background for the tile
                    let bg_fill = ui.visuals().widgets.inactive.bg_fill;
                    let bg_fill = egui::Color32::from_rgba_unmultiplied(
                        bg_fill.r(),
                        bg_fill.g(),
                        bg_fill.b(),
                        180,
                    );
                    painter.rect_filled(rect, egui::CornerRadius::same(8), bg_fill);

                    // Hover highlight
                    if response.hovered() {
                        let hover_fill = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 30);
                        painter.rect_filled(rect, egui::CornerRadius::same(8), hover_fill);
                    }

                    // Cover image area
                    let cover_rect =
                        egui::Rect::from_min_size(rect.min, egui::vec2(tile_width, cover_size));

                    if let Some(tex) = cover_textures.get(&album.cover) {
                        // Draw the cover image, maintaining aspect ratio
                        // by "cover" fitting into the square.
                        let img_size = tex.size_vec2();
                        let img_aspect = img_size.x / img_size.y;
                        let rect_aspect = cover_rect.width() / cover_rect.height();

                        let uv_rect = if img_aspect > rect_aspect {
                            let visible = rect_aspect / img_aspect;
                            let offset = (1.0 - visible) / 2.0;
                            egui::Rect::from_min_max(
                                egui::pos2(offset, 0.0),
                                egui::pos2(1.0 - offset, 1.0),
                            )
                        } else {
                            let visible = img_aspect / rect_aspect;
                            let offset = (1.0 - visible) / 2.0;
                            egui::Rect::from_min_max(
                                egui::pos2(0.0, offset),
                                egui::pos2(1.0, 1.0 - offset),
                            )
                        };

                        // Round the top corners of the cover image
                        // by clipping to the tile rect
                        painter.image(tex.id(), cover_rect, uv_rect, egui::Color32::WHITE);
                    } else {
                        // Placeholder: album emoji centered in the cover area
                        let placeholder_text = egui::RichText::new("💿").size(cover_size * 0.4);
                        let galley = ui.painter().layout_no_wrap(
                            placeholder_text.text().to_string(),
                            egui::FontId::proportional(cover_size * 0.4),
                            ui.visuals().text_color(),
                        );
                        let text_pos = cover_rect.center()
                            - egui::vec2(galley.size().x / 2.0, galley.size().y / 2.0);
                        painter.galley(text_pos, galley, ui.visuals().text_color());
                    }

                    // Text area below the cover
                    let text_rect = egui::Rect::from_min_size(
                        egui::pos2(rect.min.x + 4.0, rect.min.y + cover_size + 2.0),
                        egui::vec2(tile_width - 8.0, text_height - 4.0),
                    );

                    // Album name (bold, truncated)
                    let album_galley = ui.painter().layout(
                        album.album_name.clone(),
                        egui::FontId::proportional(13.0),
                        ui.visuals().strong_text_color(),
                        text_rect.width(),
                    );
                    painter.galley(
                        text_rect.min,
                        album_galley.clone(),
                        ui.visuals().strong_text_color(),
                    );

                    // Artist name (smaller, weak, below album name)
                    let artist_y = text_rect.min.y + album_galley.size().y.min(18.0) + 1.0;
                    let artist_galley = ui.painter().layout(
                        album.artist_name.clone(),
                        egui::FontId::proportional(11.0),
                        ui.visuals().weak_text_color(),
                        text_rect.width(),
                    );
                    painter.galley(
                        egui::pos2(text_rect.min.x, artist_y),
                        artist_galley,
                        ui.visuals().weak_text_color(),
                    );
                }

                // Handle click – play all titles from this album
                if response.clicked() {
                    actions.push(UiAction::PlayKidsAlbum {
                        source_idx,
                        album_id: album.id,
                    });
                }
            }
        });

        ui.add_space(spacing);
    }
}
