//! Rahmen der App wie ein Coding-Agent-Client: links die Seitenleiste (nrzCode, Canvas-Modus,
//! Suche, Filter, „Projekt hinzufügen“, „Neuer Thread“, letzte Threads, Gruppen „In Arbeit“ und
//! „Abgelegt“, unten nur „Nach Updates suchen“ (in der App, bis geprüft) und „Einstellungen“),
//! oben die Kopfzeile „Projekt / Thread“ mit drei Knöpfen für die Spalten und dem Ring der
//! Nutzungslimits. Die Einstellungen füllen das ganze Fenster (eigene Navigation, X zurück).
//! Lädt Status, Benutzer, Threads und Projekte und startet die Live-Ereignisse; leitet zur
//! Einrichtung bzw. Anmeldung weiter.

use super::parts::*;
use super::projects::AddProjectDialog;
use crate::Route;
use crate::api;
use crate::components::common::*;
use crate::components::overlay::*;
use crate::components::widgets::*;
use crate::i18n::{self, LANG, LANGUAGES, tr, trf};
use crate::state::{self, ME, ME_LOADED, PROJECTS, THREADS};
use crate::util;
use dioxus::prelude::*;
use nrzcode_shared::agents::{ThreadOrder, ThreadPatch};
use nrzcode_shared::workspace::{Appearance, Stats};
use nrzcode_shared::{ThreadStatus, ThreadSummary, UserRef};

/// Rahmen aller Seiten (ausser Anmeldung und Einrichtung).
#[component]
pub fn AppLayout() -> Element {
    let nav = use_navigator();
    let route = use_route::<Route>();
    let boot = use_resource(|| async {
        let status = state::load_status().await;
        let me = state::load_me().await;
        if let Some(m) = &me {
            if !m.language.is_empty() && *LANG.peek() != m.language {
                i18n::set_language(&m.language, true).await;
            }
            load_projects().await;
            load_threads().await;
            crate::events::connect();
            // Verläufe laufender und der neuesten Threads im Hintergrund laden: beim Wechsel
            // zwischen Threads und Agenten sind die Nachrichten sofort da
            let mut list: Vec<ThreadSummary> = THREADS.peek().values().cloned().collect();
            list.sort_by_key(|t| (std::cmp::Reverse(t.status.is_active()), std::cmp::Reverse(t.last_activity_at)));
            crate::pages::agents::prefetch_messages(list.into_iter().take(12).map(|t| t.id).collect());
        }
        (status, me.is_some())
    });

    super::parts::remember_new_thread_project();
    // Klick auf eine Benachrichtigung oder in der Menüleiste: Thread öffnen (-1 = neuer Thread)
    use_effect(move || {
        if let Some(id) = crate::notify::OPEN_THREAD() {
            *crate::notify::OPEN_THREAD.write() = None;
            if id > 0 {
                nav.push(Route::ThreadPage { id });
            } else if id == -1 {
                // Aus der Menüleiste: neuer Thread (im Canvas-Modus ein neues Chat-Fenster)
                if *state::MODE.peek() == state::UiMode::Canvas {
                    crate::pages::canvas::new_chat_window();
                    nav.push(Route::Canvas {});
                } else {
                    nav.push(Route::Home {});
                }
            }
        }
    });

    // Einstellungen öffnen (⌘, im Menü der App, Strg+, unter Windows und im Browser)
    use_effect(move || {
        if state::OPEN_SETTINGS() {
            *state::OPEN_SETTINGS.write() = false;
            nav.push(settings_route());
        }
    });

    // Letzte Seite ausserhalb der Einstellungen: dorthin führt ihr X zurück
    use_effect(use_reactive!(|route| {
        if !matches!(route, Route::Settings { .. }) {
            *RETURN_TO.write() = Some(route.clone());
        }
    }));

    // Canvas-Modus: Startseite, Raster und Thread-Seiten zeigen die Fläche (ein Thread wird dort
    // als Fenster geöffnet); die Adresse /canvas schaltet den Modus ein
    use_effect(use_reactive!(|route| {
        let mode = state::MODE();
        match (&route, mode) {
            (Route::Canvas {}, state::UiMode::Chat) => state::set_mode(state::UiMode::Canvas),
            (Route::ThreadPage { id }, state::UiMode::Canvas) => {
                crate::pages::canvas::open_thread(*id);
                nav.replace(Route::Canvas {});
            }
            (Route::Home {} | Route::Grid { .. }, state::UiMode::Canvas) => {
                nav.replace(Route::Canvas {});
            }
            _ => {}
        }
    }));

    // Umschalter Code | Bot folgt der Seite (z. B. nach einem Link oder Zurück im Browser)
    use_effect(use_reactive!(|route| match route {
        Route::Bots {} | Route::BotPage { .. } => state::set_work(state::Work::Bot),
        Route::Home {} | Route::ThreadPage { .. } | Route::Grid { .. } => state::set_work(state::Work::Code),
        _ => {}
    }));

    // Weiterleiten: noch nicht eingerichtet → Einrichtung; nicht angemeldet → Anmelden
    use_effect(move || {
        if let Some((Some(status), signed_in)) = boot.read().clone() {
            if status.needs_setup {
                nav.replace(Route::Setup {});
            } else if !signed_in {
                nav.replace(Route::Login { next: util::current_path() });
            }
        }
    });

    // Sekundentakt für „Arbeitet 25s“; nach einer Unterbrechung Listen neu laden
    use_future(|| async {
        let mut n = 0u32;
        loop {
            gloo_timers::future::TimeoutFuture::new(1000).await;
            *NOW.write() = util::now();
            n += 1;
            if n.is_multiple_of(120) && ME.peek().is_some() {
                load_threads().await;
                load_projects().await;
            }
        }
    });

    // Seitenwechsel: Sheets auf dem Handy schliessen
    use_effect(use_reactive!(|route| {
        let _ = &route;
        *MOBILE_NAV.write() = false;
        *MOBILE_RIGHT.write() = false;
    }));

    if boot.read().is_none() || !ME_LOADED() || ME.read().is_none() {
        return rsx! {
            div { class: "grid h-dvh place-items-center", Loading {} }
        };
    }
    if state::MODE() == state::UiMode::Canvas || matches!(route, Route::Canvas {}) {
        // Canvas-Modus: eigener Rahmen (oben Arbeitsbereiche, links Zeichnen, unten Dieter);
        // Einstellungen und Projekte erscheinen darin unter der oberen Leiste
        let board = matches!(route, Route::Canvas {} | Route::Home {} | Route::Grid { .. } | Route::ThreadPage { .. });
        return rsx! {
            crate::pages::canvas::CanvasShell { board, Outlet::<Route> {} }
            AddProjectDialog {}
        };
    }
    // Einstellungen: das ganze Fenster (eigene Navigation links, oben rechts X zurück)
    if matches!(route, Route::Settings { .. }) {
        return rsx! {
            div { class: "flex h-dvh overflow-hidden bg-background [.nrz-glass_&]:bg-transparent", Outlet::<Route> {} }
            AddProjectDialog {}
        };
    }
    // Milchglas: die Seitenleiste nur leicht getönt, der Inhalt stärker (daher die Fläche erst dort)
    rsx! {
        div { class: "flex h-dvh overflow-hidden bg-background [.nrz-glass_&]:bg-transparent",
            // Schmale Leiste ganz links (Start, Bots, Projekte, Canvas; unten Nutzung, Einstellungen, Profil)
            Rail {}
            if LEFT() {
                aside { class: "hidden w-72 shrink-0 flex-col border-r border-sidenav-border bg-sidenav md:flex", Sidebar { sheet: false } }
            }
            if MOBILE_NAV() {
                div { class: "fixed inset-0 z-50 bg-black/60 md:hidden", onclick: move |_| *MOBILE_NAV.write() = false }
                aside { class: "nrz-pane fixed inset-y-0 left-0 z-50 flex w-[85vw] max-w-80 flex-col border-r bg-sidenav shadow-xl animate-in md:hidden", Sidebar { sheet: true } }
            }
            div { class: "flex min-w-0 flex-1 flex-col [.nrz-glass_&]:bg-background",
                TopBar {}
                main { id: "nrz-main", class: "min-h-0 flex-1 overflow-y-auto", Outlet::<Route> {} }
            }
        }
        AddProjectDialog {}
        super::bots::BotEditor {}
        ThreadMenu {}
        ThreadStatsDialog {}
        ProjectMenu {}
    }
}

// ------------------------------------------------------------------ Leiste links

const RAIL_BTN: &str = "grid size-9 shrink-0 place-items-center rounded-lg text-muted-foreground transition-colors hover:bg-sidenav-accent hover:text-foreground [&_svg]:size-[18px]";
const RAIL_ON: &str = "grid size-9 shrink-0 place-items-center rounded-lg bg-sidenav-accent text-foreground shadow-[inset_0_0_0_1px_var(--color-sidenav-border)] [&_svg]:size-[18px]";

/// Schmale Leiste ganz links: oben Start (Code), Bots, Projekte und Canvas, unten der Ring der
/// Nutzungslimits, Einstellungen und das Profil. Nur ab md (auf dem Handy die Seitenleiste).
#[component]
fn Rail() -> Element {
    let route = use_route::<Route>();
    let nav = use_navigator();
    let work = state::WORK();
    let projects = matches!(route, Route::Projects {} | Route::ProjectPage { .. });
    let bots = work == state::Work::Bot && !projects;
    let code = work == state::Work::Code && !projects;
    let cls = |on: bool| if on { RAIL_ON } else { RAIL_BTN };
    let go = move |w: state::Work| {
        if w != *state::WORK.peek() {
            state::set_work(w);
        }
        let to = if w == state::Work::Bot { super::bots::bot_route() } else { Route::Home {} };
        nav.push(to);
    };
    rsx! {
        nav { class: if LEFT() { "hidden w-14 shrink-0 flex-col items-center gap-1.5 bg-sidenav pb-3 md:flex" } else { "hidden w-14 shrink-0 flex-col items-center gap-1.5 border-r border-sidenav-border bg-sidenav pb-3 md:flex" },
            // Oben die Titelleiste (Ampel am Mac, Fenster ziehen)
            div { class: "h-12 w-full shrink-0 [.nrz-mac_&]:h-14", "data-drag": "1" }
            button { class: cls(code), title: tr("Code – Coding-Agenten in deinen Projekten"), onclick: move |_| go(state::Work::Code),
                icons::House {}
            }
            button { class: cls(bots), title: tr("Bots mit Gedächtnis und Routinen"), onclick: move |_| go(state::Work::Bot),
                icons::Bot {}
            }
            button { class: cls(projects), title: tr("Projekte"), onclick: move |_| { nav.push(Route::Projects {}); },
                icons::FolderGit2 {}
            }
            button {
                class: RAIL_BTN,
                title: tr("Canvas-Modus: alle Fenster eines Arbeitsbereichs frei auf einer Fläche"),
                onclick: {
                    let route = route.clone();
                    move |_| {
                        let next = crate::pages::canvas::enter_from(&route);
                        state::set_mode(state::UiMode::Canvas);
                        nav.push(next);
                    }
                },
                icons::Frame {}
            }
            button { class: if LEFT() { RAIL_ON } else { RAIL_BTN }, title: tr("Seitenleiste ein-/ausblenden"), onclick: move |_| toggle(&LEFT, "nrz-left"),
                icons::PanelLeft {}
            }
            div { class: "flex-1" }
            // Unten links: Nach Updates suchen (in der App), Nutzung, Einstellungen, Profil
            super::update::UpdateRailButton { class: RAIL_BTN }
            crate::pages::agents::UsageRing { class: RAIL_BTN, align: Align::Start, side: Side::Top }
            Link { to: settings_route(), class: RAIL_BTN, title: format!("{} ({})", tr("Einstellungen"), settings_shortcut()),
                icons::Settings {}
            }
            UserMenu { side: Side::Top, align: Align::Start }
        }
    }
}

