//! Simple TCP Multiplexer relay for `socks-agent` and `socks-clients`
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{Level, debug, error, info, warn};
use tracing_subscriber;
// mod tagged_data; // Import the TaggedData module
use clap::Parser;
// use tagged_data::{ClientId, TaggedData, generate_client_id};
use tagger::{ClientId, TaggedData, generate_client_id};

#[derive(Parser, Debug)]
#[command(name = "tcp-relay", version, about)]
struct Args {
    /// Address to listen on for socks-clients (default: 127.0.0.1:8080)
    #[arg(short = 'c', long = "for_client", default_value = "0.0.0.0:1080")]
    for_client: String,

    /// Target address to listen for reverse-socks agent to back-connect to (default: 0.0.0.0:8080)
    #[arg(short = 's', long = "for_socks_agent", default_value = "0.0.0.0:8080")]
    for_socks_agent: String,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Initialize tracing subscriber for logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO) // Set the maximum log level to INFO
        .with_thread_ids(true) // Include thread IDs in logs
        .with_thread_names(true) // Include thread names in logs
        .init(); // Initialize the tracing subscriber
    let args = Args::parse(); // Parse command line arguments
    // Listen for the 'socks-agent' connection
    info!("Starting Reverse-Socks TCP Multiplexer Relay...");
    debug!(
        "Listening for reverse-socks agent on {}",
        args.for_socks_agent
    );
    let listen_for_socks_agent = TcpListener::bind(format!("{}", args.for_socks_agent)).await?;

    let (socks_agent_stream, _) = listen_for_socks_agent.accept().await?; // Accept `Rsocks-Agent` Connection
    debug!(
        "Reverse-Socks Agent Connected From {} To {}",
        socks_agent_stream.peer_addr().unwrap(), // Propagate error if peer_addr fails,
        socks_agent_stream.local_addr().unwrap()  // Propagate error if local_addr fails
    );

    // Separate `socket` into read and write halves.
    let (socks_agent_reader, socks_agent_writer) = socks_agent_stream.into_split();

    // Initiate new MPSC channels for all the "clients" coroutines to communicate (Send data) with socks-agent.
    let (transmitter_to_socks_agent, received_to_socks_agent) =
        tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

    // Create a mapping object to map client IDs to transmitters (So we can send data to client by our choice)
    let maps_to_client_transmitter: Arc<
        Mutex<HashMap<ClientId, tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>,
    > = Arc::new(Mutex::new(HashMap::new()));

    // Listens for socks clients (socks-clients)
    let listen_to_clients = TcpListener::bind(format!("{}", args.for_client)).await?;
    debug!(
        "Listening for clients (socks4 clients) on {}",
        listen_to_clients.local_addr().unwrap() // Panic if local_addr fails
    );

    // Read data from Socks-Agent MPSC Channel (Receiver) and write (send bytes) to rsocks socket.
    tokio::spawn(write_to_socks_agent(
        socks_agent_writer,
        received_to_socks_agent,
    ));
    debug!(
        "Spawned write_to_socks_agent task. Write received data (from mpsc channel) to rsocks socket"
    );

    // Demultiplexes: Read from rsocks responses, demultiplexes by client ID, and "transmit" payload to relevant client MPSC channel.
    tokio::spawn(demux_from_socks_agent_to_clients(
        socks_agent_reader,
        maps_to_client_transmitter.clone(),
    ));
    debug!(
        "Spawned demux_from_socks_agent_to_clients task. Read from rsocks and send to clients (Send to each client MPSC Channel)"
    );

    // Enter "Infinite" loop to accept new clients
    loop {
        // Accept new client connections.
        let (client_socket, _) = listen_to_clients.accept().await?;
        let new_connection = client_socket.peer_addr().unwrap();
        info!(
            "New client connected from {:#?}",
            new_connection // Propagate error if peer_addr fails
        );

        // Generate a unique client ID for the new client.
        let client_id = generate_client_id(); // Generate a random client ID
        debug!(
            "Generated new client ID: {} For Connetion {}",
            client_id, new_connection
        );

        // Separate the new client socket into read and write halves.
        let (client_reader, client_writer) = client_socket.into_split();

        // Create new MPSC channels for this client. Allow the rsocks coroutine's to send data to this client.
        let (transmitter_to_client, received_from_rsocks) =
            tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        debug!(
            "Created new MPSC channel for client ID {} (Source: {})",
            client_id,
            client_reader.peer_addr().unwrap()
        );

        // Clone `maps_to_client_transmitter` to a another instance. So we can use this in code block.
        let newclient = maps_to_client_transmitter.clone();
        // Insert the new client transmitter into the map.
        {
            let mut maps = newclient.lock().unwrap();
            maps.insert(client_id, transmitter_to_client); // Set maps[client_id] = transmitter_to_client;
        } // Lock automatically released here.

        // Read data (raw bytes) from the client socket and send it to rsocks transmitter (MPSC Channel)
        tokio::spawn(forward_client_to_socks_agent(
            client_reader,
            transmitter_to_socks_agent.clone(), // Move a `clone` of the transmitter of the rsocks.
            client_id,
        ));
        debug!(
            "Spawned forward_client_to_socks_agent task for client ID: {}. Read data from client {} and send to rsocks",
            client_id, new_connection
        );

        // Write to clients data that was received by the MPSC channel of the dedicated client connection.
        tokio::spawn(write_back_to_clients(client_writer, received_from_rsocks));
        debug!(
            "Spawned write_back_to_clients task for client ID: {}. Write data to client {}",
            client_id, new_connection
        );
    }
}

