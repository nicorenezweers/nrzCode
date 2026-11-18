//! Rahmen des Canvas-Modus: Die Fläche reicht bis an alle Ränder. Oben in der Mitte schwebt eine
//! kleine Blase mit dem Arbeitsbereich (Aufklappliste der Projekte), Zentrieren, Vollbild und dem
//! Profilbild (dasselbe Menü wie im Chat-Modus, dort wird auch zurück gewechselt); links senkrecht
//! in der Mitte eine Leiste mit den Zeichenwerkzeugen und darunter neuen Fenstern; unten nur Dieter.

use super::board::{working_in, Board};
use super::draw::{COLORS, NOTE_COLORS, STROKES};
use super::store::*;
use crate::components::overlay::*;
use crate::i18n::{tr, trf};
use crate::pages::workspace::parts::{ProjectBadge, ADD_PROJECT};
use crate::state::{PROJECTS, THREADS};
use crate::Route;
use dioxus::prelude::*;
use nrzcode_shared::ProjectSummary;

#[component]
pub fn CanvasShell(board: bool, children: Element) -> Element {
    let ws = WS();
    let project_id = project_of(ws.max(0));
    rsx! {
        div { class: "fixed inset-0 overflow-hidden bg-background text-foreground",
            if board {
                Board {}
            } else {
                div { id: "nrz-main", class: "absolute inset-x-0 top-16 bottom-0 overflow-y-auto", {children} }
            }
            // App (macOS, Windows): oberster Streifen zum Verschieben des Fensters (die Fläche reicht bis oben)
            div { class: "fixed inset-x-0 top-0 z-30 hidden h-2.5 [.nrz-mac_&]:block [.nrz-win_&]:block", "data-drag": "1" }
            TopBubble { board }
            if board {
                Toolbar {}
                ZoomControls {}
                div { class: "pointer-events-none fixed inset-x-0 bottom-3 z-40 flex justify-center px-4",
                    div { class: "pointer-events-auto w-[min(36rem,100%)]",
                        crate::pages::workspace::dieter::DieterDock { project_id, can_add: true, canvas: true }
                    }
                }
                SketchDialog {}
            }
        }
    }
}

const TOP_BTN: &str = "grid size-8 shrink-0 place-items-center rounded-xl text-muted-foreground hover:bg-accent hover:text-foreground [&_svg]:size-4";

/// Oben in der Mitte: Arbeitsbereich · Zentrieren · Vollbild · Profil – als kleine Blase über der
/// Fläche statt einer Leiste.
#[component]
fn TopBubble(board: bool) -> Element {
    let nav = use_navigator();
    rsx! {
        div { class: "pointer-events-none fixed inset-x-0 top-3 z-50 flex justify-center px-3 [.nrz-win_&]:px-[150px]",
            div {
                class: "nrz-pane pointer-events-auto flex max-w-full items-center gap-0.5 rounded-2xl border bg-popover/95 p-1 shadow-xl",
                "data-drag": "1",
                onpointerdown: move |e| e.stop_propagation(),
                WorkspaceMenu { board }
                div { class: "mx-1 h-5 w-px shrink-0 bg-border" }
                button {
                    class: TOP_BTN,
                    title: tr("Alles ordnen und zentrieren"),
                    onclick: move |_| {
                        if board {
                            arrange();
                        } else {
                            nav.push(Route::Canvas {});
                        }
                    },
                    icons::LocateFixed {}
                }
                button {
                    class: TOP_BTN,
                    title: if FULLSCREEN() { tr("Vollbild beenden") } else { tr("Vollbild – nur noch die Canvas") },
                    onclick: move |_| toggle_fullscreen(),
                    if FULLSCREEN() {
                        icons::Minimize {}
                    } else {
                        icons::Maximize {}
                    }
                }
                div { class: "mx-1 h-5 w-px shrink-0 bg-border" }
                div { class: "pr-0.5", crate::pages::workspace::layout::UserMenu { side: Side::Bottom } }
            }
        }
        RightBubble {}
    }
}

