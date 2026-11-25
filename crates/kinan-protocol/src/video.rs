//! Video channel (TCP 202).
//!
//! The client writes a 44-byte request after switching, then reads a stream of
//! frames made of a 16-byte header followed by an H.264 Annex B payload.

use std::net::Ipv4Addr;

use crate::{port_index, ProtocolError};

/// Length of the video request packet, in bytes.
pub const REQUEST_LEN: usize = 44;
/// Length of a per-frame video header, in bytes.
pub const HEADER_LEN: usize = 16;
/// Upper bound the client enforces on the payload length field.
pub const MAX_PAYLOAD_LEN: u32 = 0xFA_000;

/// Build the one-shot request that asks the switch to stream video.
///
/// The `kvm_ip` is the device's own IP as advertised in the web API; the
/// payload echoes it even when the TCP connection goes through a local tunnel.
pub fn request_packet(kvm_ip: Ipv4Addr, port: u8) -> Result<[u8; REQUEST_LEN], ProtocolError> {
    let idx = port_index(port)? as u16;
    let mut out = [0u8; REQUEST_LEN];
    out[0] = 0x04;
    out[1] = 0x01;
    out[2] = 0x02; // encoding family: Annex B H.264
    out[4..8].copy_from_slice(&kvm_ip.octets());
    out[0x14..0x16].copy_from_slice(&idx.to_be_bytes());
    Ok(out)
}

/// Decoded fields of a video frame header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// 24-bit, big-endian frame counter.
    pub counter: u32,
    /// Device-side timestamp (roughly seconds).
    pub timestamp: u32,
    /// Reported picture width in pixels.
    pub width: u16,
    /// Reported picture height in pixels.
    pub height: u16,
    /// Length of the following payload, in bytes.
    pub payload_len: u32,
    /// Encoded frame type: 0..3.
    pub frame_type: u8,
    /// Status flag, 0 = normal, 3 = channel busy, 0xFFFF header = refused.
    pub flag: u8,
}

/// Parse the 16-byte per-frame header.
pub fn parse_header(buf: &[u8]) -> Result<FrameHeader, ProtocolError> {
    if buf.len() < HEADER_LEN {
        return Err(ProtocolError::Malformed("video header shorter than 16 bytes"));
    }
    if buf[0] != 0x90 {
        return Err(ProtocolError::Malformed("video header missing 0x90 marker"));
    }
    let counter = u32::from_be_bytes([0, buf[1], buf[2], buf[3]]);
    let timestamp = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let width = u16::from_be_bytes([buf[8], buf[9]]);
    let height = u16::from_be_bytes([buf[10], buf[11]]);
    // Length is split: 12 low-order bits of the little-endian word at 12,
    // followed by the little-endian u16 at 14.
    let w0 = u16::from_le_bytes([buf[12], buf[13]]);
    let lo = u16::from_le_bytes([buf[14], buf[15]]);
    let payload_len = ((u32::from(w0 & 0x0FFF)) << 16) | u32::from(lo);
    Ok(FrameHeader {
        counter,
        timestamp,
        width,
        height,
        payload_len,
        frame_type: ((w0 >> 12) & 0b11) as u8,
        flag: ((w0 >> 14) & 0b11) as u8,
    })
}

/// Build a video frame header. Used by the simulator.
pub fn build_header(hdr: FrameHeader) -> [u8; HEADER_LEN] {
    let mut out = [0u8; HEADER_LEN];
    out[0] = 0x90;
    let c = hdr.counter.to_be_bytes();
    out[1] = c[1];
    out[2] = c[2];
    out[3] = c[3];
    out[4..8].copy_from_slice(&hdr.timestamp.to_be_bytes());
    out[8..10].copy_from_slice(&hdr.width.to_be_bytes());
    out[10..12].copy_from_slice(&hdr.height.to_be_bytes());
    let w0 = ((hdr.flag as u16 & 0b11) << 14)
        | ((hdr.frame_type as u16 & 0b11) << 12)
        | ((hdr.payload_len >> 16) as u16 & 0x0FFF);
    let lo = (hdr.payload_len & 0xFFFF) as u16;
    out[12..14].copy_from_slice(&w0.to_le_bytes());
    out[14..16].copy_from_slice(&lo.to_le_bytes());
    out
}

/// Classify a NAL unit at the start of an Annex B payload.
///
/// Returns `None` when the payload is empty, truncated, or does not begin with
/// an Annex B start code.
pub fn first_nal_type(payload: &[u8]) -> Option<u8> {
    if payload.len() >= 5 && payload[..4] == [0, 0, 0, 1] {
        Some(payload[4] & 0x1F)
    } else {
        None
    }
}

/// Returns `true` for NAL types that carry a decoder reset (SPS/PPS/IDR).
pub fn is_keyframe_nal(nal_type: u8) -> bool {
    nal_type == 5 || nal_type == 7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_encodes_ip_and_port_index() {
        let r = request_packet(Ipv4Addr::new(10, 40, 0, 51), 6).unwrap();
        assert_eq!(r[0], 0x04);
        assert_eq!(r[1], 0x01);
        assert_eq!(r[2], 0x02);
        assert_eq!(&r[4..8], &[10, 40, 0, 51]);
        assert_eq!(&r[0x14..0x16], &5u16.to_be_bytes());
    }

    #[test]
    fn header_roundtrip() {
        let h = FrameHeader {
            counter: 0x123456,
            timestamp: 0x0306_D7B5,
            width: 1280,
            height: 960,
            payload_len: 0x123_456,
            frame_type: 1,
            flag: 0,
        };
        let bytes = build_header(h);
        let parsed = parse_header(&bytes).unwrap();
        assert_eq!(parsed, h);
    }

    #[test]
    fn nal_classifier() {
        assert_eq!(first_nal_type(&[0, 0, 0, 1, 0x67, 0x4D, 0x00, 0x20]), Some(7));
        assert!(is_keyframe_nal(7));
        assert!(is_keyframe_nal(5));
        assert!(!is_keyframe_nal(1));
        assert!(first_nal_type(&[1, 2, 3, 4, 5]).is_none());
    }
}
