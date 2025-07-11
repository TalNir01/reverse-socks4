use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{Level, debug, error, info};
use tracing_subscriber::{self};
// mod tagged_data; // Import the tagged_data module
// use tagged_data::{ClientId, TaggedData};
use tagger::{ClientId, TaggedData};
mod socks_specification; // Import the socks_constants module
use clap::Parser;
use socks_specification::Socks4Request; // Import SOCKS4_VERSION from socks_constants

/// Your application description
#[derive(Parser, Debug)]
#[command(name = "reverse-socks-agent", version, about)]
struct Args {
    /// Address to listen on (default: 127.0.0.1:8080)
    #[arg(short, long, default_value = "127.0.0.1:8080")]
    relay_server_addr: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO) // Set the maximum log level to INFO
        .with_thread_ids(true) // Include thread IDs in logs
        .with_thread_names(true) // Include thread names in logs
        .init(); // Initialize the tracing subscriber
    let args = Args::parse(); // Parse command line arguments
    let connect_to_relay = TcpStream::connect(format!("{}", args.relay_server_addr))
        .await
        .unwrap();
    info!(
        "Connected to relay server at {}",
        connect_to_relay.peer_addr().unwrap()
    );
    // Split the TcpStream into read and write halves
    let (relay_reader, relay_writer) = connect_to_relay.into_split();

    // Generate MPSC
    let (transmit_to_relay, received_from_targets_send_to_relay) =
        tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    // Spawn a corotuine that will read data rom mpsc channel and write it back to socket.
    tokio::spawn(from_targets_to_relay(
        relay_writer,
        received_from_targets_send_to_relay,
    ));
    let maps_to_client_transmitter: Arc<
        Mutex<HashMap<ClientId, tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>,
    > = Arc::new(Mutex::new(HashMap::new()));
    tokio::spawn(read_from_relay_write_to_clients_mpsc(
        relay_reader,
        transmit_to_relay.clone(),
        maps_to_client_transmitter.clone(),
    ));

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }

    // Now spawn a new client
}

