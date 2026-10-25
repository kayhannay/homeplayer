//! UI for the Wi-Fi settings section.
//!
//! All NetworkManager I/O is done off-thread by [`crate::wifi::WifiManager`];
//! this file only reads the cached state and sends commands.

use eframe::egui;

use crate::pages::semi_transparent_group_frame;
use crate::wifi::{
    HiddenSecurity, WifiCommand, WifiError, WifiEvent, WifiManager, WifiNetwork,
};

struct PasswordDialog {
    ssid: String,
    password: String,
    show_password: bool,
}

struct HiddenDialog {
    ssid: String,
    security: HiddenSecurity,
    password: String,
    show_password: bool,
}

#[derive(Default)]
pub struct WifiSettingsState {
    /// NetworkManager could not be reached.
    pub unavailable: Option<String>,
    pub networks: Vec<WifiNetwork>,
    pub current: Option<String>,
    pub scanning: bool,
    /// SSID of the connection attempt in progress.
    pub connecting: Option<String>,
    pub message: Option<(String, bool)>, // (text, is_error)
    pub expanded: bool,
    password_dialog: Option<PasswordDialog>,
    hidden_dialog: Option<HiddenDialog>,
    confirm_forget: Option<String>,
}

impl WifiSettingsState {
    pub fn apply_events(&mut self, manager: &WifiManager) {
        for event in manager.drain_events() {
            match event {
                WifiEvent::Unavailable(msg) => self.unavailable = Some(msg),
                WifiEvent::Networks { networks, current } => {
                    self.networks = networks;
                    self.current = current;
                }
                WifiEvent::Scanning(s) => self.scanning = s,
                WifiEvent::Connecting(c) => {
                    if c.is_some() {
                        self.message = None;
                    }
                    self.connecting = c;
                }
                WifiEvent::Error(e) => {
                    self.message = Some((error_text(&e), true));
                }
                WifiEvent::Connected(ssid) => {
                    self.message = Some((
                        format!("{} {ssid}", egui_i18n::tr!("wifi_connected_to")),
                        false,
                    ));
                }
            }
        }
    }
}

fn error_text(e: &WifiError) -> String {
    match e {
        WifiError::AuthFailed => egui_i18n::tr!("wifi_err_auth"),
        WifiError::NotFound => egui_i18n::tr!("wifi_err_not_found"),
        WifiError::Timeout => egui_i18n::tr!("wifi_err_timeout"),
        WifiError::DhcpFailed => egui_i18n::tr!("wifi_err_dhcp"),
        WifiError::NoDevice => egui_i18n::tr!("wifi_err_no_device"),
        WifiError::Other(msg) => msg.clone(),
    }
}

fn signal_icon(strength: u8) -> &'static str {
    match strength {
        0..=24 => "▁",
        25..=49 => "▂▃",
        50..=74 => "▂▃▅",
        _ => "▂▃▅▇",
    }
}

fn security_label(s: HiddenSecurity) -> String {
    match s {
        HiddenSecurity::Open => egui_i18n::tr!("wifi_sec_open"),
        HiddenSecurity::Wpa2 => egui_i18n::tr!("wifi_sec_wpa2"),
        HiddenSecurity::Wpa3 => egui_i18n::tr!("wifi_sec_wpa3"),
    }
}

