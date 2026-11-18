//! Canvas-Modus: eine Fläche je Benutzer und Arbeitsbereich (Projekt; `0` = ohne Projekt) mit den
//! Fenstern der Oberfläche (Chats, Terminals, Browser) und einer Zeichenebene, die Benutzer und
//! Agenten teilen. Die Chats sind dieselben Threads wie im Chat-Modus – hier liegt nur, wo welches
//! Fenster steht und was gezeichnet ist.
//!
//! Routen unter `/api/v1`:
//! * `GET /canvas/<project>` – Fläche (Fenster, geschlossene Threads, Formen, Stand)
//! * `PUT /canvas/<project>/layout` – Fenster und geschlossene Threads (nur die Oberfläche)
//! * `POST /canvas/<project>/shapes` – Formen anlegen/ändern/löschen
//!
//! Für Agenten (MCP, siehe `agents::mcp`): [`read_for_agent`] und [`draw_for_agent`]; für Dieter
//! [`describe`]. Jede Änderung meldet `HubEvent::CanvasUpdated`.

use crate::auth::CurrentUser;
use crate::db::now;
use crate::error::{AppError, AppResult};
use crate::{hub, AppState};
use nrzcode_shared::canvas::{CanvasBoard, CanvasLayout, CanvasRev, CanvasShape, CanvasShapesChange, CanvasWindow, MAX_POINTS, MAX_SHAPES, MAX_WINDOWS};
use nrzcode_shared::HubEvent;
use rocket::serde::json::Json;
use rocket::{get, post, put, routes, Route, State};
use serde_json::{json, Value};
use std::sync::LazyLock;

pub fn api_routes() -> Vec<Route> {
    routes![get_board, put_layout, change_shapes]
}

/// Lesen–Ändern–Schreiben nacheinander (Oberfläche und Agenten schreiben gleichzeitig).
static LOCK: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

const KINDS: [&str; 8] = ["pen", "line", "arrow", "rect", "ellipse", "diamond", "text", "note"];
const WINDOW_KINDS: [&str; 4] = ["chat", "new", "terminal", "browser"];
const COORD: f64 = 1_000_000.0;

fn project_id(key: i64) -> Option<i64> {
    (key > 0).then_some(key)
}

/// Gehört das Projekt dem Benutzer? (`0` = ohne Projekt, immer erlaubt)
async fn check_project(state: &AppState, user_id: i64, key: i64) -> AppResult<()> {
    if key <= 0 {
        return Ok(());
    }
    let found: Option<i64> = sqlx::query_scalar("SELECT id FROM projects WHERE id = ? AND user_id = ?").bind(key).bind(user_id).fetch_optional(&state.db).await?;
    found.map(|_| ()).ok_or(AppError::NotFound)
}

/// Fläche laden (leer, wenn es noch keine gibt).
pub async fn load(state: &AppState, user_id: i64, key: i64) -> AppResult<CanvasBoard> {
    let key = key.max(0);
    let row: Option<(String, String, i64, i64)> = sqlx::query_as("SELECT layout, shapes, rev, updated_at FROM canvas_boards WHERE user_id = ? AND project_key = ?")
        .bind(user_id)
        .bind(key)
        .fetch_optional(&state.db)
        .await?;
    let Some((layout, shapes, rev, updated_at)) = row else {
        return Ok(CanvasBoard { project_id: project_id(key), ..Default::default() });
    };
    let layout: CanvasLayout = serde_json::from_str(&layout).unwrap_or_default();
    let shapes: Vec<CanvasShape> = serde_json::from_str(&shapes).unwrap_or_default();
    Ok(CanvasBoard { project_id: project_id(key), windows: layout.windows, closed: layout.closed, shapes, rev, updated_at })
}

