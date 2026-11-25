//! Device profiles (a saved endpoint + credentials + port metadata).
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A saved connection to one KVM switch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    /// Host the client connects to; may be a tunnel endpoint (127.0.0.1).
    pub host: String,
    /// IP of the device as the firmware advertises it (used in the video request).
    pub device_ip: String,
    #[serde(default = "default_web_port")]
    pub web_port: u16,
    #[serde(default = "default_switch_port")]
    pub switch_port: u16,
    #[serde(default = "default_video_port")]
    pub video_port: u16,
    #[serde(default = "default_km_port")]
    pub km_port: u16,
    pub web_user: String,
    pub web_pass: String,
    /// Port numbers (1..=16) where the user explicitly allowed keyboard/mouse.
    #[serde(default)]
    pub input_ports: Vec<u8>,
    /// Custom port names (`port_number` -> label).
    #[serde(default)]
    pub port_labels: HashMap<u8, String>,
}

fn default_web_port() -> u16 {
    80
}
fn default_switch_port() -> u16 {
    7777
}
fn default_video_port() -> u16 {
    202
}
fn default_km_port() -> u16 {
    203
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: String::from("new device"),
            host: String::new(),
            device_ip: String::new(),
            web_port: default_web_port(),
            switch_port: default_switch_port(),
            video_port: default_video_port(),
            km_port: default_km_port(),
            web_user: String::from("admin"),
            web_pass: String::new(),
            input_ports: Vec::new(),
            port_labels: HashMap::new(),
        }
    }
}

impl Profile {
    /// True when the host resolves to a private, link-local, or loopback address.
    /// The UI shows a warning when this returns `false`, since the protocol has no
    /// authentication and should never travel over an untrusted network.
    pub fn host_is_local(&self) -> bool {
        is_local_hostname(&self.host)
    }
}

pub fn is_local_hostname(host: &str) -> bool {
    let bare = host.split(':').next().unwrap_or(host);
    if bare.eq_ignore_ascii_case("localhost") {
        return true;
    }
    match bare.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64 // 100.64.0.0/10 CGNAT / Tailscale
        }
        Ok(IpAddr::V6(v6)) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
        // Not an IP literal: treat as potentially non-local. The UI shows a reminder
        // either way.
        Err(_) => false,
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Store {
    pub profiles: Vec<Profile>,
    /// Index of the last-used profile, if any.
    #[serde(default)]
    pub last_used: Option<usize>,
}

pub fn store_path() -> PathBuf {
    crate::config::config_dir().join("profiles.toml")
}

pub fn load() -> Store {
    match std::fs::read_to_string(store_path()) {
        Ok(s) => toml::from_str(&s).unwrap_or_default(),
        Err(_) => Store::default(),
    }
}

pub fn save(s: &Store) -> std::io::Result<()> {
    let dir = crate::config::config_dir();
    std::fs::create_dir_all(&dir)?;
    let body = toml::to_string_pretty(s).map_err(std::io::Error::other)?;
    std::fs::write(store_path(), body)
}

#[cfg(test)]
mod tests {
    use super::is_local_hostname;

    #[test]
    fn classifies_private_ipv4() {
        assert!(is_local_hostname("127.0.0.1"));
        assert!(is_local_hostname("10.0.0.5"));
        assert!(is_local_hostname("192.168.1.1"));
        assert!(is_local_hostname("172.16.0.1"));
        assert!(is_local_hostname("169.254.1.1"));
        assert!(is_local_hostname("localhost"));
    }

    #[test]
    fn classifies_public_ipv4() {
        assert!(!is_local_hostname("8.8.8.8"));
        assert!(!is_local_hostname("1.1.1.1"));
    }
}
