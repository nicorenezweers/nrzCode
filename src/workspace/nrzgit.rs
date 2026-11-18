//! Verbindung zu nrzGit: Adresse und Zugriffstoken je Benutzer (`nrzgit_connections`), Prüfung
//! über `GET <url>/api/v1/me`, Repositories und Besitzer auflisten, Repositories und Pull
//! Requests anlegen. Das Token verlässt den Server nie (nur als Hinweis „nrzg_ab…9f“).
//!
//! Neben (oder statt) nrzGit kann ein luGit verbunden sein (`kind = "lugit"`, erkannt über
//! `GET <url>/api/health`) – je Benutzer höchstens eine Verbindung je Plattform. luGit hat die
//! gleiche API für Repositories, Pull Requests und Git über HTTP, aber Repositories legt man dort
//! unter `/user/repos` bzw. `/orgs/<org>/repos` an, Listen haben höchstens 100 Einträge je Seite,
//! Organisationen heissen `orgs`. Einen MCP-Server hat luGit ab 1.1.0 (`has_mcp`, beim Verbinden
//! und Prüfen über `POST <url>/mcp` erkannt); bei älteren bekommen die Agenten keinen – Klonen,
//! Pushen und Pull Requests macht nrzCode ohnehin selbst.
//!
//! Dazu GitHub (`kind = "github"`): github.com (API unter `https://api.github.com`) oder ein GitHub
//! Enterprise Server (API unter `<url>/api/v3`, daran erkannt). Die REST-API von GitHub hat andere
//! Formen (`login` statt `username`, Datum als Text, Fehler als `{"message"}`) – der [`Client`]
//! übersetzt sie in dieselben `Remote…`-Typen. Token: Personal Access Token (klassisch mit „repo“
//! und „read:org“ oder fein eingestellt mit Contents, Pull requests, Metadata, Administration).
//! Agenten bekommen bei github.com den MCP-Server von GitHub (`https://api.githubcopilot.com/mcp/`).
//!
//! Routen: `GET/PUT/DELETE /nrzgit`, `GET /nrzgit/connections`, `POST /nrzgit/verify`,
//! `GET /nrzgit/repos?q=`, `GET /nrzgit/owners`, `GET /nrzgit/candidates` – die Plattform wählt
//! `?kind=nrzgit|lugit|github` (ohne Angabe: die erste Verbindung – nrzGit, luGit, GitHub).

use super::NrzGitAccess;
use crate::auth::CurrentUser;
use crate::db::{self, now};
use crate::error::{AppError, AppResult};
use crate::AppState;
use nrzcode_shared::workspace::{NrzGitCandidate, NrzGitConnect, NrzGitOwner, NrzGitRepo, NrzGitStatus};
use rocket::http::Status;
use rocket::serde::json::Json;
use rocket::{delete, get, post, put, routes, Route, State};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub fn routes() -> Vec<Route> {
    routes![status, connections, connect, verify, disconnect, repos, owners, candidates]
}

/// Adresse vereinheitlichen: Schema ergänzen, `/` am Ende entfernen.
pub fn normalize_url(url: &str) -> AppResult<String> {
    let mut u = url.trim().trim_end_matches('/').to_string();
    // Auch die MCP- oder API-Adresse annehmen (aus nrzGit → Einstellungen → MCP kopiert)
    for suffix in ["/mcp", "/api/v1", "/api"] {
        if let Some(s) = u.strip_suffix(suffix) {
            u = s.trim_end_matches('/').to_string();
        }
    }
    if u.is_empty() {
        return Err(AppError::bad("Bitte die Adresse von nrzGit angeben."));
    }
    if !u.starts_with("http://") && !u.starts_with("https://") {
        u = format!("http://{u}");
    }
    let rest = u.split_once("://").map(|(_, r)| r).unwrap_or("");
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() || u.chars().any(|c| c.is_whitespace() || c.is_control()) || rest.contains('@') {
        return Err(AppError::bad("Die Adresse von nrzGit ist ungültig."));
    }
    // github.com in jeder Schreibweise (auch ein Profil oder die API-Adresse) → eine feste Adresse
    if matches!(host.to_ascii_lowercase().as_str(), "github.com" | "www.github.com" | "api.github.com") {
        return Ok(GITHUB_URL.to_string());
    }
    Ok(u)
}

/// Host (mit Port) einer Adresse, z. B. `umbrel.local:8390`.
pub fn host_of(url: &str) -> String {
    url.split_once("://").map(|(_, r)| r).unwrap_or(url).split('/').next().unwrap_or("").to_string()
}

/// Art der Plattform: nrzGit (Vorgabe).
pub const KIND_NRZGIT: &str = "nrzgit";
/// Art der Plattform: luGit.
pub const KIND_LUGIT: &str = "lugit";

/// Art der Plattform: GitHub (github.com oder GitHub Enterprise Server).
pub const KIND_GITHUB: &str = "github";
/// Alle Plattformen in der Reihenfolge, in der sie gelten und angezeigt werden.
pub const KINDS: [&str; 3] = [KIND_NRZGIT, KIND_LUGIT, KIND_GITHUB];
/// Adresse von github.com (so gespeichert; die API liegt unter `https://api.github.com`).
pub const GITHUB_URL: &str = "https://github.com";
/// MCP-Server von GitHub (nur für github.com), Anmeldung mit dem Token des Benutzers.
pub const GITHUB_MCP_URL: &str = "https://api.githubcopilot.com/mcp/";

/// Name der Plattform für Meldungen.
pub fn forge_name(kind: &str) -> &'static str {
    match kind {
        KIND_LUGIT => "luGit",
        KIND_GITHUB => "GitHub",
        _ => "nrzGit",
    }
}

/// Seite, auf der man ein Token erstellt (nrzGit: Einstellungen → MCP, luGit: Zugriffstoken,
/// GitHub: Settings → Developer settings → Personal access tokens).
pub fn token_url(url: &str, kind: &str) -> String {
    if url.is_empty() {
        String::new()
    } else if kind == KIND_LUGIT {
        format!("{url}/user/settings/tokens")
    } else if kind == KIND_GITHUB {
        format!("{url}/settings/tokens")
    } else {
        format!("{url}/user/settings/mcp")
    }
}

