//! Bot-Modus (Umschalter „Code | Bot“ über der Suche): eigene KI-Teamkollegen wie Grok Bot.
//! Jeder Bot hat Persönlichkeit, ein Gedächtnis, das er selbst pflegt, einen eigenen
//! Arbeitsordner und Routinen. Ein Gespräch ist ein Thread mit `bot_id` – die Unterhaltung zeigt
//! `agents::ChatView` (mit Composer) wie bei Code-Threads.
//!
//! * `Bots` – Übersicht mit Karten und Vorlagen
//! * `BotPage` – Gespräch mit einem Bot (Kopf mit neuem Gespräch, früheren Gesprächen, Bearbeiten)
//! * `BotList` – Liste in der Seitenleiste
//! * `BotEditor` – Dialog: Profil, Gedächtnis, Routinen

use super::parts::*;
use crate::Route;
use crate::api;
use crate::components::common::*;
use crate::components::overlay::*;
use crate::i18n::{tr, trf};
use crate::state::{self, BOTS, THREADS};
use crate::util;
use dioxus::prelude::*;
use nrzcode_shared::agents::{Bot, BotInput, BotRoutine, BotRoutineInput};
use nrzcode_shared::{AgentKind, ThreadSummary};

/// Offener Editor: `Some(bot)` – `bot.id == 0` = neuer Bot (vorausgefüllt, z. B. aus einer Vorlage).
pub static EDIT_BOT: GlobalSignal<Option<(Bot, EditTab)>> = Signal::global(|| None);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EditTab {
    Profile,
    Memory,
    Routines,
}

/// Zuletzt offener Bot (Ziel des Umschalters).
const LAST_BOT_KEY: &str = "nrz-last-bot";

/// Wohin der Umschalter „Bot“ führt: zum zuletzt offenen Bot, sonst zur Übersicht.
pub fn bot_route() -> Route {
    let last = util::storage_get(LAST_BOT_KEY).and_then(|s| s.parse::<i64>().ok());
    let known = BOTS.peek().as_ref().map(|l| l.iter().any(|b| Some(b.id) == last)).unwrap_or(last.is_some());
    match last {
        Some(id) if known => Route::BotPage { id },
        _ => Route::Bots {},
    }
}

/// Bots einmal laden, falls noch nicht geschehen.
fn use_bots() -> Option<Vec<Bot>> {
    use_hook(|| {
        if BOTS.peek().is_none() {
            spawn(state::load_bots());
        }
    });
    BOTS.read().clone()
}

/// Vorlagen für neue Bots: (Emoji, Name, Rolle, Anweisung).
fn templates() -> Vec<(&'static str, String, String, String)> {
    vec![
        ("🔎", tr("Scout"), tr("Recherche"), tr("Recherchiere gründlich im Web, vergleiche mehrere Quellen und nenne sie. Fasse Ergebnisse knapp mit Stichpunkten zusammen und sage, wie sicher du dir bist.")),
        ("✍️", tr("Feder"), tr("Schreibpartner"), tr("Hilf mir beim Schreiben: entwirf, kürze und poliere Texte in meinem Ton. Frag nach Zielgruppe und Länge, wenn sie unklar sind.")),
        ("🗓️", tr("Kompass"), tr("Persönlicher Assistent"), tr("Behalte meine Aufgaben, Termine und Vorhaben im Blick. Merke dir Wichtiges, plane mit mir den Tag und erinnere mich an Offenes.")),
        ("💡", tr("Funke"), tr("Sparringspartner"), tr("Denk Ideen mit mir durch, stelle kritische Fragen, zeige blinde Flecken und schlage Alternativen vor. Sei ehrlich statt höflich.")),
    ]
}

/// Neuen Bot im Editor beginnen (leer oder aus einer Vorlage).
pub fn new_bot(template: Option<(&str, String, String, String)>) {
    let mut b = Bot { emoji: "🤖".into(), ..Default::default() };
    if let Some((e, n, r, i)) = template {
        b.emoji = e.into();
        b.name = n;
        b.role = r;
        b.instructions = i;
    }
    *EDIT_BOT.write() = Some((b, EditTab::Profile));
}

// ------------------------------------------------------------------ Bausteine

