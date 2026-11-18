//! Verwaltung (nur Administratoren): Benutzer anlegen, zum Administrator machen, sperren,
//! Passwort setzen, löschen – und die Einstellungen der Instanz (Registrierung, Vorgabe-Adresse
//! von nrzGit, Anzeige von `max_parallel_runs`, Vorschau-Port, Datenordner).

use super::parts::BusyIcon;
use crate::api;
use crate::components::common::*;
use crate::components::overlay::*;
use crate::components::widgets::*;
use crate::i18n::{tr, trf};
use crate::state::ME;
use crate::util;
use dioxus::prelude::*;
use nrzcode_shared::UserRef;
use nrzcode_shared::workspace::{CreateUser, EditUser, InstanceSettings, UserInfo};

// ------------------------------------------------------------------ Benutzer

#[component]
pub fn UsersSection() -> Element {
    let mut version = use_signal(|| 0u32);
    let users = use_resource(move || async move {
        let _ = version();
        api::get::<Vec<UserInfo>>("/users").await
    });
    let mut create_open = use_signal(|| false);
    let mut pw_for = use_signal(|| None::<UserInfo>);
    let pw_open = use_signal(|| false);
    let me_id = ME.read().as_ref().map(|m| m.id).unwrap_or(0);

    let edit = move |id: i64, body: EditUser, ok_text: String| {
        spawn(async move {
            match api::patch::<UserInfo, _>(&format!("/users/{id}"), &body).await {
                Ok(_) => toast(ok_text),
                Err(e) => toast_error(e),
            }
            version += 1;
        });
    };
    let remove = move |u: UserInfo| {
        spawn(async move {
            let ask = Ask::new(
                trf("{0} löschen?", &[&u.username]),
                tr("Das Konto mit allen Projekten, Threads und Arbeitsordnern wird gelöscht, laufende Agenten werden angehalten. Repositories in nrzGit bleiben."),
            )
            .confirm(tr("Löschen"))
            .destructive();
            if !confirm(ask).await {
                return;
            }
            match api::delete(&format!("/users/{}", u.id)).await {
                Ok(()) => toast(tr("Benutzer gelöscht.")),
                Err(e) => toast_error(e),
            }
            version += 1;
        });
    };

    let list = match users.read().clone() {
        None => return rsx! { Loading {} },
        Some(Err(e)) => return rsx! { Callout { text: e.message, error: true } },
        Some(Ok(l)) => l,
    };
    rsx! {
        Section {
            title: tr("Benutzer"),
            description: tr("Jeder Benutzer hat eigene Projekte, Threads, Agent-Konten und eine eigene Verbindung zu nrzGit."),
            actions: rsx! {
                button { class: "{btn(BTN_PRIMARY)} {BTN_SM}", onclick: move |_| create_open.set(true),
                    icons::UserPlus {}
                    {tr("Benutzer anlegen")}
                }
            },
            div { class: "-mx-6 -my-5 divide-y",
                for u in list {
                    div { key: "{u.id}", class: "flex flex-wrap items-center gap-3 px-6 py-3",
                        UserAvatar { user: UserRef { id: u.id, username: u.username.clone(), full_name: u.full_name.clone(), avatar_url: u.avatar_url.clone(), is_org: false }, size: 32 }
                        div { class: "min-w-0 flex-1",
                            div { class: "flex flex-wrap items-center gap-2",
                                span { class: "font-medium", "{u.username}" }
                                if !u.full_name.is_empty() {
                                    span { class: "text-sm text-muted-foreground", "{u.full_name}" }
                                }
                                if u.is_admin {
                                    span { class: "rounded-full bg-primary/15 px-2 py-0.5 text-[11px] font-medium", {tr("Administrator")} }
                                }
                                if !u.is_active {
                                    span { class: "rounded-full bg-destructive/15 px-2 py-0.5 text-[11px] font-medium text-destructive", {tr("Gesperrt")} }
                                }
                                if u.nrzgit_connected {
                                    span { class: "rounded-full border px-2 py-0.5 text-[11px] text-muted-foreground", "nrzGit" }
                                }
                            }
                            p { class: "mt-0.5 truncate text-xs text-muted-foreground",
                                "{u.email} · "
                                {if u.projects == 1 { tr("1 Projekt") } else { trf("{0} Projekte", &[&u.projects]) }}
                                " · "
                                {if u.threads == 1 { tr("1 Thread") } else { trf("{0} Threads", &[&u.threads]) }}
                                " · "
                                if let Some(ts) = u.last_login_at {
                                    {trf("zuletzt angemeldet {0}", &[&util::ago(ts)])}
                                } else {
                                    {tr("noch nie angemeldet")}
                                }
                            }
                        }
                        if u.id != me_id {
                            Dropdown {
                                align: Align::End,
                                trigger_class: "{btn(BTN_GHOST)} {BTN_SM} px-2",
                                trigger: rsx! { icons::Ellipsis {} },
                                MenuItem {
                                    onclick: {
                                        let admin = u.is_admin;
                                        let id = u.id;
                                        move |_| {
                                            let text = if admin { tr("Kein Administrator mehr.") } else { tr("Ist jetzt Administrator.") };
                                            edit(id, EditUser { is_admin: Some(!admin), ..Default::default() }, text)
                                        }
                                    },
                                    icons::ShieldCheck {}
                                    if u.is_admin { {tr("Administrator entfernen")} } else { {tr("Zum Administrator machen")} }
                                }
                                MenuItem {
                                    onclick: {
                                        let u = u.clone();
                                        let mut pw_open = pw_open;
                                        move |_| {
                                            pw_for.set(Some(u.clone()));
                                            pw_open.set(true);
                                        }
                                    },
                                    icons::KeyRound {}
                                    {tr("Passwort setzen")}
                                }
                                MenuItem {
                                    onclick: {
                                        let active = u.is_active;
                                        let id = u.id;
                                        move |_| {
                                            let text = if active { tr("Konto gesperrt.") } else { tr("Konto entsperrt.") };
                                            edit(id, EditUser { is_active: Some(!active), ..Default::default() }, text)
                                        }
                                    },
                                    if u.is_active {
                                        icons::Lock {}
                                        {tr("Sperren")}
                                    } else {
                                        icons::LockOpen {}
                                        {tr("Entsperren")}
                                    }
                                }
                                MenuSeparator {}
                                MenuItem {
                                    destructive: true,
                                    onclick: {
                                        let u = u.clone();
                                        move |_| remove(u.clone())
                                    },
                                    icons::Trash2 {}
                                    {tr("Löschen")}
                                }
                            }
                        }
                    }
                }
            }
        }
        Modal { open: create_open, title: tr("Benutzer anlegen"),
            CreateUserForm { open: create_open, ondone: move |_| version += 1 }
        }
        Modal { open: pw_open, title: tr("Passwort setzen"), description: pw_for().map(|u| trf("Neues Passwort für {0} – alle Sitzungen enden.", &[&u.username])).unwrap_or_default(),
            if let Some(u) = pw_for() {
                SetPasswordForm { open: pw_open, user_id: u.id }
            }
        }
    }
}