async fn save(state: &AppState, user_id: i64, key: i64, layout: Option<&CanvasLayout>, shapes: Option<&[CanvasShape]>) -> AppResult<i64> {
    let ts = now();
    sqlx::query("INSERT INTO canvas_boards (user_id, project_key, updated_at) VALUES (?, ?, ?) ON CONFLICT (user_id, project_key) DO NOTHING")
        .bind(user_id)
        .bind(key)
        .bind(ts)
        .execute(&state.db)
        .await?;
    if let Some(l) = layout {
        sqlx::query("UPDATE canvas_boards SET layout = ? WHERE user_id = ? AND project_key = ?")
            .bind(serde_json::to_string(l).unwrap_or_else(|_| "{}".into()))
            .bind(user_id)
            .bind(key)
            .execute(&state.db)
            .await?;
    }
    if let Some(s) = shapes {
        sqlx::query("UPDATE canvas_boards SET shapes = ? WHERE user_id = ? AND project_key = ?")
            .bind(serde_json::to_string(s).unwrap_or_else(|_| "[]".into()))
            .bind(user_id)
            .bind(key)
            .execute(&state.db)
            .await?;
    }
    let rev: i64 = sqlx::query_scalar("UPDATE canvas_boards SET rev = rev + 1, updated_at = ? WHERE user_id = ? AND project_key = ? RETURNING rev")
        .bind(ts)
        .bind(user_id)
        .bind(key)
        .fetch_one(&state.db)
        .await?;
    hub::publish(state, user_id, HubEvent::CanvasUpdated { project_id: project_id(key), rev });
    Ok(rev)
}

/// Fläche eines gelöschten Projekts entfernen.
pub async fn forget_project(state: &AppState, user_id: i64, key: i64) -> AppResult<()> {
    sqlx::query("DELETE FROM canvas_boards WHERE user_id = ? AND project_key = ?").bind(user_id).bind(key).execute(&state.db).await?;
    Ok(())
}

fn finite(v: f64) -> f64 {
    if v.is_finite() {
        v.clamp(-COORD, COORD)
    } else {
        0.0
    }
}

fn clean_text(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').take(max).collect()
}

fn clean_color(c: &str) -> String {
    let c = c.trim();
    let hex = c.strip_prefix('#').unwrap_or("");
    if (hex.len() == 6 || hex.len() == 3) && hex.chars().all(|x| x.is_ascii_hexdigit()) {
        c.to_lowercase()
    } else {
        String::new()
    }
}

/// Fenster prüfen und begrenzen; unbekannte Arten fallen weg.
fn clean_window(mut w: CanvasWindow) -> Option<CanvasWindow> {
    w.id = w.id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(64).collect();
    if w.id.is_empty() || !WINDOW_KINDS.contains(&w.kind.as_str()) {
        return None;
    }
    if w.kind == "chat" && w.thread_id.is_none() {
        return None;
    }
    w.terminal_id = w.terminal_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(64).collect();
    w.url = clean_text(w.url.trim(), 2000);
    w.name = clean_text(w.name.trim(), 24);
    w.x = finite(w.x);
    w.y = finite(w.y);
    w.w = finite(w.w).clamp(220.0, 4000.0);
    w.h = finite(w.h).clamp(140.0, 4000.0);
    Some(w)
}

/// Form prüfen und begrenzen; unbekannte Arten fallen weg.
fn clean_shape(mut s: CanvasShape) -> Option<CanvasShape> {
    s.kind = s.kind.trim().to_lowercase();
    if !KINDS.contains(&s.kind.as_str()) {
        return None;
    }
    s.id = s.id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(64).collect();
    if s.id.is_empty() {
        s.id = new_id();
    }
    s.points = s.points.into_iter().take(MAX_POINTS).map(|[x, y]| [finite(x), finite(y)]).collect();
    if matches!(s.kind.as_str(), "pen" | "line" | "arrow") && s.points.len() < 2 {
        return None;
    }
    s.x = finite(s.x);
    s.y = finite(s.y);
    s.w = finite(s.w);
    s.h = finite(s.h);
    s.text = clean_text(&s.text, 4000);
    s.color = clean_color(&s.color);
    s.size = if s.size.is_finite() { s.size.clamp(0.5, 40.0) } else { 2.0 };
    s.author = clean_text(s.author.trim(), 80);
    if s.created_at <= 0 {
        s.created_at = now();
    }
    Some(s)
}

