//! Der nrzCode-Server im selben Prozess: Umgebung für eine GUI-App vorbereiten (PATH, Datenordner,
//! Ports), dann Rocket auf einem eigenen Tokio-Laufzeit-Thread starten.
//!
//! Die Einstellungen gehen über `NRZCODE_*`-Umgebungsvariablen, weil Teile des Servers
//! (`config::figment()`) sie direkt lesen – so sehen alle dieselben Werte.

use std::net::{SocketAddr, TcpListener, TcpStream};
#[cfg_attr(windows, allow(unused_imports))]
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Bevorzugte Ports der App – absichtlich nicht 8410/8411, damit ein lokal laufender
/// Docker-Container von nrzCode nicht stört.
const PREFERRED_PORT: u16 = 18410;
const PREFERRED_PREVIEW_PORT: u16 = 18411;

pub struct Server {
    pub port: u16,
    shutdown: Option<rocket::Shutdown>,
}

impl Server {
    /// Server sauber beenden (laufende Anfragen abschliessen, Datenbank schliessen) und
    /// höchstens `wait` darauf warten.
    pub fn stop(&mut self, wait: Duration) {
        if let Some(s) = self.shutdown.take() {
            s.notify();
            let start = Instant::now();
            while start.elapsed() < wait && port_open(self.port) {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// `~/Library/Application Support/nrzCode`
/// Adresse, über die sich die App beim Server anmeldet (ohne Passwort).
pub fn login_url(port: u16) -> String {
    let token = std::env::var("NRZCODE_DESKTOP_TOKEN").unwrap_or_default();
    format!("http://127.0.0.1:{port}/__desktop/login?token={token}")
}

/// User-Agent des WebViews mit dem Merkmal der App: Der Server lässt im Desktop-Modus nur
/// Anfragen damit zu (nrzcode::desktop::AppOnly) – ein Browser auf localhost kommt nicht hinein.
pub fn user_agent() -> String {
    let token = std::env::var("NRZCODE_DESKTOP_TOKEN").unwrap_or_default();
    let base = if cfg!(windows) {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 Edg/131.0.0.0"
    } else {
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15"
    };
    format!("{base} nrzCode/{} nrzCodeApp/{}", env!("CARGO_PKG_VERSION"), nrzcode::desktop::app_marker(&token))
}

pub fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).expect("Zufallszahlen");
    hex::encode(buf)
}

/// Anmeldename (macOS `USER`, Windows `USERNAME`).
fn user_name() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default()
}

/// Voller Name des Benutzers (macOS: `id -F`; Windows: der Anmeldename).
fn full_name() -> String {
    if cfg!(windows) {
        return user_name();
    }
    std::process::Command::new("/usr/bin/id")
        .arg("-F")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("NRZCODE_DATA_DIR") {
        return PathBuf::from(dir);
    }
    if cfg!(windows) {
        // %APPDATA%\nrzCode
        return std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(home).join("nrzCode");
    }
    home().join("Library/Application Support/nrzCode")
}

/// `~/Library/Logs/nrzCode`
pub fn log_dir() -> PathBuf {
    if cfg!(windows) {
        // %LOCALAPPDATA%\nrzCode\Logs
        return std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(home).join("nrzCode").join("Logs");
    }
    home().join("Library/Logs/nrzCode")
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// Umgebung setzen. Muss vor dem Start weiterer Threads laufen (`set_var`).
pub fn prepare_env() -> Result<(u16, u16), String> {
    let data = data_dir();
    std::fs::create_dir_all(&data).map_err(|e| format!("{}: {e}", data.display()))?;

    // Rocket.toml des Projekts (Limits, Shutdown) im Datenordner ablegen und benutzen
    let config = data.join("Rocket.toml");
    std::fs::write(&config, include_str!("../../Rocket.toml")).map_err(|e| format!("{}: {e}", config.display()))?;

    let port = free_port(PREFERRED_PORT).ok_or("Kein freier Port für den Server gefunden")?;
    let preview = free_port(PREFERRED_PREVIEW_PORT).filter(|p| *p != port).unwrap_or(0);

    let path = login_path();
    let set = |k: &str, v: &str| std::env::set_var(k, v);
    set("PATH", &path);
    if std::env::var("LANG").map(|l| l.is_empty()).unwrap_or(true) {
        set("LANG", "en_US.UTF-8");
    }
    set("NRZCODE_CONFIG", &config.to_string_lossy());
    set("NRZCODE_PROFILE", "release");
    set("NRZCODE_DATA_DIR", &data.to_string_lossy());
    set("NRZCODE_ADDRESS", "127.0.0.1");
    set("NRZCODE_PORT", &port.to_string());
    set("NRZCODE_PREVIEW_PORT", &preview.to_string());
    // Kein Ctrl-C/Signal-Handling durch Rocket – das Beenden steuert die App
    set("NRZCODE_SHUTDOWN", "{ctrlc=false,signals=[],grace=3,mercy=2}");
    // Desktop-Modus: Anmeldung ohne Passwort über ein Token, das nur diese App kennt
    set("NRZCODE_DESKTOP_TOKEN", &random_hex(32));
    set("NRZCODE_DESKTOP_USER", &user_name());
    set("NRZCODE_DESKTOP_FULL_NAME", &full_name());
    Ok((port, preview))
}

/// Startet den Server auf einem eigenen Thread und wartet, bis er antwortet.
pub fn start(port: u16) -> Result<Server, String> {
    let (tx, rx) = std::sync::mpsc::channel::<Result<rocket::Shutdown, String>>();
    std::thread::Builder::new()
        .name("nrzcode-server".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    return;
                }
            };
            rt.block_on(async move {
                let rocket = match nrzcode::build().await {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                        return;
                    }
                };
                let rocket = match rocket.ignite().await {
                    Ok(r) => r,
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string()));
                        return;
                    }
                };
                let _ = tx.send(Ok(rocket.shutdown()));
                if let Err(e) = rocket.launch().await {
                    tracing::error!("Server beendet: {e}");
                }
            });
        })
        .map_err(|e| e.to_string())?;

    let shutdown = rx.recv_timeout(Duration::from_secs(60)).map_err(|_| "Server startet nicht".to_string())??;
    let start = Instant::now();
    while !port_open(port) {
        if start.elapsed() > Duration::from_secs(30) {
            return Err(format!("Server antwortet nicht auf Port {port}"));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Ok(Server { port, shutdown: Some(shutdown) })
}

fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(&SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(200)).is_ok()
}

