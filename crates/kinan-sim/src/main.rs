//! A tiny simulator of a Kinan KVM switch.
//!
//! It implements enough of the protocol for the client's unit and integration
//! tests to exercise the switch channel, the video stream and the keyboard /
//! mouse channel. The video stream is a software-generated test pattern that
//! advertises itself as "kinan-sim".
//!
//! Usage: `kinan-sim [--bind 127.0.0.1] [--web-port 8080] [--switch-port 8777]
//! [--video-port 8202] [--km-port 8203] [--width 1280] [--height 720]`.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use kinan_protocol::{km, switch, video};
use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::YUVBuffer;
use serde::Serialize;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let opts = Options::from_args();

    if let Some(path) = opts.dump_pattern.clone() {
        let w = opts.width as usize;
        let h = opts.height as usize;
        let yuv = render_test_pattern(w, h, 0, 0);
        let rgb = yuv420p_to_rgb(&yuv, w, h);
        write_ppm(&path, w as u32, h as u32, &rgb)?;
        println!("pattern written to {}", path.display());
        return Ok(());
    }
    log::info!(
        "kinan-sim listening on {}: web={} switch={} video={} km={}",
        opts.bind,
        opts.web_port,
        opts.switch_port,
        opts.video_port,
        opts.km_port
    );

    let state = Arc::new(State::default());

    spawn_listener(opts.bind, opts.web_port, {
        let state = state.clone();
        let opts = opts.clone();
        move |stream| serve_web(stream, &opts, &state)
    })?;
    spawn_listener(opts.bind, opts.switch_port, {
        let state = state.clone();
        move |stream| serve_switch(stream, &state)
    })?;
    spawn_listener(opts.bind, opts.video_port, {
        let state = state.clone();
        let opts = opts.clone();
        move |stream| serve_video(stream, &opts, &state)
    })?;
    spawn_listener(opts.bind, opts.km_port, serve_km)?;

    // Keep the main thread alive.
    thread::park();
    Ok(())
}

#[derive(Clone)]
struct Options {
    bind: IpAddr,
    web_port: u16,
    switch_port: u16,
    video_port: u16,
    km_port: u16,
    width: u16,
    height: u16,
    dump_pattern: Option<std::path::PathBuf>,
}

impl Options {
    fn from_args() -> Self {
        let mut o = Options {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            web_port: 8080,
            switch_port: 8777,
            video_port: 8202,
            km_port: 8203,
            width: 1280,
            height: 720,
            dump_pattern: None,
        };
        let args: Vec<String> = std::env::args().collect();
        let mut i = 1;
        while i < args.len() {
            let a = &args[i];
            let v = args.get(i + 1);
            match a.as_str() {
                "--bind" => {
                    if let Some(v) = v {
                        o.bind = v.parse().unwrap_or(o.bind);
                        i += 1
                    }
                }
                "--web-port" => {
                    if let Some(v) = v {
                        o.web_port = v.parse().unwrap_or(o.web_port);
                        i += 1
                    }
                }
                "--switch-port" => {
                    if let Some(v) = v {
                        o.switch_port = v.parse().unwrap_or(o.switch_port);
                        i += 1
                    }
                }
                "--video-port" => {
                    if let Some(v) = v {
                        o.video_port = v.parse().unwrap_or(o.video_port);
                        i += 1
                    }
                }
                "--km-port" => {
                    if let Some(v) = v {
                        o.km_port = v.parse().unwrap_or(o.km_port);
                        i += 1
                    }
                }
                "--width" => {
                    if let Some(v) = v {
                        o.width = v.parse().unwrap_or(o.width);
                        i += 1
                    }
                }
                "--height" => {
                    if let Some(v) = v {
                        o.height = v.parse().unwrap_or(o.height);
                        i += 1
                    }
                }
                "--dump-pattern" => {
                    if let Some(v) = v {
                        o.dump_pattern = Some(v.into());
                        i += 1
                    }
                }
                _ => {}
            }
            i += 1;
        }
        o
    }
}

#[derive(Default)]
struct State {
    // Zero-based index of the currently selected port, or 0xFF when none.
    current: AtomicU8,
}

