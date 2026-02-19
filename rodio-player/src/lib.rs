//! This crate is an abstraction layer for the audio library [rodio]. It is
//! made for audio player implementations like [homeplayer].
//!
//! So it provides
//! a play queue handling with common functionality like 'play', 'skip' etc.
//! but also informs about events like title changes and the current player
//! state. Currently there is support to play a (list) of files, Internet
//! streams like radio stations, and audio CDs.
//!
//! [rodio]: https://crates.io/crates/rodio
//! [homeplayer]: https://github.com/kayhannay/homeplayer

pub mod cd_audio;

use anyhow::Error;
use icy_metadata::{IcyHeaders, IcyMetadataReader, RequestIcyMetadata};
use rodio::cpal;
use rodio::cpal::traits::{DeviceTrait, HostTrait};
use rodio::{OutputStream, OutputStreamBuilder, Sink, Source};
use std::num::NonZeroUsize;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::spawn;
use std::time::Duration;
use stream_download::http::{HttpStream, reqwest::Client};
use stream_download::storage::bounded::BoundedStorageProvider;
use stream_download::storage::memory::MemoryStorageProvider;
use stream_download::{Settings, StreamDownload};
use tracing::{debug, error, info, warn};

/// Placeholder string used when no meaningful value is available (e.g. unknown
/// album or artist in stream metadata).
const UNKNOWN: &str = "-";

/// Data structure that is sent over the provided channel to inform
/// about the audio title that is currently played.
#[derive(Clone, Debug)]
pub struct TitleChanged {
    pub artist: String,
    pub album: String,
    pub title: String,
    pub cover: String,
}

/// This enum represents the different player states.
pub enum PlayerState {
    Playing,
    Paused,
    Stopped,
    Muted,
    Unmuted,
    Seekable,
    Unseekable,
    StartPlaying,
}

/// Data structure for a concrete file title which can be added
/// to the internal play list of the player.
#[derive(Clone, Debug)]
pub struct SoundItem {
    pub artist: String,
    pub album: String,
    pub title: String,
    pub path: String,
    pub cover: String,
}

/// The main struct, the player with all the functionality in it.
///
/// The sink and output stream are stored behind a double-`Arc` with a `Mutex`
/// in between (`Arc<Mutex<Arc<T>>>`).  This allows the active audio device to
/// be swapped at runtime via [`switch_device`](RodioPlayer::switch_device)
/// while spawned playback threads keep a clone of the *inner* `Arc<Sink>` and
/// continue to reference the (stopped) old sink until they exit naturally.
#[derive(Clone)]
pub struct RodioPlayer {
    sink: Arc<Mutex<Arc<Sink>>>,
    _stream: Arc<Mutex<Arc<OutputStream>>>,
    sound_queue: Arc<Mutex<Vec<SoundItem>>>,
    sound_queue_index: Arc<Mutex<usize>>,
    mute_volume: Arc<Mutex<f32>>,
    title_changed_sender: Sender<TitleChanged>,
    button_state_sender: Sender<PlayerState>,
}

/// Returns a list of names of available audio output devices.
///
/// The list always starts with a `"Default"` entry representing the
/// system's default output device.  The remaining entries are the names
/// reported by the OS audio back-end.
pub fn list_output_devices() -> Vec<String> {
    let mut names = vec!["Default".to_string()];
    if let Ok(devices) = cpal::default_host().output_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                names.push(name);
            }
        }
    }
    names
}

impl RodioPlayer {
    /// Creates a new `RodioPlayer`.
    ///
    /// When `device_name` is `None` (or `Some("Default")`), the system's
    /// default output device is used.  Otherwise the device whose name
    /// matches the given string is selected.  If the requested device
    /// cannot be found the default device is used as a fallback.
    pub fn new(
        title_changed_sender: Sender<TitleChanged>,
        button_state_sender: Sender<PlayerState>,
        device_name: Option<&str>,
    ) -> Self {
        let stream = open_output_stream(device_name);
        let sink = rodio::Sink::connect_new(stream.mixer());

        Self {
            sink: Arc::new(Mutex::new(Arc::new(sink))),
            _stream: Arc::new(Mutex::new(Arc::new(stream))),
            sound_queue: Arc::new(Mutex::new(Vec::new())),
            sound_queue_index: Arc::new(Mutex::new(0)),
            mute_volume: Arc::new(Mutex::new(0.0)),
            title_changed_sender,
            button_state_sender,
        }
    }

