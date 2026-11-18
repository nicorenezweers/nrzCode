//! Canvas-Modus (neben dem Chat-Modus, umschaltbar im Benutzermenü): alle Fenster eines
//! Arbeitsbereichs (Projekts) schweben frei auf einer unendlichen Fläche – Chats, Terminals,
//! Browser. Die Chats sind dieselben Threads wie im Chat-Modus (neue erscheinen hier, hier
//! begonnene dort). Oben die Arbeitsbereiche, links Zeichnen wie in Excalidraw (die Agenten lesen
//! und zeichnen mit), unten Dieter, der alles per Sprache steuert.
//!
//! * `store` – Zustand, Laden/Speichern (`/api/v1/canvas/<project>`), Fenster ↔ Threads, Kamera
//! * `board` – Fläche: Verschieben, Zoomen, Zeichnen, Tastenkürzel
//! * `window` – die Fenster („Windows“) und ihre Inhalte
//! * `draw` – Formen der Zeichenebene
//! * `shell` – Rahmen: obere Leiste, Werkzeuge, Zoom, Dieter
//! * `voice` – Dieters Befehle auf der Canvas

mod board;
mod draw;
mod shell;
mod store;
mod voice;
mod window;

pub use shell::CanvasShell;
pub use store::{new_chat_window, open_thread, open_workspace, remote_update};
pub use voice::{dieter_action, dieter_names, dieter_request};

use crate::pages::workspace::parts::NEW_THREAD_PROJECT;
use crate::Route;
use dioxus::prelude::*;
use store::*;

/// Route `/canvas` – die Fläche zeichnet der Rahmen ([`CanvasShell`]), hier steht nichts.
#[component]
pub fn Canvas() -> Element {
    rsx! {}
}

/// In den Canvas-Modus: Arbeitsbereich passend zur Seite (Thread → sein Projekt und Fenster).
pub fn enter_from(route: &Route) -> Route {
    match route {
        Route::ThreadPage { id } => open_thread(*id),
        Route::ProjectPage { id } => set_ws(*id),
        _ => {
            // Erst lesen, dann wechseln: `set_ws` schreibt NEW_THREAD_PROJECT (eine noch gehaltene
            // Lesesperre liesse die Oberfläche abstürzen – Wechsel aus dem Raster)
            let project = *NEW_THREAD_PROJECT.peek();
            if let Some(p) = project {
                set_ws(p);
            }
        }
    }
    Route::Canvas {}
}

/// Zurück in den Chat-Modus: der ausgewählte Chat, sonst ein neuer Thread im Arbeitsbereich.
pub fn leave_to() -> Route {
    let thread = SELECTED.peek().clone().and_then(|id| window(&id)).and_then(|w| w.thread_id);
    *NEW_THREAD_PROJECT.write() = project_of((*WS.peek()).max(0));
    match thread {
        Some(id) => Route::ThreadPage { id },
        None => Route::Home {},
    }
}
