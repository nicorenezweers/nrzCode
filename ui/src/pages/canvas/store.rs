//! Zustand des Canvas-Modus: offener Arbeitsbereich, Fläche (vom Server), Kamera, Werkzeug und
//! Auswahl. Laden und Speichern, Fenster passend zu den Threads (synchron mit dem Chat-Modus),
//! freie Plätze für neue Fenster, Anordnen und Kamerafahrten.

use crate::api;
use crate::components::overlay::toast_error;
use crate::state::{PROJECTS, THREADS};
use crate::util;
use dioxus::core::spawn_forever;
use dioxus::prelude::*;
use wasm_bindgen::JsCast;
use nrzcode_shared::canvas::{CanvasBoard, CanvasLayout, CanvasRev, CanvasShape, CanvasShapesChange, CanvasWindow, MAX_WINDOWS};
use nrzcode_shared::{ThreadStatus, ThreadSummary};

const WS_KEY: &str = "nrz-canvas-ws";

/// Offener Arbeitsbereich: Projekt-ID, `0` = ohne Projekt, `-1` = noch nicht gewählt.
pub static WS: GlobalSignal<i64> = Signal::global(|| util::storage_get(WS_KEY).and_then(|s| s.parse().ok()).unwrap_or(-1));
/// Fläche des offenen Arbeitsbereichs (None = lädt).
pub static BOARD: GlobalSignal<Option<CanvasBoard>> = Signal::global(|| None);
/// Erhöht nach jedem Laden (für den Abgleich mit den Threads).
pub static BOARD_GEN: GlobalSignal<u64> = Signal::global(|| 0);
pub static CAM: GlobalSignal<Cam> = Signal::global(Cam::default);
/// Kamera weich bewegen (Anordnen, zu einem Fenster fliegen).
pub static ANIM: GlobalSignal<bool> = Signal::global(|| false);
/// Ausgewähltes Fenster bzw. ausgewählte Form.
pub static SELECTED: GlobalSignal<Option<String>> = Signal::global(|| None);
pub static SHAPE_SEL: GlobalSignal<Option<String>> = Signal::global(|| None);
/// Text oder Notiz, die gerade geschrieben wird.
pub static EDITING: GlobalSignal<Option<String>> = Signal::global(|| None);
pub static TOOL: GlobalSignal<Tool> = Signal::global(|| Tool::Select);
/// Farbe neuer Formen (`""` = Vordergrund, passt zu hell und dunkel).
pub static COLOR: GlobalSignal<String> = Signal::global(String::new);
pub static STROKE: GlobalSignal<f64> = Signal::global(|| 2.0);
/// Laufende Zieh-Aktion (dann fängt eine Ebene über allem die Zeigerbewegungen).
pub static DRAG: GlobalSignal<Option<Drag>> = Signal::global(|| None);
/// Fenster gleiten weich an neue Plätze (Anordnen, Dieter) – nur kurz danach, nie nach dem Ziehen
/// (sonst springt ein losgelassenes Fenster zurück und gleitet dann an den neuen Platz).
pub static GLIDE: GlobalSignal<bool> = Signal::global(|| false);
/// Form, die gerade verschoben oder in der Grösse geändert wird.
pub static LIVE_SHAPE: GlobalSignal<Option<CanvasShape>> = Signal::global(|| None);
/// Kontextmenü der Fläche (Bildschirmposition).
pub static MENU: GlobalSignal<Option<(f64, f64)>> = Signal::global(|| None);
/// Dialog „Skizze an einen Chat geben“.
pub static SKETCH_OPEN: GlobalSignal<bool> = Signal::global(|| false);
/// Vollbild (App bzw. Browser).
pub static FULLSCREEN: GlobalSignal<bool> = Signal::global(|| false);

/// Nach dem Laden: zu diesem Fenster fliegen bzw. einen neuen Chat öffnen.
static PENDING_FOCUS: GlobalSignal<Option<String>> = Signal::global(|| None);
static PENDING_NEW: GlobalSignal<bool> = Signal::global(|| false);
static SAVE_GEN: GlobalSignal<u64> = Signal::global(|| 0);
/// Fensteränderungen noch nicht gespeichert.
static DIRTY: GlobalSignal<bool> = Signal::global(|| false);
static UNDO: GlobalSignal<Vec<Vec<CanvasShape>>> = Signal::global(Vec::new);
static REDO: GlobalSignal<Vec<Vec<CanvasShape>>> = Signal::global(Vec::new);

/// Grössen neuer Fenster (Breite, Höhe).
pub fn default_size(kind: &str) -> (f64, f64) {
    match kind {
        "terminal" => (680.0, 430.0),
        "browser" => (980.0, 660.0),
        "new" => (540.0, 440.0),
        _ => (540.0, 680.0),
    }
}

pub const GAP: f64 = 48.0;
const MIN_Z: f64 = 0.1;
const MAX_Z: f64 = 2.0;

/// Kamera: Bildschirm = Fläche × z + (x, y).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cam {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Default for Cam {
    fn default() -> Self {
        Cam { x: 120.0, y: 90.0, z: 0.8 }
    }
}

impl Cam {
    pub fn to_world(&self, sx: f64, sy: f64) -> (f64, f64) {
        ((sx - self.x) / self.z, (sy - self.y) / self.z)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Hand,
    Pen,
    Rect,
    Ellipse,
    Diamond,
    Arrow,
    Line,
    Text,
    Note,
    Eraser,
}

impl Tool {
    /// Art der Form, die das Werkzeug zeichnet.
    pub fn shape_kind(self) -> &'static str {
        match self {
            Tool::Pen => "pen",
            Tool::Rect => "rect",
            Tool::Ellipse => "ellipse",
            Tool::Diamond => "diamond",
            Tool::Arrow => "arrow",
            Tool::Line => "line",
            Tool::Text => "text",
            Tool::Note => "note",
            _ => "",
        }
    }