fn port_free(port: u16) -> bool {
    TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).is_ok()
        && TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).is_ok()
}

/// Bevorzugten Port nehmen, sonst die nächsten zwanzig probieren, sonst einen vom System.
fn free_port(preferred: u16) -> Option<u16> {
    (preferred..preferred.saturating_add(20))
        .find(|p| port_free(*p))
        .or_else(|| TcpListener::bind("127.0.0.1:0").ok().and_then(|l| l.local_addr().ok()).map(|a| a.port()))
}

/// Apps aus dem Finder bekommen nur `/usr/bin:/bin:/usr/sbin:/sbin`. Die Agenten brauchen
/// node/npm, git, docker … – darum den PATH der Login-Shell übernehmen und übliche Ordner ergänzen.
#[cfg(windows)]
fn login_path() -> String {
    // Windows-Apps bekommen den vollen PATH des Benutzers; übliche Ordner für node/npm/git ergänzen
    let mut parts: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    for extra in [
        env("ProgramFiles").map(|p| p.join("nodejs")),
        env("APPDATA").map(|p| p.join("npm")),
        env("ProgramFiles").map(|p| p.join("Git").join("cmd")),
        env("LOCALAPPDATA").map(|p| p.join("Programs").join("Git").join("cmd")),
        env("USERPROFILE").map(|p| p.join(".cargo").join("bin")),
        env("USERPROFILE").map(|p| p.join(".local").join("bin")),
    ]
    .into_iter()
    .flatten()
    {
        if extra.is_dir() && !parts.contains(&extra) {
            parts.push(extra);
        }
    }
    std::env::join_paths(parts).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(not(windows))]
fn login_path() -> String {
    let mut parts: Vec<String> = Vec::new();
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    if let Some(p) = shell_path(&shell) {
        parts.extend(p.split(':').map(str::to_string));
    }
    let home = home();
    for extra in [
        "/opt/homebrew/bin",
        "/opt/homebrew/sbin",
        "/usr/local/bin",
        &home.join(".cargo/bin").to_string_lossy(),
        &home.join(".local/bin").to_string_lossy(),
        "/Applications/Docker.app/Contents/Resources/bin",
        "/usr/bin",
        "/bin",
        "/usr/sbin",
        "/sbin",
    ] {
        parts.push(extra.to_string());
    }
    if let Ok(cur) = std::env::var("PATH") {
        parts.extend(cur.split(':').map(str::to_string));
    }
    let mut seen = std::collections::HashSet::new();
    parts.retain(|p| !p.is_empty() && seen.insert(p.clone()) && Path::new(p).is_dir());
    parts.join(":")
}

#[cfg(not(windows))]
fn shell_path(shell: &str) -> Option<String> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(shell)
        .args(["-ilc", "printf '__NRZ_PATH__%s__NRZ_PATH__' \"$PATH\""])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                return None;
            }
        }
    }
    let out = child.wait_with_output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut it = text.split("__NRZ_PATH__");
    it.next()?;
    it.next().map(str::to_string).filter(|p| !p.is_empty())
}