/// Basis der REST-API einer Plattform (ohne `/` am Ende).
pub fn api_base(url: &str, kind: &str) -> String {
    let url = url.trim_end_matches('/');
    match kind {
        KIND_GITHUB if url == GITHUB_URL => "https://api.github.com".to_string(),
        KIND_GITHUB => format!("{url}/api/v3"),
        _ => format!("{url}/api/v1"),
    }
}

/// Host der Container-Registry einer Plattform: bei github.com `ghcr.io`, sonst der eigene Host.
pub fn registry_host(url: &str, kind: &str) -> String {
    if kind == KIND_GITHUB && url.trim_end_matches('/') == GITHUB_URL {
        "ghcr.io".to_string()
    } else {
        host_of(url)
    }
}

/// Adresse eines Pull Requests im Browser (GitHub: `/pull/<n>`, sonst `/pulls/<n>`).
pub fn pull_url(public_url: &str, kind: &str, repo: &str, number: i64) -> String {
    let part = if kind == KIND_GITHUB { "pull" } else { "pulls" };
    format!("{public_url}/{repo}/{part}/{number}")
}

/// Zeit `2024-05-01T12:30:00Z` (so meldet GitHub) als Unix-Sekunden; 0, wenn nicht lesbar.
fn unix_from_iso(s: &str) -> i64 {
    let num = |a: usize, b: usize| s.get(a..b).and_then(|t| t.parse::<i64>().ok());
    let (Some(y), Some(m), Some(d), Some(h), Some(mi), Some(sec)) = (num(0, 4), num(5, 7), num(8, 10), num(11, 13), num(14, 16), num(17, 19)) else {
        return 0;
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return 0;
    }
    // Tage seit 1970-01-01 (bürgerlicher Kalender, Algorithmus von Howard Hinnant)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400 + h * 3600 + mi * 60 + sec
}

/// Welche Plattform antwortet unter `url`? nrzGit meldet sich unter `/api/v1/health` mit Namen,
/// luGit unter `/api/health` mit `{"ok": true, "version"}`, GitHub Enterprise unter `/api/v3`;
/// github.com steht fest. Ohne klare Antwort: nrzGit.
pub async fn detect_kind(url: &str) -> &'static str {
    if url == GITHUB_URL {
        return KIND_GITHUB;
    }
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .connect_timeout(Duration::from_secs(4))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default();
    match probe(&http, url).await {
        Ok((kind, _)) => kind,
        Err(_) => KIND_NRZGIT,
    }
}

/// Angabe `kind` einer Anfrage prüfen (`None` = keine Angabe).
pub fn parse_kind(kind: Option<&str>) -> AppResult<Option<&'static str>> {
    match kind.map(str::trim).unwrap_or("") {
        "" => Ok(None),
        KIND_NRZGIT => Ok(Some(KIND_NRZGIT)),
        KIND_LUGIT => Ok(Some(KIND_LUGIT)),
        KIND_GITHUB => Ok(Some(KIND_GITHUB)),
        _ => Err(AppError::bad("Unbekannte Plattform – „nrzgit“, „lugit“ oder „github“.")),
    }
}

/// Hat die Plattform unter `url` einen MCP-Server? nrzGit immer; luGit ab 1.1.0: `POST <url>/mcp`
/// ohne Token antwortet dann mit 401 (ältere mit 404); GitHub nur bei github.com.
pub async fn detect_mcp(url: &str, kind: &str) -> bool {
    if kind == KIND_GITHUB {
        return url == GITHUB_URL;
    }
    if kind != KIND_LUGIT {
        return true;
    }
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .connect_timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default();
    let body = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "ping"});
    match http.post(format!("{}/mcp", url.trim_end_matches('/'))).header("Accept", "application/json, text/event-stream").json(&body).send().await {
        Ok(res) => res.status().as_u16() == 401 || res.status().is_success(),
        Err(_) => false,
    }
}

/// Vor einem Lauf: Bei einem verbundenen luGit ohne MCP-Server nachsehen, ob es inzwischen einen
/// hat (luGit wurde aktualisiert) – höchstens alle fünf Minuten je Adresse.
pub async fn refresh_mcp(state: &AppState, user_id: i64) -> AppResult<()> {
    static LAST: std::sync::Mutex<Option<std::collections::HashMap<String, std::time::Instant>>> = std::sync::Mutex::new(None);
    let row: Option<(String,)> =
        sqlx::query_as("SELECT url FROM nrzgit_connections WHERE user_id = ? AND kind = 'lugit' AND has_mcp = 0 AND verified_at IS NOT NULL")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    let Some((url,)) = row else {
        return Ok(());
    };
    {
        let mut guard = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let map = guard.get_or_insert_with(Default::default);
        if map.get(&url).is_some_and(|at| at.elapsed() < Duration::from_secs(300)) {
            return Ok(());
        }
        map.insert(url.clone(), std::time::Instant::now());
    }
    if detect_mcp(&url, KIND_LUGIT).await {
        sqlx::query("UPDATE nrzgit_connections SET has_mcp = 1 WHERE user_id = ? AND kind = 'lugit'").bind(user_id).execute(&state.db).await?;
    }
    Ok(())
}

/// Vorgabe-Adresse der Instanz: Einstellung der Verwaltung, sonst Konfiguration.
pub async fn default_url(state: &AppState) -> AppResult<String> {
    let v = db::setting(&state.db, "nrzgit_url", "").await?;
    Ok(if v.trim().is_empty() { state.config.nrzgit_url.trim().trim_end_matches('/').to_string() } else { v })
}

fn hint(token: &str) -> String {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() <= 10 {
        return "…".into();
    }
    let start: String = chars[..7].iter().collect();
    let end: String = chars[chars.len() - 2..].iter().collect();
    format!("{start}…{end}")
}

