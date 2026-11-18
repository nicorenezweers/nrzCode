//! „Windows“ – die Fenster auf der Fläche: Titelleiste (Zustand, Name für Dieter, Titel, zum
//! Fenster fliegen, im Chat-Modus öffnen, schliessen), ziehen am Kopf, Grösse an der Ecke.
//! Inhalt je Art: Chat (dieselbe Unterhaltung wie im Chat-Modus), neuer Chat (Composer),
//! Terminal (Shell im Projektordner), Browser (Vorschau lokaler Server über den Proxy oder eine
//! beliebige Adresse).

use super::store::*;
use crate::api;
use crate::components::overlay::toast_error;
use crate::i18n::{tr, trf};
use crate::pages::workspace::parts::{open_tab, thread_title, ThreadState};
use crate::state::{self, THREADS};
use dioxus::prelude::*;
use nrzcode_shared::agents::TerminalInfo;
use nrzcode_shared::canvas::CanvasWindow;
use nrzcode_shared::workspace::{PreviewPort, PreviewToken, PreviewTokenRequest};
use nrzcode_shared::ThreadStatus;

/// Ab diesem Zoom zeigen Chats statt der ganzen Unterhaltung eine Karte (schneller, lesbar).
pub const CARD_BELOW: f64 = 0.42;