    /// Taste dafür.
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "v",
            Tool::Hand => "h",
            Tool::Pen => "p",
            Tool::Rect => "r",
            Tool::Ellipse => "o",
            Tool::Diamond => "d",
            Tool::Arrow => "a",
            Tool::Line => "l",
            Tool::Text => "t",
            Tool::Note => "n",
            Tool::Eraser => "e",
        }
    }

    pub const ALL: [Tool; 11] =
        [Tool::Select, Tool::Hand, Tool::Pen, Tool::Rect, Tool::Ellipse, Tool::Diamond, Tool::Arrow, Tool::Line, Tool::Text, Tool::Note, Tool::Eraser];
}

/// Zieh-Aktionen (Bildschirmkoordinaten beim Start).
#[derive(Clone, PartialEq, Debug)]
pub enum Drag {
    /// `right`: mit der rechten Maustaste (ohne Bewegung öffnet das Loslassen das Kontextmenü)
    Pan { sx: f64, sy: f64, cam: Cam, right: bool },
    Window { id: String, sx: f64, sy: f64, x: f64, y: f64, w: f64, h: f64 },
    Resize { id: String, sx: f64, sy: f64, x: f64, y: f64, w: f64, h: f64 },
    Draw(CanvasShape),
    Shape { sx: f64, sy: f64, orig: CanvasShape, moved: bool },
    ShapeResize { sx: f64, sy: f64, orig: CanvasShape },
    Erase(Vec<String>),
}

/// Lage eines Fensters während des Ziehens.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveWin {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Fenster beim nächsten Umsetzen weich gleiten lassen (Anordnen, Dieter).
pub fn glide_windows() {
    *GLIDE.write() = true;
    GLIDE_GEN.with(|g| g.set(g.get().wrapping_add(1)));
    let n = GLIDE_GEN.with(|g| g.get());
    spawn_forever(async move {
        gloo_timers::future::TimeoutFuture::new(420).await;
        if GLIDE_GEN.with(|g| g.get()) == n {
            *GLIDE.write() = false;
        }
    });
}

// ------------------------------------------------------------------ Kamera ohne Neuzeichnen

/// Element der Welt (Zeichnung und Fenster), das mit der Kamera verschoben wird.
pub const WORLD_ID: &str = "nrz-canvas-world";

thread_local! {
    static GLIDE_GEN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// Kamera, die schon im DOM steht, aber noch nicht in [`CAM`] (Verschieben, Rad, Pinch)
    static PENDING_CAM: std::cell::Cell<Option<Cam>> = const { std::cell::Cell::new(None) };
    static COMMIT_GEN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Abstand der Rasterpunkte in Bildschirmpunkten.
pub fn grid_size(z: f64) -> f64 {
    let mut g = 24.0 * z;
    while g < 10.0 {
        g *= 4.0;
    }
    g
}

/// Aktuelle Kamera (auch eine, die noch nicht übernommen ist).
pub fn cam_now() -> Cam {
    PENDING_CAM.with(|p| p.get()).unwrap_or_else(|| *CAM.peek())
}

/// Kamera sofort zeigen – direkt im DOM, ohne dass Dioxus neu zeichnet (flüssig bei jeder
/// Zeigerbewegung); in [`CAM`] kommt sie mit [`commit_cam`].
pub fn preview_cam(c: Cam) {
    PENDING_CAM.with(|p| p.set(Some(c)));
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    if let Some(view) = doc.get_element_by_id(super::board::VIEW_ID).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
        let g = grid_size(c.z);
        let st = view.style();
        let _ = st.set_property("background-position", &format!("{}px {}px", c.x, c.y));
        let _ = st.set_property("background-size", &format!("{g}px {g}px"));
        let _ = st.set_property("transition", "none");
    }
    if let Some(world) = doc.get_element_by_id(WORLD_ID).and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
        let st = world.style();
        let _ = st.set_property("transform", &format!("translate3d({}px, {}px, 0) scale({})", c.x, c.y, c.z));
        let _ = st.set_property("transition", "none");
    }
}

/// Gezeigte Kamera übernehmen (zeichnet einmal neu).
pub fn commit_cam() {
    if let Some(c) = PENDING_CAM.with(|p| p.take()) {
        if *CAM.peek() != c {
            *CAM.write() = c;
        }
    }
}

/// Gezeigte Kamera übernehmen und merken, sobald eine Weile nichts mehr kommt (Rad, Pinch).
pub fn commit_cam_soon() {
    let n = COMMIT_GEN.with(|g| {
        let n = g.get().wrapping_add(1);
        g.set(n);
        n
    });
    spawn_forever(async move {
        gloo_timers::future::TimeoutFuture::new(160).await;
        if COMMIT_GEN.with(|g| g.get()) == n {
            commit_cam();
            save_cam();
        }
    });
}

/// Weiche Kamerafahrt sofort beenden – sonst hinkt die Fläche beim Ziehen hinterher.
pub fn stop_glide() {
    if *ANIM.peek() {
        *ANIM.write() = false;
    }
}

// ------------------------------------------------------------------ Arbeitsbereich

/// Projekt eines Arbeitsbereichs.
pub fn project_of(key: i64) -> Option<i64> {
    (key > 0).then_some(key)
}

