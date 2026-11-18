//! Raster-Ansicht: ein Arbeitsbereich mit mehreren Chats nebeneinander (bis zu sechs, bei vier
//! 2×2). Jedes Fenster ist ein eigener Chat mit eigenem Composer und hat einen Vornamen (Lena,
//! Benni …), mit dem Dieter – die Sprachsteuerung unten in der Mitte – es anspricht.
//!
//! * Adresse `/grid/12,-1,7`: positive Zahlen sind Threads, negative leere Fenster (dort beginnt ein
//!   neuer Chat im gewählten Projekt). `0` aus älteren Adressen wird zu einem leeren Fenster.
//! * Fenster lassen sich am Kopf ziehen (sie tauschen die Plätze) und mit ⤢ gross stellen – die
//!   anderen stehen dann schmal daneben; ⤡ zeigt wieder alle gleich gross.
//! * Namen merkt sich der Browser (`nrz-pane-names`); Doppelklick auf den Namen benennt um.
//!   Die letzte Auswahl des Rasters merkt er sich auch (`nrz-grid`).

use super::dieter::DieterDock;
use super::parts::*;
use crate::Route;
use crate::components::common::Loading;
use crate::components::overlay::*;
use crate::i18n::{tr, trf};
use crate::state::{PROJECTS, THREADS};
use crate::util;
use dioxus::prelude::*;
use nrzcode_shared::ThreadSummary;
use std::collections::HashMap;

/// Höchstzahl der Fenster im Raster.
pub const MAX: usize = 6;
/// Schlüssel im Browser-Speicher für die zuletzt geöffnete Auswahl.
pub const GRID_KEY: &str = "nrz-grid";
const NAMES_KEY: &str = "nrz-pane-names";

/// Vornamen für Fenster: kurz, gut zu verstehen und verschieden klingend („Dieter“ ist vergeben).
pub const NAMES: [&str; 24] = [
    "Lena", "Benni", "Bernd", "Mia", "Paul", "Emma", "Finn", "Hanna", "Jonas", "Clara", "Felix", "Nina", "Theo", "Greta", "Otto", "Moritz", "Sophie",
    "Karl", "Ida", "Max", "Frieda", "Anton", "Luisa", "Emil",
];

/// Fenster-Schlüssel (Thread-ID oder negativ für ein leeres Fenster) → Vorname.
pub static PANE_NAMES: GlobalSignal<HashMap<i64, String>> = Signal::global(load_names);
/// Gross gezeigtes Fenster.
pub static FOCUS: GlobalSignal<Option<i64>> = Signal::global(|| None);
/// Fenster des offenen Rasters in Reihenfolge – für Änderungen aus laufenden Aufgaben (Dieter).
pub static PANES: GlobalSignal<Vec<i64>> = Signal::global(Vec::new);
/// Fenster, das gerade gezogen wird, und das darunter.
static DRAG: GlobalSignal<Option<i64>> = Signal::global(|| None);
static DROP_ON: GlobalSignal<Option<i64>> = Signal::global(|| None);

fn load_names() -> HashMap<i64, String> {
    util::storage_get(NAMES_KEY)
        .and_then(|s| serde_json::from_str::<HashMap<String, String>>(&s).ok())
        .map(|m| m.into_iter().filter_map(|(k, v)| Some((k.parse::<i64>().ok()?, v))).collect())
        .unwrap_or_default()
}

fn save_names(m: &HashMap<i64, String>) {
    let out: HashMap<String, String> = m.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    util::storage_set(NAMES_KEY, &serde_json::to_string(&out).unwrap_or_default());
}

/// Name eines Fensters (leer, solange noch keiner vergeben ist).
pub fn pane_name(key: i64) -> String {
    PANE_NAMES.read().get(&key).cloned().unwrap_or_default()
}

pub fn random_name(used: &[String]) -> String {
    let free: Vec<&str> = NAMES.iter().copied().filter(|n| !used.iter().any(|u| u.eq_ignore_ascii_case(n))).collect();
    if free.is_empty() {
        return format!("Kim {}", used.len() + 1);
    }
    let i = (js_sys::Math::random() * free.len() as f64) as usize;
    free[i.min(free.len() - 1)].to_string()
}

