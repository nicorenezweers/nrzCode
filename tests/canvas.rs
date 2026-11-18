//! Canvas-Modus: Fläche je Arbeitsbereich (Fenster, geschlossene Threads, Zeichnung), Rechte,
//! Aufräumen beim Löschen des Projekts.

mod common;

use common::{json, TestApp, TestUser};
use nrzcode_shared::canvas::{CanvasBoard, CanvasRev};
use rocket::http::Status;
use serde_json::{json as j, Value};

async fn get_board(t: &TestApp, u: &TestUser, key: i64) -> (Status, Option<CanvasBoard>) {
    let res = t.client.get(format!("/api/v1/canvas/{key}")).private_cookie(u.cookie.clone()).dispatch().await;
    let st = res.status();
    (st, res.into_json().await)
}

async fn send(t: &TestApp, u: &TestUser, method: &str, path: &str, body: Value) -> (Status, String) {
    let req = match method {
        "PUT" => t.client.put(path),
        _ => t.client.post(path),
    };
    let res = req.private_cookie(u.cookie.clone()).header(json()).body(body.to_string()).dispatch().await;
    (res.status(), res.into_string().await.unwrap_or_default())
}

#[rocket::async_test]
async fn board_layout_and_shapes() {
    let t = TestApp::new().await;
    let anna = t.user("anna", false).await;
    let bob = t.user("bob", false).await;
    let thread = t.thread(anna.id, None, "Webseite").await;

    // Noch leer
    let (st, b) = get_board(&t, &anna, 0).await;
    assert_eq!(st, Status::Ok);
    let b = b.unwrap();
    assert!(b.windows.is_empty() && b.shapes.is_empty());
    assert_eq!((b.rev, b.project_id), (0, None));

    // Fenster: Chat, Terminal, Browser; ungültige fallen weg, Grössen werden begrenzt
    let (st, body) = send(&t, &anna, "PUT", "/api/v1/canvas/0/layout", j!({
        "windows": [
            {"id": format!("t{thread}"), "kind": "chat", "threadId": thread, "name": "Lena", "x": 10, "y": 20, "w": 540, "h": 680, "z": 2},
            {"id": "terminal-1", "kind": "terminal", "terminalId": "abc", "name": "Max", "x": 600, "y": 0, "w": 10, "h": 10, "z": 1},
            {"id": "browser-1", "kind": "browser", "url": "http://localhost:3000", "x": 0, "y": 800, "w": 900, "h": 600},
            {"id": "x", "kind": "chat"},
            {"id": "y", "kind": "rakete"}
        ],
        "closed": [5, 5, -1]
    })).await;
    assert_eq!(st, Status::Ok, "{body}");
    let rev: CanvasRev = serde_json::from_str(&body).unwrap();
    assert_eq!(rev.rev, 1);

    // Zeichnung: Notiz, Kasten, Pfeil; unbekannte Arten fallen weg
    let (st, body) = send(&t, &anna, "POST", "/api/v1/canvas/0/shapes", j!({
        "upsert": [
            {"id": "n1", "kind": "note", "x": 0, "y": -200, "w": 240, "h": 150, "text": "Login bauen"},
            {"id": "r1", "kind": "rect", "x": 0, "y": 0, "w": 100, "h": 50, "color": "#E03131"},
            {"id": "a1", "kind": "arrow", "points": [[0, 0], [100, 100]]},
            {"id": "z1", "kind": "zauberei"}
        ]
    })).await;
    assert_eq!(st, Status::Ok, "{body}");
    let b = get_board(&t, &anna, 0).await.1.unwrap();
    assert_eq!(b.rev, 2);
    assert_eq!(b.windows.len(), 3);
    assert_eq!(b.closed, vec![5]);
    let term = b.windows.iter().find(|w| w.id == "terminal-1").unwrap();
    assert_eq!((term.w, term.h), (220.0, 140.0));
    assert_eq!(b.shapes.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["n1", "r1", "a1"]);
    assert_eq!(b.shapes[1].color, "#e03131");
    assert!(b.shapes.iter().all(|s| s.author == "user"));

    // Ändern (gleiche ID) und löschen
    let (st, _) = send(&t, &anna, "POST", "/api/v1/canvas/0/shapes", j!({"upsert": [{"id": "n1", "kind": "note", "text": "Login und Registrierung"}], "delete": ["r1"]})).await;
    assert_eq!(st, Status::Ok);
    let b = get_board(&t, &anna, 0).await.1.unwrap();
    assert_eq!(b.shapes.len(), 2);
    assert_eq!(b.shapes[0].text, "Login und Registrierung");
    // Alles löschen
    let (st, _) = send(&t, &anna, "POST", "/api/v1/canvas/0/shapes", j!({"clear": true})).await;
    assert_eq!(st, Status::Ok);
    assert!(get_board(&t, &anna, 0).await.1.unwrap().shapes.is_empty());

    // Jeder Benutzer hat seine eigene Fläche
    let b = get_board(&t, &bob, 0).await.1.unwrap();
    assert!(b.windows.is_empty());

    // Ohne Anmeldung nichts
    let res = t.client.get("/api/v1/canvas/0").dispatch().await;
    assert_eq!(res.status(), Status::Unauthorized);
}

#[rocket::async_test]
async fn project_boards_belong_to_their_owner() {
    let t = TestApp::new().await;
    let anna = t.user("anna", false).await;
    let bob = t.user("bob", false).await;
    let (pid, _) = t.project(anna.id, "Webshop").await;

    let (st, b) = get_board(&t, &anna, pid).await;
    assert_eq!(st, Status::Ok);
    assert_eq!(b.unwrap().project_id, Some(pid));
    // Fremdes Projekt: nicht gefunden
    assert_eq!(get_board(&t, &bob, pid).await.0, Status::NotFound);
    let (st, _) = send(&t, &bob, "POST", &format!("/api/v1/canvas/{pid}/shapes"), j!({"upsert": [{"kind": "note", "text": "x"}]})).await;
    assert_eq!(st, Status::NotFound);

    let (st, _) = send(&t, &anna, "POST", &format!("/api/v1/canvas/{pid}/shapes"), j!({"upsert": [{"kind": "note", "text": "Plan"}]})).await;
    assert_eq!(st, Status::Ok);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM canvas_boards WHERE project_key = ?").bind(pid).fetch_one(t.db()).await.unwrap();
    assert_eq!(n, 1);
    // Projekt gelöscht → Fläche weg
    let res = t.client.delete(format!("/api/v1/projects/{pid}")).private_cookie(anna.cookie.clone()).dispatch().await;
    assert_eq!(res.status(), Status::NoContent);
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM canvas_boards WHERE project_key = ?").bind(pid).fetch_one(t.db()).await.unwrap();
    assert_eq!(n, 0);
}

#[rocket::async_test]
async fn preview_without_thread_for_browser_windows() {
    let t = TestApp::new().await;
    let anna = t.user("anna", false).await;
    let (pid, _) = t.project(anna.id, "Webshop").await;
    // Ports eines Projekts (ohne Thread) lassen sich abfragen
    let res = t.client.get(format!("/api/v1/previews?projectId={pid}")).private_cookie(anna.cookie.clone()).dispatch().await;
    assert_eq!(res.status(), Status::Ok);
    // Token ohne Thread: Vorschau ist in den Tests aus (preview_port = 0) → Konflikt, kein 404
    let (st, _) = send(&t, &anna, "POST", "/api/v1/previews/token", j!({"threadId": 0, "port": 3000})).await;
    assert_eq!(st, Status::Conflict);
}