/// Gültigen Arbeitsbereich sicherstellen (gelöschtes Projekt → ein anderes).
pub fn ensure_ws() {
    let key = *WS.peek();
    let projects = PROJECTS.peek();
    if key == 0 || (key > 0 && projects.contains_key(&key)) {
        return;
    }
    let pick = crate::pages::workspace::parts::NEW_THREAD_PROJECT
        .peek()
        .filter(|p| projects.contains_key(p))
        .or_else(|| projects.values().max_by_key(|p| p.last_activity_at).map(|p| p.id))
        .unwrap_or(0);
    drop(projects);
    set_ws(pick);
}

/// Arbeitsbereich wechseln (lädt die Fläche neu).
pub fn set_ws(key: i64) {
    let key = key.max(0);
    util::storage_set(WS_KEY, &key.to_string());
    *crate::pages::workspace::parts::NEW_THREAD_PROJECT.write() = project_of(key);
    if *WS.peek() == key && BOARD.peek().is_some() {
        return;
    }
    if DIRTY.peek().to_owned() {
        spawn_forever(save_layout());
    }
    *WS.write() = key;
    *BOARD.write() = None;
    *SELECTED.write() = None;
    *SHAPE_SEL.write() = None;
    *EDITING.write() = None;
    UNDO.write().clear();
    REDO.write().clear();
}

/// Fläche laden (vom Board beim Wechsel des Arbeitsbereichs).
pub async fn load(key: i64) {
    match api::get::<CanvasBoard>(&format!("/canvas/{key}")).await {
        Ok(mut b) => {
            if *WS.peek() != key {
                return;
            }
            b.project_id = project_of(key);
            let changed = sync_board(&mut b, &THREADS.peek());
            *BOARD.write() = Some(b);
            *BOARD_GEN.write() += 1;
            restore_cam(key);
            if changed {
                schedule_save();
            }
            if std::mem::take(&mut *PENDING_NEW.write()) {
                new_chat_window();
            }
            // Erst nehmen, dann fliegen (`fly_to` merkt sich ein fehlendes Fenster wieder dort)
            let focus = PENDING_FOCUS.write().take();
            if let Some(id) = focus {
                fly_to(&id);
            }
        }
        Err(e) => toast_error(e),
    }
}

/// Server meldet eine Änderung (z. B. hat ein Agent gezeichnet).
pub fn remote_update(project_id: Option<i64>, rev: i64) {
    let key = project_id.unwrap_or(0);
    if *WS.peek() != key || BOARD.peek().as_ref().is_none_or(|b| b.rev >= rev) {
        return;
    }
    spawn_forever(async move {
        let Ok(fresh) = api::get::<CanvasBoard>(&format!("/canvas/{key}")).await else { return };
        if *WS.peek() != key {
            return;
        }
        let busy = *DIRTY.peek() || DRAG.peek().is_some();
        let mut w = BOARD.write();
        let Some(b) = w.as_mut() else { return };
        if fresh.rev <= b.rev {
            return;
        }
        // Formen immer übernehmen (eine gerade geschriebene, noch nicht gespeicherte bleibt),
        // Fenster nur, wenn hier gerade nichts offen ist
        let editing = EDITING.peek().clone();
        let keep = editing.and_then(|id| b.shapes.iter().find(|s| s.id == id).cloned()).filter(|s| !fresh.shapes.iter().any(|f| f.id == s.id));
        b.shapes = fresh.shapes;
        if let Some(k) = keep {
            b.shapes.push(k);
        }
        b.rev = fresh.rev;
        if !busy {
            b.windows = fresh.windows;
            b.closed = fresh.closed;
            drop(w);
            *BOARD_GEN.write() += 1;
        }
    });
}

// ------------------------------------------------------------------ Fenster ↔ Threads

/// Threads dieses Arbeitsbereichs, die von selbst ein Fenster bekommen (wie in der Seitenleiste:
/// Hauptthreads, nicht ausgeblendet, nicht abgelegt).
fn auto_open(t: &ThreadSummary, key: i64) -> bool {
    t.parent_id.is_none() && t.project_id.unwrap_or(0) == key && !t.hidden && t.status != ThreadStatus::Settled
}

/// Fenster an die Threads anpassen: gelöschte/verschobene/ausgeblendete fallen weg, offene kommen
/// dazu, jedes Fenster hat einen Namen. `true` = geändert.
pub fn sync_board(b: &mut CanvasBoard, threads: &std::collections::BTreeMap<i64, ThreadSummary>) -> bool {
    let key = b.project_id.unwrap_or(0);
    let before = b.windows.len();
    b.windows.retain(|w| match (w.kind.as_str(), w.thread_id) {
        ("chat", Some(id)) => threads.get(&id).is_some_and(|t| t.project_id.unwrap_or(0) == key && !t.hidden && t.parent_id.is_none()),
        ("chat", None) => false,
        _ => true,
    });
    let mut changed = b.windows.len() != before;
    let mut fresh: Vec<&ThreadSummary> =
        threads.values().filter(|t| auto_open(t, key) && !b.closed.contains(&t.id) && !b.windows.iter().any(|w| w.thread_id == Some(t.id))).collect();
    fresh.sort_by_key(|t| t.id);
    for t in fresh {
        if b.windows.len() >= MAX_WINDOWS - 4 {
            break;
        }
        let (w, h) = default_size("chat");
        let (x, y) = free_spot(b, w, h, None);
        let z = top_z(b) + 1;
        b.windows.push(CanvasWindow { id: format!("t{}", t.id), kind: "chat".into(), thread_id: Some(t.id), x, y, w, h, z, ..Default::default() });
        changed = true;
    }
    changed |= ensure_names(b);
    changed
}

fn top_z(b: &CanvasBoard) -> i64 {
    b.windows.iter().map(|w| w.z).max().unwrap_or(0)
}

