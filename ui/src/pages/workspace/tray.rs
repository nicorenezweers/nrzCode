//! Fenster der Menüleiste (macOS-App, `/tray`): die letzten Threads mit Zustand und Agenten,
//! darunter „Nutzung“ mit Limits, Kosten und Tokens über 24 Stunden bis 90 Tage. Ein Klick auf
//! einen Thread öffnet ihn im Hauptfenster (über `window.ipc`, Nachricht `tray:open:<id>`).
//! Ohne Rahmen der App (keine Seitenleiste) – das Fenster ist klein und schwebt unter dem Symbol.
//! Die App bleibt geladen und ruft beim Zeigen `window.__nrzTrayShown()` auf: dann lädt alles neu
//! (Threads, Limits, Verlauf), solange es sichtbar ist zusätzlich jede Minute.

use super::parts::*;
use crate::components::common::Loading;
use crate::i18n::{self, tr, trf, LANG};
use crate::state::{self, THREADS};
use crate::util;
use dioxus::prelude::*;
use nrzcode_shared::{AgentKind, ThreadSummary};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsValue;

/// Erhöht, sobald die App das Fenster zeigt (`window.__nrzTrayShown()`).
static SHOWN: GlobalSignal<u64> = Signal::global(|| 0);

/// Nachricht an die App (nur in der App vorhanden; im Browser `false`).
fn send(msg: &str) -> bool {
    let Some(w) = web_sys::window() else { return false };
    let Ok(ipc) = js_sys::Reflect::get(&w, &JsValue::from_str("ipc")) else { return false };
    if ipc.is_undefined() || ipc.is_null() {
        return false;
    }
    let Ok(post) = js_sys::Reflect::get(&ipc, &JsValue::from_str("postMessage")) else { return false };
    let Ok(f) = post.dyn_into::<js_sys::Function>() else { return false };
    f.call1(&ipc, &JsValue::from_str(msg)).is_ok()
}

use wasm_bindgen::JsCast;

#[derive(Clone, Copy, PartialEq)]
enum View {
    Threads,
    Usage,
}

#[component]
pub fn Tray() -> Element {
    let mut view = use_signal(|| View::Threads);
    // Anmelden, Daten laden, Live-Ereignisse – wie der Rahmen der App, nur schlanker
    let boot = use_resource(|| async {
        let me = state::load_me().await;
        if let Some(m) = &me {
            if !m.language.is_empty() && *LANG.peek() != m.language {
                i18n::set_language(&m.language, true).await;
            }
            load_projects().await;
            load_threads().await;
            crate::events::connect();
        }
        me.is_some()
    });
    use_future(|| async {
        loop {
            gloo_timers::future::TimeoutFuture::new(1000).await;
            *NOW.write() = util::now();
        }
    });
    // Durchsichtiger Rand: das Fenster selbst ist rund (macOS-Popover)
    use_hook(|| {
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element()) {
            let _ = el.class_list().add_1("nrz-tray");
        }
    });
    // Die App zeigt das Fenster (`window.__nrzTrayShown()`): Der Aufruf kommt von aussen, darum
    // Laufzeit und Bereich mitnehmen
    use_hook(|| {
        let rt = dioxus::dioxus_core::Runtime::current();
        let scope = dioxus::dioxus_core::current_scope_id();
        let shown = Closure::<dyn FnMut()>::new(move || rt.in_scope(scope, || *SHOWN.write() += 1));
        if let Some(w) = web_sys::window() {
            let _ = js_sys::Reflect::set(&w, &JsValue::from_str("__nrzTrayShown"), shown.as_ref());
        }
        shown.forget();
    });
    // Beim Zeigen alles neu laden (nur angemeldet – sonst führte ein 401 zur Anmeldeseite)
    use_effect(move || {
        if SHOWN() == 0 || state::ME.peek().is_none() {
            return;
        }
        spawn(load_threads());
        crate::pages::agents::refresh_usage(false);
    });
    // Sichtbar: jede Minute neu laden
    use_future(|| async {
        loop {
            gloo_timers::future::TimeoutFuture::new(60_000).await;
            let hidden = web_sys::window().and_then(|w| w.document()).is_none_or(|d| d.hidden());
            if !hidden && state::ME.peek().is_some() {
                crate::pages::agents::refresh_usage(false);
            }
        }
    });
    let body = match boot.read().clone() {
        None => rsx! { div { class: "flex flex-1 items-center justify-center", Loading {} } },
        Some(false) => rsx! {
            div { class: "flex flex-1 flex-col items-center justify-center gap-2 p-6 text-center text-sm text-muted-foreground",
                p { {tr("Nicht angemeldet.")} }
                button { class: "rounded-md border px-3 py-1.5 text-foreground hover:bg-accent", onclick: move |_| { send("tray:main"); }, {tr("nrzCode öffnen")} }
            }
        },
        Some(true) => match view() {
            View::Threads => rsx! { ThreadsView { onusage: move |_| view.set(View::Usage) } },
            View::Usage => rsx! { UsageView { onback: move |_| view.set(View::Threads) } },
        },
    };
    rsx! {
        div { class: "flex h-screen flex-col overflow-hidden rounded-xl border bg-background text-foreground select-none",
            {body}
        }
    }
}

