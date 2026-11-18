//! Canvas-Modus: eine unendliche Fläche je Arbeitsbereich (Projekt) mit frei schwebenden
//! Fenstern – Chats, Terminals, Browser – und einer Zeichenebene (Striche, Formen, Pfeile, Text,
//! Notizen), die auch die Agenten lesen und beschreiben (`/api/v1/canvas/<project>`, MCP-Werkzeuge
//! `canvas_read` und `canvas_draw`). Die Chats sind dieselben Threads wie im Chat-Modus.

use serde::{Deserialize, Serialize};

/// Höchstzahlen je Fläche.
pub const MAX_WINDOWS: usize = 48;
pub const MAX_SHAPES: usize = 3000;
pub const MAX_POINTS: usize = 4000;

/// Ein Fenster auf der Fläche.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CanvasWindow {
    /// Eindeutig auf der Fläche: `t<thread>`, `new-<n>`, `term-<n>`, `web-<n>`
    pub id: String,
    /// `chat` | `new` (neuer Chat, noch ohne Thread) | `terminal` | `browser`
    pub kind: String,
    pub thread_id: Option<i64>,
    /// Terminal: ID aus `POST /terminals`
    pub terminal_id: String,
    /// Browser: aktuelle Adresse
    pub url: String,
    /// Vorname, mit dem Dieter das Fenster anspricht („Lena“)
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Stapelfolge (grösser = weiter oben)
    pub z: i64,
}

/// Eine Form der Zeichenebene (Koordinaten der Fläche).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CanvasShape {
    pub id: String,
    /// `pen` | `line` | `arrow` | `rect` | `ellipse` | `diamond` | `text` | `note`
    pub kind: String,
    /// `pen`: Strich; `line`/`arrow`: Anfang und Ende
    pub points: Vec<[f64; 2]>,
    /// Rahmen (`rect`, `ellipse`, `diamond`, `text`, `note`)
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub text: String,
    /// Farbe als `#rrggbb`
    pub color: String,
    /// Strichstärke
    pub size: f64,
    /// Gefüllt (leicht getönt)
    pub fill: bool,
    /// `user` oder der Thread/Agent, der sie gezeichnet hat
    pub author: String,
    pub created_at: i64,
}

/// Fläche eines Arbeitsbereichs (`project_id` None = ohne Projekt).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CanvasBoard {
    pub project_id: Option<i64>,
    pub windows: Vec<CanvasWindow>,
    /// Threads, deren Fenster geschlossen wurde – sie erscheinen nicht von selbst wieder
    pub closed: Vec<i64>,
    pub shapes: Vec<CanvasShape>,
    /// Stand (jede Änderung +1)
    pub rev: i64,
    pub updated_at: i64,
}

/// `PUT /canvas/<project>/layout` – Fenster und geschlossene Threads (gehören der Oberfläche).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CanvasLayout {
    pub windows: Vec<CanvasWindow>,
    pub closed: Vec<i64>,
}

/// `POST /canvas/<project>/shapes` – Formen anlegen/ändern (gleiche ID) und löschen.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CanvasShapesChange {
    pub upsert: Vec<CanvasShape>,
    pub delete: Vec<String>,
    /// Alle Formen löschen (vor `upsert`)
    pub clear: bool,
}

/// Antwort auf Änderungen: neuer Stand.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CanvasRev {
    pub rev: i64,
}