#[component]
pub fn WindowView(win: CanvasWindow, compact: bool, selected: bool, project_id: Option<i64>, #[props(default)] animate: bool) -> Element {
    let id = win.id.clone();
    let thread = win.thread_id.and_then(|t| THREADS.read().get(&t).cloned());
    let status = thread.as_ref().map(|t| t.status);
    let dot = match status {
        Some(s) if s.is_active() => "size-2 shrink-0 rounded-full bg-success animate-pulse",
        Some(ThreadStatus::Error) => "size-2 shrink-0 rounded-full bg-destructive",
        Some(ThreadStatus::Done) => "size-2 shrink-0 rounded-full bg-info",
        _ => "size-2 shrink-0 rounded-full bg-muted-foreground/40",
    };
    let title = match win.kind.as_str() {
        "chat" => thread.as_ref().map(thread_title).unwrap_or_else(|| tr("Chat")),
        "new" => tr("Neuer Chat"),
        "terminal" => tr("Terminal"),
        _ => {
            if win.url.is_empty() {
                tr("Browser")
            } else {
                win.url.trim_start_matches("https://").trim_start_matches("http://").to_string()
            }
        }
    };
    let ring = if selected { "ring-2 ring-primary/60 border-primary/40" } else { "" };
    // Weich an den neuen Platz nur beim Anordnen und für Dieter ([`GLIDE`]). Beim Ziehen steht die
    // Lage direkt im DOM (board.rs); beim Loslassen ersetzt dieser Stil sie ohne Übergang.
    let glide = if animate { "transition: left 320ms cubic-bezier(.2,.8,.2,1), top 320ms cubic-bezier(.2,.8,.2,1), width 320ms, height 320ms;" } else { "" };
    let style = format!("left:{}px; top:{}px; width:{}px; height:{}px; z-index:{}; transform: none; {glide}", win.x, win.y, win.w, win.h, win.z.clamp(0, 100_000));
    let (id_down, id_head, id_res, id_fly, id_close, id_dbl) = (id.clone(), id.clone(), id.clone(), id.clone(), id.clone(), id.clone());
    let (wx, wy, ww, wh) = (win.x, win.y, win.w, win.h);
    rsx! {
        section {
            class: "nrz-pane absolute flex flex-col overflow-hidden rounded-xl border bg-background shadow-[0_18px_50px_-12px_rgb(0_0_0/0.45)] {ring}",
            style: "{style}",
            "data-cw": "1",
            "data-cw-id": "{win.id}",
            onpointerdown: move |e| {
                e.stop_propagation();
                raise(&id_down);
            },
            header {
                class: "flex h-9 shrink-0 cursor-grab items-center gap-2 border-b bg-card/80 px-2 active:cursor-grabbing",
                title: tr("Am Kopf ziehen zum Verschieben · Doppelklick: hinfliegen"),
                onpointerdown: move |e: PointerEvent| {
                    e.stop_propagation();
                    if e.trigger_button() != Some(dioxus::html::input_data::MouseButton::Primary) {
                        return;
                    }
                    raise(&id_head);
                    let p = e.client_coordinates();
                    super::board::capture_pointer(&e);
                    *DRAG.write() = Some(Drag::Window { id: id_head.clone(), sx: p.x, sy: p.y, x: wx, y: wy, w: ww, h: wh });
                },
                ondoubleclick: move |_| fly_to(&id_dbl),
                span { class: dot }
                CanvasName { id: id.clone(), name: win.name.clone() }
                match win.kind.as_str() {
                    "chat" => rsx! {
                        if let Some(t) = thread.clone() {
                            crate::pages::agents::AgentIcon { kind: t.agent, size: 14 }
                        }
                    },
                    "terminal" => rsx! { icons::SquareTerminal { class: "size-3.5 shrink-0 text-muted-foreground" } },
                    "browser" => rsx! { icons::Globe { class: "size-3.5 shrink-0 text-muted-foreground" } },
                    _ => rsx! { icons::MessageSquarePlus { class: "size-3.5 shrink-0 text-muted-foreground" } },
                }
                span { class: "min-w-0 flex-1 truncate text-sm font-medium", title: "{title}", "{title}" }
                if let Some(t) = thread.clone() {
                    crate::components::common::ChangeStat { thread: t.clone() }
                    ThreadState { thread: t.clone() }
                }
                button {
                    class: "grid size-6 shrink-0 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground",
                    title: tr("Hinfliegen und gross zeigen"),
                    onpointerdown: move |e| e.stop_propagation(),
                    onclick: move |_| fly_to(&id_fly),
                    icons::Focus { class: "size-3.5" }
                }
                if let Some(tid) = win.thread_id {
                    button {
                        class: "grid size-6 shrink-0 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground",
                        title: tr("Im Chat-Modus öffnen"),
                        onpointerdown: move |e| e.stop_propagation(),
                        onclick: move |_| {
                            state::set_mode(state::UiMode::Chat);
                            navigator().push(crate::Route::ThreadPage { id: tid });
                        },
                        icons::SquareArrowOutUpRight { class: "size-3.5" }
                    }
                }
                button {
                    class: "grid size-6 shrink-0 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground",
                    title: if win.thread_id.is_some() { tr("Fenster schliessen (der Chat bleibt erhalten)") } else { tr("Fenster schliessen") },
                    onpointerdown: move |e| e.stop_propagation(),
                    onclick: move |_| close_window(&id_close),
                    icons::X { class: "size-3.5" }
                }
            }
            div { class: "relative min-h-0 flex-1",
                match win.kind.as_str() {
                    "chat" => match win.thread_id {
                        Some(tid) if !compact => rsx! { div { class: "h-full text-[13px]", crate::pages::agents::ChatView { thread_id: tid } } },
                        Some(tid) => rsx! { ChatCard { thread_id: tid } },
                        None => rsx! {},
                    },
                    "new" => rsx! { NewChat { win: id.clone(), project_id } },
                    "terminal" => rsx! { TerminalBody { win: id.clone(), terminal_id: win.terminal_id.clone(), project_id } },
                    _ => rsx! { BrowserBody { win: id.clone(), url: win.url.clone(), project_id } },
                }
            }
            // Grösse ändern (rechte untere Ecke)
            div {
                class: "absolute right-0 bottom-0 z-10 size-4 cursor-nwse-resize",
                title: tr("Grösse ändern"),
                onpointerdown: move |e: PointerEvent| {
                    e.stop_propagation();
                    raise(&id_res);
                    let p = e.client_coordinates();
                    super::board::capture_pointer(&e);
                    *DRAG.write() = Some(Drag::Resize { id: id_res.clone(), sx: p.x, sy: p.y, x: wx, y: wy, w: ww, h: wh });
                },
                svg { class: "absolute right-1 bottom-1 size-2.5 text-muted-foreground/60", view_box: "0 0 10 10",
                    path { d: "M9 1 L1 9 M9 5 L5 9", stroke: "currentColor", stroke_width: "1.4", fill: "none" }
                }
            }
        }
    }
}

