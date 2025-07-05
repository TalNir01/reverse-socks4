# Reverse Socks: Relay Server

Relay Server is integral part in `Reverse Socks` Setup.

```bash
                                               |=======================|
|===============|                              |                       |                              |===============|
| Socks Client  | === [Initiate Socket] ===>[8080] TCP Relay Server [8081] <=== [Initiate Socket] === | R-Socks Agent |
|===============|                              |                       |                              |===============|
                                               |=======================|
```