/// Emoji des Bots auf einer Fläche in seiner Farbe.
#[component]
pub fn BotAvatar(name: String, emoji: String, #[props(default = 28)] size: u32) -> Element {
    let h = hue(&name);
    let font = (size as f32 * 0.55).round() as u32;
    let radius = (size as f32 * 0.3).round() as u32;
    let shown = if emoji.trim().is_empty() { initials(&name) } else { emoji };
    rsx! {
        span {
            class: "inline-grid shrink-0 select-none place-items-center font-semibold",
            style: "width: {size}px; height: {size}px; font-size: {font}px; border-radius: {radius}px; background: linear-gradient(135deg, hsl({h} 70% 55% / 0.28), hsl({h} 70% 40% / 0.14)); color: hsl({h} 80% 70%); box-shadow: inset 0 0 0 1px hsl({h} 70% 60% / 0.25)",
            "{shown}"
        }
    }
}

fn current_thread(b: &Bot) -> Option<ThreadSummary> {
    b.thread_id.and_then(|t| THREADS.read().get(&t).cloned())
}

fn memory_count(memory: &str) -> usize {
    memory.lines().filter(|l| !l.trim().is_empty()).count()
}

// ------------------------------------------------------------------ Seitenleiste

/// Bots in der Seitenleiste (Suche filtert nach Name und Rolle).
#[component]
pub fn BotList(query: String) -> Element {
    let route = use_route::<Route>();
    let bots = use_bots();
    let current = match route {
        Route::BotPage { id } => Some(id),
        _ => None,
    };
    let q = query.trim().to_lowercase();
    let Some(list) = bots else {
        return rsx! { div { class: "px-3 py-6 text-center text-sm text-muted-foreground", {tr("Lädt …")} } };
    };
    let shown: Vec<Bot> = list.into_iter().filter(|b| q.is_empty() || b.name.to_lowercase().contains(&q) || b.role.to_lowercase().contains(&q)).collect();
    rsx! {
        div { class: "flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden px-2 pb-2 [&>*]:min-w-0",
            if shown.is_empty() {
                div { class: "px-3 py-8 text-center text-sm text-muted-foreground",
                    p { if q.is_empty() { {tr("Noch keine Bots.")} } else { {tr("Keine Treffer.")} } }
                    if q.is_empty() {
                        button { class: "mt-3 inline-flex items-center gap-1.5 text-foreground hover:underline", onclick: move |_| new_bot(None),
                            icons::Plus { class: "size-4" }
                            {tr("Ersten Bot erstellen")}
                        }
                    }
                }
            }
            div { class: "grid gap-0.5",
                for b in shown {
                    BotItem { key: "{b.id}", active: current == Some(b.id), bot: b.clone() }
                }
            }
        }
    }
}

#[component]
fn BotItem(bot: Bot, active: bool) -> Element {
    let thread = current_thread(&bot);
    let card = if active { "border-sidenav-border bg-sidenav-accent/70 shadow-sm" } else { "border-transparent hover:bg-sidenav-accent/40" };
    let sub = if bot.role.is_empty() { tr("Bot") } else { bot.role.clone() };
    rsx! {
        Link { to: Route::BotPage { id: bot.id }, class: "flex min-w-0 items-center gap-2.5 rounded-lg border px-2 py-2 {card}",
            BotAvatar { name: bot.name.clone(), emoji: bot.emoji.clone(), size: 30 }
            div { class: "min-w-0 flex-1",
                div { class: "flex min-w-0 items-center gap-2",
                    span { class: "min-w-0 flex-1 truncate text-[14px] font-medium", "{bot.name}" }
                    if let Some(t) = thread.clone() {
                        if t.status.is_active() || t.unread {
                            ThreadState { thread: t }
                        }
                    }
                }
                div { class: "truncate text-xs text-muted-foreground", "{sub}" }
            }
        }
    }
}

// ------------------------------------------------------------------ Übersicht

