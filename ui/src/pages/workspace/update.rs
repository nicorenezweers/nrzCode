//! „Nach Updates suchen“ unten in der Seitenleiste (nur in der App): fragt die App ohne Dialog
//! (`update:check:quiet`), zeigt währenddessen „Suche nach Updates …“, danach kurz „nrzCode ist
//! aktuell“ und verschwindet dann (bis zum nächsten Start). Findet die App ein Update – auch bei
//! ihrer eigenen, regelmässigen Prüfung –, steht dort „Update auf … installieren“.
//!
//! Die App meldet Ergebnisse über `window.__nrzUpdateState(status, text)`: `available` (Version),
//! `current` (eigene Version) oder `error` (Text); `update:state` fragt nach einem schon
//! gefundenen Update (nach dem Neuladen der Oberfläche).

use super::layout::FOOT_ROW;
use crate::i18n::{tr, trf};
use crate::util;
use dioxus::prelude::*;

#[derive(Clone, PartialEq, Debug)]
pub enum UpdateUi {
    /// Noch nicht geprüft: „Nach Updates suchen“.
    Idle,
    Checking,
    /// Geprüft, nichts Neues: kurz zeigen, dann `Done`.
    Current,
    Available(String),
    Error(String),
    /// Ausgeblendet.
    Done,
}

pub static UPDATE: GlobalSignal<UpdateUi> = Signal::global(|| UpdateUi::Idle);

/// Rückmeldungen der App annehmen und nach einem schon gefundenen Update fragen.
pub fn install() {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    if !util::in_app() {
        return;
    }
    let Some(w) = web_sys::window() else { return };
    let cb = Closure::<dyn FnMut(String, String)>::new(|status: String, text: String| {
        let next = match status.as_str() {
            "available" => UpdateUi::Available(text),
            "current" => UpdateUi::Current,
            "error" => UpdateUi::Error(text),
            _ => return,
        };
        *UPDATE.write() = next;
    });
    let _ = js_sys::Reflect::set(&w, &"__nrzUpdateState".into(), cb.as_ref().unchecked_ref());
    cb.forget();
    util::native("update:state");
}

/// Jetzt prüfen (ohne Dialog der App); ohne Antwort nach 40 s als Fehler.
fn check() {
    *UPDATE.write() = UpdateUi::Checking;
    if !util::native("update:check:quiet") {
        *UPDATE.write() = UpdateUi::Error(tr("Die App antwortet nicht."));
        return;
    }
    spawn(async {
        gloo_timers::future::TimeoutFuture::new(40_000).await;
        if *UPDATE.peek() == UpdateUi::Checking {
            *UPDATE.write() = UpdateUi::Error(tr("Keine Antwort vom Update-Server."));
        }
    });
}

/// „Aktuell“ kurz zeigen, dann ausblenden.
fn use_fade_current() {
    use_effect(move || {
        if UPDATE() == UpdateUi::Current {
            spawn(async {
                gloo_timers::future::TimeoutFuture::new(2_500).await;
                if *UPDATE.peek() == UpdateUi::Current {
                    *UPDATE.write() = UpdateUi::Done;
                }
            });
        }
    });
}

/// Knopf in der Leiste links über dem Nutzungsring (nur in der App, bis nach der Prüfung):
/// Pfeile = suchen, dreht sich beim Suchen, Häkchen = aktuell, Pfeil nach unten = Update
/// installieren, Warnung = Fehler (Klick prüft erneut).
#[component]
pub fn UpdateRailButton(#[props(into)] class: String) -> Element {
    use_fade_current();
    if !util::in_app() {
        return rsx! {};
    }
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    match UPDATE() {
        UpdateUi::Done => rsx! {},
        UpdateUi::Idle => rsx! {
            button { class: "{class}", title: format!("{} ({version})", tr("Nach Updates suchen")), onclick: move |_| check(),
                icons::RefreshCw {}
            }
        },
        UpdateUi::Checking => rsx! {
            span { class: "{class} cursor-default", title: tr("Suche nach Updates …"),
                icons::RefreshCw { class: "animate-spin" }
            }
        },
        UpdateUi::Current => rsx! {
            span { class: "{class} nrz-fade-out cursor-default", title: tr("nrzCode ist aktuell"),
                icons::Check { class: "text-success" }
            }
        },
        UpdateUi::Available(v) => rsx! {
            button {
                class: "{class} relative text-info hover:text-info",
                title: trf("Update auf {0} installieren", &[&v]),
                onclick: move |_| {
                    util::native("update:install");
                },
                icons::CircleArrowDown {}
                span { class: "absolute right-1.5 top-1.5 size-2 rounded-full bg-info" }
            }
        },
        UpdateUi::Error(e) => rsx! {
            button { class: "{class} text-destructive hover:text-destructive", title: format!("{} – {e}", tr("Prüfung fehlgeschlagen – erneut")), onclick: move |_| check(),
                icons::CircleAlert {}
            }
        },
    }
}

/// Zeile über „Einstellungen“ (Seitenleiste auf dem Handy) – nur in der App und bis nach der Prüfung.
#[component]
pub fn UpdateRow() -> Element {
    use_fade_current();
    if !util::in_app() {
        return rsx! {};
    }
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let side = "shrink-0 text-xs tabular-nums text-muted-foreground/70";
    match UPDATE() {
        UpdateUi::Done => rsx! {},
        UpdateUi::Idle => rsx! {
            button { class: FOOT_ROW, onclick: move |_| check(),
                icons::RefreshCw {}
                span { class: "min-w-0 flex-1 truncate", {tr("Nach Updates suchen")} }
                span { class: side, "{version}" }
            }
        },
        UpdateUi::Checking => rsx! {
            div { class: "{FOOT_ROW} cursor-default hover:bg-transparent hover:text-muted-foreground",
                icons::RefreshCw { class: "animate-spin" }
                span { class: "min-w-0 flex-1 truncate", {tr("Suche nach Updates …")} }
                span { class: side, "{version}" }
            }
        },
        UpdateUi::Current => rsx! {
            div { class: "{FOOT_ROW} nrz-fade-out cursor-default hover:bg-transparent hover:text-muted-foreground",
                icons::Check { class: "text-success" }
                span { class: "min-w-0 flex-1 truncate", {tr("nrzCode ist aktuell")} }
                span { class: side, "{version}" }
            }
        },
        UpdateUi::Available(v) => rsx! {
            button {
                class: "{FOOT_ROW} text-info hover:text-info",
                title: tr("Lädt das Update und startet nrzCode neu."),
                onclick: move |_| {
                    util::native("update:install");
                },
                icons::CircleArrowDown {}
                span { class: "min-w-0 flex-1 truncate font-medium", {trf("Update auf {0} installieren", &[&v])} }
            }
        },
        UpdateUi::Error(e) => rsx! {
            button { class: "{FOOT_ROW} text-destructive hover:text-destructive", title: "{e}", onclick: move |_| check(),
                icons::CircleAlert {}
                span { class: "min-w-0 flex-1 truncate", {tr("Prüfung fehlgeschlagen – erneut")} }
                span { class: side, "{version}" }
            }
        },
    }
}
