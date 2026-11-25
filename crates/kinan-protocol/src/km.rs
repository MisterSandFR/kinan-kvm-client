//! Keyboard/mouse channel (TCP 203).
//!
//! Every packet is 12 bytes: `[command, subcode, payload(8), port, sequence]`.
//! The port byte is tested by a transport-layer guard: a packet whose port
//! field does not match the owning session is refused before it leaves the
//! client.

use crate::{port_index, ProtocolError};

/// Fixed length of every KM packet.
pub const PACKET_LEN: usize = 12;

/// Primary command bytes.
pub mod cmd {
    /// Image adjustment (brightness, contrast, saturation).
    pub const IMAGE_ADJUST: u8 = 0x02;
    /// Resolution hint to scale the mouse coordinates against.
    pub const RESOLUTION: u8 = 0x05;
    /// HID keyboard report.
    pub const KEYBOARD: u8 = 0x11;
    /// Mouse report, relative mode.
    pub const MOUSE_RELATIVE: u8 = 0x50;
    /// Mouse report, absolute mode.
    pub const MOUSE_ABSOLUTE: u8 = 0x60;
    /// Change mouse mode.
    pub const MOUSE_MODE: u8 = 0x30;
    /// Release the port on session close.
    pub const RELEASE: u8 = 0x80;
}

/// Build a packet with the fixed layout, applying port/sequence slots.
pub fn pack(
    cmd: u8,
    subcode: u8,
    payload: &[u8],
    port: u8,
    sequence: u8,
) -> Result<[u8; PACKET_LEN], ProtocolError> {
    let _ = port_index(port)?;
    if payload.len() > 8 {
        return Err(ProtocolError::Malformed("km payload longer than 8 bytes"));
    }
    let mut out = [0u8; PACKET_LEN];
    out[0] = cmd;
    out[1] = subcode;
    out[2..2 + payload.len()].copy_from_slice(payload);
    out[10] = port;
    out[11] = sequence;
    Ok(out)
}

/// Keyboard report (`cmd::KEYBOARD`, subcode 0x08).
pub fn keyboard_report(report: [u8; 8], port: u8, sequence: u8) -> Result<[u8; PACKET_LEN], ProtocolError> {
    pack(cmd::KEYBOARD, 0x08, &report, port, sequence)
}

/// Mouse report (`cmd::MOUSE_ABSOLUTE`, subcode 0x06).
pub fn mouse_absolute(
    x: u16,
    y: u16,
    buttons: u8,
    wheel: i8,
    port: u8,
    sequence: u8,
) -> Result<[u8; PACKET_LEN], ProtocolError> {
    let mut payload = [0u8; 8];
    payload[0..2].copy_from_slice(&x.to_be_bytes());
    payload[2..4].copy_from_slice(&y.to_be_bytes());
    payload[4] = buttons & 0x07;
    payload[5] = wheel as u8;
    pack(cmd::MOUSE_ABSOLUTE, 0x06, &payload, port, sequence)
}

/// Resolution hint (`cmd::RESOLUTION`, subcode 0x04).
pub fn resolution(w: u16, h: u16, port: u8, sequence: u8) -> Result<[u8; PACKET_LEN], ProtocolError> {
    let mut payload = [0u8; 8];
    payload[0..2].copy_from_slice(&w.to_be_bytes());
    payload[2..4].copy_from_slice(&h.to_be_bytes());
    pack(cmd::RESOLUTION, 0x04, &payload, port, sequence)
}

/// Explicit release of the port before closing the TCP connection.
pub fn release(port: u8, sequence: u8) -> Result<[u8; PACKET_LEN], ProtocolError> {
    let _ = port_index(port)?;
    let mut out = [0u8; PACKET_LEN];
    out[0] = cmd::RELEASE;
    out[1] = 0x01;
    out[2] = port;
    out[10] = port;
    out[11] = sequence;
    Ok(out)
}

/// Transport guard: reject any packet whose port slot is not `expected`.
pub fn assert_port_matches(packet: &[u8; PACKET_LEN], expected: u8) -> Result<(), ProtocolError> {
    if packet[10] == expected {
        Ok(())
    } else {
        Err(ProtocolError::PortMismatch {
            requested: expected,
            confirmed: packet[10],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mouse_packet_has_absolute_command() {
        let p = mouse_absolute(512, 384, 0b001, 0, 6, 42).unwrap();
        assert_eq!(p[0], 0x60);
        assert_eq!(p[1], 0x06);
        assert_eq!(&p[2..4], &512u16.to_be_bytes());
        assert_eq!(&p[4..6], &384u16.to_be_bytes());
        assert_eq!(p[6], 0b001);
        assert_eq!(p[10], 6);
        assert_eq!(p[11], 42);
    }

    #[test]
    fn keyboard_report_wraps_payload() {
        let report = [0x02u8, 0, 0x04, 0, 0, 0, 0, 0];
        let p = keyboard_report(report, 6, 1).unwrap();
        assert_eq!(p[0], 0x11);
        assert_eq!(p[1], 0x08);
        assert_eq!(&p[2..10], &report);
    }

    #[test]
    fn release_packet() {
        let p = release(6, 0).unwrap();
        assert_eq!(p[0], 0x80);
        assert_eq!(p[1], 0x01);
        assert_eq!(p[2], 6);
        assert_eq!(p[10], 6);
    }

    #[test]
    fn port_guard_rejects_other_port() {
        let p = mouse_absolute(0, 0, 0, 0, 6, 0).unwrap();
        assert!(assert_port_matches(&p, 6).is_ok());
        assert!(assert_port_matches(&p, 7).is_err());
    }

    #[test]
    fn pack_rejects_long_payload() {
        let err = pack(0x60, 0x06, &[0u8; 9], 6, 0).unwrap_err();
        assert!(matches!(err, ProtocolError::Malformed(_)));
    }
}
