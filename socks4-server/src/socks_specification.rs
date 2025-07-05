use std::io::{self};
use std::net::Ipv4Addr;

pub mod socks4_status {
    /// Request granted
    pub const REQUEST_GRANTED: u8 = 0x5A;

    /// Request rejected or failed
    pub const REQUEST_REJECTED: u8 = 0x5B;

    // /// Request failed because client is not running identd
    // const IDENTD_NOT_RUNNING: u8 = 0x5C;

    // /// Request failed because identd could not confirm the user ID
    // const IDENTD_CONFIRM_FAILED: u8 = 0x5D;
}

/// Client request structure for SOCKS4 protocol
/// This is the structure that states the target connection details
#[derive(Debug)]
pub struct Socks4Request {
    // Always 0x04
    // pub version: u8,
    // 0x01 for CONNECT
    // pub command: u8,
    // Destination port (big-endian)
    pub dst_port: u16,
    // Destination IPv4 address
    pub dst_ip: Ipv4Addr,
    /// Null-terminated USERID
    // pub user_id: String,
    /// Remaining data after the USERID (if any)
    pub remaining_data: Vec<u8>,
}

// Server response structure for SOCKS4 protocol.
// After initiating a connection, the server responds with this structure.
// So the client knows the connection status and destination details.
#[derive(Debug)]
pub struct Socks4Response {
    // pub version: u8,      // Always 0x00 in response
    pub status: u8,       // Status (0x5A = success)
    pub dst_port: u16,    // Destination port (big-endian on the wire)
    pub dst_ip: Ipv4Addr, // Destination IP address
}

impl Socks4Response {
    /// Serialize (To bytes) the Socks4Response to a byte vector.
    /// This format is sentable over the network.
    pub fn to_bytes(&self) -> Vec<u8> {
        // 8 bytes total:
        let mut buf = Vec::new(); // Vector to hold bytes
        buf.push(0x00); // Version (0x00 for response) - Constant
        buf.push(self.status);
        buf.extend(&self.dst_port.to_be_bytes()); // Big-endian port
        buf.extend(&self.dst_ip.octets()); // IPv4 address as bytes (Big-endian)
        buf // Return the serialized bytes
    }
}

impl Socks4Request {
    pub fn from_bytes(buf: &[u8]) -> io::Result<Socks4Request> {
        if buf.len() < 8 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "SOCKS4 header too short",
            ));
        }
        let _version = buf[0];
        let _command = buf[1];
        let dst_port = u16::from_be_bytes([buf[2], buf[3]]);
        let dst_ip = Ipv4Addr::new(buf[4], buf[5], buf[6], buf[7]);

        // Find the null terminator for USERID
        let mut idx = 8;
        while idx < buf.len() && buf[idx] != 0 {
            idx += 1;
        }
        if idx == buf.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "USERID not null-terminated",
            ));
        }
        let _user_id = String::from_utf8(buf[8..idx].to_vec())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        // Remaining data is everything after the null byte
        let remaining_data = if idx + 1 < buf.len() {
            buf[idx + 1..].to_vec()
        } else {
            Vec::new()
        };

        Ok(Socks4Request {
            dst_port: dst_port,
            dst_ip: dst_ip,
            remaining_data: remaining_data,
        })
    }

    pub async fn request_granted(&self) -> Socks4Response {
        // Write the response to the writer
        let response = Socks4Response {
            status: socks4_status::REQUEST_GRANTED, // Success
            dst_port: self.dst_port,
            dst_ip: self.dst_ip,
        };
        response
    }

    pub async fn request_rejected(&self) -> Socks4Response {
        // Write the response to the writer
        let response = Socks4Response {
            status: socks4_status::REQUEST_REJECTED, // Failure
            dst_port: self.dst_port,
            dst_ip: self.dst_ip,
        };
        response
    }
}
