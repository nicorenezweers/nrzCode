//! Rahmen der App (`AppLayout`: Seitenleiste mit Threads nach Zustand, Kopfzeile mit Knöpfen
//! für die Spalten), Anmeldung, Startseite, Projekte, Thread-Seite mit rechter Spalte (Umgebung,
//! Worktree, Branch, Commit, Änderungen, Vorschau, Subagenten) und Terminal, Einstellungen.
//!
//! * `layout` – `AppLayout`, Seitenleiste, Kopfzeile, Benutzermenü, Statistik
//! * `auth` – `Login` (mit Registrieren), `Setup`
//! * `bots` – Bot-Modus: `Bots`, `BotPage`, Liste in der Seitenleiste, Editor (Profil, Gedächtnis, Routinen)
//! * `home` – `Home` (Begrüssung, Projekt, Composer, letzte Threads)
//! * `projects` – `Projects`, `ProjectPage`, Dialog „Projekt hinzufügen“
//! * `grid` – `Grid` (mehrere Threads nebeneinander, parallele Sitzungen, Fenster mit Namen)
//! * `dieter` – Sprachsteuerung „Dieter“ im Raster und ihre Einstellungen
//! * `tray` – `Tray` (Fenster der Menüleiste in der macOS-App: Threads und Nutzung)
//! * `thread` – `ThreadPage` (Unterhaltung, rechte Spalte, Vorschau-Paneel, Terminal)
//! * `git` – Branch, Änderungen, Commit, Pull Request
//! * `sidepanel` – rechte Spalte mit Reitern (Dateien, Suche, Übersicht, Notizblock, Team)
//! * `team` – Reiter „Team“: Mitglieder, Agenten der anderen, gemeinsame Nachrichten
//! * `editor` – Dateibaum und Editor („Open in editor“)
//! * `settings`, `admin` – `Settings` mit Abschnitten
//! * `parts` – kleine Bausteine (Projekt-Abzeichen, Zeiten, Spalten, Laden)

mod admin;
mod auth;
pub mod bots;
pub mod cli;
pub mod dieter;
mod editor;
mod git;
pub mod grid;
mod home;
pub mod layout;
pub mod parts;
mod projects;
mod settings;
mod sidepanel;
mod team;
mod thread;
mod tray;
pub mod update;

pub use auth::{Login, Setup};
pub use bots::{BotPage, Bots};
pub use grid::Grid;
pub use home::Home;
pub use tray::Tray;
pub use layout::AppLayout;
pub use projects::{ProjectPage, Projects};
pub use settings::Settings;
pub use thread::ThreadPage;

use crate::Route;
use crate::i18n::tr;
use dioxus::prelude::*;

#[component]
pub fn NotFound(segments: Vec<String>) -> Element {
    let path = format!("/{}", segments.join("/"));
    rsx! {
        div { class: "flex min-h-screen items-center justify-center bg-background p-6",
            div { class: "max-w-sm text-center",
                img { src: "/static/icon.png", class: "mx-auto mb-4 size-12 rounded-xl", alt: "" }
                h1 { class: "text-xl font-semibold", {tr("Diese Seite gibt es nicht.")} }
                p { class: "mt-2 break-all font-mono text-xs text-muted-foreground", "{path}" }
                Link { to: Route::Home {}, class: "mt-6 inline-flex h-9 items-center gap-2 rounded-md bg-primary px-4 text-sm font-medium text-primary-foreground hover:bg-primary/90",
                    icons::House { class: "size-4" }
                    {tr("Zur Startseite")}
                }
            }
        }
    }
}
