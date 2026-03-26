//! UI for the Bluetooth pairing settings section.
//!
//! This module renders the "Bluetooth" card inside the settings page.  All
//! BlueZ I/O is done off-thread by [`crate::bluetooth::BluetoothManager`];
//! this file only reads the cached state and sends commands.

use std::collections::HashMap;

use bluer::Address;
use eframe::egui;

use crate::bluetooth::{
    BluetoothCommand, BluetoothDevice, BluetoothEvent, BluetoothManager, DeviceStatus,
    sorted_devices,
};
use crate::pages::semi_transparent_group_frame;

// ---------------------------------------------------------------------------
// Persistent UI state
// ---------------------------------------------------------------------------

/// All mutable state owned by the Bluetooth settings section.
#[derive(Default)]
pub struct BluetoothSettingsState {
    /// Whether the Bluetooth adapter was detected as powered-on.
    pub adapter_powered: bool,
    /// Whether a device scan is currently running.
    pub scanning: bool,
    /// All devices we know about, keyed by their MAC address.
    pub devices: HashMap<Address, BluetoothDevice>,
    /// A transient error / info message shown below the device list.
    pub message: Option<(String, bool)>, // (text, is_error)
    /// Whether the device list is expanded (collapsed by default).
    pub devices_expanded: bool,
    /// Address of the device for which a "remove / unpair" confirmation
    /// dialog is currently shown.
    pub confirm_remove: Option<Address>,
}