// ------------------------------------------------------------------ Seitenleiste

const SIDE_ICON_BTN: &str = "grid size-7 shrink-0 place-items-center rounded-md text-muted-foreground hover:bg-sidenav-accent hover:text-foreground [&_svg]:size-4";

/// Zeile unten in der Seitenleiste („Nach Updates suchen“, „Einstellungen“).
pub const FOOT_ROW: &str = "flex h-8 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-[13px] text-muted-foreground transition-colors hover:bg-sidenav-accent hover:text-foreground [&_svg]:size-4 [&_svg]:shrink-0";

/// Seite vor den Einstellungen (Ziel ihres X).
pub static RETURN_TO: GlobalSignal<Option<Route>> = Signal::global(|| None);

/// Zuletzt offener Abschnitt der Einstellungen.
pub fn settings_route() -> Route {
    let section = util::storage_get("nrz-settings-section").filter(|s| !s.is_empty()).unwrap_or_else(|| "profile".into());
    Route::Settings { section }
}

/// Einstellungen schliessen: zurück zur Seite davor (sonst Startseite).
pub fn close_settings(nav: dioxus::router::Navigator) {
    let to = RETURN_TO.peek().clone().filter(|r| !matches!(r, Route::Settings { .. })).unwrap_or(Route::Home {});
    nav.push(to);
}

/// Kürzel der Einstellungen: ⌘, am Mac, sonst Strg+,.
pub fn settings_shortcut() -> String {
    if util::is_mac() { "⌘,".into() } else { tr("Strg+,") }
}

/// Filter der Thread-Liste: alle, ohne Projekt, ein Projekt.
#[derive(Clone, Copy, PartialEq)]
enum ProjectFilter {
    All,
    NoProject,
    Project(i64),
}

#[component]
fn Sidebar(sheet: bool) -> Element {
    let route = use_route::<Route>();
    let nav = use_navigator();
    // Neuer Thread, der gerade angelegt wird: schon als Eintrag zeigen; ist der echte Thread
    // schon gemeldet (vor der Antwort), gilt er auf der Startseite als offen
    let pending = state::pending_visible().filter(|_| matches!(route, Route::Home {}));
    let current = match route {
        Route::ThreadPage { id } => Some(id),
        Route::Home {} => state::pending_arrived(),
        _ => None,
    };
    let mut query = use_signal(String::new);
    let mut filter = use_signal(|| ProjectFilter::All);
    // Suche im Filtermenü nach Projekten
    let fq = use_signal(String::new);
    let mut working_open = use_signal(|| util::storage_get("nrz-group-working").as_deref() != Some("0"));
    let mut settled_open = use_signal(|| util::storage_get("nrz-group-settled").as_deref() == Some("1"));
    let mut hidden_open = use_signal(|| util::storage_get("nrz-group-hidden").as_deref() == Some("1"));

    let q = query().trim().to_lowercase();
    let f = filter();
    // Der offene Subagent zählt als offener Elternthread (der bleibt aufgeklappt)
    let current_root = current.and_then(|id| {
        let all = THREADS.read();
        let mut t = all.get(&id)?.clone();
        while let Some(p) = t.parent_id.and_then(|p| all.get(&p).cloned()) {
            t = p;
        }
        Some(t.id)
    });
    let mut all: Vec<ThreadSummary> = THREADS
        .read()
        .values()
        .filter(|t| t.parent_id.is_none() && t.bot_id.is_none())
        .filter(|t| match f {
            ProjectFilter::All => true,
            ProjectFilter::NoProject => t.project_id.is_none(),
            ProjectFilter::Project(p) => t.project_id == Some(p),
        })
        .filter(|t| {
            q.is_empty()
                || t.title.to_lowercase().contains(&q)
                || t.branch.to_lowercase().contains(&q)
                || t.project_name.as_deref().unwrap_or("").to_lowercase().contains(&q)
        })
        .cloned()
        .collect();
    // Angeheftete zuerst, dann feste Plätze (Ziehen und Ablegen) bzw. letzte Aktivität
    all.sort_by_key(|x| std::cmp::Reverse((x.pinned, x.sort_key.unwrap_or(x.last_activity_at), x.id)));
    // Wie T3 Code: eine Liste über alle Projekte, unten „Arbeitet“, „Abgelegt“, „Ausgeblendet“
    let hidden: Vec<ThreadSummary> = all.iter().filter(|t| t.hidden).cloned().collect();
    let settled: Vec<ThreadSummary> = all.iter().filter(|t| !t.hidden && t.status == ThreadStatus::Settled).cloned().collect();
    let working: Vec<ThreadSummary> = all.iter().filter(|t| !t.hidden && t.status.is_active()).cloned().collect();
    let recent: Vec<ThreadSummary> = all.iter().filter(|t| !t.hidden && t.status != ThreadStatus::Settled && !t.status.is_active()).cloned().collect();
    let ids = |l: &[ThreadSummary]| l.iter().map(|t| t.id).collect::<Vec<i64>>();
    let (recent_ids, working_ids, settled_ids, hidden_ids) = (ids(&recent), ids(&working), ids(&settled), ids(&hidden));

    let projects_map = PROJECTS.read().clone();
    let mut projects: Vec<(i64, String)> = projects_map.values().map(|p| (p.id, p.name.clone())).collect();
    projects.sort_by_key(|p| p.1.to_lowercase());
    let projects_enter = projects.clone();
    let filter_label = match f {
        ProjectFilter::All => None,
        ProjectFilter::NoProject => Some(tr("Ohne Projekt")),
        ProjectFilter::Project(p) => projects.iter().find(|(id, _)| *id == p).map(|(_, n)| n.clone()),
    };
    let pending = pending.filter(|p| {
        q.is_empty()
            && match f {
                ProjectFilter::All => true,
                ProjectFilter::NoProject => p.project_id.is_none(),
                ProjectFilter::Project(id) => p.project_id == Some(id),
            }
    });
    let empty = all.is_empty() && pending.is_none();

    rsx! {
        // Kopf: Logo und Einklappen
        // In der macOS-App liegt hier die Titelleiste (Ampel links, Fenster ziehen)
        // Neben der Leiste links (56 px) reichen 24 px bis hinter die Ampel
        div { class: if sheet { "flex h-12 shrink-0 items-center gap-2 px-3 [.nrz-mac_&]:h-14 [.nrz-mac_&]:pb-[calc(56px_-_2*var(--nrz-tl-y,_24px))] [.nrz-mac_&]:pl-[80px]" } else { "flex h-12 shrink-0 items-center gap-2 px-3 [.nrz-mac_&]:h-14 [.nrz-mac_&]:pb-[calc(56px_-_2*var(--nrz-tl-y,_24px))] [.nrz-mac_&]:pl-[24px]" }, "data-drag": "1",
            Link { to: Route::Home {}, class: "flex min-w-0 flex-1 items-center gap-2.5 rounded-lg px-1 py-1 [.nrz-mac_&]:gap-1.5 [.nrz-mac_&]:px-0.5",
                img { src: "/static/icon.png", class: "size-6 rounded-md [.nrz-mac_&]:hidden", alt: "" }
                span { class: "truncate text-[14px] font-semibold tracking-tight", "nrzCode" }
            }
            // macOS: wie in nativen Apps direkt neben der Ampel
            button {
                class: "{SIDE_ICON_BTN} [.nrz-mac_&]:order-first",
                title: tr("Seitenleiste einklappen"),
                onclick: move |_| {
                    if sheet {
                        *MOBILE_NAV.write() = false;
                    } else {
                        toggle(&LEFT, "nrz-left");
                    }
                },
                icons::PanelLeftClose {}
            }
        }
        // Umschalter Code | Bot über der Suche – nur auf dem Handy, sonst in der Leiste links
        if sheet {
            WorkSwitch {}
        }
        if state::WORK() == state::Work::Bot {
            div { class: "flex shrink-0 items-center gap-0.5 px-2 pt-1 pb-2",
                label { class: "relative flex min-w-0 flex-1 items-center",
                    icons::Search { class: "pointer-events-none absolute left-2.5 size-4 text-muted-foreground" }
                    input {
                        class: "h-9 w-full rounded-lg border border-transparent bg-transparent pl-8 pr-2 text-[14px] outline-none placeholder:text-muted-foreground hover:bg-sidenav-accent/40 focus:border-sidenav-border focus:bg-sidenav-accent/50",
                        r#type: "search",
                        placeholder: tr("Bots suchen"),
                        value: "{query}",
                        oninput: move |e| query.set(e.value()),
                        onkeydown: move |e: KeyboardEvent| {
                            if e.key() == Key::Escape {
                                query.set(String::new());
                            }
                        },
                    }
                }
                button { class: SIDE_ICON_BTN, title: tr("Alle Bots"), onclick: move |_| { *MOBILE_NAV.write() = false; nav.push(Route::Bots {}); },
                    icons::LayoutGrid {}
                }
                button { class: SIDE_ICON_BTN, title: tr("Neuer Bot"), onclick: move |_| { *MOBILE_NAV.write() = false; super::bots::new_bot(None); },
                    icons::Plus {}
                }
            }
            super::bots::BotList { query: query() }
        } else {
        // Suche mit Filter, Projekt hinzufügen und neuem Thread (wie T3 Code)
        div { class: "flex shrink-0 items-center gap-0.5 px-2 pt-1 pb-2",
            label { class: "relative flex min-w-0 flex-1 items-center",
                icons::Search { class: "pointer-events-none absolute left-2.5 size-4 text-muted-foreground" }
                input {
                    class: "h-9 w-full rounded-lg border border-transparent bg-transparent pl-8 pr-2 text-[14px] outline-none placeholder:text-muted-foreground hover:bg-sidenav-accent/40 focus:border-sidenav-border focus:bg-sidenav-accent/50",
                    r#type: "search",
                    placeholder: tr("Suchen"),
                    value: "{query}",
                    oninput: move |e| query.set(e.value()),
                    onkeydown: move |e: KeyboardEvent| {
                        if e.key() == Key::Escape {
                            query.set(String::new());
                        }
                    },
                }
            }
            Dropdown {
                align: Align::End,
                class: "max-h-80 w-60 overflow-y-auto",
                trigger_class: if filter_label.is_some() { format!("{SIDE_ICON_BTN} bg-sidenav-accent text-foreground") } else { SIDE_ICON_BTN.to_string() },
                trigger: rsx! { icons::ListFilter {} },
                if projects.len() > 5 {
                    MenuSearch {
                        value: fq,
                        placeholder: tr("Projekt suchen"),
                        onenter: move |_| {
                            if let Some((id, _)) = projects_enter.iter().find(|(_, n)| search_hit(&fq.peek(), &[n])) {
                                filter.set(ProjectFilter::Project(*id));
                            }
                        },
                    }
                }
                MenuLabel { {tr("Threads filtern")} }
                MenuItem { active: f == ProjectFilter::All, onclick: move |_| filter.set(ProjectFilter::All),
                    icons::Layers {}
                    {tr("Alle Projekte")}
                }
                MenuItem { active: f == ProjectFilter::NoProject, onclick: move |_| filter.set(ProjectFilter::NoProject),
                    ProjectBadge { name: None, size: 16 }
                    {tr("Ohne Projekt")}
                }
                if !projects.is_empty() {
                    MenuSeparator {}
                }
                for (id, name) in projects.iter().filter(|(_, n)| search_hit(&fq(), &[n])).cloned() {
                    MenuItem { key: "{id}", active: f == ProjectFilter::Project(id), onclick: move |_| filter.set(ProjectFilter::Project(id)),
                        ProjectBadge { name: Some(name.clone()), size: 16 }
                        span { class: "truncate", "{name}" }
                    }
                }
                MenuSeparator {}
                MenuItem { onclick: move |_| { nav.push(Route::Projects {}); },
                    icons::FolderGit2 {}
                    {tr("Projekte verwalten")}
                }
            }
            button { class: SIDE_ICON_BTN, title: tr("Projekt hinzufügen"), onclick: move |_| *ADD_PROJECT.write() = true,
                icons::FolderPlus {}
            }
        }
        // „Neuer Thread“ als eigene Zeile (wie im Bild); gefiltert: in diesem Projekt
        div { class: "shrink-0 px-2 pb-1",
            button {
                class: "flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-[14px] hover:bg-sidenav-accent/60 [&_svg]:size-4",
                title: tr("Neuer Thread"),
                onclick: move |_| {
                    // Gefiltert: dieses Projekt; sonst bleibt das zuletzt gewählte stehen
                    match filter() {
                        ProjectFilter::Project(p) => *NEW_THREAD_PROJECT.write() = Some(p),
                        ProjectFilter::NoProject => *NEW_THREAD_PROJECT.write() = None,
                        ProjectFilter::All => {}
                    }
                    *MOBILE_NAV.write() = false;
                    nav.push(Route::Home {});
                },
                icons::SquarePen {}
                span { class: "flex-1", {tr("Neuer Thread")} }
            }
        }
        if let Some(label) = filter_label {
            div { class: "mx-3 mb-1.5 flex shrink-0 items-center gap-2 rounded-md bg-sidenav-accent/60 px-2 py-1 text-xs text-muted-foreground",
                icons::ListFilter { class: "size-3.5" }
                span { class: "flex-1 truncate", "{label}" }
                button { class: "hover:text-foreground", title: tr("Filter entfernen"), onclick: move |_| filter.set(ProjectFilter::All),
                    icons::X { class: "size-3.5" }
                }
            }
        }
        // Threads aller Projekte als Karten, darunter die Gruppen
        div { class: "flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden px-2 pb-2 [&>*]:min-w-0",
            if empty {
                div { class: "px-3 py-8 text-center text-sm text-muted-foreground",
                    p { if q.is_empty() { {tr("Noch keine Threads.")} } else { {tr("Keine Treffer.")} } }
                    if q.is_empty() {
                        Link { to: Route::Home {}, class: "mt-3 inline-flex items-center gap-1.5 text-foreground hover:underline",
                            icons::SquarePen { class: "size-4" }
                            {tr("Neuen Thread beginnen")}
                        }
                    }
                }
            }
            // Nach Tagen gruppiert (Angeheftet, Heute, Gestern, Diese Woche, Älter)
            for (label , rows) in day_groups(with_pending(recent, pending)) {
                div { key: "{label}", class: "grid gap-0.5",
                    div { class: "px-2.5 pt-3 pb-1 text-[12.5px] text-muted-foreground/80", "{label}" }
                    for row in rows {
                        match row {
                            Ok(t) => rsx! { ThreadItem { key: "{t.id}", active: current_root == Some(t.id), current, thread: t.clone(), group: recent_ids.clone() } },
                            Err(p) => rsx! { PendingItem { key: "{PENDING_KEY}", pending: p } },
                        }
                    }
                }
            }
            div { class: "min-h-4 flex-1" }
            if !settled.is_empty() {
                GroupHeader {
                    label: tr("Abgelegt"),
                    count: settled.len(),
                    open: settled_open(),
                    ontoggle: move |_| {
                        let v = !settled_open();
                        settled_open.set(v);
                        util::storage_set("nrz-group-settled", if v { "1" } else { "0" });
                    },
                }
                div { class: "grid gap-0.5",
                    for t in settled {
                        if settled_open() || current_root == Some(t.id) {
                            ThreadItem { key: "{t.id}", active: current_root == Some(t.id), current, thread: t.clone(), group: settled_ids.clone() }
                        }
                    }
                }
            }
            if !hidden.is_empty() {
                GroupHeader {
                    label: tr("Ausgeblendet"),
                    count: hidden.len(),
                    open: hidden_open(),
                    ontoggle: move |_| {
                        let v = !hidden_open();
                        hidden_open.set(v);
                        util::storage_set("nrz-group-hidden", if v { "1" } else { "0" });
                    },
                }
                div { class: "grid gap-0.5",
                    for t in hidden {
                        if hidden_open() || current_root == Some(t.id) {
                            ThreadItem { key: "{t.id}", active: current_root == Some(t.id), current, thread: t.clone(), group: hidden_ids.clone() }
                        }
                    }
                }
            }
            // Laufende Agents: schwebende Karte unten über der Liste – bleibt beim Scrollen sichtbar
            if !working.is_empty() {
                div { class: "sticky bottom-0 z-10 mt-2 shrink-0 rounded-xl border border-sidenav-border bg-popover/95 p-1 shadow-[0_-8px_24px_rgb(0_0_0/0.35)] backdrop-blur-md",
                    GroupHeader {
                        label: tr("Arbeitet"),
                        count: working.len(),
                        open: working_open(),
                        ontoggle: move |_| {
                            let v = !working_open();
                            working_open.set(v);
                            util::storage_set("nrz-group-working", if v { "1" } else { "0" });
                        },
                    }
                    div { class: "grid max-h-64 gap-0.5 overflow-y-auto",
                        for t in working {
                            if working_open() || current_root == Some(t.id) {
                                ThreadItem { key: "{t.id}", active: current_root == Some(t.id), current, thread: t.clone(), group: working_ids.clone() }
                            }
                        }
                    }
                }
            }
        }
        }
        // Auf dem Handy unten: „Nach Updates suchen“ (in der App, verschwindet nach der Prüfung) und
        // Einstellungen – sonst stehen beide in der Leiste links
        if sheet {
        div { class: "grid shrink-0 gap-0.5 px-2 pt-1 pb-2",
                super::update::UpdateRow {}
                Link { to: settings_route(), class: FOOT_ROW,
                    icons::Settings {}
                    span { class: "min-w-0 flex-1 truncate", {tr("Einstellungen")} }
                    span { class: "shrink-0 text-xs text-muted-foreground/70", {settings_shortcut()} }
                }
        }
        }
    }
}

