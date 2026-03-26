//! Bluetooth device management via BlueZ (bluer).
//!
//! This module runs all BlueZ operations asynchronously on the tokio runtime
//! and communicates results back to the UI thread through plain `std::sync`
//! channels so that egui never blocks.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use bluer::{Adapter, AdapterEvent, Address, Device};
use futures::StreamExt as _;
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{error, info, warn};

// ---------------------------------------------------------------------------
// Public data types
// ---------------------------------------------------------------------------

/// The connection / pairing status of a discovered device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceStatus {
    /// Discovered but not paired.
    Discovered,
    /// Currently being paired or connected.
    Connecting,
    /// Paired and connected.
    Connected,
    /// Paired but not currently connected.
    Paired,
    /// A pairing / connection attempt failed.
    Failed(String),
    /// Actively being disconnected / unpaired.
    Disconnecting,
}

impl fmt::Display for DeviceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceStatus::Discovered => write!(f, "Discovered"),
            DeviceStatus::Connecting => write!(f, "Connecting…"),
            DeviceStatus::Connected => write!(f, "Connected"),
            DeviceStatus::Paired => write!(f, "Paired"),
            DeviceStatus::Failed(msg) => write!(f, "Failed: {msg}"),
            DeviceStatus::Disconnecting => write!(f, "Disconnecting…"),
        }
    }
}

/// Information about a single Bluetooth device visible to BlueZ.
#[derive(Debug, Clone)]
pub struct BluetoothDevice {
    pub address: Address,
    pub name: String,
    /// Whether the device advertises an Audio Sink profile (A2DP sink ⇒
    /// Bluetooth speaker / headphones).
    pub is_audio_sink: bool,
    pub status: DeviceStatus,
    pub rssi: Option<i16>,
}

impl BluetoothDevice {
    /// A short icon string shown next to the device in the UI.
    pub fn icon(&self) -> &'static str {
        if self.is_audio_sink { "🔊" } else { "📶" }
    }
}

// ---------------------------------------------------------------------------
// Commands sent *to* the background task
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum BluetoothCommand {
    StartScan,
    StopScan,
    Connect(Address),
    Disconnect(Address),
    Remove(Address),
}

// ---------------------------------------------------------------------------
// Events sent *from* the background task back to the UI
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum BluetoothEvent {
    /// The adapter powered on / off state changed.
    AdapterPowered(bool),
    /// Discovery is now running / stopped.
    ScanningChanged(bool),
    /// A device was added or its properties changed.
    DeviceUpdated(BluetoothDevice),
    /// A device was removed (forgotten by BlueZ or went out of range and was
    /// cleaned up).
    DeviceRemoved(Address),
    /// Status change for a specific device (e.g. "Connecting…").
    DeviceStatus(Address, DeviceStatus),
    /// A human-readable error string (shown as a transient message in the UI).
    Error(String),
}

// ---------------------------------------------------------------------------
// BluetoothManager
// ---------------------------------------------------------------------------

/// Owned handle to the background Bluetooth task.
///
/// Create one instance, keep it alive for as long as Bluetooth functionality
/// is needed, then drop it to stop the background task.
pub struct BluetoothManager {
    cmd_tx: tokio_mpsc::UnboundedSender<BluetoothCommand>,
    /// Events produced by the background task, collected here so that the UI
    /// can drain them on every frame without blocking.
    pub events: Arc<Mutex<Vec<BluetoothEvent>>>,
}

impl BluetoothManager {
    /// Spawn the background task on `rt` and return a handle.
    pub fn new(rt: &tokio::runtime::Runtime) -> Self {
        let (cmd_tx, cmd_rx) = tokio_mpsc::unbounded_channel();
        let events: Arc<Mutex<Vec<BluetoothEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = events.clone();

        rt.spawn(bluetooth_task(cmd_rx, events_clone));

        Self { cmd_tx, events }
    }

