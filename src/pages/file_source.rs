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
    source: &Source,
    source_idx: usize,
    data: &FileRenderData,
    is_scanning: bool,
    actions: &mut Vec<UiAction>,
) {
    ui.add_space(8.0);

    // Header – always show the source name
    ui.heading(format!("📁 {}", source.name));
    ui.separator();

    // Action bar
    ui.horizontal(|ui| {
        if is_scanning {
            ui.spinner();
            ui.label(egui::RichText::new("Scanning...").weak().italics());
        } else if ui
            .button("🔄 Scan")
            .on_hover_text("Scan music folder for changes")
            .clicked()
        {
            actions.push(UiAction::ScanSource { source_idx });
        }
    });

    ui.add_space(4.0);

    // Browse mode toggle buttons – always visible, highlighted by what is
    // currently *displayed* (not by the stored browse_mode).
    if data.source_id.is_some() {
        let current_display = displayed_level(&data.browse_level);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("View:").weak().small());
            ui.add_space(4.0);

            let buttons: [(BrowseMode, &str, DisplayedLevel); 3] = [
                (BrowseMode::ByArtist, "🎤 Artist", DisplayedLevel::Artists),
                (BrowseMode::ByAlbum, "💿 Album", DisplayedLevel::Albums),
                (BrowseMode::ByTitle, "🎵 Title", DisplayedLevel::Titles),
            ];

            for (mode, label, level) in &buttons {
                let is_selected = *level == current_display;
                let text = egui::RichText::new(*label).size(13.0);
                let text = if is_selected { text.strong() } else { text };

                let button = egui::Button::new(text).fill(if is_selected {
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
        });

        ui.add_space(4.0);

        // Breadcrumb: show which artist / album filter is active
        paint_breadcrumb(ui, source_idx, data, actions);
    }

    if data.source_id.is_none() && !is_scanning {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                egui::RichText::new("No music indexed yet")
                    .weak()
                    .size(16.0),
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Click 'Scan' to index your music library.")
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
                    .on_hover_text("Back to all artists")
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
                    .on_hover_text("Back to all artists")
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
                        .on_hover_text("Back to albums for this artist")
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
                    .on_hover_text("Back to all albums")
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
            ui.label(egui::RichText::new("No artists found").weak().size(16.0));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Try scanning your music library.")
                    .weak()
                    .small(),
            );
        });
    } else {
        ui.label(
            egui::RichText::new(format!("{} artists", data.artists.len()))
                .weak()
                .small(),
        );
        ui.add_space(4.0);

        let fill = semi_transparent_fill(ui);
        for artist in &data.artists {
            let response = ui.add_sized(
                egui::vec2(ui.available_width(), 40.0),
                egui::Button::new(egui::RichText::new(format!("🎤  {}", artist.name)).size(15.0))
                    .fill(fill)
                    .frame(true),
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
            ui.label(egui::RichText::new("No albums found").weak().size(16.0));
        });
    } else {
        ui.label(
            egui::RichText::new(format!("{} albums", data.albums.len()))
                .weak()
                .small(),
        );
        ui.add_space(4.0);

        let fill = semi_transparent_fill(ui);
        for album in &data.albums {
            let response = ui.add_sized(
                egui::vec2(ui.available_width(), 40.0),
                egui::Button::new(egui::RichText::new(format!("💿  {}", album.name)).size(15.0))
                    .fill(fill)
                    .frame(true),
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
            ui.label(egui::RichText::new("No albums found").weak().size(16.0));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Try scanning your music library.")
                    .weak()
                    .small(),
            );
        });
    } else {
        ui.label(
            egui::RichText::new(format!("{} albums", data.albums.len()))
                .weak()
                .small(),
        );
        ui.add_space(4.0);

        let fill = semi_transparent_fill(ui);
        for album in &data.albums {
            let response = ui.add_sized(
                egui::vec2(ui.available_width(), 40.0),
                egui::Button::new(egui::RichText::new(format!("💿  {}", album.name)).size(15.0))
                    .fill(fill)
                    .frame(true),
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
            ui.label(egui::RichText::new("No titles found").weak().size(16.0));
        });
    } else {
        ui.label(
            egui::RichText::new(format!("{} titles", data.titles.len()))
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

            let response = ui.add_sized(
                egui::vec2(ui.available_width(), 36.0),
                egui::Button::new(egui::RichText::new(label).size(14.0))
                    .fill(fill)
                    .frame(true),
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