/// Handles the socks agent connection to the target server.
/// This function connects to the target server and handles the communication between the client and the target.
/// It reads data from the client, sends it to the target server, and reads the response from the target server to send it back to the client.
/// # Arguments
/// * `client_receiver` - The receiver for client data.
/// * `send_result_to_relay` - The transmitter to send data back to the relay.
/// * `child_id` - The client ID for logging purposes.
/// # Returns
/// This function does not return any value. It runs indefinitely, handling the client connection.
/// # Errors
/// This function will log errors if it fails to connect to the target server or if there are errors during communication.
/// # Example
/// ```rust
/// let (client_receiver, send_result_to_relay) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
/// let child_id = 1; // Example client ID
/// tokio::spawn(handle_client_connection_to_target(client_receiver, send_result_to_relay, child_id));
/// ```
async fn handle_client_connection_to_target(
    mut client_receiver: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    send_result_to_relay: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    client_id: ClientId,
) {
    // TODO: implement Socks4 protocol support
    // Connect to the target server
    debug!(
        "Handling client connection to target with ID: {}",
        client_id
    );
    let first_packet = match client_receiver.recv().await {
        Some(data) => {
            info!(
                "Reading first packet from client {}: {} bytes. Expecting SOCKS4 request",
                client_id,
                data.len()
            );
            data
        }
        None => {
            error!(
                "Client {} disconnected before sending SOCKS4 request",
                client_id
            );
            return;
        }
    };

    // 2. Parse the SOCKS4 request
    let socks_request: Socks4Request = match Socks4Request::from_bytes(&first_packet) {
        Ok(req) => req,
        Err(e) => {
            error!(
                "Failed to parse SOCKS4 request from client {}: {}",
                client_id, e
            );
            // Optionally send a SOCKS4 failure response here
            return;
        }
    };

    let target_address = format!("{}:{}", socks_request.dst_ip, socks_request.dst_port);
    // 3. Try to connect to the requested target
    let target_stream = match TcpStream::connect(&target_address).await {
        Ok(stream) => {
            info!("Connected to target server at {}", target_address);
            // Send SOCKS4 success response to client
            let response = socks_request.request_granted().await;
            let tagged_response = TaggedData {
                client_tag: client_id,
                data: response.to_bytes(),
            };
            if send_result_to_relay
                .send(tagged_response.to_bytes().await)
                .is_err()
            {
                error!(
                    "Failed to send SOCKS4 success response to client {}",
                    client_id
                );
                return;
            }
            stream
        }
        Err(e) => {
            error!(
                "Failed to connect to target server {}: {}",
                target_address, e
            );
            // Send SOCKS4 failure response to client
            let response = socks_request.request_rejected().await;
            let tagged_response = TaggedData {
                client_tag: client_id,
                data: response.to_bytes(),
            };
            let _ = send_result_to_relay.send(tagged_response.to_bytes().await);
            return;
        }
    };

    let (target_reader, mut target_writer) = target_stream.into_split();
    let send_result_to_relay = send_result_to_relay.clone();
    // Read from target and write to relay
    tokio::spawn(write_target_response_to_relay_mpsc(
        target_reader,
        send_result_to_relay,
        client_id,
    ));
    debug!("Spawned write_target_response_to_relay_mpsc task");
    // Sends data to socks target

    // Forward any remaining data from the first packet (section)
    let payload = socks_request.remaining_data.clone();
    if !payload.is_empty() {
        debug!(
            "Forwarding initial payload (leftover from socks-handshake) to target: {:?}",
            String::from_utf8_lossy(&payload)
        );
        if let Err(e) = target_writer.write_all(&payload).await {
            error!("Failed to write initial payload to target: {}", e);
        }
    } else {
        debug!("No initial payload to forward to target");
    }

    // Spawn a coroutine that will write incoming data to target stream.
    tokio::spawn(read_from_receiver_write_to_target(
        client_receiver,
        target_writer,
    ));
    info!("Spawned read_from_receiver_write_to_target task");
}

async fn write_target_response_to_relay_mpsc(
    mut target_reader: tokio::net::tcp::OwnedReadHalf, // Read half of the target socket
    send_result_to_socks_agent: tokio::sync::mpsc::UnboundedSender<Vec<u8>>, // Transmitter to send data to rsocks
    client_id: ClientId, // Client ID for logging purposes
) {
    loop {
        // NOTE: This can be optimized to read data in chunks
        let mut buffer = vec![0; 1024]; // Buffer to read data from target
        match target_reader.read(&mut buffer).await {
            Ok(0) => {
                debug!("write_target_response_to_relay_mpsc:: Target connection closed");
                break; // Exit the loop if the connection is closed
            }
            Ok(n) => {
                buffer.truncate(n); // Truncate the buffer to the number of bytes read
                debug!(
                    "write_target_response_to_relay_mpsc:: Read {} bytes from target",
                    n
                );
                let tagged_response = TaggedData {
                    client_tag: client_id,
                    data: buffer.clone(),
                };
                debug!(
                    "Sending tagged response to reverse-socks agent: {:?}",
                    tagged_response
                );
                match send_result_to_socks_agent.send(tagged_response.to_bytes().await) {
                    Ok(_) => debug!("Data sent to reverse-socks agent successfully"),
                    Err(e) => {
                        error!("Failed to send data to reverse-socks agent: {}", e);
                        break; // Exit the loop on error
                    }
                }
            }
            Err(e) => {
                error!("Error reading from target: {}", e);
                break; // Exit the loop on error
            }
        }
    }
}

