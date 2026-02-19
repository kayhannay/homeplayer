use eframe::egui;

use crate::BrowseLevel;
use crate::BrowseMode;
use crate::UiAction;
use crate::config::Source;
use crate::music_store::{MusicItem, MusicTitleItem};
use crate::pages::semi_transparent_fill;

// ---------------------------------------------------------------------------
// Render data (cloned for the closure)
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct FileRenderData {
    pub source_id: Option<i32>,
    pub browse_level: BrowseLevel,
    pub artists: Vec<MusicItem>,
    pub albums: Vec<MusicItem>,
    pub titles: Vec<MusicTitleItem>,
}

// ---------------------------------------------------------------------------
// Helpers to determine what is currently displayed
// ---------------------------------------------------------------------------

/// Which conceptual level the user is currently looking at, derived from `BrowseLevel`.
#[derive(PartialEq)]
enum DisplayedLevel {
    Artists,
    Albums,
    Titles,
}

fn displayed_level(bl: &BrowseLevel) -> DisplayedLevel {
    match bl {
        BrowseLevel::Artists => DisplayedLevel::Artists,
        BrowseLevel::Albums { .. } | BrowseLevel::AllAlbums => DisplayedLevel::Albums,
        BrowseLevel::Titles { .. }
        | BrowseLevel::TitlesForAlbum { .. }
        | BrowseLevel::AllTitles => DisplayedLevel::Titles,
    }
}

// ---------------------------------------------------------------------------
// File source page
// ---------------------------------------------------------------------------

pub fn paint_file_source(
    ui: &mut egui::Ui,
    _source: &Source,
    source_idx: usize,
    data: &FileRenderData,
    is_scanning: bool,
    actions: &mut Vec<UiAction>,
) {
    ui.add_space(8.0);

    // Action bar: browse mode buttons on the left, Scan on the right
    ui.horizontal(|ui| {
        // Browse mode toggle buttons (left side)
        if data.source_id.is_some() {
            let current_display = displayed_level(&data.browse_level);

            let buttons: [(BrowseMode, String, DisplayedLevel); 3] = [
                (
                    BrowseMode::ByArtist,
                    egui_i18n::tr!("browse_artist"),
                    DisplayedLevel::Artists,
                ),
                (
                    BrowseMode::ByAlbum,
                    egui_i18n::tr!("browse_album"),
                    DisplayedLevel::Albums,
                ),
                (
                    BrowseMode::ByTitle,
                    egui_i18n::tr!("browse_title"),
                    DisplayedLevel::Titles,
                ),
            ];

            for (mode, label, level) in &buttons {
                let is_selected = *level == current_display;
                let text = egui::RichText::new(label.as_str()).size(15.0);
                let text = if is_selected { text.strong() } else { text };

                let button = egui::Button::new(text)
                    .min_size(egui::vec2(0.0, 48.0))
                    .fill(if is_selected {
                        ui.visuals().selection.bg_fill
                    } else {
                        egui::Color32::TRANSPARENT
                    });

                if ui.add(button).clicked() && !is_selected {
                    // Clicking a view button always resets to the unfiltered
                    // top-level for that mode.
                    actions.push(UiAction::SwitchBrowseMode {
                        source_idx,
                        mode: mode.clone(),
                    });
                }
            }
        }

        // Scan button (right side)
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

    if data.source_id.is_some() {
        // Breadcrumb: show which artist / album filter is active
        paint_breadcrumb(ui, source_idx, data, actions);
    }

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

    match &data.browse_level {
        BrowseLevel::Artists => {
            paint_artist_list(ui, source_idx, data, actions);
        }
        BrowseLevel::Albums { .. } => {
            paint_album_list_artist_mode(ui, source_idx, data, actions);
        }
        BrowseLevel::Titles { .. } => {
            paint_title_list(ui, data, actions);
        }
        BrowseLevel::AllAlbums => {
            paint_album_list_album_mode(ui, source_idx, data, actions);
        }
        BrowseLevel::TitlesForAlbum { .. } => {
            paint_title_list(ui, data, actions);
        }
        BrowseLevel::AllTitles => {
            paint_title_list(ui, data, actions);
        }
    }
}

// ---------------------------------------------------------------------------
// Breadcrumb
// ---------------------------------------------------------------------------

fn paint_breadcrumb(
    ui: &mut egui::Ui,
    source_idx: usize,
    data: &FileRenderData,
    actions: &mut Vec<UiAction>,
) {
    match &data.browse_level {
        // Root levels – no breadcrumb needed
        BrowseLevel::Artists | BrowseLevel::AllAlbums | BrowseLevel::AllTitles => {}

        // Albums filtered by artist
        BrowseLevel::Albums { artist_name, .. } => {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🎤").size(13.0));
                // Clicking the artist name goes back to the artist list
                if ui
                    .link(egui::RichText::new(artist_name).size(13.0).strong())
                    .on_hover_text(egui_i18n::tr!("back_to_all_artists"))
                    .clicked()
                {
                    actions.push(UiAction::SwitchBrowseMode {
                        source_idx,
                        mode: BrowseMode::ByArtist,
                    });
                }
            });
            ui.add_space(2.0);
        }

        // Titles filtered by artist + album
        BrowseLevel::Titles {
            artist_name,
            album_name,
            ..
        } => {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🎤").size(13.0));
                if ui
                    .link(egui::RichText::new(artist_name).size(13.0).strong())
                    .on_hover_text(egui_i18n::tr!("back_to_all_artists"))
                    .clicked()
                {
                    actions.push(UiAction::SwitchBrowseMode {
                        source_idx,
                        mode: BrowseMode::ByArtist,
                    });
                }
                ui.label(egui::RichText::new("›").weak().size(13.0));
                ui.label(egui::RichText::new("💿").size(13.0));
                // Clicking the album name goes back to albums for this artist
                if let BrowseLevel::Titles {
                    artist_id,
                    artist_name,
                    ..
                } = &data.browse_level
                {
                    if ui
                        .link(egui::RichText::new(album_name).size(13.0).strong())
                        .on_hover_text(egui_i18n::tr!("back_to_albums_for_artist"))
                        .clicked()
                    {
                        actions.push(UiAction::BrowseAlbums {
                            source_idx,
                            artist_id: *artist_id,
                            artist_name: artist_name.clone(),
                        });
                    }
                }
            });
            ui.add_space(2.0);
        }

        // Titles filtered by album only (from "All Albums" view)
        BrowseLevel::TitlesForAlbum { album_name, .. } => {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("💿").size(13.0));
                if ui
                    .link(egui::RichText::new(album_name).size(13.0).strong())
                    .on_hover_text(egui_i18n::tr!("back_to_all_albums"))
                    .clicked()
                {
                    actions.push(UiAction::SwitchBrowseMode {
                        source_idx,
                        mode: BrowseMode::ByAlbum,
                    });
                }
            });
            ui.add_space(2.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Sub-painters
// ---------------------------------------------------------------------------

fn paint_artist_list(
    ui: &mut egui::Ui,
    source_idx: usize,
    data: &FileRenderData,
    actions: &mut Vec<UiAction>,
) {
    if data.artists.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(egui_i18n::tr!("no_artists_found"))
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
    } else {
        ui.label(
            egui::RichText::new(egui_i18n::tr!("n_artists", {count: data.artists.len()}))
                .weak()
                .small(),
        );
        ui.add_space(4.0);

        let fill = semi_transparent_fill(ui);
        for artist in &data.artists {
            let response = ui.add(
                egui::Button::new(egui::RichText::new(format!("🎤  {}", artist.name)).size(16.0))
                    .fill(fill)
                    .frame(true)
                    .min_size(egui::vec2(ui.available_width(), 48.0)),
            );
            if response.clicked() {
                actions.push(UiAction::BrowseAlbums {
                    source_idx,
                    artist_id: artist.id,
                    artist_name: artist.name.clone(),
                });
            }
        }
    }
}