#[component]
fn GroupHeader(#[props(into)] label: String, count: usize, open: bool, ontoggle: EventHandler<()>) -> Element {
    rsx! {
        button {
            class: "mt-1 flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-[13px] text-muted-foreground hover:bg-sidenav-accent/60 hover:text-foreground",
            onclick: move |_| ontoggle.call(()),
            span { class: "flex-1", "{label}" }
            span { class: "tabular-nums", "{count}" }
            icons::ChevronRight { class: if open { "size-4 rotate-90 transition-transform duration-200" } else { "size-4 transition-transform duration-200" } }
        }
    }
}

/// Zustand rechts oben in der Karte (wie T3 Code): arbeitet/wartet mit Laufzeit, „Fertig“,
/// Fehler, Rate-Limit, ungelesen – sonst wie lange die letzte Aktivität her ist.
#[component]
fn ThreadState(thread: ThreadSummary) -> Element {
    let t = thread;
    let since = t.run_started_at.unwrap_or(t.last_activity_at);
    rsx! {
        span { class: "flex shrink-0 items-center gap-1 text-[12px] tabular-nums group-hover/thread:invisible",
            match t.status {
                ThreadStatus::Working => rsx! {
                    span { class: "size-3 rounded-full border-2 border-warning border-r-transparent animate-spin" }
                    span { class: "text-foreground/80", {tr("Arbeitet")} " " Elapsed { since } }
                },
                ThreadStatus::Waiting => rsx! {
                    icons::CircleDashed { class: "size-3.5 animate-[spin_4s_linear_infinite]" }
                    span { {tr("Wartet")} " " Elapsed { since } }
                },
                ThreadStatus::Done => rsx! {
                    icons::CircleCheck { class: "size-3.5 text-success" }
                    span { class: "font-medium text-success", {tr("Fertig")} }
                },
                ThreadStatus::Error if t.resume_at.is_some() => rsx! {
                    icons::Hourglass { class: "size-3.5 text-warning" }
                    span { class: "text-warning", {tr("Rate-Limit")} }
                },
                ThreadStatus::Error => rsx! {
                    icons::CircleX { class: "size-3.5 text-destructive" }
                    span { class: "text-destructive", {tr("Fehler")} }
                },
                _ => rsx! {
                    if t.unread {
                        span { class: "size-2 rounded-full bg-info", title: tr("Ungelesen") }
                    }
                    span { class: "text-muted-foreground/80", title: util::datetime(t.last_activity_at), {short_ago(t.last_activity_at)} }
                },
            }
        }
    }
}

/// Zustand als Punkt links vom Titel: arbeitet (drehender Bogen), fertig/ungelesen (grün bzw.
/// blau), Fehler (rot), Rate-Limit (Sanduhr), sonst grau.
#[component]
pub fn StatusDot(thread: ThreadSummary) -> Element {
    let t = thread;
    let (class, title) = match t.status {
        ThreadStatus::Working => ("size-3 rounded-full border-2 border-warning border-r-transparent animate-spin", tr("Arbeitet")),
        ThreadStatus::Waiting => ("size-3 rounded-full border-2 border-info border-r-transparent animate-[spin_2.5s_linear_infinite]", tr("Wartet auf Subagenten")),
        ThreadStatus::Done => ("size-2 rounded-full bg-success", tr("Fertig")),
        ThreadStatus::Error if t.resume_at.is_some() => ("", tr("Rate-Limit – macht automatisch weiter")),
        ThreadStatus::Error => ("size-2 rounded-full bg-destructive", tr("Fehler")),
        _ if t.unread => ("size-2 rounded-full bg-info", tr("Ungelesen")),
        _ => ("size-2 rounded-full bg-muted-foreground/40", String::new()),
    };
    rsx! {
        span { class: "grid size-3.5 shrink-0 place-items-center", title: "{title}",
            if class.is_empty() {
                icons::Hourglass { class: "size-3 text-warning" }
            } else {
                span { class: "{class}" }
            }
        }
    }
}