/// Jedes Fenster bekommt einen eigenen Vornamen (Threads behalten den aus dem Raster).
fn ensure_names(b: &mut CanvasBoard) -> bool {
    let grid = crate::pages::workspace::grid::PANE_NAMES.peek();
    let mut used: Vec<String> = Vec::new();
    let mut changed = false;
    for w in b.windows.iter_mut() {
        let taken = |n: &str, used: &[String]| n.trim().is_empty() || n.eq_ignore_ascii_case("dieter") || used.iter().any(|u| u.eq_ignore_ascii_case(n));
        if taken(&w.name, &used) {
            let from_grid = w.thread_id.and_then(|id| grid.get(&id).cloned()).filter(|n| !taken(n, &used));
            w.name = from_grid.unwrap_or_else(|| crate::pages::workspace::grid::random_name(&used));
            changed = true;
        }
        used.push(w.name.clone());
    }
    changed
}

/// Fenster nach dem Abgleich mit den Threads (läuft bei jeder Änderung der Threads).
pub fn sync_threads() {
    let threads = THREADS.read();
    let _ = BOARD_GEN.read();
    let Some(mut b) = BOARD.peek().clone() else { return };
    if b.project_id.unwrap_or(0) != *WS.peek() {
        return;
    }
    if sync_board(&mut b, &threads) {
        drop(threads);
        *BOARD.write() = Some(b);
        schedule_save();
    }
}

/// Fläche ändern (ohne zu speichern).
pub fn with_board(f: impl FnOnce(&mut CanvasBoard)) {
    if let Some(b) = BOARD.write().as_mut() {
        f(b);
    }
}

/// Fensteränderungen nach einer kurzen Pause speichern.
pub fn schedule_save() {
    *DIRTY.write() = true;
    let round = *SAVE_GEN.peek() + 1;
    *SAVE_GEN.write() = round;
    spawn_forever(async move {
        gloo_timers::future::TimeoutFuture::new(450).await;
        if *SAVE_GEN.peek() == round {
            save_layout().await;
        }
    });
}

async fn save_layout() {
    let Some(b) = BOARD.peek().clone() else { return };
    let key = b.project_id.unwrap_or(0);
    let round = *SAVE_GEN.peek();
    let body = CanvasLayout { windows: b.windows, closed: b.closed };
    match api::put::<CanvasRev, _>(&format!("/canvas/{key}/layout"), &body).await {
        Ok(r) => {
            if *SAVE_GEN.peek() == round {
                *DIRTY.write() = false;
            }
            with_board(|b| {
                if b.project_id.unwrap_or(0) == key {
                    b.rev = b.rev.max(r.rev);
                }
            });
        }
        Err(e) => toast_error(e),
    }
}

/// Formen ändern: sofort hier, dann auf dem Server.
pub fn send_shapes(change: CanvasShapesChange) {
    let key = *WS.peek();
    spawn_forever(async move {
        match api::post::<CanvasRev, _>(&format!("/canvas/{key}/shapes"), &change).await {
            Ok(r) => with_board(|b| {
                if b.project_id.unwrap_or(0) == key {
                    b.rev = b.rev.max(r.rev);
                }
            }),
            Err(e) => toast_error(e),
        }
    });
}

/// Stand der Formen vor einer Änderung merken (Rückgängig).
pub fn remember_shapes() {
    let Some(list) = BOARD.peek().as_ref().map(|b| b.shapes.clone()) else { return };
    let mut u = UNDO.write();
    u.push(list);
    if u.len() > 60 {
        u.remove(0);
    }
    REDO.write().clear();
}

pub fn can_undo() -> bool {
    !UNDO.read().is_empty()
}

pub fn can_redo() -> bool {
    !REDO.read().is_empty()
}

/// Rückgängig / Wiederholen (ersetzt alle Formen).
pub fn undo(redo: bool) {
    let (from, to) = if redo { (&REDO, &UNDO) } else { (&UNDO, &REDO) };
    let Some(prev) = from.write().pop() else { return };
    let Some(cur) = BOARD.peek().as_ref().map(|b| b.shapes.clone()) else { return };
    to.write().push(cur);
    with_board(|b| b.shapes = prev.clone());
    *SHAPE_SEL.write() = None;
    send_shapes(CanvasShapesChange { upsert: prev, delete: Vec::new(), clear: true });
}

/// Neue Form anlegen.
pub fn add_shape(s: CanvasShape) {
    remember_shapes();
    with_board(|b| b.shapes.push(s.clone()));
    send_shapes(CanvasShapesChange { upsert: vec![s], ..Default::default() });
}

/// Form ersetzen (gleiche ID).
pub fn put_shape(s: CanvasShape) {
    with_board(|b| {
        if let Some(x) = b.shapes.iter_mut().find(|x| x.id == s.id) {
            *x = s.clone();
        }
    });
    send_shapes(CanvasShapesChange { upsert: vec![s], ..Default::default() });
}

/// Formen löschen.
pub fn delete_shapes(ids: Vec<String>) {
    if ids.is_empty() {
        return;
    }
    remember_shapes();
    with_board(|b| b.shapes.retain(|s| !ids.contains(&s.id)));
    if SHAPE_SEL.peek().as_ref().is_some_and(|s| ids.contains(s)) {
        *SHAPE_SEL.write() = None;
    }
    send_shapes(CanvasShapesChange { delete: ids, ..Default::default() });
}

/// Zeichnung leeren.
pub fn clear_shapes() {
    remember_shapes();
    with_board(|b| b.shapes.clear());
    *SHAPE_SEL.write() = None;
    send_shapes(CanvasShapesChange { clear: true, ..Default::default() });
}

