//! Bluetooth audio sink: let phones stream audio to this device.
//!
//! The A2DP sink role itself (receiving and playing the audio) is provided by
//! the system audio server (PipeWire + WirePlumber).  This module handles the
//! rest:
//!
//! * a pairing mode (adapter discoverable/pairable for a limited time) with an
//!   agent that accepts incoming pairing requests,
//! * trusting paired phones so they can reconnect without pairing mode,
//! * tracking the connected phone and its AVRCP media player (status, track
//!   info) via BlueZ's D-Bus API, and
//! * forwarding play/pause/next/previous to the phone.
//!
//! Like the other managers, everything runs on the tokio runtime and results
//! are handed to the UI thread through an event buffer.

use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bluer::Address;
use bluer::agent::{Agent, ReqError};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, error, info, warn};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

/// How long the pairing mode stays active.
const PAIRING_DURATION: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// UUID of the A2DP source role, advertised by phones.
const A2DP_SOURCE_UUID: &str = "0000110a-0000-1000-8000-00805f9b34fb";

const BLUEZ: &str = "org.bluez";
const DEVICE_IFACE: &str = "org.bluez.Device1";
const PLAYER_IFACE: &str = "org.bluez.MediaPlayer1";
const TRANSPORT_IFACE: &str = "org.bluez.MediaTransport1";

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerStatus {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrackInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
}

/// A phone that is connected and can stream audio to us.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phone {
    pub address: String,
    pub name: String,
    pub status: PlayerStatus,
    pub track: TrackInfo,
    /// The phone exposes a media player (AVRCP), so it can be remote-controlled.
    pub controllable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneControl {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
}

impl PhoneControl {
    fn method(self) -> &'static str {
        match self {
            Self::Play => "Play",
            Self::Pause => "Pause",
            Self::Stop => "Stop",
            Self::Next => "Next",
            Self::Previous => "Previous",
        }
    }
}

#[derive(Debug)]
pub enum BtAudioCommand {
    StartPairing,
    StopPairing,
    Control(PhoneControl),
    Disconnect,
}

#[derive(Debug, Clone)]
pub enum BtAudioEvent {
    Unavailable(String),
    /// The phone currently used as audio source changed.
    Phone(Option<Phone>),
    /// Pairing mode: remaining time, or `None` when inactive.
    Pairing(Option<Duration>),
    Error(String),
}

// ---------------------------------------------------------------------------
// BtAudioManager
// ---------------------------------------------------------------------------

pub struct BtAudioManager {
    cmd_tx: tokio_mpsc::UnboundedSender<BtAudioCommand>,
    events: Arc<Mutex<Vec<BtAudioEvent>>>,
}

impl BtAudioManager {
    pub fn new(rt: &tokio::runtime::Runtime, ctx: egui::Context) -> Self {
        let (cmd_tx, cmd_rx) = tokio_mpsc::unbounded_channel();
        let events: Arc<Mutex<Vec<BtAudioEvent>>> = Arc::new(Mutex::new(Vec::new()));
        rt.spawn(bt_audio_task(cmd_rx, events.clone(), ctx));
        Self { cmd_tx, events }
    }

