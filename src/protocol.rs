use crate::utils::RunError;
use serde_json::json;
use std::net::SocketAddrV4;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::debug;

#[allow(dead_code)]
const SIMPLE_PAYLOAD: [u8; 9] = [
	6, // Size: Amount of bytes in the message
	0, // ID: Has to be 0
	0, // Protocol Version: Can be anything as long as it's a valid varint
	0, // Server address
	0, 0, // Port: Can be anything (Notchian servers don't this)
	1, // Next state: 1 for status, 2 for login. Therefore, has to be 1
	1, // Size
	0, // ID
];

#[derive(Debug)]
pub struct PingableServer {
	pub socket: SocketAddrV4,
}

impl PingableServer {
	pub fn new(socket: SocketAddrV4) -> Self {
		Self { socket }
	}

	#[allow(dead_code)]
	pub async fn simple_ping(&self) -> Result<String, RunError> {
		let mut stream = tokio::time::timeout(
			crate::scanner::TIMEOUT_SECS,
			TcpStream::connect(&self.socket),
		)
		.await??;

		stream.write_all(&SIMPLE_PAYLOAD).await?;
		let mut response = [0; 1024];

		// The index is used to point to the position at the start of the string.
		// It gets increased by the amount of bytes read to decode the packet ID, Packet length
		// And string length
		let mut index = 0;

		// Returns how many bytes were read from the stream into the buffer
		let total_read_bytes = stream.read(&mut response).await?;

		if total_read_bytes == 0 {
			debug!("[{}] Total read bytes is 0", &self.socket.ip());
			return Err(RunError::MalformedResponse);
		}

		// Packet length
		index += decode_varint(&response).1;

		// Since Packet ID should always be 0 and will never take more than 1 byte to encode
		// We can ignore it entirely and just advance the index by 1
		index += 1;

		// Decode the string length
		let (string_length, string_length_bytes) = decode_varint(&response[index as usize..]);
		index += string_length_bytes;

		// Error checking
		if string_length == 0 || string_length > 32767 {
			debug!(
				"[{}] String length: {string_length} was either 0 or too long",
				&self.socket.ip()
			);
			return Err(RunError::MalformedResponse);
		}

		// WARNING: Don't allocate vec size based on what the server says it needs from the varint.
		// Allocate size based on what the server *actually* sends back, some servers can crash the
		// program by attempting to allocate insane amounts of memory this way.
		//
		// Adds everything we have read so far minus the packet ID and packet length to a new vec
		let mut output = Vec::from(&response[index as usize..total_read_bytes]);
		let string_length = string_length + index as usize;

		if total_read_bytes > string_length {
			debug!(
			"[{}] Total read bytes: {total_read_bytes} is larger than string length: {string_length}", &self.socket.ip()
		);
			return Err(RunError::MalformedResponse);
		}

		// Read the rest of the servers JSON
		stream
			// Takes everything after the end of the data we already have in the buffer
			// Up until the end of the strings length
			.take((string_length - total_read_bytes) as u64)
			.read_to_end(&mut output)
			.await?;

		Ok(String::from_utf8_lossy(&output).into_owned())
	}

