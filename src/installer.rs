use anyhow::{Context, Result};
use std::fs;
use std::io::Cursor;
use std::path::Path;
use tracing::info;

#[cfg(target_os = "windows")]
const RUSTSCAN_URL: &str = "https://github.com/bee-san/RustScan/releases/download/2.4.1/x86_64-windows-rustscan.exe.zip";
#[cfg(target_os = "windows")]
const MASSCAN_URL: &str = "https://github.com/Arryboom/MasscanForWindows/blob/master/masscan64.exe?raw=true";

pub async fn install_binaries() -> Result<()> {
    if !cfg!(target_os = "windows") {
        return Ok(());
    }

    let bin_dir = Path::new("bin");
    if !bin_dir.exists() {
        fs::create_dir(bin_dir).context("Failed to create bin directory")?;
    }

    install_rustscan(bin_dir).await?;
    install_masscan(bin_dir).await?;

    Ok(())
}

#[cfg(target_os = "windows")]
async fn install_rustscan(bin_dir: &Path) -> Result<()> {
    let target_path = bin_dir.join("rustscan.exe");
    if target_path.exists() {
        return Ok(());
    }

    info!("Downloading RustScan from {}", RUSTSCAN_URL);
    let response = reqwest::get(RUSTSCAN_URL)
        .await
        .context("Failed to download RustScan")?
        .bytes()
        .await
        .context("Failed to get RustScan bytes")?;

    let reader = Cursor::new(response);
    let mut zip = zip::ZipArchive::new(reader).context("Failed to open RustScan zip")?;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        // O nome dentro do zip pode ser rustscan.exe ou algo como x86_64-windows-rustscan.exe
        if file.name().ends_with(".exe") {
            info!("Extracting {} to rustscan.exe", file.name());
            let mut outfile = fs::File::create(&target_path).context("Failed to create rustscan.exe")?;
            std::io::copy(&mut file, &mut outfile).context("Failed to write content to rustscan.exe")?;
            info!("RustScan installed successfully.");
            return Ok(());
        }
    }

    Err(anyhow::anyhow!("Could not find executable in RustScan zip"))
}

#[cfg(not(target_os = "windows"))]
async fn install_rustscan(_bin_dir: &Path) -> Result<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
async fn install_masscan(bin_dir: &Path) -> Result<()> {
    let target_path = bin_dir.join("masscan.exe");
    if target_path.exists() {
        return Ok(());
    }

    info!("Downloading Masscan from {}", MASSCAN_URL);
    let response = reqwest::get(MASSCAN_URL)
        .await
        .context("Failed to download Masscan")?
        .bytes()
        .await
        .context("Failed to get Masscan bytes")?;

    fs::write(&target_path, response).context("Failed to write masscan.exe")?;
    info!("Masscan installed successfully.");

    Ok(())
}

#[cfg(not(target_os = "windows"))]
async fn install_masscan(_bin_dir: &Path) -> Result<()> {
    Ok(())
}
