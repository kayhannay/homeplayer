mod config;
mod music_store;
mod pages;
mod swipe_view;

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use eframe::{NativeOptions, egui};
use egui::{ColorImage, TextureHandle, TextureOptions};
use rodio_player::{PlayerState, RodioPlayer, SoundItem, TitleChanged};
use rusqlite::Connection;
use tracing::{debug, error, info, warn};

use crate::config::{AudioConfig, Config, ConfigSourceType, UiConfig};
use crate::music_store::{KidsAlbumItem, MusicItem, MusicStore, MusicTitleItem};
use crate::pages::{
    CdSourceState, FileRenderData, KidsFileRenderData, SettingsState, paint_cd_source,
    paint_file_source, paint_kids_file_source, paint_now_playing, paint_settings,
    paint_stream_source, source_type_icon,
};
use crate::swipe_view::SwipeView;

fn init_i18n(language: &str) {
    let en = String::from_utf8_lossy(include_bytes!("../assets/languages/en.egl"));
    let de = String::from_utf8_lossy(include_bytes!("../assets/languages/de.egl"));
    egui_i18n::load_translations_from_text("en", en).unwrap();
    egui_i18n::load_translations_from_text("de", de).unwrap();
    egui_i18n::set_fallback("en");
    egui_i18n::set_language(language);
}

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt::init();

    let config = match Config::new() {
        Ok(cfg) => {
            info!("Configuration loaded successfully");
            cfg
        }
        Err(err) => {
            error!("Failed to load configuration: {err}");
            warn!("Using default configuration");
            Config {
                sources: vec![],
                audio: AudioConfig {
                    start_volume: 50,
                    device: None,
                },
                ui: UiConfig::default(),
            }
        }
    };

    init_i18n(&config.ui.language);

    let initial_volume = (config.audio.start_volume.min(100) as f32) / 100.0;

    // Initialize music store
    let music_store = match Connection::open("music_store.db3") {
        Ok(conn) => {
            let store = MusicStore::new(conn);
            if let Err(e) = store.init() {
                error!("Failed to initialize music store: {e}");
            }
            Some(Arc::new(store))
        }
        Err(e) => {
            error!("Failed to open music store database: {e}");
            None
        }
    };

    // Create player channels
    let (title_tx, title_rx) = mpsc::channel();
    let (button_tx, button_rx) = mpsc::channel();

    // Create player
    let player = RodioPlayer::new(title_tx, button_tx, config.audio.device.as_deref());
    player.set_volume(initial_volume);

    // Build dynamic pages
    let mut pages: Vec<DynamicPage> = Vec::new();
    pages.push(DynamicPage::NowPlaying);
    for (i, _source) in config.sources.iter().enumerate() {
        pages.push(DynamicPage::Source(i));
    }
    if !config.ui.hide_settings {
        pages.push(DynamicPage::Settings);
    }

    let num_pages = pages.len();

    // Initialize file source states
    let mut file_source_states: HashMap<usize, FileSourceState> = HashMap::new();
    for (i, source) in config.sources.iter().enumerate() {
        if matches!(source.source_type, ConfigSourceType::File) {
            let mut state = FileSourceState::new();
            if let Some(ref store) = music_store
                && let Ok(source_id) = store.get_source_id(&source.name)
            {
                state.source_id = Some(source_id);
                if let Ok(artists) = store.get_artists(source_id) {
                    state.artists = artists;
                }
            }
            file_source_states.insert(i, state);
        }
    }

    // Initialize KidsFile source states
    let mut kids_file_source_states: HashMap<usize, KidsFileSourceState> = HashMap::new();
    for (i, source) in config.sources.iter().enumerate() {
        if matches!(source.source_type, ConfigSourceType::KidsFile) {
            let mut state = KidsFileSourceState::new();
            if let Some(ref store) = music_store
                && let Ok(source_id) = store.get_source_id(&source.name)
            {
                state.source_id = Some(source_id);
                if let Ok(albums) = store.get_albums_with_artist(source_id) {
                    state.albums = albums;
                }
            }
            kids_file_source_states.insert(i, state);
        }
    }

    // Initialize CD source states
    let mut cd_source_states: HashMap<usize, CdSourceState> = HashMap::new();
    for (i, source) in config.sources.iter().enumerate() {
        if matches!(source.source_type, ConfigSourceType::CD) {
            cd_source_states.insert(i, CdSourceState::new());
        }
    }

    // Tokio runtime for async stream playback
    let tokio_rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");

    let options = NativeOptions::default();
    eframe::run_native(
        "Homeplayer",
        options,
        Box::new(move |cc| {
            // Bridge player channels through repaint-requesting threads.
            // Whenever the player sends a title change or state update, these
            // threads forward the message and wake up the UI so it processes
            // the change immediately — even if no other interaction is happening.
            let ctx = cc.egui_ctx.clone();
            let (bridged_title_tx, bridged_title_rx) = mpsc::channel();
            let ctx_for_titles = ctx.clone();
            std::thread::Builder::new()
                .name("title-bridge".into())
                .spawn(move || {
                    while let Ok(msg) = title_rx.recv() {
                        let _ = bridged_title_tx.send(msg);
                        ctx_for_titles.request_repaint();
                    }
                })
                .expect("Failed to spawn title bridge thread");

            let (bridged_button_tx, bridged_button_rx) = mpsc::channel();
            std::thread::Builder::new()
                .name("button-state-bridge".into())
                .spawn(move || {
                    while let Ok(msg) = button_rx.recv() {
                        let _ = bridged_button_tx.send(msg);
                        ctx.request_repaint();
                    }
                })
                .expect("Failed to spawn button-state bridge thread");

            let settings_state = SettingsState::new(&config);
            Ok(Box::new(Homeplayer {
                swipe_view: SwipeView::new(num_pages),
                config,
                player,
                music_store,
                title_rx: bridged_title_rx,
                button_state_rx: bridged_button_rx,
                is_playing: false,
                is_paused: false,
                is_muted: false,
                current_title: TitleChanged {
                    artist: String::new(),
                    album: String::new(),
                    title: "No track selected".to_string(),
                    cover: String::new(),
                },
                volume: initial_volume,
                pages,
                file_source_states,
                kids_file_source_states,
                cd_source_states,
                cd_toc_rx: None,
                tokio_rt,
                scanning: Arc::new(AtomicBool::new(false)),
                scan_completed_source: None,
                backgrounds: BackgroundImages::new(),
                cover_texture: None,
                cover_texture_path: String::new(),
                kids_cover_textures: HashMap::new(),
                station_textures: HashMap::new(),
                settings_state,
            }))
        }),
    )
}

