//! Page shown while a phone is connected as Bluetooth audio source.

use eframe::egui;

use crate::UiAction;
use crate::bt_audio::{Phone, PhoneControl, PlayerStatus};
use crate::pages::semi_transparent_group_frame;

pub fn paint_phone_audio(ui: &mut egui::Ui, phone: &Phone, actions: &mut Vec<UiAction>) {
    ui.add_space(16.0);
    ui.vertical_centered(|ui| {
        semi_transparent_group_frame(ui).show(ui, |ui| {
            ui.set_width((ui.available_width() - 16.0).min(480.0));
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("📱").size(40.0));
                ui.label(egui::RichText::new(&phone.name).strong().size(20.0));
                let status = match phone.status {
                    PlayerStatus::Playing => egui_i18n::tr!("phone_status_playing"),
                    PlayerStatus::Paused => egui_i18n::tr!("phone_status_paused"),
                    PlayerStatus::Stopped => egui_i18n::tr!("phone_status_stopped"),
                };
                ui.label(egui::RichText::new(status).weak());
                ui.add_space(12.0);

                let t = &phone.track;
                if t.title.is_empty() && t.artist.is_empty() {
                    ui.label(egui::RichText::new(egui_i18n::tr!("phone_no_track")).weak());
                } else {
                    ui.label(egui::RichText::new(&t.title).strong().size(18.0));
                    if !t.artist.is_empty() {
                        ui.label(&t.artist);
                    }
                    if !t.album.is_empty() {
                        ui.label(egui::RichText::new(&t.album).weak());
                    }
                }
                ui.add_space(16.0);

                if !phone.controllable {
                    ui.label(
                        egui::RichText::new(egui_i18n::tr!("phone_no_remote"))
                            .weak()
                            .small(),
                    );
                    return;
                }

                let size = egui::vec2(64.0, 56.0);
                let playing = phone.status == PlayerStatus::Playing;
                ui.horizontal(|ui| {
                    let total = size.x * 3.0 + ui.spacing().item_spacing.x * 2.0;
                    ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
                    if ui.add_sized(size, egui::Button::new("⏮")).clicked() {
                        actions.push(UiAction::PhoneControl(PhoneControl::Previous));
                    }
                    let label = if playing { "⏸" } else { "▶" };
                    if ui.add_sized(size, egui::Button::new(label)).clicked() {
                        actions.push(if playing {
                            UiAction::PhoneControl(PhoneControl::Pause)
                        } else {
                            UiAction::PhoneControl(PhoneControl::Play)
                        });
                    }
                    if ui.add_sized(size, egui::Button::new("⏭")).clicked() {
                        actions.push(UiAction::PhoneControl(PhoneControl::Next));
                    }
                });
            });
        });
    });
}