pub fn new_shape_id() -> String {
    format!("s{:x}{:04x}", js_sys::Date::now() as u64, (js_sys::Math::random() * 65535.0) as u32)
}

// ------------------------------------------------------------------ Fenster

/// Läge ein Fenster an (x, y, w, h) zu nahe an einem anderen Fenster oder verdeckte es einen Teil
/// der Zeichnung (Notizen, Texte, Striche, Pfeile, Ränder)? Im Inneren eines grossen Kastens oder
/// Kreises darf es liegen, nur nicht auf dessen Rand.
fn overlaps(b: &CanvasBoard, x: f64, y: f64, w: f64, h: f64) -> bool {
    const MARGIN: f64 = 24.0;
    let (l, t, r, btm) = (x - MARGIN, y - MARGIN, x + w + MARGIN, y + h + MARGIN);
    let hits = |x0: f64, y0: f64, x1: f64, y1: f64| l < x1 && r > x0 && t < y1 && btm > y0;
    b.windows.iter().any(|o| hits(o.x, o.y, o.x + o.w, o.y + o.h))
        || b.shapes.iter().any(|s| {
            let (x0, y0, x1, y1) = super::draw::bbox(s);
            if !hits(x0, y0, x1, y1) {
                return false;
            }
            match s.kind.as_str() {
                "pen" | "line" | "arrow" if s.points.len() > 1 => s.points.windows(2).any(|p| segment_hits(p[0], p[1], (l, t, r, btm))),
                "rect" | "ellipse" | "diamond" => !(l > x0 && r < x1 && t > y0 && btm < y1),
                _ => true,
            }
        })
}

/// Schneidet die Strecke a–b das Rechteck (l, t, r, b)? (Liang–Barsky)
fn segment_hits(a: [f64; 2], b: [f64; 2], (l, t, r, btm): (f64, f64, f64, f64)) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, a[0] - l), (dx, r - a[0]), (-dy, a[1] - t), (dy, btm - a[1])] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let u = q / p;
            if p < 0.0 {
                t0 = t0.max(u);
            } else {
                t1 = t1.min(u);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// Freier Platz für ein Fenster: möglichst nahe `near` (Fläche), sonst neben den anderen –
/// nie auf einem anderen Fenster oder über der Zeichnung.
pub fn free_spot(b: &CanvasBoard, w: f64, h: f64, near: Option<(f64, f64)>) -> (f64, f64) {
    let (ox, oy) = match near {
        Some((x, y)) => (x - w / 2.0, y - h / 2.0),
        None if b.windows.is_empty() => (0.0, 0.0),
        None => {
            let x0 = b.windows.iter().map(|o| o.x).fold(f64::MAX, f64::min);
            let y0 = b.windows.iter().map(|o| o.y).fold(f64::MAX, f64::min);
            (x0, y0)
        }
    };
    let (cw, ch) = (w + GAP, h + GAP);
    if !overlaps(b, ox, oy, w, h) {
        return (ox, oy);
    }
    // Ringe um den Ausgangspunkt (ohne `near` zeilenweise nach rechts)
    for r in 0..12i32 {
        let mut cells: Vec<(i32, i32)> = Vec::new();
        if near.is_some() {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) == r {
                        cells.push((dx, dy));
                    }
                }
            }
            cells.sort_by_key(|(dx, dy)| (dy.abs() * 2 + dx.abs(), *dy, *dx));
        } else {
            for dy in 0..=r {
                cells.push((r, dy));
            }
            for dx in 0..r {
                cells.push((dx, r));
            }
        }
        for (dx, dy) in cells {
            let (x, y) = (ox + dx as f64 * cw, oy + dy as f64 * ch);
            if !overlaps(b, x, y, w, h) {
                return (x, y);
            }
        }
    }
    // Kein Platz in der Nähe: rechts neben allem
    let right = b.windows.iter().map(|o| o.x + o.w).chain(b.shapes.iter().map(|s| super::draw::bbox(s).2)).fold(ox - GAP, f64::max);
    (right + GAP, oy)
}

/// Mitte des sichtbaren Bereichs (Fläche).
pub fn view_center() -> (f64, f64) {
    let (l, t, r, btm) = avail_rect();
    cam_now().to_world((l + r) / 2.0, (t + btm) / 2.0)
}

/// Fenster hinzufügen (in der Mitte der Ansicht oder an `at`); liefert die ID.
pub fn add_window(kind: &str, thread_id: Option<i64>, url: &str, at: Option<(f64, f64)>) -> Option<String> {
    let mut out = None;
    with_board(|b| {
        if b.windows.len() >= MAX_WINDOWS {
            return;
        }
        if let Some(t) = thread_id {
            b.closed.retain(|c| *c != t);
            if let Some(w) = b.windows.iter().find(|w| w.thread_id == Some(t)) {
                out = Some(w.id.clone());
                return;
            }
        }
        let (w, h) = default_size(kind);
        let near = at.unwrap_or_else(view_center);
        let (x, y) = free_spot(b, w, h, Some(near));
        let id = match thread_id {
            Some(t) => format!("t{t}"),
            None => {
                let n = (1..).find(|n| !b.windows.iter().any(|w| w.id == format!("{kind}-{n}"))).unwrap_or(1);
                format!("{kind}-{n}")
            }
        };
        let kind = if thread_id.is_some() { "chat" } else { kind };
        let z = top_z(b) + 1;
        b.windows.push(CanvasWindow { id: id.clone(), kind: kind.into(), thread_id, url: url.into(), x, y, w, h, z, ..Default::default() });
        ensure_names(b);
        out = Some(id);
    });
    if out.is_some() {
        schedule_save();
    }
    out
}