/// Vorname des Fensters; Doppelklick benennt um (eindeutig auf der Fläche, nicht „Dieter“).
#[component]
fn CanvasName(id: String, name: String) -> Element {
    let mut editing = use_signal(|| false);
    let mut draft = use_signal(String::new);
    let h = crate::pages::workspace::parts::hue(&name);
    let save = {
        let id = id.clone();
        move |v: String| {
            let v: String = v.trim().chars().filter(|c| !c.is_control() && *c != ',').take(20).collect();
            if v.is_empty() {
                return true;
            }
            let taken = v.eq_ignore_ascii_case("dieter")
                || BOARD.peek().as_ref().is_some_and(|b| b.windows.iter().any(|w| w.id != id && w.name.eq_ignore_ascii_case(&v)));
            if taken {
                toast_error(tr("Diesen Namen hat schon ein anderes Fenster (oder Dieter)."));
                return false;
            }
            update_window(&id, |w| w.name = v);
            schedule_save();
            true
        }
    };
    if editing() {
        let save2 = save.clone();
        let save3 = save.clone();
        return rsx! {
            input {
                class: "h-6 w-24 rounded border bg-background px-1.5 text-xs font-semibold outline-none focus:border-ring",
                value: "{draft}",
                maxlength: "20",
                onmounted: move |e| async move {
                    let _ = e.set_focus(true).await;
                },
                onpointerdown: move |e| e.stop_propagation(),
                oninput: move |e| draft.set(e.value()),
                onkeydown: move |e: KeyboardEvent| {
                    e.stop_propagation();
                    match e.key() {
                        Key::Enter => {
                            if save2(draft()) {
                                editing.set(false);
                            }
                        }
                        Key::Escape => editing.set(false),
                        _ => {}
                    }
                },
                onblur: move |_| {
                    save3(draft());
                    editing.set(false);
                },
            }
        };
    }
    rsx! {
        span {
            class: "inline-flex h-5 shrink-0 cursor-text items-center rounded-md px-1.5 text-[11px] font-semibold tracking-tight",
            style: "background: hsl({h} 65% 50% / 0.16); color: hsl({h} 75% 62%)",
            title: tr("Name für Dieter – Doppelklick zum Umbenennen"),
            onpointerdown: move |e| e.stop_propagation(),
            ondoubleclick: move |e| {
                e.stop_propagation();
                draft.set(name.clone());
                editing.set(true);
            },
            if name.is_empty() { "…" } else { "{name}" }
        }
    }
}

/// Kleine Karte statt der Unterhaltung (weit herausgezoomt).
#[component]
fn ChatCard(thread_id: i64) -> Element {
    let t = THREADS.read().get(&thread_id).cloned();
    let last = crate::state::MESSAGES
        .read()
        .get(&thread_id)
        .and_then(|l| l.iter().rev().find(|m| m.role == "assistant" && !m.content.trim().is_empty()).map(|m| m.content.clone()))
        .unwrap_or_default();
    let Some(t) = t else { return rsx! {} };
    let snippet: String = last.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(420).collect();
    rsx! {
        div { class: "flex h-full flex-col gap-3 p-5",
            p { class: "line-clamp-3 text-2xl font-semibold leading-tight", {thread_title(&t)} }
            div { class: "flex items-center gap-2 text-lg text-muted-foreground",
                crate::pages::agents::AgentIcon { kind: t.agent, size: 22 }
                ThreadState { thread: t.clone() }
            }
            if !snippet.is_empty() {
                p { class: "line-clamp-[12] text-lg leading-snug text-muted-foreground", "{snippet}" }
            }
        }
    }
}

/// Neuer Chat im Fenster: Composer; danach wird das Fenster zum Chat.
#[component]
fn NewChat(win: String, project_id: Option<i64>) -> Element {
    // Eindeutige Nummer für den Entwurf dieses Fensters
    let slot = 1000 + win.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)) % 100_000;
    rsx! {
        div { class: "flex h-full flex-col justify-end overflow-y-auto p-3 text-[13px]",
            div { class: "mb-auto flex flex-col items-center gap-2 pt-6 text-center text-muted-foreground",
                icons::MessageSquarePlus { class: "size-6" }
                p { class: "text-sm", {tr("Neuer Chat in diesem Arbeitsbereich")} }
            }
            crate::pages::agents::Composer {
                project_id,
                thread_id: None,
                slot,
                on_created: move |id: i64| window_created(&win, id),
            }
        }
    }
}