/// Verständliche Ursache eines Verbindungsfehlers (statt „error sending request“) mit Hinweis.
pub fn describe_error(e: &reqwest::Error) -> String {
    let mut chain = Vec::new();
    let mut cur: Option<&dyn std::error::Error> = Some(e);
    while let Some(err) = cur {
        chain.push(err.to_string().to_lowercase());
        cur = err.source();
    }
    let all = chain.join(" / ");
    let reason = if e.is_timeout() || all.contains("timed out") {
        "keine Antwort (Zeitüberschreitung)"
    } else if all.contains("dns") || all.contains("lookup") || all.contains("name or service not known") || all.contains("nodename nor servname") {
        "der Name lässt sich hier nicht auflösen"
    } else if all.contains("refused") {
        "Verbindung abgelehnt – auf diesem Port läuft nichts"
    } else if all.contains("unreachable") || all.contains("no route") {
        "Netz nicht erreichbar"
    } else if all.contains("certificate") || all.contains("tls") {
        "Problem mit dem HTTPS-Zertifikat"
    } else {
        "Verbindung fehlgeschlagen"
    };
    format!(
        "{reason}. Der Server von nrzCode verbindet sich selbst – im Docker-Container ist meist eine interne Adresse nötig, z. B. http://nrz-git_app_1:8380 (nrzGit zeigt alle unter Einstellungen → MCP). „Automatisch suchen“ probiert sie durch."
    )
}

// ------------------------------------------------------------------ Client

/// HTTP-Zugriff auf die API von nrzGit mit dem Token des Benutzers.
pub struct Client {
    url: String,
    token: String,
    /// `KIND_NRZGIT`, `KIND_LUGIT` oder `KIND_GITHUB`
    kind: String,
    http: reqwest::Client,
}

/// Meldung aus einer Fehlerantwort: nrzGit und luGit `{ "error": { "message" } }`, GitHub
/// `{ "message", "errors": [{ "message" }] }`.
fn error_message(text: &str) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return String::new();
    };
    if let Some(m) = v["error"]["message"].as_str() {
        return m.to_string();
    }
    let main = v["message"].as_str().unwrap_or("").to_string();
    let detail = v["errors"].as_array().and_then(|l| l.iter().find_map(|e| e["message"].as_str())).unwrap_or("");
    if detail.is_empty() || main.is_empty() {
        format!("{main}{detail}")
    } else {
        format!("{}: {detail}", main.trim_end_matches('.'))
    }
}

impl Client {
    pub fn new(url: &str, token: &str, kind: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        Client { url: url.trim_end_matches('/').to_string(), token: token.to_string(), kind: kind.to_string(), http }
    }

    pub fn of(access: &NrzGitAccess) -> Self {
        Self::new(&access.url, &access.token, &access.kind)
    }

    fn lugit(&self) -> bool {
        self.kind == KIND_LUGIT
    }

    fn github(&self) -> bool {
        self.kind == KIND_GITHUB
    }

    async fn send<T: DeserializeOwned>(&self, req: reqwest::RequestBuilder) -> AppResult<T> {
        let forge = forge_name(&self.kind);
        let req = if self.github() {
            req.header("Authorization", format!("Bearer {}", self.token))
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28")
                .header("User-Agent", "nrzCode")
        } else {
            req.header("Authorization", format!("token {}", self.token)).header("Accept", "application/json")
        };
        let res = req
            .send()
            .await
            .map_err(|e| AppError::bad(format!("{forge} ist unter {} nicht erreichbar: {}", self.url, describe_error(&e))))?;
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        if status.is_success() {
            return serde_json::from_str(&text)
                .map_err(|_| AppError::bad(format!("{forge} hat unerwartet geantwortet – stimmt die Adresse?")));
        }
        if status.as_u16() == 401 {
            return Err(AppError::bad(format!("{forge} hat das Token abgelehnt.")));
        }
        if status.as_u16() == 403 && self.lugit() {
            return Err(AppError::bad("luGit: Dem Token fehlt ein Recht – es braucht „user“ und „repo“ (luGit → Einstellungen → Zugriffstoken)."));
        }
        let message = error_message(&text);
        if status.as_u16() == 403 && self.github() && !message.to_lowercase().contains("rate limit") {
            return Err(AppError::bad(format!(
                "GitHub: Dem Token fehlt ein Recht ({message}) – klassisch braucht es „repo“, fein eingestellt Contents, Pull requests und Administration (Lesen und Schreiben)."
            )));
        }
        let message = if message.is_empty() { format!("Fehler {}", status.as_u16()) } else { message };
        Err(match status.as_u16() {
            404 => AppError::bad(format!("{forge}: {message}")),
            409 => AppError::conflict(format!("{forge}: {message}")),
            _ => AppError::bad(format!("{forge}: {message}")),
        })
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> AppResult<T> {
        self.send(self.http.get(format!("{}{path}", api_base(&self.url, &self.kind)))).await
    }

    pub async fn post<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> AppResult<T> {
        self.send(self.http.post(format!("{}{path}", api_base(&self.url, &self.kind))).json(body)).await
    }
}

/// Teil von `Me` in nrzGit.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RemoteMe {
    pub username: String,
    pub full_name: String,
    /// luGit nennt die Liste `orgs`
    #[serde(alias = "orgs")]
    pub organizations: Vec<RemoteUser>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RemoteUser {
    pub username: String,
    pub full_name: String,
    pub is_org: bool,
}

/// Teil von `Repo` in nrzGit.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RemoteRepo {
    pub id: i64,
    pub owner: RemoteUser,
    pub name: String,
    pub full_name: String,
    pub description: String,
    pub default_branch: String,
    pub private: bool,
    pub empty: bool,
    pub clone_url: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct RemotePaged<T> {
    items: Vec<T>,
    total: i64,
}

/// Neues Repository in nrzGit (`POST /api/v1/repos`).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCreateRepo {
    pub owner: String,
    pub name: String,
    pub description: String,
    pub private: Option<bool>,
    pub auto_init: bool,
}

