//! End-to-end protocol roundtrip exercised against the pure-function API.
//!
//! These tests do no I/O. They verify that a request produced by the
//! client would be correctly parsed on the far side, and vice versa.

use std::net::Ipv4Addr;

use kinan_protocol::{hid, km, switch, video, Modifier, Modifiers};

#[test]
fn switch_request_and_reply_roundtrip() {
    for port in 1u8..=16 {
        let req = switch::request(port).unwrap();
        assert_eq!(req[2], port);
        // A simulator that reflects the request builds this reply:
        let reply = [0x04, 0x03, port - 1, 0x60, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
        let parsed = switch::parse_reply(&reply).unwrap();
        assert_eq!(parsed.confirmed_index, port - 1);
        assert!(switch::verify(&parsed, port).is_ok());
    }
}

#[test]
fn video_request_carries_the_port_index() {
    let buf = video::request_packet(Ipv4Addr::new(192, 168, 1, 10), 7).unwrap();
    assert_eq!(buf[2], 0x02);
    assert_eq!(&buf[4..8], &[192, 168, 1, 10]);
    assert_eq!(&buf[0x14..0x16], &6u16.to_be_bytes());
}

#[test]
fn km_packet_port_guard_catches_wrong_port() {
    let mut mods = Modifiers::default();
    mods.press(Modifier::ControlLeft);
    let mut keys = hid::PressedKeys::default();
    keys.press(0x1E); // 1
    let report = hid::build_report(mods, &keys);
    let packet = km::keyboard_report(report, 6, 42).unwrap();
    assert!(km::assert_port_matches(&packet, 6).is_ok());
    assert!(km::assert_port_matches(&packet, 7).is_err());
}

#[test]
fn mouse_coordinates_are_big_endian() {
    let p = km::mouse_absolute(0x1234, 0x5678, 0b010, -1, 3, 99).unwrap();
    assert_eq!(p[0], 0x60);
    assert_eq!(&p[2..4], &0x1234u16.to_be_bytes());
    assert_eq!(&p[4..6], &0x5678u16.to_be_bytes());
    assert_eq!(p[6], 0b010);
    assert_eq!(p[7] as i8, -1);
    assert_eq!(p[10], 3);
}

#[test]
fn video_header_payload_length_is_split_correctly() {
    // 0x12_3456 = ((w0 & 0xFFF) << 16) | lo, so w0 = 0x012, lo = 0x3456.
    let h = video::FrameHeader {
        counter: 42,
        timestamp: 1000,
        width: 1920,
        height: 1080,
        payload_len: 0x12_3456,
        frame_type: 1,
        flag: 0,
    };
    let bytes = video::build_header(h);
    // w0 byte 12-13 little-endian = 0x1012 (type=1 in high nibble shifted by 12, length-high=0x012)
    assert_eq!(&bytes[12..14], &0x1012u16.to_le_bytes());
    assert_eq!(&bytes[14..16], &0x3456u16.to_le_bytes());
    let parsed = video::parse_header(&bytes).unwrap();
    assert_eq!(parsed.payload_len, 0x12_3456);
    assert_eq!(parsed.frame_type, 1);
    assert_eq!(parsed.flag, 0);
}

#[test]
fn release_packet_targets_a_single_port() {
    let p = km::release(12, 1).unwrap();
    assert_eq!(p[0], 0x80);
    assert_eq!(p[2], 12);
    assert_eq!(p[10], 12);
    assert!(km::release(0, 0).is_err());
    assert!(km::release(17, 0).is_err());
}