// ---------------------------------------------------------------------------
// Background image loading
// ---------------------------------------------------------------------------

fn load_image_from_path(path: &Path) -> Option<ColorImage> {
    match image::open(path) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let pixels = rgba.into_raw();
            Some(ColorImage::from_rgba_unmultiplied(size, &pixels))
        }
        Err(e) => {
            warn!("Failed to load background image {}: {e}", path.display());
            None
        }
    }
}

struct BackgroundImages {
    music: Option<TextureHandle>,
    radio: Option<TextureHandle>,
    playing: Option<TextureHandle>,
    cd: Option<TextureHandle>,
    settings: Option<TextureHandle>,
    loaded: bool,
}

impl BackgroundImages {
    fn new() -> Self {
        Self {
            music: None,
            radio: None,
            playing: None,
            cd: None,
            settings: None,
            loaded: false,
        }
    }

    fn load_if_needed(&mut self, ctx: &egui::Context) {
        if self.loaded {
            return;
        }
        self.loaded = true;

        let images_dir = Path::new("images");
        let pairs: [(&str, &str); 5] = [
            ("music.jpg", "bg_music"),
            ("radio.jpg", "bg_radio"),
            ("playing.jpg", "bg_playing"),
            ("disc.jpg", "bg_cd"),
            ("settings.jpg", "bg_settings"),
        ];

        let mut textures: Vec<Option<TextureHandle>> = Vec::new();
        for (filename, tex_name) in &pairs {
            let path = images_dir.join(filename);
            let tex = load_image_from_path(&path)
                .map(|img| ctx.load_texture(*tex_name, img, TextureOptions::LINEAR));
            textures.push(tex);
        }

        self.music = textures.remove(0);
        self.radio = textures.remove(0);
        self.playing = textures.remove(0);
        self.cd = textures.remove(0);
        self.settings = textures.remove(0);

        info!("Background images loaded");
    }
}

// ---------------------------------------------------------------------------
// Dynamic page definitions
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum DynamicPage {
    Source(usize),
    NowPlaying,
    Settings,
}

fn page_label(page: &DynamicPage, config: &Config) -> String {
    match page {
        DynamicPage::Source(idx) => {
            let source = &config.sources[*idx];
            let icon = source_type_icon(&source.source_type);
            format!("{} {}", icon, source.name)
        }
        DynamicPage::NowPlaying => egui_i18n::tr!("page_playing"),
        DynamicPage::Settings => egui_i18n::tr!("page_settings"),
    }
}

// ---------------------------------------------------------------------------
// File source browsing state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BrowseMode {
    ByArtist,
    ByAlbum,
    ByTitle,
}

#[derive(Debug, Clone)]
pub(crate) enum BrowseLevel {
    Artists,
    Albums {
        artist_id: i32,
        artist_name: String,
    },
    Titles {
        artist_id: i32,
        artist_name: String,
        album_id: i32,
        album_name: String,
    },
    AllAlbums,
    TitlesForAlbum {
        album_id: i32,
        album_name: String,
    },
    AllTitles,
}

pub(crate) struct FileSourceState {
    pub source_id: Option<i32>,
    pub browse_mode: BrowseMode,
    pub browse_level: BrowseLevel,
    pub artists: Vec<MusicItem>,
    pub albums: Vec<MusicItem>,
    pub titles: Vec<MusicTitleItem>,
}

impl FileSourceState {
    fn new() -> Self {
        Self {
            source_id: None,
            browse_mode: BrowseMode::ByArtist,
            browse_level: BrowseLevel::Artists,
            artists: Vec::new(),
            albums: Vec::new(),
            titles: Vec::new(),
        }
    }
}

pub(crate) struct KidsFileSourceState {
    pub source_id: Option<i32>,
    pub albums: Vec<KidsAlbumItem>,
}