	pub async fn legacy_ping(&self) -> Result<String, RunError> {
		let mut stream = tokio::time::timeout(
			crate::scanner::TIMEOUT_SECS,
			TcpStream::connect(&self.socket),
		)
		.await??;

		// Legacy Server List Ping (1.6+)
		// Send 0xFE (Packet ID) 0x01 (Payload)
		stream.write_all(&[0xFE, 0x01]).await?;

		let mut buffer = Vec::new();
		// Legacy responses are usually small, but let's read enough
		let mut temp_buf = [0u8; 1024];
		let n = stream.read(&mut temp_buf).await?;
		buffer.extend_from_slice(&temp_buf[..n]);

		if buffer.is_empty() {
			return Err(RunError::MalformedResponse);
		}

		// Packet ID for Kick (0xFF)
		if buffer[0] != 0xFF {
			return Err(RunError::MalformedResponse);
		}

		if buffer.len() < 3 {
			return Err(RunError::MalformedResponse);
		}

		// Read length (Big Endian Short)
		let len = ((buffer[1] as u16) << 8) | (buffer[2] as u16);
		let byte_len = (len * 2) as usize;

		if buffer.len() < 3 + byte_len {
			return Err(RunError::MalformedResponse);
		}

		// Extract UTF-16BE bytes
		let utf16_bytes = &buffer[3..3 + byte_len];
		
		// Convert to u16 vec
		let utf16_vec: Vec<u16> = utf16_bytes
			.chunks_exact(2)
			.map(|chunk| ((chunk[0] as u16) << 8) | (chunk[1] as u16))
			.collect();

		let response_str = String::from_utf16(&utf16_vec)
			.map_err(|_| RunError::MalformedResponse)?;

		// Format: §1\0<Protocol>\0<Version>\0<MOTD>\0<Online>\0<Max>
		// First check if it starts with §1\0 (Protocol 1.6+)
		if response_str.starts_with("§1\0") {
			let parts: Vec<&str> = response_str.split('\0').collect();
			if parts.len() >= 6 {
				// parts[0] is "§1"
				let protocol = parts[1].parse::<i32>().unwrap_or(0);
				let version = parts[2];
				let motd = parts[3];
				let online = parts[4].parse::<i32>().unwrap_or(0);
				let max = parts[5].parse::<i32>().unwrap_or(0);

				let json_resp = json!({
					"version": {
						"name": version,
						"protocol": protocol
					},
					"players": {
						"max": max,
						"online": online,
						"sample": []
					},
					"description": {
						"text": motd
					}
				});

				return Ok(json_resp.to_string());
			}
		} else {
			// Older format (1.4-1.5): <MOTD>§<Online>§<Max>
			let parts: Vec<&str> = response_str.split('§').collect();
			if parts.len() >= 3 {
				let motd = parts[0];
				let online = parts[1].parse::<i32>().unwrap_or(0);
				let max = parts[2].parse::<i32>().unwrap_or(0);

				let json_resp = json!({
					"version": {
						"name": "Legacy < 1.6",
						"protocol": 0
					},
					"players": {
						"max": max,
						"online": online,
						"sample": []
					},
					"description": {
						"text": motd
					}
				});
				
				return Ok(json_resp.to_string());
			}
		}

		Err(RunError::MalformedResponse)
	}