/// Leeres Chat-Fenster öffnen (beim Laden nachholen) und hinfliegen.
pub fn new_chat_window() {
    if BOARD.peek().is_none() {
        *PENDING_NEW.write() = true;
        return;
    }
    if let Some(id) = add_window("new", None, "", None) {
        *SELECTED.write() = Some(id.clone());
        fly_to(&id);
    }
}

/// In einem leeren Fenster hat ein Chat begonnen: es wird zum Fenster dieses Threads.
pub fn window_created(win: &str, thread_id: i64) {
    with_board(|b| {
        let Some(i) = b.windows.iter().position(|w| w.id == win) else { return };
        // Hat der Abgleich schon ein Fenster für den Thread angelegt: das leere ersetzt es
        if let Some(j) = b.windows.iter().position(|w| w.thread_id == Some(thread_id)) {
            let dup = b.windows.remove(j);
            let i = if j < i { i - 1 } else { i };
            b.windows[i].z = b.windows[i].z.max(dup.z);
        }
        let Some(w) = b.windows.iter_mut().find(|w| w.id == win) else { return };
        w.kind = "chat".into();
        w.thread_id = Some(thread_id);
        w.id = format!("t{thread_id}");
        let (_, h) = default_size("chat");
        w.h = w.h.max(h);
        b.closed.retain(|c| *c != thread_id);
    });
    if SELECTED.peek().as_deref() == Some(win) {
        *SELECTED.write() = Some(format!("t{thread_id}"));
    }
    schedule_save();
}

/// Fenster schliessen (ein Chat bleibt erhalten und taucht nicht von selbst wieder auf).
pub fn close_window(id: &str) {
    let mut terminal = String::new();
    with_board(|b| {
        if let Some(w) = b.windows.iter().find(|w| w.id == id) {
            if let Some(t) = w.thread_id {
                if !b.closed.contains(&t) {
                    b.closed.push(t);
                }
            }
            if w.kind == "terminal" {
                terminal = w.terminal_id.clone();
            }
        }
        b.windows.retain(|w| w.id != id);
    });
    if SELECTED.peek().as_deref() == Some(id) {
        *SELECTED.write() = None;
    }
    if !terminal.is_empty() {
        spawn_forever(async move {
            let _ = api::delete(&format!("/terminals/{terminal}")).await;
        });
    }
    schedule_save();
}

/// Fenster nach vorn holen und auswählen.
pub fn raise(id: &str) {
    if SELECTED.peek().as_deref() != Some(id) {
        *SELECTED.write() = Some(id.to_string());
    }
    if SHAPE_SEL.peek().is_some() {
        *SHAPE_SEL.write() = None;
    }
    let mut changed = false;
    with_board(|b| {
        // Stapelfolge klein halten (sonst wächst sie mit jedem Klick)
        if top_z(b) > 5000 {
            let mut order: Vec<usize> = (0..b.windows.len()).collect();
            order.sort_by_key(|i| b.windows[*i].z);
            for (rank, i) in order.into_iter().enumerate() {
                b.windows[i].z = rank as i64 + 1;
            }
        }
        let top = top_z(b);
        let shared_top = b.windows.iter().filter(|o| o.z == top).count() > 1;
        if let Some(w) = b.windows.iter_mut().find(|w| w.id == id) {
            if w.z < top || shared_top {
                w.z = top + 1;
                changed = true;
            }
        }
    });
    if changed {
        schedule_save();
    }
}

pub fn window(id: &str) -> Option<CanvasWindow> {
    BOARD.peek().as_ref().and_then(|b| b.windows.iter().find(|w| w.id == id).cloned())
}

pub fn window_by_name(name: &str) -> Option<CanvasWindow> {
    BOARD.peek().as_ref().and_then(|b| b.windows.iter().find(|w| w.name.eq_ignore_ascii_case(name.trim())).cloned())
}

pub fn update_window(id: &str, f: impl FnOnce(&mut CanvasWindow)) {
    with_board(|b| {
        if let Some(w) = b.windows.iter_mut().find(|w| w.id == id) {
            f(w);
        }
    });
}

/// Alle Fenster ordentlich anordnen: Chats (ältere zuerst), dann leere Chats, Terminals,
/// Browser – in Spalten wie eine Tabelle; danach alles ins Bild.
pub fn arrange() {
    with_board(|b| {
        let rank = |w: &CanvasWindow| match w.kind.as_str() {
            "chat" => 0,
            "new" => 1,
            "terminal" => 2,
            _ => 3,
        };
        let mut order: Vec<usize> = (0..b.windows.len()).collect();
        order.sort_by_key(|i| (rank(&b.windows[*i]), b.windows[*i].thread_id.unwrap_or(i64::MAX), b.windows[*i].id.clone()));
        let n = order.len();
        if n == 0 {
            return;
        }
        let cols = match n {
            1..=3 => n,
            4 => 2,
            5..=9 => 3,
            10..=16 => 4,
            _ => 5,
        };
        let rows = n.div_ceil(cols);
        let mut col_w = vec![0f64; cols];
        let mut row_h = vec![0f64; rows];
        for (k, i) in order.iter().enumerate() {
            let w = &b.windows[*i];
            col_w[k % cols] = col_w[k % cols].max(w.w);
            row_h[k / cols] = row_h[k / cols].max(w.h);
        }
        // Zeichnungen bleiben, wo sie sind: die Fenster beginnen links oben an ihnen vorbei
        let (x0, y0) = (0.0, 0.0);
        for (k, i) in order.iter().enumerate() {
            let (c, r) = (k % cols, k / cols);
            let x = x0 + col_w[..c].iter().sum::<f64>() + GAP * c as f64;
            let y = y0 + row_h[..r].iter().sum::<f64>() + GAP * r as f64;
            let w = &mut b.windows[*i];
            w.x = x;
            w.y = y;
        }
    });
    glide_windows();
    schedule_save();
    fit_all();
}

