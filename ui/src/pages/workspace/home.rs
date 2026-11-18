//! Startseite: Begrüssung, Projekt wählen (oder ohne Projekt), Composer für einen neuen Thread,
//! darunter die letzten Threads und Projekte. Ohne nrzGit-Verbindung steht oben eine deutliche
//! Karte „Mit nrzGit verbinden“.

use super::parts::*;
use crate::Route;
use crate::components::overlay::*;
use crate::i18n::{tr, trf};
use crate::state::{ME, PROJECTS, THREADS};
use dioxus::prelude::*;
use nrzcode_shared::{ProjectSummary, ThreadSummary};

/// Begrüssung nach Tageszeit.
fn greeting(name: &str) -> String {
    let hour = js_sys::Date::new_0().get_hours();
    let key = match hour {
        5..=10 => "Guten Morgen, {0}",
        11..=17 => "Hallo, {0}",
        _ => "Guten Abend, {0}",
    };
    trf(key, &[&name])
}

#[component]
pub fn Home() -> Element {
    let me = ME.read().clone();
    let name = me.as_ref().map(|m| m.full_name.split_whitespace().next().unwrap_or(&m.username).to_string()).unwrap_or_default();
    let connected = me.as_ref().is_some_and(|m| m.nrzgit_connected);
    let mut projects: Vec<ProjectSummary> = PROJECTS.read().values().cloned().collect();
    projects.sort_by_key(|x| std::cmp::Reverse(x.last_activity_at));
    let selected = NEW_THREAD_PROJECT().filter(|id| projects.iter().any(|p| p.id == *id));
    let current = selected.and_then(|id| projects.iter().find(|p| p.id == id).cloned());
    let mut recent: Vec<ThreadSummary> = THREADS.read().values().filter(|t| t.parent_id.is_none() && t.bot_id.is_none()).cloned().collect();
    recent.sort_by_key(|x| std::cmp::Reverse(x.last_activity_at));
    recent.truncate(6);
    let nav = use_navigator();
    // Anzahl Chats im neuen Arbeitsbereich (1 = normaler neuer Thread)
    let mut chats = use_signal(|| 1usize);
    // Suche in der Projektauswahl (Name oder Repository)
    let pq = use_signal(String::new);
    let found: Vec<ProjectSummary> = projects.iter().filter(|p| search_hit(&pq(), &[&p.name, &p.repo])).cloned().collect();
    let first_found = found.first().map(|p| p.id);
    let not_ready = current.as_ref().is_some_and(|p| p.status != "ready");

    // Gerade gesendeter neuer Thread: schon wie der Chat zeigen. Die Startseite bleibt darunter
    // eingehängt (ihr Composer schliesst das Anlegen ab und wechselt dann in den Thread).
    // Frisch geöffnet: ein Platzhalter von vorhin ist verwaist (sein Composer ist weg)
    use_hook(|| {
        if crate::state::PENDING_THREAD.peek().is_some() {
            spawn(async { *crate::state::PENDING_THREAD.write() = None });
        }
    });
    let pending = crate::state::PENDING_THREAD();
    rsx! {
        if let Some(p) = pending.clone() {
            // Wie die Thread-Seite (mit der rechten Spalte, falls offen): kein Springen beim Wechsel
            div { class: "flex h-full min-h-0",
                div { class: "h-full min-w-0 flex-1", crate::pages::agents::PendingChat { pending: p } }
                if super::parts::right_open() {
                    aside { class: "hidden w-[21rem] shrink-0 border-l bg-sidenav lg:flex" }
                }
            }
        }
        div { class: if pending.is_some() { "hidden" } else { "mx-auto flex w-full max-w-3xl flex-col gap-8 px-4 py-8" },
            if !connected {
                ConnectCard {}
            }
            // Begrüssung und Composer mittig im sichtbaren Bereich, Letzte Threads und Projekte darunter
            // (mit der Karte „verbinden“ darüber entsprechend niedriger – sonst rutschen Begrüssung
            // und Composer um die Höhe der Karte aus der Mitte nach unten)
            div { class: if connected { "flex min-h-[calc(100dvh-14rem)] flex-col justify-center gap-8" } else { "flex min-h-[calc(100dvh-25rem)] flex-col justify-center gap-8" },
                div { class: "text-center",
                    h1 { class: "text-2xl font-semibold tracking-tight md:text-3xl", {greeting(&name)} }
                    p { class: "mt-2 text-sm text-muted-foreground", {tr("Woran sollen die Agenten heute arbeiten?")} }
                }
                div { class: "grid gap-3",
                    div { class: "flex flex-wrap items-center gap-2",
                        Dropdown {
                            class: "max-h-80 w-72 overflow-y-auto",
                            trigger_class: "inline-flex h-8 max-w-full items-center gap-2 rounded-md border bg-card px-2.5 text-sm hover:bg-accent",
                            trigger: rsx! {
                                if let Some(p) = current.clone() {
                                    ProjectBadge { name: Some(p.name.clone()), size: 18 }
                                    span { class: "truncate", "{p.name}" }
                                } else {
                                    ProjectBadge { name: None, size: 18 }
                                    span { {tr("Ohne Projekt")} }
                                }
                                icons::ChevronDown { class: "size-3.5 text-muted-foreground" }
                            },
                            if !projects.is_empty() {
                                MenuSearch {
                                    value: pq,
                                    placeholder: tr("Projekt suchen"),
                                    onenter: move |_| {
                                        if let Some(id) = first_found {
                                            *NEW_THREAD_PROJECT.write() = Some(id);
                                        }
                                    },
                                }
                            }
                            MenuLabel { {tr("Projekt für den neuen Thread")} }
                            if pq().trim().is_empty() {
                                MenuItem { active: selected.is_none(), onclick: move |_| *NEW_THREAD_PROJECT.write() = None,
                                    ProjectBadge { name: None, size: 16 }
                                    span { class: "flex-1", {tr("Ohne Projekt")} }
                                    span { class: "text-xs text-muted-foreground", {tr("Scratch-Ordner")} }
                                }
                                if !projects.is_empty() {
                                    MenuSeparator {}
                                }
                            } else if found.is_empty() {
                                p { class: "px-2 py-1.5 text-sm text-muted-foreground", {tr("Kein Projekt gefunden.")} }
                            }
                            for p in found.clone() {
                                MenuItem { key: "{p.id}", active: selected == Some(p.id), onclick: move |_| *NEW_THREAD_PROJECT.write() = Some(p.id),
                                    ProjectBadge { name: Some(p.name.clone()), size: 16 }
                                    span { class: "min-w-0 flex-1 truncate", "{p.name}" }
                                    ProjectStatus { status: p.status.clone() }
                                }
                            }
                            MenuSeparator {}
                            MenuItem { onclick: move |_| *ADD_PROJECT.write() = true,
                                icons::FolderPlus {}
                                {tr("Projekt hinzufügen …")}
                            }
                        }
                        if let Some(p) = current.clone() {
                            if !p.repo.is_empty() {
                                span { class: "truncate text-xs text-muted-foreground", "{p.repo} · {p.default_branch}" }
                            }
                        }
                    }
                    // Mehrere Chats in einem Arbeitsbereich: Anzahl wählen, „Fertig“ öffnet das
                    // Raster mit so vielen leeren Chats (jeder bekommt seine eigene Eingabe)
                    div { class: "flex flex-wrap items-center gap-2",
                        div { class: "flex h-8 items-center gap-0.5 rounded-full border px-1",
                            title: tr("Mehrere Chats nebeneinander in einem Arbeitsbereich (bei 4: 2×2)"),
                            icons::LayoutGrid { class: "ml-1 mr-0.5 size-3.5 text-muted-foreground" }
                            for n in 1..=6usize {
                                button {
                                    key: "{n}",
                                    class: if n == chats() { "grid size-6 place-items-center rounded-full bg-primary text-[11px] font-semibold text-primary-foreground" } else { "grid size-6 place-items-center rounded-full text-[11px] text-muted-foreground hover:bg-accent hover:text-foreground" },
                                    onclick: move |_| chats.set(n),
                                    "{n}"
                                }
                            }
                        }
                        if chats() > 1 {
                            button {
                                class: "inline-flex h-8 items-center gap-1.5 rounded-full bg-primary px-3 text-xs font-medium text-primary-foreground hover:bg-primary/90",
                                onclick: move |_| {
                                    let ids: Vec<i64> = (1..=chats() as i64).map(|n| -n).collect();
                                    nav.push(Route::Grid { ids: super::grid::join(&ids) });
                                },
                                icons::Check { class: "size-3.5" }
                                {trf("Fertig – {0} Chats öffnen", &[&chats()])}
                            }
                        }
                    }
                    if chats() == 1 && not_ready {
                        p { class: "text-xs text-muted-foreground", {tr("Das Projekt wird noch geklont – der Agent startet, sobald es bereit ist.")} }
                    }
                    if chats() == 1 {
                    for sel in std::iter::once(selected) {
                        crate::pages::agents::Composer { key: "{sel:?}", project_id: sel, thread_id: None }
                    }
                    }
                }
            }
            if !recent.is_empty() {
                section {
                    div { class: "mb-2 flex items-center justify-between",
                        h2 { class: "text-sm font-medium text-muted-foreground", {tr("Letzte Threads")} }
                    }
                    div { class: "grid gap-1.5 sm:grid-cols-2",
                        for t in recent {
                            Link { key: "{t.id}", to: Route::ThreadPage { id: t.id }, class: "flex min-w-0 flex-col gap-1 rounded-xl border bg-card px-3.5 py-3 hover:bg-accent/40",
                                div { class: "flex items-center gap-2 text-xs text-muted-foreground",
                                    ProjectBadge { name: t.project_name.clone(), size: 16 }
                                    span { class: "min-w-0 flex-1 truncate", {t.project_name.clone().unwrap_or_else(|| tr("Ohne Projekt"))} }
                                    ThreadState { thread: t.clone() }
                                }
                                div { class: "flex items-center gap-2",
                                    span { class: "min-w-0 flex-1 truncate text-sm font-medium", {thread_title(&t)} }
                                    crate::pages::agents::AgentIcon { kind: t.agent, size: 14 }
                                }
                            }
                        }
                    }
                }
            }
            section {
                div { class: "mb-2 flex items-center justify-between",
                    h2 { class: "text-sm font-medium text-muted-foreground", {tr("Projekte")} }
                    div { class: "flex items-center gap-3 text-sm",
                        button { class: "text-muted-foreground hover:text-foreground", onclick: move |_| *ADD_PROJECT.write() = true, {tr("Hinzufügen")} }
                        Link { to: Route::Projects {}, class: "text-muted-foreground hover:text-foreground", {tr("Alle")} }
                    }
                }
                if projects.is_empty() {
                    button {
                        class: "flex w-full items-center justify-center gap-2 rounded-xl border border-dashed px-4 py-8 text-sm text-muted-foreground hover:bg-accent/30 hover:text-foreground",
                        onclick: move |_| *ADD_PROJECT.write() = true,
                        icons::FolderPlus { class: "size-4" }
                        {tr("Erstes Projekt hinzufügen")}
                    }
                } else {
                    div { class: "grid gap-1.5 sm:grid-cols-3",
                        for p in projects.into_iter().take(6) {
                            Link { key: "{p.id}", to: Route::ProjectPage { id: p.id }, class: "flex min-w-0 items-center gap-2.5 rounded-xl border bg-card px-3 py-2.5 hover:bg-accent/40",
                                ProjectBadge { name: Some(p.name.clone()), size: 22 }
                                div { class: "min-w-0 flex-1",
                                    p { class: "truncate text-sm font-medium", "{p.name}" }
                                    p { class: "truncate text-xs text-muted-foreground",
                                        if p.threads == 1 { {tr("1 Thread")} } else { {trf("{0} Threads", &[&p.threads])} }
                                    }
                                }
                                ProjectStatus { status: p.status.clone() }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Deutlicher Hinweis: ohne nrzGit keine Repositories, kein Push, keine Pull Requests.
#[component]
pub fn ConnectCard() -> Element {
    rsx! {
        div { class: "flex flex-col gap-4 rounded-xl border border-info/40 bg-info/10 p-5 sm:flex-row sm:items-center",
            div { class: "grid size-10 shrink-0 place-items-center rounded-lg bg-info/20 text-info", icons::GitPullRequest { class: "size-5" } }
            div { class: "min-w-0 flex-1",
                p { class: "font-medium", {tr("Mit nrzGit, luGit oder GitHub verbinden")} }
                p { class: "mt-1 text-sm text-muted-foreground",
                    {tr("Mit einem Token aus nrzGit, luGit oder GitHub übernimmst du Repositories als Projekte – und alle Agenten können pushen, Pull Requests und Releases anlegen und Images bauen.")}
                }
            }
            Link { to: Route::Settings { section: "nrzgit".into() }, class: "{crate::components::common::btn(crate::components::common::BTN_PRIMARY)} shrink-0",
                icons::Link {}
                {tr("Verbinden")}
            }
        }
    }
}