fn new_id() -> String {
    format!("s{}", &uuid::Uuid::new_v4().simple().to_string()[..12])
}

/// Formen ändern (unter der Sperre); liefert den neuen Stand und die IDs der angelegten/geänderten.
pub async fn change(state: &AppState, user_id: i64, key: i64, change: CanvasShapesChange, author: Option<&str>) -> AppResult<(i64, Vec<String>)> {
    let key = key.max(0);
    let _guard = LOCK.lock().await;
    let board = load(state, user_id, key).await?;
    let mut shapes = if change.clear { Vec::new() } else { board.shapes };
    if !change.delete.is_empty() {
        shapes.retain(|s| !change.delete.contains(&s.id));
    }
    let mut ids = Vec::new();
    for s in change.upsert {
        let Some(mut s) = clean_shape(s) else { continue };
        if let Some(a) = author {
            s.author = a.to_string();
        } else if s.author.is_empty() {
            s.author = "user".into();
        }
        ids.push(s.id.clone());
        match shapes.iter_mut().find(|x| x.id == s.id) {
            Some(x) => *x = s,
            None => shapes.push(s),
        }
    }
    if shapes.len() > MAX_SHAPES {
        return Err(AppError::bad("Zu viele Formen auf dieser Fläche."));
    }
    let rev = save(state, user_id, key, None, Some(&shapes)).await?;
    Ok((rev, ids))
}

/// `GET /canvas/<project>`
#[get("/canvas/<project>")]
async fn get_board(state: &State<AppState>, user: CurrentUser, project: i64) -> AppResult<Json<CanvasBoard>> {
    check_project(state, user.id(), project).await?;
    Ok(Json(load(state, user.id(), project).await?))
}

/// `PUT /canvas/<project>/layout`
#[put("/canvas/<project>/layout", data = "<body>")]
async fn put_layout(state: &State<AppState>, user: CurrentUser, project: i64, body: Json<CanvasLayout>) -> AppResult<Json<CanvasRev>> {
    check_project(state, user.id(), project).await?;
    let b = body.into_inner();
    let mut windows: Vec<CanvasWindow> = Vec::new();
    for w in b.windows.into_iter().filter_map(clean_window) {
        if !windows.iter().any(|x| x.id == w.id) {
            windows.push(w);
        }
    }
    if windows.len() > MAX_WINDOWS {
        return Err(AppError::bad("Zu viele Fenster auf dieser Fläche."));
    }
    let mut closed: Vec<i64> = b.closed.into_iter().filter(|id| *id > 0).collect();
    closed.sort_unstable();
    closed.dedup();
    closed.truncate(5000);
    let layout = CanvasLayout { windows, closed };
    let _guard = LOCK.lock().await;
    let rev = save(state, user.id(), project.max(0), Some(&layout), None).await?;
    Ok(Json(CanvasRev { rev }))
}

/// `POST /canvas/<project>/shapes`
#[post("/canvas/<project>/shapes", data = "<body>")]
async fn change_shapes(state: &State<AppState>, user: CurrentUser, project: i64, body: Json<CanvasShapesChange>) -> AppResult<Json<CanvasRev>> {
    check_project(state, user.id(), project).await?;
    let (rev, _) = change(state, user.id(), project, body.into_inner(), None).await?;
    Ok(Json(CanvasRev { rev }))
}

// ------------------------------------------------------------------ Lesen für Agenten und Dieter

/// Rahmen einer Form: (x0, y0, x1, y1).
pub fn bbox(s: &CanvasShape) -> (f64, f64, f64, f64) {
    if matches!(s.kind.as_str(), "pen" | "line" | "arrow") && !s.points.is_empty() {
        let xs = s.points.iter().map(|p| p[0]);
        let ys = s.points.iter().map(|p| p[1]);
        let (x0, x1) = xs.fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
        let (y0, y1) = ys.fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
        return (x0, y0, x1, y1);
    }
    let (x0, x1) = if s.w < 0.0 { (s.x + s.w, s.x) } else { (s.x, s.x + s.w) };
    let (y0, y1) = if s.h < 0.0 { (s.y + s.h, s.y) } else { (s.y, s.y + s.h) };
    (x0, y0, x1, y1)
}

