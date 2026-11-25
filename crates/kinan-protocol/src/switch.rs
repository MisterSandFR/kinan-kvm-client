//! Port-switch channel (TCP 7777, single-shot).
//!
//! The client writes a 12-byte request and reads a 12-byte reply from which
//! only the confirmed port index and the mouse mode are load-bearing.

use crate::{port_index, ProtocolError};

/// Length, in bytes, of a switch request or reply.
pub const PACKET_LEN: usize = 12;

/// Build a switch request for `port` (1..=16).
pub fn request(port: u8) -> Result<[u8; PACKET_LEN], ProtocolError> {
    let _ = port_index(port)?;
    let mut out = [0u8; PACKET_LEN];
    out[0] = 0x04;
    out[1] = 0x01;
    out[2] = port;
    out[10] = port;
    // Byte 11 is a sequence number that the switch echoes back loosely; 0 works.
    Ok(out)
}

/// Decoded fields of a switch reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reply {
    /// Zero-based port index confirmed by the switch.
    pub confirmed_index: u8,
    /// Brightness setpoint echoed by the switch.
    pub brightness: u8,
    /// Contrast setpoint echoed by the switch.
    pub contrast: u8,
    /// Mouse mode (1 = absolute 0x60, 0 = relative 0x50).
    pub absolute_mouse: bool,
}

/// Parse a 12-byte switch reply.
pub fn parse_reply(buf: &[u8]) -> Result<Reply, ProtocolError> {
    if buf.len() < PACKET_LEN {
        return Err(ProtocolError::Malformed("switch reply shorter than 12 bytes"));
    }
    if buf[0] != 0x04 || buf[1] != 0x03 {
        return Err(ProtocolError::Malformed("switch reply header mismatch"));
    }
    Ok(Reply {
        confirmed_index: buf[2],
        brightness: buf[3],
        contrast: buf[4],
        absolute_mouse: buf[5] != 0,
    })
}

/// Ensure that the port confirmed by the switch matches the requested one.
pub fn verify(reply: &Reply, requested_port: u8) -> Result<(), ProtocolError> {
    let want = port_index(requested_port)?;
    if reply.confirmed_index == want {
        Ok(())
    } else {
        Err(ProtocolError::PortMismatch {
            requested: requested_port,
            confirmed: reply.confirmed_index.saturating_add(1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_has_port_in_two_slots() {
        let r = request(6).unwrap();
        assert_eq!(r[0], 0x04);
        assert_eq!(r[1], 0x01);
        assert_eq!(r[2], 6);
        assert_eq!(r[10], 6);
        for i in [3, 4, 5, 6, 7, 8, 9, 11] {
            assert_eq!(r[i], 0, "byte {i} must be zero");
        }
    }

    #[test]
    fn request_rejects_bad_ports() {
        assert!(request(0).is_err());
        assert!(request(17).is_err());
    }

    #[test]
    fn parse_reply_extracts_fields() {
        let buf = [0x04, 0x03, 0x05, 0x60, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
        let r = parse_reply(&buf).unwrap();
        assert_eq!(r.confirmed_index, 5);
        assert_eq!(r.brightness, 0x60);
        assert_eq!(r.contrast, 0);
        assert!(r.absolute_mouse);
    }

    #[test]
    fn verify_accepts_matching_port() {
        let r = Reply {
            confirmed_index: 5,
            brightness: 0,
            contrast: 0,
            absolute_mouse: true,
        };
        assert!(verify(&r, 6).is_ok());
    }

    #[test]
    fn verify_rejects_wrong_port() {
        let r = Reply {
            confirmed_index: 0,
            brightness: 0,
            contrast: 0,
            absolute_mouse: true,
        };
        let err = verify(&r, 6).unwrap_err();
        assert!(matches!(
            err,
            ProtocolError::PortMismatch {
                requested: 6,
                confirmed: 1
            }
        ));
    }
}
