//! This module defines the TaggedData structure and its methods for encoding and decoding data with a client tag.
use rand::Rng;
use std::fmt::Debug;
use tokio::io::AsyncReadExt;
use tracing::debug;

// Define a type alias for ClientId
pub type ClientId = u32;

pub fn generate_client_id() -> ClientId {
    // Generate a new client ID (UUID) for the client
    // In this case, we use a simple counter or random number generator.
    // For simplicity, we return a random u32 as a placeholder.
    rand::rng().random_range(0..u32::MAX) as ClientId
}

// TaggedData structure - Dataclass for data tagged with a client identifier
pub struct TaggedData {
    pub client_tag: ClientId,
    pub data: Vec<u8>,
}

/// Implement Debug trait for TaggedData
/// This allows us to print TaggedData instances for debugging purposes.
/// It formats the output to show the client tag and the data in a readable format.
/// # Example
/// ```rust
/// let tagged_data = TaggedData { client_tag: 123, data: vec![1, 2, 3] };
/// println!("{:?}", tagged_data);
/// ```
impl Debug for TaggedData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TaggedData {{ client_tag: {}, data: {:?} }}",
            self.client_tag,
            String::from_utf8_lossy(&self.data)
        )
    }
}

/// Implement TaggedData methods for encoding and decoding.
impl TaggedData {
    /// Encode TaggedData into bytes for transmission.
    /// Format
    /// <ClientId: 4 bytes><Data Length: 4 bytes><Data: variable length>
    pub async fn to_bytes(&self) -> Vec<u8> {
        // Convert TaggedData to bytes for transmission
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&self.client_tag.to_le_bytes());
        bytes.extend_from_slice(&(self.data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.data);
        bytes
    }

    /// Read (Build) TaggedData from a read socket.
    /// This function reads the client tag, data length, and data from the socket.
    /// It expects the data to be in the format:
    /// <ClientId: 4 bytes><Data Length: 4 bytes><Data: variable length>
    /// # Arguments
    /// * `reader` - The read half of the socket to read data from
    /// # Returns
    /// * `TaggedData` - The TaggedData instance containing the client tag and data
    /// # Errors
    /// * Returns an error if reading from the socket fails or if the data format is incorrect
    /// # Example
    /// ```rust
    /// let tagged_data = TaggedData::from_read_socket(reader).await?;
    /// ```
    /// # Note
    /// This function is asynchronous and should be awaited.
    pub async fn from_read_socket(
        reader: &mut tokio::net::tcp::OwnedReadHalf,
    ) -> Result<Self, std::io::Error> {
        // Read TaggedData from the socket
        let mut tag_buf: [u8; 4] = [0u8; 4];
        if reader.read_exact(&mut tag_buf).await.is_err() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Failed to read client tag",
            ));
        }
        let client_tag = u32::from_le_bytes(tag_buf); // Extract client tag (UUID, 4 bytes / u32)

        let mut len_buf = [0u8; 4];
        if reader.read_exact(&mut len_buf).await.is_err() {
            // Read the length of the data
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Failed to read data length",
            ));
        }
        let data_len = u32::from_le_bytes(len_buf) as usize; // Convert to little endian

        let mut data = vec![0u8; data_len]; // read data (payload) into a buffer
        if reader.read_exact(&mut data).await.is_err() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "Failed to read data",
            ));
        }
        debug!(
            "Read TaggedData from socket: client_tag={}, data_len={}",
            client_tag, data_len
        );
        Ok(TaggedData { client_tag, data })
    }
}
