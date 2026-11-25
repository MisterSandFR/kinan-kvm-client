//! End-to-end session: switch -> video -> optional keyboard/mouse.
use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use kinan_protocol::{km, switch, video, Modifier, Modifiers};
use openh264::decoder::{Decoder, DecoderConfig};
use openh264::formats::YUVSource;

use crate::profiles::Profile;

/// Events emitted from the session thread to the UI.
#[derive(Debug, Clone)]
pub enum Event {
    Status(String),
    Resolution(u16, u16),
    Frame { w: u32, h: u32, rgb: Vec<u8> },
    NoSignal(bool),
    Error(String),
    Ended,
}

/// Commands from the UI to the session thread.
#[derive(Debug, Clone)]
pub enum Command {
    EnableInput(bool),
    Mouse {
        x_norm: f32,
        y_norm: f32,
        buttons: u8,
        wheel: i8,
    },
    Key {
        down_usage: u8,
        up_usage: u8,
        add_mod: Option<Modifier>,
        remove_mod: Option<Modifier>,
    },
    CtrlAltDel,
    Stop,
}

#[allow(dead_code)]
pub struct Handle {
    pub port: u8,
    pub events: mpsc::Receiver<Event>,
    pub commands: mpsc::Sender<Command>,
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Handle {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.commands.send(Command::Stop);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

pub fn spawn(profile: Profile, port: u8, h264_path: std::path::PathBuf) -> Handle {
    let (ev_tx, ev_rx) = mpsc::sync_channel(64);
    let (cmd_tx, cmd_rx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop2 = stop.clone();
    let join = std::thread::Builder::new()
        .name(format!("kinan-session-{port}"))
        .spawn(move || run(profile, port, h264_path, ev_tx, cmd_rx, stop2))
        .expect("spawn session thread");
    Handle {
        port,
        events: ev_rx,
        commands: cmd_tx,
        stop,
        join: Some(join),
    }
}

fn connect(host: &str, port: u16, timeout: Duration) -> Result<TcpStream> {
    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .with_context(|| format!("resolve {host}:{port}"))?
        .collect();
    let mut last_err: Option<anyhow::Error> = None;
    for a in &addrs {
        match TcpStream::connect_timeout(a, timeout) {
            Ok(s) => {
                let _ = s.set_read_timeout(Some(timeout));
                let _ = s.set_write_timeout(Some(timeout));
                let _ = s.set_nodelay(true);
                return Ok(s);
            }
            Err(e) => last_err = Some(anyhow!("{a}: {e}")),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("no address for {host}:{port}")))
}

fn run(
    profile: Profile,
    port: u8,
    h264_path: std::path::PathBuf,
    ev: SyncSender<Event>,
    cmds: mpsc::Receiver<Command>,
    stop: Arc<AtomicBool>,
) {
    let r: Result<()> = (|| {
        let _ = ev.try_send(Event::Status(format!("switching to port {port}")));

        // 1. Switch channel (single-shot).
        let mut sw = connect(&profile.host, profile.switch_port, Duration::from_secs(5))?;
        sw.write_all(&switch::request(port)?)?;
        let mut rep = [0u8; switch::PACKET_LEN];
        sw.read_exact(&mut rep)?;
        drop(sw);
        let reply = switch::parse_reply(&rep)?;
        switch::verify(&reply, port)?;

        // 2. Video channel.
        let device_ip: IpAddr = profile.device_ip.parse().with_context(|| {
            format!(
                "device_ip `{}` is not a valid IPv4/IPv6 literal",
                profile.device_ip
            )
        })?;
        let v4 = match device_ip {
            IpAddr::V4(v) => v,
            IpAddr::V6(_) => return Err(anyhow!("device_ip must be an IPv4 address")),
        };
        let mut video = connect(&profile.host, profile.video_port, Duration::from_secs(5))?;
        video.set_read_timeout(Some(Duration::from_millis(500)))?;
        video.write_all(&video::request_packet(v4, port)?)?;

        // 3. Decoder: OpenH264 is loaded from the Cisco binary verified at startup.
        let api = kinan_h264::load(&h264_path)?;
        let mut decoder = Decoder::with_api_config(api, DecoderConfig::new())?;

        // 4. Keyboard/mouse is closed by default.
        let allowed = profile.input_ports.contains(&port);
        let mut km_stream: Option<KmState> = None;
        let mut width: u16 = 1024;
        let mut height: u16 = 768;
        let _ = ev.try_send(Event::Resolution(width, height));
        let _ = ev.try_send(Event::Status(format!(
            "video requested (input allowed: {})",
            if allowed { "yes" } else { "no" }
        )));

        let mut stale = true;
        let mut seen_fresh = 0u8;
        let mut skip_until_key = true;
        let mut no_signal = false;
        let mut last_frame = Instant::now();

        loop {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            loop {
                match cmds.try_recv() {
                    Ok(cmd) => {
                        if !handle_command(cmd, &profile, port, allowed, &mut km_stream, width, height, &ev)?
                        {
                            return Ok(());
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
                }
            }

            let mut hdr = [0u8; video::HEADER_LEN];
            let t = Instant::now();
            match video.read_exact(&mut hdr) {
                Ok(()) => {}
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock
                        || e.kind() == std::io::ErrorKind::TimedOut =>
                {
                    if last_frame.elapsed() > Duration::from_secs(3) && !no_signal {
                        no_signal = true;
                        let _ = ev.try_send(Event::NoSignal(true));
                    }
                    continue;
                }
                Err(e) => return Err(anyhow!("video read: {e}")),
            }
            last_frame = Instant::now();
            if hdr == [0xFFu8; video::HEADER_LEN] {
                return Err(anyhow!("video channel refused or busy"));
            }
            let h = video::parse_header(&hdr)?;
            if h.payload_len > video::MAX_PAYLOAD_LEN || h.flag == 3 {
                return Err(anyhow!("invalid frame or busy channel"));
            }
            let waited = t.elapsed();
            let mut body = vec![0u8; h.payload_len as usize];
            if h.payload_len > 0 {
                video.read_exact(&mut body)?;
            }
            if stale {
                if waited >= Duration::from_millis(5) {
                    seen_fresh = seen_fresh.saturating_add(1);
                    if seen_fresh >= 3 {
                        stale = false;
                    }
                } else {
                    seen_fresh = 0;
                }
                if stale {
                    continue;
                }
            }
            if no_signal {
                no_signal = false;
                let _ = ev.try_send(Event::NoSignal(false));
            }
            if h.width != 0 && h.height != 0 && (h.width, h.height) != (width, height) {
                width = h.width;
                height = h.height;
                let _ = ev.try_send(Event::Resolution(width, height));
                if let Some(km_state) = km_stream.as_mut() {
                    let _ = km_state.send_resolution(width, height);
                }
            }
            let nal = match video::first_nal_type(&body) {
                Some(n) => n,
                None => continue,
            };
            if nal == 3 {
                continue;
            }
            let is_key = video::is_keyframe_nal(nal);
            if skip_until_key && !is_key {
                continue;
            }
            skip_until_key = false;
            match decoder.decode(&body) {
                Ok(Some(dec)) => {
                    let (w, hh) = dec.dimensions();
                    let mut rgb = vec![0u8; w * hh * 3];
                    dec.write_rgb8(&mut rgb);
                    let _ = ev.try_send(Event::Frame {
                        w: w as u32,
                        h: hh as u32,
                        rgb,
                    });
                }
                Ok(None) => {}
                Err(e) => {
                    skip_until_key = true;
                    log::warn!("decoder: {e}");
                }
            }
        }

        if let Some(k) = km_stream.take() {
            let _ = k.close();
        }
        Ok(())
    })();

    if let Err(e) = r {
        let _ = ev.try_send(Event::Error(format!("{e:#}")));
    }
    let _ = ev.try_send(Event::Ended);
}

struct KmState {
    stream: TcpStream,
    port: u8,
    seq: u8,
    mods: Modifiers,
    pressed: kinan_protocol::hid::PressedKeys,
}

impl KmState {
    fn open(host: &str, tcp_port: u16, kvm_port: u8, timeout: Duration) -> Result<Self> {
        let stream = connect(host, tcp_port, timeout)?;
        Ok(Self {
            stream,
            port: kvm_port,
            seq: 0,
            mods: Modifiers::default(),
            pressed: kinan_protocol::hid::PressedKeys::default(),
        })
    }

    fn next_seq(&mut self) -> u8 {
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    fn send(&mut self, packet: [u8; km::PACKET_LEN]) -> Result<()> {
        km::assert_port_matches(&packet, self.port)?;
        self.stream.write_all(&packet).context("km write")
    }

    fn send_resolution(&mut self, w: u16, h: u16) -> Result<()> {
        let seq = self.next_seq();
        self.send(km::resolution(w, h, self.port, seq)?)
    }

    fn send_mouse(&mut self, x: u16, y: u16, buttons: u8, wheel: i8) -> Result<()> {
        let seq = self.next_seq();
        self.send(km::mouse_absolute(x, y, buttons, wheel, self.port, seq)?)
    }

    fn send_report(&mut self) -> Result<()> {
        let report = kinan_protocol::hid::build_report(self.mods, &self.pressed);
        let seq = self.next_seq();
        self.send(km::keyboard_report(report, self.port, seq)?)
    }

    fn apply_key(
        &mut self,
        down_usage: u8,
        up_usage: u8,
        add_mod: Option<Modifier>,
        remove_mod: Option<Modifier>,
    ) -> Result<()> {
        if let Some(m) = add_mod {
            self.mods.press(m);
        }
        if let Some(m) = remove_mod {
            self.mods.release(m);
        }
        if down_usage != 0 {
            self.pressed.press(down_usage);
        }
        if up_usage != 0 {
            self.pressed.release(up_usage);
        }
        self.send_report()
    }

    fn ctrl_alt_del(&mut self) -> Result<()> {
        self.mods.clear();
        self.mods.press(Modifier::ControlLeft);
        self.mods.press(Modifier::AltLeft);
        self.pressed.clear();
        self.pressed.press(0x4C);
        self.send_report()?;
        self.mods.clear();
        self.pressed.clear();
        self.send_report()
    }

    fn release_all(&mut self) -> Result<()> {
        self.mods.clear();
        self.pressed.clear();
        self.send_report()
    }

    fn close(mut self) -> Result<()> {
        let _ = self.release_all();
        let seq = self.next_seq();
        let _ = self.send(km::release(self.port, seq)?);
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_command(
    cmd: Command,
    profile: &Profile,
    port: u8,
    allowed: bool,
    km_state: &mut Option<KmState>,
    width: u16,
    height: u16,
    ev: &SyncSender<Event>,
) -> Result<bool> {
    match cmd {
        Command::EnableInput(on) => {
            if on && !allowed {
                let _ = ev.try_send(Event::Status("input refused: port not in allowed list".into()));
                return Ok(true);
            }
            if on && km_state.is_none() {
                let mut k = KmState::open(&profile.host, profile.km_port, port, Duration::from_secs(5))?;
                let _ = k.send_resolution(width, height);
                *km_state = Some(k);
                let _ = ev.try_send(Event::Status("input ACTIVE".into()));
            } else if !on && km_state.is_some() {
                let k = km_state.take().unwrap();
                let _ = k.close();
                let _ = ev.try_send(Event::Status("input off".into()));
            }
        }
        Command::Mouse {
            x_norm,
            y_norm,
            buttons,
            wheel,
        } => {
            if let Some(k) = km_state.as_mut() {
                let x = (x_norm.clamp(0.0, 1.0) * (width.saturating_sub(1) as f32)) as u16;
                let y = (y_norm.clamp(0.0, 1.0) * (height.saturating_sub(1) as f32)) as u16;
                let _ = k.send_mouse(x, y, buttons, wheel);
            }
        }
        Command::Key {
            down_usage,
            up_usage,
            add_mod,
            remove_mod,
        } => {
            if let Some(k) = km_state.as_mut() {
                let _ = k.apply_key(down_usage, up_usage, add_mod, remove_mod);
            }
        }
        Command::CtrlAltDel => {
            if let Some(k) = km_state.as_mut() {
                let _ = k.ctrl_alt_del();
            }
        }
        Command::Stop => {
            if let Some(k) = km_state.take() {
                let _ = k.close();
            }
            return Ok(false);
        }
    }
    Ok(true)
}
