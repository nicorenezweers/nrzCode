//! Einstellungen im ganzen Fenster: links die Abschnitte (statt der Seitenleiste), oben rechts X
//! zurück zur Seite davor. Profil (mit Passwort und Abmelden), Nutzung (Limits, Kosten, Tokens,
//! Statistik), nrzGit (Adresse, Token, Anleitung), Agenten (`agents::AccountsSettings`), KI-Ketten
//! (`agents::ChainsSettings`), Werkzeuge (`agents::ToolsSettings`), Dieter (Sprachsteuerung),
//! Erscheinungsbild (Thema, Durchsichtig, Modus, Sprache); in der App „App“; für Administratoren
//! Benutzer und Instanz.

use super::admin::{InstanceSection, UsersSection};
use super::layout::{choose_language, choose_theme, close_settings, logout, ModeSwitch, StatsBody, ThemePicker};
use super::parts::*;
use crate::Route;
use crate::api;
use crate::components::common::*;
use crate::components::overlay::*;
use crate::components::widgets::*;
use crate::i18n::{LANG, LANGUAGES, tr, trf};
use crate::state::{self, ME};
use dioxus::prelude::*;
use nrzcode_shared::Me;
use nrzcode_shared::workspace::{MobilePair, MobileStatus};
use nrzcode_shared::workspace::{AvatarUpload, NrzGitCandidate, NrzGitConnect, NrzGitStatus, PasswordChange, ProfileUpdate};

#[component]
pub fn Settings(section: String) -> Element {
    let Some(me) = ME.read().clone() else {
        return rsx! { div { class: "mx-auto max-w-xl p-6", Callout { text: tr("Bitte anmelden.") } } };
    };
    let mut known = vec!["profile", "usage", "nrzgit", "agents", "chains", "tools", "dieter", "mobile", "appearance"];
    // Nur in der App (macOS, Windows): Updates, Code-Sperre, Ordner
    let app = crate::util::in_app();
    if app {
        known.push("app");
    }
    if me.is_admin {
        known.extend(["users", "instance"]);
    }
    let section = if known.contains(&section.as_str()) { section } else { "profile".to_string() };
    let entry = |key: &str, label: String, icon: Element| NavEntry { label, to: Route::Settings { section: key.to_string() }, active: section == key, icon };
    let mut entries = vec![
        entry("profile", tr("Profil"), rsx! { icons::User {} }),
        entry("usage", tr("Nutzung"), rsx! { icons::Gauge {} }),
        entry("nrzgit", tr("Git-Plattformen"), rsx! { icons::GitPullRequest {} }),
        entry("agents", tr("Agenten"), rsx! { icons::Bot {} }),
        entry("chains", tr("KI-Ketten"), rsx! { icons::Workflow {} }),
        entry("tools", tr("Werkzeuge"), rsx! { icons::Wrench {} }),
        entry("dieter", tr("Dieter (Sprache)"), rsx! { icons::Mic {} }),
        entry("mobile", tr("Mobil"), rsx! { icons::Smartphone {} }),
        entry("appearance", tr("Erscheinungsbild"), rsx! { icons::Palette {} }),
    ];
    if app {
        entries.push(entry("app", tr("App"), rsx! { icons::AppWindow {} }));
    }
    if me.is_admin {
        entries.push(entry("users", tr("Benutzer"), rsx! { icons::Users {} }));
        entries.push(entry("instance", tr("Instanz"), rsx! { icons::Server {} }));
    }
    let title = entries.iter().find(|e| e.active).map(|e| e.label.clone()).unwrap_or_default();
    // Abschnitt merken (⌘, und „Einstellungen“ öffnen ihn wieder)
    use_effect(use_reactive!(|section| crate::util::storage_set("nrz-settings-section", &section)));
    let nav = use_navigator();
    // In der Canvas liegen die Einstellungen unter der Blase: ohne Platz für Ampel und Fensterknöpfe
    let canvas = state::MODE() == state::UiMode::Canvas;
    let head = if canvas {
        "flex h-12 shrink-0 items-center px-4"
    } else {
        "flex h-12 shrink-0 items-center px-4 [.nrz-mac_&]:h-14 [.nrz-mac_&]:pb-[calc(56px_-_2*var(--nrz-tl-y,_24px))] [.nrz-mac_&]:pl-[84px]"
    };
    let bar = if canvas {
        "flex h-14 shrink-0 items-center gap-3 border-b px-4 md:px-6"
    } else {
        "flex h-14 shrink-0 items-center gap-3 border-b px-4 md:px-6 [.nrz-win_&]:pr-[150px] [.nrz-mac_&]:max-md:pl-[84px]"
    };
    let content = match section.as_str() {
        "usage" => rsx! { UsageSection {} },
        "nrzgit" => rsx! { NrzGitSection {} },
        "agents" => rsx! { crate::pages::agents::AccountsSettings {} },
        "chains" => rsx! { crate::pages::agents::ChainsSettings {} },
        "tools" => rsx! { crate::pages::agents::ToolsSettings {} },
        "dieter" => rsx! { super::dieter::DieterSettings {} },
        "appearance" => rsx! { AppearanceSection {} },
        "mobile" => rsx! { MobileSection {} },
        "app" => rsx! { AppSection {} },
        "users" => rsx! { UsersSection {} },
        "instance" => rsx! { InstanceSection {} },
        _ => rsx! {
            ProfileSection { me: me.clone() }
            PasswordSection {}
            LogoutSection {}
        },
    };
    rsx! {
        div { class: "flex h-full min-h-0 w-full",
            // Links: Abschnitte wie eine Seitenleiste (in der App mit Platz für die Ampel)
            aside { class: "hidden w-60 shrink-0 flex-col border-r border-sidenav-border bg-sidenav md:flex",
                div { class: head, "data-drag": "1",
                    span { class: "truncate text-[14px] font-semibold tracking-tight", {tr("Einstellungen")} }
                }
                div { class: "min-h-0 flex-1 overflow-y-auto px-2 pt-2 pb-3",
                    SettingsNav { entries: entries.clone(), heading: me.username.clone() }
                }
            }
            div { class: "flex min-w-0 flex-1 flex-col [.nrz-glass_&]:bg-background",
                header { class: bar, "data-drag": "1",
                    h1 { class: "min-w-0 flex-1 truncate text-[15px] font-semibold", "{title}" }
                    button {
                        class: "grid size-8 shrink-0 place-items-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground [&_svg]:size-[18px]",
                        title: tr("Einstellungen schliessen"),
                        onclick: move |_| close_settings(nav),
                        icons::X {}
                    }
                }
                main { class: "min-h-0 flex-1 overflow-y-auto",
                    div { class: "mx-auto max-w-4xl space-y-6 p-4 md:p-8",
                        // Schmal: Abschnitte als Leiste über dem Inhalt
                        div { class: "md:hidden", SectionNav { entries } }
                        {content}
                    }
                }
            }
        }
    }
}