/// Offenes Kontextmenü eines Threads (Rechtsklick oder „…“) mit Position im Fenster.
static THREAD_MENU: GlobalSignal<Option<(ThreadSummary, f64, f64)>> = Signal::global(|| None);
/// Offenes Kontextmenü eines Projekts (Rechtsklick auf den Projektkopf): Id, Name, Position.
static PROJECT_MENU: GlobalSignal<Option<(i64, String, f64, f64)>> = Signal::global(|| None);
/// Ziehen und Ablegen: gezogener Thread und der, über dem er gerade ist.
static DRAGGING: GlobalSignal<Option<i64>> = Signal::global(|| None);
static DROP_ON: GlobalSignal<Option<i64>> = Signal::global(|| None);

/// Ein Thread in der Seitenleiste: Zustand, Titel, Branch; der offene als Karte mit seinen
/// Subagenten („3 Agenten“, aufklappbar). Rechtsklick oder „…“ öffnet das Menü (Öffnen,
/// Anheften, Ausblenden, Schliessen, Löschen), Ziehen ändert die Reihenfolge innerhalb der
/// Gruppe (`group`: IDs der Gruppe in der angezeigten Reihenfolge).
/// Zeilen einer Gruppe, der startende Thread (Err) dort, wo der echte landen wird: nach den
/// angehefteten.
fn with_pending(list: Vec<ThreadSummary>, pending: Option<state::PendingThread>) -> Vec<Result<ThreadSummary, state::PendingThread>> {
    let at = list.iter().position(|t| !t.pinned).unwrap_or(list.len());
    let mut rows: Vec<Result<ThreadSummary, state::PendingThread>> = list.into_iter().map(Ok).collect();
    if let Some(p) = pending {
        rows.insert(at, Err(p));
    }
    rows
}

/// Schlüssel des Platzhalters in der Liste.
const PENDING_KEY: &str = "pending";

/// Eintrag des startenden Threads (offen, wie ein laufender Thread).
#[component]
fn PendingItem(pending: state::PendingThread) -> Element {
    rsx! {
        div { class: "relative min-w-0 rounded-xl border border-sidenav-border bg-sidenav-accent/70 shadow-sm animate-in",
            div { class: "flex w-full min-w-0 items-start gap-2.5 px-2.5 py-2",
                div { class: "pt-[3px]", icons::LoaderCircle { class: "size-3 animate-spin text-muted-foreground" } }
                div { class: "min-w-0 flex-1",
                    div { class: "flex min-w-0 items-center gap-1.5",
                        span { class: "min-w-0 flex-1 truncate text-[14px] font-medium", "{pending.title}" }
                    }
                    div { class: "mt-0.5 flex min-w-0 items-center gap-1.5 text-[12.5px] text-muted-foreground",
                        span { class: "min-w-0 flex-1 truncate", {tr("Startet …")} }
                        crate::pages::agents::AgentIcon { kind: pending.agent, size: 13 }
                    }
                }
            }
        }
    }
}

