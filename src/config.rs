use std::fs::File;
use std::io::Write;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[allow(unused)]
pub struct Config {
    pub sources: Vec<Source>,
    pub audio: AudioConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub power: PowerConfig,
}

/// What happens when the hardware power button is pressed.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
pub enum PowerButtonAction {
    /// Ignore the button.
    #[default]
    None,
    /// Shut the system down immediately.
    Shutdown,
    /// Show a dialog that lets the user choose between shutdown and restart.
    Dialog,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PowerConfig {
    #[serde(default)]
    pub button_action: PowerButtonAction,
    /// GPIO character device, e.g. `/dev/gpiochip0` (`/dev/gpiochip4` on a Pi 5).
    #[serde(default = "default_gpio_chip")]
    pub gpio_chip: String,
    /// GPIO line (BCM number) the button is connected to. No button when `None`.
    #[serde(default)]
    pub gpio_pin: Option<u32>,
    /// `true` if the button pulls the line to ground when pressed (internal
    /// pull-up is enabled), `false` if it drives the line high.
    #[serde(default = "default_true")]
    pub active_low: bool,
    #[serde(default = "default_shutdown_command")]
    pub shutdown_command: Vec<String>,
    #[serde(default = "default_restart_command")]
    pub restart_command: Vec<String>,
}

fn default_gpio_chip() -> String {
    "/dev/gpiochip0".to_string()
}

fn default_true() -> bool {
    true
}

fn default_shutdown_command() -> Vec<String> {
    vec!["systemctl".into(), "poweroff".into()]
}

fn default_restart_command() -> Vec<String> {
    vec!["systemctl".into(), "reboot".into()]
}

impl Default for PowerConfig {
    fn default() -> Self {
        Self {
            button_action: PowerButtonAction::None,
            gpio_chip: default_gpio_chip(),
            gpio_pin: None,
            active_low: true,
            shutdown_command: default_shutdown_command(),
            restart_command: default_restart_command(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum ConfigSourceType {
    File,
    Stream,
    CD,
    KidsFile,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[allow(unused)]
pub struct Source {
    pub source_type: ConfigSourceType,
    pub name: String,
    pub path: String,
    pub stations: Vec<Station>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[allow(unused)]
pub struct Station {
    pub name: String,
    pub url: String,
    pub icon: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[allow(unused)]
pub struct AudioConfig {
    pub start_volume: u8,
    /// Maximum volume the player is allowed to reach (0–100).  Defaults to
    /// 100 when not present in the configuration file.
    #[serde(default = "default_max_volume")]
    pub max_volume: u8,
    /// Name of the audio output device to use. When `None` or `"Default"` the
    /// system default device is used.
    #[serde(default)]
    pub device: Option<String>,
}

fn default_max_volume() -> u8 {
    100
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[allow(unused)]
pub struct UiConfig {
    #[serde(default)]
    pub hide_settings: bool,
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub keyboard_layout: KeyboardLayout,
}

/// Layout of the on-screen keyboard.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
pub enum KeyboardLayout {
    #[default]
    Qwertz,
    Qwerty,
    Colemak,
}

impl KeyboardLayout {
    pub const ALL: [KeyboardLayout; 3] = [Self::Qwertz, Self::Qwerty, Self::Colemak];

    pub fn label(self) -> &'static str {
        match self {
            Self::Qwertz => "QWERTZ",
            Self::Qwerty => "QWERTY",
            Self::Colemak => "Colemak",
        }
    }

    pub fn to_keyboard(self) -> egui_keyboard::layouts::KeyboardLayout {
        use egui_keyboard::layouts::KeyboardLayout as K;
        match self {
            Self::Qwertz => K::Qwertz,
            Self::Qwerty => K::Qwerty,
            Self::Colemak => K::Colemak,
        }
    }
}

fn default_language() -> String {
    "en".to_string()
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            hide_settings: false,
            language: default_language(),
            keyboard_layout: KeyboardLayout::default(),
        }
    }
}

impl Config {
    pub fn new() -> Result<Self, config::ConfigError> {
        let config = config::Config::builder()
            .add_source(config::File::new("config", config::FileFormat::Json).required(false))
            .add_source(
                config::File::new("/etc/homeplayer/config", config::FileFormat::Json)
                    .required(false),
            )
            .add_source(config::Environment::with_prefix("APP"))
            .build()?;
        config.try_deserialize()
    }

    pub fn save(&self) -> Result<(), anyhow::Error> {
        let json = serde_json::to_string_pretty(self).unwrap();

        // Try the system-wide path first, fall back to local config.json
        let primary = "/etc/homeplayer/config.json";
        let fallback = "config.json";

        let (path, file) = match File::create(primary) {
            Ok(f) => (primary, Ok(f)),
            Err(e) => {
                debug!("Cannot write to {primary} ({e}), falling back to {fallback}");
                (fallback, File::create(fallback))
            }
        };

        match file {
            Ok(mut config_file) => {
                debug!("Saving settings to {path} ...");
                config_file.write_all(json.as_bytes())?;
                let _ = config_file.flush();
                debug!("Updated configuration: {json}");
                Ok(())
            }
            Err(error) => {
                error!("Could not write config to file {path}: {error}");
                Err(anyhow!(error))
            }
        }
    }
}
