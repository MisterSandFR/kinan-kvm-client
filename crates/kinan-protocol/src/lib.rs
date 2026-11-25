//! Pure, I/O-free protocol types for Kinan-made KVM-over-IP switches.
//!
//! This crate only builds and parses byte buffers. All networking lives in the
//! `kinan-transport` and client crates. Every function is deterministic and
//! unit-tested so transport-layer bugs cannot be confused with protocol bugs.
//!
//! The protocol is described in `docs/PROTOCOL.md`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::net::Ipv4Addr;

pub mod des;
pub mod hid;
pub mod km;
pub mod switch;
pub mod video;

/// Default TCP ports used by the switch.
pub mod ports {
    /// Video stream.
    pub const VIDEO: u16 = 202;
    /// Keyboard and mouse control.
    pub const KEYBOARD_MOUSE: u16 = 203;
    /// Remote port switching.
    pub const SWITCH: u16 = 7777;
    /// Image adjustments (brightness, etc.).
    pub const FRAME: u16 = 8112;
    /// JSON web API (login, port list).
    pub const WEB: u16 = 80;
}

/// Protocol-level errors.
#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    /// Port number is outside the supported range `1..=16`.
    #[error("invalid port number {0}; must be between 1 and 16")]
    PortOutOfRange(u16),
    /// A received packet did not match any known layout.
    #[error("malformed packet: {0}")]
    Malformed(&'static str),
    /// The switch reported a port index that does not match the request.
    #[error("switch reply confirmed port {confirmed} while {requested} was requested")]
    PortMismatch {
        /// Port number that was requested.
        requested: u8,
        /// Port number confirmed by the switch.
        confirmed: u8,
    },
}

/// Validate a port number and return its zero-based index.
pub fn port_index(port: u8) -> Result<u8, ProtocolError> {
    if (1..=16).contains(&port) {
        Ok(port - 1)
    } else {
        Err(ProtocolError::PortOutOfRange(port as u16))
    }
}

/// Re-exported here for convenience.
pub use hid::{Modifier, Modifiers};

/// IPv4 helper used by [`video::request_packet`].
pub fn ipv4_from_str(s: &str) -> Result<Ipv4Addr, std::net::AddrParseError> {
    s.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_index_valid() {
        for p in 1..=16u8 {
            assert_eq!(port_index(p).unwrap(), p - 1);
        }
    }

    #[test]
    fn port_index_rejects_out_of_range() {
        assert!(matches!(port_index(0), Err(ProtocolError::PortOutOfRange(_))));
        assert!(matches!(port_index(17), Err(ProtocolError::PortOutOfRange(_))));
    }
}