/// Oben rechts dieselben Knöpfe wie im Chat-Modus (Coding, Canvas, Nutzung) an derselben Stelle –
/// nur in einer Blase. Senkrecht wie die Knöpfe der Kopfzeile (am Mac auf der Höhe der Ampel).
#[component]
fn RightBubble() -> Element {
    let nav = use_navigator();
    rsx! {
        div {
            class: "nrz-pane fixed top-[6.5px] right-[7px] z-50 flex items-center gap-0.5 rounded-2xl border bg-popover/95 p-1 shadow-xl md:right-[11px] [.nrz-win_&]:right-[145px]",
            "data-drag": "1",
            onpointerdown: move |e| e.stop_propagation(),
            crate::pages::workspace::cli::CliButton { class: TOP_BTN }
            button {
                class: "{TOP_BTN} bg-accent text-foreground",
                title: tr("Zurück zum Chat-Modus"),
                onclick: move |_| {
                    let next = super::leave_to();
                    crate::state::set_mode(crate::state::UiMode::Chat);
                    nav.push(next);
                },
                icons::Frame {}
            }
            crate::pages::agents::UsageRing { class: TOP_BTN }
        }
    }
}

/// Aufklappliste der Arbeitsbereiche (Projekte, „Ohne Projekt“) mit „Projekt hinzufügen“.
#[component]
fn WorkspaceMenu(board: bool) -> Element {
    let nav = use_navigator();
    let ws = WS();
    let mut projects: Vec<ProjectSummary> = PROJECTS.read().values().cloned().collect();
    projects.sort_by_key(|p| (p.name.to_lowercase(), p.id));
    let loose = THREADS.read().values().any(|t| t.parent_id.is_none() && t.project_id.is_none() && t.bot_id.is_none() && !t.hidden);
    let current = projects.iter().find(|p| p.id == ws).cloned();
    let working = working_in(ws.max(0));
    let open = move |key: i64| {
        set_ws(key);
        if !board {
            nav.push(Route::Canvas {});
        }
    };
    rsx! {
        Dropdown {
            align: Align::Start,
            class: "max-h-[70vh] w-72 overflow-y-auto",
            trigger_class: "flex h-8 max-w-64 min-w-0 items-center gap-2 rounded-xl px-2.5 text-sm font-medium hover:bg-accent",
            trigger: rsx! {
                match &current {
                    Some(p) => rsx! { ProjectBadge { name: Some(p.name.clone()), size: 18 } },
                    None => rsx! { icons::MessagesSquare { class: "size-4 shrink-0" } },
                }
                span { class: "min-w-0 truncate",
                    match &current {
                        Some(p) => p.name.clone(),
                        None => tr("Ohne Projekt"),
                    }
                }
                if working > 0 {
                    WorkingBadge { n: working }
                }
                icons::ChevronDown { class: "size-3.5 shrink-0 text-muted-foreground" }
            },
            MenuLabel { {tr("Arbeitsbereich")} }
            for p in projects {
                MenuItem { key: "{p.id}", active: board && ws == p.id, onclick: move |_| open(p.id),
                    ProjectBadge { name: Some(p.name.clone()), size: 18 }
                    span { class: "min-w-0 flex-1 truncate", "{p.name}" }
                    if p.status == "cloning" {
                        icons::LoaderCircle { class: "animate-spin" }
                    }
                    if working_in(p.id) > 0 {
                        WorkingBadge { n: working_in(p.id) }
                    }
                }
            }
            if loose || ws <= 0 {
                MenuItem { active: board && ws == 0, onclick: move |_| open(0),
                    icons::MessagesSquare {}
                    span { class: "min-w-0 flex-1 truncate", {tr("Ohne Projekt")} }
                    if working_in(0) > 0 {
                        WorkingBadge { n: working_in(0) }
                    }
                }
            }
            MenuSeparator {}
            MenuItem { onclick: move |_| *ADD_PROJECT.write() = true,
                icons::Plus {}
                {tr("Projekt hinzufügen")}
            }
        }
    }
}

#[component]
fn WorkingBadge(n: usize) -> Element {
    rsx! {
        span {
            class: "inline-flex h-4 min-w-4 shrink-0 items-center justify-center rounded-full bg-success/20 px-1 text-[10px] font-semibold tabular-nums text-success",
            title: tr("Arbeiten gerade"),
            "{n}"
        }
    }
}