fn inside(p: [f64; 2], b: (f64, f64, f64, f64), pad: f64) -> bool {
    p[0] >= b.0 - pad && p[0] <= b.2 + pad && p[1] >= b.1 - pad && p[1] <= b.3 + pad
}

/// Beschriftung einer Form: eigener Text oder Text-Formen, die in ihr liegen.
fn label_of(s: &CanvasShape, all: &[CanvasShape]) -> String {
    if !s.text.trim().is_empty() {
        return one_line(&s.text, 120);
    }
    if matches!(s.kind.as_str(), "rect" | "ellipse" | "diamond") {
        let b = bbox(s);
        let inner: Vec<String> = all
            .iter()
            .filter(|t| t.kind == "text" && !t.text.trim().is_empty())
            .filter(|t| {
                let tb = bbox(t);
                inside([(tb.0 + tb.2) / 2.0, (tb.1 + tb.3) / 2.0], b, 0.0)
            })
            .map(|t| one_line(&t.text, 80))
            .collect();
        return inner.join(" / ");
    }
    String::new()
}

fn one_line(s: &str, max: usize) -> String {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > max {
        format!("{}…", t.chars().take(max).collect::<String>())
    } else {
        t
    }
}

/// Was liegt an diesem Punkt (für Pfeile): Fenster oder beschriftete Form.
fn target_at(p: [f64; 2], board: &CanvasBoard, skip: &str) -> Option<String> {
    let mut best: Option<(f64, String)> = None;
    for w in &board.windows {
        let b = (w.x, w.y, w.x + w.w, w.y + w.h);
        if inside(p, b, 48.0) {
            let area = w.w * w.h;
            if best.as_ref().is_none_or(|(a, _)| area < *a) {
                best = Some((area, format!("window {}", if w.name.is_empty() { w.id.clone() } else { w.name.clone() })));
            }
        }
    }
    for s in &board.shapes {
        if s.id == skip || matches!(s.kind.as_str(), "pen" | "line" | "arrow") {
            continue;
        }
        let b = bbox(s);
        if inside(p, b, 32.0) {
            let area = ((b.2 - b.0) * (b.3 - b.1)).abs().max(1.0);
            if best.as_ref().is_none_or(|(a, _)| area < *a) {
                let label = label_of(s, &board.shapes);
                let name = if label.is_empty() { format!("{} {}", s.kind, s.id) } else { format!("\"{label}\"") };
                best = Some((area, name));
            }
        }
    }
    best.map(|(_, n)| n)
}

/// Fensterinfos (Titel, Zustand) der Chat-Fenster.
async fn thread_info(state: &AppState, user_id: i64, id: i64) -> Option<(String, String)> {
    sqlx::query_as::<_, (String, String)>("SELECT title, status FROM threads WHERE id = ? AND user_id = ?").bind(id).bind(user_id).fetch_optional(&state.db).await.ok().flatten()
}