fn spawn_listener<F>(bind: IpAddr, port: u16, handler: F) -> Result<()>
where
    F: Fn(TcpStream) -> Result<()> + Send + Sync + 'static,
{
    let listener =
        TcpListener::bind(SocketAddr::new(bind, port)).with_context(|| format!("binding port {port}"))?;
    let handler = Arc::new(handler);
    thread::Builder::new()
        .name(format!("listen-{port}"))
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let handler = handler.clone();
                thread::spawn(move || {
                    if let Err(e) = handler(stream) {
                        log::debug!("port {port} connection ended: {e:#}");
                    }
                });
            }
        })?;
    Ok(())
}

#[derive(Serialize)]
struct PortJson {
    #[serde(rename = "sNum")]
    s_num: String,
    #[serde(rename = "cName")]
    c_name: String,
    #[serde(rename = "cStatus")]
    c_status: String,
    availability: &'static str,
    control_pri_int: &'static str,
}

#[derive(Serialize)]
struct Envelope<T> {
    status: &'static str,
    data: T,
}

fn serve_web(mut stream: TcpStream, _opts: &Options, _state: &State) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let head = String::from_utf8_lossy(&buf[..n]);
    let (method, path) = parse_request_line(&head);
    let (status, body, content_type) = match (method.as_str(), path.as_str()) {
        ("POST", p) if p.starts_with("/kinan/Login") => {
            // Intentionally lax: accept any credentials.
            (
                "200 OK",
                serde_json::to_string(&Envelope {
                    status: "true",
                    data: serde_json::Value::Null,
                })?,
                "application/json",
            )
        }
        ("GET", p) if p.starts_with("/kinan/PortInfo") => {
            let list = demo_ports();
            (
                "200 OK",
                serde_json::to_string(&Envelope {
                    status: "true",
                    data: list,
                })?,
                "application/json",
            )
        }
        ("GET", p) if p.starts_with("/kinan/SideBar") => {
            let mut m: HashMap<&str, &str> = HashMap::new();
            m.insert("loginUser", "admin");
            m.insert("firmware", "sim-0.1");
            m.insert("model", "kinan-sim");
            (
                "200 OK",
                serde_json::to_string(&Envelope {
                    status: "true",
                    data: m,
                })?,
                "application/json",
            )
        }
        _ => ("404 Not Found", "{}".to_string(), "application/json"),
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(reply.as_bytes())?;
    Ok(())
}

fn parse_request_line(head: &str) -> (String, String) {
    let line = head.lines().next().unwrap_or("");
    let mut it = line.split_whitespace();
    let method = it.next().unwrap_or("").to_string();
    let path = it.next().unwrap_or("").to_string();
    (method, path)
}

fn demo_ports() -> Vec<PortJson> {
    let names = [
        "web-1",
        "web-2",
        "db-primary",
        "db-replica",
        "monitor",
        "test-rig",
        "spare-1",
        "spare-2",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "console-ha",
    ];
    (1..=16u8)
        .map(|n| PortJson {
            s_num: n.to_string(),
            c_name: names[(n - 1) as usize].to_string(),
            c_status: if (n as usize) == 6 { "0".into() } else { "1".into() },
            availability: if n % 7 == 0 { "connect" } else { "close" },
            control_pri_int: "2",
        })
        .collect()
}

fn serve_switch(mut stream: TcpStream, state: &State) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = [0u8; switch::PACKET_LEN];
    stream.read_exact(&mut buf)?;
    if buf[0] != 0x04 || buf[1] != 0x01 {
        return Ok(());
    }
    let port = buf[2];
    if !(1..=16).contains(&port) {
        return Ok(());
    }
    let idx = port - 1;
    state.current.store(idx, Ordering::SeqCst);
    let reply = [0x04, 0x03, idx, 0x60, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
    stream.write_all(&reply)?;
    Ok(())
}