// Niedrige Fenster: kleinere Knöpfe, damit die ganze Leiste links Platz hat
const TOOL_BTN: &str = "grid size-8 place-items-center rounded-lg text-muted-foreground hover:bg-accent hover:text-foreground disabled:pointer-events-none disabled:opacity-30 [&_svg]:size-[17px] [@media(max-height:820px)]:size-7 [@media(max-height:700px)]:size-6 [@media(max-height:700px)]:[&_svg]:size-[15px]";
const TOOL_ON: &str = "grid size-8 place-items-center rounded-lg bg-primary text-primary-foreground [&_svg]:size-[17px] [@media(max-height:820px)]:size-7 [@media(max-height:700px)]:size-6 [@media(max-height:700px)]:[&_svg]:size-[15px]";
// Ohne backdrop-filter: das machte die Leiste zum Bezugsrahmen für `fixed`, und ein Klick neben
// ein offenes Menü schloss es nicht
const PILL: &str = "nrz-pane pointer-events-auto flex flex-col items-center gap-0.5 rounded-2xl border bg-popover/95 p-1 shadow-xl";
const PILL_SEP: &str = "my-0.5 h-px w-5 shrink-0 bg-border";

fn tool_label(t: Tool) -> String {
    let name = match t {
        Tool::Select => tr("Auswählen"),
        Tool::Hand => tr("Verschieben (Hand)"),
        Tool::Pen => tr("Stift"),
        Tool::Rect => tr("Rechteck"),
        Tool::Ellipse => tr("Ellipse"),
        Tool::Diamond => tr("Raute"),
        Tool::Arrow => tr("Pfeil"),
        Tool::Line => tr("Linie"),
        Tool::Text => tr("Text"),
        Tool::Note => tr("Notizzettel"),
        Tool::Eraser => tr("Radierer"),
    };
    format!("{name} ({})", t.key().to_uppercase())
}

fn tool_icon(t: Tool) -> Element {
    match t {
        Tool::Select => rsx! { icons::MousePointer2 {} },
        Tool::Hand => rsx! { icons::Hand {} },
        Tool::Pen => rsx! { icons::Pencil {} },
        Tool::Rect => rsx! { icons::Square {} },
        Tool::Ellipse => rsx! { icons::Circle {} },
        Tool::Diamond => rsx! { icons::Diamond {} },
        Tool::Arrow => rsx! { icons::ArrowUpRight {} },
        Tool::Line => rsx! { icons::Minus {} },
        Tool::Text => rsx! { icons::Type {} },
        Tool::Note => rsx! { icons::StickyNote {} },
        Tool::Eraser => rsx! { icons::Eraser {} },
    }
}

/// Farbe/Strich auf die ausgewählte Form anwenden.
fn apply_to_selection(f: impl Fn(&mut nrzcode_shared::canvas::CanvasShape)) {
    let Some(id) = SHAPE_SEL.peek().clone() else { return };
    let Some(mut s) = BOARD.peek().as_ref().and_then(|b| b.shapes.iter().find(|x| x.id == id).cloned()) else { return };
    f(&mut s);
    super::draw::fit_text(&mut s);
    remember_shapes();
    put_shape(s);
}