pub fn paint_wifi_settings(
    ui: &mut egui::Ui,
    state: &mut WifiSettingsState,
    manager: &Option<WifiManager>,
) {
    if let Some(mgr) = manager {
        state.apply_events(mgr);
    }

    semi_transparent_group_frame(ui).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(egui_i18n::tr!("settings_wifi"))
                    .strong()
                    .size(15.0),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(mgr) = manager {
                    if state.scanning {
                        ui.spinner();
                    } else if ui.small_button(egui_i18n::tr!("wifi_scan")).clicked() {
                        state.message = None;
                        mgr.send(WifiCommand::Refresh);
                    }
                }
            });
        });
        ui.add_space(4.0);

        let (Some(mgr), None) = (manager, &state.unavailable) else {
            ui.label(
                egui::RichText::new(egui_i18n::tr!("wifi_unavailable"))
                    .color(egui::Color32::from_rgb(255, 120, 80))
                    .small(),
            );
            return;
        };

        match &state.current {
            Some(ssid) => ui.label(
                egui::RichText::new(format!("{} {ssid}", egui_i18n::tr!("wifi_connected_to")))
                    .color(egui::Color32::from_rgb(100, 220, 100))
                    .small(),
            ),
            None => ui.label(
                egui::RichText::new(egui_i18n::tr!("wifi_not_connected"))
                    .weak()
                    .small(),
            ),
        };

        let header = format!(
            "{} {} ({})",
            if state.expanded { "▼" } else { "▶" },
            egui_i18n::tr!("wifi_networks"),
            state.networks.len()
        );
        ui.add_space(4.0);
        if ui
            .add(egui::Button::new(egui::RichText::new(header).strong()).frame(false))
            .clicked()
        {
            state.expanded = !state.expanded;
        }

        if state.expanded {
            let mut connect: Option<(String, bool)> = None; // (ssid, needs password)
            let mut disconnect = false;
            let mut forget: Option<String> = None;
            let mut ask_forget: Option<String> = None;
            let mut cancel_forget = false;
            let busy = state.connecting.is_some();

            for net in &state.networks {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    let lock = if net.secured { "🔒" } else { "  " };
                    ui.label(
                        egui::RichText::new(format!(
                            "{lock} {}  {}",
                            net.ssid,
                            signal_icon(net.strength)
                        ))
                        .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if state.connecting.as_deref() == Some(net.ssid.as_str()) {
                            ui.spinner();
                            return;
                        }
                        if net.known {
                            if state.confirm_forget.as_deref() == Some(net.ssid.as_str()) {
                                ui.label(
                                    egui::RichText::new(egui_i18n::tr!("wifi_confirm_forget"))
                                        .color(egui::Color32::from_rgb(255, 100, 100))
                                        .small(),
                                );
                                if ui.small_button(egui_i18n::tr!("yes")).clicked() {
                                    forget = Some(net.ssid.clone());
                                }
                                if ui.small_button(egui_i18n::tr!("no")).clicked() {
                                    cancel_forget = true;
                                }
                            } else if ui
                                .small_button(
                                    egui::RichText::new("🗑")
                                        .color(egui::Color32::from_rgb(255, 100, 100)),
                                )
                                .on_hover_text(egui_i18n::tr!("wifi_forget_hover"))
                                .clicked()
                            {
                                ask_forget = Some(net.ssid.clone());
                            }
                        }
                        if net.active {
                            if ui
                                .add_enabled(
                                    !busy,
                                    egui::Button::new(egui_i18n::tr!("wifi_disconnect"))
                                        .small(),
                                )
                                .clicked()
                            {
                                disconnect = true;
                            }
                        } else if ui
                            .add_enabled(
                                !busy,
                                egui::Button::new(egui_i18n::tr!("wifi_connect")).small(),
                            )
                            .clicked()
                        {
                            connect = Some((net.ssid.clone(), net.secured && !net.known));
                        }
                    });
                });
            }

            if let Some((ssid, needs_password)) = connect {
                state.message = None;
                if needs_password {
                    state.password_dialog = Some(PasswordDialog {
                        ssid,
                        password: String::new(),
                        show_password: false,
                    });
                } else {
                    mgr.send(WifiCommand::Connect {
                        ssid,
                        password: None,
                    });
                }
            }
            if disconnect {
                mgr.send(WifiCommand::Disconnect);
            }
            if let Some(ssid) = forget {
                mgr.send(WifiCommand::Forget(ssid));
                state.confirm_forget = None;
            }
            if let Some(ssid) = ask_forget {
                state.confirm_forget = Some(ssid);
            }
            if cancel_forget {
                state.confirm_forget = None;
            }
        }

        ui.add_space(6.0);
        if ui
            .add_enabled(
                state.connecting.is_none(),
                egui::Button::new(egui_i18n::tr!("wifi_other_network")).small(),
            )
            .clicked()
        {
            state.message = None;
            state.hidden_dialog = Some(HiddenDialog {
                ssid: String::new(),
                security: HiddenSecurity::Wpa2,
                password: String::new(),
                show_password: false,
            });
        }

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

    if let Some(mgr) = manager {
        paint_password_dialog(ui.ctx(), state, mgr);
        paint_hidden_dialog(ui.ctx(), state, mgr);
    }
}

/// Modal placed at the top of the screen so the on-screen keyboard at the
/// bottom does not cover the input fields.
fn dialog(id: &'static str) -> egui::Modal {
    egui::Modal::new(egui::Id::new(id)).area(
        egui::Area::new(egui::Id::new((id, "area")))
            .anchor(egui::Align2::CENTER_TOP, [0.0, 16.0])
            .order(egui::Order::Foreground),
    )
}

