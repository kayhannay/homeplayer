//! This crate is an abstraction layer for the audio library [rodio]. It is
//! made for audio player implementations like [homeplayer].
//!
//! So it provides
//! a play queue handling with common functionality like 'play', 'skip' etc.
//! but also informs about events like title changes and the current player
//! state. Currently there is support to play a (list) of files or Internet
//! streams like radio stations.
//!
//! [rodio]: https://crates.io/crates/rodio
//! [homeplayer]: https://github.com/kayhannay/homeplayer

use anyhow::Error;
use icy_metadata::{IcyHeaders, IcyMetadataReader, RequestIcyMetadata};
use rodio::{OutputStream, Sink, Source};
use std::num::NonZeroUsize;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::spawn;
use std::time::Duration;
use stream_download::http::{HttpStream, reqwest::Client};
use stream_download::storage::bounded::BoundedStorageProvider;
use stream_download::storage::memory::MemoryStorageProvider;
use stream_download::{Settings, StreamDownload};
use tracing::{debug, error};

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
#[derive(Clone)]
pub struct RodioPlayer {
    sink: Arc<Sink>,
    _stream: Arc<OutputStream>,
    sound_queue: Arc<Mutex<Vec<SoundItem>>>,
    sound_queue_index: Arc<Mutex<usize>>,
    mute_volume: f32,
    title_changed_sender: Sender<TitleChanged>,
    button_state_sender: Sender<PlayerState>,
}

unsafe impl Send for RodioPlayer {}

impl RodioPlayer {
    pub fn new(
        title_changed_sender: Sender<TitleChanged>,
        button_state_sender: Sender<PlayerState>,
    ) -> Self {
        let stream = rodio::OutputStreamBuilder::open_default_stream().unwrap();
        let sink = rodio::Sink::connect_new(stream.mixer());
        let wrapped_sink = Arc::new(sink);

        Self {
            sink: wrapped_sink,
            _stream: Arc::new(stream),
            sound_queue: Arc::new(Mutex::new(Vec::new())),
            sound_queue_index: Arc::new(Mutex::new(0)),
            mute_volume: 0.0,
            title_changed_sender,
            button_state_sender,
        }
    }

    pub fn append(&mut self, mut sound_items: Vec<SoundItem>) {
        self.sound_queue.lock().unwrap().append(&mut sound_items);
    }

    pub fn play(&mut self) -> Result<(), Error> {
        let player_sink = self.sink.clone();
        let player_queue = Arc::clone(&self.sound_queue);
        let queue_index = Arc::clone(&self.sound_queue_index);
        let title_changed_sender = self.title_changed_sender.clone();
        let button_state_sender = self.button_state_sender.clone();
        let _ = spawn(move || {
            match start_playback_queue(
                player_sink,
                player_queue,
                queue_index,
                title_changed_sender,
                button_state_sender,
            ) {
                Ok(_) => (),
                Err(error) => error!("Could not start playback: {error}"),
            };
        });
        Ok(())
    }

    pub async fn play_stream(&mut self, url: &str, icon: &str) -> Result<(), Error> {
        let client = Client::builder().request_icy_metadata().build()?;
        let stream = HttpStream::new(client, url.parse()?).await?;

        debug!("content type={:?}", stream.content_type());
        let bitrate: u64 = stream.header("Icy-Br").unwrap_or("256").parse()?;
        debug!("bitrate={bitrate}");

        //println!("Headers:");
        // stream.headers().iter().for_each(|(k, v)| {
        //     println!("{}={:?}", k, v);
        // });
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

        //let sender = title_changed_sender.unwrap().clone();

        let _ = self._play_stream(reader, icy_headers, icon);

        Ok(())
    }

    fn _play_stream(
        &mut self,
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
            // Print the stream metadata whenever we receive new values
            move |metadata| {
                //println!("{metadata:#?}\n");

                let mut stream_title = "-".to_string();
                match metadata {
                    Ok(meta) => {
                        stream_title = meta.stream_title().unwrap().to_string();
                    }
                    Err(e) => {
                        error!("Could not get music title from stream: {}", e);
                    }
                }
                debug!("Update title: {}", &stream_title);
                let (artist, title) = stream_title.split_once("-").unwrap_or((&stream_title, "-"));
                let (normalized_title, _) = title.split_once("'").unwrap_or((title, ""));
                let _ = title_changed_sender.send(TitleChanged {
                    title: normalized_title.trim().to_string(),
                    artist: artist.trim().to_string(),
                    album: "-".to_string(),
                    cover: icon.clone(),
                });
            },
        );

        let player_sink = self.sink.clone();
        let button_state_sender = self.button_state_sender.clone();
        let _ = spawn(move || {
            let _ = button_state_sender.send(PlayerState::Playing);
            let _ = button_state_sender.send(PlayerState::StartPlaying);
            let source = rodio::Decoder::new(stream_reader).unwrap();
            //let duration = source.total_duration().unwrap();
            //println!("Duration: {:?}", duration);
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
        *self.sound_queue_index.lock().unwrap() = self.sound_queue.lock().unwrap().len();
        self.sink.stop();
        let _ = self.button_state_sender.send(PlayerState::Stopped);
    }

    pub fn pause(&self) {
        debug!("Pause: {}", self.sink.is_paused());
        match self.sink.is_paused() {
            true => {
                self.sink.play();
                let _ = self.button_state_sender.send(PlayerState::Playing);
            }
            false => {
                self.sink.pause();
                let _ = self.button_state_sender.send(PlayerState::Paused);
            }
        }
    }

    pub fn volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }

    pub fn get_volume(&self) -> f32 {
        self.sink.volume()
    }

    pub fn mute(&mut self) {
        if self.sink.volume() != 0.0 {
            self.mute_volume = self.sink.volume();
            self.sink.set_volume(0.0);
            let _ = self.button_state_sender.send(PlayerState::Muted);
        } else {
            self.sink.set_volume(self.mute_volume);
            self.mute_volume = 0.0;
            let _ = self.button_state_sender.send(PlayerState::Unmuted);
        }
    }

    pub fn clear(&self) {
        self.sound_queue.lock().unwrap().clear();
        *self.sound_queue_index.lock().unwrap() = 0;
        self.sink.clear();
    }

    pub fn skip_next(&self) {
        self.sink.stop();
    }

    pub fn skip_previous(&self) {
        if *self.sound_queue_index.lock().unwrap() > 0 {
            *self.sound_queue_index.lock().unwrap() -= 2;
        }
        self.sink.stop();
    }

    pub fn forward(&self) {
        let s = &self.sink;
        let _ = s.try_seek(s.get_pos() + Duration::from_secs(5));
    }

    pub fn rewind(&self) {
        let s = &self.sink;
        if s.get_pos() > Duration::from_secs(5) {
            let result = s.try_seek(s.get_pos() - Duration::from_secs(5));
            if let Err(error) = result {
                debug!("Error when rewind: {error}");
            }
        }
    }
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
    while player_queue.lock().unwrap().len() > *queue_index.lock().unwrap() {
        let sound_item = player_queue
            .lock()
            .unwrap()
            .get(*queue_index.lock().unwrap())
            .unwrap()
            .clone();
        *queue_index.lock().unwrap() += 1;
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