    pub fn send(&self, cmd: BtAudioCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn drain_events(&self) -> Vec<BtAudioEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

// ---------------------------------------------------------------------------
// D-Bus snapshot
// ---------------------------------------------------------------------------

type Props = HashMap<String, OwnedValue>;
type ManagedObjects = HashMap<OwnedObjectPath, HashMap<String, Props>>;

fn prop<T: TryFrom<OwnedValue>>(props: &Props, key: &str) -> Option<T> {
    props
        .get(key)
        .and_then(|v| v.try_clone().ok())
        .and_then(|v| T::try_from(v).ok())
}

struct Snapshot {
    phone: Option<Phone>,
    player_path: Option<OwnedObjectPath>,
    /// Addresses of all connected audio-source devices.
    connected: HashSet<String>,
    /// Connected, paired audio-source devices that are not yet trusted.
    untrusted: Vec<String>,
}

async fn snapshot(conn: &zbus::Connection) -> zbus::Result<Snapshot> {
    let proxy = zbus::fdo::ObjectManagerProxy::builder(conn)
        .destination(BLUEZ)?
        .path("/")?
        .build()
        .await?;
    let objects: ManagedObjects = proxy
        .get_managed_objects()
        .await?
        .into_iter()
        .map(|(path, ifaces)| {
            let ifaces = ifaces
                .into_iter()
                .map(|(name, props)| (name.to_string(), props))
                .collect();
            (path, ifaces)
        })
        .collect();

    // (device path, phone, has player)
    let mut candidates: Vec<(OwnedObjectPath, Phone)> = Vec::new();
    let mut connected = HashSet::new();
    let mut untrusted = Vec::new();

    for (path, ifaces) in &objects {
        let Some(dev) = ifaces.get(DEVICE_IFACE) else {
            continue;
        };
        if !prop::<bool>(dev, "Connected").unwrap_or(false) {
            continue;
        }
        let uuids: Vec<String> = prop(dev, "UUIDs").unwrap_or_default();
        if !uuids
            .iter()
            .any(|u| u.eq_ignore_ascii_case(A2DP_SOURCE_UUID))
        {
            continue;
        }
        let address: String = prop(dev, "Address").unwrap_or_default();
        let name: String = prop(dev, "Alias")
            .or_else(|| prop(dev, "Name"))
            .unwrap_or_else(|| address.clone());
        connected.insert(address.clone());
        if prop::<bool>(dev, "Paired").unwrap_or(false)
            && !prop::<bool>(dev, "Trusted").unwrap_or(false)
        {
            untrusted.push(address.clone());
        }
        candidates.push((
            path.clone(),
            Phone {
                address,
                name,
                status: PlayerStatus::Stopped,
                track: TrackInfo::default(),
                controllable: false,
            },
        ));
    }

    let mut player_paths: HashMap<String, OwnedObjectPath> = HashMap::new();
    for (dev_path, phone) in &mut candidates {
        let prefix = format!("{}/", dev_path.as_str());

        // Media player (AVRCP): status and track info.
        let player = objects
            .iter()
            .filter(|(p, _)| p.as_str().starts_with(&prefix))
            .find_map(|(p, ifaces)| ifaces.get(PLAYER_IFACE).map(|props| (p, props)));
        if let Some((player_path, props)) = player {
            phone.controllable = true;
            player_paths.insert(phone.address.clone(), player_path.clone());
            phone.status = match prop::<String>(props, "Status").as_deref() {
                Some("playing") | Some("forward-seek") | Some("reverse-seek") => {
                    PlayerStatus::Playing
                }
                Some("paused") => PlayerStatus::Paused,
                _ => PlayerStatus::Stopped,
            };
            if let Some(track) = prop::<Props>(props, "Track") {
                phone.track = TrackInfo {
                    title: prop(&track, "Title").unwrap_or_default(),
                    artist: prop(&track, "Artist").unwrap_or_default(),
                    album: prop(&track, "Album").unwrap_or_default(),
                };
            }
        }

        // Without a media player, an active transport still tells us that
        // audio is flowing.
        if phone.status != PlayerStatus::Playing {
            let active = objects.iter().any(|(p, ifaces)| {
                p.as_str().starts_with(&prefix)
                    && ifaces
                        .get(TRANSPORT_IFACE)
                        .and_then(|t| prop::<String>(t, "State"))
                        .is_some_and(|s| s == "active")
            });
            if active {
                phone.status = PlayerStatus::Playing;
            }
        }
    }

    // Prefer the phone that is playing, otherwise the first by name.
    candidates.sort_by(|a, b| {
        (a.1.status != PlayerStatus::Playing, &a.1.name)
            .cmp(&(b.1.status != PlayerStatus::Playing, &b.1.name))
    });
    let phone = candidates.into_iter().next().map(|(_, p)| p);
    let player_path = phone
        .as_ref()
        .and_then(|p| player_paths.get(&p.address).cloned());

    Ok(Snapshot {
        phone,
        player_path,
        connected,
        untrusted,
    })
}

// ---------------------------------------------------------------------------
// Background task
// ---------------------------------------------------------------------------

struct Pairing {
    deadline: Instant,
    /// Audio sources that were already connected when pairing mode started.
    connected_at_start: HashSet<String>,
    original_pairable: bool,
}

async fn bt_audio_task(
    mut cmd_rx: tokio_mpsc::UnboundedReceiver<BtAudioCommand>,
    events: Arc<Mutex<Vec<BtAudioEvent>>>,
    ctx: egui::Context,
) {
    let push = move |ev: BtAudioEvent| {
        if let Ok(mut g) = events.lock() {
            g.push(ev);
        }
        ctx.request_repaint();
    };

    let session = match bluer::Session::new().await {
        Ok(s) => s,
        Err(e) => {
            error!("Bluetooth audio: cannot connect to BlueZ: {e}");
            push(BtAudioEvent::Unavailable(e.to_string()));
            return;
        }
    };
    let adapter = match session.default_adapter().await {
        Ok(a) => a,
        Err(e) => {
            error!("Bluetooth audio: no adapter: {e}");
            push(BtAudioEvent::Unavailable(e.to_string()));
            return;
        }
    };
    let conn = match zbus::Connection::system().await {
        Ok(c) => c,
        Err(e) => {
            error!("Bluetooth audio: cannot connect to system bus: {e}");
            push(BtAudioEvent::Unavailable(e.to_string()));
            return;
        }
    };
    if let Err(e) = adapter.set_powered(true).await {
        warn!("Bluetooth audio: could not power adapter: {e}");
    }

    // The agent accepts incoming pairing requests.  Pairing is only possible
    // while the adapter is pairable, which is limited to the pairing mode.
    let agent = Agent {
        request_default: true,
        request_confirmation: Some(Box::new(|_| Box::pin(async { Ok(()) }))),
        request_authorization: Some(Box::new(|_| Box::pin(async { Ok(()) }))),
        authorize_service: Some(Box::new(|_| Box::pin(async { Ok(()) }))),
        request_pin_code: Some(Box::new(|_| {
            Box::pin(async { Ok::<_, ReqError>("0000".to_string()) })
        })),
        ..Default::default()
    };
    let _agent_handle = match session.register_agent(agent).await {
        Ok(h) => Some(h),
        Err(e) => {
            warn!("Bluetooth audio: could not register agent: {e}");
            None
        }
    };

    let mut pairing: Option<Pairing> = None;
    let mut last_phone: Option<Phone> = None;
    let mut player_path: Option<OwnedObjectPath> = None;
    let mut phone_address: Option<String> = None;
    let mut ticker = tokio::time::interval(POLL_INTERVAL);

    loop {
        tokio::select! {
            cmd = cmd_rx.recv() => {
                let Some(cmd) = cmd else { break };
                match cmd {
                    BtAudioCommand::StartPairing => {
                        if pairing.is_some() {
                            continue;
                        }
                        let secs = PAIRING_DURATION.as_secs() as u32;
                        let original_pairable = adapter.is_pairable().await.unwrap_or(true);
                        let res = async {
                            adapter.set_pairable_timeout(secs).await?;
                            adapter.set_pairable(true).await?;
                            adapter.set_discoverable_timeout(secs).await?;
                            adapter.set_discoverable(true).await
                        }
                        .await;
                        match res {
                            Ok(()) => {
                                info!("Bluetooth audio: pairing mode on");
                                let connected_at_start = snapshot(&conn)
                                    .await
                                    .map(|s| s.connected)
                                    .unwrap_or_default();
                                pairing = Some(Pairing {
                                    deadline: Instant::now() + PAIRING_DURATION,
                                    connected_at_start,
                                    original_pairable,
                                });
                                push(BtAudioEvent::Pairing(Some(PAIRING_DURATION)));
                            }
                            Err(e) => push(BtAudioEvent::Error(e.to_string())),
                        }
                    }
                    BtAudioCommand::StopPairing => {
                        if let Some(p) = pairing.take() {
                            end_pairing(&adapter, &p).await;
                            push(BtAudioEvent::Pairing(None));
                        }
                    }
                    BtAudioCommand::Control(control) => {
                        let Some(path) = &player_path else {
                            push(BtAudioEvent::Error("No media player on phone".into()));
                            continue;
                        };
                        debug!("Bluetooth audio: {}", control.method());
                        if let Err(e) = conn
                            .call_method(
                                Some(BLUEZ),
                                path.as_str(),
                                Some(PLAYER_IFACE),
                                control.method(),
                                &(),
                            )
                            .await
                        {
                            push(BtAudioEvent::Error(e.to_string()));
                        }
                    }
                    BtAudioCommand::Disconnect => {
                        let device = phone_address
                            .as_deref()
                            .and_then(|a| Address::from_str(a).ok())
                            .and_then(|a| adapter.device(a).ok());
                        if let Some(device) = device
                            && let Err(e) = device.disconnect().await
                        {
                            push(BtAudioEvent::Error(e.to_string()));
                        }
                    }
                }
            }
            _ = ticker.tick() => {
                let snap = match snapshot(&conn).await {
                    Ok(s) => s,
                    Err(e) => {
                        debug!("Bluetooth audio: snapshot failed: {e}");
                        continue;
                    }
                };

                // Trust paired phones so that they can reconnect later.
                for addr in &snap.untrusted {
                    if let Ok(a) = Address::from_str(addr)
                        && let Ok(dev) = adapter.device(a)
                        && let Err(e) = dev.set_trusted(true).await
                    {
                        warn!("Bluetooth audio: could not trust {addr}: {e}");
                    }
                }

                player_path = snap.player_path;
                phone_address = snap.phone.as_ref().map(|p| p.address.clone());
                if snap.phone != last_phone {
                    last_phone = snap.phone.clone();
                    push(BtAudioEvent::Phone(snap.phone));
                }

                if let Some(p) = &pairing {
                    let new_phone = snap
                        .connected
                        .iter()
                        .any(|a| !p.connected_at_start.contains(a));
                    let remaining = p.deadline.saturating_duration_since(Instant::now());
                    if new_phone || remaining.is_zero() {
                        let p = pairing.take().unwrap();
                        end_pairing(&adapter, &p).await;
                        push(BtAudioEvent::Pairing(None));
                    } else {
                        push(BtAudioEvent::Pairing(Some(remaining)));
                    }
                }
            }
        }
    }
}

async fn end_pairing(adapter: &bluer::Adapter, pairing: &Pairing) {
    info!("Bluetooth audio: pairing mode off");
    if let Err(e) = adapter.set_discoverable(false).await {
        warn!("Bluetooth audio: could not disable discoverable: {e}");
    }
    if let Err(e) = adapter.set_pairable(pairing.original_pairable).await {
        warn!("Bluetooth audio: could not restore pairable: {e}");
    }
}