/// Neuer Pull Request (`POST /api/v1/repos/<o>/<r>/pulls`).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteCreatePull {
    pub base: String,
    pub head: String,
    pub title: String,
    pub body: String,
    pub draft: bool,
}

/// Teil der Antwort auf einen neuen Pull Request (ein Issue).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RemoteIssue {
    pub number: i64,
    pub title: String,
}

/// Benutzer oder Organisation bei GitHub.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct GhUser {
    login: String,
    name: Option<String>,
    #[serde(rename = "type")]
    kind: String,
}

/// Repository bei GitHub.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct GhRepo {
    id: i64,
    owner: GhUser,
    name: String,
    full_name: String,
    description: Option<String>,
    default_branch: String,
    private: bool,
    /// Grösse in KB – 0 bei einem Repository ohne Commit
    size: i64,
    clone_url: String,
    updated_at: Option<String>,
    pushed_at: Option<String>,
}

impl From<GhUser> for RemoteUser {
    fn from(u: GhUser) -> Self {
        RemoteUser { is_org: u.kind == "Organization", full_name: u.name.unwrap_or_default(), username: u.login }
    }
}

impl From<GhRepo> for RemoteRepo {
    fn from(r: GhRepo) -> Self {
        let updated = r.pushed_at.as_deref().or(r.updated_at.as_deref()).map(unix_from_iso).unwrap_or(0);
        RemoteRepo {
            id: r.id,
            owner: r.owner.into(),
            name: r.name,
            full_name: r.full_name,
            description: r.description.unwrap_or_default(),
            default_branch: if r.default_branch.is_empty() { "main".into() } else { r.default_branch },
            private: r.private,
            empty: r.size == 0,
            clone_url: r.clone_url,
            updated_at: updated,
        }
    }
}

impl Client {
    pub async fn me(&self) -> AppResult<RemoteMe> {
        if self.github() {
            let user: GhUser = self.get("/user").await?;
            // Fein eingestellte Token dürfen die Organisationen evtl. nicht lesen – dann ohne
            let orgs: Vec<GhUser> = self.get("/user/orgs?per_page=100").await.unwrap_or_default();
            return Ok(RemoteMe {
                username: user.login,
                full_name: user.name.unwrap_or_default(),
                organizations: orgs.into_iter().map(|o| RemoteUser { username: o.login, full_name: o.name.unwrap_or_default(), is_org: true }).collect(),
            });
        }
        self.get("/me").await
    }

    /// Alle Repositories, an denen der Benutzer mitarbeitet (inkl. Organisationen), höchstens 1000.
    pub async fn repos(&self, q: &str) -> AppResult<Vec<RemoteRepo>> {
        let mut all = Vec::new();
        if self.github() {
            // GitHub kann die eigene Liste nicht durchsuchen – hier filtern
            let needle = q.to_lowercase();
            for page in 1..=10 {
                let list: Vec<GhRepo> = self.get(&format!("/user/repos?per_page=100&page={page}&sort=pushed")).await?;
                let n = list.len();
                all.extend(
                    list.into_iter()
                        .filter(|r| needle.is_empty() || r.full_name.to_lowercase().contains(&needle) || r.description.as_deref().unwrap_or("").to_lowercase().contains(&needle))
                        .map(RemoteRepo::from),
                );
                if n < 100 {
                    break;
                }
            }
            return Ok(all);
        }
        // 100 je Seite: mehr liefert luGit nicht (nrzGit bis 200)
        for page in 1..=10 {
            let p: RemotePaged<RemoteRepo> =
                self.get(&format!("/me/repos?limit=100&page={page}&q={}", urlencoding::encode(q))).await?;
            let n = p.items.len();
            all.extend(p.items);
            if n < 100 || all.len() as i64 >= p.total {
                break;
            }
        }
        Ok(all)
    }

    pub async fn repo(&self, full_name: &str) -> AppResult<RemoteRepo> {
        let (owner, name) = split_repo(full_name)?;
        let path = format!("/repos/{}/{}", urlencoding::encode(owner), urlencoding::encode(name));
        if self.github() {
            return Ok(self.get::<GhRepo>(&path).await?.into());
        }
        self.get(&path).await
    }

    pub async fn create_repo(&self, body: &RemoteCreateRepo) -> AppResult<RemoteRepo> {
        if self.github() {
            // Wie luGit: der Besitzer steht im Pfad; ohne Angabe wäre ein Repository bei GitHub öffentlich
            let path = if body.owner.is_empty() { "/user/repos".to_string() } else { format!("/orgs/{}/repos", urlencoding::encode(&body.owner)) };
            let req = serde_json::json!({
                "name": body.name, "description": body.description, "private": body.private.unwrap_or(true), "auto_init": body.auto_init,
            });
            return Ok(self.post::<GhRepo, _>(&path, &req).await?.into());
        }
        if self.lugit() {
            // luGit: der Besitzer steht im Pfad, nicht im Körper
            let me = body.owner.is_empty() || body.owner.eq_ignore_ascii_case(&self.me().await?.username);
            let path = if me { "/user/repos".to_string() } else { format!("/orgs/{}/repos", urlencoding::encode(&body.owner)) };
            return self.post(&path, body).await;
        }
        self.post("/repos", body).await
    }

    pub async fn create_pull(&self, full_name: &str, body: &RemoteCreatePull) -> AppResult<RemoteIssue> {
        let (owner, name) = split_repo(full_name)?;
        self.post(&format!("/repos/{}/{}/pulls", urlencoding::encode(owner), urlencoding::encode(name)), body).await
    }
}

/// `owner/name` zerlegen und prüfen.
pub fn split_repo(full_name: &str) -> AppResult<(&str, &str)> {
    let ok = |s: &str| {
        !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) && !s.starts_with('.')
    };
    match full_name.trim().split_once('/') {
        Some((o, n)) if ok(o) && ok(n) => Ok((o, n)),
        _ => Err(AppError::bad("Repository bitte als „besitzer/name“ angeben.")),
    }
}

