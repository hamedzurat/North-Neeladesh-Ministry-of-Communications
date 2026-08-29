# Frontend transport options

Research date: 2026-08-09. Scope: a Rust authoritative backend and Odin/raylib GUI, initially on one laptop or a LAN, with a later Raspberry Pi or ESP32 frontend.

## Decision

Use **two long-lived transport associations**:

1. **Control/state:** length-prefixed CBOR messages over one TCP connection.
2. **Audio:** RTP/L16 over one UDP socket, using 10 or 20 ms packets.

This is the lowest-risk combination across the intended platforms. Rust, Odin, Raspberry Pi/Linux, and ESP-IDF all have direct TCP and UDP support. Odin also has a first-party RFC 8949-compatible CBOR package. RTP supplies the small but important audio fields we would otherwise invent: sequence number, sampling timestamp, payload type, and source/stream identifier.

A single WebSocket carrying both CBOR and PCM is a reasonable laptop-only prototype, but is not the recommended contract: because WebSocket runs over TCP, one lost TCP segment or a backed-up audio send queue can delay later control messages. QUIC streams plus datagrams have better transport semantics, but Odin and ESP32 implementation support is not yet evidenced well enough to make QUIC the baseline.

“Long-lived/persistent” needs one correction: TCP, WebSocket, and QUIC have connections; UDP is connectionless. A UDP socket and an application-level RTP session can remain open for the run, but reconnect, ownership, and resynchronization are application protocol responsibilities in every option.

## Comparison

| Candidate | Behavior under loss | Library/platform evidence | Assessment |
| --- | --- | --- | --- |
| One WebSocket/CBOR connection for state and PCM | Reliable, ordered delivery for everything. Loss delays subsequent audio **and control** because the underlying TCP byte stream cannot expose later bytes first. Application queueing can create the same delay even without network loss. | Strong Rust and ESP32 support; no first-party Odin WebSocket package found. | Fine for a quick localhost prototype. Do not freeze it as the hardware-facing contract. |
| Framed TCP/CBOR control + UDP/RTP L16 audio | Control is reliable and ordered. Audio loss stays an audio gap; sequence numbers and timestamps allow detection, reordering, and paced playback without waiting for retransmission. | Strongest common denominator: first-party Odin TCP/UDP and CBOR; Rust standard/Tokio sockets and RTP crates; ESP-IDF BSD sockets. | **Recommended now.** |
| One QUIC/WebTransport-style session: reliable control stream + unreliable datagrams | Reliable streams are ordered individually; loss on one stream does not impose TCP-style head-of-line blocking on other streams. DATAGRAM frames are unreliable/unordered and share QUIC encryption and congestion control. | Good Rust QUIC support. No first-party Odin QUIC/WebTransport package or official ESP-IDF QUIC component was found. WebTransport-over-HTTP/3 is still an IETF draft, rather than an RFC, at the research date. | Attractive later, especially across changing network paths; too much integration risk now. |

## What the transports actually guarantee

