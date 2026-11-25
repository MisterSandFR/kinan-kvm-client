//! Thin HTTP client over the device's JSON web API.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use kinan_protocol::des;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct PortInfo {
    pub num: u8,
    pub name: String,
    pub online: bool,
    pub state: String, // "connect" | "close"
}

pub struct KinanWeb {
    base: String,
    user: String,
    password: String,
    agent: ureq::Agent,
    cookie: Arc<Mutex<Option<String>>>,
}

#[derive(Deserialize)]
struct LoginResp {
    status: String,
}

#[derive(Deserialize)]
struct PortRaw {
    #[serde(rename = "sNum")]
    s_num: String,
    #[serde(rename = "cName", default)]
    c_name: String,
    #[serde(rename = "cStatus", default)]
    c_status: String,
    #[serde(default)]
    availability: String,
}

#[derive(Deserialize)]
struct PortInfoResp {
    #[serde(default)]
    status: String,
    #[serde(default)]
    data: Vec<PortRaw>,
}

impl KinanWeb {
    pub fn new(base: String, user: String, password: String) -> Self {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(5)).build();
        Self {
            base,
            user,
            password,
            agent,
            cookie: Arc::new(Mutex::new(None)),
        }
    }

    pub fn login(&self) -> Result<()> {
        let enc = des::str_enc(&self.password, "123456789");
        let body = format!(
            "username={}&password={}",
            urlencoded(&self.user),
            urlencoded(&enc)
        );
        let resp = self
            .agent
            .post(&format!("{}/kinan/Login?lang=en", self.base))
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&body)
            .context("POST /kinan/Login")?;
        let cookie = resp
            .all("set-cookie")
            .iter()
            .find_map(|v| v.split(';').next().map(|s| s.to_string()));
        let text = resp.into_string().context("login body")?;
        let r: LoginResp = serde_json::from_str(&text).context("login json")?;
        if r.status != "true" {
            return Err(anyhow!("login refused: {}", text));
        }
        if let Some(c) = cookie {
            *self.cookie.lock().unwrap() = Some(c);
        }
        Ok(())
    }

    fn get(&self, path: &str) -> Result<String> {
        let mut req = self.agent.get(&format!("{}{}", self.base, path));
        if let Some(c) = self.cookie.lock().unwrap().as_ref() {
            req = req.set("Cookie", c);
        }
        let text = req.call().context("GET")?.into_string().context("body")?;
        Ok(text)
    }

    fn authed(&self, path: &str) -> Result<String> {
        if self.cookie.lock().unwrap().is_none() {
            self.login()?;
        }
        let t = self.get(path)?;
        if t.contains("\"status\":\"false\"") {
            self.login()?;
            return self.get(path);
        }
        Ok(t)
    }

    pub fn ports(&self) -> Result<Vec<PortInfo>> {
        let text = self.authed("/kinan/PortInfo?lang=en")?;
        let r: PortInfoResp = serde_json::from_str(&text).context("PortInfo json")?;
        if r.status != "true" {
            return Err(anyhow!("PortInfo refused"));
        }
        let mut out = Vec::new();
        for d in r.data {
            let num: u8 = d.s_num.parse().unwrap_or(0);
            if !(1..=16).contains(&num) {
                continue;
            }
            out.push(PortInfo {
                num,
                name: d.c_name,
                online: d.c_status == "1",
                state: d.availability,
            });
        }
        Ok(out)
    }
}

fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => {
                use std::fmt::Write;
                let _ = write!(&mut out, "%{:02X}", b);
            }
        }
    }
    out
}