impl BluetoothSettingsState {
    /// Apply every pending event from the manager to the local state.
    ///
    /// Call this once per frame *before* painting so the UI always reflects
    /// the latest information.
    pub fn apply_events(&mut self, manager: &BluetoothManager) {
        for event in manager.drain_events() {
            match event {
                BluetoothEvent::AdapterPowered(on) => {
                    self.adapter_powered = on;
                    if !on {
                        self.scanning = false;
                    }
                }
                BluetoothEvent::ScanningChanged(s) => {
                    self.scanning = s;
                }
                BluetoothEvent::DeviceUpdated(dev) => {
                    self.devices.insert(dev.address, dev);
                }
                BluetoothEvent::DeviceRemoved(addr) => {
                    self.devices.remove(&addr);
                    if self.confirm_remove == Some(addr) {
                        self.confirm_remove = None;
                    }
                }
                BluetoothEvent::DeviceStatus(addr, status) => {
                    if let Some(dev) = self.devices.get_mut(&addr) {
                        dev.status = status;
                    }
                }
                BluetoothEvent::Error(msg) => {
                    self.message = Some((msg, true));
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Paint function
// ---------------------------------------------------------------------------

/// Render the complete Bluetooth section inside the settings scroll area.
///
/// `manager` may be `None` if Bluetooth initialisation failed (e.g. no
/// adapter, BlueZ not running).  In that case a clear error card is shown.
pub fn paint_bluetooth_settings(
    ui: &mut egui::Ui,
    state: &mut BluetoothSettingsState,
    manager: &Option<BluetoothManager>,
) {
    semi_transparent_group_frame(ui).show(ui, |ui| {
        // ── Header row ──────────────────────────────────────────────────
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(egui_i18n::tr!("settings_bluetooth"))
                    .strong()
                    .size(15.0),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(mgr) = manager {
                    if state.scanning {
                        // Animated spinner text while scanning.
                        ui.spinner();
                        if ui
                            .small_button(egui_i18n::tr!("bt_stop_scan"))
                            .on_hover_text(egui_i18n::tr!("bt_stop_scan_hover"))
                            .clicked()
                        {
                            mgr.send(BluetoothCommand::StopScan);
                        }
                    } else {
                        if ui
                            .small_button(egui_i18n::tr!("bt_scan"))
                            .on_hover_text(egui_i18n::tr!("bt_scan_hover"))
                            .clicked()
                        {
                            state.message = None;
                            mgr.send(BluetoothCommand::StartScan);
                        }
                    }
                }
            });
        });

        ui.add_space(4.0);

        // ── No manager / adapter unavailable ────────────────────────────
        let Some(mgr) = manager else {
            ui.label(
                egui::RichText::new(egui_i18n::tr!("bt_unavailable"))
                    .color(egui::Color32::from_rgb(255, 120, 80))
                    .small(),
            );
            return;
        };

        // ── Adapter powered-off warning ──────────────────────────────────
        if !state.adapter_powered {
            ui.label(
                egui::RichText::new(egui_i18n::tr!("bt_adapter_off"))
                    .color(egui::Color32::from_rgb(255, 200, 50))
                    .small(),
            );
            ui.add_space(2.0);
        }

        // ── Device list ──────────────────────────────────────────────────
        if state.devices.is_empty() {
            ui.label(
                egui::RichText::new(if state.scanning {
                    egui_i18n::tr!("bt_scanning_hint")
                } else {
                    egui_i18n::tr!("bt_no_devices")
                })
                .weak()
                .small(),
            );
        } else {
            // Collapsible header for the device list.
            let device_count = state.devices.len();
            let header_icon = if state.devices_expanded { "▼" } else { "▶" };
            let header_text = format!(
                "{} {} ({})",
                header_icon,
                egui_i18n::tr!("bt_devices"),
                device_count
            );

            ui.add_space(4.0);
            if ui
                .add(egui::Button::new(egui::RichText::new(header_text).strong()).frame(false))
                .clicked()
            {
                state.devices_expanded = !state.devices_expanded;
            }

            if state.devices_expanded {
                let devices = sorted_devices(&state.devices);
                let mut connect_addr: Option<Address> = None;
                let mut disconnect_addr: Option<Address> = None;
                let mut remove_addr: Option<Address> = None;
                let mut confirm_remove_addr: Option<Address> = None;
                let mut cancel_confirm: bool = false;

                for dev in devices {
                    let addr = dev.address;
                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(2.0);

                    paint_device_row(
                        ui,
                        dev,
                        state.confirm_remove == Some(addr),
                        &mut connect_addr,
                        &mut disconnect_addr,
                        &mut remove_addr,
                        &mut confirm_remove_addr,
                        &mut cancel_confirm,
                    );
                }

                // Apply deferred actions so we don't borrow `state.devices`
                // mutably while iterating.
                if let Some(addr) = connect_addr {
                    state.message = None;
                    mgr.send(BluetoothCommand::Connect(addr));
                    if let Some(dev) = state.devices.get_mut(&addr) {
                        dev.status = DeviceStatus::Connecting;
                    }
                }
                if let Some(addr) = disconnect_addr {
                    state.message = None;
                    mgr.send(BluetoothCommand::Disconnect(addr));
                    if let Some(dev) = state.devices.get_mut(&addr) {
                        dev.status = DeviceStatus::Disconnecting;
                    }
                }
                if let Some(addr) = remove_addr {
                    mgr.send(BluetoothCommand::Remove(addr));
                    state.confirm_remove = None;
                }
                if let Some(addr) = confirm_remove_addr {
                    state.confirm_remove = Some(addr);
                }
                if cancel_confirm {
                    state.confirm_remove = None;
                }
            }
        }

        // ── Transient message ────────────────────────────────────────────
        if let Some((msg, is_err)) = &state.message {
            ui.add_space(6.0);
            let color = if *is_err {
                egui::Color32::from_rgb(255, 100, 100)
            } else {
                egui::Color32::from_rgb(100, 220, 100)
            };
            ui.label(egui::RichText::new(msg).color(color).small());
        }
    });
}

// ---------------------------------------------------------------------------
// Single device row
// ---------------------------------------------------------------------------

/// Render one row for `dev` and record which button was clicked (if any)
/// into the `*_addr` / flag out-parameters so that the caller can apply the
/// actions after the iteration over the device list has finished.
#[allow(clippy::too_many_arguments)]
fn paint_device_row(
    ui: &mut egui::Ui,
    dev: &BluetoothDevice,
    confirm_remove_shown: bool,
    connect_out: &mut Option<Address>,
    disconnect_out: &mut Option<Address>,
    remove_out: &mut Option<Address>,
    confirm_remove_out: &mut Option<Address>,
    cancel_confirm_out: &mut bool,
) {
    let addr = dev.address;

    ui.horizontal(|ui| {
        // Icon + name + RSSI
        let icon = dev.icon();
        let name_text = egui::RichText::new(format!("{icon}  {}", dev.name)).strong();
        ui.label(name_text);

        if let Some(rssi) = dev.rssi {
            ui.label(egui::RichText::new(format!("({rssi} dBm)")).weak().small());
        }

        // Right-aligned action buttons
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            match &dev.status {
                DeviceStatus::Connected => {
                    // Disconnect button
                    if ui
                        .small_button(egui_i18n::tr!("bt_disconnect"))
                        .on_hover_text(egui_i18n::tr!("bt_disconnect_hover"))
                        .clicked()
                    {
                        *disconnect_out = Some(addr);
                    }

                    // Remove / unpair (with confirmation)
                    if confirm_remove_shown {
                        ui.label(
                            egui::RichText::new(egui_i18n::tr!("bt_confirm_remove"))
                                .color(egui::Color32::from_rgb(255, 100, 100))
                                .small(),
                        );
                        if ui.small_button(egui_i18n::tr!("yes")).clicked() {
                            *remove_out = Some(addr);
                        }
                        if ui.small_button(egui_i18n::tr!("no")).clicked() {
                            *cancel_confirm_out = true;
                        }
                    } else if ui
                        .small_button(
                            egui::RichText::new("🗑").color(egui::Color32::from_rgb(255, 100, 100)),
                        )
                        .on_hover_text(egui_i18n::tr!("bt_remove_hover"))
                        .clicked()
                    {
                        *confirm_remove_out = Some(addr);
                    }
                }

                DeviceStatus::Paired => {
                    // Reconnect button
                    if ui
                        .small_button(egui_i18n::tr!("bt_connect"))
                        .on_hover_text(egui_i18n::tr!("bt_connect_hover"))
                        .clicked()
                    {
                        *connect_out = Some(addr);
                    }

                    // Remove / unpair
                    if confirm_remove_shown {
                        ui.label(
                            egui::RichText::new(egui_i18n::tr!("bt_confirm_remove"))
                                .color(egui::Color32::from_rgb(255, 100, 100))
                                .small(),
                        );
                        if ui.small_button(egui_i18n::tr!("yes")).clicked() {
                            *remove_out = Some(addr);
                        }
                        if ui.small_button(egui_i18n::tr!("no")).clicked() {
                            *cancel_confirm_out = true;
                        }
                    } else if ui
                        .small_button(
                            egui::RichText::new("🗑").color(egui::Color32::from_rgb(255, 100, 100)),
                        )
                        .on_hover_text(egui_i18n::tr!("bt_remove_hover"))
                        .clicked()
                    {
                        *confirm_remove_out = Some(addr);
                    }
                }

                DeviceStatus::Discovered => {
                    // Pair + connect button
                    if ui
                        .small_button(egui_i18n::tr!("bt_pair"))
                        .on_hover_text(egui_i18n::tr!("bt_pair_hover"))
                        .clicked()
                    {
                        *connect_out = Some(addr);
                    }
                }

                DeviceStatus::Connecting | DeviceStatus::Disconnecting => {
                    ui.spinner();
                }

                DeviceStatus::Failed(_) => {
                    // Allow retrying after failure
                    if ui
                        .small_button(egui_i18n::tr!("bt_retry"))
                        .on_hover_text(egui_i18n::tr!("bt_retry_hover"))
                        .clicked()
                    {
                        *connect_out = Some(addr);
                    }
                }
            }
        });
    });

    // Status label on a second line (subtle, small)
    let (status_text, status_color) = status_label(&dev.status);
    ui.label(
        egui::RichText::new(format!("  {}", status_text))
            .color(status_color)
            .small(),
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn status_label(status: &DeviceStatus) -> (String, egui::Color32) {
    match status {
        DeviceStatus::Discovered => (egui_i18n::tr!("bt_status_discovered"), egui::Color32::GRAY),
        DeviceStatus::Connecting => (
            egui_i18n::tr!("bt_status_connecting"),
            egui::Color32::from_rgb(100, 180, 255),
        ),
        DeviceStatus::Connected => (
            egui_i18n::tr!("bt_status_connected"),
            egui::Color32::from_rgb(100, 220, 100),
        ),
        DeviceStatus::Paired => (
            egui_i18n::tr!("bt_status_paired"),
            egui::Color32::from_rgb(180, 220, 255),
        ),
        DeviceStatus::Failed(msg) => (
            format!("{} {msg}", egui_i18n::tr!("bt_status_failed")),
            egui::Color32::from_rgb(255, 100, 100),
        ),
        DeviceStatus::Disconnecting => (
            egui_i18n::tr!("bt_status_disconnecting"),
            egui::Color32::from_rgb(255, 180, 80),
        ),
    }
}