/// Adresse zum Klonen: die von nrzGit gemeldete, wenn sie auf derselben Instanz liegt, sonst
/// `<url>/<owner>/<name>.git`. Nie eine andere Adresse – sonst ginge das Token woandershin.
pub fn clone_url(access_url: &str, repo: &RemoteRepo) -> String {
    let reported = repo.clone_url.trim();
    let same_origin = reported.starts_with(&format!("{access_url}/"));
    if same_origin && !reported.contains('@') {
        reported.to_string()
    } else {
        format!("{access_url}/{}.git", repo.full_name)
    }
}

/// Zugang eines Benutzers, nur wenn die Verbindung geprüft ist – sonst eine verständliche Meldung.
/// `kind`: gewünschte Plattform (`None` = die erste Verbindung, nrzGit vor luGit).
pub async fn require(state: &AppState, user_id: i64, kind: Option<&str>) -> AppResult<NrzGitAccess> {
    let found = match kind {
        Some(k) => super::forge_access(state, user_id, k).await?,
        None => super::nrzgit_access(state, user_id).await?,
    };
    match (found, kind) {
        (Some(access), _) => Ok(access),
        (None, Some(k)) => Err(AppError::bad(format!("Bitte zuerst {} verbinden (Einstellungen → Git-Plattformen).", forge_name(k)))),
        (None, None) => Err(AppError::bad("Bitte zuerst nrzGit, luGit oder GitHub verbinden (Einstellungen → Git-Plattformen).")),
    }
}

// ------------------------------------------------------------------ Routen

type ConnRow = (String, String, String, Option<i64>, String, String, String, bool);