    /// Send a command to the background task (fire-and-forget).
    pub fn send(&self, cmd: BluetoothCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// Drain all pending events.  The caller gets ownership of the events and
    /// the internal buffer is cleared.
    pub fn drain_events(&self) -> Vec<BluetoothEvent> {
        let mut guard = self.events.lock().unwrap();
        std::mem::take(&mut *guard)
    }
}

// ---------------------------------------------------------------------------
// Background async task
// ---------------------------------------------------------------------------

/// UUID for Audio Sink (A2DP sink – Bluetooth speakers / headphones).
const AUDIO_SINK_UUID: &str = "0000110b-0000-1000-8000-00805f9b34fb";

async fn bluetooth_task(
    mut cmd_rx: tokio_mpsc::UnboundedReceiver<BluetoothCommand>,
    events: Arc<Mutex<Vec<BluetoothEvent>>>,
) {
    let push = {
        let events = events.clone();
        move |ev: BluetoothEvent| {
            if let Ok(mut g) = events.lock() {
                g.push(ev);
            }
        }
    };

    // Initialise bluer session.
    let session = match bluer::Session::new().await {
        Ok(s) => s,
        Err(e) => {
            error!("Bluetooth: failed to create bluer session: {e}");
            push(BluetoothEvent::Error(format!(
                "Cannot connect to BlueZ: {e}"
            )));
            return;
        }
    };

    let adapter = match session.default_adapter().await {
        Ok(a) => a,
        Err(e) => {
            error!("Bluetooth: no adapter available: {e}");
            push(BluetoothEvent::Error(format!(
                "No Bluetooth adapter found: {e}"
            )));
            return;
        }
    };

    // Make sure the adapter is powered on.
    if let Err(e) = adapter.set_powered(true).await {
        warn!("Bluetooth: could not power adapter: {e}");
    }
    let powered = adapter.is_powered().await.unwrap_or(false);
    push(BluetoothEvent::AdapterPowered(powered));

    // Load already-known devices immediately so the UI is not empty on first open.
    refresh_known_devices(&adapter, &events).await;

    // Main event loop.
    loop {
        let cmd = match cmd_rx.recv().await {
            Some(c) => c,
            None => break, // channel closed → manager dropped
        };

        match cmd {
            BluetoothCommand::StartScan => {
                info!("Bluetooth: starting discovery");
                match adapter.discover_devices().await {
                    Ok(mut stream) => {
                        push(BluetoothEvent::ScanningChanged(true));

                        // Drain discovery events until we receive a StopScan.
                        loop {
                            tokio::select! {
                                maybe_event = stream.next() => {
                                    match maybe_event {
                                        Some(AdapterEvent::DeviceAdded(addr)) => {
                                            if let Ok(dev) = adapter.device(addr)
                                                && let Some(bt_dev) = build_bt_device(&dev).await
                                            {
                                                push(BluetoothEvent::DeviceUpdated(bt_dev));
                                            }
                                        }
                                        Some(AdapterEvent::DeviceRemoved(addr)) => {
                                            push(BluetoothEvent::DeviceRemoved(addr));
                                        }
                                        Some(AdapterEvent::PropertyChanged(_)) => {
                                            // Adapter property changed; ignore for now.
                                        }
                                        None => break, // stream ended unexpectedly
                                    }
                                }
                                next_cmd = cmd_rx.recv() => {
                                    match next_cmd {
                                        Some(BluetoothCommand::StopScan) | None => break,
                                        Some(other) => {
                                            handle_device_cmd(other, &adapter, &events).await;
                                        }
                                    }
                                }
                            }
                        }

                        // `stream` is dropped here which stops discovery automatically.
                        push(BluetoothEvent::ScanningChanged(false));
                        // Refresh the known-device list one more time after scan.
                        refresh_known_devices(&adapter, &events).await;
                    }
                    Err(e) => {
                        error!("Bluetooth: discover_devices failed: {e}");
                        push(BluetoothEvent::Error(format!("Scan failed: {e}")));
                    }
                }
            }

            other => {
                handle_device_cmd(other, &adapter, &events).await;
            }
        }
    }

    info!("Bluetooth task exiting");
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a [`BluetoothDevice`] by querying the device's properties.
async fn build_bt_device(dev: &Device) -> Option<BluetoothDevice> {
    let address = dev.address();

    let name = dev
        .alias()
        .await
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| address.to_string());

    let uuids: Vec<String> = dev
        .uuids()
        .await
        .ok()
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .map(|u| u.to_string().to_lowercase())
        .collect();

    let is_audio_sink = uuids.iter().any(|u| u == AUDIO_SINK_UUID);

    let paired = dev.is_paired().await.unwrap_or(false);
    let connected = dev.is_connected().await.unwrap_or(false);
    let rssi = dev.rssi().await.ok().flatten();

    let status = if connected {
        DeviceStatus::Connected
    } else if paired {
        DeviceStatus::Paired
    } else {
        DeviceStatus::Discovered
    };

    Some(BluetoothDevice {
        address,
        name,
        is_audio_sink,
        status,
        rssi,
    })
}

/// Re-read all devices the adapter already knows about and push update events.
async fn refresh_known_devices(adapter: &Adapter, events: &Arc<Mutex<Vec<BluetoothEvent>>>) {
    let push = |ev: BluetoothEvent| {
        if let Ok(mut g) = events.lock() {
            g.push(ev);
        }
    };

    match adapter.device_addresses().await {
        Ok(addrs) => {
            for addr in addrs {
                if let Ok(dev) = adapter.device(addr)
                    && let Some(bt_dev) = build_bt_device(&dev).await
                {
                    push(BluetoothEvent::DeviceUpdated(bt_dev));
                }
            }
        }
        Err(e) => {
            warn!("Bluetooth: could not list known devices: {e}");
        }
    }
}

/// Handle a connect / disconnect / remove command.
async fn handle_device_cmd(
    cmd: BluetoothCommand,
    adapter: &Adapter,
    events: &Arc<Mutex<Vec<BluetoothEvent>>>,
) {
    let push = |ev: BluetoothEvent| {
        if let Ok(mut g) = events.lock() {
            g.push(ev);
        }
    };

    match cmd {
        BluetoothCommand::Connect(addr) => {
            info!("Bluetooth: connecting to {addr}");
            push(BluetoothEvent::DeviceStatus(addr, DeviceStatus::Connecting));
            match adapter.device(addr) {
                Ok(dev) => {
                    // Pair first if not yet paired.
                    let paired = dev.is_paired().await.unwrap_or(false);
                    if !paired {
                        if let Err(e) = dev.pair().await {
                            error!("Bluetooth: pairing {addr} failed: {e}");
                            push(BluetoothEvent::DeviceStatus(
                                addr,
                                DeviceStatus::Failed(format!("Pairing failed: {e}")),
                            ));
                            return;
                        }
                        if let Err(e) = dev.set_trusted(true).await {
                            warn!("Bluetooth: could not set {addr} as trusted: {e}");
                        }
                        info!("Bluetooth: paired {addr}");
                    }
                    // Now connect.
                    match dev.connect().await {
                        Ok(()) => {
                            info!("Bluetooth: connected to {addr}");
                            push(BluetoothEvent::DeviceStatus(addr, DeviceStatus::Connected));
                            // Refresh full device info.
                            if let Some(bt_dev) = build_bt_device(&dev).await {
                                push(BluetoothEvent::DeviceUpdated(bt_dev));
                            }
                        }
                        Err(e) => {
                            error!("Bluetooth: connect {addr} failed: {e}");
                            push(BluetoothEvent::DeviceStatus(
                                addr,
                                DeviceStatus::Failed(format!("Connect failed: {e}")),
                            ));
                        }
                    }
                }
                Err(e) => {
                    error!("Bluetooth: device {addr} not found: {e}");
                    push(BluetoothEvent::DeviceStatus(
                        addr,
                        DeviceStatus::Failed(format!("Device not found: {e}")),
                    ));
                }
            }
        }

        BluetoothCommand::Disconnect(addr) => {
            info!("Bluetooth: disconnecting {addr}");
            push(BluetoothEvent::DeviceStatus(
                addr,
                DeviceStatus::Disconnecting,
            ));
            match adapter.device(addr) {
                Ok(dev) => match dev.disconnect().await {
                    Ok(()) => {
                        info!("Bluetooth: disconnected {addr}");
                        if let Some(bt_dev) = build_bt_device(&dev).await {
                            push(BluetoothEvent::DeviceUpdated(bt_dev));
                        } else {
                            push(BluetoothEvent::DeviceStatus(addr, DeviceStatus::Paired));
                        }
                    }
                    Err(e) => {
                        error!("Bluetooth: disconnect {addr} failed: {e}");
                        push(BluetoothEvent::DeviceStatus(
                            addr,
                            DeviceStatus::Failed(format!("Disconnect failed: {e}")),
                        ));
                    }
                },
                Err(e) => {
                    push(BluetoothEvent::DeviceStatus(
                        addr,
                        DeviceStatus::Failed(format!("Device not found: {e}")),
                    ));
                }
            }
        }

        BluetoothCommand::Remove(addr) => {
            info!("Bluetooth: removing / unpairing {addr}");
            match adapter.remove_device(addr).await {
                Ok(()) => {
                    info!("Bluetooth: removed {addr}");
                    push(BluetoothEvent::DeviceRemoved(addr));
                }
                Err(e) => {
                    error!("Bluetooth: remove {addr} failed: {e}");
                    push(BluetoothEvent::Error(format!(
                        "Could not remove device: {e}"
                    )));
                }
            }
        }

        // StartScan / StopScan are handled in the outer loop.
        BluetoothCommand::StartScan | BluetoothCommand::StopScan => {}
    }
}

// ---------------------------------------------------------------------------
// UI-facing helper: ordered, deduplicated device list
// ---------------------------------------------------------------------------

/// A snapshot of all known devices, sorted for display.
///
/// Audio-sink devices are listed first, then by name.
pub fn sorted_devices(map: &HashMap<Address, BluetoothDevice>) -> Vec<&BluetoothDevice> {
    let mut devices: Vec<&BluetoothDevice> = map.values().collect();
    devices.sort_by(|a, b| {
        b.is_audio_sink
            .cmp(&a.is_audio_sink)
            .then(a.name.cmp(&b.name))
    });
    devices
}