fn serve_video(mut stream: TcpStream, opts: &Options, state: &State) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buf = [0u8; video::REQUEST_LEN];
    stream.read_exact(&mut buf)?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let width = opts.width as usize;
    let height = opts.height as usize;
    let cfg = EncoderConfig::new();
    let mut encoder = Encoder::with_api_config(openh264::OpenH264API::from_source(), cfg)?;
    let mut counter = 0u32;
    let started = Instant::now();
    while counter < 450 {
        // ~15 seconds at 30 fps.
        let idx = state.current.load(Ordering::SeqCst);
        let frame = render_test_pattern(width, height, counter, idx);
        let yuv = YUVBuffer::from_vec(frame, width, height);
        let bits = encoder.encode(&yuv)?;
        let mut body = Vec::new();
        for i in 0..bits.num_layers() {
            let layer = bits.layer(i).unwrap();
            for j in 0..layer.nal_count() {
                body.extend_from_slice(layer.nal_unit(j).unwrap());
            }
        }
        let hdr = video::FrameHeader {
            counter,
            timestamp: started.elapsed().as_secs() as u32,
            width: width as u16,
            height: height as u16,
            payload_len: body.len() as u32,
            frame_type: if counter % 30 == 0 { 0 } else { 1 },
            flag: 0,
        };
        stream.write_all(&video::build_header(hdr))?;
        stream.write_all(&body)?;
        counter = counter.wrapping_add(1);
        thread::sleep(Duration::from_millis(33));
    }
    Ok(())
}

fn render_test_pattern(width: usize, height: usize, counter: u32, selected: u8) -> Vec<u8> {
    // YUV 4:2:0 planar. We draw a vertical blue → cyan gradient with a moving
    // white bar so it is obvious which port is live.
    let mut y = vec![0u8; width * height];
    let u = vec![128u8; (width / 2) * (height / 2)];
    let v = vec![128u8; (width / 2) * (height / 2)];
    for row in 0..height {
        let base = 32 + ((row * 160) / height.max(1)) as u8;
        for col in 0..width {
            y[row * width + col] = base;
        }
    }
    // Moving bar.
    let bar_h = (height / 20).max(4);
    let bar_top = (counter as usize * 4) % height.saturating_sub(bar_h).max(1);
    for row in bar_top..(bar_top + bar_h).min(height) {
        for col in 0..width {
            y[row * width + col] = 230;
        }
    }
    // Port tag: a thick vertical stripe whose position reflects `selected`.
    let col = if selected == 0xFF {
        width / 2
    } else {
        (selected as usize * width) / 16
    };
    for row in 0..height {
        let c0 = col.saturating_sub(4);
        let c1 = (col + 4).min(width);
        for cc in c0..c1 {
            y[row * width + cc] = 255;
        }
    }
    let mut out = Vec::with_capacity(width * height * 3 / 2);
    out.extend_from_slice(&y);
    out.extend_from_slice(&u);
    out.extend_from_slice(&v);
    out
}

/// Convert a YUV 4:2:0 planar buffer to an interleaved RGB24 buffer.
fn yuv420p_to_rgb(buf: &[u8], w: usize, h: usize) -> Vec<u8> {
    let y_size = w * h;
    let uv_w = w / 2;
    let y = &buf[..y_size];
    let u = &buf[y_size..y_size + uv_w * (h / 2)];
    let v = &buf[y_size + uv_w * (h / 2)..];
    let mut rgb = vec![0u8; w * h * 3];
    for row in 0..h {
        for col in 0..w {
            let yy = y[row * w + col] as i32;
            let uu = u[(row / 2) * uv_w + col / 2] as i32 - 128;
            let vv = v[(row / 2) * uv_w + col / 2] as i32 - 128;
            // BT.601 YCbCr to RGB.
            let r = yy + ((91881 * vv) >> 16);
            let g = yy - ((22554 * uu + 46802 * vv) >> 16);
            let b = yy + ((116130 * uu) >> 16);
            let off = (row * w + col) * 3;
            rgb[off] = r.clamp(0, 255) as u8;
            rgb[off + 1] = g.clamp(0, 255) as u8;
            rgb[off + 2] = b.clamp(0, 255) as u8;
        }
    }
    rgb
}

fn write_ppm(path: &std::path::Path, w: u32, h: u32, rgb: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext.eq_ignore_ascii_case("png") {
        let f = std::fs::File::create(path)?;
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()?.write_image_data(rgb)?;
    } else {
        let mut f = std::fs::File::create(path)?;
        use std::io::Write;
        write!(f, "P6\n{w} {h}\n255\n")?;
        f.write_all(rgb)?;
    }
    Ok(())
}

fn serve_km(mut stream: TcpStream) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(60)))?;
    let mut buf = [0u8; km::PACKET_LEN];
    loop {
        if stream.read_exact(&mut buf).is_err() {
            return Ok(());
        }
        log::debug!(
            "km <- {}",
            buf.iter().map(|b| format!("{:02x}", b)).collect::<String>()
        );
    }
}
