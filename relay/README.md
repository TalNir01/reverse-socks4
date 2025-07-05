# Reverse SOCKS Multiplexer Relay

This project is a **Reverse SOCKS Multiplexer Relay** written in Rust using Tokio for async networking.  
It allows multiple SOCKS clients to communicate with a single backend agent (the "relay backend") over a single TCP connection, multiplexing and demultiplexing traffic by client ID.

---

## Features

- Accepts multiple SOCKS client connections on port `8080`
- Accepts a single backend agent connection on port `8081`
- Tags and routes data between clients and backend using unique client IDs
- Uses async tasks and MPSC channels for efficient, concurrent data handling
- Structured logging with `tracing`

---

## How It Works

1. **Startup**
    - Listens for a backend agent on port `8081`
    - Listens for SOCKS clients on port `8080`

2. **Client Connection**
    - Assigns a unique client ID
    - Sets up MPSC channels for communication

3. **Data Flow**
    - **Client → Relay → Backend:**  
      Data from each client is tagged with its ID and sent to the backend agent.
    - **Backend → Relay → Client:**  
      Data from the backend is tagged with a client ID and routed to the correct client.

---

## Pseudo-code

```text
Start relay server
  Wait for backend agent connection on port 8081
  Split backend socket into read and write halves

  Create a map: client_id -> client_sender_channel

  Spawn task: write_to_backend
    Loop: receive data from channel, write to backend socket

  Spawn task: demux_from_backend
    Loop: read tagged data from backend socket
      Extract client_id
      Send payload to client_sender_channel[client_id]

  Loop: accept new client connections on port 8080
    Assign unique client_id
    Split client socket into read and write halves
    Create MPSC channel for this client
    Insert client_id -> sender_channel into map

    Spawn task: forward_client_to_backend
      Loop: read from client socket
        Tag data with client_id
        Send to backend channel

    Spawn task: write_to_client
      Loop: receive data from channel, write to client socket
```

---

## Usage

1. Build and run the relay server.
2. Connect your backend agent to port `8081`.
3. Connect SOCKS clients to port `8080`.
4. The relay will automatically route traffic between clients and the backend.

---

## Notes

- All data is tagged with a 4-byte client ID and a 4-byte length prefix.
- The relay uses async Rust and is designed for high concurrency and efficiency.
- Logging is enabled at the DEBUG level for troubleshooting.

---

## License