/// Abschnitte untereinander (links in den Einstellungen).
#[component]
fn SettingsNav(entries: Vec<NavEntry>, heading: String) -> Element {
    rsx! {
        nav { class: "grid gap-0.5",
            p { class: "truncate px-2.5 pb-1 text-xs font-medium text-muted-foreground", "{heading}" }
            for e in entries {
                Link {
                    to: e.to.clone(),
                    class: if e.active {
                        "flex items-center gap-2.5 rounded-lg bg-sidenav-accent px-2.5 py-1.5 text-[13px] font-medium text-foreground [&_svg]:size-4"
                    } else {
                        "flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-[13px] text-muted-foreground hover:bg-sidenav-accent hover:text-foreground [&_svg]:size-4"
                    },
                    {e.icon.clone()}
                    "{e.label}"
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Nutzung

/// Limits, Kosten und Tokens (wie im Fenster der Menüleiste) und die genaue Statistik.
#[component]
fn UsageSection() -> Element {
    rsx! {
        Section {
            title: tr("Nutzung"),
            description: tr("Limits der Abos (wo der Anbieter sie meldet), Kosten und Tokens aller Läufe – auch laufender und gelöschter Threads."),
            crate::pages::agents::UsageBoard { compact: false }
        }
        Section { title: tr("Statistik"), description: tr("Genaue Zahlen zu Projekten, Threads, Läufen, Tokens, Kosten und Laufzeit."),
            StatsBody {}
        }
    }
}

// ------------------------------------------------------------------ Abmelden

#[component]
fn LogoutSection() -> Element {
    let nav = use_navigator();
    rsx! {
        Section { title: tr("Abmelden"), description: tr("Meldet dich auf diesem Gerät ab – laufende Agenten arbeiten auf dem Server weiter."),
            button {
                class: "inline-flex h-9 items-center gap-1.5 rounded-md border px-3 text-sm text-destructive hover:bg-destructive/10 [&_svg]:size-4",
                onclick: move |_| logout(nav),
                icons::LogOut {}
                {tr("Abmelden")}
            }
        }
    }
}

// ------------------------------------------------------------------ Profil

// ------------------------------------------------------------------ Profilbild

/// Profilbild gerade beim Hochladen (für Ladeanzeigen).
pub static AVATAR_BUSY: GlobalSignal<bool> = Signal::global(|| false);

/// Bild mittig quadratisch zuschneiden, auf `size` Pixel verkleinern und als PNG (Data-URL) liefern.
async fn square_png(file: web_sys::File, size: u32) -> Result<String, String> {
    use wasm_bindgen::JsCast;
    let fail = || tr("Das Bild lässt sich nicht lesen.");
    let url = web_sys::Url::create_object_url_with_blob(&file).map_err(|_| fail())?;
    let img = web_sys::HtmlImageElement::new().map_err(|_| fail())?;
    img.set_src(&url);
    let decoded = wasm_bindgen_futures::JsFuture::from(img.decode()).await;
    let _ = web_sys::Url::revoke_object_url(&url);
    decoded.map_err(|_| fail())?;
    let (w, h) = (img.natural_width() as f64, img.natural_height() as f64);
    if w < 1.0 || h < 1.0 {
        return Err(fail());
    }
    let doc = web_sys::window().and_then(|w| w.document()).ok_or_else(fail)?;
    let canvas: web_sys::HtmlCanvasElement = doc.create_element("canvas").map_err(|_| fail())?.dyn_into().map_err(|_| fail())?;
    canvas.set_width(size);
    canvas.set_height(size);
    let ctx: web_sys::CanvasRenderingContext2d = canvas.get_context("2d").ok().flatten().and_then(|c| c.dyn_into().ok()).ok_or_else(fail)?;
    let side = w.min(h);
    ctx.draw_image_with_html_image_element_and_sw_and_sh_and_dx_and_dy_and_dw_and_dh(&img, (w - side) / 2.0, (h - side) / 2.0, side, side, 0.0, 0.0, size as f64, size as f64)
        .map_err(|_| fail())?;
    canvas.to_data_url_with_type("image/png").map_err(|_| fail())
}

/// Neues Profilbild aus einer Datei (mittig zugeschnitten auf 256 × 256). Läuft unabhängig von der
/// Komponente weiter (z. B. wenn sich das Menü schliesst).
pub fn upload_avatar(files: Vec<web_sys::File>) {
    let Some(file) = files.into_iter().next() else { return };
    if !file.type_().starts_with("image/") {
        toast_error(tr("Bitte ein Bild als PNG, JPEG, WebP oder GIF wählen."));
        return;
    }
    *AVATAR_BUSY.write() = true;
    dioxus::core::spawn_forever(async move {
        let res = match square_png(file, 256).await {
            Ok(image) => api::put::<Me, _>("/me/avatar", &AvatarUpload { image }).await.map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        *AVATAR_BUSY.write() = false;
        match res {
            Ok(m) => {
                *ME.write() = Some(m);
                toast(tr("Profilbild geändert."));
            }
            Err(e) => toast_error(e),
        }
    });
}

/// Eigenes Profilbild entfernen (wieder das erzeugte Muster).
pub fn remove_avatar() {
    *AVATAR_BUSY.write() = true;
    dioxus::core::spawn_forever(async move {
        let res = api::delete_json::<Me>("/me/avatar").await;
        *AVATAR_BUSY.write() = false;
        match res {
            Ok(m) => {
                *ME.write() = Some(m);
                toast(tr("Profilbild entfernt."));
            }
            Err(e) => toast_error(e),
        }
    });
}

/// Profilbild mit Kamera beim Überfahren – ein Klick öffnet die Dateiauswahl.
#[component]
pub fn AvatarPicker(#[props(default = 48)] size: u32) -> Element {
    let Some(user) = super::layout::me_ref() else { return rsx! {} };
    let busy = AVATAR_BUSY();
    let shade = if busy { "opacity-100" } else { "opacity-0 group-hover/avatar:opacity-100" };
    rsx! {
        label {
            class: "group/avatar relative inline-grid shrink-0 cursor-pointer place-items-center rounded-full",
            title: tr("Profilbild ändern"),
            UserAvatar { user, size }
            span { class: "absolute inset-0 grid place-items-center rounded-full bg-black/45 text-white transition-opacity {shade}",
                if busy {
                    icons::LoaderCircle { class: "size-4 animate-spin" }
                } else {
                    icons::Camera { class: "size-4" }
                }
            }
            input {
                r#type: "file",
                accept: "image/png,image/jpeg,image/webp,image/gif",
                class: "hidden",
                onchange: move |e| upload_avatar(crate::util::input_files(&e)),
            }
        }
    }
}

#[component]
fn ProfileSection(me: Me) -> Element {
    let mut full_name = use_signal(|| me.full_name.clone());
    let mut email = use_signal(|| me.email.clone());
    let mut busy = use_signal(|| false);
    let submit = move |e: FormEvent| {
        e.prevent_default();
        busy.set(true);
        spawn(async move {
            let res = api::put::<Me, _>("/me/profile", &ProfileUpdate { full_name: full_name(), email: email() }).await;
            busy.set(false);
            match res {
                Ok(m) => {
                    *ME.write() = Some(m);
                    toast(tr("Profil gespeichert."));
                }
                Err(e) => toast_error(e),
            }
        });
    };
    rsx! {
        Section { title: tr("Profil"), description: tr("Name und E-Mail erscheinen als Autor der Commits, die du oder deine Agenten anlegen."),
            form { class: "grid max-w-lg gap-4", onsubmit: submit,
                div { class: "flex items-center gap-4",
                    AvatarPicker { size: 64 }
                    div { class: "min-w-0 flex-1",
                        p { class: "font-medium", "{me.username}" }
                        p { class: "text-xs text-muted-foreground",
                            if me.is_admin { {tr("Administrator")} } else { {tr("Benutzer")} }
                        }
                        div { class: "mt-2 flex flex-wrap gap-2",
                            label { class: "{btn(BTN_OUTLINE)} cursor-pointer",
                                icons::Camera {}
                                {tr("Bild ändern")}
                                input {
                                    r#type: "file",
                                    accept: "image/png,image/jpeg,image/webp,image/gif",
                                    class: "hidden",
                                    onchange: move |e| upload_avatar(crate::util::input_files(&e)),
                                }
                            }
                            if me.custom_avatar {
                                button { r#type: "button", class: btn(BTN_GHOST), disabled: AVATAR_BUSY(), onclick: move |_| remove_avatar(),
                                    icons::Trash2 {}
                                    {tr("Entfernen")}
                                }
                            }
                        }
                    }
                }
                Field { label: tr("Voller Name"),
                    input { class: INPUT, autocomplete: "name", value: "{full_name}", oninput: move |e| full_name.set(e.value()) }
                }
                Field { label: tr("E-Mail"),
                    input { class: INPUT, r#type: "email", autocomplete: "email", required: true, value: "{email}", oninput: move |e| email.set(e.value()) }
                }
                div {
                    button { r#type: "submit", class: btn(BTN_PRIMARY), disabled: busy(),
                        BusyIcon { busy: busy() }
                        {tr("Speichern")}
                    }
                }
            }
        }
    }
}

#[component]
fn PasswordSection() -> Element {
    let mut current = use_signal(String::new);
    let mut new_pw = use_signal(String::new);
    let mut repeat = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let submit = move |e: FormEvent| {
        e.prevent_default();
        if new_pw() != repeat() {
            toast_error(tr("Die neuen Passwörter stimmen nicht überein."));
            return;
        }
        busy.set(true);
        spawn(async move {
            let res = api::put::<serde_json::Value, _>("/me/password", &PasswordChange { current_password: current(), new_password: new_pw() }).await;
            busy.set(false);
            match res {
                Ok(_) => {
                    current.set(String::new());
                    new_pw.set(String::new());
                    repeat.set(String::new());
                    toast(tr("Passwort geändert – andere Sitzungen sind abgemeldet."));
                }
                Err(e) => toast_error(e),
            }
        });
    };
    rsx! {
        Section { title: tr("Passwort"), description: tr("Nach dem Ändern sind alle anderen Sitzungen abgemeldet."),
            form { class: "grid max-w-lg gap-4", onsubmit: submit,
                Field { label: tr("Bisheriges Passwort"),
                    input { class: INPUT, r#type: "password", autocomplete: "current-password", required: true, value: "{current}", oninput: move |e| current.set(e.value()) }
                }
                Field { label: tr("Neues Passwort"), hint: tr("Mindestens 8 Zeichen."),
                    input { class: INPUT, r#type: "password", autocomplete: "new-password", required: true, value: "{new_pw}", oninput: move |e| new_pw.set(e.value()) }
                }
                Field { label: tr("Neues Passwort wiederholen"),
                    input { class: INPUT, r#type: "password", autocomplete: "new-password", required: true, value: "{repeat}", oninput: move |e| repeat.set(e.value()) }
                }
                div {
                    button { r#type: "submit", class: btn(BTN_PRIMARY), disabled: busy(),
                        BusyIcon { busy: busy() }
                        {tr("Passwort ändern")}
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ nrzGit

#[component]
fn NrzGitSection() -> Element {
    let mut version = use_signal(|| 0u32);
    // Alle Plattformen (nrzGit, luGit, GitHub) – jede mit eigener Karte, einzeln verbinden und trennen
    let status = use_resource(move || async move {
        let _ = version();
        api::get::<Vec<NrzGitStatus>>("/nrzgit/connections").await
    });
    match status.read().clone() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! { Callout { text: e.message, error: true } },
        Some(Ok(list)) => rsx! {
            for s in list {
                NrzGitForm { key: "{s.kind}-{version}", status: s.clone(), onchange: move |_| version += 1 }
            }
            Section { title: tr("So geht’s"), description: tr("Einmal einrichten – danach arbeiten alle Agenten mit nrzGit, luGit und GitHub."),
                ol { class: "grid gap-3 text-sm",
                    Step { n: 1, text: tr("Öffne nrzGit und gehe zu Einstellungen → MCP (der Knopf „Token in nrzGit erstellen“ führt direkt dorthin).") }
                    Step { n: 2, text: tr("Erstelle ein Token für nrzCode und kopiere es – es wird nur einmal angezeigt.") }
                    Step { n: 3, text: tr("Füge Adresse und Token oben ein und klicke auf „Verbinden“. nrzCode prüft das Token sofort.") }
                    Step { n: 4, text: tr("Fertig: Unter „Projekt hinzufügen“ erscheinen deine Repositories. Alle Agenten können Repositories anlegen, pushen, Pull Requests und Releases erstellen und Images in die Registry von nrzGit bauen.") }
                }
                p { class: "mt-4 max-w-xl text-sm text-muted-foreground",
                    {tr("GitHub geht zusätzlich zu beiden: ein Personal Access Token einfügen (github.com → Settings → Developer settings). Klassisch braucht es das Recht „repo“ (für Organisationen dazu „read:org“), fein eingestellt Contents, Pull requests und Administration mit Lesen und Schreiben. nrzCode klont, pusht und legt Pull Requests an; die Agenten bekommen GitHub als MCP-Server (github) und können das CLI gh benutzen.")}
                }
                p { class: "mt-4 max-w-xl text-sm text-muted-foreground",
                    {tr("luGit geht genauso und zusätzlich zu nrzGit: Adresse (z. B. http://umbrel.local:8430) und ein Zugriffstoken (lgt_…) mit den Rechten „user“ und „repo“ eintragen. Ab luGit 1.1.0 bekommen die Agenten luGit ebenfalls als MCP-Server (lugit); bei älteren Versionen klont, pusht und legt nrzCode Pull Requests an, die Agenten arbeiten dort mit git.")}
                }
            }
        },
    }
}

#[component]
fn NrzGitForm(status: NrzGitStatus, onchange: EventHandler<()>) -> Element {
    let s = status.clone();
    let lugit = s.kind == "lugit";
    let github = s.kind == "github";
    let kind = if lugit {
        "lugit"
    } else if github {
        "github"
    } else {
        "nrzgit"
    };
    let forge = if lugit {
        "luGit"
    } else if github {
        "GitHub"
    } else {
        "nrzGit"
    };
    let mut url = use_signal(|| if s.url.is_empty() { s.default_url.clone() } else { s.url.clone() });
    let mut public_url = use_signal(|| s.public_url.clone());
    let mut token = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut searching = use_signal(|| false);
    let mut found = use_signal(|| None::<Vec<NrzGitCandidate>>);
    // Link „Token erstellen“ passt sich der Adresse für den Browser an (sonst der Verbindungsadresse)
    let typed = if public_url().trim().is_empty() { url() } else { public_url() };
    let typed = typed.trim().trim_end_matches('/').trim_end_matches("/mcp").to_string();
    let token_page = if lugit {
        "user/settings/tokens"
    } else if github {
        "settings/tokens"
    } else {
        "user/settings/mcp"
    };
    let token_link = if typed.is_empty() {
        String::new()
    } else if typed.starts_with("http://") || typed.starts_with("https://") {
        format!("{typed}/{token_page}")
    } else {
        format!("http://{typed}/{token_page}")
    };
    let submit = move |e: FormEvent| {
        e.prevent_default();
        busy.set(true);
        spawn(async move {
            let body = NrzGitConnect { url: url(), token: token(), public_url: public_url(), kind: kind.to_string() };
            let res = api::put::<NrzGitStatus, _>("/nrzgit", &body).await;
            busy.set(false);
            match res {
                Ok(s) => {
                    token.set(String::new());
                    toast(trf("Verbunden mit {0} als {1}.", &[&forge, &s.username]));
                    state::load_me().await;
                    onchange.call(());
                }
                Err(e) => toast_error(e),
            }
        });
    };
    let verify = move |_| {
        busy.set(true);
        spawn(async move {
            match api::action::<NrzGitStatus>(&format!("/nrzgit/verify?kind={kind}")).await {
                Ok(s) if s.connected => toast(tr("Die Verbindung funktioniert.")),
                Ok(s) => toast_error(s.error),
                Err(e) => toast_error(e),
            }
            busy.set(false);
            state::load_me().await;
            onchange.call(());
        });
    };
    let disconnect = move |_| {
        spawn(async move {
            let ask = Ask::new(
                trf("Verbindung zu {0} trennen?", &[&forge]),
                trf("Das Token wird gelöscht. Agenten können danach dort nicht mehr pushen, und neue Projekte aus {0} gehen erst wieder nach dem Verbinden.", &[&forge]),
            )
            .confirm(tr("Trennen"))
            .destructive();
            if !confirm(ask).await {
                return;
            }
            match api::delete(&format!("/nrzgit?kind={kind}")).await {
                Ok(()) => toast(tr("Verbindung getrennt.")),
                Err(e) => toast_error(e),
            }
            state::load_me().await;
            onchange.call(());
        });
    };
    let search = move |_| {
        searching.set(true);
        spawn(async move {
            match api::get::<Vec<NrzGitCandidate>>(&format!("/nrzgit/candidates?url={}", api::enc(&url()))).await {
                Ok(list) => {
                    // Nur Adressen dieser Plattform zeigen (erreichbare der anderen und deren übliche Adressen nicht)
                    let list: Vec<NrzGitCandidate> = list
                        .into_iter()
                        .filter(|c| if c.reachable { (c.kind == "lugit") == lugit } else { c.label.starts_with("luGit") == lugit || c.label == "Eingegeben" })
                        .collect();
                    // Erste erreichbare Adresse gleich übernehmen
                    if let Some(c) = list.iter().find(|c| c.reachable) {
                        if !url().trim().is_empty() && url() != c.url && public_url().trim().is_empty() && !url().contains("_app_1") {
                            // Bisherige Adresse (vom Browser) bleibt für Links erhalten
                            public_url.set(url());
                        }
                        url.set(c.url.clone());
                    }
                    found.set(Some(list));
                }
                Err(e) => toast_error(e),
            }
            searching.set(false);
        });
    };
    let has_saved = !s.url.is_empty();
    let (placeholder, public_placeholder, token_placeholder) = if lugit {
        ("http://lu-git_app_1:8430", "http://umbrel.local:8430", "lgt_…")
    } else if github {
        ("https://github.com", "", "ghp_… / github_pat_…")
    } else {
        ("http://nrz-git_app_1:8380", "http://umbrel.local:8390", "nrzg_…")
    };
    rsx! {
        Section {
            title: forge.to_string(),
            description: if lugit {
                tr("nrzCode klont, pusht und legt Pull Requests mit deinem Zugriffstoken an – zusätzlich zu nrzGit. Ab luGit 1.1.0 bekommen die Agenten luGit als MCP-Server.")
            } else if github {
                tr("nrzCode klont, pusht und legt Pull Requests mit deinem Personal Access Token an – zusätzlich zu nrzGit und luGit. Die Agenten bekommen GitHub als MCP-Server und können das CLI gh benutzen.")
            } else {
                tr("nrzCode klont, pusht und legt Pull Requests mit deinem Zugriffstoken an – die Agenten bekommen nrzGit zusätzlich als MCP-Server.")
            },
            actions: rsx! {
                if s.connected {
                    span { class: "inline-flex items-center gap-1.5 rounded-full bg-success/15 px-2.5 py-1 text-xs font-medium text-success",
                        icons::CircleCheck { class: "size-3.5" }
                        {tr("Verbunden")}
                        if s.has_mcp { " · MCP" }
                    }
                } else if has_saved {
                    span { class: "inline-flex items-center gap-1.5 rounded-full bg-destructive/15 px-2.5 py-1 text-xs font-medium text-destructive",
                        icons::CircleAlert { class: "size-3.5" }
                        {tr("Gestört")}
                    }
                } else {
                    span { class: "inline-flex items-center gap-1.5 rounded-full bg-muted px-2.5 py-1 text-xs font-medium text-muted-foreground", {tr("Nicht verbunden")} }
                }
            },
            if has_saved {
                dl { class: "mb-5 grid gap-x-6 gap-y-3 text-sm sm:grid-cols-2",
                    div {
                        dt { class: "text-xs text-muted-foreground", {tr("Adresse")} }
                        dd { class: "mt-0.5 break-all",
                            ExternalLink { href: s.url.clone(), class: "hover:underline", "{s.url}" }
                        }
                    }
                    div {
                        dt { class: "text-xs text-muted-foreground", {trf("Benutzer in {0}", &[&forge])} }
                        dd { class: "mt-0.5", if s.username.is_empty() { "–" } else { "{s.username}" } }
                    }
                    div {
                        dt { class: "text-xs text-muted-foreground", {tr("Token")} }
                        dd { class: "mt-0.5 font-mono", "{s.token_hint}" }
                    }
                    div {
                        dt { class: "text-xs text-muted-foreground", {tr("Zuletzt geprüft")} }
                        dd { class: "mt-0.5",
                            if let Some(ts) = s.verified_at { RelTime { ts } } else { "–" }
                        }
                    }
                }
                if !s.error.is_empty() {
                    div { class: "mb-5", Callout { text: tr(&s.error), error: true } }
                }
                if s.connected && !s.has_mcp && github {
                    div { class: "mb-5", Callout { text: tr("Den MCP-Server von GitHub gibt es nur für github.com – hier arbeiten die Agenten mit git und dem CLI gh."), error: false } }
                }
                if s.connected && !s.has_mcp && !github {
                    div { class: "mb-5", Callout { text: tr("Dieses luGit hat noch keinen MCP-Server (den gibt es ab luGit 1.1.0). Aktualisiere luGit und klicke auf „Prüfen“ – bis dahin arbeiten die Agenten dort nur mit git."), error: false } }
                }
            }
            form { class: "grid max-w-xl gap-4", onsubmit: submit,
                Field {
                    label: if github { tr("Adresse von GitHub") } else { trf("Adresse von {0} (vom Server aus)", &[&forge]) },
                    hint: if github {
                        tr("https://github.com – oder die Adresse eures GitHub Enterprise Server.")
                    } else if lugit {
                        tr("Der Server von nrzCode verbindet sich selbst. Im Docker-Netz von Umbrel ist das meist http://lu-git_app_1:8430, sonst die Adresse, unter der du luGit öffnest.")
                    } else {
                        tr("Der Server von nrzCode verbindet sich selbst. Im Docker-Netz von Umbrel ist das meist http://nrz-git_app_1:8380 – nrzGit zeigt alle internen Adressen unter Einstellungen → MCP.")
                    },
                    div { class: "flex gap-2",
                        input { class: INPUT, r#type: "url", required: true, placeholder: placeholder, value: "{url}", oninput: move |e| url.set(e.value()) }
                        if !github {
                            button { r#type: "button", class: "{btn(BTN_OUTLINE)} shrink-0", disabled: searching(), onclick: search, title: tr("Übliche Adressen vom Server aus durchprobieren"),
                                BusyIcon { busy: searching() }
                                if !searching() { icons::Radar {} }
                                {tr("Automatisch suchen")}
                            }
                        }
                    }
                }
                if let Some(list) = found() {
                    div { class: "grid gap-1 rounded-lg border p-2",
                        for c in list {
                            div { key: "{c.url}", class: "flex items-center gap-2 rounded-md px-2 py-1.5 text-sm",
                                if c.reachable {
                                    icons::CircleCheck { class: "size-4 shrink-0 text-success" }
                                } else {
                                    icons::CircleX { class: "size-4 shrink-0 text-muted-foreground" }
                                }
                                div { class: "min-w-0 flex-1",
                                    p { class: "truncate font-mono text-[13px]", "{c.url}" }
                                    p { class: "truncate text-xs text-muted-foreground",
                                        {tr(&c.label)}
                                        " · "
                                        if c.reachable && c.kind == "lugit" { "luGit {c.version}" } else if c.reachable { {trf("nrzGit {0}", &[&c.version])} } else { {tr(&c.error)} }
                                    }
                                }
                                if c.reachable {
                                    button { r#type: "button", class: "{btn(BTN_GHOST)} {BTN_SM}",
                                        onclick: {
                                            let u = c.url.clone();
                                            move |_| url.set(u.clone())
                                        },
                                        {tr("Übernehmen")}
                                    }
                                }
                            }
                        }
                    }
                }
                if !github {
                    Field { label: tr("Adresse im Browser (optional)"), hint: trf("Für Links zu {0} (Repositories, Pull Requests, Token erstellen), wenn die Adresse oben intern ist – z. B. {1}.", &[&forge, &public_placeholder]),
                        input { class: INPUT, r#type: "url", placeholder: public_placeholder, value: "{public_url}", oninput: move |e| public_url.set(e.value()) }
                    }
                }
                Field {
                    label: tr("Zugriffstoken"),
                    hint: if has_saved {
                        tr("Leer lassen, um das gespeicherte Token zu behalten.")
                    } else if lugit {
                        tr("Beginnt mit lgt_ und braucht die Rechte „user“ und „repo“ – wird nie wieder angezeigt.")
                    } else if github {
                        tr("Personal Access Token: klassisch (ghp_…) mit „repo“ und „read:org“, oder fein eingestellt (github_pat_…) mit Contents, Pull requests und Administration (Lesen und Schreiben) – wird nie wieder angezeigt.")
                    } else {
                        tr("Beginnt mit nrzg_ – wird nie wieder angezeigt.")
                    },
                    input { class: "{INPUT} font-mono", r#type: "password", autocomplete: "off", placeholder: token_placeholder, value: "{token}", oninput: move |e| token.set(e.value()) }
                }
                div { class: "flex flex-wrap items-center gap-2",
                    button { r#type: "submit", class: btn(BTN_PRIMARY), disabled: busy(),
                        BusyIcon { busy: busy() }
                        if has_saved { {tr("Speichern und prüfen")} } else { {tr("Verbinden")} }
                    }
                    if !token_link.is_empty() {
                        ExternalLink { href: token_link.clone(), class: btn(BTN_OUTLINE),
                            icons::KeyRound {}
                            if lugit {
                                {tr("Token in luGit erstellen")}
                            } else if github {
                                {tr("Token bei GitHub erstellen")}
                            } else {
                                {tr("Token in nrzGit erstellen")}
                            }
                            icons::ExternalLink { class: "size-3.5 text-muted-foreground" }
                        }
                    }
                    if has_saved {
                        button { r#type: "button", class: btn(BTN_GHOST), disabled: busy(), onclick: verify,
                            icons::RefreshCw {}
                            {tr("Prüfen")}
                        }
                        button { r#type: "button", class: "{btn(BTN_GHOST)} text-destructive", onclick: disconnect,
                            icons::Unlink {}
                            {tr("Trennen")}
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn Step(n: u32, #[props(into)] text: String) -> Element {
    rsx! {
        li { class: "flex gap-3",
            span { class: "grid size-6 shrink-0 place-items-center rounded-full bg-muted text-xs font-semibold", "{n}" }
            span { class: "pt-0.5", "{text}" }
        }
    }
}

// ------------------------------------------------------------------ Erscheinungsbild

#[component]
fn AppearanceSection() -> Element {
    let theme = ME.read().as_ref().map(|m| m.theme.clone()).unwrap_or_else(|| "dark".into());
    rsx! {
        Section { title: tr("Thema"), description: tr("Hell, dunkel, wie das Betriebssystem oder Mondnacht."),
            div { class: "max-w-md",
                ThemePicker { current: theme, onpick: move |t: String| choose_theme(&t) }
            }
        }
        if crate::util::glass_available() {
            Section { title: tr("Durchsichtig"), description: tr("Milchglas wie bei nativen Apps: Der Schreibtisch scheint weich gezeichnet durch Seitenleiste und Fenster."),
                Toggle { checked: state::GLASS_ON(), onchange: move |on: bool| state::set_glass(on), label: tr("Fenster durchsichtig") }
            }
        }
        Section { title: tr("Modus"), description: tr("Chat: Seitenleiste mit Threads, ein Chat im Fokus. Canvas: alle Fenster eines Arbeitsbereichs frei auf einer Fläche."),
            div { class: "max-w-xs", ModeSwitch { label: false } }
        }
        Section { title: tr("Sprache"), description: tr("Die Oberfläche erscheint in dieser Sprache."),
            div { class: "grid max-w-md gap-2",
                for (code, label) in LANGUAGES.iter().copied() {
                    button {
                        class: if *LANG.read() == code { "flex items-center justify-between rounded-lg border border-ring bg-accent px-4 py-2.5 text-sm font-medium" } else { "flex items-center justify-between rounded-lg border px-4 py-2.5 text-sm hover:bg-accent/50" },
                        onclick: move |_| choose_language(code),
                        "{label}"
                        if *LANG.read() == code {
                            icons::Check { class: "size-4" }
                        }
                    }
                }
            }
        }
        NotifySection {}
        Section { title: tr("Ansicht"), description: tr("Was die Oberfläche anbietet, merkt sich dieser Browser. Die rechte Spalte ist ab Werk an und startet eingeklappt, das Terminal ist aus – eingeschaltet gibt es oben rechts je einen Knopf dafür."),
            div { class: "grid max-w-md gap-3",
                Toggle { checked: LEFT(), onchange: move |_| toggle(&LEFT, "nrz-left"), label: tr("Seitenleiste") }
                Toggle {
                    checked: RIGHT_FEATURE(),
                    onchange: move |_| {
                        toggle(&RIGHT_FEATURE, "nrz-feature-right");
                        // Eingeschaltet: gleich zeigen
                        if *RIGHT_FEATURE.peek() && !*RIGHT.peek() {
                            toggle(&RIGHT, "nrz-right");
                        }
                    },
                    label: tr("Rechte Spalte auf der Thread-Seite (Git, Vorschau, Subagenten)"),
                }
                Toggle {
                    checked: BOTTOM_FEATURE(),
                    onchange: move |_| toggle(&BOTTOM_FEATURE, "nrz-feature-terminal"),
                    label: tr("Terminal unten auf der Thread-Seite"),
                }
            }
        }
    }
}

// ------------------------------------------------------------------ App

/// Nur in der App: Updates, Code-Sperre und Ordner (unter Windows gibt es dafür kein Menü mehr,
/// am Mac stehen sie zusätzlich im Menü).
#[component]
fn AppSection() -> Element {
    let btn = "inline-flex h-9 items-center gap-1.5 rounded-md border px-3 text-sm hover:bg-accent [&_svg]:size-4";
    rsx! {
        Section { title: tr("Updates"), description: tr("nrzCode sucht regelmässig selbst nach Updates. Hier gleich nachsehen – ein neues Update erscheint oben als Hinweis zum Installieren."),
            div { class: "flex flex-wrap gap-2",
                button {
                    class: btn,
                    onclick: move |_| {
                        crate::util::native("update:check");
                    },
                    icons::RefreshCw {}
                    {tr("Nach Updates suchen")}
                }
            }
        }
        Section { title: tr("Code-Sperre"), description: tr("Sperrt nrzCode nach einer Pause oder sofort – entsperrt wird mit deinem Code."),
            div { class: "flex flex-wrap gap-2",
                button {
                    class: btn,
                    onclick: move |_| {
                        crate::util::native("settings:open");
                    },
                    icons::Settings2 {}
                    {tr("Code-Sperre einstellen …")}
                }
                button {
                    class: btn,
                    onclick: move |_| {
                        crate::util::native("lock:now");
                    },
                    icons::Lock {}
                    {tr("Jetzt sperren")}
                }
            }
        }
        Section { title: tr("Ordner"), description: tr("Daten der App (Datenbank, Konten, Arbeitsbereiche) und ihr Protokoll."),
            div { class: "flex flex-wrap gap-2",
                button {
                    class: btn,
                    onclick: move |_| {
                        crate::util::native("app:data");
                    },
                    icons::FolderOpen {}
                    {tr("Datenordner öffnen")}
                }
                button {
                    class: btn,
                    onclick: move |_| {
                        crate::util::native("app:logs");
                    },
                    icons::FileText {}
                    {tr("Protokoll öffnen")}
                }
            }
        }
    }
}

/// Benachrichtigungen (früher die Glocke oben rechts).
#[component]
fn NotifySection() -> Element {
    use crate::notify::{MODE, Mode, set_mode};
    let mode = MODE();
    rsx! {
        Section { title: tr("Benachrichtigungen"), description: tr("Bei Rückfragen, wenn ein Agent fertig ist, bei Fehlern und Rate-Limits."),
            div { class: "grid max-w-md gap-2",
                for m in Mode::ALL {
                    button {
                        class: if m == mode { "flex items-start gap-2.5 rounded-lg border border-ring bg-accent px-4 py-2.5 text-left" } else { "flex items-start gap-2.5 rounded-lg border px-4 py-2.5 text-left hover:bg-accent/50" },
                        onclick: move |_| set_mode(m),
                        span { class: "min-w-0 flex-1",
                            span { class: "block text-sm font-medium", {m.label()} }
                            span { class: "block text-xs text-muted-foreground", {m.hint()} }
                        }
                        if m == mode {
                            icons::Check { class: "mt-0.5 size-4 shrink-0" }
                        }
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Mobil

/// Kopplung mit nrzCodeMobile (App auf dem Umbrel): Adresse und Code aus nrzCodeMobile eingeben;
/// danach verbindet sich nrzCode von selbst (ausgehend) und hält die Verbindung.
#[component]
fn MobileSection() -> Element {
    let mut status = use_signal(|| None::<MobileStatus>);
    let mut url = use_signal(String::new);
    let mut code = use_signal(String::new);
    let mut name = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    use_future(move || async move {
        loop {
            if let Ok(s) = api::get::<MobileStatus>("/mobile").await {
                status.set(Some(s));
            }
            gloo_timers::future::TimeoutFuture::new(4000).await;
        }
    });
    let submit = move |e: FormEvent| {
        e.prevent_default();
        // „Adresse Code“ in einem Feld eingefügt: aufteilen
        let (mut u, mut c) = (url().trim().to_string(), code().trim().to_string());
        if c.is_empty() {
            if let Some((a, b)) = u.clone().rsplit_once(char::is_whitespace) {
                u = a.trim().to_string();
                c = b.trim().to_string();
            }
        }
        busy.set(true);
        error.set(None);
        spawn(async move {
            match api::post::<MobileStatus, _>("/mobile", &MobilePair { url: u, code: c, name: name() }).await {
                Ok(s) => {
                    status.set(Some(s));
                    code.set(String::new());
                }
                Err(e) => error.set(Some(e.message)),
            }
            busy.set(false);
        });
    };
    let unpair = move |_| {
        spawn(async move {
            match api::delete_json::<MobileStatus>("/mobile").await {
                Ok(s) => status.set(Some(s)),
                Err(e) => toast_error(e),
            }
        });
    };
    let s = status();
    rsx! {
        Section {
            title: tr("nrzCodeMobile"),
            description: tr("Chats von unterwegs im Blick behalten, Limits sehen, Agenten stoppen und weiter anweisen – mit der App nrzCodeMobile aus dem nrzStore auf deinem Umbrel. Dieser PC verbindet sich von sich aus dorthin, er braucht keinen offenen Port."),
            div { class: "grid max-w-lg gap-4",
                match s {
                    None => rsx! { Loading {} },
                    Some(st) if st.configured => rsx! {
                        div { class: "flex items-center gap-3 rounded-lg border px-3 py-2.5",
                            span { class: if st.connected { "size-2.5 shrink-0 rounded-full bg-success" } else { "size-2.5 shrink-0 rounded-full bg-warning animate-pulse" } }
                            div { class: "min-w-0 flex-1",
                                p { class: "text-sm font-medium", if st.connected { {tr("Verbunden")} } else { {tr("Verbindet …")} } }
                                p { class: "truncate text-xs text-muted-foreground", "{st.name} · {st.url}" }
                                if !st.connected && !st.error.is_empty() {
                                    p { class: "mt-0.5 text-xs text-destructive", "{st.error}" }
                                }
                            }
                            button { class: "{btn(BTN_OUTLINE)} {BTN_SM}", onclick: unpair, {tr("Trennen")} }
                        }
                    },
                    Some(_) => rsx! {
                        ol { class: "list-decimal space-y-1 pl-5 text-sm text-muted-foreground",
                            li { {tr("nrzCodeMobile aus dem nrzStore auf dem Umbrel installieren, öffnen (auch am Handy) und anmelden – beim ersten Mal das eigene Konto anlegen.")} }
                            li { {tr("Dort „PC verbinden“ wählen – es zeigt eine Adresse und einen Code.")} }
                            li { {tr("Beides hier eintragen und koppeln.")} }
                        }
                        form { class: "grid gap-3", onsubmit: submit,
                            Field { label: tr("Adresse"), hint: tr("z. B. http://umbrel.local:8471"),
                                input { class: INPUT, placeholder: "http://umbrel.local:8471", value: "{url}", oninput: move |e| url.set(e.value()) }
                            }
                            Field { label: tr("Code"),
                                input { class: "{INPUT} font-mono uppercase tracking-widest", placeholder: "ABCD-1234", value: "{code}", oninput: move |e| code.set(e.value()) }
                            }
                            Field { label: tr("Name dieses PCs"), hint: tr("Leer = Rechnername"),
                                input { class: INPUT, value: "{name}", oninput: move |e| name.set(e.value()) }
                            }
                            if let Some(e) = error() {
                                Callout { text: e, error: true }
                            }
                            div {
                                button { class: btn(BTN_PRIMARY), r#type: "submit", disabled: busy(),
                                    if busy() { icons::LoaderCircle { class: "animate-spin" } } else { icons::Smartphone {} }
                                    {tr("Koppeln")}
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}