/// Jedem Fenster einen eigenen Namen geben (vorhandene bleiben, doppelte bekommen einen neuen).
fn ensure_names(list: &[i64]) {
    let mut names = PANE_NAMES.peek().clone();
    let mut used: Vec<String> = Vec::new();
    let mut changed = false;
    for k in list {
        let name = match names.get(k).filter(|n| !n.trim().is_empty() && !used.iter().any(|u| u.eq_ignore_ascii_case(n))) {
            Some(n) => n.clone(),
            None => {
                changed = true;
                random_name(&used)
            }
        };
        names.insert(*k, name.clone());
        used.push(name);
    }
    // Nicht endlos wachsen: alte Einträge fallen weg, die offenen bleiben
    if names.len() > 300 {
        names.retain(|k, _| list.contains(k));
        changed = true;
    }
    if changed {
        save_names(&names);
        *PANE_NAMES.write() = names;
    }
}

/// Fenster umbenennen; `false`, wenn der Name im Raster schon vergeben ist.
fn rename(key: i64, name: &str) -> bool {
    let name: String = name.trim().chars().filter(|c| !c.is_control() && *c != ',').take(20).collect();
    if name.is_empty() {
        return true;
    }
    let panes = PANES.peek().clone();
    let mut names = PANE_NAMES.peek().clone();
    let taken = name.eq_ignore_ascii_case("dieter") || panes.iter().any(|k| *k != key && names.get(k).is_some_and(|n| n.eq_ignore_ascii_case(&name)));
    if taken {
        return false;
    }
    names.insert(key, name);
    save_names(&names);
    *PANE_NAMES.write() = names;
    true
}

/// IDs aus der Adresse lesen (doppelte und ungültige fallen weg, `0` wird ein leeres Fenster).
fn parse(ids: &str) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    for id in ids.split(',').filter_map(|s| s.trim().parse::<i64>().ok()) {
        if out.len() >= MAX {
            break;
        }
        if id == 0 || !out.contains(&id) {
            out.push(id);
        }
    }
    for i in 0..out.len() {
        if out[i] == 0 {
            out[i] = fresh_empty(&out);
        }
    }
    out
}

/// Schlüssel für ein neues leeres Fenster.
pub fn fresh_empty(list: &[i64]) -> i64 {
    (1..).map(|n: i64| -n).find(|k| !list.contains(k)).unwrap_or(-1)
}

pub fn join(ids: &[i64]) -> String {
    crate::pages::agents::grid_ids(ids)
}

/// Raster mit dem aktuellen Stand ändern (auch aus laufenden Aufgaben wie Dieter).
pub fn update_panes(f: impl FnOnce(&mut Vec<i64>)) {
    let mut l = PANES.peek().clone();
    f(&mut l);
    l.truncate(MAX);
    *PANES.write() = l.clone();
    if FOCUS.peek().is_some_and(|k| !l.contains(&k)) {
        *FOCUS.write() = None;
    }
    let nav = navigator();
    if l.is_empty() {
        util::storage_set(GRID_KEY, "");
        nav.push(Route::Home {});
    } else {
        nav.replace(Route::Grid { ids: join(&l) });
    }
}

/// In einem leeren Fenster hat ein Chat begonnen: Name und Fokus gehen mit.
pub fn pane_created(key: i64, thread_id: i64) {
    let mut names = PANE_NAMES.peek().clone();
    if let Some(n) = names.remove(&key) {
        names.insert(thread_id, n);
        save_names(&names);
        *PANE_NAMES.write() = names;
    }
    if *FOCUS.peek() == Some(key) {
        *FOCUS.write() = Some(thread_id);
    }
    update_panes(|l| {
        if let Some(x) = l.iter_mut().find(|x| **x == key) {
            *x = thread_id;
        }
    });
}

/// Spalten je nach Anzahl: 2 → 2, 3 → 3, 4 → 2×2, 5–6 → 3×2.
fn grid_class(n: usize) -> &'static str {
    match n {
        0 | 1 => "grid-cols-1 grid-rows-1",
        2 => "grid-cols-1 grid-rows-2 md:grid-cols-2 md:grid-rows-1",
        3 => "grid-cols-1 grid-rows-3 lg:grid-cols-3 lg:grid-rows-1",
        4 => "grid-cols-1 grid-rows-4 md:grid-cols-2 md:grid-rows-2",
        _ => "grid-cols-1 grid-rows-6 md:grid-cols-2 md:grid-rows-3 xl:grid-cols-3 xl:grid-rows-2",
    }
}

