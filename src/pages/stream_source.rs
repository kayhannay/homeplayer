use std::collections::HashMap;

use eframe::egui;

use crate::UiAction;
use crate::config::Source;
use crate::pages::semi_transparent_fill;

pub fn paint_stream_source(
    ui: &mut egui::Ui,
    source: &Source,
    station_textures: &HashMap<String, egui::TextureHandle>,
    actions: &mut Vec<UiAction>,
) {
    ui.add_space(8.0);

    if source.stations.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(egui_i18n::tr!("no_stations_configured"))
                    .weak()
                    .size(16.0),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(egui_i18n::tr!("add_stations_hint"))
                    .weak()
                    .small(),
            );
        });
        return;
    }

    ui.label(
        egui::RichText::new(egui_i18n::tr!("n_stations", {count: source.stations.len()}))
            .weak()
            .small(),
    );
    ui.add_space(4.0);

    let fill = semi_transparent_fill(ui);
    let icon_size = 40.0;
    let row_height = 48.0;

    for station in &source.stations {
        ui.add_space(2.0);

        let desired_size = egui::vec2(ui.available_width(), row_height);
        let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

        if ui.is_rect_visible(rect) {
            let painter = ui.painter();
            let rounding = egui::CornerRadius::same(6);

            // Background fill
            let bg = if response.hovered() {
                let f = ui.visuals().widgets.hovered.bg_fill;
                egui::Color32::from_rgba_unmultiplied(f.r(), f.g(), f.b(), 200)
            } else {
                fill
            };
            painter.rect_filled(rect, rounding, bg);

            let content_left = rect.min.x + 6.0;
            let text_left;

            // Station icon
            if let Some(tex) = station_textures.get(&station.icon) {
                let icon_rect = egui::Rect::from_min_size(
                    egui::pos2(content_left, rect.center().y - icon_size / 2.0),
                    egui::vec2(icon_size, icon_size),
                );

                let img_size = tex.size_vec2();
                let img_aspect = img_size.x / img_size.y;
                let rect_aspect = icon_rect.width() / icon_rect.height();

                let uv_rect = if img_aspect > rect_aspect {
                    let visible = rect_aspect / img_aspect;
                    let offset = (1.0 - visible) / 2.0;
                    egui::Rect::from_min_max(egui::pos2(offset, 0.0), egui::pos2(1.0 - offset, 1.0))
                } else {
                    let visible = img_aspect / rect_aspect;
                    let offset = (1.0 - visible) / 2.0;
                    egui::Rect::from_min_max(egui::pos2(0.0, offset), egui::pos2(1.0, 1.0 - offset))
                };

                let icon_rounding = egui::CornerRadius::same(4);
                painter.rect_filled(icon_rect, icon_rounding, egui::Color32::BLACK);

                // Clip to rounded rect for the icon
                ui.painter().with_clip_rect(icon_rect).image(
                    tex.id(),
                    icon_rect,
                    uv_rect,
                    egui::Color32::WHITE,
                );

                text_left = icon_rect.max.x + 10.0;
            } else {
                // Fallback: radio emoji as placeholder
                let emoji = "📻";
                let galley = painter.layout_no_wrap(
                    emoji.to_string(),
                    egui::FontId::proportional(icon_size * 0.55),
                    ui.visuals().text_color(),
                );
                let emoji_pos = egui::pos2(
                    content_left + (icon_size - galley.size().x) / 2.0,
                    rect.center().y - galley.size().y / 2.0,
                );
                painter.galley(emoji_pos, galley, ui.visuals().text_color());
                text_left = content_left + icon_size + 10.0;
            }

            // Station name
            let text_max_width = rect.max.x - text_left - 6.0;
            let galley = painter.layout(
                station.name.clone(),
                egui::FontId::proportional(16.0),
                ui.visuals().text_color(),
                text_max_width.max(0.0),
            );
            let text_pos = egui::pos2(text_left, rect.center().y - galley.size().y / 2.0);
            painter.galley(text_pos, galley, ui.visuals().text_color());
        }

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
