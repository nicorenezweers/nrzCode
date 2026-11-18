//! Globaler Zustand: angemeldeter Benutzer, Instanz-Status, Threads und Projekte (live über
//! `events.rs`), Nachrichten geöffneter Threads, Vorschau.

use dioxus::prelude::*;
use nrzcode_shared::{AuthStatus, Me, Message, ProjectSummary, ThreadSummary};
use std::collections::{BTreeMap, HashMap};

/// Angemeldeter Benutzer (None = abgemeldet oder noch nicht geladen, siehe `ME_LOADED`).
pub static ME: GlobalSignal<Option<Me>> = Signal::global(|| None);
pub static ME_LOADED: GlobalSignal<bool> = Signal::global(|| false);
pub static STATUS: GlobalSignal<Option<AuthStatus>> = Signal::global(|| None);

/// Alle Threads des Benutzers (Seitenleiste), nach ID. Live aktualisiert.
pub static THREADS: GlobalSignal<BTreeMap<i64, ThreadSummary>> = Signal::global(BTreeMap::new);
/// Alle Projekte des Benutzers, nach ID. Live aktualisiert.
pub static PROJECTS: GlobalSignal<BTreeMap<i64, ProjectSummary>> = Signal::global(BTreeMap::new);
/// Nachrichten geöffneter Threads (nur Threads, deren Verlauf geladen wurde, bekommen
/// neue Nachrichten aus den Ereignissen angehängt bzw. ersetzt).
pub static MESSAGES: GlobalSignal<HashMap<i64, Vec<Message>>> = Signal::global(HashMap::new);
/// Threads, deren Verlauf vollständig geladen ist (danach live über die Ereignisse).
pub static LOADED: GlobalSignal<std::collections::HashSet<i64>> = Signal::global(std::collections::HashSet::new);
/// Änderungen je Thread: (Zusätze, Löschungen, Dateien).
pub static CHANGES: GlobalSignal<HashMap<i64, (i64, i64, i64)>> = Signal::global(HashMap::new);
/// Zuletzt von einem Agenten geöffnete Vorschau: (Thread, Port, Pfad).
pub static PREVIEW: GlobalSignal<Option<(i64, u16, String)>> = Signal::global(|| None);
/// Team je Projekt: zählt hoch, wenn der Server eine Änderung meldet (der Team-Reiter lädt dann neu).
pub static TEAM_REV: GlobalSignal<HashMap<i64, u64>> = Signal::global(HashMap::new);

/// Neuer Thread, der gerade angelegt wird: Seitenleiste und Startseite zeigen ihn sofort mit
/// der gesendeten Nachricht, bis der Server antwortet und in den echten Chat gewechselt wird.
#[derive(Clone, PartialEq)]
pub struct PendingThread {
    pub project_id: Option<i64>,
    pub title: String,
    pub text: String,
    pub attachments: Vec<nrzcode_shared::Attachment>,
    pub agent: nrzcode_shared::AgentKind,
    pub created_at: i64,
    /// Threads, die es beim Senden schon gab (ein neu auftauchender mit gleichem Titel ersetzt
    /// den Platzhalter in der Seitenleiste, auch wenn sein Ereignis vor der Antwort kommt)
    pub known: std::collections::HashSet<i64>,
}

pub static PENDING_THREAD: GlobalSignal<Option<PendingThread>> = Signal::global(|| None);

/// Schon in der Liste angekommener echter Thread des Platzhalters (falls ja).
pub fn pending_arrived() -> Option<i64> {
    let p = PENDING_THREAD.read().clone()?;
    THREADS.read().values().find(|t| t.parent_id.is_none() && !p.known.contains(&t.id) && t.title == p.title).map(|t| t.id)
}

/// Platzhalter noch zeigen? (Nein, sobald der echte Thread in der Liste steht.)
pub fn pending_visible() -> Option<PendingThread> {
    let p = PENDING_THREAD.read().clone()?;
    let arrived = THREADS.read().values().any(|t| t.parent_id.is_none() && !p.known.contains(&t.id) && t.title == p.title);
    (!arrived).then_some(p)
}

