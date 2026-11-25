# Kinan KVM Client

> **Isolated management networks only.** The device this client talks to has
> **no authentication** on its video, switching and keyboard/mouse TCP ports.
> Anyone on the same network segment can see every console and type into
> every connected server. Deploy the switch on a dedicated management VLAN,
> never on a shared or public network. Reach it remotely only through a VPN
> or an SSH tunnel. See [`SECURITY.md`](SECURITY.md) for details.

A modern, cross-platform, open source desktop client for KVM-over-IP
switches manufactured by Kinan (also sold under several resellers'
badges). The vendor's own client requires Internet Explorer and an
ActiveX control; this one is a self-contained Rust program with an
eframe/egui interface, H.264 decoding via OpenH264, and translations in
English, French and Simplified Chinese.

A *lire en français :* [`README.fr.md`](README.fr.md).

![test pattern from the bundled simulator](docs/screenshots/test-pattern.png)

> The screenshot above is rendered by the bundled test-pattern simulator,
> not from a real console.

## What it does

* Login and device-profile screen, with saved profiles under
  `~/.config/kinan-kvm-client/` (and the platform-appropriate equivalents).
* Grid of the 16 ports with the reported power / "in session" status and
  user-defined labels.
* H.264 video viewer, with a resolution and frame-rate counter, and a
  "no signal" indicator.
* **Opt-in** keyboard and mouse: off by default, allowed per port, and
  guarded by a confirmation dialog. Each keyboard/mouse packet carries the
  session port and the client refuses to send anything bound to a different
  port.
* Ctrl+Alt+Del shortcut, release-on-close.
* Light and dark theme, English / French / Simplified Chinese.
* First-launch privacy notice, and a warning in the UI whenever the host
  does not look like a private, link-local, loopback or CGNAT/Tailscale
  address.

## Status

This is an early release. It aims to be publishable and useful, not
feature-complete:

* Video adjustments (brightness, contrast) and virtual media (`RemoteVM`)
  are not implemented.
* The protocol is validated against one firmware revision (see the "Tested
  firmware" section below). Reports of other revisions welcome.

## Install

Binary releases for Windows (and, where the toolchain allows, Linux) are
attached to each tag on GitHub Releases. Download the archive for your
platform, extract it, and run `kinan-kvm-client`.

## Build from source

Rust 1.75 or newer. The video stack uses the [`openh264`](https://crates.io/crates/openh264)
crate with the `source` feature, which compiles OpenH264 from the Cisco
BSD-2-Clause source on first build; you need a working C/C++ toolchain:

* **Windows**: the "Desktop development with C++" workload from
  the Visual Studio Build Tools, or a MinGW-w64 posix distribution on `PATH`.
* **Linux**: `gcc`, `g++`, `nasm`, plus whatever your distribution needs for
  `winit`/`glow` (`libgl1`, `libxkbcommon-x11-0`, `libwayland-dev`, ...).
* **macOS**: Xcode command-line tools.

Then:

```sh
cargo build --release
./target/release/kinan-kvm-client
```

For a cross-compiled Windows build from Linux, install
`mingw-w64` (both C and C++, posix flavour) and run

```sh
cargo build --release --target x86_64-pc-windows-gnu
```

## Simulator

The repository ships a tiny simulator — `kinan-sim` — that implements
enough of the protocol to drive the client without an actual device. It
serves a software-generated video test pattern, a port list and the
switching channel, and silently records keyboard/mouse packets. Use it
to develop, to run the test suite, or to take screenshots that are not
of a real console.

```sh
cargo run -p kinan-sim -- --bind 127.0.0.1 --web-port 8080 \
    --switch-port 8777 --video-port 8202 --km-port 8203
```

Then open the client and create a profile pointing at `127.0.0.1` with
those ports. `device_ip` can be `127.0.0.1` as well.

## Reaching a real switch

The device has no authentication; never expose it directly. The
recommended setup is:

* Put the switch on a management VLAN that routes nowhere else.
* On a workstation that is *not* on the management VLAN, open an SSH
  tunnel to a jump host inside it and point the client at the tunnel:

```sh
ssh -N \
  -L 127.0.0.1:18080:<kvm>:80 \
  -L 127.0.0.1:18202:<kvm>:202 \
  -L 127.0.0.1:18203:<kvm>:203 \
  -L 127.0.0.1:18777:<kvm>:7777 \
  <jump-host>
```

In the profile, set `host = 127.0.0.1`, the tunnel ports, and
`device_ip = <the device's real IP>`.

## Factory credentials

Many of these devices ship with the username `admin` and the password
`admin` out of the box, as documented by the manufacturer. Change them.
This client makes no assumption about what you use.

## Tested firmware

* `V1.01-F17050CA20B35` (model name `KVM-160219IP` style), 16-port variant.

## Not affiliated

This project is not affiliated with or endorsed by the device
manufacturer. Trademarks and model names are used only for
interoperability.

## License

Licensed under either of

* MIT license ([LICENSE-MIT](LICENSE-MIT))
* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

## Third-party notices

See [`NOTICE`](NOTICE) for the licenses of embedded third-party code.
