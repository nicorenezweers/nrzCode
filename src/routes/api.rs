//! Allgemeine API: `GET /health`, `POST /markdown` (Vorschau), `GET /events` (Live-Ereignisse).

use crate::auth::CurrentUser;
use crate::markdown;
use nrzcode_shared::{Health, MarkdownRequest, MarkdownResponse};
use rocket::serde::json::Json;
use rocket::{get, post, routes, Route};

pub fn routes() -> Vec<Route> {
    routes![health, render_markdown, crate::hub::events]
}

#[get("/health")]
fn health() -> Json<Health> {
    Json(Health { status: "ok".into(), name: "nrzCode".into(), version: env!("CARGO_PKG_VERSION").into() })
}

#[post("/markdown", data = "<body>")]
fn render_markdown(_user: CurrentUser, body: Json<MarkdownRequest>) -> Json<MarkdownResponse> {
    Json(MarkdownResponse { html: markdown::render_plain(&body.text) })
}