/// Erhöhen, damit Listen neu laden (nach Änderungen).
pub static REFRESH: GlobalSignal<u64> = Signal::global(|| 0);

/// Ansicht: Chat-Modus (Seitenleiste, eine Unterhaltung) oder Canvas-Modus (alle Fenster eines
/// Arbeitsbereichs frei auf einer Fläche). Dieselben Threads – nur anders gezeigt.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UiMode {
    Chat,
    Canvas,
}

const MODE_KEY: &str = "nrz-mode";

pub static MODE: GlobalSignal<UiMode> =
    Signal::global(|| if crate::util::storage_get(MODE_KEY).as_deref() == Some("canvas") { UiMode::Canvas } else { UiMode::Chat });

/// Modus setzen und merken (die Seite wechselt der Aufrufer).
pub fn set_mode(m: UiMode) {
    crate::util::storage_set(MODE_KEY, if m == UiMode::Canvas { "canvas" } else { "chat" });
    if *MODE.peek() != m {
        *MODE.write() = m;
    }
}

/// Arbeitsbereich der Seitenleiste: Code (Threads in Projekten) oder Bot (eigene KI-Teamkollegen
/// mit Gedächtnis). Umschalter über der Suche.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Work {
    Code,
    Bot,
}

const WORK_KEY: &str = "nrz-work";

pub static WORK: GlobalSignal<Work> =
    Signal::global(|| if crate::util::storage_get(WORK_KEY).as_deref() == Some("bot") { Work::Bot } else { Work::Code });

/// Arbeitsbereich setzen und merken (die Seite wechselt der Aufrufer).
pub fn set_work(w: Work) {
    crate::util::storage_set(WORK_KEY, if w == Work::Bot { "bot" } else { "code" });
    if *WORK.peek() != w {
        *WORK.write() = w;
    }
}

/// Bots des Benutzers (`GET /api/v1/bots`), `None` = noch nicht geladen.
pub static BOTS: GlobalSignal<Option<Vec<nrzcode_shared::agents::Bot>>> = Signal::global(|| None);

/// Bots neu laden.
pub async fn load_bots() {
    if let Ok(list) = crate::api::get::<Vec<nrzcode_shared::agents::Bot>>("/bots").await {
        *BOTS.write() = Some(list);
    }
}

/// Einen geänderten Bot in die Liste übernehmen.
pub fn put_bot(b: nrzcode_shared::agents::Bot) {
    let mut w = BOTS.write();
    let list = w.get_or_insert_with(Vec::new);
    match list.iter_mut().find(|x| x.id == b.id) {
        Some(x) => *x = b,
        None => list.push(b),
    }
    list.sort_by_key(|x| (x.name.to_lowercase(), x.id));
}

pub fn refresh() {
    *REFRESH.write() += 1;
}

pub fn signed_in() -> bool {
    ME.read().is_some()
}

/// Nach 401: zur Anmeldung (ausser auf der Anmeldung selbst).
pub fn on_unauthorized() {
    let Some(window) = web_sys::window() else { return };
    let path = window.location().pathname().unwrap_or_default();
    if path.starts_with("/user/login") || path.starts_with("/install") {
        return;
    }
    *ME.write() = None;
    let next = crate::api::enc(&crate::util::current_path());
    let _ = window.location().assign(&format!("/user/login?next={next}"));
}

pub async fn load_status() -> Option<AuthStatus> {
    match crate::api::get::<AuthStatus>("/auth/status").await {
        Ok(s) => {
            *STATUS.write() = Some(s.clone());
            Some(s)
        }
        Err(_) => None,
    }
}

/// `/me` laden – ohne Anmeldung `None` (ohne Umleiten).
pub async fn load_me() -> Option<Me> {
    let res = gloo_net::http::Request::get("/api/v1/me").header("Accept", "application/json").send().await;
    let me = match res {
        Ok(r) if r.ok() => r.json::<Me>().await.ok(),
        _ => None,
    };
    *ME.write() = me.clone();
    *ME_LOADED.write() = true;
    if let Some(m) = &me {
        apply_theme(&m.theme);
    }
    me
}