/// Write data to the socks client (socket) from data received in the MPSC channel of the dedicated client
/// # Arguments
/// * `write_to_client` - The write half of the client socket to write data to
/// * `client_receiver` - The receiver to get data to write to the client socket
/// # Returns
/// * `()` - This function does not return a value, it runs indefinitely until the receiver is closed or an error occurs.
/// # Example
/// ```rust
/// let write_to_client = ...; // The write half of the client socket
/// let client_receiver = ...; // The receiver to get data to write to the client socket
/// write_back_to_clients(write_to_client, client_receiver).await;
/// ```
/// # Note
/// This function is asynchronous and should be awaited. It runs in a separate task to handle writing
/// to the client socket. It writes data received from the MPSC channel to the client socket.
/// If the receiver is closed or an error occurs, it will print an error message and exit the
async fn write_back_to_clients(
    mut write_to_client: tokio::net::tcp::OwnedWriteHalf, // Write half of the client socket
    mut client_receiver: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>, // Receiver to get data to write
) {
    // This is a separate task to handle the writing to the client socket
    loop {
        // Receive data from the MPSC channel
        match client_receiver.recv().await {
            Some(data) => {
                // Write the data received from the MPSC channel to the client socket
                if write_to_client.write_all(&data).await.is_err() {
                    error!(
                        "Failed to write to client socket {}",
                        write_to_client.peer_addr().unwrap()
                    );
                    // Socket write error, "exit" the loop (End coroutine)
                    break;
                }
                debug!(
                    "Wrote {} bytes to client socket `{}`",
                    data.len(),
                    write_to_client.peer_addr().unwrap()
                );
            }
            None => {
                warn!(
                    "Receiver closed, stopping write task: Client Disconnected {}",
                    write_to_client.peer_addr().unwrap()
                );
                break; // Exit if the receiver is closed
            }
        }
    }
}

/// Demultiplexes data received from rsocks and sends it to the appropriate client (MPSC Channel) based on the client tag.
/// # Arguments
/// * `socks_agent_reader` - The read half of the rsocks socket to read data from
/// * `maps_to_client_transmitter` - A map of client IDs to transmitters
/// # Returns
/// * `()` - This function does not return a value, it runs indefinitely until the rsocks socket is closed or an error occurs.
/// # Example
/// ```rust
/// let socks_agent_reader = ...; // The read half of the rsocks socket
/// let maps_to_client_transmitter = ...; // A map of client IDs to transmitters
/// demux_from_socks_agent_to_clients(socks_agent_reader, maps_to_client_transmitter).await;
/// ```
/// # Note
/// This function is asynchronous and should be awaited. It runs in a separate task to handle reading
/// from the rsocks socket and sending data to the appropriate client MPSC channel.
/// It reads tagged data from the rsocks socket, extracts the client tag, and sends the data to the corresponding client transmitter.
/// If the rsocks socket is closed or an error occurs, it will print an error message and exit the loop.
async fn demux_from_socks_agent_to_clients(
    mut socks_agent_reader: tokio::net::tcp::OwnedReadHalf, // Read half of the rsocks socket)
    maps_to_client_transmitter: Arc<
        Mutex<HashMap<ClientId, tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>,
    >, // Map of client IDs to transmitters
) {
    // This is a separate task to handle the reading from the rsocks socket

    loop {
        match TaggedData::from_read_socket(&mut socks_agent_reader).await {
            // Read the tagged data from rsocks
            Ok(tagged_data) => {
                debug!("Read back from socks agent {:#?}", tagged_data);

                // Get the transmitter for the client ID (By mapping client_tag to transmitter)
                let transmitter: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>> = {
                    let maps = maps_to_client_transmitter.lock().unwrap();
                    maps.get(&tagged_data.client_tag).cloned()
                };

                if let Some(tx) = transmitter {
                    // Sender exists in maps_to_client_transmitter
                    if tx.send(tagged_data.data).is_err() {
                        // Failure to send data to client
                        error!("Failed to send data to client {}", tagged_data.client_tag);
                    }
                    // Data was sent to the client successfully
                    debug!("Sent data to client[{:#?}] (MPSC)", tagged_data.client_tag);
                } else {
                    // No transmitter to relay data back to client.
                    error!(
                        "No transmitter found for client[{:#?}]. Data: {:#?} will be dropped.",
                        tagged_data.client_tag,
                        String::from_utf8_lossy(&tagged_data.data)
                    );
                }
            }
            Err(e) => {
                error!("Failure to read from socks-agent: {:#?}", e);
                break; // Exit on error
            }
        }
    }
}

