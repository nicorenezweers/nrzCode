//! Dieter auf der Canvas: Lage aller Fenster (mit Art und Ort), Arbeitsbereiche; führt seine
//! Aktionen aus – Aufträge, Stopp, hinfliegen, verschieben, anordnen, Arbeitsbereich wechseln,
//! Terminal/Browser öffnen, Adresse zeigen, schliessen, Notiz anheften.

use super::store::*;
use crate::api;
use crate::i18n::{tr, trf};
use crate::state::{self, PROJECTS};
use dioxus::prelude::*;
use nrzcode_shared::agents::PostMessage;
use nrzcode_shared::canvas::{CanvasShape, CanvasWindow, MAX_WINDOWS};
use nrzcode_shared::voice::{DieterAction, DieterPane, DieterRequest, DieterTurn};
use nrzcode_shared::ThreadSummary;

/// Namen für die Spracherkennung (Fenster und Arbeitsbereiche, damit sie richtig geschrieben werden).
pub fn dieter_names() -> Vec<String> {
    let mut out: Vec<String> = BOARD.peek().as_ref().map(|b| b.windows.iter().map(|w| w.name.clone()).filter(|n| !n.is_empty()).collect()).unwrap_or_default();
    out.extend(PROJECTS.peek().values().map(|p| p.name.clone()));
    out.truncate(40);
    out
}

fn kind_of(w: &CanvasWindow) -> &'static str {
    match w.kind.as_str() {
        "terminal" => "terminal",
        "browser" => "browser",
        _ => "chat",
    }
}

/// Lage eines Fensters im Verhältnis zu den anderen („top left“ …).
fn place_of(w: &CanvasWindow, all: &[CanvasWindow]) -> String {
    if all.len() < 2 {
        return String::new();
    }
    let x0 = all.iter().map(|o| o.x).fold(f64::MAX, f64::min);
    let x1 = all.iter().map(|o| o.x + o.w).fold(f64::MIN, f64::max);
    let y0 = all.iter().map(|o| o.y).fold(f64::MAX, f64::min);
    let y1 = all.iter().map(|o| o.y + o.h).fold(f64::MIN, f64::max);
    let (cx, cy) = ((w.x + w.w / 2.0 - x0) / (x1 - x0).max(1.0), (w.y + w.h / 2.0 - y0) / (y1 - y0).max(1.0));
    let h = if cx < 0.34 { "left" } else if cx > 0.66 { "right" } else { "center" };
    let v = if cy < 0.34 { "top" } else if cy > 0.66 { "bottom" } else { "middle" };
    match (v, h) {
        ("middle", "center") => "center".into(),
        ("middle", h) => h.into(),
        (v, "center") => v.into(),
        (v, h) => format!("{v} {h}"),
    }
}

/// Anfrage an Dieter mit der Lage auf der Canvas (die Zeichnung ergänzt der Server).
pub fn dieter_request(text: String, history: Vec<DieterTurn>) -> DieterRequest {
    let board = BOARD.peek().clone().unwrap_or_default();
    let mut windows = board.windows.clone();
    windows.sort_by(|a, b| (a.y as i64 / 200, a.x as i64).cmp(&(b.y as i64 / 200, b.x as i64)));
    let panes = windows
        .iter()
        .filter(|w| !w.name.is_empty())
        .map(|w| DieterPane {
            name: w.name.clone(),
            thread_id: w.thread_id,
            kind: kind_of(w).into(),
            detail: match w.kind.as_str() {
                "browser" => w.url.clone(),
                "terminal" => tr("Projektordner"),
                _ => String::new(),
            },
            place: place_of(w, &windows),
        })
        .collect();
    let key = (*WS.peek()).max(0);
    let mut spaces: Vec<String> = PROJECTS.peek().values().map(|p| p.name.clone()).collect();
    spaces.sort();
    spaces.push(tr("Ohne Projekt"));
    let current = project_of(key).and_then(|p| PROJECTS.peek().get(&p).map(|p| p.name.clone())).unwrap_or_else(|| tr("Ohne Projekt"));
    let focused = SELECTED.peek().clone().and_then(|id| window(&id)).map(|w| w.name);
    DieterRequest {
        text,
        panes,
        focused,
        can_add: board.windows.len() < MAX_WINDOWS,
        history,
        mode: "canvas".into(),
        workspaces: spaces,
        workspace: current,
        workspace_id: project_of(key),
        board: String::new(),
    }
}

fn find(name: &str) -> Result<CanvasWindow, String> {
    window_by_name(name).ok_or_else(|| trf("Kein Fenster „{0}“.", &[&name]))
}