    /// Switch the audio output device at runtime.
    ///
    /// This stops any current playback, creates a new output stream for the
    /// requested device, and swaps the internal sink so that subsequent
    /// playback uses the new device.  The volume level is preserved across
    /// the switch.
    ///
    /// Spawned playback threads that still hold a reference to the old sink
    /// will see it in a stopped state and exit naturally.
    pub fn switch_device(&self, device_name: Option<&str>) {
        // Grab the current volume before stopping so we can restore it.
        let volume = {
            let sink = self.sink.lock().unwrap();
            sink.volume()
        };

        // Stop current playback (sets queue index to MAX so spawned threads
        // break out of their loop, and stops the sink so sleep_until_end
        // returns).
        self.stop();
        self.clear();

        // Create a new output stream and sink for the requested device.
        let stream = open_output_stream(device_name);
        let new_sink = Arc::new(rodio::Sink::connect_new(stream.mixer()));
        new_sink.set_volume(volume);

        // Swap the sink and stream.  Old values are dropped when the last
        // reference (held by any still-running spawned thread) goes away.
        *self.sink.lock().unwrap() = new_sink;
        *self._stream.lock().unwrap() = Arc::new(stream);

        info!(
            "Switched audio output device to {:?}",
            device_name.unwrap_or("Default")
        );
    }

    /// Clone the current inner `Arc<Sink>`.  Spawned threads should use this
    /// to obtain a handle that remains valid even if the device is switched.
    fn current_sink(&self) -> Arc<Sink> {
        self.sink.lock().unwrap().clone()
    }

    pub fn append(&self, mut sound_items: Vec<SoundItem>) {
        self.sound_queue.lock().unwrap().append(&mut sound_items);
    }

    pub fn play_cd(
        &self,
        device: &str,
        tracks: Vec<cd_audio::CdTrackInfo>,
        start_index: usize,
    ) -> Result<(), Error> {
        self.stop();
        self.clear();

        let player_sink = self.current_sink();
        let title_changed_sender = self.title_changed_sender.clone();
        let button_state_sender = self.button_state_sender.clone();
        let device = device.to_string();
        let queue_index = Arc::clone(&self.sound_queue_index);

        let _ = spawn(move || {
            if let Err(error) = start_cd_playback(
                player_sink,
                queue_index,
                &device,
                tracks,
                start_index,
                title_changed_sender,
                button_state_sender,
            ) {
                error!("Could not start CD playback: {error}");
            }
        });
        Ok(())
    }

    pub fn play(&self) -> Result<(), Error> {
        let player_sink = self.current_sink();
        let player_queue = Arc::clone(&self.sound_queue);
        let queue_index = Arc::clone(&self.sound_queue_index);
        let title_changed_sender = self.title_changed_sender.clone();
        let button_state_sender = self.button_state_sender.clone();
        let _ = spawn(move || {
            if let Err(error) = start_playback_queue(
                player_sink,
                player_queue,
                queue_index,
                title_changed_sender,
                button_state_sender,
            ) {
                error!("Could not start playback: {error}");
            }
        });
        Ok(())
    }

    pub async fn play_stream(&self, url: &str, icon: &str) -> Result<(), Error> {
        let client = Client::builder().request_icy_metadata().build()?;
        let stream = HttpStream::new(client, url.parse()?).await?;

        debug!("content type={:?}", stream.content_type());
        let bitrate: u64 = stream.header("Icy-Br").unwrap_or("256").parse()?;
        debug!("bitrate={bitrate}");

        let icy_headers = IcyHeaders::parse_from_headers(stream.headers());

        // buffer 5 seconds of audio
        // bitrate (in kilobits) / bits per byte * bytes per kilobyte * 5 seconds
        let prefetch_bytes = bitrate / 8 * 1024 * 5;
        debug!("prefetch bytes={prefetch_bytes}");

        let reader = StreamDownload::from_stream(
            stream,
            // use bounded storage to keep the underlying size from growing indefinitely
            BoundedStorageProvider::new(
                MemoryStorageProvider,
                // be liberal with the buffer size, you need to make sure it holds enough space to
                // prevent any out-of-bounds reads
                NonZeroUsize::new(512 * 1024).unwrap(),
            ),
            Settings::default().prefetch_bytes(prefetch_bytes),
        )
        .await?;

        let _ = self.start_stream_playback(reader, icy_headers, icon);

        Ok(())
    }