/// Reads data from the client receiver and writes it to the target socket.
async fn read_from_receiver_write_to_target(
    mut client_receiver: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>, // Receiver for client data
    mut target_writer: tokio::net::tcp::OwnedWriteHalf, // Write half of the rsocks socket
) {
    // Block forever reading from the client receiver
    debug!(
        "Starting to read from client receiver {:#?}",
        target_writer.peer_addr().unwrap()
    );
    while let Some(data) = client_receiver.recv().await {
        // Write the data to the target writer
        if target_writer.write_all(&data).await.is_err() {
            error!("Failed to write data to target socket");
            break; // Exit the loop on error
        } else {
            debug!(
                "Data of {} bytes, was written to target socket {:#?} successfully",
                data.len(),
                target_writer.peer_addr().unwrap()
            );
        }
    }
}

async fn read_from_relay_write_to_clients_mpsc(
    mut relay_reader: tokio::net::tcp::OwnedReadHalf, // Read half of the rsocks socket)
    send_result_to_relay: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    maps_to_client_transmitter: Arc<
        Mutex<HashMap<ClientId, tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>,
    >, // Map of client IDs to transmitters
) {
    loop {
        // Read TaggedData from the reverse-socks agent reader
        //let tagged_data = TaggedData::from_read_socket(&mut relay_reader).await;
        debug!("Reading from relay socket");
        match TaggedData::from_read_socket(&mut relay_reader).await {
            Ok(data) => {
                error!("Received tagged data: {:#?}", data);
                // Get the transmitter for the client ID
                debug!("Looking for transmitter for client {}", data.client_tag);

                let not_new_client = {
                    maps_to_client_transmitter
                        .lock()
                        .unwrap()
                        .contains_key(&data.client_tag)
                };

                if not_new_client {
                    // If the transmitter exists, send the data
                    debug!("Transmitter found for client {}", data.client_tag);
                    let transmitter = maps_to_client_transmitter
                        .lock()
                        .unwrap()
                        .get(&data.client_tag)
                        .cloned();
                    if let Some(transmitter) = transmitter {
                        if transmitter.send(data.data).is_err() {
                            error!("Failed to send data to client {}", data.client_tag);
                        } else {
                            debug!("Data sent to client {}", data.client_tag);
                        }
                    } else {
                        error!("No transmitter found for client {}", data.client_tag);
                    }
                } else {
                    info!("No transmitter found for client {}", data.client_tag);
                    // We will generate new (receiver, transmitter) MPSC channel for this client.
                    let (client_transmitter, client_receiver) =
                        tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
                    let another_transmitter = client_transmitter.clone();
                    {
                        // Insert the new transmitter into the map. So next time we will able to communicate with target easily..
                        maps_to_client_transmitter
                            .lock()
                            .unwrap()
                            .insert(data.client_tag, client_transmitter);
                        info!("Created new transmitter for client {}", data.client_tag);
                    }
                    if another_transmitter.send(data.data).is_err() {
                        error!("Failed to send data to client {}", data.client_tag);
                    } else {
                        debug!("Data sent to client {}", data.client_tag);
                    }
                    // Spawn a new task to handle the client connection
                    tokio::spawn(handle_client_connection_to_target(
                        client_receiver,
                        send_result_to_relay.clone(),
                        data.client_tag,
                    ));
                }
            }
            Err(e) => {
                error!("Error reading tagged data: {}", e);
                break; // Exit the loop on error
            }
        }
    }
}

async fn from_targets_to_relay(
    mut relay_writer: tokio::net::tcp::OwnedWriteHalf,
    mut received_from_all_targets: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
) {
    while let Some(data) = received_from_all_targets.recv().await {
        debug!(
            "Received data from targets: {:#?}",
            String::from_utf8_lossy(&data)
        );
        // Write the data to the target writer
        if relay_writer.write_all(&data).await.is_err() {
            error!(
                "Failed to write data back to relay: Data {:#?}",
                relay_writer.peer_addr().unwrap()
            );
            break; // Exit the loop on error
        } else {
            debug!(
                "Data {} bytes, was written to relay server {:#?} successfully",
                data.len(),
                relay_writer.peer_addr().unwrap()
            );
        }
    }
}
