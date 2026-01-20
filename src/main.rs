mod bot_scanner;
mod config;
mod country_tracking;
mod database;
mod installer;
mod protocol;
mod response;
mod scanner;
mod targeting;
mod utils;

use crate::scanner::Scanner;
use clap::Parser;
use config::{load_config, ScanEngine};
use scanner::Mode;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::ConnectOptions;
use std::time::Duration;
use tracing::log::LevelFilter;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[clap(about = "Scans the internet for minecraft servers and indexes them")]
#[clap(rename_all = "kebab-case")]
struct Args {
	#[clap(help = "Specifies the mode to run")]
	#[clap(default_value = "rescanner")]
	#[clap(long, short = 'm')]
	mode: Mode,

	#[clap(help = "Specifies the location of the config file")]
	#[clap(default_value = "config.toml")]
	#[clap(long, short = 'c')]
	config_file: String,

	#[clap(help = "Specifies the scan engine")]
	#[clap(long, short = 'e')]
	engine: Option<ScanEngine>,

	#[clap(help = "Specifies a country code to target (e.g. BR, US)")]
	#[clap(long)]
	country: Option<String>,

	#[clap(help = "Specifies a CIDR or IP to target (e.g. 192.168.1.0/24)", long, short = 't')]
	target: Option<String>,

	#[clap(help = "Specifies a port range (e.g. 25565 or 25500-25600)", long, short = 'p')]
	ports: Option<String>,
}

#[tokio::main]
async fn main() {
	tracing_subscriber::fmt::init();

	if let Err(e) = installer::install_binaries().await {
		error!("Failed to install binaries: {}", e);
	}

	let arguments = Args::parse();
	let mut config = match load_config(&arguments.config_file) {
		Ok(config) => config,
		Err(e) => {
			error!("Fatal error loading config file: {}", e);
			std::process::exit(1);
		}
	};

	if let Some(engine) = arguments.engine {
		config.scanner.engine = engine;
	}

	if let Some(country) = arguments.country {
		config.targeting.country = Some(country);
	}

	if let Some(target) = arguments.target {
		config.targeting.custom_target = Some(target);
		// Disable country targeting if specific target is provided
		config.targeting.country = None;
	}

	if let Some(ports_str) = arguments.ports {
		if let Some((start, end)) = ports_str.split_once('-') {
			config.scanner.port_range_start = start.parse().expect("Invalid start port");
			config.scanner.port_range_end = end.parse().expect("Invalid end port");
		} else {
			let port = ports_str.parse().expect("Invalid port");
			config.scanner.port_range_start = port;
			config.scanner.port_range_end = port;
		}
	}

	info!("Using config file: {}", arguments.config_file);

	let options = PgConnectOptions::new()
		.username(&config.database.user)
		.password(&config.database.password)
		.host(&config.database.host)
		.port(config.database.port)
		.database(&config.database.table)
		// Turn off slow statement logging, this clogs the console
		.log_slow_statements(LevelFilter::Off, Duration::from_secs(60));

	let pool = PgPoolOptions::new()
		// Refresh connections every 24 hours
		.max_lifetime(Duration::from_secs(86400))
		.acquire_slow_threshold(Duration::from_secs(60))
		.connect_with(options)
		.await
		.ok();

	if let Some(pool) = &pool {
		// Run migrations automatically
		if let Err(e) = sqlx::migrate!("./migrations").run(pool).await {
			error!("Failed to run migrations: {}", e);
			std::process::exit(1);
		}

		if config.country_tracking.enabled {
			// Create tables
			if country_tracking::create_tables(pool).await.is_err() {
				error!("failed to create tables");
				std::process::exit(1);
			}

			// Spawn task to update database
			tokio::task::spawn(country_tracking::country_tracking(
				pool.clone(),
				config.clone(),
			));
		}
	} else {
		error!("Failed to connect to database");
		std::process::exit(1);
	}

	let mut backoff = Duration::from_secs(1);

	loop {
		info!("Starting scanner task...");
		
		let config_clone = config.clone();
		let pool_clone = pool.clone();
		let mode_clone = arguments.mode.clone();

		let handle = tokio::spawn(async move {
			Scanner::new()
				.config(config_clone)
				.mode(mode_clone)
				.pool(pool_clone)
				.build()
				.start()
				.await;
		});

		match handle.await {
			Ok(_) => {
				info!("Scanner finished successfully. Restarting in 5s...");
				tokio::time::sleep(Duration::from_secs(5)).await;
				backoff = Duration::from_secs(1);
			}
			Err(e) => {
				error!("Scanner task panicked: {}. Restarting in {:?}...", e, backoff);
				tokio::time::sleep(backoff).await;
				backoff = std::cmp::min(backoff * 2, Duration::from_secs(60));
			}
		}
	}
}
