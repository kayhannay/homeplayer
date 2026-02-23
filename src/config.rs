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
}

fn default_language() -> String {
    "en".to_string()
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            hide_settings: false,
            language: default_language(),
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