/// Links senkrecht in der Mitte, eine Leiste: die Zeichenwerkzeuge wie in Excalidraw (mit Stil und
/// Rückgängig), darunter neue Fenster (Chat, Terminal, Browser), Threads zurückholen und die Skizze
/// an einen Chat geben.
#[component]
fn Toolbar() -> Element {
    let tool = TOOL();
    let has_shapes = BOARD.read().as_ref().is_some_and(|b| !b.shapes.is_empty());
    rsx! {
        div { class: "pointer-events-none fixed inset-y-0 left-3 z-40 flex items-center",
        div { class: PILL, onpointerdown: move |e| e.stop_propagation(),
            for t in Tool::ALL {
                button {
                    key: "{t.key()}",
                    class: if tool == t { TOOL_ON } else { TOOL_BTN },
                    title: tool_label(t),
                    onclick: move |_| {
                        *TOOL.write() = t;
                        if t != Tool::Select {
                            *SHAPE_SEL.write() = None;
                        }
                    },
                    {tool_icon(t)}
                }
            }
            div { class: PILL_SEP }
            StylePicker {}
            button { class: TOOL_BTN, title: crate::util::keys(&tr("Rückgängig (⌘Z)")), disabled: !can_undo(), onclick: move |_| undo(false), icons::Undo2 {} }
            button { class: TOOL_BTN, title: crate::util::keys(&tr("Wiederholen (⇧⌘Z)")), disabled: !can_redo(), onclick: move |_| undo(true), icons::Redo2 {} }
            if has_shapes {
                button {
                    class: TOOL_BTN,
                    title: tr("Zeichnung löschen"),
                    onclick: move |_| {
                        spawn(async move {
                            let ask = Ask::new(tr("Ganze Zeichnung löschen?"), tr("Alle Striche, Formen, Texte und Notizen dieses Arbeitsbereichs verschwinden. Die Fenster bleiben.")).confirm(tr("Löschen")).destructive();
                            if confirm(ask).await {
                                clear_shapes();
                            }
                        });
                    },
                    icons::Trash2 {}
                }
            }
            div { class: PILL_SEP }
            button { class: TOOL_BTN, title: tr("Neuer Chat"), onclick: move |_| new_chat_window(), icons::MessageSquarePlus {} }
            button {
                class: TOOL_BTN,
                title: tr("Terminal"),
                onclick: move |_| {
                    if let Some(id) = add_window("terminal", None, "", None) {
                        fly_to(&id);
                    }
                },
                icons::SquareTerminal {}
            }
            button {
                class: TOOL_BTN,
                title: tr("Browser"),
                onclick: move |_| {
                    if let Some(id) = add_window("browser", None, "", None) {
                        fly_to(&id);
                    }
                },
                icons::Globe {}
            }
            ReopenMenu {}
            if has_shapes {
                button { class: TOOL_BTN, title: tr("Skizze an einen Chat geben"), onclick: move |_| *SKETCH_OPEN.write() = true, icons::Send {} }
            }
        }
        }
    }
}

/// Farbe und Strichstärke (gilt für neue Formen und die ausgewählte).
#[component]
fn StylePicker() -> Element {
    let mut open = use_signal(|| false);
    let color = COLOR();
    let stroke = STROKE();
    let note_selected = SHAPE_SEL().and_then(|id| BOARD.read().as_ref().and_then(|b| b.shapes.iter().find(|x| x.id == id).map(|s| s.kind == "note"))).unwrap_or(false);
    rsx! {
        div { class: "relative",
            button {
                class: if open() { "grid size-8 place-items-center rounded-lg bg-accent [@media(max-height:820px)]:size-7 [@media(max-height:700px)]:size-6" } else { "grid size-8 place-items-center rounded-lg hover:bg-accent [@media(max-height:820px)]:size-7 [@media(max-height:700px)]:size-6" },
                title: tr("Farbe und Strich"),
                onclick: move |_| open.toggle(),
                span { class: "size-4 rounded-full ring-2 ring-border", style: "background: {super::draw::css_color(&color)}" }
            }
            if open() {
                div { class: "fixed inset-0 z-40", onclick: move |_| open.set(false) }
                div { class: "absolute top-0 left-full z-50 ml-2 grid w-44 gap-2 rounded-xl border bg-popover p-2.5 shadow-xl animate-in",
                    p { class: "text-[11px] font-medium text-muted-foreground", if note_selected { {tr("Farbe der Notiz")} } else { {tr("Farbe")} } }
                    div { class: "flex flex-wrap gap-1",
                        if note_selected {
                            for c in NOTE_COLORS {
                                button {
                                    key: "{c}",
                                    class: "grid size-7 place-items-center rounded-md hover:bg-accent",
                                    onclick: move |_| apply_to_selection(move |s| s.color = c.to_string()),
                                    span { class: "size-4 rounded-sm ring-1 ring-black/10", style: "background: {c}" }
                                }
                            }
                        } else {
                            for c in COLORS {
                                button {
                                    key: "c{c}",
                                    class: if color == c { "grid size-7 place-items-center rounded-md bg-accent" } else { "grid size-7 place-items-center rounded-md hover:bg-accent" },
                                    title: if c.is_empty() { tr("Vordergrund") } else { c.to_string() },
                                    onclick: move |_| {
                                        *COLOR.write() = c.to_string();
                                        apply_to_selection(move |s| if s.kind != "note" { s.color = c.to_string() });
                                    },
                                    span { class: "size-4 rounded-full ring-1 ring-border", style: "background: {super::draw::css_color(c)}" }
                                }
                            }
                        }
                    }
                    p { class: "text-[11px] font-medium text-muted-foreground", {tr("Strich / Schrift")} }
                    div { class: "flex gap-1",
                        for w in STROKES {
                            button {
                                key: "{w}",
                                class: if (stroke - w).abs() < 0.1 { "grid h-7 flex-1 place-items-center rounded-md bg-accent" } else { "grid h-7 flex-1 place-items-center rounded-md hover:bg-accent" },
                                onclick: move |_| {
                                    *STROKE.write() = w;
                                    apply_to_selection(move |s| s.size = w);
                                },
                                span { class: "w-6 rounded-full bg-foreground/80", style: "height: {(w * 0.8).max(1.5)}px" }
                            }
                        }
                    }
                    label { class: "flex items-center gap-2 text-xs",
                        input {
                            r#type: "checkbox",
                            checked: SHAPE_SEL().and_then(|id| BOARD.read().as_ref().and_then(|b| b.shapes.iter().find(|x| x.id == id).map(|s| s.fill))).unwrap_or(false),
                            onchange: move |e| {
                                let v = e.checked();
                                apply_to_selection(move |s| s.fill = v);
                            },
                        }
                        {tr("Gefüllt (ausgewählte Form)")}
                    }
                }
            }
        }
    }
}

