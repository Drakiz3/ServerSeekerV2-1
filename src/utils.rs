use thiserror::Error;

#[derive(Debug, Error)]
pub enum RunError {
	#[error("Failed to parse address")]
	AddressParseError(#[from] std::net::AddrParseError),
	#[error("I/O error")]
	IOError(#[from] std::io::Error),
	#[error("Malformed response")]
	MalformedResponse,
	#[error("Error while parsing response")]
	ParseResponse(#[from] serde_json::Error),
	#[error("Connection timed out")]
	TimedOut(#[from] tokio::time::error::Elapsed),
	#[error("Server opted out of scanning")]
	ServerOptOut,
	#[error("Error while updating server in database")]
	DatabaseError(#[from] sqlx::Error),
}

impl From<RunError> for usize {
	fn from(value: RunError) -> Self {
		use RunError::*;

		match value {
			AddressParseError(_) => 0,
			IOError(_) => 1,
			MalformedResponse => 2,
			ParseResponse(_) => 3,
			TimedOut(_) => 4,
			ServerOptOut => 5,
			DatabaseError(_) => 6,
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinecraftColorCodes {
	Black,
	DarkBlue,
	DarkGreen,
	DarkAqua,
	DarkRed,
	DarkPurple,
	Gold,
	Gray,
	DarkGray,
	Blue,
	Green,
	Aqua,
	Red,
	LightPurple,
	Yellow,
	White,
	Reset,
	UnknownValue,
}

impl From<&str> for MinecraftColorCodes {
	fn from(s: &str) -> Self {
		use MinecraftColorCodes::*;

		match s {
			"black" => Black,
			"dark_blue" => DarkBlue,
			"dark_green" => DarkGreen,
			"dark_aqua" => DarkAqua,
			"dark_red" => DarkRed,
			"dark_purple" | "purple" => DarkPurple,
			"gold" => Gold,
			"gray" | "grey" => Gray,
			"dark_gray" | "dark_grey" => DarkGray,
			"blue" => Blue,
			"green" => Green,
			"aqua" => Aqua,
			"red" => Red,
			"pink" | "light_purple" => LightPurple,
			"yellow" => Yellow,
			"white" => White,
			"reset" => Reset,
			// Try to parse hex color
			s if s.starts_with('#') && s.len() == 7 => hex_to_nearest_legacy(s),
			_ => UnknownValue,
		}
	}
}

fn hex_to_nearest_legacy(hex: &str) -> MinecraftColorCodes {
	let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(0) as i32;
	let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(0) as i32;
	let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(0) as i32;

	let colors = [
		(MinecraftColorCodes::Black, 0, 0, 0),
		(MinecraftColorCodes::DarkBlue, 0, 0, 170),
		(MinecraftColorCodes::DarkGreen, 0, 170, 0),
		(MinecraftColorCodes::DarkAqua, 0, 170, 170),
		(MinecraftColorCodes::DarkRed, 170, 0, 0),
		(MinecraftColorCodes::DarkPurple, 170, 0, 170),
		(MinecraftColorCodes::Gold, 255, 170, 0),
		(MinecraftColorCodes::Gray, 170, 170, 170),
		(MinecraftColorCodes::DarkGray, 85, 85, 85),
		(MinecraftColorCodes::Blue, 85, 85, 255),
		(MinecraftColorCodes::Green, 85, 255, 85),
		(MinecraftColorCodes::Aqua, 85, 255, 255),
		(MinecraftColorCodes::Red, 255, 85, 85),
		(MinecraftColorCodes::LightPurple, 255, 85, 255),
		(MinecraftColorCodes::Yellow, 255, 255, 85),
		(MinecraftColorCodes::White, 255, 255, 255),
	];

	let mut min_dist = i32::MAX;
	let mut closest = MinecraftColorCodes::Reset;

	for (code, cr, cg, cb) in colors {
		let dr = cr - r;
		let dg = cg - g;
		let db = cb - b;
		let dist = dr * dr + dg * dg + db * db;

		if dist < min_dist {
			min_dist = dist;
			closest = code;
		}
	}

	closest
}

impl MinecraftColorCodes {
	pub fn get_code(&self) -> char {
		use MinecraftColorCodes::*;

		match self {
			Black => '0',
			DarkBlue => '1',
			DarkGreen => '2',
			DarkAqua => '3',
			DarkRed => '4',
			DarkPurple => '5',
			Gold => '6',
			Gray => '7',
			DarkGray => '8',
			Blue => '9',
			Green => 'a',
			Aqua => 'b',
			Red => 'c',
			LightPurple => 'd',
			Yellow => 'e',
			White => 'f',
			Reset => 'r',
			UnknownValue => 'r',
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_hex_conversion() {
		// Exact matches for legacy color definitions
		assert_eq!(MinecraftColorCodes::from("#FF5555"), MinecraftColorCodes::Red);
		assert_eq!(MinecraftColorCodes::from("#AA0000"), MinecraftColorCodes::DarkRed);
		assert_eq!(MinecraftColorCodes::from("#00AA00"), MinecraftColorCodes::DarkGreen);
		assert_eq!(MinecraftColorCodes::from("#FFFFFF"), MinecraftColorCodes::White);
		assert_eq!(MinecraftColorCodes::from("#000000"), MinecraftColorCodes::Black);
		
		// Approximate colors
		// #FF0000 (Pure Red) is actually closer to Dark Red (AA0000) than Red (FF5555) in Euclidean RGB
		assert_eq!(MinecraftColorCodes::from("#FF0000"), MinecraftColorCodes::DarkRed);
		assert_eq!(MinecraftColorCodes::from("#FE5555"), MinecraftColorCodes::Red); // Almost Red
		assert_eq!(MinecraftColorCodes::from("#111111"), MinecraftColorCodes::Black); // Almost black
	}
}