fn password_field(ui: &mut egui::Ui, password: &mut String, show: &mut bool, focus: bool) {
    ui.horizontal(|ui| {
        let edit = ui.add(
            egui::TextEdit::singleline(password)
                .password(!*show)
                .desired_width(220.0),
        );
        if focus {
            edit.request_focus();
        }
        ui.checkbox(show, egui_i18n::tr!("wifi_show_password"));
    });
}

fn paint_password_dialog(ctx: &egui::Context, state: &mut WifiSettingsState, mgr: &WifiManager) {
    let Some(dlg) = &mut state.password_dialog else {
        return;
    };
    let mut connect = false;
    let mut cancel = false;
    dialog("wifi_password_dialog").show(ctx, |ui| {
        ui.set_width(360.0);
        ui.heading(format!("{} {}", egui_i18n::tr!("wifi_connect_to"), dlg.ssid));
        ui.add_space(8.0);
        ui.label(egui_i18n::tr!("wifi_password"));
        let focus = ui.data_mut(|d| {
            let seen = d.get_temp::<bool>(egui::Id::new("wifi_pw_focus")).is_some();
            d.insert_temp(egui::Id::new("wifi_pw_focus"), true);
            !seen
        });
        password_field(ui, &mut dlg.password, &mut dlg.show_password, focus);
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let size = egui::vec2(110.0, 44.0);
            if ui
                .add_enabled(
                    dlg.password.len() >= 8,
                    egui::Button::new(egui_i18n::tr!("wifi_connect")).min_size(size),
                )
                .clicked()
            {
                connect = true;
            }
            if ui
                .add(egui::Button::new(egui_i18n::tr!("power_cancel")).min_size(size))
                .clicked()
            {
                cancel = true;
            }
        });
    });

    if connect {
        let dlg = state.password_dialog.take().unwrap();
        mgr.send(WifiCommand::Connect {
            ssid: dlg.ssid,
            password: Some(dlg.password),
        });
    }
    if connect || cancel {
        state.password_dialog = None;
        ctx.data_mut(|d| d.remove::<bool>(egui::Id::new("wifi_pw_focus")));
    }
}

fn paint_hidden_dialog(ctx: &egui::Context, state: &mut WifiSettingsState, mgr: &WifiManager) {
    let Some(dlg) = &mut state.hidden_dialog else {
        return;
    };

    let mut connect = false;
    let mut cancel = false;
    dialog("wifi_hidden_dialog").show(ctx, |ui| {
        ui.set_width(360.0);
        ui.heading(egui_i18n::tr!("wifi_other_network_title"));
        ui.add_space(8.0);
        ui.label(egui_i18n::tr!("wifi_ssid"));
        let focus = ui.data_mut(|d| {
            let seen = d.get_temp::<bool>(egui::Id::new("wifi_hidden_focus")).is_some();
            d.insert_temp(egui::Id::new("wifi_hidden_focus"), true);
            !seen
        });
        let edit = ui.add(egui::TextEdit::singleline(&mut dlg.ssid).desired_width(280.0));
        if focus {
            edit.request_focus();
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(egui_i18n::tr!("wifi_security"));
            egui::ComboBox::from_id_salt("wifi_hidden_security")
                .selected_text(security_label(dlg.security))
                .show_ui(ui, |ui| {
                    for s in HiddenSecurity::ALL {
                        ui.selectable_value(&mut dlg.security, s, security_label(s));
                    }
                });
        });
        if dlg.security != HiddenSecurity::Open {
            ui.add_space(6.0);
            ui.label(egui_i18n::tr!("wifi_password"));
            password_field(ui, &mut dlg.password, &mut dlg.show_password, false);
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            let size = egui::vec2(110.0, 44.0);
            let valid = !dlg.ssid.is_empty()
                && (dlg.security == HiddenSecurity::Open || dlg.password.len() >= 8);
            if ui
                .add_enabled(
                    valid,
                    egui::Button::new(egui_i18n::tr!("wifi_connect")).min_size(size),
                )
                .clicked()
            {
                connect = true;
            }
            if ui
                .add(egui::Button::new(egui_i18n::tr!("power_cancel")).min_size(size))
                .clicked()
            {
                cancel = true;
            }
        });
    });

    if connect {
        let dlg = state.hidden_dialog.take().unwrap();
        mgr.send(WifiCommand::ConnectHidden {
            ssid: dlg.ssid,
            security: dlg.security,
            password: dlg.password,
        });
    }
    if connect || cancel {
        state.hidden_dialog = None;
        ctx.data_mut(|d| d.remove::<bool>(egui::Id::new("wifi_hidden_focus")));
    }
}