/// Letzte Threads; unten „Nutzung“.
#[component]
fn ThreadsView(onusage: EventHandler<()>) -> Element {
    let all = THREADS.read();
    let mut list: Vec<ThreadSummary> = all.values().filter(|t| t.parent_id.is_none() && !t.hidden).cloned().collect();
    list.sort_by_key(|t| std::cmp::Reverse(t.last_activity_at));
    list.truncate(8);
    // Agenten je Thread (eigener und die seiner Subagenten)
    let agents: Vec<Vec<AgentKind>> = list
        .iter()
        .map(|t| {
            let mut v = vec![t.agent];
            for c in all.values().filter(|c| c.parent_id == Some(t.id)) {
                if !v.contains(&c.agent) {
                    v.push(c.agent);
                }
            }
            v
        })
        .collect();
    let working = all.values().filter(|t| t.status.is_active()).count();
    drop(all);
    rsx! {
        div { class: "flex h-11 shrink-0 items-center gap-2 border-b px-3",
            img { src: "/static/icon.png", class: "size-5 rounded", alt: "" }
            span { class: "text-sm font-semibold", "nrzCode" }
            if working > 0 {
                span { class: "inline-flex items-center gap-1 rounded-full bg-info/10 px-2 py-0.5 text-[11px] text-info",
                    icons::LoaderCircle { class: "size-3 animate-spin" }
                    {trf("{0} arbeiten", &[&working])}
                }
            }
            div { class: "ml-auto flex items-center gap-0.5",
                button {
                    class: "grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground",
                    title: tr("Neuer Thread"),
                    onclick: move |_| { send("tray:new"); },
                    icons::SquarePen { class: "size-4" }
                }
                button {
                    class: "grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground",
                    title: tr("nrzCode öffnen"),
                    onclick: move |_| { send("tray:main"); },
                    icons::AppWindow { class: "size-4" }
                }
            }
        }
        div { class: "min-h-0 flex-1 overflow-y-auto py-1",
            if list.is_empty() {
                p { class: "px-4 py-10 text-center text-sm text-muted-foreground", {tr("Noch keine Threads.")} }
            }
            for (t , kinds) in list.into_iter().zip(agents) {
                button {
                    key: "{t.id}",
                    class: "flex w-full flex-col gap-0.5 px-4 py-2.5 text-left hover:bg-accent/50",
                    onclick: move |_| { send(&format!("tray:open:{}", t.id)); },
                    div { class: "flex items-center gap-2",
                        ProjectBadge { name: t.project_name.clone(), size: 18 }
                        span { class: "min-w-0 flex-1 truncate text-[13px] text-muted-foreground", {t.project_name.clone().unwrap_or_else(|| tr("Ohne Projekt"))} }
                        ThreadState { thread: t.clone() }
                    }
                    div { class: "flex items-center gap-2",
                        span { class: "min-w-0 flex-1 truncate text-[15px] font-semibold", {thread_title(&t)} }
                    }
                    div { class: "flex items-center gap-2",
                        span { class: "min-w-0 flex-1 truncate text-xs text-muted-foreground", if t.branch.is_empty() { "–" } else { "{t.branch}" } }
                        div { class: "flex shrink-0 items-center gap-1",
                            for k in kinds {
                                crate::pages::agents::AgentIcon { kind: k, size: 15 }
                            }
                        }
                    }
                }
            }
        }
        button {
            class: "flex h-11 shrink-0 items-center gap-2 border-t px-4 text-sm text-muted-foreground hover:bg-accent/50 hover:text-foreground",
            onclick: move |_| onusage.call(()),
            icons::ChartColumn { class: "size-4" }
            span { class: "flex-1 text-left", {tr("Nutzung")} }
            icons::ChevronRight { class: "size-4" }
        }
    }
}

/// Nutzung: Reiter Limits | Kosten | Tokens, Zeitraum 24h–90d (dasselbe Board wie in der App).
#[component]
fn UsageView(onback: EventHandler<()>) -> Element {
    rsx! {
        div { class: "flex h-11 shrink-0 items-center gap-1 px-2",
            button { class: "grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground", title: tr("Zurück"), onclick: move |_| onback.call(()),
                icons::ChevronLeft { class: "size-4" }
            }
            span { class: "text-[15px] font-semibold", {tr("Nutzung")} }
        }
        crate::pages::agents::UsageBoard { compact: true }
    }
}