fn paint_album_list_artist_mode(
    ui: &mut egui::Ui,
    source_idx: usize,
    data: &FileRenderData,
    actions: &mut Vec<UiAction>,
) {
    let (artist_id, artist_name) = match &data.browse_level {
        BrowseLevel::Albums {
            artist_id,
            artist_name,
        } => (*artist_id, artist_name.clone()),
        _ => return,
    };

    if data.albums.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(egui_i18n::tr!("no_albums_found"))
                    .weak()
                    .size(16.0),
            );
        });
    } else {
        ui.label(
            egui::RichText::new(egui_i18n::tr!("n_albums", {count: data.albums.len()}))
                .weak()
                .small(),
        );
        ui.add_space(4.0);

        let fill = semi_transparent_fill(ui);
        for album in &data.albums {
            let response = ui.add(
                egui::Button::new(egui::RichText::new(format!("💿  {}", album.name)).size(16.0))
                    .fill(fill)
                    .frame(true)
                    .min_size(egui::vec2(ui.available_width(), 48.0)),
            );
            if response.clicked() {
                actions.push(UiAction::BrowseTitles {
                    source_idx,
                    artist_id,
                    artist_name: artist_name.clone(),
                    album_id: album.id,
                    album_name: album.name.clone(),
                });
            }
        }
    }
}

fn paint_album_list_album_mode(
    ui: &mut egui::Ui,
    source_idx: usize,
    data: &FileRenderData,
    actions: &mut Vec<UiAction>,
) {
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
    } else {
        ui.label(
            egui::RichText::new(egui_i18n::tr!("n_albums", {count: data.albums.len()}))
                .weak()
                .small(),
        );
        ui.add_space(4.0);

        let fill = semi_transparent_fill(ui);
        for album in &data.albums {
            let response = ui.add(
                egui::Button::new(egui::RichText::new(format!("💿  {}", album.name)).size(16.0))
                    .fill(fill)
                    .frame(true)
                    .min_size(egui::vec2(ui.available_width(), 48.0)),
            );
            if response.clicked() {
                actions.push(UiAction::BrowseAlbumTitles {
                    source_idx,
                    album_id: album.id,
                    album_name: album.name.clone(),
                });
            }
        }
    }
}

fn paint_title_list(ui: &mut egui::Ui, data: &FileRenderData, actions: &mut Vec<UiAction>) {
    if data.titles.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new(egui_i18n::tr!("no_titles_found"))
                    .weak()
                    .size(16.0),
            );
        });
    } else {
        ui.label(
            egui::RichText::new(egui_i18n::tr!("n_titles", {count: data.titles.len()}))
                .weak()
                .small(),
        );
        ui.add_space(4.0);

        let fill = semi_transparent_fill(ui);
        let show_extra_info = matches!(
            data.browse_level,
            BrowseLevel::AllTitles | BrowseLevel::TitlesForAlbum { .. }
        );

        for (i, title) in data.titles.iter().enumerate() {
            let label = if show_extra_info {
                format!("🎵  {} — {} — {}", title.artist, title.album, title.name)
            } else {
                format!("🎵  {}", title.name)
            };

            let response = ui.add(
                egui::Button::new(egui::RichText::new(label).size(15.0))
                    .fill(fill)
                    .frame(true)
                    .min_size(egui::vec2(ui.available_width(), 48.0)),
            );
            if response.clicked() {
                actions.push(UiAction::PlayTitles {
                    titles: data.titles.clone(),
                    start_index: i,
                });
            }
        }
    }
}