TCP provides a reliable, in-order byte stream; it detects loss with sequence numbers, checks every segment with a mandatory checksum, and corrects loss by retransmission. It is connection-oriented but does not inherently detect liveness ([RFC 9293, sections 2.2 and 3.1](https://www.rfc-editor.org/rfc/rfc9293.html#section-2.2)). The checksum detects accidental corruption, not malicious modification. CBOR messages therefore still need a schema, size limits, and semantic validation. Add TLS if the link leaves a trusted machine/LAN or needs peer authentication.

WebSocket provides message framing, binary messages, Ping/Pong, and a closing handshake over a TCP connection ([RFC 6455](https://www.rfc-editor.org/rfc/rfc6455.html)). It does not remove TCP ordering or retransmission. A single WebSocket is workable at this data rate if the writer has bounded queues and always schedules control ahead of audio, but it cannot prevent network-level TCP head-of-line blocking. Splitting control and audio across two WebSockets would isolate application queues but still gives audio retransmission semantics that are usually undesirable for real-time playback.

UDP preserves datagram boundaries but provides no retransmission, duplicate suppression, ordering, or flow control. Its checksum is a 16-bit integrity check: optional in IPv4, required by default in IPv6, and not cryptographic ([RFC 8085, sections 3.3-3.4](https://www.rfc-editor.org/rfc/rfc8085.html#section-3.3)). Thus “UDP with error checking” means corrupted packets are normally discarded, not recovered. RTP on top supplies sequence/timing metadata; it does not make delivery reliable.

QUIC is connection-oriented, carries ordered byte streams, authenticates and encrypts packets, and supports connection migration between network paths ([RFC 9000, section 1](https://www.rfc-editor.org/rfc/rfc9000.html#section-1)). Multiple QUIC streams avoid cross-stream head-of-line blocking, though streams still share connection congestion capacity. The QUIC DATAGRAM extension is explicitly unreliable: DATAGRAM frames are not retransmitted and their order is not guaranteed ([RFC 9221, sections 1 and 5](https://www.rfc-editor.org/rfc/rfc9221.html)). QUIC path migration may preserve a live connection across address changes; it does not restore backend/frontend process state after a crash.

## Recommended wire shape

### Control/state over TCP

Frame each message as:

```text
u32be payload_length | CBOR payload
```

Set a conservative maximum frame size and close/reject on invalid lengths. The CBOR envelope should include at least:

```text
protocol_version
message_kind
session_id
controller_epoch
message_id
state_revision
payload
```

Use explicit maps/records described by a shared schema and golden test vectors. Do not serialize language-specific object layouts. In particular, Odin's CBOR package documents an Odin-specific tag for unions; avoid such extensions in the cross-language contract unless the Rust codec implements them deliberately. CBOR itself is standardized by [RFC 8949](https://www.rfc-editor.org/rfc/rfc8949.html); CDDL can describe the interoperable data model ([RFC 8610](https://www.rfc-editor.org/rfc/rfc8610.html)).

Application heartbeat and takeover semantics should sit above TCP:

- Backend grants exactly one `controller_epoch`; messages from an older epoch are stale.
- Frontend reconnects with its session identity and last applied state revision.
- Backend answers with a complete authoritative state snapshot, plus any persistent counters/ledgers the snapshot does not contain.
- Commands carry unique message IDs so retries can be deduplicated.
- On reconnect or takeover, allocate a new audio stream identifier/SSRC and discard packets from the old one.

The same resync protocol is needed with WebSocket or QUIC. A transport connection is not durable application state.

### Audio over UDP/RTP

At 24,000 samples/s, mono, signed 16-bit PCM:

```text
24,000 * 1 * 16 = 384,000 bit/s = 48,000 byte/s payload
```

Suggested packetizations:

| Packet time | Samples | PCM payload | Packets/s | IPv4 + UDP + 12-byte RTP total |
| --- | ---: | ---: | ---: | ---: |
| 10 ms | 240 | 480 B | 100 | 52,000 B/s = 416 kbit/s |
| 20 ms | 480 | 960 B | 50 | 50,000 B/s = 400 kbit/s |

The totals include 20-byte IPv4, 8-byte UDP, and RTP's 12-byte fixed header, but not link-layer overhead. For IPv6, add 20 bytes per packet: 432 kbit/s at 10 ms or 408 kbit/s at 20 ms. This is modest on a laptop or normal LAN. A 20 ms IPv4 RTP packet is 1,000 bytes, comfortably below the usual 1,500-byte Ethernet MTU without fragmentation.

Use standard RTP rather than an “RTP-like” custom header:

- RTP version 2, a dynamically assigned payload type (for example 96), sequence number, timestamp, and random SSRC ([RFC 3550, section 5.1](https://www.rfc-editor.org/rfc/rfc3550.html#section-5.1)).
- Negotiate the mapping as `L16/24000/1`; payload types 96-127 are reserved for dynamic assignment ([RFC 3551, section 6](https://www.rfc-editor.org/rfc/rfc3551.html#section-6)).
- Increment the RTP timestamp by the number of samples in each packet (240 or 480), even if a packet is later lost.
- L16 samples are signed two's-complement in network byte order, so a little-endian producer must byte-swap them on the wire ([RFC 2586, section 3](https://www.rfc-editor.org/rfc/rfc2586.html#section-3)).
- Start with a small bounded jitter buffer (a design hypothesis: two or three 20 ms packets), play by RTP timestamp, drop packets arriving after their playout deadline, and measure before fixing the buffer target.

RTP/UDP has no peer authentication or confidentiality by itself. On a trusted isolated LAN, bind the expected source address/port and negotiate the SSRC through the authenticated control channel. For an untrusted network, use SRTP or reconsider QUIC; a checksum is not a security mechanism.

A minimal custom audio header could encode `version`, `stream_id`, `sequence`, `sample_timestamp`, and `sample_count` in fewer or similar bytes. It offers no meaningful bandwidth win here and creates a proprietary protocol plus new wraparound, validation, and tooling work. RTP is the better default.

## Implementation support

### Rust backend

- The Rust standard library and Tokio expose maintained [`TcpStream`](https://docs.rs/tokio/latest/tokio/net/struct.TcpStream.html) and [`UdpSocket`](https://docs.rs/tokio/latest/tokio/net/struct.UdpSocket.html) APIs.
- [`minicbor`](https://github.com/twittner/minicbor) is an actively released CBOR codec and supports `no_std`; [`ciborium`](https://github.com/enarx/ciborium) is a Serde-oriented alternative. Pin a version and verify it against Odin golden vectors.
- [`tokio-tungstenite`](https://github.com/snapview/tokio-tungstenite) and [`axum`](https://github.com/tokio-rs/axum) provide maintained WebSocket server choices.
- The [`webrtc-rs/rtp`](https://github.com/webrtc-rs/rtp) crate implements RTP packet structures. The 12-byte subset is also small enough to encode explicitly against RFC 3550 if dependency scope matters.
- [`Quinn`](https://github.com/quinn-rs/quinn) is a maintained pure-Rust QUIC implementation with streams and application datagrams. [`wtransport`](https://github.com/BiagioFesta/wtransport) provides Rust WebTransport-over-HTTP/3 on Quinn.

### Odin/raylib GUI

- Odin's first-party [`core:net`](https://pkg.odin-lang.org/core/net/) package supports cross-platform TCP and UDP sockets on Windows, Linux, and macOS.
- Odin's first-party [`core:encoding/cbor`](https://pkg.odin-lang.org/core/encoding/cbor/) package encodes/decodes RFC 8949 CBOR and documents untrusted-input allocation limits.
- No WebSocket, RTP, QUIC, or WebTransport implementation was found in Odin's official core/vendor package catalogue. RTP's fixed header is straightforward to implement locally; a fully conforming WebSocket or QUIC stack is not. This evidence favors raw TCP + UDP for the current native GUI.

### Raspberry Pi and ESP32 later

A 64-bit Raspberry Pi running Raspberry Pi OS is an ordinary Arm64 Linux target. Odin officially supports Arm64 ([Odin FAQ](https://odin-lang.org/docs/faq/#what-architectures-does-odin-support)), Rust supports `aarch64-unknown-linux-gnu` ([Rust platform support](https://doc.rust-lang.org/rustc/platform-support.html)), and Raspberry Pi OS is Debian-based ([Raspberry Pi documentation](https://www.raspberrypi.com/documentation/usage/phone/)). TCP/UDP is therefore low risk; QUIC is plausible through Rust/Quinn but still needs an on-device build and latency test.

ESP-IDF's lwIP layer officially supports common BSD socket operations, including TCP/UDP send/receive and `SO_KEEPALIVE` ([ESP-IDF lwIP guide](https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-guides/lwip.html)). Espressif also maintains an [`esp_websocket_client`](https://components.espressif.com/components/espressif/esp_websocket_client) component, so a WebSocket frontend is feasible. No official Espressif QUIC/WebTransport component was found. Raw TCP/UDP plus a small RTP parser therefore has the clearest ESP32 path; actual simultaneous Wi-Fi receive, jitter-buffer, audio-output, and device-I/O performance remains a hardware prototype question.

## Claims still unproven

- The acceptable jitter-buffer depth and whether 10 or 20 ms packets best fit the audio API and later device scheduler.
- Packet loss, latency, and TCP head-of-line behavior on the actual exhibit LAN; localhost testing will not expose meaningful loss.
- Odin↔Rust CBOR interoperability for the final schema, especially integer widths, enums/unions, absent fields, and forward-compatible unknown fields.
- Whether the chosen ESP32 model has enough RAM/task budget for the jitter buffer and all Cabinet I/O; socket availability alone does not prove end-to-end performance.
- A maintained, production-suitable Odin QUIC/WebTransport binding or a supported ESP32 QUIC stack. None was established from first-party packages.
- Raspberry Pi QUIC performance and certificate provisioning. Architecture support makes it plausible, not measured.

## Practical next proof

Before any hardware commitment, build a laptop-only transport harness with the real Rust and Odin codecs: framed CBOR/TCP control plus 20 ms RTP/L16 UDP audio. Inject loss, delay, reordering, TCP disconnects, and frontend replacement; verify bounded control latency, clean audio degradation, epoch-based single-controller takeover, and complete resynchronization. Keep the application envelopes transport-independent so QUIC can be tested later without redesigning authoritative state.
