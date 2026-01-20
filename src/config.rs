use serde::Deserialize;
use std::cmp::max;
use std::fs::File;
use std::io::{ErrorKind, Read};
use tracing::error;

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
	pub database: Database,
	pub scanner: ScannerConfig,
	pub masscan: Masscan,
	#[serde(default)]
	pub rustscan: Rustscan,
	#[serde(default)]
	pub targeting: Targeting,
	#[allow(dead_code)]
	pub player_tracking: PlayerTracking,
	pub country_tracking: CountryTracking,
	#[serde(default)]
	pub bot: BotConfig,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Database {
	pub host: String,
	pub port: u16,
	pub table: String,
	pub user: String,
	pub password: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ScannerConfig {
	pub repeat: bool,
	pub scan_delay: u64,
	pub port_range_start: u16,
	pub port_range_end: u16,
	#[serde(default)]
	pub engine: ScanEngine,
}

#[derive(Deserialize, Clone, Debug, Default, clap::ValueEnum, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ScanEngine {
	#[default]
	Masscan,
	Rustscan,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Masscan {
	pub config_file: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Rustscan {
	pub command: String,
}

impl Default for Rustscan {
	fn default() -> Self {
		Rustscan {
			command: "rustscan".to_string(),
		}
	}
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Targeting {
	pub country: Option<String>,
	#[serde(skip)]
	pub custom_target: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
#[allow(dead_code)]
pub struct PlayerTracking {
	pub enabled: bool,
	pub players: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct CountryTracking {
	pub enabled: bool,
	pub update_frequency: u64,
	pub ipinfo_token: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct BotConfig {
	pub enabled: bool,
	pub api_port: u16,
	pub script_path: String,
	pub concurrency: usize,
}

impl Default for BotConfig {
	fn default() -> Self {
		BotConfig {
			enabled: false,
			api_port: 3000,
			script_path: "bot/index.js".to_string(),
			concurrency: 5,
		}
	}
}

impl Default for Config {
	fn default() -> Self {
		Config {
			database: Database {
				host: "localhost".to_string(),
				port: 5432,
				table: "postgres".to_string(),
				user: "postgres".to_string(),
				password: "password".to_string(),
			},
			scanner: ScannerConfig {
				repeat: true,
				scan_delay: 60,
				port_range_start: 25565,
				port_range_end: 25565,
				engine: ScanEngine::Masscan,
			},
			masscan: Masscan {
				config_file: "masscan.conf".to_string(),
			},
			rustscan: Rustscan::default(),
			targeting: Targeting::default(),
			player_tracking: PlayerTracking {
				enabled: false,
				players: vec![],
			},
			country_tracking: CountryTracking {
				enabled: false,
				update_frequency: 48,
				ipinfo_token: "".to_string(),
			},
			bot: BotConfig::default(),
		}
	}
}

impl ScannerConfig {
	pub fn total_ports(&self) -> u16 {
		let start = self.port_range_start;
		let end = self.port_range_end;

		if start > end {
			error!("port_range_start cannot be greater than port_range_end!");
			std::process::exit(1);
		}

		max(1, end - start)
	}
}

pub fn load_config(path: &str) -> Result<Config, std::io::Error> {
	let mut file = File::open(path)?;
	let mut contents = String::new();
	file.read_to_string(&mut contents).unwrap_or_default();
	toml::from_str(&contents).map_err(|e| std::io::Error::new(ErrorKind::InvalidData, e))
}
