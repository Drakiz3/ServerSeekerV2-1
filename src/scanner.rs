use crate::config::{Config, ScanEngine};
use crate::database::Database;
use crate::protocol::PingableServer;
use crate::response::Server;
use crate::targeting;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use sqlx::types::ipnet::{IpNet, Ipv4Net};
use sqlx::{Pool, Postgres, Row};
use std::fmt::Debug;
use std::fs::File;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

pub static PERMITS: Semaphore = Semaphore::const_new(1000);
pub const TIMEOUT_SECS: Duration = Duration::from_secs(5);

#[derive(Debug, Default)]
pub struct ScanBuilder {
	config: Config,
	mode: Mode,
	pool: Option<Pool<Postgres>>,
}

impl ScanBuilder {
	pub fn config(mut self, config: Config) -> ScanBuilder {
		self.config = config;
		self
	}

	pub fn pool(mut self, pool: Option<Pool<Postgres>>) -> ScanBuilder {
		self.pool = pool;
		self
	}

	pub fn mode(mut self, mode: Mode) -> ScanBuilder {
		self.mode = mode;
		self
	}

	pub fn build(self) -> Scanner {
		Scanner {
			config: self.config,
			mode: self.mode,
			database: {
				match self.pool {
					Some(pool) => Database::new(pool),
					None => {
						error!("Failed to connect to database!");
						std::process::exit(1);
					}
				}
			},
		}
	}
}

#[derive(clap::ValueEnum, Clone, Debug, Default)]
pub enum Mode {
	#[default]
	Discovery,
	Rescanner,
}

#[derive(Debug, Clone)]
enum Target {
	File(PathBuf),
	Direct(String),
}

#[derive(Debug)]
pub struct Scanner {
	pub config: Config,
	pub mode: Mode,
	pub database: Database,
}

impl Scanner {
	/// Creates a new instance of a ScanBuilder
	pub fn new() -> ScanBuilder {
		ScanBuilder::default()
	}

	/// Starts the scanner based on the selected mode
	pub async fn start(&self) {
		if !self.config.scanner.repeat {
			warn!("Repeat is not enabled in config file! Will only scan once!");
		}

		match self.mode {
			Mode::Discovery => self.discovery().await,
			Mode::Rescanner => self.rescan().await,
		}
	}

	/// Rescan servers already found in the database
	async fn rescan(&self) {
		self.database.log_event(
			None,
			"INFO".to_string(),
			"SCAN_START".to_string(),
			format!(
				"Rescan started. Ports: {}-{}",
				self.config.scanner.port_range_start, self.config.scanner.port_range_end
			),
		);

		loop {
			let start_time = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
				Ok(n) => n.as_secs(),
				Err(_) => panic!("system time before unix epoch!"),
			};

			let ports = self.config.scanner.port_range_start..=self.config.scanner.port_range_end;
			let (tx, mut rx) = tokio::sync::mpsc::channel::<SocketAddrV4>(10);

			let mut stream = sqlx::query(
				"SELECT (address - '0.0.0.0'::inet) AS address FROM servers ORDER BY last_seen ASC",
			)
			.fetch(&self.database.0);

			// Spawn a task to produce values and send them down the transmitter
			tokio::spawn(async move {
				// Streams results from database. This works great for memory usage
				while let Some(Ok(row)) = stream.next().await {
					let address = match row.try_get::<i64, _>("address") {
						Ok(a) => Ipv4Addr::from_bits(a as u32),
						Err(_) => continue,
					};

					// Run for each port specified in config
					//
					// NOTE: clone is needed because RangeInclusive<T> doesn't implement copy
					// This should be optimized away anyway
					for port in ports.clone() {
						match tx.send(SocketAddrV4::new(address, port)).await {
							Ok(_) => {}
							Err(e) => debug!("send channel has been closed! {e}"),
						}
					}
				}
			});

			let total_servers = self
				.database
				.count_servers()
				.await
				.expect("failed to count servers!");

			let style = ProgressStyle::with_template(
				"[{elapsed_precise}] [{bar:40.white/blue}] {human_pos}/{human_len} {msg}",
			)
			.expect("failed to create progress bar style")
			.progress_chars("=>-");

			let bar =
				ProgressBar::new((total_servers * self.config.scanner.total_ports() as i64) as u64)
					.with_style(style);

			// Consume values from the receiver
			while let Some(socket) = rx.recv().await {
				let permit = PERMITS.acquire().await;

				let pool = self.database.clone();
				let bar = bar.clone();

				tokio::spawn(async move {
					// Move permit to future so it blocks the task as well
					let _permit = permit;

					task_wrapper(socket, pool).await;
					bar.inc(1);
				});
			}

			// Sleep for 10 seconds to ensure that all tasks finish
			tokio::time::sleep(Duration::from_secs(10)).await;
			bar.finish_and_clear();

			let end_time = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
				Ok(d) => d.as_secs(),
				Err(_) => panic!("system time before unix epoch!"),
			};

