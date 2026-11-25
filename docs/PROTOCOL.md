# Wire protocol used by Kinan-made KVM-over-IP switches

This document describes the binary protocol spoken by Kinan-manufactured
KVM-over-IP appliances well enough to interoperate with them. It is written
from the outside, by observing packets and following the behaviour of the
vendor's official browser client. **No reverse-engineered source code,
decompilation output, or vendor-supplied binary is reproduced here.**

The values below have been validated against one commercial model. Different
firmware revisions may vary.

> This project is an independent client. It is not affiliated with or
> endorsed by the manufacturer. Names are used only for interoperability.

## 1. Threat model

The video, switching and keyboard/mouse TCP ports accept *any* client that
can open a TCP connection. No authentication, challenge or encryption takes
place on those ports. The device must therefore be reachable only from a
trusted management network. See `SECURITY.md`.

## 2. TCP ports

| Port | Role                                                        |
|------|-------------------------------------------------------------|
|   80 | JSON web API (login, port list), cookie session             |
|  202 | H.264 video stream                                          |
|  203 | Keyboard and mouse (12-byte packets)                        |
| 7777 | Remote-port switching (single-shot, one request per socket) |
| 8112 | Image adjustments (brightness, contrast, reported size)     |

Ports are numbered `1..=16`. The protocol uses the zero-based index `port - 1`
internally; the UI exposes the one-based number.

## 3. Web API (TCP 80)

Login and authenticated reads use ordinary HTTP with a session cookie named
`CSESSIONID`.

* `POST /kinan/Login?lang=en` — form body
  `username=<name>&password=<strEnc(password, "123456789")>`. The response
  JSON is `{"status":"true"}` on success and includes a `Set-Cookie: CSESSIONID=...`
  header. `strEnc` is described in §7.
* `GET /kinan/PortInfo?lang=en` — list of ports, each with
  * `sNum` — port number as a string, 1 to 16,
  * `cName` — user-defined label,
  * `cStatus` — `"1"` if the port is powered, `"0"` otherwise,
  * `availability` — `"close"` if free, `"connect"` if a session is in progress,
  * `control_pri_int` — `"2"` full control, `"1"` view only, `"3"` refused.
* `GET /kinan/SideBar?lang=en` — current user, firmware, model string.

Every authenticated endpoint accepts the session cookie in the standard way.
A `"status":"false"` reply means "session expired"; log in again.

## 4. Switching (TCP 7777)

Opened once per switch. The client writes 12 bytes, reads 12 bytes, and the
device closes the TCP connection.

### Request (12 bytes)

| Offset | Size | Value                             |
|-------:|-----:|-----------------------------------|
|      0 |    1 | `0x04`                            |
|      1 |    1 | `0x01`                            |
|      2 |    1 | port number (`1..=16`)            |
|   3..9 |    7 | zero                              |
|     10 |    1 | port number (same as offset 2)    |
|     11 |    1 | sequence byte (0 works)           |

### Reply (12 bytes)

| Offset | Size | Meaning                                                                 |
|-------:|-----:|-------------------------------------------------------------------------|
|      0 |    1 | `0x04`                                                                  |
|      1 |    1 | `0x03`                                                                  |
|      2 |    1 | **confirmed port index** (`port - 1`), authoritative                    |
|      3 |    1 | brightness setpoint echoed back                                         |
|      4 |    1 | contrast setpoint echoed back                                           |
|      5 |    1 | mouse mode: `1` → absolute (see §6), `0` → relative                     |
|   6..11|    6 | zero                                                                    |

The client **must** verify that byte 2 equals `port - 1` before opening the
video and keyboard/mouse channels. Two concurrent switch connections make the
device echo the index of the *previous* port, which is why sessions must be
serialised.

## 5. Video (TCP 202)

### Request (44 bytes)

| Offset | Size | Value                                           |
|-------:|-----:|-------------------------------------------------|
|      0 |    1 | `0x04`                                          |
|      1 |    1 | `0x01`                                          |
|      2 |    1 | encoding family: `2` for Annex B H.264          |
|   4..8 |    4 | device IPv4, network byte order                 |
| `0x14` |    2 | zero-based port index, big-endian               |
|   rest |      | zero                                            |

The device IPv4 is the one reported by the web API, not the host the TCP
connection was made to (which may be a tunnel endpoint).

### Frame header (16 bytes)

