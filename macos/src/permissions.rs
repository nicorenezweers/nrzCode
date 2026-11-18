//! Rechte von macOS (Datenschutz & Sicherheit) vorab abfragen – unter nrzCode → Einstellungen …
//!
//! Die Agenten laufen als Kindprozesse der App; macOS schreibt ihre Zugriffe der App zu und fragt
//! sonst erst mitten in einem Lauf nach (oder ein Agent wartet unbemerkt auf den Dialog). Hier lassen
//! sich die Rechte im Voraus erteilen:
//!
//! * Schreibtisch, Dokumente, Downloads: Ordner einmal lesen → macOS zeigt den Dialog. Beim Start
//!   fragt die App fehlende Ordner von selbst an ([`ask_missing_folders`]) – erteilt bleibt erteilt,
//!   solange die Signatur der App gleich bleibt (scripts/macos.sh signiert dafür stabil).
//! * Mikrofon (Sprachsteuerung „Dieter“): AVCaptureDevice, Dialog von macOS.
//! * Automation: `osascript` spricht System Events und Finder an → Dialog je Ziel-App.
//! * Bedienungshilfen, Bildschirmaufnahme: Systemaufruf mit Dialog, Freigabe in den Systemeinstellungen.
//! * Festplattenvollzugriff: lässt sich nicht anfragen, nur in den Systemeinstellungen setzen.
//!
//! Ohne Rückfrage prüfbar sind Festplattenvollzugriff, Mikrofon, Bedienungshilfen und Bildschirmaufnahme;
//! Ordner und Automation zeigen ihren Zustand nach der ersten Abfrage (gemerkt für die Laufzeit).

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Erteilt.
    Granted,
    /// Verweigert (oder noch nicht in den Systemeinstellungen eingeschaltet).
    Denied,
    /// Noch nicht abgefragt bzw. ohne Abfrage nicht feststellbar.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Desktop,
    Documents,
    Downloads,
    Microphone,
    Automation,
    Accessibility,
    ScreenRecording,
    FullDisk,
}

pub const ALL: [Kind; 8] = [
    Kind::Desktop,
    Kind::Documents,
    Kind::Downloads,
    Kind::Microphone,
    Kind::Automation,
    Kind::Accessibility,
    Kind::ScreenRecording,
    Kind::FullDisk,
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: &'static str,
    pub name: &'static str,
    pub detail: &'static str,
    pub state: State,
    /// Lässt sich direkt anfragen (sonst nur über die Systemeinstellungen).
    pub requestable: bool,
}

impl Kind {
    pub fn id(self) -> &'static str {
        match self {
            Kind::Desktop => "desktop",
            Kind::Documents => "documents",
            Kind::Downloads => "downloads",
            Kind::Microphone => "microphone",
            Kind::Automation => "automation",
            Kind::Accessibility => "accessibility",
            Kind::ScreenRecording => "screen",
            Kind::FullDisk => "fulldisk",
        }
    }

    pub fn from_id(id: &str) -> Option<Kind> {
        ALL.into_iter().find(|k| k.id() == id)
    }

    fn name(self) -> &'static str {
        match self {
            Kind::Desktop => "Schreibtisch",
            Kind::Documents => "Dokumente",
            Kind::Downloads => "Downloads",
            Kind::Microphone => "Mikrofon",
            Kind::Automation => "Automation",
            Kind::Accessibility => "Bedienungshilfen",
            Kind::ScreenRecording => "Bildschirmaufnahme",
            Kind::FullDisk => "Festplattenvollzugriff",
        }
    }

    fn detail(self) -> &'static str {
        match self {
            Kind::Desktop => "Dateien auf dem Schreibtisch lesen und schreiben.",
            Kind::Documents => "Projekte und Dateien im Ordner Dokumente.",
            Kind::Downloads => "Heruntergeladene Dateien und Anhänge.",
            Kind::Microphone => "Sprachsteuerung „Dieter“ – Sätze werden lokal erkannt.",
            Kind::Automation => "Andere Apps über AppleScript steuern (System Events, Finder).",
            Kind::Accessibility => "Tastatur und Maus steuern, z. B. für UI-Tests.",
            Kind::ScreenRecording => "Bildschirmfotos für Agenten. Wirkt nach einem Neustart der App.",
            Kind::FullDisk => "Zugriff auf alle Dateien, auch Mail, Safari und geschützte Ordner.",
        }
    }

    fn requestable(self) -> bool {
        !matches!(self, Kind::FullDisk)
    }

    /// Bereich in Systemeinstellungen → Datenschutz & Sicherheit.
    fn pane(self) -> &'static str {
        match self {
            Kind::Desktop | Kind::Documents | Kind::Downloads => "Privacy_FilesAndFolders",
            Kind::Microphone => "Privacy_Microphone",
            Kind::Automation => "Privacy_Automation",
            Kind::Accessibility => "Privacy_Accessibility",
            Kind::ScreenRecording => "Privacy_ScreenCapture",
            Kind::FullDisk => "Privacy_AllFiles",
        }
    }

    fn folder(self) -> Option<PathBuf> {
        let sub = match self {
            Kind::Desktop => "Desktop",
            Kind::Documents => "Documents",
            Kind::Downloads => "Downloads",
            _ => return None,
        };
        Some(crate::server::home().join(sub))
    }
}