			info!("Scan completed in {} seconds", end_time - start_time);

			// Quit if only one scan is requested in config
			if !self.config.scanner.repeat {
				info!("Exiting");
				std::process::exit(0);
			}

			// Wait rescan delay before starting a new scan
			if self.config.scanner.scan_delay > 0 {
				info!(
					"Waiting {} seconds before starting another scan...",
					self.config.scanner.scan_delay
				);
				tokio::time::sleep(Duration::from_secs(self.config.scanner.scan_delay)).await;
			}
		}
	}

	/// Starts discovery mode (scanning for new servers)
	async fn discovery(&self) {
		self.database.log_event(
			None,
			"INFO".to_string(),
			"SCAN_START".to_string(),
			format!(
				"Engine: {:?}, Ports: {}-{}",
				self.config.scanner.engine,
				self.config.scanner.port_range_start,
				self.config.scanner.port_range_end
			),
		);

		loop {
			// Prepare targets
			let target = if let Some(custom) = &self.config.targeting.custom_target {
				Some(Target::Direct(custom.clone()))
			} else if let Some(country) = &self.config.targeting.country {
				match targeting::fetch_country_cidrs(country).await {
					Ok(path) => Some(Target::File(path)),
					Err(e) => {
						error!("Failed to fetch targets for country {}: {}", country, e);
						None
					}
				}
			} else {
				None
			};

			match self.config.scanner.engine {
				ScanEngine::Masscan => self.run_masscan_once(target).await,
				ScanEngine::Rustscan => self.run_rustscan_once(target).await,
			}

			// Quit if only one scan is requested in config
			if !self.config.scanner.repeat {
				info!("Exiting");
				std::process::exit(0);
			}

			// Wait rescan delay before starting a new scan
			if self.config.scanner.scan_delay > 0 {
				info!(
					"Waiting {} seconds before starting another scan",
					self.config.scanner.scan_delay
				);
				tokio::time::sleep(Duration::from_secs(self.config.scanner.scan_delay)).await;
			}
		}
	}

	async fn run_masscan_once(&self, target: Option<Target>) {
		let mut args = vec!["masscan".to_string(), "-c".to_string(), self.config.masscan.config_file.clone()];

	       // Safety exclusion required by masscan for large ranges
	       args.push("--exclude".to_string());
	       args.push("255.255.255.255".to_string());

		if let Some(t) = target {
			match t {
				Target::File(path) => {
					args.push("-iL".to_string());
					args.push(path.to_string_lossy().to_string());
				}
				Target::Direct(cidr) => {
					args.push(cidr);
				}
			}
		}

		// Determine command and args based on OS
		let (program, final_args) = if cfg!(target_os = "windows") {
			let local_bin = Path::new("bin/masscan.exe");
			if local_bin.exists() {
				(local_bin.to_string_lossy().to_string(), &args[1..])
			} else {
				("masscan.exe".to_string(), &args[1..])
			}
		} else {
			("sudo".to_string(), &args[..])
		};

		// Spawn masscan
		let mut command = Command::new(program)
			.args(final_args)
			.stdout(std::process::Stdio::piped())
			.stderr(std::process::Stdio::piped())
			.spawn()
			.expect("error while executing masscan");

		let stderr = command.stderr.take();
		if let Some(stderr) = stderr {
			tokio::spawn(async move {
				let mut reader = BufReader::new(stderr).lines();
				while let Ok(Some(line)) = reader.next_line().await {
					error!("Masscan stderr: {}", line);
				}
			});
		}

		// Verify stdout is valid
		let stdout = match command.stdout.take() {
			Some(o) => o,
			None => {
				error!("Failed to get stdout from masscan!");
				return;
			}
		};

		let mut reader = BufReader::new(stdout).lines();

		// Iterate over the lines of output from masscan
		while let Ok(Some(line)) = reader.next_line().await {
			let line_str = line.clone(); // Keep original line for parsing if needed, or just split
			let mut line = line_str.split_whitespace();

			let port = match line
				.nth(3)
				// Split on port/tcp
				.and_then(|p| p.split('/').nth(0))
				// Parse as u16
				.and_then(|s| s.parse::<u16>().ok())
			{
				Some(port) => port,
				None => continue,
			};

			// .nth() consumes all preceding elements so address will be the 2nd
			let address = match line.nth(1) {
				Some(address) => Ipv4Addr::from_str(address).unwrap(),
				None => continue,
			};

			self.database.log_event(
				Some(IpNet::from(Ipv4Net::from(address))),
				"INFO".to_string(),
				"HOST_FOUND".to_string(),
				format!("Port: {} (Masscan)", port),
			);

			let pool = self.database.clone();

			// Spawn a pinging task for each server found
			tokio::spawn(async move {
				let socket = SocketAddrV4::new(address, port);

				task_wrapper(socket, pool).await;
			});
		}
	}

	async fn run_rustscan_once(&self, target: Option<Target>) {
		let mut args = vec![self.config.rustscan.command.clone()];

		if self.config.scanner.port_range_start != self.config.scanner.port_range_end {
			args.push("-r".to_string());
			args.push(format!("{}-{}", self.config.scanner.port_range_start, self.config.scanner.port_range_end));
		} else {
			args.push("-p".to_string());
			args.push(self.config.scanner.port_range_start.to_string());
		}

		if let Some(t) = target {
			args.push("-a".to_string());
			match t {
				Target::File(path) => args.push(path.to_string_lossy().to_string()),
				Target::Direct(cidr_str) => {
					// Expand CIDR to file to avoid RustScan resolution issues on Windows
					if let Ok(net) = cidr_str.parse::<IpNet>() {
						let temp_path = Path::new("temp_rustscan_targets.txt");
						match File::create(temp_path) {
							Ok(mut file) => {
								let mut count = 0;
								for ip in net.hosts() {
									if let Err(_) = writeln!(file, "{}", ip) {
										break;
									}
									count += 1;
								}
								// If it's a single IP or empty (network address only), write the address itself
								if count == 0 {
									let _ = writeln!(file, "{}", net.addr());
								}
								args.push(temp_path.to_string_lossy().to_string());
							}
							Err(e) => {
								error!("Failed to create temp targets file: {}", e);
								args.push(cidr_str); // Fallback
							}
						}
					} else {
						args.push(cidr_str);
					}
				}
			}
		} else {
			warn!("No targets specified for RustScan (use --country or configure targeting). Skipping scan.");
			return;
		}

		args.push("--scripts".to_string());
		args.push("none".to_string());

		info!("Starting RustScan: {:?}", args);

		// Determine command and args based on OS
		let (program, final_args) = if cfg!(target_os = "windows") {
			let local_bin = Path::new("bin/rustscan.exe");
			if local_bin.exists() {
				(local_bin.to_string_lossy().to_string(), &args[1..])
			} else {
				let cmd = self.config.rustscan.command.clone();
				if !cmd.to_lowercase().ends_with(".exe") {
					(cmd + ".exe", &args[1..])
				} else {
					(cmd, &args[1..])
				}
			}
		} else {
			("sudo".to_string(), &args[..])
		};

		let mut command = Command::new(program)
			.args(final_args)
			.stdout(std::process::Stdio::piped())
			.stderr(std::process::Stdio::piped())
			.spawn()
			.expect("error while executing rustscan");

		let stdout = match command.stdout.take() {
			Some(o) => o,
			None => {
				error!("Failed to get stdout from rustscan!");
				return;
			}
		};

		let stderr = command.stderr.take();

		// Spawn a task to read stderr asynchronously to avoid blocking
		if let Some(stderr) = stderr {
			tokio::spawn(async move {
				let mut reader = BufReader::new(stderr).lines();
				while let Ok(Some(line)) = reader.next_line().await {
					error!("RustScan stderr: {}", line);
				}
			});
		}

		let mut reader = BufReader::new(stdout).lines();

		while let Ok(Some(line)) = reader.next_line().await {
			info!("RustScan output: {}", line); // Log output for debug
			if !line.starts_with("Open") {
				continue;
			}

			let parts: Vec<&str> = line.split_whitespace().collect();
			if parts.len() < 2 { continue; }

			let ip_port = parts[1];
			let Some((ip_str, port_str)) = ip_port.split_once(':') else { continue };

			let Ok(address) = Ipv4Addr::from_str(ip_str) else { continue };
			let Ok(port) = port_str.parse::<u16>() else { continue };

			self.database.log_event(
				Some(IpNet::from(Ipv4Net::from(address))),
				"INFO".to_string(),
				"HOST_FOUND".to_string(),
				format!("Port: {} (Rustscan)", port),
			);

			let pool = self.database.clone();
			tokio::spawn(async move {
				let socket = SocketAddrV4::new(address, port);
				task_wrapper(socket, pool).await;
			});
		}
	}
}