#[component]
pub fn Grid(ids: String) -> Element {
    let nav = use_navigator();
    let list = parse(&ids);
    // Stand merken (Dieter, „Parallel-Ansicht“ in der Seitenleiste), Namen vergeben, ältere
    // Adressen (mit 0) in die feste Form bringen
    {
        let l = list.clone();
        let raw = ids.clone();
        use_effect(use_reactive!(|(l, raw)| {
            ensure_names(&l);
            *PANES.write() = l.clone();
            let canon = join(&l);
            util::storage_set(GRID_KEY, &canon);
            if canon != raw && !l.is_empty() {
                navigator().replace(Route::Grid { ids: canon });
            }
        }));
    }
    let threads = THREADS.read();
    // Fenster in Reihenfolge: (Schlüssel, Thread oder leer); gelöschte Threads fallen weg
    let shown: Vec<i64> = list.iter().copied().filter(|id| *id < 0 || threads.contains_key(id)).collect();
    let active = shown.iter().filter(|id| threads.get(id).is_some_and(|t| t.status.is_active())).count();
    let mut others: Vec<ThreadSummary> = threads.values().filter(|t| t.parent_id.is_none() && !list.contains(&t.id)).cloned().collect();
    let infos: HashMap<i64, ThreadSummary> = shown.iter().filter_map(|k| threads.get(k).map(|t| (*k, t.clone()))).collect();
    drop(threads);
    others.sort_by_key(|t| std::cmp::Reverse(t.last_activity_at));
    others.truncate(12);
    let n = shown.len();
    let full = list.len() >= MAX;
    let focus = FOCUS().filter(|k| n > 1 && shown.contains(k));

    let project = NEW_THREAD_PROJECT();
    let project_name = project.and_then(|id| PROJECTS.read().get(&id).map(|p| p.name.clone()));

    let remove = move |key: i64| update_panes(|l| l.retain(|x| *x != key));

    rsx! {
        div { class: "flex h-full min-h-0 flex-col gap-2 p-2",
            div { class: "flex shrink-0 flex-wrap items-center gap-2 px-1",
                icons::LayoutGrid { class: "size-4 text-muted-foreground" }
                span { class: "text-sm font-medium",
                    if n == 1 { {tr("1 Chat")} } else { {trf("{0} Chats", &[&n])} }
                }
                span { class: "inline-flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground",
                    ProjectBadge { name: project_name.clone(), size: 16 }
                    span { class: "truncate", {project_name.clone().unwrap_or_else(|| tr("Ohne Projekt"))} }
                }
                if active > 0 {
                    span { class: "inline-flex items-center gap-1 rounded-full bg-info/10 px-2 py-0.5 text-xs text-info",
                        icons::LoaderCircle { class: "size-3 animate-spin" }
                        {trf("{0} arbeiten", &[&active])}
                    }
                }
                if n > 1 {
                    span { class: "hidden text-xs text-muted-foreground lg:inline", {tr("Fenster am Kopf ziehen zum Umstellen · ⤢ zeigt eines gross")} }
                }
                div { class: "ml-auto flex items-center gap-2",
                    if focus.is_some() {
                        button {
                            class: "inline-flex h-8 items-center gap-1.5 rounded-md border bg-card px-2.5 text-sm hover:bg-accent",
                            onclick: move |_| *FOCUS.write() = None,
                            icons::Minimize2 { class: "size-3.5" }
                            {tr("Alle gleich gross")}
                        }
                    }
                    Link { to: Route::Home {}, class: "inline-flex h-8 items-center gap-1.5 rounded-md border bg-card px-2.5 text-sm hover:bg-accent",
                        icons::Plus { class: "size-3.5" }
                        {tr("Neuer Arbeitsbereich")}
                    }
                    if !full {
                        Dropdown {
                            align: Align::End,
                            class: "max-h-96 w-80 overflow-y-auto",
                            trigger_class: "inline-flex h-8 items-center gap-1.5 rounded-md border bg-card px-2.5 text-sm hover:bg-accent",
                            trigger: rsx! {
                                icons::Columns3 { class: "size-3.5" }
                                {tr("Thread hinzufügen")}
                                icons::ChevronDown { class: "size-3.5 text-muted-foreground" }
                            },
                            MenuItem {
                                onclick: {
                                    let list = list.clone();
                                    move |_| {
                                        let mut l = list.clone();
                                        l.push(fresh_empty(&l));
                                        nav.replace(Route::Grid { ids: join(&l) });
                                    }
                                },
                                icons::Plus {}
                                span { class: "flex-1", {tr("Leerer Chat")} }
                            }
                            if !others.is_empty() {
                                MenuSeparator {}
                                MenuLabel { {tr("In das Raster holen")} }
                            }
                            for t in others {
                                MenuItem {
                                    key: "{t.id}",
                                    onclick: {
                                        let list = list.clone();
                                        move |_| {
                                            let mut l = list.clone();
                                            l.push(t.id);
                                            nav.replace(Route::Grid { ids: join(&l) });
                                        }
                                    },
                                    crate::pages::agents::AgentIcon { kind: t.agent, size: 14 }
                                    span { class: "min-w-0 flex-1 truncate", {thread_title(&t)} }
                                    ThreadState { thread: t.clone() }
                                }
                            }
                        }
                    }
                }
            }
            if n == 0 {
                div { class: "flex flex-1 items-center justify-center text-sm text-muted-foreground",
                    if list.is_empty() { {tr("Keine Threads im Raster.")} } else { Loading {} }
                }
            } else {
                div { class: if focus.is_some() { "flex min-h-0 flex-1 gap-2" } else { "grid min-h-0 flex-1 gap-2 {grid_class(n)}" },
                    for key in shown.iter().copied() {
                        if key < 0 {
                            EmptyPane {
                                key: "{key}",
                                pane: key,
                                project_id: project,
                                focused: focus == Some(key),
                                hidden: focus.is_some_and(|f| f != key),
                                can_focus: n > 1,
                                onclose: remove,
                            }
                        } else {
                            Pane {
                                key: "{key}",
                                id: key,
                                focused: focus == Some(key),
                                hidden: focus.is_some_and(|f| f != key),
                                can_focus: n > 1,
                                onclose: remove,
                            }
                        }
                    }
                    if let Some(f) = focus {
                        FocusRail { panes: shown.iter().copied().filter(|k| *k != f).collect::<Vec<_>>(), infos: infos.clone() }
                    }
                }
            }
            DieterDock { project_id: project, can_add: !full }
        }
    }
}

