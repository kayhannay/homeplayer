use std::fs::{self, File};
use std::io::Write;

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tracing::{debug, error};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[allow(unused)]
pub struct Config {
    pub sources: Vec<Source>,
    pub audio: AudioConfig,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ConfigSourceType {
    File,
    Stream,
    CD,
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
        let path = "/etc/homeplayer/config.json";
        let file = match fs::exists(path) {
            Ok(exists) => match exists {
                true => File::open(path),
                false => File::create(path),
            },
            Err(error) => Err(error),
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