/// Terminal im Projektordner (wird beim ersten Zeigen gestartet; nach einem Neustart des Servers
/// neu startbar).
#[component]
fn TerminalBody(win: String, terminal_id: String, project_id: Option<i64>) -> Element {
    let mut state = use_signal(|| if terminal_id.is_empty() { "start" } else { "check" });
    let mut busy = use_signal(|| false);
    let start = {
        let win = win.clone();
        move || {
            if busy() {
                return;
            }
            busy.set(true);
            let win = win.clone();
            spawn(async move {
                let body = serde_json::json!({ "projectId": project_id, "cols": 100, "rows": 28 });
                match api::post::<TerminalInfo, _>("/terminals", &body).await {
                    Ok(t) => {
                        update_window(&win, |w| w.terminal_id = t.id.clone());
                        schedule_save();
                        state.set("ok");
                    }
                    Err(e) => {
                        toast_error(e);
                        state.set("gone");
                    }
                }
                busy.set(false);
            });
        }
    };
    // Beim Zeigen: vorhandenes Terminal prüfen bzw. ein neues starten
    {
        let tid = terminal_id.clone();
        let mut start = start.clone();
        use_hook(move || {
            if tid.is_empty() {
                start();
            } else {
                spawn(async move {
                    match api::get::<TerminalInfo>(&format!("/terminals/{tid}")).await {
                        Ok(_) => state.set("ok"),
                        Err(_) => state.set("gone"),
                    }
                });
            }
        });
    }
    let mut start_btn = start.clone();
    match state() {
        "ok" if !terminal_id.is_empty() => rsx! {
            div { class: "absolute inset-0 bg-[#141414]",
                crate::pages::agents::TerminalView { key: "{terminal_id}", id: terminal_id.clone() }
            }
        },
        "gone" => rsx! {
            div { class: "flex h-full flex-col items-center justify-center gap-3 p-6 text-center text-sm text-muted-foreground",
                icons::SquareTerminal { class: "size-6" }
                p { {tr("Das Terminal ist beendet.")} }
                button {
                    class: "inline-flex h-8 items-center gap-1.5 rounded-md border bg-card px-3 text-sm text-foreground hover:bg-accent",
                    onclick: move |_| start_btn(),
                    icons::RotateCw { class: "size-3.5" }
                    {tr("Neu starten")}
                }
            }
        },
        _ => rsx! {
            div { class: "grid h-full place-items-center", crate::components::common::Loading {} }
        },
    }
}

/// Adresse auflösen: lokale Server (Port, localhost, 127.0.0.1) über den Vorschau-Proxy,
/// sonst direkt (ohne Schema mit https://).
async fn resolve(url: &str) -> Result<String, String> {
    let u = url.trim();
    if u.is_empty() {
        return Ok(String::new());
    }
    if let Some((port, path)) = local_port(u) {
        let body = PreviewTokenRequest { thread_id: 0, port, path };
        return api::post::<PreviewToken, _>("/previews/token", &body).await.map(|t| t.url).map_err(|e| e.message);
    }
    if u.contains("://") {
        Ok(u.to_string())
    } else {
        Ok(format!("https://{u}"))
    }
}

/// `3000`, `:3000/x`, `localhost:3000/x`, `http://127.0.0.1:3000` → (Port, Pfad).
pub fn local_port(u: &str) -> Option<(u16, String)> {
    let rest = u.trim().trim_start_matches("http://").trim_start_matches("https://");
    let rest = rest.strip_prefix("localhost").or_else(|| rest.strip_prefix("127.0.0.1")).or_else(|| rest.strip_prefix("0.0.0.0")).unwrap_or(rest);
    let rest = rest.strip_prefix(':').unwrap_or(rest);
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 5 {
        return None;
    }
    let after = &rest[digits.len()..];
    if !(after.is_empty() || after.starts_with('/') || after.starts_with('?')) {
        return None;
    }
    let port: u16 = digits.parse().ok().filter(|p| *p > 0)?;
    let path = if after.is_empty() { "/".to_string() } else if after.starts_with('/') { after.to_string() } else { format!("/{after}") };
    Some((port, path))
}

