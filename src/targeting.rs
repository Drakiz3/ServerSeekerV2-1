use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::info;

const BASE_URL: &str = "https://raw.githubusercontent.com/herrbischoff/country-ip-blocks/master/ipv4/";
const CACHE_TTL_SECS: u64 = 7 * 24 * 60 * 60; // 7 days

pub async fn fetch_country_cidrs(country_code: &str) -> Result<PathBuf> {
    let country_code = country_code.to_lowercase();
    
    let cache_dir = Path::new("cache");
    if !cache_dir.exists() {
        fs::create_dir(cache_dir).context("Failed to create cache directory")?;
    }

    let file_name = format!("{}.txt", country_code);
    let file_path = cache_dir.join(&file_name);

    // Check if cache exists and is fresh
    let mut use_cache = false;
    if file_path.exists() {
        match fs::metadata(&file_path) {
            Ok(metadata) => match metadata.modified() {
                Ok(modified) => match SystemTime::now().duration_since(modified) {
                    Ok(age) => {
                        if age.as_secs() < CACHE_TTL_SECS {
                            use_cache = true;
                        } else {
                            info!("Cache for {} is expired (age: {:?}), downloading fresh copy", country_code, age);
                        }
                    }
                    Err(_) => info!("System time seems to be before file modification time, forcing download"),
                },
                Err(e) => info!("Could not get modification time for cache file: {}, forcing download", e),
            },
            Err(e) => info!("Could not get metadata for cache file: {}, forcing download", e),
        }
    }

    if use_cache {
        info!("Using cached CIDR list for {}", country_code);
        return Ok(file_path);
    }

    let url = format!("{}{}.cidr", BASE_URL, country_code);
    info!("Downloading CIDR list for {} from {}", country_code, url);

    let response = reqwest::get(&url)
        .await
        .context("Failed to download CIDR list")?
        .error_for_status()
        .context("Server returned error")?;

    let content = response
        .text()
        .await
        .context("Failed to get response text")?;

    fs::write(&file_path, content).context("Failed to write CIDR list to file")?;

    Ok(file_path)
}
