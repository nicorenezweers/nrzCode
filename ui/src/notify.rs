//! Benachrichtigungen (Modus je Gerät in `localStorage`): Agent fertig, Fehler/Rate-Limit,
//! Rückfrage und Hinweise von `notify_user`. In der macOS-App über die App (native Mitteilung,
//! `window.__nrzNative`), im Browser über die Notification-API. Ein Klick öffnet den Thread.

use crate::i18n::tr;
use crate::util;
use dioxus::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

const KEY: &str = "nrz-notify";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Keine Benachrichtigungen
    Off,
    /// Nur wenn das Fenster nicht im Vordergrund ist oder ein anderer Thread offen ist
    Background,
    /// Immer
    Always,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Off, Mode::Background, Mode::Always];

    pub fn key(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::Background => "background",
            Mode::Always => "always",
        }
    }

    pub fn label(self) -> String {
        match self {
            Mode::Off => tr("Aus"),
            Mode::Background => tr("Wenn ich woanders bin"),
            Mode::Always => tr("Immer"),
        }
    }

    pub fn hint(self) -> String {
        match self {
            Mode::Off => tr("Keine Benachrichtigungen."),
            Mode::Background => tr("Wenn nrzCode im Hintergrund ist oder ein anderer Thread offen ist."),
            Mode::Always => tr("Bei jedem Ereignis, auch im offenen Thread."),
        }
    }
}

/// Aktueller Modus (für die Oberfläche).
pub static MODE: GlobalSignal<Mode> = Signal::global(load);

/// Thread, den eine angeklickte Benachrichtigung öffnen soll (die Seite navigiert dorthin).
pub static OPEN_THREAD: GlobalSignal<Option<i64>> = Signal::global(|| None);

fn load() -> Mode {
    match util::storage_get(KEY).as_deref() {
        Some("off") => Mode::Off,
        Some("always") => Mode::Always,
        _ => Mode::Background,
    }
}

/// Modus setzen; beim Einschalten im Browser um Erlaubnis fragen.
pub fn set_mode(m: Mode) {
    util::storage_set(KEY, m.key());
    *MODE.write() = m;
    if m != Mode::Off {
        request_permission();
    }
}

fn js(args: &str, body: &str) -> js_sys::Function {
    js_sys::Function::new_with_args(args, body)
}

fn request_permission() {
    let f = js(
        "",
        "if (window.__nrzNative) return; if ('Notification' in window && Notification.permission === 'default') Notification.requestPermission();",
    );
    let _ = f.call0(&JsValue::NULL);
}

/// Klicks auf Benachrichtigungen an die Oberfläche weiterreichen (einmal beim Start).
pub fn install() {
    let open = Closure::<dyn FnMut(JsValue)>::new(|id: JsValue| {
        if let Some(id) = id.as_f64() {
            *OPEN_THREAD.write() = Some(id as i64);
        }
    });
    if let Some(w) = web_sys::window() {
        let _ = js_sys::Reflect::set(&w, &JsValue::from_str("__nrzOpenThread"), open.as_ref().unchecked_ref());
    }
    open.forget();
}

/// (Fenster im Vordergrund, dieser Thread geöffnet)
fn view_state(thread_id: i64) -> (bool, bool) {
    let Some(w) = web_sys::window() else { return (false, false) };
    let Some(doc) = w.document() else { return (false, false) };
    let focused = !doc.hidden() && doc.has_focus().unwrap_or(false);
    let path = w.location().pathname().unwrap_or_default();
    (focused, focused && path == format!("/t/{thread_id}"))
}

/// Benachrichtigung je nach Modus: im Hintergrund als Mitteilung des Systems, im Vordergrund als
/// Meldung in der App (`toast_in_app` = false, wenn die App schon selbst eine zeigt).
pub fn send(thread_id: i64, title: &str, body: &str) {
    send_with(thread_id, title, body, true)
}

pub fn send_with(thread_id: i64, title: &str, body: &str, toast_in_app: bool) {
    let mode = *MODE.peek();
    let (focused, looking) = view_state(thread_id);
    match mode {
        Mode::Off => return,
        Mode::Background if looking => return,
        _ => {}
    }
    let body: String = body.chars().take(240).collect();
    if focused {
        if toast_in_app {
            crate::components::overlay::toast(format!("{title} – {body}"));
        }
        return;
    }
    let f = js(
        "t, b, id",
        "if (window.__nrzNative && window.__nrzNative.notify) { window.__nrzNative.notify(t, b, id); return; }
         if (!('Notification' in window) || Notification.permission !== 'granted') return;
         var n = new Notification(t, { body: b, tag: 'nrz-' + id + '-' + Date.now(), icon: '/static/icon.png' });
         n.onclick = function () { window.focus(); if (window.__nrzOpenThread) window.__nrzOpenThread(id); n.close(); };",
    );
    let _ = f.call3(&JsValue::NULL, &JsValue::from_str(title), &JsValue::from_str(&body), &JsValue::from_f64(thread_id as f64));
}