/// Browser im Fenster: Adresszeile, Seite, lokale Server zum schnellen Öffnen.
#[component]
fn BrowserBody(win: String, url: String, project_id: Option<i64>) -> Element {
    let mut input = use_signal(|| url.clone());
    let mut src = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut version = use_signal(|| 0u64);
    // Adresse (auch von Dieter) → Seite laden
    use_effect(use_reactive!(|url| {
        input.set(url.clone());
        let _ = version();
        spawn(async move {
            match resolve(&url).await {
                Ok(s) => {
                    error.set(None);
                    src.set(s);
                }
                Err(e) => error.set(Some(e)),
            }
        });
    }));
    let ports = use_resource(move || async move {
        let _ = version();
        let q = project_id.map(|p| format!("?projectId={p}")).unwrap_or_default();
        api::get::<Vec<PreviewPort>>(&format!("/previews{q}")).await.unwrap_or_default()
    });
    let go = {
        let win = win.clone();
        move |u: String| {
            let u = u.trim().to_string();
            update_window(&win, |w| w.url = u.clone());
            schedule_save();
            version += 1;
        }
    };
    let mut go2 = go.clone();
    let list = ports.read().clone().unwrap_or_default();
    rsx! {
        div { class: "flex h-full flex-col",
            div { class: "flex h-9 shrink-0 items-center gap-1 border-b px-1.5",
                button {
                    class: "grid size-7 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground",
                    title: tr("Neu laden"),
                    onclick: move |_| version += 1,
                    icons::RotateCw { class: "size-3.5" }
                }
                input {
                    class: "h-7 min-w-0 flex-1 rounded-md border bg-transparent px-2 font-mono text-xs outline-none focus:border-ring",
                    placeholder: tr("Adresse oder Port, z. B. 3000 oder nrzgit.de"),
                    value: "{input}",
                    onpointerdown: move |e| e.stop_propagation(),
                    oninput: move |e| input.set(e.value()),
                    onkeydown: move |e: KeyboardEvent| {
                        e.stop_propagation();
                        if e.key() == Key::Enter {
                            go2(input());
                        }
                    },
                }
                if !list.is_empty() {
                    crate::components::overlay::Dropdown {
                        align: crate::components::overlay::Align::End,
                        class: "w-56",
                        trigger_class: "inline-flex h-7 items-center gap-1 rounded-md px-1.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground",
                        trigger: rsx! {
                            icons::AppWindow { class: "size-3.5" }
                            "{list.len()}"
                        },
                        crate::components::overlay::MenuLabel { {tr("Lokale Server")} }
                        for p in list.clone() {
                            crate::components::overlay::MenuItem {
                                key: "{p.port}",
                                onclick: {
                                    let mut go = go.clone();
                                    move |_| go(format!("http://localhost:{}", p.port))
                                },
                                span { class: if p.this_thread { "size-2 rounded-full bg-success" } else { "size-2 rounded-full bg-muted-foreground/50" } }
                                span { class: "font-mono", ":{p.port}" }
                                span { class: "min-w-0 flex-1 truncate text-xs text-muted-foreground", "{p.process}" }
                            }
                        }
                    }
                }
                button {
                    class: "grid size-7 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground disabled:opacity-30",
                    title: tr("Im Browser öffnen"),
                    disabled: src().is_empty(),
                    onclick: move |_| {
                        let u = input();
                        spawn(async move {
                            match resolve(&u).await {
                                Ok(s) if !s.is_empty() => open_tab(&s),
                                Ok(_) => {}
                                Err(e) => toast_error(e),
                            }
                        });
                    },
                    icons::ExternalLink { class: "size-3.5" }
                }
            }
            if let Some(e) = error() {
                div { class: "px-3 py-2 text-xs text-destructive", "{e}" }
            }
            if src().is_empty() {
                div { class: "flex min-h-0 flex-1 flex-col items-center justify-center gap-3 p-6 text-center",
                    icons::Globe { class: "size-7 text-muted-foreground" }
                    p { class: "text-sm text-muted-foreground", {tr("Adresse eingeben – lokale Server der Agenten öffnen sich über die Vorschau.")} }
                    if !list.is_empty() {
                        div { class: "flex flex-wrap justify-center gap-1.5",
                            for p in list {
                                button {
                                    key: "{p.port}",
                                    class: "inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 font-mono text-xs hover:bg-accent",
                                    onclick: {
                                        let mut go = go.clone();
                                        move |_| go(format!("http://localhost:{}", p.port))
                                    },
                                    ":{p.port}"
                                    if !p.process.is_empty() {
                                        span { class: "font-sans text-muted-foreground", "{p.process}" }
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                iframe { key: "{src}", class: "min-h-0 w-full flex-1 bg-white", src: "{src}", title: tr("Browser") }
                p { class: "shrink-0 border-t px-2 py-1 text-[11px] text-muted-foreground",
                    {trf("Manche Seiten verbieten die Anzeige in einem Fenster – dann {0} benutzen.", &[&"↗"])}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::local_port;

    #[test]
    fn local_ports() {
        assert_eq!(local_port("3000"), Some((3000, "/".into())));
        assert_eq!(local_port(":5173/app"), Some((5173, "/app".into())));
        assert_eq!(local_port("http://localhost:8080/x?y=1"), Some((8080, "/x?y=1".into())));
        assert_eq!(local_port("127.0.0.1:3000"), Some((3000, "/".into())));
        assert_eq!(local_port("nrzgit.de"), None);
        assert_eq!(local_port("https://example.com:8443"), None);
        assert_eq!(local_port("2024abc"), None);
    }
}