/// Gespeicherte Verbindungen des Benutzers (auch ungeprüfte), nrzGit zuerst.
async fn rows(state: &AppState, user_id: i64) -> AppResult<Vec<ConnRow>> {
    Ok(sqlx::query_as(
        "SELECT url, username, token, verified_at, last_error, public_url, kind, has_mcp FROM nrzgit_connections WHERE user_id = ? \
         ORDER BY CASE kind WHEN 'nrzgit' THEN 0 WHEN 'lugit' THEN 1 ELSE 2 END",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?)
}

fn status_from(row: ConnRow, default_url: &str) -> NrzGitStatus {
    let (url, username, token, verified_at, error, public_url, kind, has_mcp) = row;
    NrzGitStatus {
        connected: verified_at.is_some(),
        token_url: token_url(if public_url.is_empty() { &url } else { &public_url }, &kind),
        has_mcp: has_mcp || kind == KIND_NRZGIT,
        default_url: default_for(&kind, default_url),
        kind,
        public_url,
        url,
        username,
        token_hint: hint(&token),
        verified_at,
        error,
    }
}

/// Vorgabe-Adresse je Plattform: die der Instanz gilt für nrzGit, GitHub ist github.com, luGit hat keine.
fn default_for(kind: &str, default_url: &str) -> String {
    match kind {
        KIND_LUGIT => String::new(),
        KIND_GITHUB => GITHUB_URL.to_string(),
        _ => default_url.to_string(),
    }
}

/// Leerer Stand einer nicht verbundenen Plattform.
fn status_empty(kind: &str, default_url: &str) -> NrzGitStatus {
    let default_url = default_for(kind, default_url);
    NrzGitStatus { token_url: token_url(&default_url, kind), kind: kind.into(), has_mcp: kind != KIND_LUGIT, default_url, ..Default::default() }
}

/// Stand einer Verbindung: die der gewünschten Plattform, ohne Angabe die erste (nrzGit vor luGit).
async fn status_of(state: &AppState, user_id: i64, kind: Option<&str>) -> AppResult<NrzGitStatus> {
    let default_url = default_url(state).await?;
    let row = rows(state, user_id).await?.into_iter().find(|r| kind.is_none_or(|k| r.6 == k));
    Ok(match row {
        Some(row) => status_from(row, &default_url),
        None => status_empty(kind.unwrap_or(KIND_NRZGIT), &default_url),
    })
}

/// `GET /nrzgit?kind=` – Stand der Verbindung (ohne Token).
#[get("/nrzgit?<kind>")]
async fn status(state: &State<AppState>, user: CurrentUser, kind: Option<&str>) -> AppResult<Json<NrzGitStatus>> {
    Ok(Json(status_of(state, user.id(), parse_kind(kind)?).await?))
}

/// `GET /nrzgit/connections` – alle Plattformen (nrzGit, luGit, GitHub), verbunden oder nicht.
#[get("/nrzgit/connections")]
async fn connections(state: &State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<NrzGitStatus>>> {
    let default_url = default_url(state).await?;
    let mut rows = rows(state, user.id()).await?;
    let mut out = Vec::new();
    for kind in KINDS {
        out.push(match rows.iter().position(|r| r.6 == kind) {
            Some(i) => status_from(rows.remove(i), &default_url),
            None => status_empty(kind, &default_url),
        });
    }
    Ok(Json(out))
}

/// `PUT /nrzgit` – Adresse und Token speichern; wird sofort geprüft und nur gespeichert, wenn
/// die Plattform das Token annimmt. Welche Plattform (nrzGit oder luGit) antwortet, wird erkannt;
/// die Verbindung der anderen bleibt bestehen.
#[put("/nrzgit", data = "<body>")]
async fn connect(state: &State<AppState>, user: CurrentUser, body: Json<NrzGitConnect>) -> AppResult<Json<NrzGitStatus>> {
    let wanted = parse_kind(Some(&body.kind))?;
    // GitHub ohne Adresse: github.com
    let url = if body.url.trim().is_empty() && wanted == Some(KIND_GITHUB) { GITHUB_URL.to_string() } else { normalize_url(&body.url)? };
    let kind = detect_kind(&url).await;
    if let Some(w) = wanted.filter(|w| *w != kind) {
        return Err(AppError::bad(format!(
            "Unter dieser Adresse antwortet {}, nicht {} – bitte dort eintragen.",
            forge_name(kind),
            forge_name(w)
        )));
    }
    let mut token = body.token.trim().to_string();
    if token.is_empty() {
        // Gespeichertes Token nur für dieselbe Adresse weiterverwenden
        let old: Option<(String, String)> = sqlx::query_as("SELECT url, token FROM nrzgit_connections WHERE user_id = ? AND kind = ?")
            .bind(user.id())
            .bind(kind)
            .fetch_optional(&state.db)
            .await?;
        match old {
            Some((old_url, old_token)) if old_url == url => token = old_token,
            _ => return Err(AppError::bad(format!("Bitte ein Zugriffstoken von {} einfügen.", forge_name(kind)))),
        }
    }
    if token.len() > 500 || token.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(AppError::bad("Das Token ist ungültig."));
    }
    let me = Client::new(&url, &token, kind).me().await?;
    if me.username.is_empty() {
        return Err(AppError::bad(format!("{} hat unerwartet geantwortet – stimmt die Adresse?", forge_name(kind))));
    }
    let has_mcp = detect_mcp(&url, kind).await;
    let public_url = if body.public_url.trim().is_empty() { String::new() } else { normalize_url(&body.public_url)? };
    let ts = now();
    sqlx::query(
        "INSERT INTO nrzgit_connections (user_id, kind, url, token, username, verified_at, last_error, public_url, has_mcp, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, '', ?, ?, ?, ?) ON CONFLICT (user_id, kind) DO UPDATE SET url = excluded.url, token = excluded.token, \
         username = excluded.username, verified_at = excluded.verified_at, last_error = '', public_url = excluded.public_url, \
         has_mcp = excluded.has_mcp, updated_at = excluded.updated_at",
    )
    .bind(user.id())
    .bind(kind)
    .bind(&url)
    .bind(&token)
    .bind(&me.username)
    .bind(ts)
    .bind(&public_url)
    .bind(has_mcp)
    .bind(ts)
    .bind(ts)
    .execute(&state.db)
    .await?;
    super::gitops::write_docker_config(state, user.id()).await?;
    Ok(Json(status_of(state, user.id(), Some(kind)).await?))
}

/// `POST /nrzgit/verify?kind=` – gespeicherte Verbindung erneut prüfen (auch, ob die Plattform
/// inzwischen einen MCP-Server hat).
#[post("/nrzgit/verify?<kind>")]
async fn verify(state: &State<AppState>, user: CurrentUser, kind: Option<&str>) -> AppResult<Json<NrzGitStatus>> {
    let wanted = parse_kind(kind)?;
    let row = rows(state, user.id()).await?.into_iter().find(|r| wanted.is_none_or(|k| r.6 == k));
    let Some((url, _, token, _, _, _, kind, _)) = row else {
        return Err(AppError::bad(format!("Es ist keine Verbindung zu {} gespeichert.", forge_name(wanted.unwrap_or(KIND_NRZGIT)))));
    };
    // Die Art bleibt, wie sie gespeichert ist – antwortet dort inzwischen die andere Plattform, ist das ein Fehler
    let detected = detect_kind(&url).await;
    let result = if detected != kind && probe_ok(&url).await {
        Err(AppError::bad(format!("Unter {url} antwortet jetzt {}, nicht {}.", forge_name(detected), forge_name(&kind))))
    } else {
        Client::new(&url, &token, &kind).me().await
    };
    match result {
        Ok(me) => {
            let has_mcp = detect_mcp(&url, &kind).await;
            sqlx::query("UPDATE nrzgit_connections SET username = ?, has_mcp = ?, verified_at = ?, last_error = '', updated_at = ? WHERE user_id = ? AND kind = ?")
                .bind(&me.username)
                .bind(has_mcp)
                .bind(now())
                .bind(now())
                .bind(user.id())
                .bind(&kind)
                .execute(&state.db)
                .await?;
        }
        Err(e) => {
            sqlx::query("UPDATE nrzgit_connections SET verified_at = NULL, last_error = ?, updated_at = ? WHERE user_id = ? AND kind = ?")
                .bind(e.public_message())
                .bind(now())
                .bind(user.id())
                .bind(&kind)
                .execute(&state.db)
                .await?;
        }
    }
    super::gitops::write_docker_config(state, user.id()).await?;
    Ok(Json(status_of(state, user.id(), Some(&kind)).await?))
}

/// `DELETE /nrzgit?kind=` – Verbindung trennen (Token löschen); ohne Angabe die erste.
#[delete("/nrzgit?<kind>")]
async fn disconnect(state: &State<AppState>, user: CurrentUser, kind: Option<&str>) -> AppResult<Status> {
    let wanted = parse_kind(kind)?;
    let found = rows(state, user.id()).await?.into_iter().find(|r| wanted.is_none_or(|k| r.6 == k));
    if let Some(row) = found {
        sqlx::query("DELETE FROM nrzgit_connections WHERE user_id = ? AND kind = ?").bind(user.id()).bind(&row.6).execute(&state.db).await?;
    }
    super::gitops::write_docker_config(state, user.id()).await?;
    Ok(Status::NoContent)
}

/// `GET /nrzgit/repos?q=&kind=` – Repositories der Plattform (eigene, Organisationen, Mitarbeit)
/// mit Verweis auf schon übernommene Projekte.
#[get("/nrzgit/repos?<q>&<kind>")]
async fn repos(state: &State<AppState>, user: CurrentUser, q: Option<&str>, kind: Option<&str>) -> AppResult<Json<Vec<NrzGitRepo>>> {
    let access = require(state, user.id(), parse_kind(kind)?).await?;
    let list = Client::of(&access).repos(q.unwrap_or("").trim()).await?;
    let projects: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, lower(repo) FROM projects WHERE user_id = ? AND repo <> '' AND (CASE forge WHEN '' THEN 'nrzgit' ELSE forge END) = ?",
    )
    .bind(user.id())
    .bind(&access.kind)
    .fetch_all(&state.db)
    .await?;
    let mut out: Vec<NrzGitRepo> = list
        .into_iter()
        .map(|r| NrzGitRepo {
            project_id: projects.iter().find(|(_, n)| *n == r.full_name.to_lowercase()).map(|(id, _)| *id),
            web_url: format!("{}/{}", access.public_url, r.full_name),
            forge: access.kind.clone(),
            id: r.id,
            owner: r.owner.username,
            owner_is_org: r.owner.is_org,
            full_name: r.full_name,
            name: r.name,
            description: r.description,
            private: r.private,
            empty: r.empty,
            default_branch: r.default_branch,
            updated_at: r.updated_at,
        })
        .collect();
    out.sort_by_key(|r| std::cmp::Reverse(r.updated_at));
    Ok(Json(out))
}

