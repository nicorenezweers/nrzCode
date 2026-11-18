//! Code-Sperre der App: optionaler 6-stelliger Code, der nach einer einstellbaren Zeit ohne
//! Eingabe (Standard 10 Minuten) und beim Start abgefragt wird. Die Sperre legt sich nur über die
//! Oberfläche – Server, Sitzung und laufende Agenten arbeiten weiter.
//!
//! Einstellungen in `<Datenordner>/desktop.json`; vom Code liegt nur ein gesalzener Hash dort.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub const DEFAULT_MINUTES: u32 = 10;
/// Wartezeit nach fünf falschen Eingaben in Folge.
const MAX_ATTEMPTS: u32 = 5;
const BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Hash des Codes (leer = keine Sperre).
    pub pin_hash: String,
    pub pin_salt: String,
    /// Minuten ohne Eingabe bis zur Sperre.
    pub lock_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { pin_hash: String::new(), pin_salt: String::new(), lock_minutes: DEFAULT_MINUTES }
    }
}

impl Settings {
    pub fn enabled(&self) -> bool {
        !self.pin_hash.is_empty()
    }
}

pub struct Lock {
    pub settings: Settings,
    pub locked: bool,
    /// Letzte Eingabe – Wanduhr statt `Instant`, damit auch der Ruhezustand des Macs zählt.
    last_activity: SystemTime,
    failed: u32,
    blocked_until: Option<SystemTime>,
}

fn path() -> PathBuf {
    crate::server::data_dir().join("desktop.json")
}

fn hash(salt: &str, pin: &str) -> String {
    // Viele Runden, damit Durchprobieren des Hashs aus der Datei wenigstens etwas kostet
    let mut h = Sha256::digest(format!("{salt}:{pin}").as_bytes());
    for _ in 0..200_000 {
        h = Sha256::digest(h);
    }
    hex::encode(h)
}

pub fn valid_pin(pin: &str) -> bool {
    pin.len() == 6 && pin.chars().all(|c| c.is_ascii_digit())
}

impl Lock {
    pub fn load() -> Self {
        let settings: Settings =
            std::fs::read(path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { locked: settings.enabled(), settings, last_activity: SystemTime::now(), failed: 0, blocked_until: None }
    }

    fn save(&self) -> Result<(), String> {
        let data = serde_json::to_vec_pretty(&self.settings).map_err(|e| e.to_string())?;
        let p = path();
        std::fs::write(&p, data).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn activity(&mut self) {
        if !self.locked {
            self.last_activity = SystemTime::now();
        }
    }

    /// true, wenn die Sperre jetzt greifen soll.
    pub fn due(&self) -> bool {
        self.settings.enabled()
            && !self.locked
            && self.settings.lock_minutes > 0
            && self.last_activity.elapsed().unwrap_or_default() >= Duration::from_secs(self.settings.lock_minutes as u64 * 60)
    }

    pub fn lock_now(&mut self) -> bool {
        if self.settings.enabled() {
            self.locked = true;
        }
        self.locked
    }

    fn check(&mut self, pin: &str) -> Result<(), String> {
        if let Some(until) = self.blocked_until {
            if let Ok(rest) = until.duration_since(SystemTime::now()) {
                return Err(format!("Zu viele Versuche – bitte {} s warten.", rest.as_secs() + 1));
            }
            self.blocked_until = None;
        }
        if valid_pin(pin) && hash(&self.settings.pin_salt, pin) == self.settings.pin_hash {
            self.failed = 0;
            return Ok(());
        }
        self.failed += 1;
        if self.failed >= MAX_ATTEMPTS {
            self.failed = 0;
            self.blocked_until = Some(SystemTime::now() + BACKOFF);
            return Err(format!("Falscher Code – bitte {} s warten.", BACKOFF.as_secs()));
        }
        Err("Falscher Code.".into())
    }

    /// Entsperren mit dem Code.
    pub fn unlock(&mut self, pin: &str) -> Result<(), String> {
        self.check(pin)?;
        self.locked = false;
        self.last_activity = SystemTime::now();
        Ok(())
    }

    /// Einstellungen ändern. Ist schon ein Code gesetzt, muss `current` stimmen.
    /// `new_pin`: None = Code behalten, Some("") = Sperre aus, Some(code) = neuer Code.
    pub fn update(&mut self, current: &str, new_pin: Option<&str>, minutes: u32) -> Result<(), String> {
        if self.settings.enabled() {
            self.check(current)?;
        }
        if !(1..=24 * 60).contains(&minutes) {
            return Err("Zeit bis zur Sperre: 1 bis 1440 Minuten.".into());
        }
        match new_pin {
            Some("") => {
                self.settings.pin_hash.clear();
                self.settings.pin_salt.clear();
            }
            Some(p) if valid_pin(p) => {
                self.settings.pin_salt = crate::server::random_hex(16);
                self.settings.pin_hash = hash(&self.settings.pin_salt, p);
            }
            Some(_) => return Err("Der Code muss genau 6 Ziffern haben.".into()),
            None => {}
        }
        self.settings.lock_minutes = minutes;
        self.last_activity = SystemTime::now();
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock() -> Lock {
        Lock { settings: Settings::default(), locked: false, last_activity: SystemTime::now(), failed: 0, blocked_until: None }
    }

    #[test]
    fn pin_roundtrip() {
        let mut l = lock();
        l.settings.pin_salt = "s".into();
        l.settings.pin_hash = hash("s", "123456");
        assert!(l.lock_now());
        assert!(l.unlock("000000").is_err());
        assert!(l.locked);
        assert!(l.unlock("123456").is_ok());
        assert!(!l.locked);
    }

    #[test]
    fn backoff_after_five_failures() {
        let mut l = lock();
        l.settings.pin_salt = "s".into();
        l.settings.pin_hash = hash("s", "123456");
        for _ in 0..5 {
            let _ = l.unlock("111111");
        }
        assert!(l.unlock("123456").unwrap_err().contains("warten"));
    }

    #[test]
    fn due_after_inactivity() {
        let mut l = lock();
        l.settings.pin_hash = "x".into();
        l.settings.lock_minutes = 10;
        assert!(!l.due());
        l.last_activity = SystemTime::now() - Duration::from_secs(601);
        assert!(l.due());
    }

    #[test]
    fn known_hash() {
        // Gleiche Datei-Formate über Versionen hinweg: Hash von "123456" mit Salz "abc"
        assert_eq!(hash("abc", "123456"), "3fc06dad73edcbc5fe46070e2f9b5d4f6d964e7a969a489b16f549eddec3ad4e");
    }

    #[test]
    fn pin_format() {
        assert!(valid_pin("012345"));
        assert!(!valid_pin("12345"));
        assert!(!valid_pin("12345a"));
    }
}