/// Neben dem gross gezeigten Fenster: die anderen als schmale Karten (Klick zeigt sie gross).
#[component]
fn FocusRail(panes: Vec<i64>, infos: HashMap<i64, ThreadSummary>) -> Element {
    rsx! {
        div { class: "hidden w-56 shrink-0 flex-col gap-1.5 overflow-y-auto md:flex",
            button {
                class: "flex h-8 shrink-0 items-center justify-center gap-1.5 rounded-lg border border-dashed text-xs text-muted-foreground hover:bg-accent hover:text-foreground",
                onclick: move |_| *FOCUS.write() = None,
                icons::Minimize2 { class: "size-3.5" }
                {tr("Alle gleich gross")}
            }
            for key in panes {
                button {
                    key: "{key}",
                    class: "flex shrink-0 flex-col gap-1 rounded-xl border bg-card/60 p-2.5 text-left hover:border-primary/40 hover:bg-accent/50",
                    title: tr("Gross zeigen"),
                    onclick: move |_| *FOCUS.write() = Some(key),
                    div { class: "flex items-center gap-1.5",
                        NameBadge { pane: key }
                        if let Some(t) = infos.get(&key) {
                            crate::pages::agents::AgentIcon { kind: t.agent, size: 13 }
                            span { class: "ml-auto", ThreadState { thread: t.clone() } }
                        }
                    }
                    match infos.get(&key) {
                        Some(t) => rsx! { span { class: "line-clamp-2 text-xs text-muted-foreground", {thread_title(t)} } },
                        None => rsx! { span { class: "text-xs text-muted-foreground", {tr("Neuer Chat")} } },
                    }
                }
            }
        }
    }
}