/// Threads des Arbeitsbereichs, die (noch) kein Fenster haben – wieder auf die Fläche holen.
#[component]
fn ReopenMenu() -> Element {
    let ws = WS().max(0);
    let on_board: Vec<i64> = BOARD.read().as_ref().map(|b| b.windows.iter().filter_map(|w| w.thread_id).collect()).unwrap_or_default();
    let mut list: Vec<_> = THREADS.read().values().filter(|t| t.parent_id.is_none() && t.bot_id.is_none() && t.project_id.unwrap_or(0) == ws && !on_board.contains(&t.id)).cloned().collect();
    list.sort_by_key(|t| std::cmp::Reverse(t.last_activity_at));
    list.truncate(30);
    if list.is_empty() {
        return rsx! {};
    }
    rsx! {
        Dropdown {
            align: Align::Start,
            side: Side::Top,
            class: "left-full bottom-0 mb-0 ml-2 max-h-96 w-80 overflow-y-auto",
            trigger_class: TOOL_BTN,
            trigger: rsx! { icons::Layers {} },
            MenuLabel { {trf("Wieder auf die Canvas ({0})", &[&list.len()])} }
            for t in list {
                MenuItem {
                    key: "{t.id}",
                    onclick: move |_| {
                        if let Some(id) = add_window("chat", Some(t.id), "", None) {
                            fly_to(&id);
                        }
                    },
                    crate::pages::agents::AgentIcon { kind: t.agent, size: 14 }
                    span { class: "min-w-0 flex-1 truncate", {crate::pages::workspace::parts::thread_title(&t)} }
                    crate::pages::workspace::parts::ThreadState { thread: t.clone() }
                }
            }
        }
    }
}

/// Unten rechts: Zoom und „alles zeigen“.
#[component]
fn ZoomControls() -> Element {
    let z = CAM().z;
    let btn = "grid size-8 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground [&_svg]:size-4";
    rsx! {
        div { class: "nrz-pane fixed right-3 bottom-3 z-40 hidden items-center gap-0.5 rounded-xl border bg-popover/90 p-1 shadow-lg backdrop-blur-md md:flex",
            button { class: btn, title: crate::util::keys(&tr("Verkleinern (⌘−)")), onclick: move |_| zoom_step(0.8), icons::ZoomOut {} }
            button { class: "h-8 min-w-14 rounded-md px-1.5 text-xs tabular-nums text-muted-foreground hover:bg-accent hover:text-foreground", title: crate::util::keys(&tr("Auf 100 % (⌘0)")), onclick: move |_| zoom_step(1.0 / CAM.peek().z), "{(z * 100.0).round()} %" }
            button { class: btn, title: crate::util::keys(&tr("Vergrössern (⌘+)")), onclick: move |_| zoom_step(1.25), icons::ZoomIn {} }
            button { class: btn, title: tr("Alles zeigen (⇧1)"), onclick: move |_| fit_all(), icons::Scan {} }
        }
    }
}