| Offset | Size | Field                                                      |
|-------:|-----:|------------------------------------------------------------|
|      0 |    1 | `0x90` marker                                              |
|      1 |    3 | frame counter, big-endian                                  |
|      4 |    4 | device-side timestamp (roughly seconds), big-endian        |
|      8 |    2 | width, big-endian                                          |
|     10 |    2 | height, big-endian                                         |
|     12 |    2 | `w0`, little-endian: payload length high, type, flag       |
|     14 |    2 | payload length low, little-endian                          |

The payload length is `((w0 & 0xFFF) << 16) | lo` bytes. The client rejects
lengths above `0xFA000`. A header full of `0xFF` bytes means the channel is
refused or busy. The `flag` field equals `3` when the device considers its
output channel occupied.

### Payload

H.264 Annex B byte stream, start codes `00 00 00 01`. Observed main profile
`profile_idc=77`, level 3.2, typical GOP of around 25 frames, around 40
frames per second on a text console.

* First NAL type `7` → sequence parameter set, treated as a keyframe.
* Type `5` → IDR slice (keyframe).
* Types `1` and `8` → inter-frame slices.
* Type `3` → ignored (not video).

Immediately after the request, the device drains a few seconds of frames
captured on the *previous* port. A robust client throws away any header that
was already in the TCP buffer and waits for the next keyframe before feeding
the decoder.

## 6. Keyboard and mouse (TCP 203)

The device accepts packets without a handshake. Every packet is exactly 12
bytes:

| Offset | Size | Field                                        |
|-------:|-----:|----------------------------------------------|
|      0 |    1 | command                                      |
|      1 |    1 | subcode / payload length                     |
|   2..9 |    8 | payload                                      |
|     10 |    1 | **port number of the session (1..=16)**      |
|     11 |    1 | sequence byte, incremented by the sender     |

A conforming client refuses any packet whose byte 10 does not match the
current session's port, as a belt-and-braces guard against "wrong window
focus" accidents.

### Commands

| Cmd    | Sub  | Payload                                                             | Meaning                                               |
|-------:|-----:|---------------------------------------------------------------------|-------------------------------------------------------|
| `0x05` | `04` | width BE16, height BE16, zeros                                      | resolution hint used to scale mouse coordinates       |
| `0x11` | `08` | USB HID boot keyboard report: `mods, 0, usage1..usage6`             | keyboard state                                        |
| `0x30` | `08` | `[mouse mode, port]`                                                | switch between absolute (`0x60`) and relative (`0x50`)|
| `0x50` | `06` | `X BE16, Y BE16, buttons, wheel i8`                                  | mouse report, relative                                |
| `0x60` | `06` | `X BE16, Y BE16, buttons, wheel i8`                                  | mouse report, absolute (preferred)                    |
| `0x80` | `01` | `[port]`                                                            | release the port on close                             |

Modifier byte: `0x01` Ctrl left, `0x02` Shift left, `0x04` Alt left, `0x08`
Meta/Win left; the high nibble mirrors the right-hand side. Mouse buttons:
bit 0 primary, bit 1 secondary, bit 2 middle. Wheel: `+1` up, `-1` down.
Coordinates in the absolute report are in pixels relative to the top-left
of the remote picture, so the client scales its own pointer position to
match the resolution advertised by the most recent video frame.

The device never sends anything on this channel. A client reconnects only
after an error.

## 7. `strEnc` login encoding (web only)

The login form sends the password processed by a routine the device's own
JavaScript bundle calls `strEnc`. It is a textbook DES-ECB encoder with two
deviations from FIPS 46-3:

* the final `L16/R16` swap of the Feistel network is omitted,
* the key is zero-padded to eight bytes rather than refused.

The output is the concatenation of each block's ciphertext as uppercase
hexadecimal. A pure-Rust implementation lives in `kinan-protocol::des`. **It
is only a compatibility shim for the login form and must not be used as a
general-purpose cryptographic primitive.**

## 8. Known limitations

* Multiple concurrent switch connections make the reply unreliable. Sessions
  must therefore be serialised.
* The device sends roughly three seconds of stale video from the previous
  session each time the channel is opened. The client discards it by waiting
  for the first header that arrives after a measurable idle period and then
  dropping everything until a keyframe.
* Image adjustments (brightness, contrast) can be written over TCP 8112 with
  the `0x20` command. This client only reads values back, to stay out of the
  way of operators.
* Virtual media (`RemoteVM`) is out of scope.