    /// Set up the ICY metadata reader and spawn a thread that drives playback
    /// of the given internet radio / audio stream.
    fn start_stream_playback(
        &self,
        reader: StreamDownload<BoundedStorageProvider<MemoryStorageProvider>>,
        icy_headers: IcyHeaders,
        icon: &str,
    ) -> Result<(), Error> {
        self.stop();
        let title_changed_sender = self.title_changed_sender.clone();
        let icon = icon.to_string();
        let stream_reader = IcyMetadataReader::new(
            reader,
            // Since we requested icy metadata, the metadata interval header should be present in the
            // response. This will allow us to parse the metadata within the stream
            icy_headers.metadata_interval(),
            // Parse stream metadata whenever we receive new values.
            move |metadata| {
                // ICY stream titles typically use the format "Artist - Title"
                // and some stations append extra info after a single-quote
                // (e.g. "Artist - Title'extra"). We split on "-" for
                // artist/title and strip anything after "'" from the title.
                let stream_title = match metadata {
                    Ok(meta) => meta.stream_title().unwrap_or(UNKNOWN).to_string(),
                    Err(e) => {
                        error!("Could not get music title from stream: {}", e);
                        UNKNOWN.to_string()
                    }
                };
                debug!("Update title: {}", &stream_title);
                let (artist, title) = stream_title
                    .split_once("-")
                    .unwrap_or((&stream_title, UNKNOWN));
                let (normalized_title, _) = title.split_once("'").unwrap_or((title, ""));
                let _ = title_changed_sender.send(TitleChanged {
                    title: normalized_title.trim().to_string(),
                    artist: artist.trim().to_string(),
                    album: UNKNOWN.to_string(),
                    cover: icon.clone(),
                });
            },
        );

        let player_sink = self.current_sink();
        let button_state_sender = self.button_state_sender.clone();
        let _ = spawn(move || {
            let _ = button_state_sender.send(PlayerState::Playing);
            let _ = button_state_sender.send(PlayerState::StartPlaying);
            let source = rodio::Decoder::new(stream_reader).unwrap();
            player_sink.append(source);
            debug!("Start Play now ...");
            player_sink.play();
            player_sink.sleep_until_end();
            debug!("Play finished ...");

            let _ = button_state_sender.send(PlayerState::Stopped);
        });
        Ok(())
    }

    pub fn stop(&self) {
        let mut idx = self.sound_queue_index.lock().unwrap();
        *idx = usize::MAX;
        drop(idx);
        let sink = self.current_sink();
        sink.stop();
        let _ = self.button_state_sender.send(PlayerState::Stopped);
    }

    pub fn pause(&self) {
        let sink = self.current_sink();
        debug!("Pause: {}", sink.is_paused());
        if sink.is_paused() {
            sink.play();
            let _ = self.button_state_sender.send(PlayerState::Playing);
        } else {
            sink.pause();
            let _ = self.button_state_sender.send(PlayerState::Paused);
        }
    }

    pub fn set_volume(&self, volume: f32) {
        self.current_sink().set_volume(volume);
    }

    pub fn get_volume(&self) -> f32 {
        self.current_sink().volume()
    }

    pub fn mute(&self) {
        let sink = self.current_sink();
        let mut mute_vol = self.mute_volume.lock().unwrap();
        if sink.volume() != 0.0 {
            *mute_vol = sink.volume();
            sink.set_volume(0.0);
            let _ = self.button_state_sender.send(PlayerState::Muted);
        } else {
            sink.set_volume(*mute_vol);
            *mute_vol = 0.0;
            let _ = self.button_state_sender.send(PlayerState::Unmuted);
        }
    }

    pub fn clear(&self) {
        self.sound_queue.lock().unwrap().clear();
        *self.sound_queue_index.lock().unwrap() = 0;
        self.current_sink().clear();
    }

    pub fn skip_next(&self) {
        self.current_sink().stop();
    }

    pub fn skip_previous(&self) {
        let mut idx = self.sound_queue_index.lock().unwrap();
        *idx = idx.saturating_sub(2);
        drop(idx);
        self.current_sink().stop();
    }

    pub fn forward(&self) {
        let s = self.current_sink();
        let _ = s.try_seek(s.get_pos() + Duration::from_secs(5));
    }

    pub fn rewind(&self) {
        let s = self.current_sink();
        if s.get_pos() > Duration::from_secs(5) {
            let result = s.try_seek(s.get_pos() - Duration::from_secs(5));
            if let Err(error) = result {
                debug!("Error when rewind: {error}");
            }
        }
    }
}