/// Farbiger Vorname eines Fensters; Doppelklick benennt um.
#[component]
fn NameBadge(pane: i64, #[props(default)] editable: bool) -> Element {
    let mut editing = use_signal(|| false);
    let mut draft = use_signal(String::new);
    let name = pane_name(pane);
    let h = hue(&name);
    if editing() {
        return rsx! {
            input {
                class: "h-6 w-24 rounded border bg-background px-1.5 text-xs font-semibold outline-none focus:border-ring",
                value: "{draft}",
                maxlength: "20",
                autofocus: true,
                onmounted: move |e| {
                    spawn(async move {
                        let _ = e.set_focus(true).await;
                    });
                },
                oninput: move |e| draft.set(e.value()),
                onkeydown: move |e: KeyboardEvent| {
                    e.stop_propagation();
                    match e.key() {
                        Key::Enter => {
                            if rename(pane, &draft()) {
                                editing.set(false);
                            } else {
                                toast_error(tr("Diesen Namen hat schon ein anderes Fenster (oder Dieter)."));
                            }
                        }
                        Key::Escape => editing.set(false),
                        _ => {}
                    }
                },
                onblur: move |_| {
                    if !rename(pane, &draft()) {
                        toast_error(tr("Diesen Namen hat schon ein anderes Fenster (oder Dieter)."));
                    }
                    editing.set(false);
                },
            }
        };
    }
    rsx! {
        span {
            class: "inline-flex h-5 shrink-0 items-center rounded-md px-1.5 text-[11px] font-semibold tracking-tight",
            style: "background: hsl({h} 65% 50% / 0.16); color: hsl({h} 75% 62%)",
            title: if editable { tr("Name für Dieter – Doppelklick zum Umbenennen") } else { tr("Name für Dieter") },
            ondoubleclick: move |e| {
                if editable {
                    e.stop_propagation();
                    draft.set(pane_name(pane));
                    editing.set(true);
                }
            },
            if name.is_empty() { "…" } else { "{name}" }
        }
    }
}

/// Klassen eines Fensters (Rahmen, Ziehen, versteckt im Fokus eines anderen).
fn pane_class(focused: bool, hidden: bool, key: i64) -> String {
    let mut c = String::from("flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-xl border bg-background transition-shadow focus-within:border-primary/60 focus-within:ring-1 focus-within:ring-primary/30");
    if hidden {
        c.push_str(" hidden");
    }
    if focused {
        c.push_str(" border-primary/40");
    }
    if *DRAG.read() == Some(key) {
        c.push_str(" opacity-50");
    } else if *DROP_ON.read() == Some(key) && DRAG.read().is_some() {
        c.push_str(" ring-2 ring-primary");
    }
    c
}

/// Ziehen: Fenster `from` auf das Fenster `to` fallen lassen → beide tauschen die Plätze.
fn swap(from: i64, to: i64) {
    update_panes(|l| {
        if let (Some(i), Some(j)) = (l.iter().position(|x| *x == from), l.iter().position(|x| *x == to)) {
            l.swap(i, j);
        }
    });
}

/// Ereignisse zum Ziehen und Ablegen für ein Fenster (Kopf zieht, ganzes Fenster nimmt an).
fn on_drag_over(key: i64, e: &DragEvent) {
    if DRAG.peek().is_some_and(|d| d != key) {
        e.prevent_default();
        e.data_transfer().set_drop_effect("move");
        if *DROP_ON.peek() != Some(key) {
            *DROP_ON.write() = Some(key);
        }
    }
}

fn on_drop(key: i64, e: &DragEvent) {
    let from = *DRAG.peek();
    if let Some(from) = from {
        e.prevent_default();
        if from != key {
            swap(from, key);
        }
    }
    *DRAG.write() = None;
    *DROP_ON.write() = None;
}

fn on_drag_start(key: i64, e: &DragEvent) {
    let dt = e.data_transfer();
    // Eigenes Format: Textfelder übernehmen es beim Ablegen nicht als Text
    let _ = dt.set_data("application/x-nrz-pane", &key.to_string());
    dt.set_effect_allowed("move");
    *DRAG.write() = Some(key);
}

fn on_drag_end() {
    *DRAG.write() = None;
    *DROP_ON.write() = None;
}

/// Knöpfe rechts im Kopf: gross zeigen / wieder gleich gross.
#[component]
fn FocusButton(pane: i64, focused: bool) -> Element {
    rsx! {
        button {
            class: if focused { "grid size-6 place-items-center rounded bg-accent text-foreground hover:bg-accent/70" } else { "grid size-6 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground" },
            title: if focused { tr("Wieder alle gleich gross") } else { tr("Gross zeigen (die anderen bleiben daneben)") },
            onclick: move |_| *FOCUS.write() = if focused { None } else { Some(pane) },
            if focused {
                icons::Minimize2 { class: "size-3.5" }
            } else {
                icons::Maximize2 { class: "size-3.5" }
            }
        }
    }
}

