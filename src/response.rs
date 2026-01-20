use crate::utils::MinecraftColorCodes;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[allow(dead_code)]
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Server {
	pub latency: Option<i32>,
	pub version: Version,
	pub favicon: Option<String>,
	pub players: Players,
	#[serde(rename = "description")]
	pub description_raw: Option<Value>,
	pub description_formatted: Option<String>,
	#[serde(rename = "preventsChatReports")]
	pub prevents_reports: Option<bool>,
	#[serde(rename = "enforcesSecureChat")]
	pub enforces_secure_chat: Option<bool>,
	#[serde(rename = "isModded")]
	pub modded: Option<bool>,
	// "forgeData" is for modern versions of forge
	// "modinfo" is for legacy versions of forge
	#[serde(rename = "forgeData", alias = "modinfo")]
	pub forge_data: Option<ForgeData>,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, PartialEq, Clone, Debug)]
pub struct Version {
	pub name: String,
	pub protocol: i32,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, PartialEq, Clone, Debug)]
pub struct Players {
	pub max: i32,
	pub online: i32,
	pub sample: Option<Vec<Player>>,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, PartialEq, Clone, Debug)]
pub struct Player {
	pub id: String,
	pub name: String,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, PartialEq, Clone, Debug)]
pub struct ForgeData {
	// "mods", is for modern versions of forge
	// "modList" is legacy forge versions
	#[serde(rename = "mods", alias = "modList")]
	pub mods: Vec<Mod>,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, PartialEq, Clone, Debug)]
pub struct Mod {
	#[serde(rename = "modId", alias = "modid")]
	pub id: String,
	#[serde(rename = "modmarker", alias = "version")]
	pub version: String,
}

impl Server {
	pub fn get_type(&self) -> &'static str {
		// Check for modded servers first, as they have distinct identifiers.
		// Neoforge sends an "isModded" field.
		if self.modded.is_some() {
			return "Neoforge";
		}

		// Forge sends a "forgeData" object for modern servers and "modinfo" for legacy versions.
		if self.forge_data.is_some() {
			return "Lexforge";
		}

		let version_name = self.version.name.to_lowercase();

		// The order of these checks is important. Forks often include the parent software's
		// name in their version string (e.g., Paper may contain "Spigot").
		// To ensure accuracy, we check for the most specific forks first before falling
		// back to the more generic ones.

		// Proxies - These are checked first as they are distinct from server jars.
		if version_name.contains("velocity") {
			return "Velocity";
		}
		if version_name.contains("waterfall") {
			return "Waterfall";
		}
		if version_name.contains("bungeecord") {
			return "Bungeecord";
		}

		// Paper and its forks (from most to least specific)
		if version_name.contains("leaves") {
			return "Leaves";
		}
		if version_name.contains("folia") {
			return "Folia";
		}
		if version_name.contains("purpur") {
			return "Purpur";
		}
		if version_name.contains("pufferfish") {
			return "Pufferfish";
		}
		if version_name.contains("paper") {
			return "Paper";
		}

		// Spigot and its base
		if version_name.contains("spigot") {
			return "Spigot";
		}
		if version_name.contains("bukkit") {
			return "Bukkit";
		}

		// Fallback for any other Java server if no specific type is identified.
		"Java"
	}

	// Has the user opted out of scanning?
	pub fn check_opt_out(&self) -> bool {
		match &self.description_formatted {
			Some(description) => String::from(description).contains("§b§d§f§d§b"),
			None => false,
		}
	}

	#[rustfmt::skip]
	pub fn build_formatted_description(&self, value: &Value) -> String {
		let mut output = String::new();

		match value {
			Value::String(s) => output.push_str(s),
			Value::Array(array) => {
				for value in array {
					output.push_str(&self.build_formatted_description(value));
				}
			}
			Value::Object(object) => {
				for (key, value) in object {
					match key.as_str() {
						"obfuscated" => {
							if let Some(b) = value.as_bool() {
								if b {
									output.push_str("§k")
								}
							}
						},
						"bold" => {
							if let Some(b) = value.as_bool() {
								if b {
									output.push_str("§l")
								}
							}
						},
						"strikethrough" => {
							if let Some(b) = value.as_bool() {
								if b {
									output.push_str("§m")
								}
							}
						},
						"underline" => {
							if let Some(b) = value.as_bool() {
								if b {
									output.push_str("§n")
								}
							}
						},
						"italic" => {
							if let Some(b) = value.as_bool() {
								if b {
									output.push_str("§o")
								}
							}
						},
						"color" => {
							if let Some(c) = value.as_str() {
								let color = MinecraftColorCodes::from(c);
								output.push_str(format!("§{}", color.get_code()).as_str())
							}
						},
						_ => (),
					}
				}

				// MiniMOTD can put the "extra" field before the text field, this causes some servers
				// using it to format incorrectly unless we specifically add the text AFTER
				// all other format codes but BEFORE the extra field
				if object.contains_key("text") {
					if let Some(text) = object.get("text") {
						if let Some(text) = text.as_str() {
							output.push_str(text);
						}
					}
				}

				if object.contains_key("extra") {
					if let Some(extra) = object.get("extra") {
						output.push_str(&self.build_formatted_description(extra));
					}
				}
			}
			_ => {}
		}

		output
	}
}
