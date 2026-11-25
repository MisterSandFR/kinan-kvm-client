//! Fetch and load Cisco's OpenH264 shared library at runtime.
//!
//! Cisco distributes prebuilt OpenH264 binaries at
//! <https://www.openh264.org/BINARY_LICENSE.txt> under terms that cover the
//! H.264 royalties *only* when the binary is obtained directly from Cisco's
//! own host. Compiling OpenH264 from source and shipping the resulting binary
//! yourself would not grant the same coverage, which is why this project
//! downloads the library at first launch rather than embedding it.
//!
//! The downloaded file is cached under the OS user data directory and its
//! SHA-256 is checked against the hashes bundled with `openh264-sys2`, so a
//! tampered download cannot be loaded.

#![forbid(unsafe_code)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use openh264::OpenH264API;

/// The pinned OpenH264 release.
pub const VERSION: &str = "2.4.0";

/// SHA-256 hashes of the Cisco binaries this crate agrees to load, per
/// platform. They match the entries shipped by `openh264-sys2`'s
/// `blobs/hashes.txt`.
const KNOWN_HASHES: &[(&str, &str)] = &[
    (
        "openh264-2.4.0-win64.dll",
        "fb18f5e4b912c9066b55b09fa558c5f5b0c3fb88e5add931ac3aac263d1a3ca0",
    ),
    (
        "libopenh264-2.4.0-linux64.7.so",
        "d66b0381e7158c6a61f7ba90e074f25d15ed6516231300e5319c8ad6afbf9f3b",
    ),
    (
        "libopenh264-2.4.0-mac-arm64.dylib",
        "e7da95f9b10712548d13a1eaf1fe2ea89b42f754a75c7a069ca28f777864214e",
    ),
    (
        "libopenh264-2.4.0-mac-x64.dylib",
        "37b9a8c107bc87bb03dcc99acdedbc22da090c850a50ed34b6b50faa35b55b26",
    ),
];

/// Where to put the cached DLL on this machine.
pub fn cache_path() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("io", "kinan-kvm-client", "kinan-kvm-client")
        .context("could not resolve a per-user data directory")?;
    let dir = dirs.data_dir().join("openh264");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(platform_file_name()))
}

fn platform_file_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "openh264-2.4.0-win64.dll"
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        "libopenh264-2.4.0-linux64.7.so"
    }
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        "libopenh264-2.4.0-mac-arm64.dylib"
    }
    #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
    {
        "libopenh264-2.4.0-mac-x64.dylib"
    }
    #[cfg(not(any(
        target_os = "windows",
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "macos", target_arch = "aarch64"),
        all(target_os = "macos", target_arch = "x86_64"),
    )))]
    {
        "unsupported"
    }
}

/// Report a progress update: bytes downloaded, total if known.
pub type ProgressFn = Arc<dyn Fn(u64, Option<u64>) + Send + Sync>;

/// Return the path to a verified OpenH264 binary, downloading it if needed.
///
/// `progress` is called periodically during the download. Pass `None` for a
/// silent fetch (the library still emits a few `log::info!` lines).
pub fn ensure_available(progress: Option<ProgressFn>) -> Result<PathBuf> {
    let file_name = platform_file_name();
    if file_name == "unsupported" {
        bail!("no Cisco OpenH264 binary is published for this operating system / CPU combination");
    }
    let path = cache_path()?;
    if path.exists() {
        match verify(&path) {
            Ok(()) => return Ok(path),
            Err(e) => {
                log::warn!("cached OpenH264 is unusable ({e}); re-downloading");
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    let url = format!("https://ciscobinary.openh264.org/{file_name}.bz2");
    log::info!("downloading OpenH264 {} from {}", VERSION, url);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(60))
        .build();
    let resp = agent.get(&url).call().with_context(|| format!("GET {url}"))?;
    let total: Option<u64> = resp.header("content-length").and_then(|s| s.parse().ok());
    let mut reader = resp.into_reader();

    // Decompress on the fly.
    let mut compressed = Vec::with_capacity(total.unwrap_or(512 * 1024) as usize);
    let downloaded = Arc::new(AtomicU64::new(0));
    let mut buf = [0u8; 16 * 1024];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                compressed.extend_from_slice(&buf[..n]);
                let d = downloaded.fetch_add(n as u64, Ordering::SeqCst) + n as u64;
                if let Some(cb) = &progress {
                    cb(d, total);
                }
            }
            Err(e) => return Err(anyhow!("read: {e}")),
        }
    }

    let mut decomp = bzip2_rs::DecoderReader::new(std::io::Cursor::new(compressed));
    let mut plain = Vec::new();
    decomp
        .read_to_end(&mut plain)
        .context("bzip2 decompression of the OpenH264 binary")?;
    std::fs::write(&path, &plain).with_context(|| format!("writing {}", path.display()))?;
    verify(&path).context("the downloaded OpenH264 did not match the expected SHA-256")?;
    log::info!("OpenH264 cached at {}", path.display());
    Ok(path)
}

/// Check the file's SHA-256 against the pinned list.
pub fn verify(path: &Path) -> Result<()> {
    verify_by_known_name(path, None)
}

/// Verify a user-provided OpenH264 library by brute-forcing its hash against
/// the list. Used by the "local file" escape hatch so the operator can point
/// at any of the hashes we know about, under any file name.
pub fn verify_any(path: &Path) -> Result<()> {
    verify_by_known_name(path, Some("<any>"))
}

fn verify_by_known_name(path: &Path, allow_any: Option<&str>) -> Result<()> {
    use sha2::{Digest, Sha256};
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let digest = Sha256::digest(&bytes);
    let got = hex(&digest);
    if allow_any.is_some() {
        if KNOWN_HASHES.iter().any(|(_, h)| *h == got) {
            return Ok(());
        }
        bail!(
            "SHA-256 {got} for {} does not match any known Cisco OpenH264 release",
            path.display()
        );
    }
    let expected = KNOWN_HASHES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, h)| *h)
        .ok_or_else(|| anyhow!("unknown OpenH264 file name {name}"))?;
    if got != expected {
        bail!(
            "SHA-256 mismatch for {}: expected {expected}, got {got}",
            path.display()
        );
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

/// Open the library handed back by [`ensure_available`].
pub fn load(path: &Path) -> Result<OpenH264API> {
    OpenH264API::from_blob_path(path).with_context(|| format!("loading {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_name_is_known() {
        let n = platform_file_name();
        if n == "unsupported" {
            return;
        }
        assert!(KNOWN_HASHES.iter().any(|(k, _)| *k == n));
    }

    #[test]
    fn verify_rejects_missing_file() {
        let tmp = std::env::temp_dir().join("no-such-openh264.dll");
        let _ = std::fs::remove_file(&tmp);
        assert!(verify(&tmp).is_err());
    }
}
