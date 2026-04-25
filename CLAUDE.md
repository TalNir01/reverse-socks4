# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Run

Cargo workspace (Rust edition 2024). All commands run from repo root.

```bash
cargo build                                   # debug build, all members
cargo build --release                         # size-optimized (opt-level="z", LTO, panic=abort, strip)
cargo build --release --target x86_64-unknown-linux-musl   # static binary (requires `rustup target add x86_64-unknown-linux-musl`)

cargo run -p relay -- --help
cargo run -p socks4-server -- --help

cargo test                                    # workspace-wide
cargo test -p tagger                          # one crate
cargo test -p tagger -- <test_name>           # one test (exact or substring)
cargo check                                   # fastest type/borrow check
cargo clippy --workspace --all-targets        # lint
cargo fmt                                     # format
```

The release profile is tuned for **size, not speed** (`opt-level = "z"`, `codegen-units = 1`, `panic = "abort"`). Don't change these casually — they exist to ship small static binaries.

## Architecture

Three-process reverse SOCKS4 system. Naming below uses code reality, not README wording (see "Naming gotchas").

```
SOCKS4 client  ──TCP──►  relay (1080)
                         relay (8080) ◄──TCP── reverse-socks agent  ──►  target servers
```

The **agent dials out** to the relay (so it works from inside firewalled networks). The relay multiplexes many SOCKS4 clients over the **single** agent connection.

### Crates

- **`tagger/`** — Shared library defining the wire framing used between relay ↔ agent:
  - `TaggedData { client_tag: u32, data: Vec<u8> }`
  - On-wire format: `<client_id: 4 bytes LE><data_len: 4 bytes LE><payload>`
  - `from_read_socket` is generic over `R: AsyncRead + Unpin`, so it works on both plain `OwnedReadHalf` and `tokio_rustls`-derived halves. Any change to the framing must land in both consumers.

- **`relay/`** (binary `relay`) — TCP multiplexer.
  - Accepts **one** agent on `--for_socks_agent` (default `0.0.0.0:8080`), then accepts many SOCKS4 clients on `--for_client` (default `0.0.0.0:1080`).
  - For each client: generates a random `u32` `ClientId`, creates an unbounded MPSC channel, and stores `client_id → Sender` in an `Arc<Mutex<HashMap<...>>>`.
  - Two long-running tasks per agent connection: `write_to_socks_agent` (drains a shared MPSC into the agent socket) and `demux_from_socks_agent_to_clients` (reads `TaggedData` frames from the agent and routes payloads via the per-client sender).
  - Two long-running tasks per client: `forward_client_to_socks_agent` (reads raw bytes, wraps in `TaggedData`, sends to the shared MPSC) and `write_back_to_clients` (drains the per-client MPSC into the client socket).
  - The relay does **no** SOCKS4 parsing — it's pure framed multiplexing.

- **`socks4-server/`** (binary `reverse-socks-agent`) — Despite the crate name, this is the **reverse-SOCKS agent**, not a SOCKS server.
  - Connects out to `--relay-server-addr` (default `127.0.0.1:8080`).
  - Reads `TaggedData` frames; the **first** frame for a new `client_tag` is treated as the SOCKS4 CONNECT request and parsed via `Socks4Request::from_bytes` (`socks_specification.rs`).
  - On parse success: dials the requested `dst_ip:dst_port`, sends a SOCKS4 success/failure response back through the same tagged channel, then bridges target ↔ relay byte streams. Any payload sitting after the null-terminated USERID in the first frame (`remaining_data`) is forwarded to the target before the bidirectional pump starts — clients that pipeline data after the SOCKS4 handshake (common) rely on this.
  - Per `client_tag`, three tasks run: `handle_client_connection_to_target` (one-shot SOCKS4 handshake + setup), `write_target_response_to_relay_mpsc` (target → relay), `read_from_receiver_write_to_target` (relay → target).
  - SOCKS4 only — SOCKS4a hostname resolution and SOCKS5 are not implemented.

### Concurrency model

- Tokio multi-thread runtime (`#[tokio::main]`) with `tokio::sync::mpsc::unbounded_channel` for fan-in/fan-out.
- The shared `client_id → Sender` map uses `std::sync::Mutex` (not `tokio::sync::Mutex`). This is fine because the lock is only held for `HashMap::get/insert/contains_key` — never across `.await`. Preserve that property when editing.
- Channels are **unbounded**, so a slow consumer cannot apply backpressure. A misbehaving target can grow memory unbounded; keep this in mind before adding new producers.

### Lifecycle quirks worth knowing

- The relay accepts **exactly one** agent and then enters the client-accept loop; if the agent disconnects, the relay does not currently re-accept. Spawning multiple agents is not supported.
- `ClientId` is a random `u32` with no collision check. Collisions are astronomically unlikely in practice but possible in long-running deployments.
- On the agent side, a client tag is registered in the map only **after** the first frame arrives — so the parse/connect path runs on `handle_client_connection_to_target`, not on the dispatch loop.

### Optional TLS on the agent ↔ relay link

Only the agent ↔ relay leg can be wrapped in TLS — the SOCKS4 client ↔ relay leg is always plain TCP (SOCKS4 has no TLS upgrade story). TLS is opt-in on both ends; if either side runs without TLS while the other has it on, the connection deadlocks (each side waiting to read protocol bytes the other will never send).

- Implemented with `tokio-rustls` (rustls 0.23 + ring crypto provider) — pure-Rust, builds cleanly with the musl static target.
- Cert/connector loading lives in `relay/src/tls.rs` and `socks4-server/src/tls.rs`; they're tiny and intentionally not shared via `tagger`.
- After accept/connect the stream is upgraded with `acceptor.accept` / `connector.connect`, then `tokio::io::split` produces halves that are boxed as `Box<dyn AsyncRead + Send + Unpin + 'static>` / `Box<dyn AsyncWrite + Send + Unpin + 'static>`. The agent-side helper functions (`write_to_socks_agent`, `demux_from_socks_agent_to_clients`, `from_targets_to_relay`, `read_from_relay_write_to_clients_mpsc`) take these boxed types — that lets a single function body serve both transports without monomorphization. Client/target-side helpers stay on `OwnedReadHalf` / `OwnedWriteHalf`.

Flags:

| End | Flag | Purpose |
|-----|------|---------|
| relay | `--tls-cert <PEM>` + `--tls-key <PEM>` | Both required to enable TLS on the agent listener; otherwise plain TCP. |
| agent | `--tls` | Enable TLS toward the relay. |
| agent | `--tls-domain <NAME>` | SNI / cert verification name. Defaults to the host portion of `--relay-server-addr`. |
| agent | `--tls-ca <PEM>` | Stack additional trust anchors on top of the bundled webpki Mozilla roots. |
| agent | `--tls-insecure` | Skip server-cert verification. **Testing/lab only.** Conflicts with `--tls-ca`. |

## Naming gotchas

- The crate `socks4-server` produces the binary `reverse-socks-agent` and is described in its own README as "relay-client". All three names refer to the **same component** — the reverse-SOCKS agent that dials the relay.
- Default ports in the **root README are stale**. Authoritative defaults live in `clap` derives:
  - `relay`: clients on `0.0.0.0:1080`, agent on `0.0.0.0:8080`
  - `reverse-socks-agent`: connects to `127.0.0.1:8080`
- Logging defaults to `INFO` level via `tracing-subscriber`; there is no env-var override wired up — change the `with_max_level` call if you need `DEBUG`.