/// Kurzbeschreibung der Zeichenebene als Text (für Dieter und `canvas_read`).
pub fn describe_shapes(board: &CanvasBoard, max_lines: usize) -> String {
    let mut out = String::new();
    let mut strokes = 0usize;
    let mut lines = 0usize;
    for s in &board.shapes {
        if lines >= max_lines {
            break;
        }
        let b = bbox(s);
        let at = format!("at ({:.0}, {:.0})", b.0, b.1);
        let by = if s.author.is_empty() || s.author == "user" { String::new() } else { format!(" [by {}]", s.author) };
        let line = match s.kind.as_str() {
            "pen" => {
                strokes += 1;
                continue;
            }
            "note" => format!("- sticky note \"{}\" {at}{by}", one_line(&s.text, 300)),
            "text" => format!("- text \"{}\" {at}{by}", one_line(&s.text, 300)),
            "rect" | "ellipse" | "diamond" => {
                let label = label_of(s, &board.shapes);
                let shape = match s.kind.as_str() {
                    "rect" => "box",
                    "ellipse" => "ellipse",
                    _ => "diamond",
                };
                if label.is_empty() {
                    format!("- {shape} {at}, {:.0}×{:.0}{by}", b.2 - b.0, b.3 - b.1)
                } else {
                    format!("- {shape} \"{label}\" {at}{by}")
                }
            }
            "arrow" | "line" => {
                let first = s.points[0];
                let last = s.points[s.points.len() - 1];
                let from = target_at(first, board, &s.id).unwrap_or_else(|| format!("({:.0}, {:.0})", first[0], first[1]));
                let to = target_at(last, board, &s.id).unwrap_or_else(|| format!("({:.0}, {:.0})", last[0], last[1]));
                let label = if s.text.trim().is_empty() { String::new() } else { format!(" labelled \"{}\"", one_line(&s.text, 120)) };
                if s.kind == "arrow" {
                    format!("- arrow from {from} to {to}{label}{by}")
                } else {
                    format!("- line from {from} to {to}{label}{by}")
                }
            }
            _ => continue,
        };
        out.push_str(&line);
        out.push('\n');
        lines += 1;
    }
    if strokes > 0 {
        out.push_str(&format!("- {strokes} freehand stroke(s) (drawings without text)\n"));
    }
    out
}

/// Text für Dieter: was auf der Fläche steht.
pub async fn describe(state: &AppState, user_id: i64, key: i64) -> AppResult<String> {
    let board = load(state, user_id, key).await?;
    Ok(describe_shapes(&board, 40))
}

/// Freie Stelle für neue Formen eines Agenten: rechts neben seinem Fenster, sonst rechts von allem.
fn free_spot(board: &CanvasBoard, thread_id: i64, index: usize) -> (f64, f64) {
    let own = board.windows.iter().find(|w| w.thread_id == Some(thread_id));
    let (x, y) = match own {
        Some(w) => (w.x + w.w + 48.0, w.y),
        None => {
            let right = board
                .windows
                .iter()
                .map(|w| w.x + w.w)
                .chain(board.shapes.iter().map(|s| bbox(s).2))
                .fold(f64::MIN, f64::max);
            (if right == f64::MIN { 0.0 } else { right + 64.0 }, 0.0)
        }
    };
    (x, y + index as f64 * 150.0)
}

/// `canvas_read` – Fläche des Arbeitsbereichs als JSON (mit Text-Zusammenfassung).
pub async fn read_for_agent(state: &AppState, user_id: i64, key: i64) -> AppResult<Value> {
    let board = load(state, user_id, key).await?;
    let mut windows = Vec::new();
    for w in &board.windows {
        let (title, status) = match w.thread_id {
            Some(id) => thread_info(state, user_id, id).await.unwrap_or_default(),
            None => (String::new(), String::new()),
        };
        windows.push(json!({
            "id": w.id, "name": w.name, "kind": w.kind, "threadId": w.thread_id, "title": title, "status": status,
            "url": w.url, "x": w.x.round(), "y": w.y.round(), "w": w.w.round(), "h": w.h.round()
        }));
    }
    let shapes: Vec<Value> = board
        .shapes
        .iter()
        .map(|s| {
            let b = bbox(s);
            let mut v = json!({
                "id": s.id, "kind": s.kind, "x": b.0.round(), "y": b.1.round(), "w": (b.2 - b.0).round(), "h": (b.3 - b.1).round(),
                "text": s.text, "color": s.color, "author": s.author
            });
            if matches!(s.kind.as_str(), "arrow" | "line") {
                let first = s.points[0];
                let last = s.points[s.points.len() - 1];
                v["from"] = json!(target_at(first, &board, &s.id));
                v["to"] = json!(target_at(last, &board, &s.id));
            }
            if s.kind == "pen" {
                v["points"] = json!(s.points.len());
            }
            v
        })
        .collect();
    let project: Option<String> = match project_id(key) {
        Some(p) => sqlx::query_scalar("SELECT name FROM projects WHERE id = ?").bind(p).fetch_optional(&state.db).await?,
        None => None,
    };
    Ok(json!({
        "workspace": project.unwrap_or_else(|| "(no project)".into()),
        "summary": describe_shapes(&board, 200),
        "windows": windows,
        "shapes": shapes,
        "rev": board.rev
    }))
}