fn start_cd_playback(
    player_sink: Arc<Sink>,
    queue_index: Arc<Mutex<usize>>,
    device: &str,
    tracks: Vec<cd_audio::CdTrackInfo>,
    start_index: usize,
    title_changed_sender: Sender<TitleChanged>,
    button_state_sender: Sender<PlayerState>,
) -> Result<(), Error> {
    button_state_sender.send(PlayerState::Playing)?;
    button_state_sender.send(PlayerState::Seekable)?;
    button_state_sender.send(PlayerState::StartPlaying)?;

    // Reset the queue index so stop/skip controls work
    *queue_index.lock().unwrap() = start_index;

    let audio_tracks: Vec<&cd_audio::CdTrackInfo> = tracks.iter().filter(|t| t.is_audio).collect();

    loop {
        let idx = {
            let mut idx = queue_index.lock().unwrap();
            let current = *idx;
            if current >= audio_tracks.len() {
                break;
            }
            *idx = current + 1;
            current
        };

        let track = audio_tracks[idx];

        info!(
            "Playing CD track {} (LBA {}–{})",
            track.number, track.start_lba, track.end_lba
        );

        let _ = title_changed_sender.send(TitleChanged {
            artist: String::new(),
            album: "Audio CD".to_string(),
            title: format!("Track {}", track.number),
            cover: String::new(),
        });

        match cd_audio::open_track(device, track) {
            Ok(source) => {
                player_sink.append(source);
                debug!("Start CD track {} playback...", track.number);
                player_sink.play();
                player_sink.sleep_until_end();
                debug!("CD track {} finished", track.number);
            }
            Err(e) => {
                error!("Failed to open CD track {}: {e}", track.number);
            }
        }
    }

    button_state_sender.send(PlayerState::Stopped)?;
    button_state_sender.send(PlayerState::Unseekable)?;
    Ok(())
}

/// Open an [`OutputStream`] for the device identified by `device_name`.
///
/// When the name is `None`, empty, or `"Default"` the system default device
/// is used.  If a specific device cannot be found, falls back to the default.
fn open_output_stream(device_name: Option<&str>) -> OutputStream {
    let use_default = match device_name {
        None => true,
        Some(name) => name.is_empty() || name == "Default",
    };

    if !use_default {
        let requested = device_name.unwrap();
        if let Ok(devices) = cpal::default_host().output_devices() {
            for device in devices {
                if let Ok(name) = device.name() {
                    if name == requested {
                        match OutputStreamBuilder::from_device(device).and_then(|b| b.open_stream())
                        {
                            Ok(stream) => {
                                info!("Opened audio output device: {requested}");
                                return stream;
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to open audio device '{requested}': {e}, \
                                     falling back to default"
                                );
                            }
                        }
                        break;
                    }
                }
            }
        }
        warn!("Audio device '{requested}' not found, falling back to default");
    }

    OutputStreamBuilder::open_default_stream().expect("Failed to open default audio output stream")
}

fn start_playback_queue(
    player_sink: Arc<Sink>,
    player_queue: Arc<Mutex<Vec<SoundItem>>>,
    queue_index: Arc<Mutex<usize>>,
    title_changed_sender: Sender<TitleChanged>,
    button_state_sender: Sender<PlayerState>,
) -> Result<(), Error> {
    button_state_sender.send(PlayerState::Playing)?;
    button_state_sender.send(PlayerState::Seekable)?;
    button_state_sender.send(PlayerState::StartPlaying)?;

    loop {
        // Lock both the queue and the index in one critical section to
        // extract the next item (or break if we've reached the end).
        let sound_item = {
            let queue = player_queue.lock().unwrap();
            let mut idx = queue_index.lock().unwrap();
            if *idx >= queue.len() {
                break;
            }
            let item = queue[*idx].clone();
            *idx += 1;
            item
        };

        debug!("Change title: {}", &sound_item.title);
        let _ = title_changed_sender.send(TitleChanged {
            artist: sound_item.artist.clone(),
            album: sound_item.album.clone(),
            title: sound_item.title.clone(),
            cover: sound_item.cover.clone(),
        });
        debug!("Open file: {}", &sound_item.path);
        let file = std::fs::File::open(&sound_item.path)?;
        let metadata = file.metadata()?;
        let source = rodio::Decoder::builder()
            .with_seekable(true)
            .with_byte_len(metadata.len())
            .with_data(file)
            .build()?;
        let duration = source.total_duration().unwrap_or(Duration::from_secs(0));
        debug!("Duration: {:?}", duration);
        player_sink.append(source);
        debug!("Start Play now ...");
        player_sink.play();
        player_sink.sleep_until_end();
        debug!("Play finished ...");
    }

    button_state_sender.send(PlayerState::Stopped)?;
    button_state_sender.send(PlayerState::Unseekable)?;
    Ok(())
}
