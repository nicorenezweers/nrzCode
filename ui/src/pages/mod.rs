//! Seiten je Bereich (je ein Ordner, eigene Unter-Module): `workspace`, `agents`, `canvas`.

pub mod agents;
pub mod canvas;
pub mod workspace;

use dioxus::prelude::*;

/// Platzhalter für noch nicht gebaute Seiten.
#[component]
pub fn Placeholder(#[props(into)] title: String) -> Element {
    rsx! {
        div { class: "p-8",
            h1 { class: "text-2xl font-semibold", "{title}" }
        }
    }
}
