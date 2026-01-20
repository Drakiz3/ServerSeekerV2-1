use crate::config::BotConfig;
use crate::database::{BotServerDetails, Database, ScanCandidate};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::sleep;
use tracing::{error, info, warn};

#[derive(Debug, Serialize, Deserialize)]
struct BotResponse {
    status: String,
    online: bool,
    version: Option<String>,
    plugins: Option<Vec<String>>,
    chat: Option<Vec<String>>,
    reason: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct BotRequest {
    host: String,
    port: u16,
    version: Option<String>,
}

pub struct BotScanner {
    config: BotConfig,
    database: Database,
    client: Client,
}

impl BotScanner {
    pub fn new(config: BotConfig, database: Database) -> Self {
        Self {
            config,
            database,
            client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    pub async fn start(&self) {
        if !self.config.enabled {
            info!("Bot scanner is disabled in config.");
            return;
        }

        info!("Starting Bot Scanner...");

        // Try to check if bot is already running by pinging the port?
        // Or just try to spawn. 
        // For robustness, let's try to spawn if we can't connect.
        // But simply spawning is safer for this task's requirements.

        let script_path = &self.config.script_path;
        let api_port = self.config.api_port;

        let child = Command::new("node")
            .arg(script_path)
            .env("PORT", api_port.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        match child {
            Ok(mut child_proc) => {
                info!("Bot process started with PID: {:?}", child_proc.id());
                // Monitor process in background
                tokio::spawn(async move {
                    match child_proc.wait().await {
                        Ok(status) => error!("Bot process exited with status: {}", status),
                        Err(e) => error!("Bot process error: {}", e),
                    }
                });
            }
            Err(e) => {
                warn!("Failed to spawn bot process: {}. Assuming external bot is running or node is missing.", e);
            }
        }

        // Give it some time to start
        sleep(Duration::from_secs(5)).await;

        self.scan_loop().await;
    }

    async fn scan_loop(&self) {
        info!("Entering bot scan loop...");
        loop {
            // Fetch candidates
            let candidates = match self.database.get_bot_scan_candidates(50).await {
                Ok(c) => c,
                Err(e) => {
                    error!("Failed to fetch bot scan candidates: {}", e);
                    sleep(Duration::from_secs(10)).await;
                    continue;
                }
            };

            if candidates.is_empty() {
                info!("No candidates found. Sleeping for 30s...");
                sleep(Duration::from_secs(30)).await;
                continue;
            }

            info!("Processing {} candidates...", candidates.len());

            for candidate in candidates {
                self.process_candidate(candidate).await;
            }
        }
    }

    async fn process_candidate(&self, candidate: ScanCandidate) {
        let ip_str = candidate.address.addr().to_string();
        let port = candidate.port as u16;
        let url = format!("http://localhost:{}/join", self.config.api_port);

        let request = BotRequest {
            host: ip_str.clone(),
            port,
            version: candidate.version.clone(),
        };

        info!("Scanning {}:{} with bot...", ip_str, port);

        match self.client.post(&url).json(&request).send().await {
            Ok(resp) => {
                if resp.status().is_success() {
                    match resp.json::<BotResponse>().await {
                        Ok(bot_res) => {
                            let details = BotServerDetails {
                                plugins: bot_res.plugins.unwrap_or_default(),
                                world_info: None, // Bot doesn't return this yet
                                detailed_version: bot_res.version,
                                auth_type: None, // Bot doesn't return this explicitly yet
                                join_success: bot_res.online,
                            };

                            if let Err(e) = self.database.save_server_details(candidate.address, candidate.port, details).await {
                                error!("Failed to save server details for {}: {}", ip_str, e);
                            } else {
                                info!("Saved details for {}:{} (Success: {})", ip_str, port, bot_res.online);
                                
                                self.database.log_event(
                                    Some(candidate.address),
                                    "INFO".to_string(),
                                    "BOT_SCAN_COMPLETE".to_string(),
                                    format!("Bot scan finished. Success: {}", bot_res.online)
                                );
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse bot response for {}: {}", ip_str, e);
                        }
                    }
                } else {
                    error!("Bot API returned error status: {}", resp.status());
                }
            }
            Err(e) => {
                error!("Failed to contact Bot API for {}: {}", ip_str, e);
            }
        }
        
        // Small delay to not overwhelm the bot if concurrency is not handled
        sleep(Duration::from_millis(500)).await;
    }
}