/// `GET /nrzgit/owners?kind=` – ich und meine Organisationen (Besitzer neuer Repositories).
#[get("/nrzgit/owners?<kind>")]
async fn owners(state: &State<AppState>, user: CurrentUser, kind: Option<&str>) -> AppResult<Json<Vec<NrzGitOwner>>> {
    let access = require(state, user.id(), parse_kind(kind)?).await?;
    let me = Client::of(&access).me().await?;
    let mut out = vec![NrzGitOwner { username: me.username.clone(), full_name: me.full_name.clone(), is_org: false }];
    out.extend(me.organizations.into_iter().map(|o| NrzGitOwner { username: o.username, full_name: o.full_name, is_org: true }));
    Ok(Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(normalize_url(" umbrel.local:8390/ ").unwrap(), "http://umbrel.local:8390");
        assert_eq!(normalize_url("https://git.example.com").unwrap(), "https://git.example.com");
        assert!(normalize_url("http://user:pw@host").is_err());
        assert!(normalize_url("").is_err());
        assert_eq!(host_of("http://umbrel.local:8390/x"), "umbrel.local:8390");
        assert_eq!(hint("nrzg_abcdefghijklmnop"), "nrzg_ab…op");
    }

    #[test]
    fn clone_urls_stay_on_the_instance() {
        let mut r = RemoteRepo { full_name: "anna/demo".into(), clone_url: "http://git:8390/anna/demo.git".into(), ..Default::default() };
        assert_eq!(clone_url("http://git:8390", &r), "http://git:8390/anna/demo.git");
        r.clone_url = "/etc/passwd".into();
        assert_eq!(clone_url("http://git:8390", &r), "http://git:8390/anna/demo.git");
        r.clone_url = "http://git:8390.evil.com/x.git".into();
        assert_eq!(clone_url("http://git:8390", &r), "http://git:8390/anna/demo.git");
    }

    #[test]
    fn lugit_differs_in_token_page_and_orgs() {
        assert_eq!(token_url("http://git:8390", KIND_NRZGIT), "http://git:8390/user/settings/mcp");
        assert_eq!(token_url("http://git:8430", KIND_LUGIT), "http://git:8430/user/settings/tokens");
        assert_eq!(token_url("", KIND_LUGIT), "");
        assert_eq!((forge_name(KIND_LUGIT), forge_name(KIND_NRZGIT), forge_name("")), ("luGit", "nrzGit", "nrzGit"));
        // nrzGit: `organizations`, luGit: `orgs`
        let a: RemoteMe = serde_json::from_str(r#"{"username":"anna","organizations":[{"username":"team","isOrg":true}]}"#).unwrap();
        let b: RemoteMe = serde_json::from_str(r#"{"username":"luna","orgs":[{"username":"LunaOS","isOrg":true}]}"#).unwrap();
        assert_eq!((a.organizations[0].username.as_str(), b.organizations[0].username.as_str()), ("team", "LunaOS"));
    }

    #[test]
    fn github_shapes() {
        assert_eq!(normalize_url("github.com").unwrap(), GITHUB_URL);
        assert_eq!(normalize_url("https://www.github.com/luna/").unwrap(), GITHUB_URL);
        assert_eq!(normalize_url("https://api.github.com").unwrap(), GITHUB_URL);
        assert_eq!(normalize_url("https://ghe.example.com").unwrap(), "https://ghe.example.com");
        assert_eq!(api_base(GITHUB_URL, KIND_GITHUB), "https://api.github.com");
        assert_eq!(api_base("https://ghe.example.com", KIND_GITHUB), "https://ghe.example.com/api/v3");
        assert_eq!(api_base("http://git:8390", KIND_NRZGIT), "http://git:8390/api/v1");
        assert_eq!(registry_host(GITHUB_URL, KIND_GITHUB), "ghcr.io");
        assert_eq!(registry_host("http://git:8430", KIND_LUGIT), "git:8430");
        assert_eq!(token_url(GITHUB_URL, KIND_GITHUB), "https://github.com/settings/tokens");
        assert_eq!(pull_url(GITHUB_URL, KIND_GITHUB, "luna/demo", 7), "https://github.com/luna/demo/pull/7");
        assert_eq!(pull_url("http://git:8390", KIND_NRZGIT, "anna/demo", 7), "http://git:8390/anna/demo/pulls/7");
        assert_eq!(unix_from_iso("1970-01-01T00:00:00Z"), 0);
        assert_eq!(unix_from_iso("2024-02-29T12:30:05Z"), 1_709_209_805);
        assert_eq!(unix_from_iso("kaputt"), 0);
        let r: GhRepo = serde_json::from_str(
            r#"{"id":5,"name":"demo","full_name":"Luna/demo","description":null,"default_branch":"main","private":true,"size":0,
                "clone_url":"https://github.com/Luna/demo.git","pushed_at":"2024-02-29T12:30:05Z","owner":{"login":"Luna","type":"User"}}"#,
        )
        .unwrap();
        let r = RemoteRepo::from(r);
        assert_eq!((r.full_name.as_str(), r.owner.username.as_str(), r.owner.is_org, r.empty, r.private, r.updated_at), ("Luna/demo", "Luna", false, true, true, 1_709_209_805));
        assert_eq!(clone_url(GITHUB_URL, &r), "https://github.com/Luna/demo.git");
        assert_eq!(error_message(r#"{"message":"Validation Failed","errors":[{"message":"name already exists on this account"}]}"#), "Validation Failed: name already exists on this account");
        assert_eq!(error_message(r#"{"error":{"message":"Kein Zugriff."}}"#), "Kein Zugriff.");
    }

    #[test]
    fn repo_names() {
        assert!(split_repo("anna/demo").is_ok());
        assert!(split_repo("anna/../x").is_err());
        assert!(split_repo("anna").is_err());
    }
}

/// Gateway des Docker-Netzes (Linux `/proc/net/route`) – dort ist der Host erreichbar.
fn default_gateway() -> Option<std::net::Ipv4Addr> {
    let table = std::fs::read_to_string("/proc/net/route").ok()?;
    table.lines().skip(1).find_map(|line| {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() > 2 && cols[1] == "00000000" {
            let raw = u32::from_str_radix(cols[2], 16).ok()?;
            Some(std::net::Ipv4Addr::from(raw.to_le_bytes())).filter(|ip| !ip.is_unspecified())
        } else {
            None
        }
    })
}

/// Antwortet unter `url` erkennbar eine der beiden Plattformen?
async fn probe_ok(url: &str) -> bool {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .connect_timeout(Duration::from_secs(4))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default();
    probe(&http, url).await.is_ok()
}

/// Antwortet unter `url` ein nrzGit, luGit oder GitHub Enterprise? (Art und Version oder
/// verständlicher Fehler)
async fn probe(http: &reqwest::Client, url: &str) -> Result<(&'static str, String), String> {
    if url == GITHUB_URL {
        return Ok((KIND_GITHUB, String::new()));
    }
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Health {
        name: String,
        version: String,
        ok: bool,
    }
    let res = http.get(format!("{url}/api/v1/health")).send().await.map_err(|e| {
        let full = describe_error(&e);
        full.split(". ").next().unwrap_or(&full).to_string()
    })?;
    if let Ok(h) = res.json::<Health>().await {
        if h.name == "nrzGit" {
            return Ok((KIND_NRZGIT, h.version));
        }
        if !h.name.is_empty() {
            return Err(format!("antwortet als {}", h.name));
        }
    }
    // luGit: `/api/health` → `{"ok": true, "version": "…"}`
    if let Ok(res) = http.get(format!("{url}/api/health")).send().await {
        if let Ok(h) = res.json::<Health>().await {
            if h.ok && !h.version.is_empty() {
                return Ok((KIND_LUGIT, h.version));
            }
        }
    }
    // GitHub Enterprise Server: die Wurzel der API nennt ihre Adressen
    if let Ok(res) = http.get(format!("{url}/api/v3")).header("User-Agent", "nrzCode").send().await {
        if let Ok(v) = res.json::<serde_json::Value>().await {
            if v.get("current_user_url").is_some() {
                return Ok((KIND_GITHUB, String::new()));
            }
        }
    }
    Err("antwortet, ist aber weder nrzGit noch luGit noch GitHub".to_string())
}

/// `GET /nrzgit/candidates?url=` – übliche Adressen von nrzGit und luGit vom Server aus durchprobieren
/// (eingegebene Adresse, Vorgabe, Docker-Name auf Umbrel, Gateway des Docker-Netzes, Host).
#[get("/nrzgit/candidates?<url>")]
async fn candidates(state: &State<AppState>, user: CurrentUser, url: Option<String>) -> AppResult<Json<Vec<NrzGitCandidate>>> {
    let mut list: Vec<(String, String)> = Vec::new();
    let mut add = |u: String, label: &str| {
        if let Ok(u) = normalize_url(&u) {
            if !list.iter().any(|(x, _)| *x == u) {
                list.push((u, label.to_string()));
            }
        }
    };
    if let Some(u) = url.filter(|u| !u.trim().is_empty()) {
        add(u, "Eingegeben");
    }
    for a in super::forge_accesses(state, user.id()).await? {
        add(a.url, "Gespeichert");
    }
    let default = default_url(state).await?;
    if !default.is_empty() {
        add(default, "Vorgabe der Instanz");
    }
    add("http://nrz-git_app_1:8380".into(), "Docker-Name auf Umbrel (empfohlen)");
    if let Some(gw) = default_gateway() {
        add(format!("http://{gw}:8390"), "Host über das Docker-Gateway (App-Proxy)");
        add(format!("http://{gw}:8380"), "Host über das Docker-Gateway");
    }
    add("http://host.docker.internal:8390".into(), "Host (Docker Desktop)");
    add("http://umbrel.local:8390".into(), "umbrel.local");
    add("http://localhost:8390".into(), "Gleicher Rechner");
    add("http://localhost:8380".into(), "Gleicher Rechner (direkt)");
    // luGit (Port 8430)
    if let Some(gw) = default_gateway() {
        add(format!("http://{gw}:8430"), "luGit über das Docker-Gateway");
    }
    add("http://lu-git_app_1:8430".into(), "luGit: Docker-Name auf Umbrel");
    add("http://umbrel.local:8430".into(), "luGit auf umbrel.local");
    add("http://localhost:8430".into(), "luGit auf dem gleichen Rechner");

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(4))
        .connect_timeout(Duration::from_secs(3))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_default();
    let checks = list.iter().map(|(u, _)| probe(&http, u));
    let results = futures::future::join_all(checks).await;
    let mut out: Vec<NrzGitCandidate> = list
        .into_iter()
        .zip(results)
        .map(|((url, label), r)| match r {
            Ok((kind, version)) => NrzGitCandidate { url, label, reachable: true, version, error: String::new(), kind: kind.into() },
            Err(error) => NrzGitCandidate { url, label, reachable: false, version: String::new(), error, kind: String::new() },
        })
        .collect();
    // Erreichbare zuerst, sonst Reihenfolge wie oben
    out.sort_by_key(|c| !c.reachable);
    Ok(Json(out))
}