impl KidsFileSourceState {
    fn new() -> Self {
        Self {
            source_id: None,
            albums: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// UI actions collected during rendering
// ---------------------------------------------------------------------------

pub(crate) enum UiAction {
    PlayTitles {
        titles: Vec<MusicTitleItem>,
        start_index: usize,
    },
    PlayStream {
        url: String,
        icon: String,
    },
    BrowseAlbums {
        source_idx: usize,
        artist_id: i32,
        artist_name: String,
    },
    BrowseTitles {
        source_idx: usize,
        artist_id: i32,
        artist_name: String,
        album_id: i32,
        album_name: String,
    },
    BrowseAlbumTitles {
        source_idx: usize,
        album_id: i32,
        album_name: String,
    },

    ScanSource {
        source_idx: usize,
    },
    SwitchBrowseMode {
        source_idx: usize,
        mode: BrowseMode,
    },
    LoadCdToc {
        source_idx: usize,
    },
    PlayCd {
        source_idx: usize,
        start_track: usize,
    },
    EjectCd {
        source_idx: usize,
    },
    PlayKidsAlbum {
        source_idx: usize,
        album_id: i32,
    },
    PlayerPlay,
    PlayerPause,
    PlayerStop,
    PlayerNext,
    PlayerPrevious,
    PlayerVolume(f32),
    PlayerMute,
    SaveConfig {
        config: Config,
    },
    ResetSettings,
}

// ---------------------------------------------------------------------------
// Application state
// ---------------------------------------------------------------------------

struct Homeplayer {
    swipe_view: SwipeView,
    config: Config,
    player: RodioPlayer,
    music_store: Option<Arc<MusicStore>>,
    title_rx: Receiver<TitleChanged>,
    button_state_rx: Receiver<PlayerState>,
    is_playing: bool,
    is_paused: bool,
    is_muted: bool,
    current_title: TitleChanged,
    volume: f32,
    pages: Vec<DynamicPage>,
    file_source_states: HashMap<usize, FileSourceState>,
    kids_file_source_states: HashMap<usize, KidsFileSourceState>,
    cd_source_states: HashMap<usize, CdSourceState>,
    cd_toc_rx: Option<(
        usize,
        mpsc::Receiver<Result<rodio_player::cd_audio::CdInfo, anyhow::Error>>,
    )>,
    tokio_rt: tokio::runtime::Runtime,
    scanning: Arc<AtomicBool>,
    scan_completed_source: Option<usize>,
    backgrounds: BackgroundImages,
    cover_texture: Option<TextureHandle>,
    cover_texture_path: String,
    kids_cover_textures: HashMap<String, TextureHandle>,
    station_textures: HashMap<String, TextureHandle>,
    settings_state: SettingsState,
}

impl Homeplayer {
    /// Navigate the swipe view to the "Now Playing" page.
    fn navigate_to_now_playing(&mut self) {
        if let Some(idx) = self
            .pages
            .iter()
            .position(|p| matches!(p, DynamicPage::NowPlaying))
        {
            self.swipe_view.set_page(idx);
        }
    }

    fn drain_channels(&mut self) {
        // Drain title changes
        while let Ok(title) = self.title_rx.try_recv() {
            debug!("Title changed: {} - {}", title.artist, title.title);
            self.current_title = title;
        }

        // Poll for CD TOC read completion
        if let Some((source_idx, ref rx)) = self.cd_toc_rx {
            if let Ok(result) = rx.try_recv() {
                let idx = source_idx;
                self.cd_toc_rx = None;
                if let Some(state) = self.cd_source_states.get_mut(&idx) {
                    state.loading = false;
                    match result {
                        Ok(cd_info) => {
                            let audio_count = cd_info.audio_tracks().len();
                            info!(
                                "CD TOC loaded: {} tracks ({} audio)",
                                cd_info.tracks.len(),
                                audio_count
                            );
                            state.disc_present = true;
                            state.status = format!("{audio_count} audio tracks found.");
                            state.tracks = cd_info.tracks;
                        }
                        Err(e) => {
                            error!("Failed to read CD TOC: {e}");
                            state.disc_present = false;
                            state.tracks.clear();
                            state.status = format!("Failed to read disc: {e}");
                        }
                    }
                }
            }
        }

        // Drain button state changes
        while let Ok(state) = self.button_state_rx.try_recv() {
            match state {
                PlayerState::Playing => {
                    self.is_playing = true;
                    self.is_paused = false;
                }
                PlayerState::Paused => {
                    self.is_paused = true;
                }
                PlayerState::Stopped => {
                    self.is_playing = false;
                    self.is_paused = false;
                }
                PlayerState::StartPlaying => {
                    self.is_playing = true;
                    self.is_paused = false;
                }
                PlayerState::Muted => {
                    self.is_muted = true;
                }
                PlayerState::Unmuted => {
                    self.is_muted = false;
                }
                PlayerState::Seekable | PlayerState::Unseekable => {}
            }
        }
    }

    /// Rebuild pages, source states, and player configuration from the
    /// current `self.config`.  Called after saving a new configuration so
    /// that changes take effect immediately without a restart.
    fn apply_config(&mut self) {
        // ── 1. Audio device ────────────────────────────────────────────
        // Switch the audio output device if it changed.  The player
        // preserves its volume across the switch.
        self.player
            .switch_device(self.config.audio.device.as_deref());

        // ── 1b. Language ───────────────────────────────────────────────
        egui_i18n::set_language(&self.config.ui.language);

        // ── 2. Rebuild pages ───────────────────────────────────────────
        let mut pages: Vec<DynamicPage> = Vec::new();
        pages.push(DynamicPage::NowPlaying);
        for (i, _source) in self.config.sources.iter().enumerate() {
            pages.push(DynamicPage::Source(i));
        }
        if !self.config.ui.hide_settings {
            pages.push(DynamicPage::Settings);
        }
        self.pages = pages;
        self.swipe_view.set_num_pages(self.pages.len());

        // ── 3. Rebuild file source states ──────────────────────────────
        let mut file_source_states: HashMap<usize, FileSourceState> = HashMap::new();
        for (i, source) in self.config.sources.iter().enumerate() {
            if matches!(source.source_type, ConfigSourceType::File) {
                let mut state = FileSourceState::new();
                if let Some(ref store) = self.music_store
                    && let Ok(source_id) = store.get_source_id(&source.name)
                {
                    state.source_id = Some(source_id);
                    if let Ok(artists) = store.get_artists(source_id) {
                        state.artists = artists;
                    }
                }
                file_source_states.insert(i, state);
            }
        }
        self.file_source_states = file_source_states;

        // ── 4. Rebuild KidsFile source states ──────────────────────────
        let mut kids_file_source_states: HashMap<usize, KidsFileSourceState> = HashMap::new();
        for (i, source) in self.config.sources.iter().enumerate() {
            if matches!(source.source_type, ConfigSourceType::KidsFile) {
                let mut state = KidsFileSourceState::new();
                if let Some(ref store) = self.music_store
                    && let Ok(source_id) = store.get_source_id(&source.name)
                {
                    state.source_id = Some(source_id);
                    if let Ok(albums) = store.get_albums_with_artist(source_id) {
                        state.albums = albums;
                    }
                }
                kids_file_source_states.insert(i, state);
            }
        }
        self.kids_file_source_states = kids_file_source_states;

        // ── 5. Rebuild CD source states ────────────────────────────────
        let mut cd_source_states: HashMap<usize, CdSourceState> = HashMap::new();
        for (i, source) in self.config.sources.iter().enumerate() {
            if matches!(source.source_type, ConfigSourceType::CD) {
                cd_source_states.insert(i, CdSourceState::new());
            }
        }
        self.cd_source_states = cd_source_states;

        // ── 6. Clear cached textures so they are re-loaded ─────────────
        // Station icon textures may have changed (different sources or
        // edited stations), so drop them and let the lazy-loading in
        // update() re-create them.
        self.station_textures.clear();
        self.kids_cover_textures.clear();

        info!(
            "Configuration applied (pages: {}, file sources: {}, kids sources: {}, CD sources: {})",
            self.pages.len(),
            self.file_source_states.len(),
            self.kids_file_source_states.len(),
            self.cd_source_states.len()
        );
    }

    fn process_action(&mut self, action: UiAction) {
        match action {
            UiAction::PlayTitles {
                titles,
                start_index,
            } => {
                self.play_titles(titles, start_index);
            }
            UiAction::PlayStream { url, icon } => {
                self.play_stream(url, icon);
            }
            UiAction::BrowseAlbums {
                source_idx,
                artist_id,
                artist_name,
            } => {
                self.browse_albums(source_idx, artist_id, artist_name);
            }
            UiAction::BrowseTitles {
                source_idx,
                artist_id,
                artist_name,
                album_id,
                album_name,
            } => {
                self.browse_titles(source_idx, artist_id, artist_name, album_id, album_name);
            }
            UiAction::BrowseAlbumTitles {
                source_idx,
                album_id,
                album_name,
            } => {
                self.browse_album_titles(source_idx, album_id, album_name);
            }

            UiAction::ScanSource { source_idx } => {
                self.scan_source(source_idx);
            }
            UiAction::SwitchBrowseMode { source_idx, mode } => {
                self.switch_browse_mode(source_idx, mode);
            }
            UiAction::PlayerPlay => {
                self.play();
            }
            UiAction::PlayerPause => {
                self.player.pause();
            }
            UiAction::PlayerStop => {
                self.player.stop();
            }
            UiAction::PlayerNext => {
                self.player.skip_next();
            }
            UiAction::PlayerPrevious => {
                self.player.skip_previous();
            }
            UiAction::PlayerVolume(vol) => {
                self.player.set_volume(vol);
                self.is_muted = false;
            }
            UiAction::PlayerMute => {
                self.player.mute();
            }
            UiAction::LoadCdToc { source_idx } => {
                self.load_cd_toc(source_idx);
            }
            UiAction::PlayCd {
                source_idx,
                start_track,
            } => {
                self.play_cd(source_idx, start_track);
            }
            UiAction::EjectCd { source_idx } => {
                self.eject_cd(source_idx);
            }
            UiAction::PlayKidsAlbum {
                source_idx,
                album_id,
            } => {
                self.play_kids_album(source_idx, album_id);
            }
            UiAction::SaveConfig { config } => match config.save() {
                Ok(_) => {
                    info!("Configuration saved successfully");
                    self.config = config;
                    self.apply_config();
                    self.settings_state.config = self.config.clone();
                    self.settings_state.dirty = false;
                    self.settings_state.save_message =
                        Some((egui_i18n::tr!("config_saved_ok"), true));
                }
                Err(e) => {
                    error!("Failed to save configuration: {e}");
                    self.settings_state.save_message = Some((
                        format!("{}: {e}", egui_i18n::tr!("config_save_failed")),
                        false,
                    ));
                }
            },
            UiAction::ResetSettings => {
                self.settings_state.reset(&self.config);
            }
        }
    }

    fn eject_cd(&mut self, source_idx: usize) {
        let source = &self.config.sources[source_idx];
        let device = source.path.clone();
        self.player.stop();
        match rodio_player::cd_audio::eject_cd(&device) {
            Ok(_) => {
                info!("CD ejected");
                if let Some(state) = self.cd_source_states.get_mut(&source_idx) {
                    state.tracks.clear();
                    state.disc_present = false;
                    state.status = "Disc ejected. Insert a CD and press Refresh.".to_string();
                }
            }
            Err(e) => {
                error!("Failed to eject CD: {e}");
                if let Some(state) = self.cd_source_states.get_mut(&source_idx) {
                    state.status = format!("Eject failed: {e}");
                }
            }
        }
    }

    fn play_cd(&mut self, source_idx: usize, start_track: usize) {
        let source = &self.config.sources[source_idx];
        let device = source.path.clone();
        if let Some(state) = self.cd_source_states.get(&source_idx) {
            let tracks = state.tracks.clone();
            if let Err(e) = self.player.play_cd(&device, tracks, start_track) {
                error!("Failed to start CD playback: {e}");
            } else {
                self.navigate_to_now_playing();
            }
        }
    }

    fn load_cd_toc(&mut self, source_idx: usize) {
        let source = &self.config.sources[source_idx];
        let device = source.path.clone();
        if let Some(state) = self.cd_source_states.get_mut(&source_idx) {
            state.loading = true;
            state.status = "Reading disc…".to_string();
            state.tracks.clear();
        }
        // Read the TOC synchronously on a background thread so the UI
        // stays responsive.
        let (toc_tx, toc_rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = rodio_player::cd_audio::read_cd_toc(&device);
            let _ = toc_tx.send(result);
        });
        // We cannot block the UI thread, so we poll the result channel
        // in drain_channels.  Store the receiver for later polling.
        self.cd_toc_rx = Some((source_idx, toc_rx));
    }

    fn play(&mut self) {
        if self.is_paused {
            self.player.pause(); // toggles pause→play
        } else if !self.is_playing {
            // Nothing playing – start playback depending on the
            // current page type (file source or CD source).
            let current_page = self.swipe_view.current_page();
            if let Some(DynamicPage::Source(source_idx)) = self.pages.get(current_page) {
                let source_idx = *source_idx;
                let source_type = &self.config.sources[source_idx].source_type;
                match source_type {
                    ConfigSourceType::File => {
                        if let Some(state) = self.file_source_states.get(&source_idx)
                            && let Some(source_id) = state.source_id
                            && let Some(ref store) = self.music_store
                        {
                            let titles = match &state.browse_level {
                                BrowseLevel::Artists
                                | BrowseLevel::AllAlbums
                                | BrowseLevel::AllTitles => store.get_titles(source_id).ok(),
                                BrowseLevel::Albums { artist_id, .. } => {
                                    store.get_titles_by_artist(source_id, *artist_id).ok()
                                }
                                BrowseLevel::Titles {
                                    artist_id,
                                    album_id,
                                    ..
                                } => store
                                    .get_titles_by_artist_and_album(
                                        source_id, *artist_id, *album_id,
                                    )
                                    .ok(),
                                BrowseLevel::TitlesForAlbum { album_id, .. } => {
                                    store.get_titles_by_album(source_id, *album_id).ok()
                                }
                            };
                            if let Some(titles) = titles {
                                self.play_titles(titles, 0);
                            }
                        }
                    }
                    ConfigSourceType::CD => {
                        // Play all audio tracks from the beginning
                        if let Some(state) = self.cd_source_states.get(&source_idx) {
                            if !state.tracks.is_empty() {
                                self.process_action(UiAction::PlayCd {
                                    source_idx,
                                    start_track: 0,
                                });
                            }
                        }
                    }
                    ConfigSourceType::KidsFile => {
                        // Kids view: play all titles from the source
                        if let Some(state) = self.kids_file_source_states.get(&source_idx)
                            && let Some(source_id) = state.source_id
                            && let Some(ref store) = self.music_store
                        {
                            if let Ok(titles) = store.get_titles(source_id) {
                                self.play_titles(titles, 0);
                            }
                        }
                    }
                    ConfigSourceType::Stream => {
                        // Stream sources don't support generic "play all"
                    }
                }
            }
        }
    }

    fn switch_browse_mode(&mut self, source_idx: usize, mode: BrowseMode) {
        if let Some(state) = self.file_source_states.get_mut(&source_idx)
            && let Some(source_id) = state.source_id
            && let Some(ref store) = self.music_store
        {
            state.browse_mode = mode.clone();
            state.albums.clear();
            state.titles.clear();
            match mode {
                BrowseMode::ByArtist => {
                    state.browse_level = BrowseLevel::Artists;
                    if let Ok(artists) = store.get_artists(source_id) {
                        state.artists = artists;
                    }
                }
                BrowseMode::ByAlbum => {
                    state.browse_level = BrowseLevel::AllAlbums;
                    if let Ok(albums) = store.get_albums(source_id) {
                        state.albums = albums;
                    }
                }
                BrowseMode::ByTitle => {
                    state.browse_level = BrowseLevel::AllTitles;
                    if let Ok(titles) = store.get_titles(source_id) {
                        state.titles = titles;
                    }
                }
            }
        }
    }

    fn scan_source(&mut self, source_idx: usize) {
        let source = &self.config.sources[source_idx];
        if let Some(ref store) = self.music_store {
            let store = Arc::clone(store);
            let source_name = source.name.clone();
            let source_path = source.path.clone();
            let scanning = Arc::clone(&self.scanning);
            scanning.store(true, Ordering::SeqCst);
            self.scan_completed_source = None;
            let scan_source_idx = source_idx;
            std::thread::spawn(move || {
                info!("Starting scan of source '{source_name}' at '{source_path}'...");
                match store.update(&source_name, &source_path) {
                    Ok(_) => info!("Scan of '{source_name}' completed successfully"),
                    Err(e) => error!("Scan of '{source_name}' failed: {e}"),
                }
                scanning.store(false, Ordering::SeqCst);
            });
            self.scan_completed_source = Some(scan_source_idx);
        }
    }

    fn browse_album_titles(&mut self, source_idx: usize, album_id: i32, album_name: String) {
        if let Some(state) = self.file_source_states.get_mut(&source_idx)
            && let Some(source_id) = state.source_id
            && let Some(ref store) = self.music_store
        {
            match store.get_titles_by_album(source_id, album_id) {
                Ok(titles) => {
                    state.titles = titles;
                    state.browse_mode = BrowseMode::ByTitle;
                    state.browse_level = BrowseLevel::TitlesForAlbum {
                        album_id,
                        album_name,
                    };
                }
                Err(e) => error!("Failed to load titles for album: {e}"),
            }
        }
    }

    fn browse_titles(
        &mut self,
        source_idx: usize,
        artist_id: i32,
        artist_name: String,
        album_id: i32,
        album_name: String,
    ) {
        if let Some(state) = self.file_source_states.get_mut(&source_idx)
            && let Some(source_id) = state.source_id
            && let Some(ref store) = self.music_store
        {
            match store.get_titles_by_artist_and_album(source_id, artist_id, album_id) {
                Ok(titles) => {
                    state.titles = titles;
                    state.browse_mode = BrowseMode::ByTitle;
                    state.browse_level = BrowseLevel::Titles {
                        artist_id,
                        artist_name,
                        album_id,
                        album_name,
                    };
                }
                Err(e) => error!("Failed to load titles: {e}"),
            }
        }
    }

    fn browse_albums(&mut self, source_idx: usize, artist_id: i32, artist_name: String) {
        if let Some(state) = self.file_source_states.get_mut(&source_idx)
            && let Some(source_id) = state.source_id
            && let Some(ref store) = self.music_store
        {
            match store.get_albums_by_artist(source_id, artist_id) {
                Ok(albums) => {
                    state.albums = albums;
                    state.browse_mode = BrowseMode::ByAlbum;
                    state.browse_level = BrowseLevel::Albums {
                        artist_id,
                        artist_name,
                    };
                }
                Err(e) => error!("Failed to load albums: {e}"),
            }
        }
    }

    fn play_stream(&mut self, url: String, icon: String) {
        self.player.stop();
        self.player.clear();
        let player_clone = self.player.clone();
        self.tokio_rt.spawn(async move {
            if let Err(e) = player_clone.play_stream(&url, &icon).await {
                error!("Failed to play stream: {e}");
            }
        });
        self.navigate_to_now_playing();
    }

    fn play_kids_album(&mut self, source_idx: usize, album_id: i32) {
        if let Some(state) = self.kids_file_source_states.get(&source_idx)
            && let Some(source_id) = state.source_id
            && let Some(ref store) = self.music_store
        {
            match store.get_titles_by_album(source_id, album_id) {
                Ok(titles) => {
                    self.play_titles(titles, 0);
                }
                Err(e) => error!("Failed to load titles for kids album: {e}"),
            }
        }
    }

    fn play_titles(&mut self, titles: Vec<MusicTitleItem>, start_index: usize) {
        self.player.clear();
        let sound_items: Vec<SoundItem> = titles
            .iter()
            .map(|t| SoundItem {
                artist: t.artist.clone(),
                album: t.album.clone(),
                title: t.name.clone(),
                path: t.path.clone(),
                cover: t.cover.clone(),
            })
            .collect();

        // Skip to start_index by only appending from that index
        let items_to_play: Vec<SoundItem> = sound_items.into_iter().skip(start_index).collect();
        self.player.append(items_to_play);
        if let Err(e) = self.player.play() {
            error!("Failed to start playback: {e}");
        } else {
            self.navigate_to_now_playing();
        }
    }

    /// Reload data for a file source after a scan completes, respecting current browse mode.
    fn reload_file_source(&mut self, source_idx: usize) {
        let source = &self.config.sources[source_idx];

        // Handle KidsFile sources
        if matches!(source.source_type, ConfigSourceType::KidsFile) {
            if let Some(ref store) = self.music_store
                && let Some(state) = self.kids_file_source_states.get_mut(&source_idx)
            {
                if let Ok(source_id) = store.get_source_id(&source.name) {
                    state.source_id = Some(source_id);
                    if let Ok(albums) = store.get_albums_with_artist(source_id) {
                        state.albums = albums;
                    }
                }
            }
            return;
        }

        if let Some(ref store) = self.music_store
            && let Some(state) = self.file_source_states.get_mut(&source_idx)
        {
            if let Ok(source_id) = store.get_source_id(&source.name) {
                state.source_id = Some(source_id);
                match state.browse_mode {
                    BrowseMode::ByArtist => {
                        if let Ok(artists) = store.get_artists(source_id) {
                            state.artists = artists;
                        }
                        state.browse_level = BrowseLevel::Artists;
                    }
                    BrowseMode::ByAlbum => {
                        if let Ok(albums) = store.get_albums(source_id) {
                            state.albums = albums;
                        }
                        state.browse_level = BrowseLevel::AllAlbums;
                    }
                    BrowseMode::ByTitle => {
                        if let Ok(titles) = store.get_titles(source_id) {
                            state.titles = titles;
                        }
                        state.browse_level = BrowseLevel::AllTitles;
                    }
                }
            }
            match state.browse_mode {
                BrowseMode::ByArtist => {
                    state.albums.clear();
                    state.titles.clear();
                }
                BrowseMode::ByAlbum => {
                    state.artists.clear();
                    state.titles.clear();
                }
                BrowseMode::ByTitle => {
                    state.artists.clear();
                    state.albums.clear();
                }
            }
        }
    }
}

impl eframe::App for Homeplayer {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Load background images on first frame
        self.backgrounds.load_if_needed(ctx);

        // Drain player channels for state updates
        self.drain_channels();

        // Update cover texture if the cover path changed
        if self.current_title.cover != self.cover_texture_path {
            self.cover_texture_path = self.current_title.cover.clone();
            if !self.cover_texture_path.is_empty() {
                let cover_path = Path::new(&self.cover_texture_path);
                if cover_path.exists() {
                    self.cover_texture = load_image_from_path(cover_path)
                        .map(|img| ctx.load_texture("cover_art", img, TextureOptions::LINEAR));
                } else {
                    self.cover_texture = None;
                }
            } else {
                self.cover_texture = None;
            }
        }

        // Lazily load cover textures for KidsFile album covers
        for state in self.kids_file_source_states.values() {
            for album in &state.albums {
                if !album.cover.is_empty()
                    && !self.kids_cover_textures.contains_key(&album.cover)
                    && Path::new(&album.cover).exists()
                {
                    if let Some(img) = load_image_from_path(Path::new(&album.cover)) {
                        let tex = ctx.load_texture(
                            format!("kids_cover_{}", album.cover),
                            img,
                            TextureOptions::LINEAR,
                        );
                        self.kids_cover_textures.insert(album.cover.clone(), tex);
                    }
                }
            }
        }

        // Lazily load station icon textures
        for source in &self.config.sources {
            if source.source_type == ConfigSourceType::Stream {
                for station in &source.stations {
                    if !station.icon.is_empty()
                        && !self.station_textures.contains_key(&station.icon)
                        && Path::new(&station.icon).exists()
                    {
                        if let Some(img) = load_image_from_path(Path::new(&station.icon)) {
                            let tex = ctx.load_texture(
                                format!("station_{}", station.icon),
                                img,
                                TextureOptions::LINEAR,
                            );
                            self.station_textures.insert(station.icon.clone(), tex);
                        }
                    }
                }
            }
        }

        // If a scan just finished, reload the source data
        if !self.scanning.load(Ordering::SeqCst) {
            if let Some(source_idx) = self.scan_completed_source.take() {
                self.reload_file_source(source_idx);
            }
        }

        // Request repaint while playing or scanning for live updates
        if self.is_playing || self.scanning.load(Ordering::SeqCst) {
            ctx.request_repaint();
        }

        // Collect actions during rendering
        let mut actions: Vec<UiAction> = Vec::new();

        // --- Top panel: media player controls ---
        egui::TopBottomPanel::top("media_player_bar").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let button_size = egui::vec2(48.0, 48.0);

                // Previous track
                if ui.add_sized(button_size, egui::Button::new("⏮")).clicked() {
                    actions.push(UiAction::PlayerPrevious);
                }

                // Play / Pause
                let play_pause_label = if self.is_playing && !self.is_paused {
                    "⏸"
                } else {
                    "▶"
                };
                if ui
                    .add_sized(button_size, egui::Button::new(play_pause_label))
                    .clicked()
                {
                    if self.is_playing && !self.is_paused {
                        actions.push(UiAction::PlayerPause);
                    } else {
                        actions.push(UiAction::PlayerPlay);
                    }
                }

                // Stop
                if ui.add_sized(button_size, egui::Button::new("⏹")).clicked() {
                    actions.push(UiAction::PlayerStop);
                }

                // Next track
                if ui.add_sized(button_size, egui::Button::new("⏭")).clicked() {
                    actions.push(UiAction::PlayerNext);
                }

                ui.separator();

                // Track info
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Volume controls on the right (laid out first so they claim space)
                    ui.spacing_mut().slider_width = 150.0;
                    let mute_icon = if self.is_muted { "🔇" } else { "🔊" };
                    if ui
                        .add_sized(
                            egui::vec2(36.0, 36.0),
                            egui::Button::new(egui::RichText::new(mute_icon).size(20.0)),
                        )
                        .clicked()
                    {
                        actions.push(UiAction::PlayerMute);
                    }
                    let mut vol = self.volume;
                    let response = ui.add(egui::Slider::new(&mut vol, 0.0..=1.0).show_value(false));
                    if response.changed() {
                        self.volume = vol;
                        actions.push(UiAction::PlayerVolume(vol));
                    }

                    // Title text in remaining space (left-to-right, truncated)
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let status = if self.is_playing && !self.is_paused {
                            "▶"
                        } else if self.is_paused {
                            "⏸"
                        } else {
                            "⏹"
                        };

                        let title_text = if self.current_title.artist.is_empty() {
                            self.current_title.title.clone()
                        } else {
                            format!(
                                "{} - {}",
                                self.current_title.artist, self.current_title.title
                            )
                        };

                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(format!("{} {}", status, title_text)).strong(),
                            )
                            .truncate(),
                        );
                    });
                });
            });
            ui.add_space(4.0);
        });

        // --- Bottom panel: tab buttons ---
        egui::TopBottomPanel::bottom("tab_bar").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let available_width = ui.available_width();
                let tab_width = available_width / self.pages.len() as f32;
                let current = self.swipe_view.current_page();

                for (i, page) in self.pages.iter().enumerate() {
                    let is_selected = i == current;
                    let label = page_label(page, &self.config);
                    let text = egui::RichText::new(label);
                    let text = if is_selected {
                        text.strong()
                    } else {
                        text.weak()
                    };

                    let button = egui::Button::new(text)
                        .corner_radius(egui::CornerRadius {
                            nw: 4,
                            ne: 4,
                            sw: 0,
                            se: 0,
                        })
                        .fill(if is_selected {
                            ui.visuals().selection.bg_fill
                        } else {
                            egui::Color32::TRANSPARENT
                        });

                    let response = ui.add_sized(egui::vec2(tab_width - 4.0, 48.0), button);

                    if response.clicked() {
                        self.swipe_view.set_page(i);
                    }
                }
            });
            ui.add_space(2.0);
        });

        // --- Central panel: swipe view with page content ---
        // Pre-clone/copy data needed for rendering
        let pages = self.pages.clone();
        let config = self.config.clone();
        let current_title = self.current_title.clone();
        let is_playing = self.is_playing;
        let is_paused = self.is_paused;
        let volume = self.volume;
        let is_scanning = self.scanning.load(Ordering::SeqCst);

        // Pre-extract cover texture reference to avoid borrow conflict with swipe_view
        let cover_texture = self.cover_texture.clone();

        // Clone kids cover textures for rendering
        let kids_cover_textures = self.kids_cover_textures.clone();

        // Clone station textures for rendering
        let station_textures = self.station_textures.clone();

        // Clone file source state data for rendering
        let file_render_data: HashMap<usize, FileRenderData> = self
            .file_source_states
            .iter()
            .map(|(k, v)| {
                (
                    *k,
                    FileRenderData {
                        source_id: v.source_id,
                        browse_level: v.browse_level.clone(),
                        artists: v.artists.clone(),
                        albums: v.albums.clone(),
                        titles: v.titles.clone(),
                    },
                )
            })
            .collect();

        // Clone KidsFile source state data for rendering
        let kids_file_render_data: HashMap<usize, KidsFileRenderData> = self
            .kids_file_source_states
            .iter()
            .map(|(k, v)| {
                (
                    *k,
                    KidsFileRenderData {
                        source_id: v.source_id,
                        albums: v.albums.clone(),
                    },
                )
            })
            .collect();

        // Clone CD source states for rendering
        let cd_render_data: HashMap<usize, CdSourceState> = self.cd_source_states.clone();

        // Pre-build a lookup of which background texture to use for each page
        let bg_for_page: Vec<Option<egui::TextureId>> = pages
            .iter()
            .map(|page| match page {
                DynamicPage::Source(idx) => {
                    let source = &config.sources[*idx];
                    match source.source_type {
                        ConfigSourceType::File | ConfigSourceType::KidsFile => {
                            self.backgrounds.music.as_ref().map(|t| t.id())
                        }
                        ConfigSourceType::Stream => self.backgrounds.radio.as_ref().map(|t| t.id()),
                        ConfigSourceType::CD => self.backgrounds.cd.as_ref().map(|t| t.id()),
                    }
                }
                DynamicPage::NowPlaying => self.backgrounds.playing.as_ref().map(|t| t.id()),
                DynamicPage::Settings => self.backgrounds.settings.as_ref().map(|t| t.id()),
            })
            .collect();

        // Collect texture sizes for aspect-ratio-correct rendering
        let bg_sizes: Vec<Option<egui::Vec2>> = pages
            .iter()
            .map(|page| match page {
                DynamicPage::Source(idx) => {
                    let source = &config.sources[*idx];
                    match source.source_type {
                        ConfigSourceType::File | ConfigSourceType::KidsFile => {
                            self.backgrounds.music.as_ref().map(|t| t.size_vec2())
                        }
                        ConfigSourceType::Stream => {
                            self.backgrounds.radio.as_ref().map(|t| t.size_vec2())
                        }
                        ConfigSourceType::CD => self.backgrounds.cd.as_ref().map(|t| t.size_vec2()),
                    }
                }
                DynamicPage::NowPlaying => self.backgrounds.playing.as_ref().map(|t| t.size_vec2()),
                DynamicPage::Settings => self.backgrounds.settings.as_ref().map(|t| t.size_vec2()),
            })
            .collect();

        let settings_state = &mut self.settings_state;

        egui::CentralPanel::default().show(ctx, |ui| {
            self.swipe_view.show(
                ui,
                |painter, rect, page_idx| {
                    if page_idx >= bg_for_page.len() {
                        return;
                    }
                    if let (Some(tex_id), Some(img_size)) =
                        (bg_for_page[page_idx], bg_sizes[page_idx])
                    {
                        // "Cover" scaling: fill the rect while preserving aspect ratio
                        let rect_aspect = rect.width() / rect.height();
                        let img_aspect = img_size.x / img_size.y;

                        let uv_rect = if img_aspect > rect_aspect {
                            // Image is wider than rect — crop sides
                            let visible_fraction = rect_aspect / img_aspect;
                            let offset = (1.0 - visible_fraction) / 2.0;
                            egui::Rect::from_min_max(
                                egui::pos2(offset, 0.0),
                                egui::pos2(1.0 - offset, 1.0),
                            )
                        } else {
                            // Image is taller than rect — crop top/bottom
                            let visible_fraction = img_aspect / rect_aspect;
                            let offset = (1.0 - visible_fraction) / 2.0;
                            egui::Rect::from_min_max(
                                egui::pos2(0.0, offset),
                                egui::pos2(1.0, 1.0 - offset),
                            )
                        };

                        // Paint the background with reduced opacity so content remains readable
                        let tint = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 50);
                        painter.image(tex_id, rect, uv_rect, tint);
                    }
                },
                |ui, page_idx| {
                    if page_idx >= pages.len() {
                        return;
                    }
                    match &pages[page_idx] {
                        DynamicPage::Source(source_idx) => {
                            let source = &config.sources[*source_idx];
                            match source.source_type {
                                ConfigSourceType::File => {
                                    if let Some(data) = file_render_data.get(source_idx) {
                                        paint_file_source(
                                            ui,
                                            source,
                                            *source_idx,
                                            data,
                                            is_scanning,
                                            &mut actions,
                                        );
                                    }
                                }
                                ConfigSourceType::Stream => {
                                    paint_stream_source(
                                        ui,
                                        source,
                                        &station_textures,
                                        &mut actions,
                                    );
                                }
                                ConfigSourceType::KidsFile => {
                                    if let Some(data) = kids_file_render_data.get(source_idx) {
                                        paint_kids_file_source(
                                            ui,
                                            source,
                                            *source_idx,
                                            data,
                                            &kids_cover_textures,
                                            is_scanning,
                                            &mut actions,
                                        );
                                    }
                                }
                                ConfigSourceType::CD => {
                                    let cd_state = cd_render_data
                                        .get(source_idx)
                                        .cloned()
                                        .unwrap_or_else(CdSourceState::new);
                                    paint_cd_source(
                                        ui,
                                        source,
                                        *source_idx,
                                        &cd_state,
                                        &mut actions,
                                    );
                                }
                            }
                        }
                        DynamicPage::NowPlaying => {
                            paint_now_playing(
                                ui,
                                &current_title,
                                is_playing,
                                is_paused,
                                volume,
                                cover_texture.as_ref(),
                            );
                        }
                        DynamicPage::Settings => {
                            paint_settings(ui, settings_state, &mut actions);
                        }
                    }
                },
            );
        });

        // Process collected actions
        for action in actions {
            self.process_action(action);
        }
    }
}