/// Ergebnisse der Abfragen, die sich nicht ohne Dialog prüfen lassen (Ordner, Automation).
static KNOWN: Mutex<Option<HashMap<&'static str, State>>> = Mutex::new(None);

fn remember(kind: Kind, state: State) {
    KNOWN.lock().unwrap().get_or_insert_with(HashMap::new).insert(kind.id(), state);
}

fn remembered(kind: Kind) -> Option<State> {
    KNOWN.lock().unwrap().as_ref().and_then(|m| m.get(kind.id()).copied())
}

/// Zustand ohne Dialog.
pub fn state(kind: Kind) -> State {
    match kind {
        Kind::FullDisk => full_disk(),
        Kind::Accessibility => bool_state(ffi::accessibility(false)),
        Kind::ScreenRecording => bool_state(ffi::screen_preflight()),
        Kind::Microphone => match ffi::mic_status() {
            3 => State::Granted,
            1 | 2 => State::Denied,
            _ => State::Unknown,
        },
        Kind::Desktop | Kind::Documents | Kind::Downloads => {
            // Mit Festplattenvollzugriff sind auch die Ordner frei
            if full_disk() == State::Granted {
                return State::Granted;
            }
            remembered(kind).unwrap_or(State::Unknown)
        }
        Kind::Automation => remembered(kind).unwrap_or(State::Unknown),
    }
}

pub fn list() -> Vec<Entry> {
    ALL.into_iter()
        .map(|k| Entry { id: k.id(), name: k.name(), detail: k.detail(), state: state(k), requestable: k.requestable() })
        .collect()
}

/// Recht anfragen. Kann blockieren, bis der Dialog beantwortet ist – nicht im Event-Loop aufrufen.
pub fn request(kind: Kind) -> State {
    let state = match kind {
        Kind::Desktop | Kind::Documents | Kind::Downloads => {
            let dir = kind.folder().unwrap_or_default();
            // Lesen des Inhalts löst den Dialog aus; macOS hält den Aufruf an, bis er beantwortet ist
            match std::fs::read_dir(&dir) {
                Ok(mut it) => {
                    let _ = it.next();
                    State::Granted
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::Granted,
                Err(e) => {
                    tracing::info!("Zugriff auf {}: {e}", dir.display());
                    State::Denied
                }
            }
        }
        Kind::Automation => {
            let mut state = State::Granted;
            for app in ["System Events", "Finder"] {
                if !automate(app) {
                    state = State::Denied;
                }
            }
            state
        }
        Kind::Accessibility => bool_state(ffi::accessibility(true)),
        Kind::Microphone => match ffi::mic_status() {
            0 => bool_state(ffi::mic_request()),
            _ => state(kind),
        },
        Kind::ScreenRecording => {
            if ffi::screen_preflight() {
                State::Granted
            } else {
                bool_state(ffi::screen_request())
            }
        }
        Kind::FullDisk => {
            open_settings(kind);
            full_disk()
        }
    };
    if matches!(kind, Kind::Desktop | Kind::Documents | Kind::Downloads | Kind::Automation) {
        remember(kind, state);
    }
    state
}

/// Alle anfragbaren Rechte nacheinander (jeder Dialog einzeln).
pub fn request_all() {
    for k in ALL {
        if k.requestable() && state(k) != State::Granted {
            request(k);
        }
    }
}

/// Fehlende Ordnerrechte anfragen (Schreibtisch, Dokumente, Downloads) – ohne Dialog, wenn sie
/// schon erteilt sind; noch nie gefragt → macOS fragt jetzt. Ergebnis: verweigerte Ordner.
/// Blockiert, bis die Dialoge beantwortet sind.
pub fn ask_missing_folders() -> Vec<Kind> {
    if full_disk() == State::Granted {
        return Vec::new();
    }
    [Kind::Desktop, Kind::Documents, Kind::Downloads].into_iter().filter(|k| request(*k) == State::Denied).collect()
}

/// Hinweis auf verweigerte Ordner einmal je Version zeigen (Datei im Datenordner merkt es sich).
pub fn hint_due() -> bool {
    let file = crate::server::data_dir().join(".permission-hint");
    let version = env!("CARGO_PKG_VERSION");
    if std::fs::read_to_string(&file).is_ok_and(|v| v.trim() == version) {
        return false;
    }
    let _ = std::fs::write(&file, version);
    true
}

pub fn open_settings(kind: Kind) {
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{}", kind.pane());
    let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
}

fn bool_state(b: bool) -> State {
    if b {
        State::Granted
    } else {
        State::Denied
    }
}

/// Festplattenvollzugriff: Die TCC-Datenbank des Benutzers ist nur damit lesbar (ohne Dialog).
fn full_disk() -> State {
    let db = crate::server::home().join("Library/Application Support/com.apple.TCC/TCC.db");
    bool_state(std::fs::File::open(db).is_ok())
}

/// Ein Apple Event an `app` senden; beim ersten Mal fragt macOS nach.
fn automate(app: &str) -> bool {
    let script = format!("tell application \"{app}\" to get name");
    match std::process::Command::new("/usr/bin/osascript").arg("-e").arg(script).output() {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            tracing::info!("Automation {app}: {}", String::from_utf8_lossy(&o.stderr).trim());
            false
        }
        Err(e) => {
            tracing::warn!("osascript: {e}");
            false
        }
    }
}