// ------------------------------------------------------------------ Kamera

/// Bereich ohne die schwebenden Leisten (links, oben, rechts, unten) in Bildschirmpunkten.
pub fn avail_rect() -> (f64, f64, f64, f64) {
    let (w, h) = viewport();
    (76.0, 64.0, w - 24.0, h - 92.0)
}

pub fn viewport() -> (f64, f64) {
    let win = web_sys::window();
    let w = win.as_ref().and_then(|w| w.inner_width().ok()).and_then(|v| v.as_f64()).unwrap_or(1280.0);
    let h = win.as_ref().and_then(|w| w.inner_height().ok()).and_then(|v| v.as_f64()).unwrap_or(800.0);
    (w, h)
}

fn cam_key(key: i64) -> String {
    format!("nrz-canvas-cam-{key}")
}

pub fn save_cam() {
    let c = cam_now();
    util::storage_set(&cam_key(*WS.peek()), &format!("{:.1},{:.1},{:.4}", c.x, c.y, c.z));
}

fn restore_cam(key: i64) {
    let saved = util::storage_get(&cam_key(key)).and_then(|s| {
        let v: Vec<f64> = s.split(',').filter_map(|x| x.parse().ok()).collect();
        (v.len() == 3 && v[2] > 0.0).then(|| Cam { x: v[0], y: v[1], z: v[2].clamp(MIN_Z, MAX_Z) })
    });
    match saved {
        Some(c) => *CAM.write() = c,
        None => fit_all_now(false),
    }
}

/// Kamera setzen (auf Wunsch weich).
pub fn set_cam(c: Cam, animate: bool) {
    PENDING_CAM.with(|p| p.set(None));
    let c = Cam { z: c.z.clamp(MIN_Z, MAX_Z), ..c };
    if animate {
        *ANIM.write() = true;
        spawn_forever(async {
            gloo_timers::future::TimeoutFuture::new(420).await;
            *ANIM.write() = false;
        });
    }
    *CAM.write() = c;
    save_cam();
}

/// Um einen Bildschirmpunkt zoomen.
/// Um einen Bildschirmpunkt zoomen (Rad, Pinch: sofort gezeigt, gesammelt übernommen).
pub fn zoom_at(sx: f64, sy: f64, factor: f64) {
    let c = cam_now();
    let z = (c.z * factor).clamp(MIN_Z, MAX_Z);
    let k = z / c.z;
    preview_cam(Cam { x: sx - (sx - c.x) * k, y: sy - (sy - c.y) * k, z });
    commit_cam_soon();
}

/// Zoom um die Mitte der Ansicht.
pub fn zoom_step(factor: f64) {
    let (l, t, r, b) = avail_rect();
    let c = cam_now();
    let z = (c.z * factor).clamp(MIN_Z, MAX_Z);
    let (sx, sy) = ((l + r) / 2.0, (t + b) / 2.0);
    let k = z / c.z;
    set_cam(Cam { x: sx - (sx - c.x) * k, y: sy - (sy - c.y) * k, z }, true);
}