#[component]
fn ThreadItem(thread: ThreadSummary, active: bool, current: Option<i64>, group: Vec<i64>) -> Element {
    let t = thread.clone();
    let id = t.id;
    let dragged = DRAGGING();
    let pos = |x: i64| group.iter().position(|g| *g == x);
    // Markierung: beim Ziehen nach unten unter dem Ziel, nach oben darüber
    let marker = match (dragged, DROP_ON() == Some(id)) {
        (Some(d), true) if d != id => match (pos(d), pos(id)) {
            (Some(from), Some(to)) => Some(from < to),
            _ => None,
        },
        _ => None,
    };
    let fade = if dragged == Some(id) { "opacity-40" } else { "" };
    let menu_thread = thread.clone();
    let dots_thread = thread.clone();
    let over_group = group.clone();
    let drop_group = group.clone();
    let mut agents_open = use_signal(|| true);
    let kids: Vec<ThreadSummary> = {
        let all = THREADS.read();
        let mut k: Vec<ThreadSummary> = all.values().filter(|c| c.parent_id == Some(t.id)).cloned().collect();
        k.sort_by_key(|c| c.id);
        k
    };
    let primary = t
        .project_id
        .and_then(|p| PROJECTS.read().get(&p).map(|p| p.default_branch.clone()))
        .is_some_and(|d| !d.is_empty() && d == t.branch);
    let self_open = current == Some(t.id);
    let project = t
        .project_id
        .and_then(|p| PROJECTS.read().get(&p).map(|p| p.name.clone()))
        .or_else(|| t.project_name.clone())
        .unwrap_or_default();
    let card = if active {
        "rounded-lg border border-sidenav-border bg-sidenav-accent/70 shadow-sm"
    } else {
        "rounded-lg border border-transparent hover:bg-sidenav-accent/40"
    };
    let n = kids.len();
    rsx! {
        div {
            class: "group/thread relative min-w-0 {card} {fade}",
            draggable: "true",
            ondragstart: move |e: DragEvent| {
                *THREAD_MENU.write() = None;
                if let Some(dt) = util::drag_event(&e).and_then(|d| d.data_transfer()) {
                    dt.set_effect_allowed("move");
                    // Eigenes Format: Textfelder übernehmen es beim Ablegen nicht als Text
                    let _ = dt.set_data("application/x-nrz-thread", &format!("{}", id));
                }
                *DRAGGING.write() = Some(id);
            },
            ondragend: move |_| {
                *DRAGGING.write() = None;
                *DROP_ON.write() = None;
            },
            ondragover: move |e: DragEvent| {
                if DRAGGING.peek().is_some_and(|d| over_group.contains(&d)) {
                    e.prevent_default();
                    if *DROP_ON.peek() != Some(id) {
                        *DROP_ON.write() = Some(id);
                    }
                }
            },
            ondrop: move |e: DragEvent| {
                e.prevent_default();
                let d = DRAGGING.write().take();
                *DROP_ON.write() = None;
                if let Some(d) = d {
                    reorder(&drop_group, d, id);
                }
            },
            oncontextmenu: move |e: MouseEvent| {
                e.prevent_default();
                let p = e.client_coordinates();
                *THREAD_MENU.write() = Some((menu_thread.clone(), p.x, p.y));
            },
            if marker == Some(false) {
                div { class: "pointer-events-none absolute inset-x-2 -top-px h-0.5 rounded bg-primary" }
            }
            // „…“ für das gleiche Menü (Touch, ohne Rechtsklick)
            button {
                class: "absolute right-1.5 top-1.5 z-10 grid size-6 place-items-center rounded-md bg-sidenav text-muted-foreground opacity-0 hover:bg-sidenav-accent hover:text-foreground focus:opacity-100 group-hover/thread:opacity-100 [@media(hover:none)]:opacity-100",
                title: tr("Mehr"),
                onclick: move |e: MouseEvent| {
                    e.prevent_default();
                    e.stop_propagation();
                    let p = e.client_coordinates();
                    *THREAD_MENU.write() = Some((dots_thread.clone(), p.x, p.y));
                },
                icons::Ellipsis { class: "size-4" }
            }
            // Zwei Zeilen: Agent, Titel und Zustand; darunter Projekt · Branch und Änderungen
            Link { to: Route::ThreadPage { id: t.id }, class: "flex w-full min-w-0 flex-col gap-1 px-2.5 py-2",
                div { class: "flex min-w-0 items-center gap-2",
                    crate::pages::agents::AgentIcon { kind: t.agent, size: 14 }
                    span { class: if self_open || !active { "min-w-0 flex-1 truncate text-[14px]" } else { "min-w-0 flex-1 truncate text-[14px] text-foreground/90" },
                        {thread_title(&t)}
                    }
                    if t.pinned {
                        span { class: "shrink-0 text-muted-foreground", title: tr("Angeheftet"), icons::Pin { class: "size-3.5" } }
                    }
                    if t.read_only {
                        span { class: "shrink-0 text-info", title: tr("Nur lesen"), icons::Eye { class: "size-3.5" } }
                    }
                    ThreadState { thread: t.clone() }
                }
                div { class: "flex min-w-0 items-center gap-1.5 pl-[22px] text-[12.5px] text-muted-foreground",
                    span {
                        class: "flex min-w-0 shrink items-center gap-1.5",
                        oncontextmenu: {
                            let (name, pid) = (project.clone(), t.project_id);
                            move |e: MouseEvent| {
                                if let Some(p) = pid {
                                    e.prevent_default();
                                    e.stop_propagation();
                                    let c = e.client_coordinates();
                                    *THREAD_MENU.write() = None;
                                    *PROJECT_MENU.write() = Some((p, name.clone(), c.x, c.y));
                                }
                            }
                        },
                        ProjectBadge { name: t.project_id.map(|_| project.clone()), size: 14 }
                        span { class: "min-w-0 truncate", if t.project_id.is_some() { "{project}" } else { {tr("Ohne Projekt")} } }
                    }
                    if !t.branch.is_empty() {
                        icons::GitBranch { class: "size-3 shrink-0 opacity-70" }
                        span { class: "min-w-0 flex-1 truncate", title: if primary { tr("Arbeitet direkt auf dem Hauptbranch") } else { t.branch.clone() }, "{t.branch}" }
                    } else {
                        span { class: "flex-1" }
                    }
                    crate::components::common::ChangeStat { thread: t.clone() }
                }
            }
            if active && n > 0 {
                div { class: "px-2 pb-1.5",
                    button {
                        class: "flex w-full items-center gap-1.5 rounded-md px-2 py-1 text-left text-[12.5px] text-muted-foreground hover:text-foreground",
                        onclick: move |_| agents_open.toggle(),
                        span { class: "flex-1",
                            {if n == 1 { tr("1 Agent") } else { trf("{0} Agenten", &[&n]) }}
                            if t.children_running > 0 {
                                span { class: "text-info", " · " {trf("{0} arbeiten", &[&t.children_running])} }
                            }
                        }
                        if agents_open() {
                            icons::ChevronDown { class: "size-3.5" }
                        } else {
                            icons::ChevronRight { class: "size-3.5" }
                        }
                    }
                    if agents_open() {
                        div { class: "grid gap-0.5",
                            for k in kids {
                                Link {
                                    key: "{k.id}",
                                    to: Route::ThreadPage { id: k.id },
                                    class: if current == Some(k.id) { "flex min-w-0 items-center gap-2 rounded-md bg-sidenav-accent px-2 py-1 text-[12.5px]" } else { "flex min-w-0 items-center gap-2 rounded-md px-2 py-1 text-[12.5px] text-muted-foreground hover:bg-sidenav-accent/60 hover:text-foreground" },
                                    match k.status {
                                        ThreadStatus::Working | ThreadStatus::Waiting => rsx! { span { class: "size-3 shrink-0 rounded-full border-2 border-warning border-r-transparent animate-spin" } },
                                        ThreadStatus::Error => rsx! { icons::CircleX { class: "size-3.5 shrink-0 text-destructive" } },
                                        _ => rsx! { icons::CircleCheck { class: "size-3.5 shrink-0 text-success" } },
                                    }
                                    crate::pages::agents::AgentIcon { kind: k.agent, size: 13 }
                                    span { class: "min-w-0 flex-1 truncate", {thread_title(&k)} }
                                    crate::components::common::ChangeStat { thread: k.clone() }
                                    span { class: "shrink-0 text-[11px] tabular-nums text-muted-foreground/80",
                                        if k.status.is_active() {
                                            Elapsed { since: k.run_started_at.unwrap_or(k.last_activity_at) }
                                        } else {
                                            {short_ago(k.last_activity_at)}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if marker == Some(true) {
                div { class: "pointer-events-none absolute inset-x-2 -bottom-px h-0.5 rounded bg-primary" }
            }
        }
    }
}

/// Threads der Liste nach Tagen (lokale Zeit): angeheftete zuerst, dann „Heute“, „Gestern“,
/// „Diese Woche“, „Älter“ – nach dem Platz in der Liste (fester Platz bzw. letzte Aktivität),
/// damit Ziehen und Ablegen und die Gruppen zusammenpassen. Ein neuer Thread zählt zu „Heute“.
fn day_groups(rows: Vec<Result<ThreadSummary, state::PendingThread>>) -> Vec<(String, Vec<Result<ThreadSummary, state::PendingThread>>)> {
    let offset = js_sys::Date::new_0().get_timezone_offset() as i64 * 60;
    let day = |ts: i64| (ts - offset).div_euclid(86_400);
    let today = day(util::now());
    let label = |r: &Result<ThreadSummary, state::PendingThread>| match r {
        Ok(t) if t.pinned => tr("Angeheftet"),
        Ok(t) => match today - day(t.sort_key.unwrap_or(t.last_activity_at)) {
            i64::MIN..=0 => tr("Heute"),
            1 => tr("Gestern"),
            2..=6 => tr("Diese Woche"),
            _ => tr("Älter"),
        },
        Err(_) => tr("Heute"),
    };
    let mut out: Vec<(String, Vec<Result<ThreadSummary, state::PendingThread>>)> = Vec::new();
    for r in rows {
        let l = label(&r);
        match out.iter_mut().find(|(k, _)| *k == l) {
            Some((_, v)) => v.push(r),
            None => out.push((l, vec![r])),
        }
    }
    out
}

/// Gezogenen Thread vor bzw. hinter das Ziel setzen und die Reihenfolge speichern.
fn reorder(group: &[i64], dragged: i64, target: i64) {
    if dragged == target {
        return;
    }
    let mut ids = group.to_vec();
    let (Some(from), Some(to)) = (ids.iter().position(|x| *x == dragged), ids.iter().position(|x| *x == target)) else { return };
    let item = ids.remove(from);
    ids.insert(to, item);
    // Gleich anzeigen (wie der Server: feste Plätze knapp unter dem neuesten)
    {
        let mut all = THREADS.write();
        let top = ids.iter().filter_map(|i| all.get(i).map(|t| t.sort_key.unwrap_or(t.last_activity_at))).max().unwrap_or_else(util::now);
        for (n, i) in ids.iter().enumerate() {
            if let Some(t) = all.get_mut(i) {
                t.sort_key = Some(top - n as i64);
            }
        }
    }
    spawn(async move {
        match api::post::<Vec<ThreadSummary>, _>("/threads/order", &ThreadOrder { ids }).await {
            Ok(list) => list.into_iter().for_each(state::upsert_thread),
            Err(e) => {
                toast_error(e);
                load_threads().await;
            }
        }
    });
}

/// Kontextmenü eines Threads – einmal im Rahmen eingebunden.
#[component]
fn ThreadMenu() -> Element {
    let nav = use_navigator();
    let route = use_route::<Route>();
    let Some((t, x, y)) = THREAD_MENU() else { return rsx! {} };
    let (w, h) = web_sys::window()
        .map(|w| (w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1200.0), w.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(800.0)))
        .unwrap_or((1200.0, 800.0));
    let left = x.min(w - 236.0).max(8.0);
    let top = y.min(h - 400.0).max(8.0);
    let id = t.id;
    let current = matches!(route, Route::ThreadPage { id: c } if c == id);
    let close = || *THREAD_MENU.write() = None;
    let settled = t.status == ThreadStatus::Settled;
    let (pinned, hidden, read_only, freestyle) = (t.pinned, t.hidden, t.read_only, t.freestyle);
    let patch = move |p: ThreadPatch| {
        close();
        spawn(async move {
            match api::patch::<ThreadSummary, _>(&format!("/threads/{id}"), &p).await {
                Ok(s) => state::upsert_thread(s),
                Err(e) => toast_error(e),
            }
        });
    };
    let title = thread_title(&t);
    rsx! {
        div {
            class: "fixed inset-0 z-[55]",
            onclick: move |_| close(),
            oncontextmenu: move |e| {
                e.prevent_default();
                close();
            },
        }
        div {
            class: "fixed z-[56] w-56 overflow-hidden rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg animate-in",
            style: "left:{left}px;top:{top}px",
            role: "menu",
            tabindex: "-1",
            onmounted: move |e| async move {
                let _ = e.set_focus(true).await;
            },
            onkeydown: move |e| {
                if e.key() == Key::Escape {
                    close();
                }
            },
            MenuLabel { span { class: "block truncate", "{title}" } }
            MenuItem {
                onclick: move |_| {
                    close();
                    nav.push(Route::ThreadPage { id });
                },
                icons::MessageSquare {}
                {tr("Öffnen")}
            }
            MenuItem { onclick: move |_| patch(ThreadPatch { pinned: Some(!pinned), ..Default::default() }),
                if pinned {
                    icons::PinOff {}
                    {tr("Lösen")}
                } else {
                    icons::Pin {}
                    {tr("Anheften")}
                }
            }
            MenuItem { onclick: move |_| patch(ThreadPatch { hidden: Some(!hidden), ..Default::default() }),
                if hidden {
                    icons::Eye {}
                    {tr("Wieder einblenden")}
                } else {
                    icons::EyeOff {}
                    {tr("Ausblenden")}
                }
            }
            MenuSeparator {}
            // Modus des Threads – gilt ab dem nächsten Lauf
            MenuItem { onclick: move |_| patch(ThreadPatch { read_only: Some(!read_only), ..Default::default() }),
                icons::Eye {}
                span { class: "flex-1", {tr("Nur lesen")} }
                if read_only {
                    icons::Check { class: "text-muted-foreground" }
                }
            }
            if !read_only {
                MenuItem { onclick: move |_| patch(ThreadPatch { freestyle: Some(!freestyle), ..Default::default() }),
                    icons::Sparkles {}
                    span { class: "flex-1", "FreeStyle" }
                    if freestyle {
                        icons::Check { class: "text-muted-foreground" }
                    }
                }
            }
            MenuItem {
                onclick: move |_| {
                    close();
                    *THREAD_STATS.write() = Some(id);
                },
                icons::ChartColumn {}
                {tr("Statistik")}
            }
            MenuSeparator {}
            MenuItem {
                onclick: move |_| {
                    close();
                    spawn(async move {
                        let path = if settled { format!("/threads/{id}/reopen") } else { format!("/threads/{id}/settle") };
                        match api::action::<ThreadSummary>(&path).await {
                            Ok(s) => state::upsert_thread(s),
                            Err(e) => toast_error(e),
                        }
                    });
                },
                if settled {
                    icons::ArchiveRestore {}
                    {tr("Wieder öffnen")}
                } else {
                    icons::Archive {}
                    {tr("Schliessen (ablegen)")}
                }
            }
            MenuSeparator {}
            MenuItem {
                destructive: true,
                onclick: move |_| {
                    close();
                    spawn(async move {
                        let ask = Ask::new(tr("Thread löschen?"), tr("Der Verlauf dieses Threads und seiner Subagenten wird gelöscht, ein eigener Worktree ebenfalls. Branches und Commits bleiben erhalten."))
                            .confirm(tr("Löschen"))
                            .destructive();
                        if !confirm(ask).await {
                            return;
                        }
                        match api::delete(&format!("/threads/{id}")).await {
                            Ok(()) => {
                                THREADS.write().remove(&id);
                                if current {
                                    nav.push(Route::Home {});
                                }
                                toast(tr("Thread gelöscht."));
                            }
                            Err(e) => toast_error(e),
                        }
                    });
                },
                icons::Trash2 {}
                {tr("Löschen")}
            }
        }
    }
}

/// Thread, dessen Statistik gerade offen ist.
static THREAD_STATS: GlobalSignal<Option<i64>> = Signal::global(|| None);

/// Statistik eines Threads (aus dem Kontextmenü) – einmal im Rahmen eingebunden.
#[component]
fn ThreadStatsDialog() -> Element {
    let mut open = use_signal(|| false);
    let id = THREAD_STATS();
    // Nur schreiben, wenn sich etwas ändert: Jedes `set`/`write` weckt die Leser, auch bei gleichem
    // Wert – sonst lösen sich die beiden Effekte gegenseitig endlos aus (App hing beim Start).
    use_effect(move || {
        let want = THREAD_STATS().is_some();
        if *open.peek() != want {
            open.set(want);
        }
    });
    use_effect(move || {
        if !open() && THREAD_STATS.peek().is_some() {
            *THREAD_STATS.write() = None;
        }
    });
    let Some(id) = id else { return rsx! {} };
    let title = THREADS.peek().get(&id).map(thread_title).unwrap_or_default();
    rsx! {
        Modal { open, title: tr("Statistik"), description: title, class: "max-w-2xl",
            ThreadStatsBody { key: "{id}", id }
        }
    }
}

#[component]
fn ThreadStatsBody(id: i64) -> Element {
    let stats = use_resource(move || async move { api::get::<nrzcode_shared::workspace::ThreadStats>(&format!("/threads/{id}/stats")).await });
    let s = match stats.read().clone() {
        None => return rsx! { Loading {} },
        Some(Err(e)) => return rsx! { Callout { text: e.message, error: true } },
        Some(Ok(s)) => s,
    };
    let n = util::number;
    let subs = s.subagents > 0;
    type Getter = fn(&nrzcode_shared::workspace::TokenSums) -> String;
    let rows: Vec<(String, bool, Getter)> = vec![
        (tr("Läufe"), false, |x| util::number(x.runs)),
        (tr("Eingabe gesamt"), true, |x| util::number(x.input_tokens)),
        (tr("davon neu"), false, |x| util::number(x.fresh_input())),
        (tr("davon Cache gelesen"), false, |x| util::number(x.cache_read_tokens)),
        (tr("davon Cache geschrieben"), false, |x| util::number(x.cache_write_tokens)),
        (tr("Ausgabe gesamt"), true, |x| util::number(x.output_tokens)),
        (tr("davon Denken"), false, |x| util::number(x.reasoning_tokens)),
        (tr("Tokens gesamt"), true, |x| util::number(x.total_tokens())),
        (tr("Kosten"), true, |x| format!("$ {:.4}", x.cost_usd)),
        (tr("Laufzeit"), false, |x| duration(x.seconds)),
    ];
    let th = "px-2 py-1.5 text-right font-medium text-muted-foreground whitespace-nowrap";
    let td = "px-2 py-1.5 text-right tabular-nums whitespace-nowrap";
    rsx! {
        div { class: "grid grid-cols-3 gap-2",
            div { class: "rounded-lg border bg-card px-3 py-2",
                p { class: "text-xs text-muted-foreground", {tr("Nachrichten")} }
                p { class: "mt-0.5 text-lg font-semibold tabular-nums", {n(s.user_messages + s.agent_messages)} }
            }
            div { class: "rounded-lg border bg-card px-3 py-2",
                p { class: "text-xs text-muted-foreground", {tr("Werkzeugaufrufe")} }
                p { class: "mt-0.5 text-lg font-semibold tabular-nums", {n(s.tool_calls)} }
            }
            div { class: "rounded-lg border bg-card px-3 py-2",
                p { class: "text-xs text-muted-foreground", {tr("Subagenten")} }
                p { class: "mt-0.5 text-lg font-semibold tabular-nums", {n(s.subagents)} }
            }
        }
        div { class: "mt-4 overflow-x-auto rounded-lg border",
            table { class: "w-full text-xs",
                thead { class: "border-b bg-muted/40",
                    tr {
                        th { class: "px-2 py-1.5 text-left font-medium text-muted-foreground", "" }
                        th { class: th, {tr("Dieser Thread")} }
                        if subs {
                            th { class: th, {tr("Mit Subagenten")} }
                        }
                    }
                }
                tbody {
                    for (label, strong, get) in rows {
                        tr { class: "border-b last:border-0",
                            td { class: if strong { "px-2 py-1.5 font-medium whitespace-nowrap" } else { "px-2 py-1.5 pl-5 text-muted-foreground whitespace-nowrap" }, "{label}" }
                            td { class: if strong { "{td} font-medium" } else { "{td} text-muted-foreground" }, {get(&s.own)} }
                            if subs {
                                td { class: if strong { "{td} font-medium" } else { "{td} text-muted-foreground" }, {get(&s.with_subagents)} }
                            }
                        }
                    }
                }
            }
        }
        if !s.models.is_empty() {
            p { class: "mt-4 mb-2 text-sm font-medium", {tr("Je Modell")} }
            div { class: "grid gap-1.5",
                for m in s.models.iter().cloned() {
                    div { key: "{m.agent.key()}-{m.model}", class: "flex items-center gap-2 text-xs",
                        crate::pages::agents::AgentIcon { kind: m.agent, size: 14 }
                        span { class: "min-w-0 flex-1 truncate", {crate::pages::agents::model_label(m.agent, &m.model)} }
                        span { class: "tabular-nums text-muted-foreground", {trf("{0} Läufe", &[&n(m.sums.runs)])} }
                        span { class: "w-28 text-right tabular-nums", {trf("{0} Tokens", &[&n(m.sums.total_tokens())])} }
                        span { class: "w-20 text-right tabular-nums text-muted-foreground", {cost(m.sums.cost_usd)} }
                    }
                }
            }
        }
    }
}

/// Kontextmenü eines Projekts in der Seitenleiste – einmal im Rahmen eingebunden.
#[component]
fn ProjectMenu() -> Element {
    let nav = use_navigator();
    let route = use_route::<Route>();
    let Some((id, name, x, y)) = PROJECT_MENU() else { return rsx! {} };
    let (w, h) = web_sys::window()
        .map(|w| (w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1200.0), w.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(800.0)))
        .unwrap_or((1200.0, 800.0));
    let left = x.min(w - 236.0).max(8.0);
    let top = y.min(h - 170.0).max(8.0);
    let close = || *PROJECT_MENU.write() = None;
    // Liegt die offene Seite in diesem Projekt? Dann nach dem Löschen zur Startseite.
    let inside = match route {
        Route::ProjectPage { id: p } => p == id,
        Route::ThreadPage { id: t } => THREADS.peek().get(&t).is_some_and(|t| t.project_id == Some(id)),
        _ => false,
    };
    let label = name.clone();
    rsx! {
        div {
            class: "fixed inset-0 z-[55]",
            onclick: move |_| close(),
            oncontextmenu: move |e| {
                e.prevent_default();
                close();
            },
        }
        div {
            class: "fixed z-[56] w-56 overflow-hidden rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg animate-in",
            style: "left:{left}px;top:{top}px",
            role: "menu",
            tabindex: "-1",
            onmounted: move |e| async move {
                let _ = e.set_focus(true).await;
            },
            onkeydown: move |e| {
                if e.key() == Key::Escape {
                    close();
                }
            },
            MenuLabel { span { class: "block truncate", "{label}" } }
            MenuItem {
                onclick: move |_| {
                    close();
                    *MOBILE_NAV.write() = false;
                    nav.push(Route::ProjectPage { id });
                },
                icons::Folder {}
                {tr("Öffnen")}
            }
            MenuItem {
                onclick: move |_| {
                    close();
                    *NEW_THREAD_PROJECT.write() = Some(id);
                    *MOBILE_NAV.write() = false;
                    nav.push(Route::Home {});
                },
                icons::Plus {}
                {tr("Neuer Thread in diesem Projekt")}
            }
            MenuSeparator {}
            MenuItem {
                destructive: true,
                onclick: move |_| {
                    close();
                    let name = name.clone();
                    spawn(async move {
                        let ask = Ask::new(
                            trf("Projekt „{0}“ löschen?", &[&name]),
                            tr("Alle Threads des Projekts und die Arbeitsordner auf dem Server werden gelöscht, laufende Agenten angehalten. Das Repository in nrzGit bleibt erhalten."),
                        )
                        .confirm(tr("Löschen"))
                        .destructive();
                        if !confirm(ask).await {
                            return;
                        }
                        match api::delete(&format!("/projects/{id}")).await {
                            Ok(()) => {
                                PROJECTS.write().remove(&id);
                                THREADS.write().retain(|_, t| t.project_id != Some(id));
                                if *NEW_THREAD_PROJECT.peek() == Some(id) {
                                    *NEW_THREAD_PROJECT.write() = None;
                                }
                                if inside {
                                    nav.push(Route::Home {});
                                }
                                toast(tr("Projekt gelöscht."));
                            }
                            Err(e) => toast_error(e),
                        }
                    });
                },
                icons::Trash2 {}
                {tr("Projekt löschen")}
            }
        }
    }
}

// ------------------------------------------------------------------ Benutzermenü

/// Sprache oder Thema im Konto speichern.
async fn save_appearance() {
    let Some(me) = ME.peek().clone() else { return };
    let body = Appearance { theme: me.theme.clone(), language: LANG.peek().clone() };
    if let Err(e) = api::put::<Appearance, _>("/me/appearance", &body).await {
        toast_error(e);
    }
}

/// Thema setzen (Browser + Konto).
pub fn choose_theme(theme: &str) {
    state::apply_theme(theme);
    if let Some(me) = ME.write().as_mut() {
        me.theme = theme.to_string();
    }
    spawn(save_appearance());
}

/// Sprache setzen (Browser + Konto).
pub fn choose_language(code: &str) {
    let code = code.to_string();
    spawn(async move {
        i18n::set_language(&code, true).await;
        if let Some(me) = ME.write().as_mut() {
            me.language = code.clone();
        }
        save_appearance().await;
    });
}

pub fn theme_label(t: &str) -> String {
    match t {
        "light" => tr("Hell"),
        "system" => tr("Wie das System"),
        "mondnacht" => "Mondnacht".to_string(),
        _ => tr("Dunkel"),
    }
}

pub fn language_name() -> String {
    let l = LANG.read().clone();
    LANGUAGES.iter().find(|(c, _)| *c == l).map(|(_, n)| n.to_string()).unwrap_or(l)
}

/// Der angemeldete Benutzer als `UserRef` (für das Avatar).
pub fn me_ref() -> Option<UserRef> {
    ME.read().as_ref().map(|m| UserRef { id: m.id, username: m.username.clone(), full_name: m.full_name.clone(), avatar_url: m.avatar_url.clone(), is_org: false })
}

/// Benutzermenü (Profilbild): Modus Chat/Canvas, Profil, Projekte, Agenten, Sprache, Thema,
/// Abmelden. In der Seitenleiste klappt es nach oben auf, im Canvas-Modus (oben rechts) nach unten.
#[component]
pub fn UserMenu(#[props(default = Side::Top)] side: Side, #[props(default = Align::End)] align: Align) -> Element {
    let nav = use_navigator();
    let mut lang_open = use_signal(|| false);
    let mut theme_open = use_signal(|| false);
    let Some(me) = ME.read().clone() else { return rsx! {} };
    let Some(user) = me_ref() else { return rsx! {} };
    let current_theme = me.theme.clone();
    rsx! {
        Dropdown {
            side,
            align,
            class: "w-64",
            trigger_class: "grid size-8 place-items-center rounded-full hover:ring-2 hover:ring-sidenav-accent",
            trigger: rsx! { UserAvatar { user: user.clone(), size: 26 } },
            div { class: "flex items-center gap-2.5 px-2 py-2",
                super::settings::AvatarPicker { size: 36 }
                div { class: "min-w-0",
                    p { class: "truncate text-sm font-medium", "{me.username}" }
                    p { class: "truncate text-xs text-muted-foreground",
                        if me.is_admin { {tr("Administrator")} } else if me.full_name.is_empty() { {tr("Konto")} } else { "{me.full_name}" }
                    }
                }
            }
            ModeSwitch {}
            MenuSeparator {}
            MenuItem { onclick: move |_| { nav.push(settings_route()); },
                icons::Settings {}
                span { class: "flex-1", {tr("Einstellungen")} }
                span { class: "text-xs text-muted-foreground", {settings_shortcut()} }
            }
            MenuItem { onclick: move |_| { nav.push(Route::Settings { section: "usage".into() }); },
                icons::Gauge {}
                {tr("Nutzung")}
            }
            MenuItem { onclick: move |_| { nav.push(Route::Settings { section: "profile".into() }); },
                icons::User {}
                {tr("Profil")}
            }
            MenuItem { onclick: move |_| { nav.push(Route::Projects {}); },
                icons::FolderGit2 {}
                {tr("Projekte")}
            }
            MenuItem { onclick: move |_| { nav.push(Route::Settings { section: "agents".into() }); },
                icons::Bot {}
                {tr("Agenten")}
            }
            if me.is_admin {
                MenuItem { onclick: move |_| { nav.push(Route::Settings { section: "users".into() }); },
                    icons::ShieldCheck {}
                    {tr("Verwaltung")}
                }
            }
            MenuSeparator {}
            MenuItem { onclick: move |_| lang_open.set(true),
                icons::Languages {}
                span { class: "flex-1", {tr("Sprache")} }
                span { class: "text-xs text-muted-foreground", {language_name()} }
            }
            MenuItem { onclick: move |_| theme_open.set(true),
                icons::SunMoon {}
                span { class: "flex-1", {tr("Thema")} }
                span { class: "text-xs text-muted-foreground", {theme_label(&me.theme)} }
            }
            MenuSeparator {}
            MenuItem { onclick: move |_| logout(nav),
                icons::LogOut {}
                {tr("Abmelden")}
            }
        }
        Modal { open: lang_open, title: tr("Sprache"), description: tr("Die Oberfläche erscheint in dieser Sprache."),
            div { class: "grid gap-2",
                for (code, label) in LANGUAGES.iter().copied() {
                    button {
                        class: if *LANG.read() == code { "flex items-center justify-between rounded-lg border border-ring bg-accent px-4 py-2.5 text-sm font-medium" } else { "flex items-center justify-between rounded-lg border px-4 py-2.5 text-sm hover:bg-accent/50" },
                        onclick: move |_| {
                            choose_language(code);
                            lang_open.set(false);
                        },
                        "{label}"
                        if *LANG.read() == code {
                            icons::Check { class: "size-4" }
                        }
                    }
                }
            }
        }
        Modal { open: theme_open, title: tr("Thema"), description: tr("Hell, dunkel, wie das Betriebssystem oder Mondnacht."),
            ThemePicker {
                current: current_theme,
                onpick: move |t: String| {
                    choose_theme(&t);
                    theme_open.set(false);
                },
            }
        }
    }
}

/// Abmelden und zur Anmeldung.
pub fn logout(nav: dioxus::router::Navigator) {
    spawn(async move {
        let _: api::ApiResult<serde_json::Value> = api::action("/auth/logout").await;
        crate::events::disconnect();
        *ME.write() = None;
        THREADS.write().clear();
        PROJECTS.write().clear();
        nav.replace(Route::Login { next: String::new() });
    });
}

/// Umschalter Code | Bot über der Suche: Code = Threads in Projekten, Bot = eigene
/// KI-Teamkollegen mit Gedächtnis (`bots`). Wechselt zur letzten Seite des Bereichs.
#[component]
fn WorkSwitch() -> Element {
    let nav = use_navigator();
    let work = state::WORK();
    let seg = |on: bool| {
        if on {
            "inline-flex h-8 items-center justify-center gap-2 rounded-[10px] bg-sidenav-accent text-[13.5px] font-medium text-foreground shadow-[0_1px_2px_rgb(0_0_0/0.25),inset_0_0_0_1px_var(--color-sidenav-border)] transition-colors [&_svg]:size-4"
        } else {
            "inline-flex h-8 items-center justify-center gap-2 rounded-[10px] text-[13.5px] text-muted-foreground transition-colors hover:text-foreground [&_svg]:size-4"
        }
    };
    let go = move |w: state::Work| {
        *MOBILE_NAV.write() = false;
        if w == *state::WORK.peek() {
            return;
        }
        state::set_work(w);
        let to = if w == state::Work::Bot { super::bots::bot_route() } else { Route::Home {} };
        nav.push(to);
    };
    rsx! {
        div { class: "shrink-0 px-2 pt-1 pb-1.5",
            div { class: "grid grid-cols-2 gap-1 rounded-xl border border-sidenav-border bg-black/15 p-1 dark:bg-black/25", role: "tablist",
                button { class: seg(work == state::Work::Code), role: "tab", "aria-selected": work == state::Work::Code, title: tr("Coding-Agenten in deinen Projekten"), onclick: move |_| go(state::Work::Code),
                    icons::CodeXml {}
                    {tr("Code")}
                }
                button { class: seg(work == state::Work::Bot), role: "tab", "aria-selected": work == state::Work::Bot, title: tr("Eigene Bots mit Gedächtnis und Routinen"), onclick: move |_| go(state::Work::Bot),
                    icons::Bot {}
                    {tr("Bot")}
                }
            }
        }
    }
}

/// Umschalter Chat-Modus / Canvas-Modus (im Benutzermenü und unter Einstellungen →
/// Erscheinungsbild; `label` = Überschrift „Modus“ darüber).
#[component]
pub fn ModeSwitch(#[props(default = true)] label: bool) -> Element {
    let menu = crate::components::overlay::menu_open_signal();
    let route = use_route::<Route>();
    let nav = use_navigator();
    let mode = state::MODE();
    let seg = |on: bool| {
        if on {
            "inline-flex items-center justify-center gap-1.5 rounded-md bg-background px-2 py-1.5 text-[13px] font-medium text-foreground shadow-xs [&_svg]:size-3.5"
        } else {
            "inline-flex items-center justify-center gap-1.5 rounded-md px-2 py-1.5 text-[13px] text-muted-foreground hover:text-foreground [&_svg]:size-3.5"
        }
    };
    let switch = move |target: state::UiMode| {
        if let Some(mut o) = menu {
            o.set(false);
        }
        if target == *state::MODE.peek() {
            return;
        }
        let next = if target == state::UiMode::Canvas { crate::pages::canvas::enter_from(&route) } else { crate::pages::canvas::leave_to() };
        state::set_mode(target);
        nav.push(next);
    };
    rsx! {
        div { class: if label { "px-1 pb-1.5" } else { "" },
            if label {
                p { class: "px-1 pb-1 text-[11px] font-medium text-muted-foreground", {tr("Modus")} }
            }
            div { class: "grid grid-cols-2 gap-1 rounded-lg bg-muted p-1",
                button { class: seg(mode == state::UiMode::Chat), title: tr("Seitenleiste mit Threads, ein Chat im Fokus"), onclick: {
                        let switch = switch.clone();
                        move |_| switch(state::UiMode::Chat)
                    },
                    icons::MessagesSquare {}
                    {tr("Chat")}
                }
                button { class: seg(mode == state::UiMode::Canvas), title: tr("Alle Fenster eines Arbeitsbereichs frei auf einer Fläche"), onclick: move |_| switch(state::UiMode::Canvas),
                    icons::Frame {}
                    {tr("Canvas")}
                }
            }
        }
    }
}

/// Vier Kacheln: dunkel, hell, wie das System, Mondnacht.
#[component]
pub fn ThemePicker(#[props(into)] current: String, onpick: EventHandler<String>) -> Element {
    rsx! {
        div { class: "grid grid-cols-2 gap-2 sm:grid-cols-4",
            for (t, icon) in [("dark", rsx! { icons::Moon {} }), ("light", rsx! { icons::Sun {} }), ("system", rsx! { icons::Monitor {} }), ("mondnacht", rsx! { icons::Sparkles {} })] {
                button {
                    class: if current == t { "flex flex-col items-center gap-2 rounded-lg border border-ring bg-accent px-3 py-4 text-sm font-medium [&_svg]:size-5" } else { "flex flex-col items-center gap-2 rounded-lg border px-3 py-4 text-sm hover:bg-accent/50 [&_svg]:size-5" },
                    onclick: move |_| onpick.call(t.to_string()),
                    {icon}
                    {theme_label(t)}
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Statistik

/// Dauer „3 h 12 min“, „4 min 5 s“.
fn duration(secs: i64) -> String {
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    if h > 0 {
        format!("{h} h {m} min")
    } else if m > 0 {
        format!("{m} min {s} s")
    } else {
        format!("{s} s")
    }
}

fn cost(c: f64) -> String {
    format!("$ {c:.4}")
}

/// Genaue Zahlen (unter Einstellungen → Nutzung).
#[component]
pub fn StatsBody() -> Element {
    let stats = use_resource(|| async { api::get::<Stats>(&format!("/me/stats?tz={}", crate::pages::agents::tz_offset())).await });
    let s = match stats.read().clone() {
        None => return rsx! { Loading {} },
        Some(Err(e)) => return rsx! { Callout { text: e.message, error: true } },
        Some(Ok(s)) => s,
    };
    let n = util::number;
    let tile = |label: String, value: String, sub: String| {
        rsx! {
            div { class: "rounded-lg border bg-card px-4 py-3",
                p { class: "text-xs text-muted-foreground", "{label}" }
                p { class: "mt-1 text-xl font-semibold tabular-nums", "{value}" }
                if !sub.is_empty() {
                    p { class: "mt-0.5 text-[11px] text-muted-foreground tabular-nums", "{sub}" }
                }
            }
        }
    };
    let t = s.total.clone();
    let avg = if t.runs > 0 { t.seconds / t.runs } else { 0 };
    // Token-Tabelle: Zeilen × Zeiträume
    let win = |key: &str| s.windows.iter().find(|w| w.key == key).map(|w| w.sums.clone()).unwrap_or_default();
    let cols = [win("today"), win("7d"), win("30d"), win("all")];
    type Getter = fn(&nrzcode_shared::workspace::TokenSums) -> String;
    let rows: Vec<(String, bool, Getter)> = vec![
        (tr("Läufe"), false, |x| util::number(x.runs)),
        (tr("Eingabe gesamt"), true, |x| util::number(x.input_tokens)),
        (tr("davon neu"), false, |x| util::number(x.fresh_input())),
        (tr("davon Cache gelesen"), false, |x| util::number(x.cache_read_tokens)),
        (tr("davon Cache geschrieben"), false, |x| util::number(x.cache_write_tokens)),
        (tr("Ausgabe gesamt"), true, |x| util::number(x.output_tokens)),
        (tr("davon Denken"), false, |x| util::number(x.reasoning_tokens)),
        (tr("Tokens gesamt"), true, |x| util::number(x.total_tokens())),
        (tr("Kosten"), true, |x| format!("$ {:.4}", x.cost_usd)),
        (tr("Laufzeit"), false, |x| duration(x.seconds)),
    ];
    let th = "px-2 py-1.5 text-right font-medium text-muted-foreground whitespace-nowrap";
    let td = "px-2 py-1.5 text-right tabular-nums whitespace-nowrap";
    rsx! {
        div { class: "grid grid-cols-2 gap-2 sm:grid-cols-4",
            {tile(tr("Projekte"), n(s.projects), String::new())}
            {tile(tr("Threads"), n(s.threads), trf("{0} gerade aktiv", &[&n(s.threads_active)]))}
            {tile(tr("Subagenten"), n(s.subagents), String::new())}
            {tile(tr("Läufe"), n(t.runs), trf("{0} fertig · {1} Fehler · {2} gestoppt", &[&n(s.runs_done), &n(s.runs_error), &n(s.runs_stopped)]))}
            {tile(tr("Nachrichten"), n(s.user_messages + s.agent_messages), trf("{0} von dir · {1} vom Agenten", &[&n(s.user_messages), &n(s.agent_messages)]))}
            {tile(tr("Werkzeugaufrufe"), n(s.tool_calls), String::new())}
            {tile(tr("Laufzeit"), duration(t.seconds), trf("Ø {0} je Lauf", &[&duration(avg)]))}
            {tile(tr("Kosten"), cost(t.cost_usd), trf("{0} Läufe heute", &[&n(s.runs_today)]))}
        }
        p { class: "mt-5 mb-2 text-sm font-medium", {tr("Tokens")} }
        div { class: "overflow-x-auto rounded-lg border",
            table { class: "w-full text-xs",
                thead { class: "border-b bg-muted/40",
                    tr {
                        th { class: "px-2 py-1.5 text-left font-medium text-muted-foreground", "" }
                        th { class: th, {tr("Heute")} }
                        th { class: th, {tr("7 Tage")} }
                        th { class: th, {tr("30 Tage")} }
                        th { class: th, {tr("Gesamt")} }
                    }
                }
                tbody {
                    for (label, strong, get) in rows {
                        tr { class: "border-b last:border-0",
                            td { class: if strong { "px-2 py-1.5 font-medium whitespace-nowrap" } else { "px-2 py-1.5 pl-5 text-muted-foreground whitespace-nowrap" }, "{label}" }
                            for c in cols.iter() {
                                td { class: if strong { "{td} font-medium" } else { "{td} text-muted-foreground" }, {get(c)} }
                            }
                        }
                    }
                }
            }
        }
        p { class: "mt-1.5 text-[11px] text-muted-foreground",
            {tr("Eingabe enthält Tokens aus dem Cache, Ausgabe die Denk-Tokens. Kosten melden nur Claude und OpenCode; Cache- und Denk-Tokens werden erst ab dieser Version gezählt.")}
        }
        if !s.agents.is_empty() {
            p { class: "mt-5 mb-2 text-sm font-medium", {tr("Je Agent")} }
            div { class: "overflow-x-auto rounded-lg border",
                table { class: "w-full text-xs",
                    thead { class: "border-b bg-muted/40",
                        tr {
                            th { class: "px-2 py-1.5 text-left font-medium text-muted-foreground", {tr("Agent")} }
                            th { class: th, {tr("Threads")} }
                            th { class: th, {tr("Läufe")} }
                            th { class: th, {tr("Eingabe")} }
                            th { class: th, {tr("Cache gelesen")} }
                            th { class: th, {tr("Ausgabe")} }
                            th { class: th, {tr("Denken")} }
                            th { class: th, {tr("Kosten")} }
                            th { class: th, {tr("Laufzeit")} }
                        }
                    }
                    tbody {
                        for a in s.agents.iter().cloned() {
                            tr { key: "{a.agent.key()}", class: "border-b last:border-0",
                                td { class: "px-2 py-1.5",
                                    div { class: "flex items-center gap-2 whitespace-nowrap",
                                        crate::pages::agents::AgentIcon { kind: a.agent, size: 14 }
                                        {a.agent.name()}
                                    }
                                }
                                td { class: td, {n(a.threads)} }
                                td { class: td, {n(a.sums.runs)} }
                                td { class: td, {n(a.sums.input_tokens)} }
                                td { class: td, {n(a.sums.cache_read_tokens)} }
                                td { class: td, {n(a.sums.output_tokens)} }
                                td { class: td, {n(a.sums.reasoning_tokens)} }
                                td { class: td, {cost(a.sums.cost_usd)} }
                                td { class: td, {duration(a.sums.seconds)} }
                            }
                        }
                    }
                }
            }
        }
        if !s.models.is_empty() {
            p { class: "mt-5 mb-2 text-sm font-medium", {tr("Je Modell")} }
            div { class: "overflow-x-auto rounded-lg border",
                table { class: "w-full text-xs",
                    thead { class: "border-b bg-muted/40",
                        tr {
                            th { class: "px-2 py-1.5 text-left font-medium text-muted-foreground", {tr("Modell")} }
                            th { class: th, {tr("Läufe")} }
                            th { class: th, {tr("Eingabe")} }
                            th { class: th, {tr("Cache gelesen")} }
                            th { class: th, {tr("Ausgabe")} }
                            th { class: th, {tr("Kosten")} }
                            th { class: th, {tr("Ø Tokens/Lauf")} }
                        }
                    }
                    tbody {
                        for m in s.models.iter().cloned() {
                            tr { key: "{m.agent.key()}-{m.model}", class: "border-b last:border-0",
                                td { class: "px-2 py-1.5",
                                    div { class: "flex items-center gap-2 whitespace-nowrap",
                                        crate::pages::agents::AgentIcon { kind: m.agent, size: 14 }
                                        {crate::pages::agents::model_label(m.agent, &m.model)}
                                    }
                                }
                                td { class: td, {n(m.sums.runs)} }
                                td { class: td, {n(m.sums.input_tokens)} }
                                td { class: td, {n(m.sums.cache_read_tokens)} }
                                td { class: td, {n(m.sums.output_tokens)} }
                                td { class: td, {cost(m.sums.cost_usd)} }
                                td { class: td, {n(if m.sums.runs > 0 { m.sums.total_tokens() / m.sums.runs } else { 0 })} }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Kopfzeile

const TOP_BTN: &str = "grid size-8 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground disabled:pointer-events-none disabled:opacity-30 [&_svg]:size-4";

#[component]
fn TopBar() -> Element {
    let route = use_route::<Route>();
    let thread = match route {
        Route::ThreadPage { id } => Some(id),
        _ => None,
    };
    let thread_page = thread.is_some();
    let active = |on: bool| if on { "bg-accent text-foreground" } else { "" };
    let right_on = thread_page && if narrow() { MOBILE_RIGHT() } else { right_open() };
    rsx! {
        // Titel und Knöpfe mittig in der Leiste (auch am Mac nicht mehr oben an der Ampel) –
        // mit und ohne Seitenleiste auf derselben Höhe; in der Canvas steht die Blase gleich hoch
        header {
            class: if LEFT() { "flex h-14 shrink-0 items-center gap-2 border-b px-3 md:px-4 [.nrz-win_&]:pr-[150px]" } else { "flex h-14 shrink-0 items-center gap-2 border-b px-3 md:px-4 [.nrz-win_&]:pr-[150px] [.nrz-mac_&]:pl-[80px] md:[.nrz-mac_&]:pl-[24px]" },
            "data-drag": "1",
            button { class: "{TOP_BTN} md:hidden", title: tr("Menü"), onclick: move |_| *MOBILE_NAV.write() = true,
                icons::Menu {}
            }
            if !LEFT() {
                // Neben der Ampel und auf ihrer Höhe (wie in der Seitenleiste)
                button { class: "{TOP_BTN} hidden md:grid [.nrz-mac_&]:translate-y-[calc(var(--nrz-tl-y,_24px)_-_28px)]", title: tr("Seitenleiste zeigen"), onclick: move |_| toggle(&LEFT, "nrz-left"),
                    icons::PanelLeft {}
                }
            }
            div { class: "flex min-w-0 flex-1 items-center gap-2",
                div { class: "min-w-0 translate-y-0.5", Breadcrumb { route: route.clone() } }
                // Hostet der Chat eine Seite (Server im Arbeitsordner): Browser-Knopf
                if let Some(id) = thread {
                    BrowserButton { key: "{id}", thread_id: id }
                }
            }
            // Rechts; in der Canvas stehen dieselben Knöpfe an derselben Stelle in einer Blase
            // (`canvas::shell::RightBubble`)
            div {
                class: "flex shrink-0 items-center gap-0.5",
                if BOTTOM_FEATURE() {
                    button {
                        class: "{TOP_BTN} {active(thread_page && BOTTOM())}",
                        disabled: !thread_page,
                        title: tr("Terminal ein-/ausblenden"),
                        onclick: move |_| toggle(&BOTTOM, "nrz-bottom"),
                        icons::PanelBottom {}
                    }
                }
                if RIGHT_FEATURE() {
                    button {
                        class: "{TOP_BTN} {active(right_on)}",
                        disabled: !thread_page,
                        title: tr("Rechte Spalte ein-/ausblenden"),
                        onclick: move |_| {
                            if narrow() {
                                let v = !MOBILE_RIGHT();
                                *MOBILE_RIGHT.write() = v;
                            } else {
                                close_or_open_right();
                            }
                        },
                        icons::PanelRight {}
                    }
                }
                // Natives CLI des Agenten statt des Chats
                super::cli::CliButton { class: TOP_BTN }
            }
        }
    }
}

/// Rechte Spalte umschalten (auch eine vorübergehend geöffnete schliessen).
fn close_or_open_right() {
    let open = (*RIGHT_FEATURE.peek() && *RIGHT.peek()) || *RIGHT_TEMP.peek();
    if *RIGHT_TEMP.peek() {
        *RIGHT_TEMP.write() = false;
    }
    if open == *RIGHT.peek() {
        toggle(&RIGHT, "nrz-right");
    }
}

/// Browser-Knopf neben dem Titel, sobald im Arbeitsordner des Threads ein Server lauscht (alle
/// 15 Sekunden nachgesehen): öffnet die Seite im Paneel neben dem Chat (über `state::PREVIEW`).
#[component]
fn BrowserButton(thread_id: i64) -> Element {
    let mut tick = use_signal(|| 0u64);
    let ports = use_resource(move || async move {
        let _ = tick();
        api::get::<Vec<nrzcode_shared::workspace::PreviewPort>>(&format!("/previews?threadId={thread_id}")).await.unwrap_or_default()
    });
    use_future(move || async move {
        loop {
            gloo_timers::future::TimeoutFuture::new(15_000).await;
            tick += 1;
        }
    });
    let mine: Vec<u16> = ports.read().clone().unwrap_or_default().into_iter().filter(|p| p.this_thread).map(|p| p.port).collect();
    let Some(&port) = mine.first() else { return rsx! {} };
    let title = if mine.len() > 1 { trf("Browser: Seite des Chats öffnen (:{0}, {1} Server)", &[&port, &mine.len()]) } else { trf("Browser: Seite des Chats öffnen (:{0})", &[&port]) };
    rsx! {
        button {
            class: "flex h-7 shrink-0 items-center gap-1.5 rounded-full border px-2.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground",
            title: "{title}",
            onclick: move |_| *state::PREVIEW.write() = Some((thread_id, port, "/".to_string())),
            icons::Globe { class: "size-3.5" }
            {tr("Browser")}
            span { class: "font-mono tabular-nums text-muted-foreground/70", ":{port}" }
        }
    }
}

/// „Projekt / Thread-Titel“ bzw. der Name der Seite.
#[component]
fn Breadcrumb(route: Route) -> Element {
    let sep = rsx! { span { class: "text-muted-foreground/60", "/" } };
    match route {
        Route::ThreadPage { id } => {
            let t = THREADS.read().get(&id).cloned();
            let title = t.as_ref().map(thread_title).unwrap_or_else(|| trf("Thread #{0}", &[&id]));
            let project = t.as_ref().and_then(|t| t.project_id.zip(t.project_name.clone()));
            rsx! {
                div { class: "flex min-w-0 items-center gap-2 text-[15px]",
                    if let Some((pid, name)) = project {
                        Link { to: Route::ProjectPage { id: pid }, class: "hidden min-w-0 shrink items-center gap-2 text-muted-foreground hover:text-foreground sm:flex",
                            ProjectBadge { name: Some(name.clone()), size: 18 }
                            span { class: "truncate", "{name}" }
                        }
                        span { class: "hidden sm:inline", {sep} }
                    }
                    span { class: "truncate font-semibold", "{title}" }
                }
            }
        }
        Route::ProjectPage { id } => {
            let name = PROJECTS.read().get(&id).map(|p| p.name.clone()).unwrap_or_default();
            rsx! {
                div { class: "flex min-w-0 items-center gap-2 text-[15px]",
                    Link { to: Route::Projects {}, class: "text-muted-foreground hover:text-foreground", {tr("Projekte")} }
                    {sep}
                    ProjectBadge { name: Some(name.clone()), size: 18 }
                    span { class: "truncate font-semibold", "{name}" }
                }
            }
        }
        other => {
            let label = match other {
                Route::Home {} => tr("Neuer Thread"),
                Route::Grid { .. } => tr("Parallele Sitzungen"),
                Route::Bots {} => tr("Bots"),
                Route::BotPage { id } => state::BOTS.read().as_ref().and_then(|l| l.iter().find(|b| b.id == id).map(|b| b.name.clone())).unwrap_or_else(|| tr("Bot")),
                Route::Projects {} => tr("Projekte"),
                Route::Settings { .. } => tr("Einstellungen"),
                _ => String::new(),
            };
            rsx! { span { class: "truncate text-[15px] font-semibold", "{label}" } }
        }
    }
}
