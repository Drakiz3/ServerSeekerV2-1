use crate::config::Config;
use anyhow::bail;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressIterator, ProgressStyle};
use serde::Deserialize;
use sqlx::types::ipnet::IpNet;
use sqlx::PgPool;
use std::fs::File;
use std::io::{Read, Write};
use std::str::FromStr;
use std::time::Duration;
use tracing::info;

const DOWNLOAD_URL: &str = "https://ipinfo.io/data/ipinfo_lite.json.gz?token=";

#[derive(Deserialize, Debug)]
struct CountryRow {
	network: String,
	country: String,
	country_code: String,
	asn: Option<String>,
	#[serde(rename = "as_name")]
	company: Option<String>,
}

pub async fn country_tracking(pool: PgPool, config: Config) -> anyhow::Result<()> {
	tracing::info!("Starting country tracking service...");
	loop {
		tracing::info!("Starting ipinfo database download...");
		match download_database(&config).await {
			Ok(_) => tracing::info!("ipinfo database downloaded successfully."),
			Err(e) => tracing::error!("Failed to download ipinfo database: {}", e),
		}
		
		tracing::info!("Starting to insert data into countries table...");
		match insert_json_to_table(&pool).await {
			Ok(_) => tracing::info!("Successfully inserted data into countries table."),
			Err(e) => tracing::error!("Failed to insert data into countries table: {}", e),
		}

		// Sleep
		let sleep_duration = config.country_tracking.update_frequency * 60 * 60;
		tracing::info!(
			"Country tracking service sleeping for {} hours.",
			sleep_duration / 3600
		);
		tokio::time::sleep(Duration::from_secs(sleep_duration)).await;
	}
}

async fn download_database(config: &Config) -> anyhow::Result<()> {
	let url = format!("{}{}", DOWNLOAD_URL, config.country_tracking.ipinfo_token);
	tracing::debug!("Downloading from URL: {}", url);
	let response = reqwest::get(url).await?;

	// If response is OK write to file and unzip
	if response.status().is_success() {
		// Content length header is required for the progress bar
		// IPInfo should always supply this
		let content_length = match response.content_length() {
			Some(len) => len,
			None => bail!("Content-Length header was not set!"),
		};

		let mut downloaded: u64 = 0;
		let mut output_file = File::create("ipinfo.json.gz")?;
		let mut reader = response.bytes_stream();

		let style = ProgressStyle::with_template(
			"[{elapsed_precise}] [{bar:40.white/blue}] {bytes}/{total_bytes} {msg}",
		)
		.expect("failed to create progress bar style")
		.progress_chars("=>-");

		let bar = ProgressBar::new(content_length).with_style(style);
		bar.set_message("Downloading the latest version of the IPInfo database...");

		while let Some(Ok(chunk)) = reader.next().await {
			output_file.write_all(&chunk)?;

			// Update download bar position
			let new = std::cmp::min(downloaded + (chunk.len() as u64), content_length);
			downloaded = new;
			bar.set_position(new);
		}

		// Done
		bar.finish_with_message("Finished!");
		info!("Decompressing output file...");

		// Decompress file
		let mut decoder = GzDecoder::new(File::open("ipinfo.json.gz")?);
		let mut file = File::create("ipinfo.json")?;
		let mut string = String::new();

		// Write to output file
		decoder.read_to_string(&mut string)?;
		let mut output_file = File::create("ipinfo.json")?;
		output_file.write_all(string.as_bytes())?;
		file.flush()?;

		// Delete compressed file
		std::fs::remove_file("ipinfo.json.gz")?;

		Ok(())
	} else {
		bail!(
			"IPInfo download failed: {} {:?}",
			response.status(),
			response.status().canonical_reason()
		)
	}
}

// JSON from ipinfo is not valid JSON, we need to parse it into valid JSON manually
async fn parse_json_to_vec(string: String) -> serde_json::Result<Vec<CountryRow>> {
	serde_json::from_str(&format!(
		"[{}]",
		string
			// Split at the end of every object
			.split("}\n{")
			// Skip all IPv6 netblocks
			.map_while(|x| {
				match x.contains("::") {
					true => None,
					false => Some(x),
				}
			})
			.map(|s| s.trim_matches(&['\n', '{', '}'][..]))
			.map(|s| format!("{{{}}}", s))
			// Collect everything
			.collect::<Vec<_>>()
			// Join everything with commas
			.join(",")
	))
}

pub async fn create_tables(pool: &PgPool) -> Result<(), sqlx::Error> {
	// Create table
	sqlx::query(
		"CREATE TABLE IF NOT EXISTS countries (
    		network CIDR,
    		country VARCHAR(255),
    		country_code VARCHAR(2),
    		asn VARCHAR(16),
    		company VARCHAR(255),
    		PRIMARY KEY(network)
    	)",
	)
	.execute(pool)
	.await?;

	// Use a GIST inet_ops index for the network column
	// This allows for really fast lookup times from my testing
	//
	// Countries table needs to exist before we can create an index on it
	sqlx::query("CREATE INDEX IF NOT EXISTS countries_table_index ON countries USING GIST (network inet_ops);")
		.execute(pool)
		.await?;

	Ok(())
}

async fn insert_json_to_table(pool: &PgPool) -> anyhow::Result<()> {
	let mut file = File::open("ipinfo.json")?;
	let mut string = String::new();
	file.read_to_string(&mut string)?;

	let json = parse_json_to_vec(string).await?;
	info!("JSON Parsed successfully.");

	let mut transaction = pool.begin().await?;

	let style = ProgressStyle::with_template(
		"[{elapsed_precise}] [{bar:40.white/blue}] {human_pos}/{human_len} {msg}",
	)
	.expect("failed to create progress bar style")
	.progress_chars("=>-");

	let bar = ProgressBar::new(json.len() as u64).with_style(style);
	bar.set_message("Inserting rows to countries table...");

	for netblock in json.into_iter().progress_with(bar) {
			if let Ok(cidr) = IpNet::from_str(&netblock.network) {
				let result = sqlx::query(
					"INSERT INTO countries (network, country, country_code, asn, company)
						VALUES ($1, $2, $3, $4, $5)
						ON CONFLICT (network) DO UPDATE SET
						country = EXCLUDED.country,
						country_code = EXCLUDED.country_code,
						asn = EXCLUDED.asn,
						company = EXCLUDED.company",
				)
				.bind(cidr)
				.bind(netblock.country)
				.bind(netblock.country_code)
				.bind(netblock.asn)
				.bind(netblock.company)
				.execute(&mut *transaction)
				.await;
	
				if let Err(e) = result {
					tracing::warn!("Error while inserting row into countries table: {}", e);
				}
			} else {
				tracing::warn!("Failed to parse network CIDR: {}", netblock.network);
			}
		}

	transaction.commit().await?;

	info!("All done!");
	Ok(())
}
