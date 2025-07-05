# reverse-socks4

A Rust-based toolkit for reverse SOCKS4 proxying, including a relay server, SOCKS4 server, and a tagging library.

## Project Structure
- `relay/` — TCP relay server for forwarding connections
- `socks4-server/` — SOCKS4 server implementation
- `tagger/` — Library for tagging and handling data with metadata

## Binaries

### 1. Relay Server (`relay`)
A TCP relay that forwards connections between clients and agents.

**Usage:**
```bash
# Run relay server (default ports: 8080 for client, 8081 for agent)
./target/release/relay
```

**Options:**
- Configure ports and addresses via command-line arguments if supported (see `relay/README.md` or `--help`).

### 2. SOCKS4 Server (`socks4-server`)
Implements a SOCKS4 proxy server for client connections.

**Usage:**
```bash
# Run SOCKS4 server (default port: 1080)
./target/release/socks4-server
```

**Options:**
- Configure listening port and other options via command-line arguments if supported (see `socks4-server/README.md` or `--help`).

### 3. Tagger Library (`tagger`)
A Rust library for tagging data. Not a binary, but can be used as a dependency in your Rust projects.

**Usage:**
Add to your `Cargo.toml`:
```toml
[dependencies]
tagger = { path = "./tagger" }
```

## Building (Static Release Binaries)

To build all binaries in optimized, static mode:

1. Install the musl target for static linking:
```bash
rustup target add x86_64-unknown-linux-musl
```

2. Build in release mode for all workspace members:
```bash
cargo build --release --target x86_64-unknown-linux-musl
# Non static version
cargo build --release
```

3. The statically linked binaries will be in `target/x86_64-unknown-linux-musl/release/`.

4. (Optional) Further reduce binary size:
```bash
strip target/x86_64-unknown-linux-musl/release/relay
strip target/x86_64-unknown-linux-musl/release/socks4-server
```

## Example Network Diagram

```bash
                                               |=======================|
|===============|                              |                       |                              |===============|
| Socks Client  | === [Initiate Socket] ===>[8080] TCP Relay Server [8081] <=== [Initiate Socket] === | R-Socks Agent |
|===============|                              |                       |                              |===============|
                                               |=======================|
```

## License
MIT