#[component]
pub fn Bots() -> Element {
    let bots = use_bots();
    let Some(list) = bots else {
        return rsx! { Loading {} };
    };
    rsx! {
        div { class: "mx-auto w-full max-w-4xl px-4 py-10 md:px-8",
            div { class: "mb-8 flex flex-wrap items-end gap-4",
                div { class: "min-w-0 flex-1",
                    h1 { class: "text-2xl font-semibold tracking-tight", {tr("Deine Bots")} }
                    p { class: "mt-1 text-sm text-muted-foreground",
                        {tr("KI-Teamkollegen mit eigenem Gedächtnis, eigenem Arbeitsordner und Routinen – sie merken sich, was dir wichtig ist.")}
                    }
                }
                button { class: "{BTN} {BTN_PRIMARY}", onclick: move |_| new_bot(None),
                    icons::Plus {}
                    {tr("Neuer Bot")}
                }
            }
            if !list.is_empty() {
                div { class: "grid gap-3 sm:grid-cols-2",
                    for b in list.clone() {
                        BotCard { key: "{b.id}", bot: b.clone() }
                    }
                }
            }
            h2 { class: "mt-10 mb-3 text-sm font-medium text-muted-foreground", if list.is_empty() { {tr("Mit einer Vorlage beginnen")} } else { {tr("Vorlagen")} } }
            div { class: "grid gap-3 sm:grid-cols-2",
                for (i, (emoji, name, role, instr)) in templates().into_iter().enumerate() {
                    button {
                        key: "{i}",
                        class: "group flex items-start gap-3 rounded-xl border border-dashed p-4 text-left transition-colors hover:border-solid hover:bg-accent/40",
                        onclick: {
                            let (n, r, ins) = (name.clone(), role.clone(), instr.clone());
                            move |_| new_bot(Some((emoji, n.clone(), r.clone(), ins.clone())))
                        },
                        BotAvatar { name: name.clone(), emoji: emoji.to_string(), size: 40 }
                        div { class: "min-w-0 flex-1",
                            div { class: "flex items-center gap-2",
                                span { class: "font-medium", "{role}" }
                                icons::Plus { class: "size-3.5 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" }
                            }
                            p { class: "mt-1 line-clamp-2 text-xs text-muted-foreground", "{instr}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn BotCard(bot: Bot) -> Element {
    let thread = current_thread(&bot);
    let mem = memory_count(&bot.memory);
    let routines = bot.routines.iter().filter(|r| r.enabled).count();
    rsx! {
        Link { to: Route::BotPage { id: bot.id }, class: "flex items-start gap-3 rounded-xl border bg-card p-4 transition-colors hover:bg-accent/40",
            BotAvatar { name: bot.name.clone(), emoji: bot.emoji.clone(), size: 44 }
            div { class: "min-w-0 flex-1",
                div { class: "flex items-center gap-2",
                    span { class: "min-w-0 flex-1 truncate font-medium", "{bot.name}" }
                    if let Some(t) = thread {
                        ThreadState { thread: t }
                    }
                }
                if !bot.role.is_empty() {
                    p { class: "truncate text-sm text-muted-foreground", "{bot.role}" }
                }
                div { class: "mt-2 flex flex-wrap items-center gap-3 text-xs text-muted-foreground [&_svg]:size-3.5",
                    span { class: "inline-flex items-center gap-1", icons::Brain {} {trf("{0} Erinnerungen", &[&mem])} }
                    if routines > 0 {
                        span { class: "inline-flex items-center gap-1", icons::CalendarClock {} {trf("{0} Routinen", &[&routines])} }
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Gespräch

#[component]
pub fn BotPage(id: i64) -> Element {
    rsx! {
        for bid in std::iter::once(id) {
            BotView { key: "{bid}", id: bid }
        }
    }
}

#[component]
fn BotView(id: i64) -> Element {
    let nav = use_navigator();
    let bots = use_bots();
    let mut busy = use_signal(|| false);
    use_hook(move || util::storage_set(LAST_BOT_KEY, &id.to_string()));
    let Some(list) = bots else {
        return rsx! { Loading {} };
    };
    let Some(bot) = list.into_iter().find(|b| b.id == id) else {
        return rsx! {
            EmptyState { title: tr("Diesen Bot gibt es nicht mehr."),
                Link { to: Route::Bots {}, class: "{BTN} {BTN_OUTLINE}", {tr("Zu den Bots")} }
            }
        };
    };
    let mut past: Vec<ThreadSummary> = THREADS.read().values().filter(|t| t.bot_id == Some(id) && t.parent_id.is_none()).cloned().collect();
    past.sort_by_key(|t| std::cmp::Reverse(t.last_activity_at));
    let new_conversation = move |_| {
        if busy() {
            return;
        }
        busy.set(true);
        spawn(async move {
            match api::action::<Bot>(&format!("/bots/{id}/conversation")).await {
                Ok(b) => state::put_bot(b),
                Err(e) => toast_error(e),
            }
            busy.set(false);
        });
    };
    let open = move |tid: i64| {
        spawn(async move {
            match api::action::<Bot>(&format!("/bots/{id}/conversation/{tid}")).await {
                Ok(b) => state::put_bot(b),
                Err(e) => toast_error(e),
            }
        });
    };
    let edit = {
        let b = bot.clone();
        move |tab: EditTab| *EDIT_BOT.write() = Some((b.clone(), tab))
    };
    let edit2 = edit.clone();
    let edit3 = edit.clone();
    let mem = memory_count(&bot.memory);
    let routines = bot.routines.iter().filter(|r| r.enabled).count();
    let remove = {
        let name = bot.name.clone();
        move |_| {
            let name = name.clone();
            spawn(async move {
                let ask = Ask::new(trf("„{0}“ löschen?", &[&name]), tr("Alle Gespräche, das Gedächtnis, die Routinen und der Arbeitsordner des Bots werden gelöscht."))
                    .confirm(tr("Löschen"))
                    .destructive();
                if !confirm(ask).await {
                    return;
                }
                match api::delete(&format!("/bots/{id}")).await {
                    Ok(()) => {
                        if let Some(l) = BOTS.write().as_mut() {
                            l.retain(|b| b.id != id);
                        }
                        nav.replace(Route::Bots {});
                    }
                    Err(e) => toast_error(e),
                }
            });
        }
    };
    let chip = "inline-flex h-7 items-center gap-1.5 rounded-full border px-2.5 text-xs text-muted-foreground hover:bg-accent hover:text-foreground [&_svg]:size-3.5";
    rsx! {
        div { class: "flex h-full min-h-0 flex-col",
            // Kopf: wer der Bot ist, was er sich merkt, was er regelmässig tut
            div { class: "flex shrink-0 flex-wrap items-center gap-3 border-b px-4 py-3 md:px-6",
                BotAvatar { name: bot.name.clone(), emoji: bot.emoji.clone(), size: 40 }
                div { class: "min-w-0 flex-1",
                    div { class: "truncate font-semibold", "{bot.name}" }
                    div { class: "flex flex-wrap items-center gap-x-3 text-xs text-muted-foreground",
                        if !bot.role.is_empty() {
                            span { class: "truncate", "{bot.role}" }
                        }
                        span { class: "inline-flex items-center gap-1",
                            crate::pages::agents::AgentIcon { kind: bot.agent, size: 12 }
                            {crate::pages::agents::model_label(bot.agent, &bot.model)}
                        }
                    }
                }
                div { class: "flex flex-wrap items-center gap-1.5",
                    button { class: chip, title: tr("Was sich der Bot über Gespräche hinweg merkt"), onclick: move |_| edit(EditTab::Memory),
                        icons::Brain {}
                        {trf("{0} Erinnerungen", &[&mem])}
                    }
                    button { class: chip, title: tr("Aufträge, die der Bot regelmässig erledigt"), onclick: move |_| edit2(EditTab::Routines),
                        icons::CalendarClock {}
                        if routines > 0 { {trf("{0} Routinen", &[&routines])} } else { {tr("Routine")} }
                    }
                    button { class: "{BTN} {BTN_OUTLINE} {BTN_SM}", disabled: busy(), title: tr("Neues Gespräch – das Gedächtnis bleibt"), onclick: new_conversation,
                        icons::MessageSquarePlus {}
                        span { class: "hidden sm:inline", {tr("Neues Gespräch")} }
                    }
                    Dropdown {
                        align: Align::End,
                        class: "max-h-96 w-72 overflow-y-auto",
                        trigger_class: "grid size-8 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground [&_svg]:size-4".to_string(),
                        trigger: rsx! { icons::Ellipsis {} },
                        MenuItem { onclick: move |_| edit3(EditTab::Profile),
                            icons::Pencil {}
                            {tr("Bot bearbeiten")}
                        }
                        if past.len() > 1 {
                            MenuSeparator {}
                            MenuLabel { {tr("Gespräche")} }
                            for t in past.clone() {
                                MenuItem { key: "{t.id}", active: bot.thread_id == Some(t.id), onclick: move |_| open(t.id),
                                    icons::History {}
                                    span { class: "min-w-0 flex-1 truncate", {util::datetime(t.created_at)} }
                                    span { class: "text-xs text-muted-foreground", {short_ago(t.last_activity_at)} }
                                }
                            }
                        }
                        MenuSeparator {}
                        MenuItem { onclick: remove,
                            icons::Trash2 { class: "text-destructive" }
                            span { class: "text-destructive", {tr("Bot löschen")} }
                        }
                    }
                }
            }
            div { class: "relative min-h-0 flex-1",
                match bot.thread_id {
                    Some(tid) => rsx! {
                        div { class: "absolute inset-0", crate::pages::agents::ChatView { thread_id: tid } }
                    },
                    None => rsx! {
                        div { class: "grid h-full place-items-center p-6",
                            div { class: "text-center",
                                BotAvatar { name: bot.name.clone(), emoji: bot.emoji.clone(), size: 64 }
                                p { class: "mt-4 text-sm text-muted-foreground", {tr("Kein offenes Gespräch.")} }
                                button { class: "{BTN} {BTN_PRIMARY} mt-4", disabled: busy(), onclick: new_conversation,
                                    icons::MessageSquarePlus {}
                                    {tr("Gespräch beginnen")}
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Editor

const EMOJIS: [&str; 16] = ["🤖", "🦊", "🔎", "✍️", "🗓️", "💡", "🧠", "📈", "🎨", "🧑‍💻", "📚", "🌍", "🛠️", "🎧", "🍳", "⚡"];
const DAYS: [&str; 7] = ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"];

/// Einmal im Rahmen einbinden.
#[component]
pub fn BotEditor() -> Element {
    let mut open = use_signal(|| false);
    let edit = EDIT_BOT();
    use_effect(move || open.set(EDIT_BOT().is_some()));
    // Schliessen über Esc, X oder daneben
    use_effect(move || {
        if !open() && EDIT_BOT.peek().is_some() {
            *EDIT_BOT.write() = None;
        }
    });
    let Some((bot, tab)) = edit else { return rsx! {} };
    let title = if bot.id == 0 { tr("Neuer Bot") } else { bot.name.clone() };
    rsx! {
        Modal { open, title, class: "max-w-xl",
            for key in std::iter::once(format!("{}-{:?}", bot.id, tab)) {
                EditorBody { key: "{key}", bot: bot.clone(), tab }
            }
        }
    }
}

#[component]
fn EditorBody(bot: Bot, tab: EditTab) -> Element {
    let nav = use_navigator();
    let agents = crate::pages::agents::use_agents().unwrap_or_default();
    let is_new = bot.id == 0;
    let mut tab = use_signal(|| tab);
    let mut name = use_signal(|| bot.name.clone());
    let mut emoji = use_signal(|| bot.emoji.clone());
    let mut role = use_signal(|| bot.role.clone());
    let mut instructions = use_signal(|| bot.instructions.clone());
    let mut memory = use_signal(|| bot.memory.clone());
    // Agent: beim neuen Bot die zuletzt im Composer gewählte Auswahl
    let first = if is_new {
        let s = crate::pages::agents::data::Selection::remembered().normalized(&agents);
        (s.agent, s.account_id, s.model, s.effort)
    } else {
        (bot.agent, bot.account_id, bot.model.clone(), bot.effort.clone())
    };
    let mut agent = use_signal(|| first.0);
    let mut account = use_signal(|| first.1);
    let mut model = use_signal(|| first.2.clone());
    let mut effort = use_signal(|| first.3.clone());
    let mut saving = use_signal(|| false);
    let info = agents.iter().find(|a| a.kind == agent()).cloned();
    let with_accounts: Vec<(AgentKind, String)> = agents.iter().filter(|a| !a.accounts.is_empty() || a.kind == agent()).map(|a| (a.kind, a.name.clone())).collect();
    let bot_id = bot.id;
    let routines = bot.routines.clone();

    let save = move |_| {
        if saving() {
            return;
        }
        if name().trim().is_empty() {
            toast_error(tr("Bitte einen Namen eingeben."));
            return;
        }
        saving.set(true);
        let body = BotInput {
            name: Some(name()),
            emoji: Some(emoji()),
            role: Some(role()),
            instructions: Some(instructions()),
            memory: Some(memory()),
            agent: Some(agent()),
            account_id: Some(account()),
            model: Some(model()),
            effort: Some(effort()),
        };
        spawn(async move {
            let r = if is_new { api::post::<Bot, _>("/bots", &body).await } else { api::patch::<Bot, _>(&format!("/bots/{bot_id}"), &body).await };
            saving.set(false);
            match r {
                Ok(b) => {
                    let id = b.id;
                    state::put_bot(b);
                    *EDIT_BOT.write() = None;
                    if is_new {
                        nav.push(Route::BotPage { id });
                    }
                }
                Err(e) => toast_error(e),
            }
        });
    };
    let tab_btn = |on: bool| {
        if on {
            "inline-flex items-center justify-center gap-1.5 rounded-md bg-background px-3 py-1.5 text-[13px] font-medium text-foreground shadow-xs [&_svg]:size-3.5"
        } else {
            "inline-flex items-center justify-center gap-1.5 rounded-md px-3 py-1.5 text-[13px] text-muted-foreground hover:text-foreground [&_svg]:size-3.5"
        }
    };
    rsx! {
        if !is_new {
            div { class: "mb-5 grid grid-cols-3 gap-1 rounded-lg bg-muted p-1",
                button { class: tab_btn(tab() == EditTab::Profile), onclick: move |_| tab.set(EditTab::Profile), icons::Smile {} {tr("Profil")} }
                button { class: tab_btn(tab() == EditTab::Memory), onclick: move |_| tab.set(EditTab::Memory), icons::Brain {} {tr("Gedächtnis")} }
                button { class: tab_btn(tab() == EditTab::Routines), onclick: move |_| tab.set(EditTab::Routines), icons::CalendarClock {} {tr("Routinen")} }
            }
        }
        if tab() == EditTab::Routines {
            Routines { bot_id, routines }
        } else {
            div { class: "grid gap-4",
                if tab() == EditTab::Profile {
                    div { class: "flex items-end gap-3",
                        BotAvatar { name: name(), emoji: emoji(), size: 48 }
                        Field { label: tr("Name"), class: "flex-1",
                            input { class: INPUT, maxlength: "60", placeholder: tr("z. B. Lena"), value: "{name}", oninput: move |e| name.set(e.value()) }
                        }
                    }
                    div { class: "flex flex-wrap gap-1",
                        for e in EMOJIS {
                            button {
                                key: "{e}",
                                class: if emoji() == e { "grid size-8 place-items-center rounded-md bg-accent text-lg ring-1 ring-ring" } else { "grid size-8 place-items-center rounded-md text-lg hover:bg-accent" },
                                onclick: move |_| emoji.set(e.to_string()),
                                "{e}"
                            }
                        }
                        input { class: "{INPUT} h-8 w-16 text-center", maxlength: "8", title: tr("Eigenes Emoji"), value: "{emoji}", oninput: move |e| emoji.set(e.value()) }
                    }
                    Field { label: tr("Rolle"), hint: tr("Kurz, steht unter dem Namen."),
                        input { class: INPUT, maxlength: "80", placeholder: tr("z. B. Recherche, Schreibpartner, Assistent"), value: "{role}", oninput: move |e| role.set(e.value()) }
                    }
                    Field { label: tr("Anweisungen"), hint: tr("Persönlichkeit, Ton und Aufgaben – gilt in jedem Gespräch."),
                        textarea { class: "{TEXTAREA} min-h-28", placeholder: tr("Wie soll sich der Bot verhalten?"), value: "{instructions}", oninput: move |e| instructions.set(e.value()) }
                    }
                    div { class: "grid gap-3 sm:grid-cols-2",
                        Field { label: tr("Agent"),
                            select {
                                class: SELECT,
                                onchange: {
                                    let agents = agents.clone();
                                    move |e: FormEvent| {
                                        if let Some(k) = AgentKind::parse(&e.value()) {
                                            let s = crate::pages::agents::data::Selection { agent: agent(), account_id: account(), model: model(), effort: effort() }.with_agent(k, &agents);
                                            agent.set(s.agent);
                                            account.set(s.account_id);
                                            model.set(s.model);
                                            effort.set(s.effort);
                                        }
                                    }
                                },
                                for (k, n) in with_accounts.clone() {
                                    option { key: "{k.key()}", value: "{k.key()}", selected: k == agent(), "{n}" }
                                }
                            }
                        }
                        Field { label: tr("Konto"),
                            select {
                                class: SELECT,
                                onchange: move |e: FormEvent| account.set(e.value().parse::<i64>().ok()),
                                for a in info.as_ref().map(|i| i.accounts.clone()).unwrap_or_default() {
                                    option { key: "{a.id}", value: "{a.id}", selected: Some(a.id) == account(), "{a.label}" }
                                }
                            }
                        }
                        Field { label: tr("Modell"),
                            select {
                                class: SELECT,
                                onchange: move |e: FormEvent| model.set(e.value()),
                                option { value: "", selected: model().is_empty(), {tr("Standard")} }
                                for m in info.as_ref().map(|i| i.models.clone()).unwrap_or_default() {
                                    option { key: "{m.id}", value: "{m.id}", selected: m.id == model(), "{m.label}" }
                                }
                                if !model().is_empty() && !info.as_ref().is_some_and(|i| i.models.iter().any(|m| m.id == model())) {
                                    option { value: "{model}", selected: true, "{model}" }
                                }
                            }
                        }
                        if info.as_ref().is_some_and(|i| !i.efforts.is_empty()) {
                            Field { label: tr("Denkstufe"),
                                select {
                                    class: SELECT,
                                    onchange: move |e: FormEvent| effort.set(e.value()),
                                    option { value: "", selected: effort().is_empty(), {tr("Standard")} }
                                    for x in info.as_ref().map(|i| i.efforts.clone()).unwrap_or_default() {
                                        option { key: "{x}", value: "{x}", selected: x == effort(), {crate::pages::agents::data::effort_label(&x)} }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    p { class: "text-sm text-muted-foreground",
                        {tr("Das merkt sich der Bot über alle Gespräche hinweg. Er ergänzt es selbst, wenn er etwas Wichtiges lernt – du kannst es hier lesen und ändern.")}
                    }
                    textarea { class: "{TEXTAREA} min-h-64 font-mono text-[13px]", placeholder: tr("- Ich heisse …\n- Ich mag kurze Antworten"), value: "{memory}", oninput: move |e| memory.set(e.value()) }
                }
                div { class: "flex justify-end gap-2 pt-1",
                    button { class: "{BTN} {BTN_GHOST}", onclick: move |_| *EDIT_BOT.write() = None, {tr("Abbrechen")} }
                    button { class: "{BTN} {BTN_PRIMARY}", disabled: saving(), onclick: save,
                        if is_new { icons::Sparkles {} } else { icons::Check {} }
                        if is_new { {tr("Bot erstellen")} } else { {tr("Speichern")} }
                    }
                }
            }
        }
    }
}

fn time_label(min: i64) -> String {
    format!("{:02}:{:02}", min / 60, min % 60)
}

fn days_label(days: i64) -> String {
    match days {
        127 => tr("Täglich"),
        31 => tr("Werktags"),
        96 => tr("Am Wochenende"),
        _ => DAYS.iter().enumerate().filter(|(i, _)| days & (1 << i) != 0).map(|(_, d)| tr(d)).collect::<Vec<_>>().join(", "),
    }
}

/// Routinen eines Bots: Liste und Formular (neu oder ändern).
#[component]
fn Routines(bot_id: i64, routines: Vec<BotRoutine>) -> Element {
    let mut list = use_signal(|| routines.clone());
    let mut editing = use_signal(|| None::<i64>);
    let mut title = use_signal(String::new);
    let mut prompt = use_signal(String::new);
    let mut time = use_signal(|| "08:00".to_string());
    let mut days = use_signal(|| 31i64);
    let mut saving = use_signal(|| false);
    let tz = -crate::pages::agents::tz_offset();

    let mut reset = move || {
        editing.set(None);
        title.set(String::new());
        prompt.set(String::new());
        time.set("08:00".into());
        days.set(31);
    };
    let mut apply = move |b: Bot| {
        list.set(b.routines.clone());
        state::put_bot(b.clone());
        if let Some((cur, tab)) = EDIT_BOT.peek().clone() {
            if cur.id == b.id {
                *EDIT_BOT.write() = Some((b, tab));
            }
        }
    };
    let send = move |r: BotRoutineInput, id: Option<i64>| {
        spawn(async move {
            saving.set(true);
            let res = match id {
                Some(rid) => api::patch::<Bot, _>(&format!("/bots/{bot_id}/routines/{rid}"), &r).await,
                None => api::post::<Bot, _>(&format!("/bots/{bot_id}/routines"), &r).await,
            };
            saving.set(false);
            match res {
                Ok(b) => {
                    apply(b);
                    reset();
                }
                Err(e) => toast_error(e),
            }
        });
    };
    let submit = move |_| {
        let (h, m) = time().split_once(':').map(|(h, m)| (h.parse::<i64>().unwrap_or(8), m.parse::<i64>().unwrap_or(0))).unwrap_or((8, 0));
        let r = BotRoutineInput { title: title(), prompt: prompt(), time_min: (h * 60 + m).clamp(0, 1439), days: days(), tz_offset: tz, enabled: true };
        send(r, editing());
    };
    rsx! {
        div { class: "grid gap-4",
            p { class: "text-sm text-muted-foreground",
                {tr("Der Bot bekommt den Auftrag zur gewählten Zeit in sein aktuelles Gespräch – z. B. jeden Morgen die wichtigsten News.")}
            }
            if !list().is_empty() {
                div { class: "grid gap-2",
                    for r in list() {
                        div { key: "{r.id}", class: "flex items-start gap-3 rounded-lg border p-3",
                            Toggle {
                                checked: r.enabled,
                                onchange: {
                                    let r = r.clone();
                                    move |v: bool| {
                                        let body = BotRoutineInput { title: r.title.clone(), prompt: r.prompt.clone(), time_min: r.time_min, days: r.days, tz_offset: r.tz_offset, enabled: v };
                                        send(body, Some(r.id));
                                    }
                                },
                            }
                            button {
                                class: "min-w-0 flex-1 text-left",
                                onclick: {
                                    let r = r.clone();
                                    move |_| {
                                        editing.set(Some(r.id));
                                        title.set(r.title.clone());
                                        prompt.set(r.prompt.clone());
                                        time.set(time_label(r.time_min));
                                        days.set(r.days);
                                    }
                                },
                                div { class: "flex items-center gap-2 text-sm",
                                    span { class: "font-medium tabular-nums", {time_label(r.time_min)} }
                                    span { class: "text-muted-foreground", {days_label(r.days)} }
                                    if !r.title.is_empty() {
                                        span { class: "truncate", "· {r.title}" }
                                    }
                                }
                                p { class: "mt-0.5 line-clamp-2 text-xs text-muted-foreground", "{r.prompt}" }
                            }
                            button {
                                class: "grid size-7 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-destructive [&_svg]:size-4",
                                title: tr("Routine löschen"),
                                onclick: move |_| {
                                    let rid = r.id;
                                    spawn(async move {
                                        match api::delete_json::<Bot>(&format!("/bots/{bot_id}/routines/{rid}")).await {
                                            Ok(b) => apply(b),
                                            Err(e) => toast_error(e),
                                        }
                                    });
                                },
                                icons::Trash2 {}
                            }
                        }
                    }
                }
            }
            div { class: "grid gap-3 rounded-xl border border-dashed p-4",
                p { class: "text-sm font-medium", if editing().is_some() { {tr("Routine ändern")} } else { {tr("Neue Routine")} } }
                div { class: "flex flex-wrap items-center gap-2",
                    input { class: "{INPUT} w-28", r#type: "time", value: "{time}", oninput: move |e| time.set(e.value()) }
                    div { class: "flex gap-1",
                        for (i, d) in DAYS.iter().enumerate() {
                            button {
                                key: "{i}",
                                class: if days() & (1 << i) != 0 { "h-8 w-9 rounded-md bg-primary text-xs font-medium text-primary-foreground" } else { "h-8 w-9 rounded-md border text-xs text-muted-foreground hover:bg-accent" },
                                onclick: move |_| days.set(days() ^ (1 << i)),
                                {tr(d)}
                            }
                        }
                    }
                }
                input { class: INPUT, maxlength: "80", placeholder: tr("Titel (optional), z. B. Morgen-Briefing"), value: "{title}", oninput: move |e| title.set(e.value()) }
                textarea { class: "{TEXTAREA} min-h-20", placeholder: tr("Auftrag, z. B. Fasse die wichtigsten Tech-News von heute in 5 Punkten zusammen."), value: "{prompt}", oninput: move |e| prompt.set(e.value()) }
                div { class: "flex justify-end gap-2",
                    if editing().is_some() {
                        button { class: "{BTN} {BTN_GHOST} {BTN_SM}", onclick: move |_| reset(), {tr("Abbrechen")} }
                    }
                    button { class: "{BTN} {BTN_PRIMARY} {BTN_SM}", disabled: saving() || prompt().trim().is_empty() || days() == 0, onclick: submit,
                        if editing().is_some() { icons::Check {} } else { icons::Plus {} }
                        if editing().is_some() { {tr("Speichern")} } else { {tr("Routine hinzufügen")} }
                    }
                }
            }
        }
    }
}
