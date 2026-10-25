//! Wi-Fi management via NetworkManager (nmrs, D-Bus).
//!
//! Mirrors [`crate::bluetooth`]: all NetworkManager operations run on the
//! tokio runtime and results travel back to the UI thread through a shared
//! event buffer, so egui never blocks.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmrs::builders::WifiConnectionBuilder;
use nmrs::{ConnectionError, NetworkManager, WifiSecurity};
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    /// Signal strength, 0–100.
    pub strength: u8,
    pub secured: bool,
    /// A saved NetworkManager profile exists for this SSID.
    pub known: bool,
    pub active: bool,
}

/// Security type to use when connecting to a hidden network, where it cannot
/// be detected from a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenSecurity {
    Open,
    /// WPA/WPA2 personal.
    Wpa2,
    /// WPA3 personal (SAE).
    Wpa3,
}

impl HiddenSecurity {
    pub const ALL: [HiddenSecurity; 3] = [Self::Wpa2, Self::Wpa3, Self::Open];
}

#[derive(Debug)]
pub enum WifiCommand {
    /// Scan and refresh the network list.
    Refresh,
    /// Connect to a visible network. `password` is `None` for open networks
    /// and for known networks (the stored credentials are reused).
    Connect {
        ssid: String,
        password: Option<String>,
    },
    ConnectHidden {
        ssid: String,
        security: HiddenSecurity,
        password: String,
    },
    Disconnect,
    Forget(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WifiError {
    AuthFailed,
    NotFound,
    Timeout,
    DhcpFailed,
    NoDevice,
    Other(String),
}

impl From<ConnectionError> for WifiError {
    fn from(e: ConnectionError) -> Self {
        match e {
            ConnectionError::AuthFailed
            | ConnectionError::SupplicantConfigFailed
            | ConnectionError::MissingPassword => Self::AuthFailed,
            ConnectionError::NotFound => Self::NotFound,
            ConnectionError::Timeout
            | ConnectionError::SupplicantTimeout
            | ConnectionError::Stuck(_) => Self::Timeout,
            ConnectionError::DhcpFailed => Self::DhcpFailed,
            ConnectionError::NoWifiDevice | ConnectionError::WifiNotReady => Self::NoDevice,
            other => Self::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum WifiEvent {
    /// NetworkManager is not reachable.
    Unavailable(String),
    /// Fresh scan result.
    Networks {
        networks: Vec<WifiNetwork>,
        current: Option<String>,
    },
    /// A scan is running.
    Scanning(bool),
    /// A connection attempt to this SSID is in progress (`None` = finished).
    Connecting(Option<String>),
    /// An operation failed.
    Error(WifiError),
    /// A connection attempt succeeded.
    Connected(String),
}

// ---------------------------------------------------------------------------
// WifiManager
// ---------------------------------------------------------------------------

pub struct WifiManager {
    cmd_tx: tokio_mpsc::UnboundedSender<WifiCommand>,
    events: Arc<Mutex<Vec<WifiEvent>>>,
}

impl WifiManager {
    pub fn new(rt: &tokio::runtime::Runtime) -> Self {
        let (cmd_tx, cmd_rx) = tokio_mpsc::unbounded_channel();
        let events: Arc<Mutex<Vec<WifiEvent>>> = Arc::new(Mutex::new(Vec::new()));
        rt.spawn(wifi_task(cmd_rx, events.clone()));
        Self { cmd_tx, events }
    }

    pub fn send(&self, cmd: WifiCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub fn drain_events(&self) -> Vec<WifiEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

// ---------------------------------------------------------------------------
// Background task
// ---------------------------------------------------------------------------

async fn wifi_task(
    mut cmd_rx: tokio_mpsc::UnboundedReceiver<WifiCommand>,
    events: Arc<Mutex<Vec<WifiEvent>>>,
) {
    let push = move |ev: WifiEvent| {
        if let Ok(mut g) = events.lock() {
            g.push(ev);
        }
    };

    let nm = match NetworkManager::new().await {
        Ok(nm) => nm,
        Err(e) => {
            error!("Wi-Fi: cannot connect to NetworkManager: {e}");
            push(WifiEvent::Unavailable(e.to_string()));
            return;
        }
    };

    // Show already-visible networks right away, without forcing a new scan
    // (scanning can disturb audio streaming on small devices).
    publish_networks(&nm, &push).await;

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            WifiCommand::Refresh => {
                push(WifiEvent::Scanning(true));
                if let Err(e) = nm.scan_networks(None).await {
                    warn!("Wi-Fi: scan request failed: {e}");
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
                publish_networks(&nm, &push).await;
                push(WifiEvent::Scanning(false));
            }
            WifiCommand::Connect { ssid, password } => {
                push(WifiEvent::Connecting(Some(ssid.clone())));
                let security = match password {
                    Some(psk) => WifiSecurity::WpaPsk { psk },
                    None => WifiSecurity::Open,
                };
                // Known secured networks: an empty PSK means "use stored secret".
                let security = match security {
                    WifiSecurity::Open if is_known_secured(&nm, &ssid).await => {
                        WifiSecurity::WpaPsk { psk: String::new() }
                    }
                    other => other,
                };
                let result = nm.connect(&ssid, None, security).await;
                finish_connect(&nm, &push, &ssid, result.map_err(Into::into)).await;
            }
            WifiCommand::ConnectHidden {
                ssid,
                security,
                password,
            } => {
                push(WifiEvent::Connecting(Some(ssid.clone())));
                let builder = WifiConnectionBuilder::new(ssid.clone()).hidden(true);
                let builder = match security {
                    HiddenSecurity::Open => builder.open(),
                    HiddenSecurity::Wpa2 => builder.wpa_psk(password),
                    HiddenSecurity::Wpa3 => builder.sae(password),
                };
                let settings = builder.ipv4_auto().ipv6_auto().build();
                let result = nm
                    .add_and_activate_connection(settings, None, None)
                    .await
                    .map(|_| ())
                    .map_err(WifiError::from);
                if result.is_err() {
                    // Do not leave a broken profile behind.
                    let _ = nm.forget(&ssid).await;
                }
                finish_connect(&nm, &push, &ssid, result).await;
            }
            WifiCommand::Disconnect => {
                if let Err(e) = nm.disconnect(None).await {
                    push(WifiEvent::Error(e.into()));
                }
                publish_networks(&nm, &push).await;
            }
            WifiCommand::Forget(ssid) => {
                if let Err(e) = nm.forget(&ssid).await {
                    push(WifiEvent::Error(e.into()));
                }
                publish_networks(&nm, &push).await;
            }
        }
    }
}

async fn is_known_secured(nm: &NetworkManager, ssid: &str) -> bool {
    nm.has_saved_connection(ssid).await.unwrap_or(false)
        && nm
            .list_networks(None)
            .await
            .map(|n| n.iter().any(|n| n.ssid == ssid && n.secured))
            .unwrap_or(false)
}

async fn finish_connect(
    nm: &NetworkManager,
    push: &impl Fn(WifiEvent),
    ssid: &str,
    result: Result<(), WifiError>,
) {
    match result {
        Ok(()) => {
            info!("Wi-Fi: connected to {ssid}");
            push(WifiEvent::Connected(ssid.to_string()));
        }
        Err(e) => {
            warn!("Wi-Fi: connecting to {ssid} failed: {e:?}");
            push(WifiEvent::Error(e));
        }
    }
    push(WifiEvent::Connecting(None));
    publish_networks(nm, push).await;
}

async fn publish_networks(nm: &NetworkManager, push: &impl Fn(WifiEvent)) {
    let list = match nm.list_networks(None).await {
        Ok(l) => l,
        Err(e) => {
            push(WifiEvent::Error(e.into()));
            return;
        }
    };

    let mut networks: Vec<WifiNetwork> = Vec::new();
    for n in list {
        // Enterprise networks and hotspots are not supported; hidden APs have
        // no usable name.
        if n.is_eap || n.is_hotspot || n.ssid.is_empty() || n.ssid == "<Hidden Network>" {
            continue;
        }
        let candidate = WifiNetwork {
            ssid: n.ssid,
            strength: n.strength.unwrap_or(0),
            secured: n.secured,
            known: n.known,
            active: n.is_active,
        };
        match networks.iter_mut().find(|e| e.ssid == candidate.ssid) {
            Some(existing) => {
                existing.strength = existing.strength.max(candidate.strength);
                existing.known |= candidate.known;
                existing.active |= candidate.active;
            }
            None => networks.push(candidate),
        }
    }
    networks.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.strength.cmp(&a.strength))
            .then(a.ssid.cmp(&b.ssid))
    });

    let current = nm.current_ssid().await;
    push(WifiEvent::Networks { networks, current });
}