#[cfg(target_os = "macos")]
mod ffi {
    use std::ffi::c_void;

    type CFTypeRef = *const c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: CFTypeRef;
        fn AXIsProcessTrusted() -> u8;
        fn AXIsProcessTrustedWithOptions(options: CFTypeRef) -> u8;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> u8;
        fn CGRequestScreenCaptureAccess() -> u8;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFBooleanTrue: CFTypeRef;
        // Nur die Adresse wird gebraucht
        static kCFTypeDictionaryKeyCallBacks: u8;
        static kCFTypeDictionaryValueCallBacks: u8;
        fn CFDictionaryCreate(
            allocator: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            count: isize,
            key_callbacks: *const u8,
            value_callbacks: *const u8,
        ) -> CFTypeRef;
        fn CFRelease(cf: CFTypeRef);
    }

    /// Bedienungshilfen erlaubt? Mit `prompt` zeigt macOS den Dialog zu den Systemeinstellungen.
    pub fn accessibility(prompt: bool) -> bool {
        unsafe {
            if !prompt {
                return AXIsProcessTrusted() != 0;
            }
            let keys = [kAXTrustedCheckOptionPrompt];
            let values = [kCFBooleanTrue];
            let dict = CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &raw const kCFTypeDictionaryKeyCallBacks,
                &raw const kCFTypeDictionaryValueCallBacks,
            );
            let ok = AXIsProcessTrustedWithOptions(dict) != 0;
            if !dict.is_null() {
                CFRelease(dict);
            }
            ok
        }
    }

    pub fn screen_preflight() -> bool {
        unsafe { CGPreflightScreenCaptureAccess() != 0 }
    }

    pub fn screen_request() -> bool {
        unsafe { CGRequestScreenCaptureAccess() != 0 }
    }

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {
        static AVMediaTypeAudio: &'static objc2_foundation::NSString;
    }

    /// AVAuthorizationStatus für Audio: 0 = nicht gefragt, 1 = eingeschränkt, 2 = verweigert, 3 = erlaubt.
    pub fn mic_status() -> isize {
        let Some(cls) = objc2::runtime::AnyClass::get(c"AVCaptureDevice") else { return 0 };
        // SAFETY: Klassenmethode von AVFoundation mit einer Konstante als Argument.
        unsafe { objc2::msg_send![cls, authorizationStatusForMediaType: AVMediaTypeAudio] }
    }

    /// Dialog von macOS zeigen; wartet auf die Antwort (höchstens 2 Minuten).
    pub fn mic_request() -> bool {
        let Some(cls) = objc2::runtime::AnyClass::get(c"AVCaptureDevice") else { return false };
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        let tx = std::sync::Mutex::new(Some(tx));
        let block = block2::RcBlock::new(move |granted: objc2::runtime::Bool| {
            if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(granted.as_bool());
            }
        });
        // SAFETY: Klassenmethode von AVFoundation; der Block lebt, bis macOS ihn aufgerufen hat.
        unsafe {
            let _: () = objc2::msg_send![cls, requestAccessForMediaType: AVMediaTypeAudio, completionHandler: &*block];
        }
        rx.recv_timeout(std::time::Duration::from_secs(120)).unwrap_or(false)
    }
}

#[cfg(not(target_os = "macos"))]
mod ffi {
    pub fn accessibility(_prompt: bool) -> bool {
        false
    }
    pub fn screen_preflight() -> bool {
        false
    }
    pub fn screen_request() -> bool {
        false
    }
    pub fn mic_status() -> isize {
        0
    }
    pub fn mic_request() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_roundtrip() {
        for k in ALL {
            assert_eq!(Kind::from_id(k.id()), Some(k));
        }
        assert_eq!(Kind::from_id("x"), None);
    }

    #[test]
    fn list_serializes() {
        let json = serde_json::to_string(&list()).unwrap();
        assert!(json.contains("\"id\":\"desktop\""));
        assert!(json.contains("\"requestable\""));
    }
}