/// `canvas_draw` – Formen eines Agenten anlegen (Pfeile auch zwischen Fenstern/Formen über
/// `from`/`to`), eigene löschen. Liefert die neuen IDs.
pub async fn draw_for_agent(state: &AppState, user_id: i64, key: i64, thread_id: i64, author: &str, args: &Value) -> AppResult<Value> {
    let board = load(state, user_id, key).await?;
    // Arbeitskopie: Pfeile dürfen auf Formen zeigen, die im selben Aufruf entstehen
    let mut work = board.clone();
    let mut upsert = Vec::new();
    let items = args["shapes"].as_array().cloned().unwrap_or_default();
    if items.len() > 200 {
        return Err(AppError::bad("Höchstens 200 Formen auf einmal."));
    }
    let mut auto = 0usize;
    for item in items {
        let mut s: CanvasShape = serde_json::from_value(item.clone()).map_err(|e| AppError::bad(format!("shape: {e}")))?;
        s.id = String::new();
        s.kind = s.kind.trim().to_lowercase();
        if s.kind == "sticky" || s.kind.is_empty() {
            s.kind = "note".into();
        }
        if matches!(s.kind.as_str(), "arrow" | "line") && s.points.len() < 2 {
            let from = item["from"].as_str().and_then(|f| anchor(&work, f));
            let to = item["to"].as_str().and_then(|t| anchor(&work, t));
            match (from, to) {
                (Some(a), Some(b)) => s.points = edge_points(a, b),
                _ => return Err(AppError::bad("arrow/line: points or from/to (window name or shape id/text) needed")),
            }
        }
        let boxed = matches!(s.kind.as_str(), "rect" | "ellipse" | "diamond" | "text" | "note");
        if boxed {
            if s.w == 0.0 && s.h == 0.0 {
                let (w, h) = if s.kind == "text" { (320.0, 48.0) } else if s.kind == "note" { (240.0, 140.0) } else { (220.0, 120.0) };
                s.w = w;
                s.h = h;
            }
            if item.get("x").is_none() && item.get("y").is_none() {
                // Spalte neben dem Fenster bzw. rechts vom Bestand, untereinander
                let (x, y) = free_spot(&board, thread_id, auto);
                auto += 1;
                s.x = x;
                s.y = y;
            }
        }
        if s.size == 0.0 {
            s.size = 2.0;
        }
        if s.kind == "note" && s.color.is_empty() {
            s.color = "#f5c542".into();
        }
        if KINDS.contains(&s.kind.as_str()) {
            s.id = new_id();
            work.shapes.push(s.clone());
        }
        upsert.push(s);
    }
    let mut delete: Vec<String> = args["delete"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    // Agenten löschen nur ihre eigenen Formen
    delete.retain(|id| board.shapes.iter().any(|s| s.id == *id && s.author == author));
    if args["clearMine"].as_bool() == Some(true) {
        delete.extend(board.shapes.iter().filter(|s| s.author == author).map(|s| s.id.clone()));
    }
    let (rev, ids) = change(state, user_id, key, CanvasShapesChange { upsert, delete, clear: false }, Some(author)).await?;
    Ok(json!({"created": ids, "rev": rev}))
}

/// Mittelpunkt und Rahmen eines Ziels (Fenstername, Form-ID oder Text einer Form).
fn anchor(board: &CanvasBoard, name: &str) -> Option<(f64, f64, f64, f64)> {
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    if let Some(w) = board.windows.iter().find(|w| w.name.to_lowercase() == n || w.id.to_lowercase() == n) {
        return Some((w.x, w.y, w.x + w.w, w.y + w.h));
    }
    if let Some(s) = board.shapes.iter().find(|s| s.id.to_lowercase() == n) {
        return Some(bbox(s));
    }
    board.shapes.iter().find(|s| !matches!(s.kind.as_str(), "pen" | "line" | "arrow") && label_of(s, &board.shapes).to_lowercase() == n).map(bbox)
}

/// Pfeil von Rand zu Rand zweier Rahmen.
fn edge_points(a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)) -> Vec<[f64; 2]> {
    let ca = ((a.0 + a.2) / 2.0, (a.1 + a.3) / 2.0);
    let cb = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
    let edge = |c: (f64, f64), r: (f64, f64, f64, f64), toward: (f64, f64)| -> [f64; 2] {
        let (dx, dy) = (toward.0 - c.0, toward.1 - c.1);
        let (hw, hh) = (((r.2 - r.0) / 2.0).max(1.0), ((r.3 - r.1) / 2.0).max(1.0));
        if dx == 0.0 && dy == 0.0 {
            return [c.0, c.1];
        }
        let t = (hw / dx.abs().max(1e-9)).min(hh / dy.abs().max(1e-9));
        [c.0 + dx * t, c.1 + dy * t]
    };
    vec![edge(ca, a, cb), edge(cb, b, ca)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(kind: &str, x: f64, y: f64, w: f64, h: f64, text: &str) -> CanvasShape {
        CanvasShape { id: format!("{kind}{x}"), kind: kind.into(), x, y, w, h, text: text.into(), ..Default::default() }
    }

    #[test]
    fn labels_and_arrows() {
        let mut board = CanvasBoard::default();
        board.shapes.push(shape("rect", 0.0, 0.0, 200.0, 100.0, ""));
        board.shapes.push(shape("text", 20.0, 30.0, 100.0, 30.0, "API"));
        board.shapes.push(shape("ellipse", 400.0, 0.0, 200.0, 100.0, "Datenbank"));
        board.windows.push(CanvasWindow { id: "t1".into(), kind: "chat".into(), thread_id: Some(1), name: "Lena".into(), x: 0.0, y: 400.0, w: 500.0, h: 400.0, ..Default::default() });
        let mut a = shape("arrow", 0.0, 0.0, 0.0, 0.0, "");
        a.points = vec![[190.0, 50.0], [410.0, 50.0]];
        board.shapes.push(a);
        let mut b = shape("arrow", 1.0, 0.0, 0.0, 0.0, "");
        b.points = vec![[100.0, 90.0], [100.0, 420.0]];
        board.shapes.push(b);
        let text = describe_shapes(&board, 50);
        assert!(text.contains("box \"API\""), "{text}");
        assert!(text.contains("arrow from \"API\" to \"Datenbank\""), "{text}");
        assert!(text.contains("to window Lena"), "{text}");
    }

    #[test]
    fn cleans_input() {
        assert!(clean_shape(shape("blob", 0.0, 0.0, 1.0, 1.0, "")).is_none());
        let s = clean_shape(CanvasShape { kind: "PEN".into(), points: vec![[0.0, 0.0], [f64::NAN, 5.0]], color: "red".into(), ..Default::default() }).unwrap();
        assert_eq!(s.kind, "pen");
        assert_eq!(s.points[1], [0.0, 5.0]);
        assert_eq!(s.color, "");
        assert!(!s.id.is_empty());
        assert_eq!(clean_color("#AbC123"), "#abc123");
        let w = clean_window(CanvasWindow { id: "t 5!".into(), kind: "chat".into(), thread_id: Some(5), w: 10.0, h: 10.0, ..Default::default() }).unwrap();
        assert_eq!(w.id, "t5");
        assert_eq!((w.w, w.h), (220.0, 140.0));
        assert!(clean_window(CanvasWindow { id: "x".into(), kind: "chat".into(), ..Default::default() }).is_none());
    }

    #[test]
    fn arrow_between_boxes() {
        let p = edge_points((0.0, 0.0, 100.0, 100.0), (300.0, 0.0, 400.0, 100.0));
        assert_eq!(p[0], [100.0, 50.0]);
        assert_eq!(p[1], [300.0, 50.0]);
    }
}