/// Write data to rsocks (socket). Data is received from the MPSC channel.
/// # Arguments
/// * `writer` - The write half of the rsocks socket to write data to
/// * `rsocks_receiver` - The receiver to get data to write to the rsocks   socket
/// # Returns
/// * `()` - This function does not return a value, it runs indefinitely until the receiver is closed or an error occurs.
/// # Example
/// ```rust
///     let writer = ...; // The write half of the rsocks socket    
///     let rsocks_receiver = ...; // The receiver to get data to write to the rsocks socket
///     write_to_socks_agent(writer, rsocks_receiver).await;
/// ```
async fn write_to_socks_agent(
    mut socks_agent_writer: tokio::net::tcp::OwnedWriteHalf, // Write half of the socket
    mut rsocks_receiver: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>, // Receiver to get data to write
) {
    // This is a separate task to handle the writing to the socket
    loop {
        // Read from 'rsocks_receiver' MPSC channel
        match rsocks_receiver.recv().await {
            Some(data) => {
                if socks_agent_writer.write_all(&data).await.is_err() {
                    error!(
                        "Failed to write to socket {}",
                        socks_agent_writer.peer_addr().unwrap()
                    );
                    break;
                }
                info!(
                    "write_to_socks_agent: Wrote {} bytes to socket {}",
                    data.len(),
                    socks_agent_writer.peer_addr().unwrap()
                );
            }
            None => {
                warn!("Receiver closed, stopping write task");
                break; // Exit if the receiver is closed
            }
        }
    }
}

/// Read data from client socket and send (forward) it to rsocks transmitter.
/// # Arguments
/// * `client_reader` - The read half of the client socket to read data from
/// * `rsocks_transmitter` - The transmitter to send data to rsocks
/// * `id` - The client ID for tagging the data. Basically a u32
/// # Returns
/// * `()` - This function does not return a value, it runs indefinitely until the client socket is closed or an error occurs.
/// # Example
/// ```rust
/// let client_reader = ...; // The read half of the client socket
/// let rsocks_transmitter = ...; // The transmitter to send data to rsocks
/// let id = 123; // The client ID for tagging
/// forward_client_to_socks_agent(client_reader, rsocks_transmitter, id).await;
/// ```
/// # Note
/// This function is asynchronous and should be awaited. It runs in a separate task to handle reading
/// from the client socket and sending data to the rsocks transmitter.
/// It reads data in chunks of 1024 bytes and sends it to the rsocks transmitter.
/// If the client socket is closed or an error occurs, it will print an error message and exit the loop.
async fn forward_client_to_socks_agent(
    mut client_reader: tokio::net::tcp::OwnedReadHalf, // Read half of the socket
    rsocks_transmitter: tokio::sync::mpsc::UnboundedSender<Vec<u8>>, // Transmitter to send data to rsocks
    id: ClientId,                                                    // Client ID for tagging
) {
    // This is a separate task to handle the reading from the client socket
    loop {
        let mut buf = [0u8; 1024]; // Buffer to read data into
        match client_reader.read(&mut buf).await {
            Ok(0) => {
                warn!(
                    "Client socket {} reached EOF, closing connection",
                    client_reader.peer_addr().unwrap()
                );
                break; // Connection closed
            }
            Ok(n) => {
                debug!(
                    "Read `{n}` bytes from client {}",
                    client_reader.peer_addr().unwrap()
                );
                let tagged_data = TaggedData {
                    client_tag: id,          // Generate a random client tag
                    data: buf[..n].to_vec(), // Convert the buffer to a vector
                };
                // Transmit a `TaggedData` to the rsocks transmitter (So it can be forwarded)
                if rsocks_transmitter
                    .send(tagged_data.to_bytes().await)
                    .is_err()
                {
                    error!(
                        "Failed to send data {:#?} to rsocks transmitter",
                        tagged_data
                    );
                    break;
                }
            }
            Err(e) => {
                error!("Failed to read from client socket; err = {:#?}", e);
                break;
            }
        }
    }
}
