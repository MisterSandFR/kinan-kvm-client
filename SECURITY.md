# Security

## Short version

* **Only run the device on an isolated management network.** A dedicated
  VLAN that routes to nothing else is the baseline.
* **Never expose it on the public internet** and never on a shared office
  or guest LAN.
* **Reach it remotely only through a VPN or an SSH tunnel.**
* **Change factory credentials.**

## Why: the device is unauthenticated on the wire

The switch exposes several TCP services. The web administration on port 80
asks for a username and password. The three TCP services that actually
move pixels and keypresses — **the port-switching channel on TCP 7777, the
H.264 video stream on TCP 202 and the keyboard/mouse channel on TCP 203**
— **do not authenticate anyone**. Any device that can open a TCP connection
to those ports can:

* ask the switch to display any of the 16 ports to any remote watcher,
* read the live video of every console, and
* send arbitrary keystrokes and mouse events to the host currently
  selected, as if sitting in front of its keyboard.

This is a property of the appliance. This project **did not introduce**
this weakness; it exists in firmware shipped to customers. Documenting it
publicly is intentional: operators should know what they are deploying.
Hiding the fact would be worse.

## What this client does not do

* It does not add authentication where the protocol has none. It cannot.
* It does not relay traffic to the device. All networking is local to the
  machine that runs the binary (optionally through an SSH tunnel the user
  sets up).
* It does not send keyboard or mouse packets unless the operator has
  explicitly allowed input on the target port and confirmed enabling
  input in the current session.

## What the client does do to help

* The first launch shows an opt-in notice explaining the exposure.
* The host-address field triggers a warning whenever the value is not a
  loopback, RFC1918 private, link-local or CGNAT/Tailscale address.
* Keyboard and mouse are disabled by default. Enabling them is per port,
  needs a confirmation, and is dropped as soon as the switch confirms a
  different port than the one that was requested.

## Reporting a vulnerability in this client

If you find a security issue **in this client itself** (as opposed to the
device's protocol), please report it privately before filing a public
issue. Contact: open a GitHub Security Advisory on this repository.

We will acknowledge within a reasonable delay and coordinate a fix. If the
issue involves the appliance's firmware, please also contact the vendor;
we do not claim to represent them.

## Supported versions

The main branch is the only supported line until the project reaches a
stable release.