/// Ein Fenster im Raster: Kopf (Name, Zustand, Agent, Titel, Gross, Seite, Schliessen) und
/// darunter die Unterhaltung mit Composer.
#[component]
fn Pane(id: i64, focused: bool, hidden: bool, can_focus: bool, onclose: EventHandler<i64>) -> Element {
    let Some(t) = THREADS.read().get(&id).cloned() else {
        return rsx! {};
    };
    let dot = if t.status.is_active() { "size-2 shrink-0 rounded-full bg-success animate-pulse" } else { "size-2 shrink-0 rounded-full bg-muted-foreground/40" };
    rsx! {
        section {
            class: pane_class(focused, hidden, id),
            ondragover: move |e| on_drag_over(id, &e),
            ondrop: move |e| on_drop(id, &e),
            header {
                class: "flex h-9 shrink-0 cursor-grab items-center gap-2 border-b bg-card/60 px-2 active:cursor-grabbing",
                draggable: "true",
                title: tr("Ziehen, um das Fenster umzustellen"),
                ondragstart: move |e| on_drag_start(id, &e),
                ondragend: move |_| on_drag_end(),
                ondoubleclick: move |_| {
                    if can_focus {
                        *FOCUS.write() = if focused { None } else { Some(id) };
                    }
                },
                icons::GripVertical { class: "size-3.5 shrink-0 text-muted-foreground/60" }
                span { class: dot }
                NameBadge { pane: id, editable: true }
                crate::pages::agents::AgentIcon { kind: t.agent, size: 14 }
                span { class: "min-w-0 flex-1 truncate text-sm font-medium", title: thread_title(&t), {thread_title(&t)} }
                crate::components::common::ChangeStat { thread: t.clone() }
                ThreadState { thread: t.clone() }
                if can_focus {
                    FocusButton { pane: id, focused }
                }
                Link {
                    to: Route::ThreadPage { id },
                    class: "grid size-6 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground",
                    title: tr("Als ganze Seite öffnen"),
                    icons::SquareArrowOutUpRight { class: "size-3.5" }
                }
                button {
                    class: "grid size-6 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground",
                    title: tr("Aus dem Raster nehmen (der Thread bleibt erhalten)"),
                    onclick: move |_| onclose.call(id),
                    icons::X { class: "size-3.5" }
                }
            }
            div { class: "min-h-0 flex-1 text-[13px]", crate::pages::agents::ChatView { thread_id: id } }
        }
    }
}

/// Leeres Fenster: ein neuer Chat beginnt hier, ohne die Seite zu verlassen.
#[component]
fn EmptyPane(pane: i64, project_id: Option<i64>, focused: bool, hidden: bool, can_focus: bool, onclose: EventHandler<i64>) -> Element {
    rsx! {
        section {
            class: pane_class(focused, hidden, pane),
            ondragover: move |e| on_drag_over(pane, &e),
            ondrop: move |e| on_drop(pane, &e),
            header {
                class: "flex h-9 shrink-0 cursor-grab items-center gap-2 border-b bg-card/60 px-2 active:cursor-grabbing",
                draggable: "true",
                title: tr("Ziehen, um das Fenster umzustellen"),
                ondragstart: move |e| on_drag_start(pane, &e),
                ondragend: move |_| on_drag_end(),
                icons::GripVertical { class: "size-3.5 shrink-0 text-muted-foreground/60" }
                span { class: "size-2 shrink-0 rounded-full bg-muted-foreground/40" }
                NameBadge { pane, editable: true }
                span { class: "min-w-0 flex-1 truncate text-sm text-muted-foreground", {tr("Neuer Chat")} }
                if can_focus {
                    FocusButton { pane, focused }
                }
                button {
                    class: "grid size-6 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground",
                    title: tr("Fenster schliessen"),
                    onclick: move |_| onclose.call(pane),
                    icons::X { class: "size-3.5" }
                }
            }
            div { class: "flex min-h-0 flex-1 flex-col justify-end overflow-y-auto p-3 text-[13px]",
                crate::pages::agents::Composer { project_id, thread_id: None, slot: pane.unsigned_abs() as usize, on_created: move |id: i64| pane_created(pane, id) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keys_and_legacy_zero() {
        assert_eq!(parse("12,-1,12,7"), vec![12, -1, 7]);
        assert_eq!(parse("0,0,5"), vec![-1, -2, 5]);
        assert_eq!(parse("-2,0"), vec![-2, -1]);
        assert_eq!(fresh_empty(&[-1, -3]), -2);
    }
}