/// „Skizze an einen Chat geben“: Auftrag und die Zeichnung als Text an ein Chat-Fenster.
#[component]
fn SketchDialog() -> Element {
    let mut open = use_signal(|| false);
    use_effect(move || {
        if SKETCH_OPEN() && !open() {
            open.set(true);
        }
    });
    use_effect(move || {
        if !open() && *SKETCH_OPEN.peek() {
            *SKETCH_OPEN.write() = false;
        }
    });
    rsx! {
        Modal { open, title: tr("Skizze an einen Chat geben"), description: tr("Der Agent bekommt deinen Auftrag und die Zeichnung als Beschreibung – und kann die Canvas selbst lesen und darauf zeichnen."), class: "max-w-xl",
            SketchBody { open }
        }
    }
}

#[component]
fn SketchBody(open: Signal<bool>) -> Element {
    let board = BOARD.read().clone().unwrap_or_default();
    let chats: Vec<(String, String, Option<i64>)> = board
        .windows
        .iter()
        .filter(|w| matches!(w.kind.as_str(), "chat" | "new"))
        .map(|w| (w.id.clone(), w.name.clone(), w.thread_id))
        .collect();
    let mut target = use_signal(|| chats.first().map(|c| c.0.clone()).unwrap_or_else(|| "+".into()));
    let mut task = use_signal(|| tr("Setze diese Skizze um."));
    let mut busy = use_signal(|| false);
    let desc = super::draw::describe(&board.shapes);
    let project_id = board.project_id;
    let send = {
        let desc = desc.clone();
        let chats = chats.clone();
        move |_| {
            let text = format!("{}\n\n{}\n{}\n{}", task().trim(), tr("Skizze auf der Canvas:"), desc.trim_end(), tr("(Mit canvas_read siehst du die ganze Canvas.)"));
            let pick = chats.iter().find(|c| c.0 == target()).cloned();
            busy.set(true);
            spawn(async move {
                let res = match pick {
                    Some((_, _, Some(tid))) => crate::api::post::<nrzcode_shared::ThreadSummary, _>(
                        &format!("/threads/{tid}/messages"),
                        &nrzcode_shared::agents::PostMessage { text, attachments: Vec::new(), interrupt: false },
                    )
                    .await
                    .map(|t| {
                        crate::state::upsert_thread(t);
                        format!("t{tid}")
                    }),
                    Some((win, _, None)) => crate::pages::agents::start_thread(project_id, text).await.map(|t| {
                        window_created(&win, t.id);
                        format!("t{}", t.id)
                    }),
                    None => crate::pages::agents::start_thread(project_id, text).await.map(|t| {
                        add_window("chat", Some(t.id), "", None);
                        format!("t{}", t.id)
                    }),
                };
                busy.set(false);
                match res {
                    Ok(id) => {
                        open.set(false);
                        fly_to(&id);
                    }
                    Err(e) => toast_error(e),
                }
            });
        }
    };
    rsx! {
        div { class: "grid gap-4",
            label { class: "grid gap-1.5 text-sm",
                span { class: "font-medium", {tr("An")} }
                select {
                    class: crate::components::common::SELECT,
                    onchange: move |e| target.set(e.value()),
                    for (id , name , tid) in chats.iter().cloned() {
                        option { key: "{id}", value: "{id}", selected: target() == id,
                            if tid.is_some() { "{name}" } else { {trf("{0} (neuer Chat)", &[&name])} }
                        }
                    }
                    option { value: "+", selected: target() == "+", {tr("Neues Chat-Fenster")} }
                }
            }
            label { class: "grid gap-1.5 text-sm",
                span { class: "font-medium", {tr("Auftrag")} }
                textarea { class: "{crate::components::common::TEXTAREA} min-h-20", value: "{task}", oninput: move |e| task.set(e.value()) }
            }
            div { class: "grid gap-1.5 text-sm",
                span { class: "font-medium", {tr("So sieht der Agent die Zeichnung")} }
                pre { class: "max-h-48 overflow-auto rounded-lg border bg-muted/40 p-3 font-sans text-xs whitespace-pre-wrap text-muted-foreground", "{desc}" }
            }
            div { class: "flex justify-end gap-2",
                button { class: crate::components::common::btn(crate::components::common::BTN_OUTLINE), onclick: move |_| open.set(false), {tr("Abbrechen")} }
                button { class: crate::components::common::btn(crate::components::common::BTN_PRIMARY), disabled: busy() || task().trim().is_empty(), onclick: send,
                    crate::pages::workspace::parts::BusyIcon { busy: busy() }
                    {tr("Senden")}
                }
            }
        }
    }
}
