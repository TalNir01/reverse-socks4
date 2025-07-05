# relay-client

A reverse SOCKS4 agent that connects to a relay server and proxies client connections to target servers. This project is implemented in Rust using async I/O with Tokio and supports multiple concurrent clients via MPSC channels.

## Usage

Build and run the agent:

```sh
cargo build --release
./target/release/relay-client --relay-server-addr <RELAY_SERVER_ADDR>
```

- `--relay-server-addr` (or `-r`): Address of the relay server to connect to (default: `127.0.0.1:8080`).

Example:

```sh
./relay-client --relay-server-addr 192.168.1.100:9000
```

## Detailed Workflow

1. **Startup**:  
   The agent connects to the relay server at the specified address and splits the connection into read and write halves.

2. **Channel Setup**:  
   - Sets up an MPSC channel for sending data from targets back to the relay.
   - Maintains a map of client IDs to their respective transmitters for handling multiple clients.

3. **Relay Communication**:  
   - Spawns a task to forward data from targets to the relay server.
   - Spawns a task to read data from the relay and dispatch it to the appropriate client handler.

4. **Client Handling**:  
   - For each new client (identified by a unique tag), creates a new MPSC channel and spawns a handler.
   - The handler parses the SOCKS4 request, connects to the target server, and manages bidirectional data transfer.

5. **Data Flow**:  
   - Data from the relay is tagged and routed to the correct client handler.
   - Data from the client is forwarded to the target server.
   - Responses from the target server are tagged and sent back to the relay.

## Pseudo-code

```
main():
    connect to relay server
    split connection into relay_reader, relay_writer
    create MPSC channels for communication
    spawn from_targets_to_relay(relay_writer, received_from_targets)
    spawn read_from_relay_write_to_clients_mpsc(relay_reader, transmit_to_relay, client_map)
    loop forever

read_from_relay_write_to_clients_mpsc(relay_reader, send_result_to_relay, client_map):
    while true:
        tagged_data = read TaggedData from relay_reader
        if client_map contains tagged_data.client_tag:
            send data to existing client transmitter
        else:
            create new transmitter/receiver for client
            insert into client_map
            spawn handle_client_connection_to_target(client_receiver, send_result_to_relay, client_tag)

handle_client_connection_to_target(client_receiver, send_result_to_relay, client_id):
    first_packet = await client_receiver.recv()
    parse SOCKS4 request
    connect to target server
    if success:
        send SOCKS4 success response to relay
        split target_stream into reader/writer
        spawn write_target_response_to_relay_mpsc(target_reader, send_result_to_relay, client_id)
        forward any remaining data to target_writer
        spawn read_from_receiver_write_to_target(client_receiver, target_writer)
    else:
        send SOCKS4 failure response to relay

write_target_response_to_relay_mpsc(target_reader, send_result_to_socks_agent, client_id):
    while true:
        read data from target_reader
        send tagged response to relay

read_from_receiver_write_to_target(client_receiver, target_writer):
    while true:
        data = await client_receiver.recv()
        write data to target_writer

from_targets_to_relay(relay_writer, received_from_all_targets):
    while true:
        data = await received_from_all_targets.recv()
        write data to relay_writer
```

## Possible Errors

- **Connection Errors**:  
  - Failure to connect to the relay server or target server.
  - Network interruptions.

- **SOCKS4 Parsing Errors**:  
  - Malformed or incomplete SOCKS4 requests from clients.

- **Channel Errors**:  
  - Failure to send or receive data on MPSC channels (e.g., if a receiver is dropped).

- **I/O Errors**:  
  - Errors reading from or writing to sockets (e.g., connection reset, broken pipe).

- **Synchronization Errors**:  
  - Poisoned mutexes if a thread panics while holding a lock on the client map.

- **Resource Exhaustion**:  
  - Too many concurrent connections may exhaust system resources.

All errors are logged using the `tracing` crate for debugging and monitoring.

---

For more details,