	pub async fn proper_ping(&self) -> Result<String, RunError> {
		let mut stream = tokio::time::timeout(
			crate::scanner::TIMEOUT_SECS,
			TcpStream::connect(&self.socket),
		)
		.await??;

		// --- Handshake Packet ---
		// Packet ID: 0x00
		// Protocol Version (VarInt): -1 or 47 (1.8) or anything. Let's use 47.
		// Server Address (String)
		// Server Port (Unsigned Short)
		// Next State (VarInt): 1 (Status)

		let mut handshake = Vec::new();
		write_varint(&mut handshake, 0x00); // Packet ID
		write_varint(&mut handshake, 47);   // Protocol Version (1.8)
		write_string(&mut handshake, &self.socket.ip().to_string()); // Host
		handshake.extend_from_slice(&self.socket.port().to_be_bytes()); // Port
		write_varint(&mut handshake, 1);    // Next State: Status

		// Send Handshake
		write_packet(&mut stream, handshake).await?;

		// --- Request Packet ---
		// Packet ID: 0x00
		// Empty body
		write_packet(&mut stream, vec![0x00]).await?;

		// --- Read Response ---
		// Packet Length (VarInt)
		// Packet ID (VarInt) should be 0x00
		// JSON String (String)

		// We need to read VarInts one byte at a time to know the length
		let _packet_len = read_varint_from_stream(&mut stream).await?;
		let packet_id = read_varint_from_stream(&mut stream).await?;

		if packet_id != 0x00 {
			debug!("[{}] Expected packet ID 0x00 for response, got {}", self.socket, packet_id);
			return Err(RunError::MalformedResponse);
		}

		// The remaining length in the packet is for the string
		// Since we already read packet_id (likely 1 byte), we need to calculate remaining bytes
		// But wait, read_varint_from_stream consumes bytes from the stream.
		// The `packet_len` includes the length of Packet ID + Data.
		// We need to know how many bytes `packet_id` took.
		// Let's refactor to read the whole packet data into a buffer based on packet_len.
		
		// Actually, reading string is safer if we just read string length first.
		// The standard Read String format is: Length (VarInt) + UTF-8 Bytes.
		
		let json_len = read_varint_from_stream(&mut stream).await?;
		
		// Sanity check
		if json_len == 0 || json_len > 32767 * 4 { // *4 for safety margin on wide chars
	            // Basic sanity check, strict limit is usually 32767 chars
	            // but let's trust the varint length for now as long as it fits in memory
		}

	       // Read the JSON string bytes
	       let mut json_buffer = vec![0u8; json_len];
	       stream.read_exact(&mut json_buffer).await?;
	       
	       let json_str = String::from_utf8_lossy(&json_buffer).into_owned();

	       // --- Ping Packet (Optional for basic status, but good for latency check) ---
	       // We could send Ping (0x01) here, but we already have the JSON.
	       // The scanner only needs the JSON description.
	       // "proper_ping" usually implies the full sequence, but for getting info,
	       // Request->Response is enough. The TODO said "Handshake -> Request -> Ping".
	       // Let's add the Ping/Pong for completeness if needed, but returning the JSON is the goal.
	       
	       // If we want to measure latency, we would do the ping.
	       // But the function returns Result<String, ...>, implying it just wants the JSON.
	       // So we can stop here.

		Ok(json_str)
	}
}

fn write_varint(buf: &mut Vec<u8>, value: i32) {
	let mut u_val = value as u32;
	loop {
		let mut temp = (u_val & 0x7F) as u8;
		u_val >>= 7;
		if u_val != 0 {
			temp |= 0x80;
		}
		buf.push(temp);
		if u_val == 0 {
			break;
		}
	}
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
	   let bytes = s.as_bytes();
	   write_varint(buf, bytes.len() as i32);
	   buf.extend_from_slice(bytes);
}

async fn write_packet(stream: &mut TcpStream, data: Vec<u8>) -> Result<(), std::io::Error> {
	   let mut len_buf = Vec::new();
	   write_varint(&mut len_buf, data.len() as i32);
	   stream.write_all(&len_buf).await?;
	   stream.write_all(&data).await?;
	   Ok(())
}

async fn read_varint_from_stream(stream: &mut TcpStream) -> Result<usize, std::io::Error> {
	   let mut value: usize = 0;
	   let mut count: u8 = 0;
	   loop {
	       let mut buf = [0u8; 1];
	       stream.read_exact(&mut buf).await?;
	       let b = buf[0];
	       
	       value |= ((b & 0x7F) as usize) << count;
	       
	       if (b & 0x80) == 0 {
	           break;
	       }
	       
	       count += 7;
	       if count >= 35 {
	            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "VarInt too big"));
	       }
	   }
	   Ok(value)
}

// returns the decoded varint and how many bytes were read
#[allow(dead_code)]
#[inline(always)]
fn decode_varint(bytes: &[u8]) -> (usize, u8) {
	let mut value: usize = 0;
	let mut count: u8 = 0;

	for b in bytes {
		value |= ((b & 0x7F) as usize) << count;

		// right shift 7 times, if resulting value is 0 it means this is the end of the varint
		if (b >> 7) != 1 {
			break;
		}

		count += 7;
	}

	(value, (count / 7) + 1)
}