#[component]
fn CreateUserForm(open: Signal<bool>, ondone: EventHandler<()>) -> Element {
    let mut username = use_signal(String::new);
    let mut email = use_signal(String::new);
    let mut full_name = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut admin = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let submit = move |e: FormEvent| {
        e.prevent_default();
        busy.set(true);
        spawn(async move {
            let body = CreateUser { username: username(), email: email(), password: password(), full_name: full_name(), is_admin: admin() };
            let res = api::post::<UserInfo, _>("/users", &body).await;
            busy.set(false);
            match res {
                Ok(u) => {
                    toast(trf("{0} angelegt.", &[&u.username]));
                    open.set(false);
                    ondone.call(());
                }
                Err(e) => toast_error(e),
            }
        });
    };
    rsx! {
        form { class: "grid gap-4", onsubmit: submit,
            div { class: "grid gap-4 sm:grid-cols-2",
                Field { label: tr("Benutzername"),
                    input { class: INPUT, autofocus: true, required: true, value: "{username}", oninput: move |e| username.set(e.value()) }
                }
                Field { label: tr("Voller Name"),
                    input { class: INPUT, value: "{full_name}", oninput: move |e| full_name.set(e.value()) }
                }
            }
            Field { label: tr("E-Mail"),
                input { class: INPUT, r#type: "email", required: true, value: "{email}", oninput: move |e| email.set(e.value()) }
            }
            Field { label: tr("Passwort"), hint: tr("Mindestens 8 Zeichen."),
                input { class: INPUT, r#type: "password", autocomplete: "new-password", required: true, value: "{password}", oninput: move |e| password.set(e.value()) }
            }
            Checkbox { checked: admin(), onchange: move |v| admin.set(v), label: tr("Administrator"), hint: tr("Darf Benutzer und die Instanz verwalten.") }
            div { class: "flex justify-end gap-2",
                button { r#type: "button", class: btn(BTN_OUTLINE), onclick: move |_| open.set(false), {tr("Abbrechen")} }
                button { r#type: "submit", class: btn(BTN_PRIMARY), disabled: busy(),
                    BusyIcon { busy: busy() }
                    {tr("Anlegen")}
                }
            }
        }
    }
}

#[component]
fn SetPasswordForm(open: Signal<bool>, user_id: i64) -> Element {
    let mut password = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let submit = move |e: FormEvent| {
        e.prevent_default();
        busy.set(true);
        spawn(async move {
            let res = api::patch::<UserInfo, _>(&format!("/users/{user_id}"), &EditUser { password: Some(password()), ..Default::default() }).await;
            busy.set(false);
            match res {
                Ok(_) => {
                    toast(tr("Passwort gesetzt."));
                    open.set(false);
                }
                Err(e) => toast_error(e),
            }
        });
    };
    rsx! {
        form { class: "grid gap-4", onsubmit: submit,
            Field { label: tr("Neues Passwort"), hint: tr("Mindestens 8 Zeichen."),
                input { class: INPUT, r#type: "password", autocomplete: "new-password", autofocus: true, required: true, value: "{password}", oninput: move |e| password.set(e.value()) }
            }
            div { class: "flex justify-end gap-2",
                button { r#type: "button", class: btn(BTN_OUTLINE), onclick: move |_| open.set(false), {tr("Abbrechen")} }
                button { r#type: "submit", class: btn(BTN_PRIMARY), disabled: busy(),
                    BusyIcon { busy: busy() }
                    {tr("Speichern")}
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Instanz

#[component]
pub fn InstanceSection() -> Element {
    let data = use_resource(|| async { api::get::<InstanceSettings>("/settings/instance").await });
    match data.read().clone() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! { Callout { text: e.message, error: true } },
        Some(Ok(s)) => rsx! { InstanceForm { settings: s } },
    }
}

#[component]
fn InstanceForm(settings: InstanceSettings) -> Element {
    let s = settings.clone();
    let mut registration = use_signal(|| s.registration_open);
    let mut url = use_signal(|| s.nrzgit_url.clone());
    let mut busy = use_signal(|| false);
    let submit = move |e: FormEvent| {
        e.prevent_default();
        busy.set(true);
        let mut body = settings.clone();
        spawn(async move {
            body.registration_open = registration();
            body.nrzgit_url = url();
            let res = api::put::<InstanceSettings, _>("/settings/instance", &body).await;
            busy.set(false);
            match res {
                Ok(s) => {
                    url.set(s.nrzgit_url);
                    toast(tr("Einstellungen gespeichert."));
                }
                Err(e) => toast_error(e),
            }
        });
    };
    let preview = if s.preview_port == 0 { tr("aus") } else { s.preview_port.to_string() };
    rsx! {
        Section { title: tr("Instanz"), description: tr("Gilt für alle Benutzer dieses nrzCode-Servers."),
            form { class: "grid max-w-xl gap-5", onsubmit: submit,
                Toggle { checked: registration(), onchange: move |v| registration.set(v), label: tr("Registrierung erlauben – neue Personen dürfen sich selbst ein Konto anlegen") }
                Field { label: tr("Vorgabe-Adresse von nrzGit"), hint: tr("Steht für neue Verbindungen schon im Formular. Leer = aus der Konfiguration (NRZCODE_NRZGIT_URL)."),
                    input { class: INPUT, r#type: "url", placeholder: "http://umbrel.local:8390", value: "{url}", oninput: move |e| url.set(e.value()) }
                }
                div {
                    button { r#type: "submit", class: btn(BTN_PRIMARY), disabled: busy(),
                        BusyIcon { busy: busy() }
                        {tr("Speichern")}
                    }
                }
            }
        }
        Section { title: tr("Betrieb"), description: tr("Aus der Konfiguration (Rocket.toml bzw. NRZCODE_*) – nur zur Anzeige."),
            dl { class: "grid gap-x-6 gap-y-3 text-sm sm:grid-cols-2",
                div {
                    dt { class: "text-xs text-muted-foreground", {tr("Gleichzeitige Läufe (max_parallel_runs)")} }
                    dd { class: "mt-0.5 font-medium tabular-nums", "{s.max_parallel_runs}" }
                }
                div {
                    dt { class: "text-xs text-muted-foreground", {tr("Port der Vorschau")} }
                    dd { class: "mt-0.5 font-medium tabular-nums", "{preview}" }
                }
                div {
                    dt { class: "text-xs text-muted-foreground", {tr("Datenordner")} }
                    dd { class: "mt-0.5 break-all font-mono text-xs", "{s.data_dir}" }
                }
                div {
                    dt { class: "text-xs text-muted-foreground", {tr("Version")} }
                    dd { class: "mt-0.5 font-medium", "{s.version}" }
                }
                div {
                    dt { class: "text-xs text-muted-foreground", {tr("Benutzer")} }
                    dd { class: "mt-0.5 font-medium tabular-nums", "{s.users}" }
                }
            }
        }
    }
}