/// Rahmen (Fläche) ins Bild holen, höchstens mit `max_z`.
pub fn fit_rect(x0: f64, y0: f64, x1: f64, y1: f64, max_z: f64, animate: bool) {
    let (l, t, r, b) = avail_rect();
    let (aw, ah) = ((r - l).max(200.0), (b - t).max(200.0));
    let (w, h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
    let z = (aw / w).min(ah / h).clamp(MIN_Z, max_z);
    let x = l + (aw - w * z) / 2.0 - x0 * z;
    let y = t + (ah - h * z) / 2.0 - y0 * z;
    set_cam(Cam { x, y, z }, animate);
}

/// Alles (Fenster und Zeichnung) ins Bild.
pub fn fit_all() {
    fit_all_now(true);
}

fn fit_all_now(animate: bool) {
    let Some(b) = BOARD.peek().clone() else { return };
    let mut rects: Vec<(f64, f64, f64, f64)> = b.windows.iter().map(|w| (w.x, w.y, w.x + w.w, w.y + w.h)).collect();
    rects.extend(b.shapes.iter().map(super::draw::bbox));
    if rects.is_empty() {
        set_cam(Cam::default(), animate);
        return;
    }
    let x0 = rects.iter().map(|r| r.0).fold(f64::MAX, f64::min);
    let y0 = rects.iter().map(|r| r.1).fold(f64::MAX, f64::min);
    let x1 = rects.iter().map(|r| r.2).fold(f64::MIN, f64::max);
    let y1 = rects.iter().map(|r| r.3).fold(f64::MIN, f64::max);
    fit_rect(x0 - 24.0, y0 - 24.0, x1 + 24.0, y1 + 24.0, 1.0, animate);
}

/// Zu einem Fenster fliegen (nach dem Laden nachholen).
pub fn fly_to(id: &str) {
    match window(id) {
        Some(w) => {
            *SELECTED.write() = Some(w.id.clone());
            fit_rect(w.x - 16.0, w.y - 16.0, w.x + w.w + 16.0, w.y + w.h + 16.0, 1.0, true);
        }
        None => *PENDING_FOCUS.write() = Some(id.to_string()),
    }
}

/// Vollbild umschalten: in der App das Fenster, im Browser die Seite.
pub fn toggle_fullscreen() {
    let on = !*FULLSCREEN.peek();
    if util::in_app() {
        util::native("window:fullscreen");
        *FULLSCREEN.write() = on;
        return;
    }
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    if doc.fullscreen_element().is_some() {
        doc.exit_fullscreen();
        *FULLSCREEN.write() = false;
    } else if let Some(el) = doc.document_element() {
        if el.request_fullscreen().is_ok() {
            *FULLSCREEN.write() = true;
        }
    }
}

// ------------------------------------------------------------------ Wechsel zwischen den Modi

/// Thread als Fenster öffnen (Arbeitsbereich seines Projekts) und hinfliegen.
pub fn open_thread(id: i64) {
    let Some(t) = THREADS.peek().get(&id).cloned() else { return };
    // Subagenten: der Hauptthread bekommt das Fenster
    let mut root = t;
    while let Some(p) = root.parent_id.and_then(|p| THREADS.peek().get(&p).cloned()) {
        root = p;
    }
    let key = root.project_id.unwrap_or(0);
    let win = format!("t{}", root.id);
    if *WS.peek() != key || BOARD.peek().is_none() {
        set_ws(key);
        // Erst nach dem Laden: Fenster sicherstellen und hinfliegen
        let rid = root.id;
        spawn_forever(async move {
            for _ in 0..100 {
                if BOARD.peek().is_some() {
                    break;
                }
                gloo_timers::future::TimeoutFuture::new(50).await;
            }
            add_window("chat", Some(rid), "", None);
            fly_to(&format!("t{rid}"));
        });
        return;
    }
    add_window("chat", Some(root.id), "", None);
    fly_to(&win);
}

/// Arbeitsbereich öffnen, auf Wunsch mit einem leeren Chat-Fenster.
pub fn open_workspace(project: Option<i64>, new_chat: bool) {
    set_ws(project.unwrap_or(0));
    if new_chat {
        if BOARD.peek().is_some() {
            new_chat_window();
        } else {
            *PENDING_NEW.write() = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: &str, x: f64, y: f64) -> CanvasWindow {
        CanvasWindow { id: id.into(), kind: "terminal".into(), x, y, w: 500.0, h: 400.0, ..Default::default() }
    }

    #[test]
    fn free_spots_do_not_overlap() {
        let mut b = CanvasBoard::default();
        assert_eq!(free_spot(&b, 500.0, 400.0, None), (0.0, 0.0));
        b.windows.push(win("a", 0.0, 0.0));
        let (x, y) = free_spot(&b, 500.0, 400.0, None);
        assert!(!overlaps(&b, x, y, 500.0, 400.0), "{x},{y}");
        // Nahe der Mitte der Ansicht, aber nicht auf einem anderen Fenster
        let (x, y) = free_spot(&b, 500.0, 400.0, Some((250.0, 200.0)));
        assert!(!overlaps(&b, x, y, 500.0, 400.0), "{x},{y}");
        b.windows.push(win("b", x, y));
        let (x2, y2) = free_spot(&b, 500.0, 400.0, Some((250.0, 200.0)));
        assert!(!overlaps(&b, x2, y2, 500.0, 400.0));
    }

    #[test]
    fn free_spots_keep_drawings_visible() {
        let shape = |kind: &str, x: f64, y: f64, w: f64, h: f64| CanvasShape { id: kind.into(), kind: kind.into(), x, y, w, h, ..Default::default() };
        let apart = |(x, y): (f64, f64), (x0, y0, x1, y1): (f64, f64, f64, f64)| x >= x1 || x + 500.0 <= x0 || y >= y1 || y + 400.0 <= y0;
        // Notiz mitten in der Ansicht: das Fenster weicht aus, die Notiz bleibt sichtbar
        let mut b = CanvasBoard::default();
        b.shapes.push(shape("note", 0.0, 0.0, 240.0, 150.0));
        let p = free_spot(&b, 500.0, 400.0, Some((120.0, 75.0)));
        assert!(apart(p, (0.0, 0.0, 240.0, 150.0)), "{p:?}");
        // Ohne Fenster und ohne `near` ebenso
        let p = free_spot(&b, 500.0, 400.0, None);
        assert!(apart(p, (0.0, 0.0, 240.0, 150.0)), "{p:?}");
        // Im Inneren eines grossen Rahmens ist Platz …
        let mut b = CanvasBoard::default();
        b.shapes.push(shape("rect", -2000.0, -2000.0, 4000.0, 4000.0));
        assert_eq!(free_spot(&b, 500.0, 400.0, Some((0.0, 0.0))), (-250.0, -200.0));
        // … aber nicht auf einem Pfeil quer durch die Mitte
        b.shapes.push(CanvasShape { id: "a".into(), kind: "arrow".into(), points: vec![[-1000.0, 0.0], [1000.0, 0.0]], ..Default::default() });
        let (x, y) = free_spot(&b, 500.0, 400.0, Some((0.0, 0.0)));
        assert!(y > 0.0 || y + 400.0 < 0.0, "{x},{y}");
        // Ein schräger Strich, dessen Rahmen das Fenster nur streift, stört nicht
        assert!(!segment_hits([0.0, 1000.0], [1000.0, 0.0], (0.0, 0.0, 100.0, 100.0)));
        assert!(segment_hits([0.0, 150.0], [150.0, 0.0], (0.0, 0.0, 100.0, 100.0)));
    }

    #[test]
    fn camera_maps_screen_to_world() {
        let c = Cam { x: 100.0, y: 50.0, z: 2.0 };
        assert_eq!(c.to_world(100.0, 50.0), (0.0, 0.0));
        assert_eq!(c.to_world(300.0, 250.0), (100.0, 100.0));
    }
}