/// Eine Aktion ausführen; Ergebnis als kurzer Text für die Sprechblase.
pub async fn dieter_action(a: DieterAction) -> Result<String, String> {
    let project = project_of((*WS.peek()).max(0));
    match a.kind.as_str() {
        "send" => {
            let w = find(&a.to)?;
            match w.thread_id {
                Some(tid) => {
                    let t = api::post::<ThreadSummary, _>(&format!("/threads/{tid}/messages"), &PostMessage { text: a.prompt.clone(), attachments: Vec::new(), interrupt: false })
                        .await
                        .map_err(|e| e.message)?;
                    let queued = t.status.is_active();
                    state::upsert_thread(t);
                    Ok(if queued { trf("{0}: wartet, bis der Agent fertig ist", &[&w.name]) } else { trf("{0}: Auftrag übergeben", &[&w.name]) })
                }
                None if w.kind == "new" => {
                    let t = crate::pages::agents::start_thread(project, a.prompt.clone()).await.map_err(|e| e.message)?;
                    window_created(&w.id, t.id);
                    Ok(trf("{0}: neuer Chat gestartet", &[&w.name]))
                }
                None => Err(trf("„{0}“ ist kein Chat.", &[&w.name])),
            }
        }
        "stop" => {
            let w = find(&a.to)?;
            let tid = w.thread_id.ok_or_else(|| trf("In „{0}“ läuft nichts.", &[&w.name]))?;
            let t = api::action::<ThreadSummary>(&format!("/threads/{tid}/stop")).await.map_err(|e| e.message)?;
            state::upsert_thread(t);
            Ok(trf("{0}: gestoppt", &[&w.name]))
        }
        "focus" => {
            let w = find(&a.to)?;
            fly_to(&w.id);
            Ok(trf("{0}: im Blick", &[&w.name]))
        }
        "unfocus" => {
            fit_all();
            Ok(tr("Alles im Blick"))
        }
        "new" => {
            let t = crate::pages::agents::start_thread(project, a.prompt.clone()).await.map_err(|e| e.message)?;
            let id = add_window("chat", Some(t.id), "", None).ok_or_else(|| tr("Die Canvas ist voll."))?;
            fly_to(&id);
            Ok(tr("Neues Fenster gestartet"))
        }
        "move" => {
            let w = find(&a.to)?;
            move_window(&w, &a.target)?;
            Ok(trf("{0}: verschoben", &[&w.name]))
        }
        "arrange" => {
            arrange();
            Ok(tr("Fenster angeordnet"))
        }
        "workspace" => {
            let name = a.target.trim();
            let key = if name.eq_ignore_ascii_case(&tr("Ohne Projekt")) {
                0
            } else {
                PROJECTS.peek().values().find(|p| p.name.eq_ignore_ascii_case(name)).map(|p| p.id).ok_or_else(|| trf("Kein Arbeitsbereich „{0}“.", &[&name]))?
            };
            set_ws(key);
            Ok(trf("Arbeitsbereich: {0}", &[&name]))
        }
        "open" => {
            let kind = if a.target == "browser" { "browser" } else { "terminal" };
            let id = add_window(kind, None, &a.url, None).ok_or_else(|| tr("Die Canvas ist voll."))?;
            fly_to(&id);
            Ok(if kind == "browser" { tr("Browser geöffnet") } else { tr("Terminal geöffnet") })
        }
        "navigate" => {
            let w = find(&a.to)?;
            update_window(&w.id, |x| x.url = a.url.clone());
            schedule_save();
            Ok(trf("{0}: {1}", &[&w.name, &a.url]))
        }
        "close" => {
            let w = find(&a.to)?;
            close_window(&w.id);
            Ok(trf("{0}: geschlossen", &[&w.name]))
        }
        "note" => {
            let (cx, cy) = view_center();
            let s = CanvasShape {
                id: new_shape_id(),
                kind: "note".into(),
                x: cx - 120.0 + (js_sys::Math::random() - 0.5) * 80.0,
                y: cy - 75.0 + (js_sys::Math::random() - 0.5) * 60.0,
                w: 240.0,
                h: 150.0,
                text: a.prompt.clone(),
                color: super::draw::NOTE_COLORS[0].into(),
                size: 2.0,
                author: "Dieter".into(),
                ..Default::default()
            };
            add_shape(s);
            Ok(tr("Notiz angeheftet"))
        }
        _ => Err(String::new()),
    }
}

/// Fenster verschieben: in eine Richtung, in die Mitte der Ansicht oder neben ein anderes.
fn move_window(w: &CanvasWindow, target: &str) -> Result<(), String> {
    let (x, y) = match target {
        "left" => (w.x - (w.w * 0.5 + GAP), w.y),
        "right" => (w.x + (w.w * 0.5 + GAP), w.y),
        "up" => (w.x, w.y - (w.h * 0.5 + GAP)),
        "down" => (w.x, w.y + (w.h * 0.5 + GAP)),
        "center" => {
            let (cx, cy) = view_center();
            (cx - w.w / 2.0, cy - w.h / 2.0)
        }
        other => {
            let o = find(other)?;
            (o.x + o.w + GAP, o.y)
        }
    };
    glide_windows();
    update_window(&w.id, |win| {
        win.x = x.round();
        win.y = y.round();
    });
    raise(&w.id);
    schedule_save();
    Ok(())
}