#[inline(always)]
async fn task_wrapper(socket: SocketAddrV4, pool: Database) {
	info!("Attempting to ping server: {}", socket);
	let server = PingableServer::new(socket);
	let start_time = std::time::Instant::now();

	// Try proper ping first (Modern servers 1.7+)
	// Wrap with timeout to prevent hanging reads
	let proper_result = tokio::time::timeout(TIMEOUT_SECS, server.proper_ping()).await;

	let response = match proper_result {
		Ok(Ok(r)) => Some(r),
		// If proper ping failed (error or timeout), try legacy
		_ => {
			match tokio::time::timeout(TIMEOUT_SECS, server.legacy_ping()).await {
				Ok(Ok(r)) => Some(r),
				Ok(Err(e)) => {
					// Log specific error
					warn!("Ping failed for {}. Proper result: {:?}, Legacy error: {:?}", socket, proper_result, e);
					None
				}
				Err(_) => {
					warn!("Ping timed out for {} (both Proper and Legacy)", socket);
					None
				}
			}
		}
	};
	let latency = start_time.elapsed().as_millis() as i32;

	if let Some(response) = response {
		match serde_json::from_str::<Server>(&response) {
			Ok(mut server) => {
				server.latency = Some(latency);
				if let Err(e) = pool.update_server(server, socket).await {
					error!("Error updating server in database! {e}");
				} else {
					info!("Successfully updated server: {}", socket);
					pool.log_event(
						Some(IpNet::from(Ipv4Net::from(*socket.ip()))),
						"INFO".to_string(),
						"SERVER_UPDATED".to_string(),
						format!("Server updated on port {}", socket.port())
					);
				}
			}
			Err(e) => {
				warn!("Failed to parse server response for {}: {}. Response: {}", socket, e, response);
			}
		}
	}
}