/// Thema anwenden: "light" → `html.light`, "system" → nach Betriebssystem, "mondnacht" →
/// `html.mondnacht` (dunkel, Lunas Farben), sonst dunkel. In der
/// App folgt das Fenster (und damit das Milchglas dahinter) dem Thema (`theme:<thema>`).
pub fn apply_theme(theme: &str) {
    let Some(win) = web_sys::window() else { return };
    let light = match theme {
        "light" => true,
        "system" => win.match_media("(prefers-color-scheme: light)").ok().flatten().is_some_and(|m| m.matches()),
        _ => false,
    };
    crate::util::storage_set("nrz-theme", theme);
    if let Some(el) = win.document().and_then(|d| d.document_element()) {
        let _ = if light { el.class_list().add_1("light") } else { el.class_list().remove_1("light") };
        let _ = if theme == "mondnacht" { el.class_list().add_1("mondnacht") } else { el.class_list().remove_1("mondnacht") };
    }
    let native = match theme {
        "light" | "system" | "mondnacht" => theme,
        _ => "dark",
    };
    crate::util::native(&format!("theme:{native}"));
}

/// Thema „wie das System“: beim Wechsel des Systems (oder wenn die App ihr Fenster vom festen
/// Thema zurück aufs System stellt) neu anwenden.
pub fn watch_system_theme() {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    let Some(mq) = web_sys::window().and_then(|w| w.match_media("(prefers-color-scheme: light)").ok().flatten()) else { return };
    let cb = Closure::<dyn FnMut()>::new(|| {
        if crate::util::storage_get("nrz-theme").as_deref() == Some("system") {
            apply_theme("system");
        }
    });
    let _ = mq.add_event_listener_with_callback("change", cb.as_ref().unchecked_ref());
    cb.forget();
}

/// Einstellungen öffnen (gesetzt von ⌘, im Menü der App bzw. Strg+,; der Rahmen navigiert).
pub static OPEN_SETTINGS: GlobalSignal<bool> = Signal::global(|| false);

/// `window.__nrzOpenSettings()` für die App und Strg+, / ⌘, im Browser.
pub fn install_settings_shortcut() {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    let Some(w) = web_sys::window() else { return };
    let open = Closure::<dyn FnMut()>::new(|| *OPEN_SETTINGS.write() = true);
    let _ = js_sys::Reflect::set(&w, &"__nrzOpenSettings".into(), open.as_ref().unchecked_ref());
    open.forget();
    let key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|e: web_sys::KeyboardEvent| {
        if e.key() == "," && (e.meta_key() || e.ctrl_key()) && !e.alt_key() && !e.shift_key() {
            e.prevent_default();
            *OPEN_SETTINGS.write() = true;
        }
    });
    let _ = w.add_event_listener_with_callback("keydown", key.as_ref().unchecked_ref());
    key.forget();
}

/// Milchglas eingeschaltet (Einstellungen → Erscheinungsbild; nur wirksam, wenn die App es kann).
pub static GLASS_ON: GlobalSignal<bool> = Signal::global(|| crate::util::storage_get("nrz-glass").as_deref() != Some("0"));

/// Milchglas ein-/ausschalten und anwenden.
pub fn set_glass(on: bool) {
    crate::util::storage_set("nrz-glass", if on { "1" } else { "0" });
    *GLASS_ON.write() = on;
    apply_glass();
}

/// `html.nrz-glass`, wenn die App Milchglas hinter das Fenster legt und es eingeschaltet ist.
pub fn apply_glass() {
    let on = crate::util::glass_available() && *GLASS_ON.peek();
    if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
        let _ = if on { el.class_list().add_1("nrz-glass") } else { el.class_list().remove_1("nrz-glass") };
    }
}

/// Thread in der Liste setzen/ersetzen.
pub fn upsert_thread(t: ThreadSummary) {
    THREADS.write().insert(t.id, t);
}

/// Nachricht einfügen oder (gleiche ID, z. B. beim Streamen) ersetzen – nur für geladene Threads.
pub fn upsert_message(m: Message) {
    let mut all = MESSAGES.write();
    if let Some(list) = all.get_mut(&m.thread_id) {
        match list.iter_mut().find(|x| x.id == m.id) {
            Some(x) => *x = m,
            None => list.push(m),
        }
    }
}
