//! Saved connections.
//!
//! Reads and writes `~/.config/sshm/connections.json`, the same file the CEF
//! build uses, so your existing hosts show up in both apps. Fields this app
//! does not understand yet (for example `password`) are kept in `extra` and
//! written back unchanged, so saving here never drops data the CEF app needs.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Connection {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub group: String,
    /// "ssh" or "local".
    #[serde(default = "default_kind", rename = "type")]
    pub kind: String,
    /// "password" or "key".
    #[serde(default)]
    pub auth: String,
    #[serde(default)]
    pub key_path: String,

    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn default_port() -> u16 {
    22
}

fn default_kind() -> String {
    "ssh".into()
}

impl Connection {
    pub fn new_ssh() -> Self {
        Self {
            id: new_id(),
            port: 22,
            kind: "ssh".into(),
            auth: "password".into(),
            ..Default::default()
        }
    }

    pub fn display_name(&self) -> String {
        if !self.name.is_empty() {
            self.name.clone()
        } else if !self.username.is_empty() {
            format!("{}@{}", self.username, self.host)
        } else {
            self.host.clone()
        }
    }

    /// Program + args to run inside the PTY. `None` means the user's shell.
    pub fn command(&self) -> Option<(String, Vec<String>)> {
        if self.kind == "local" {
            return None;
        }
        let mut args = vec!["-p".to_string(), self.port.to_string()];
        if self.auth == "key" && !self.key_path.is_empty() {
            args.push("-i".into());
            args.push(expand_tilde(&self.key_path));
        }
        // ssh prompts for the password inside the terminal. Native auth with
        // the stored secret comes when the backend moves to `russh`.
        args.push(if self.username.is_empty() {
            self.host.clone()
        } else {
            format!("{}@{}", self.username, self.host)
        });
        Some(("ssh".into(), args))
    }
}

fn expand_tilde(path: &str) -> String {
    match (path.strip_prefix("~/"), dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest).to_string_lossy().into_owned(),
        _ => path.to_string(),
    }
}

fn new_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("{nanos:x}")
}

fn store_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("sshm").join("connections.json"))
}

pub fn load() -> Vec<Connection> {
    let Some(path) = store_path() else { return Vec::new() };
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
            eprintln!("sshm: could not parse {}: {e}", path.display());
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

/// Atomic write (temp file + rename), 0600 because the file can hold secrets.
pub fn save(connections: &[Connection]) -> anyhow::Result<()> {
    let path = store_path().ok_or_else(|| anyhow::anyhow!("no config directory"))?;
    let dir = path.parent().expect("store path has a parent");
    fs::create_dir_all(dir)?;

    let tmp = path.with_extension("json.tmp");
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp)?;
    file.write_all(serde_json::to_string_pretty(connections)?.as_bytes())?;
    file.sync_all()?;
    fs::rename(tmp, path)?;
    Ok(())
}
