//! Dieter versteht einen gesprochenen Satz (Text aus Whisper, mit Hörfehlern) und macht daraus
//! Aufträge für die Fenster des Rasters: Ein schneller Agent (`runs::oneshot_user`, nur lesen, in
//! einem leeren Ordner) bekommt die Lage aller Fenster und antwortet mit `{reply, actions}`.
//! Für Aufträge schreibt er einen ausführlichen, klaren Prompt statt den Satz wörtlich zu
//! übernehmen. Ausgeführt werden die Aktionen in der Oberfläche (sie kennt das Raster).

use crate::agents::messages::clip;
use crate::agents::threads::{self, ThreadRow};
use crate::agents::{accounts, kinds::spec, runs, tools};
use crate::error::{AppError, AppResult};
use crate::AppState;
use nrzcode_shared::agents::ToolCall;
use nrzcode_shared::voice::{DieterAction, DieterReply, DieterRequest, VoiceSettings};
use nrzcode_shared::{AgentKind, ThreadStatus};

const HINT: &str = "You are Dieter, the voice assistant of nrzCode. You turn spoken commands into actions for \
                    coding-agent chat windows and answer with exactly one JSON object. Never use tools, never \
                    read or modify files.";

/// Antwort und Aktionen zu einem gesprochenen Satz.
pub async fn answer(state: &AppState, user_id: i64, s: &VoiceSettings, mut req: DieterRequest) -> AppResult<DieterReply> {
    if req.text.trim().is_empty() {
        return Ok(DieterReply::default());
    }
    // Canvas: die Zeichnung des Arbeitsbereichs (auch was Agenten gezeichnet haben)
    if req.mode == "canvas" && req.board.trim().is_empty() {
        req.board = crate::canvas::describe(state, user_id, req.workspace_id.unwrap_or(0)).await.unwrap_or_default();
    }
    let pick = pick(state, user_id, s).await?;
    let prompt = build_prompt(state, user_id, &req).await?;
    let raw = runs::oneshot_user(state, user_id, &prompt, pick, HINT.to_string()).await?;
    Ok(parse(&raw, &req))
}

/// Gewählter Agent oder automatisch der erste mit Konto (Claude, Codex, Gemini …).
async fn pick(state: &AppState, user_id: i64, s: &VoiceSettings) -> AppResult<runs::SidePick> {
    if let Some(agent) = s.agent {
        return Ok(runs::SidePick { agent, account_id: s.account_id, model: s.model.clone(), effort: String::new() });
    }
    for kind in [AgentKind::Claude, AgentKind::Codex, AgentKind::Gemini, AgentKind::Opencode, AgentKind::Kilo, AgentKind::Grok] {
        if tools::find_agent(state, kind).is_some() && accounts::default_for(state, user_id, kind).await?.is_some() {
            return Ok(runs::SidePick { agent: kind, account_id: None, model: String::new(), effort: String::new() });
        }
    }
    Err(AppError::bad("Für Dieter ist noch kein Agent mit Konto eingerichtet (Einstellungen → Agenten)."))
}

/// Schnelles Modell, wenn keines gewählt ist.
pub fn fast_model(kind: AgentKind) -> &'static str {
    match kind {
        AgentKind::Claude => "haiku",
        _ => "",
    }
}

/// Fertige Stufe für einen schnellen Lauf („low“, wenn der Agent Stufen kennt).
pub fn fast_effort(kind: AgentKind) -> String {
    if spec(kind).effort("low").is_some() {
        "low".into()
    } else {
        String::new()
    }
}

fn clean_name(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-').take(24).collect::<String>().trim().to_string()
}

async fn build_prompt(state: &AppState, user_id: i64, req: &DieterRequest) -> AppResult<String> {
    let canvas = req.mode == "canvas";
    let mut panes = String::new();
    for p in req.panes.iter().take(if canvas { 24 } else { 8 }) {
        let name = clean_name(&p.name);
        if name.is_empty() {
            continue;
        }
        let big = if req.focused.as_deref().is_some_and(|f| f.eq_ignore_ascii_case(&p.name)) { " [currently shown big]" } else { "" };
        let place = if p.place.is_empty() { String::new() } else { format!(" [{}]", clip(&clean_text(&p.place), 60)) };
        let big = format!("{big}{place}");
        match (p.kind.as_str(), p.thread_id) {
            ("terminal", _) => panes.push_str(&format!("- {name}{big}: terminal window (shell in {})\n", clip(&clean_text(&p.detail), 200))),
            ("browser", _) => panes.push_str(&format!("- {name}{big}: browser window showing {}\n", if p.detail.is_empty() { "nothing yet".to_string() } else { clip(&clean_text(&p.detail), 300) })),
            (_, None) => panes.push_str(&format!("- {name}{big}: empty chat window – no chat yet; a task sent here starts a new chat.\n")),
            (_, Some(id)) => match threads::get_row(state, user_id, id).await {
                Ok(t) => panes.push_str(&describe(state, &t, &name, &big).await?),
                Err(_) => panes.push_str(&format!("- {name}{big}: (chat not available)\n")),
            },
        }
    }
    if panes.is_empty() {
        panes.push_str("(no windows)\n");
    }
    let mut history = String::new();
    for turn in req.history.iter().rev().take(8).collect::<Vec<_>>().into_iter().rev() {
        let who = if turn.role == "dieter" { "Dieter" } else { "User" };
        history.push_str(&format!("{who}: {}\n", clip(turn.text.trim(), 400)));
    }
    if history.is_empty() {
        history.push_str("(nothing yet)\n");
    }
    let add = if req.can_add { "More windows can be opened (action \"new\")." } else { "No more windows can be opened (the grid is full) – do not use \"new\"." };
    let said = clip(req.text.trim(), 2000);
    if canvas {
        return Ok(canvas_prompt(req, &panes, &history, &said));
    }
    Ok(format!(
        r#"You are "Dieter", the voice assistant of nrzCode – a desktop app in which several coding agents work side by side, each in its own chat window.
The user talks to you. The text comes from automatic speech recognition and may contain recognition errors (misheard names or words, missing punctuation).

Windows (each has a first name):
{panes}{add}

Actions:
- {{"type":"send","to":"<window>","prompt":"<text>"}} – give the window a task or a message. In an empty window this starts a new chat with that prompt. If the agent there is working, the message waits and is sent when it is done.
- {{"type":"stop","to":"<window>"}} – stop the agent in that window.
- {{"type":"focus","to":"<window>"}} – show that window big; {{"type":"unfocus"}} – show all windows the same size again.
- {{"type":"new","prompt":"<text>"}} – open one more window and start a chat there.

How to write the prompt for a window:
- Write it in the user's language, directly to the coding agent (imperative), as a clear and complete task: the goal, every requirement the user mentioned, sensible defaults for missing details (say they are assumptions the agent may adapt) and what counts as done.
- Keep the user's intent; do not add requirements that change it. Never mention Dieter, voice or speech recognition.
- Usually 3–12 lines of plain text; short lists are fine.

Rules:
- Map misheard names to the most similar window name. "sag Lena, …", "Lena soll …", "an Lena: …" are tasks for Lena. One sentence can contain tasks for several windows – one action each.
- If it is unclear which window is meant or what should be done, ask one short question in "reply" and take no action.
- Questions about the state ("Status?", "Was macht Benni?", "Ist Lena fertig?"): answer in "reply" from the window information above – spoken style, one or two short sentences per window, no lists, no markdown.
- "reply" is read aloud: short and natural (usually one sentence confirming what you did), in the user's language, no markdown, no code, no emojis.
- If the words are not meant for you (noise, a fragment, the user talking to someone else), answer {{"reply":"","actions":[]}}.

Conversation so far:
{history}
The user just said: «{said}»

Answer with only the JSON object: {{"reply":"…","actions":[…]}}"#
    ))
}

fn clean_text(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Prompt im Canvas-Modus: mehr Fenster-Arten, Arbeitsbereiche, Zeichenebene und Aktionen.
fn canvas_prompt(req: &DieterRequest, panes: &str, history: &str, said: &str) -> String {
    let mut spaces = String::new();
    for w in req.workspaces.iter().take(40) {
        let w = clip(&clean_text(w), 80);
        if !w.is_empty() {
            spaces.push_str(&format!("- {w}{}\n", if clean_text(&req.workspace).eq_ignore_ascii_case(&w) { " [open now]" } else { "" }));
        }
    }
    if spaces.is_empty() {
        spaces.push_str("(none)\n");
    }
    let board = if req.board.trim().is_empty() { "(nothing drawn)\n".to_string() } else { format!("{}\n", clip(req.board.trim(), 6000)) };
    format!(
        r#"You are "Dieter", the voice assistant of nrzCode – a desktop app in which coding agents work in floating windows on an infinite canvas (one canvas per workspace = project).
The user talks to you. The text comes from automatic speech recognition and may contain recognition errors (misheard names or words, missing punctuation).

Workspaces (projects):
{spaces}
Windows on the open canvas (each has a first name; chat windows are coding-agent chats):
{panes}
Drawn on the canvas (sticky notes, text, boxes, arrows – the user's sketch/plan):
{board}
Actions:
- {{"type":"send","to":"<chat window>","prompt":"<text>"}} – give a chat window a task or a message. In an empty chat window this starts a new chat. If the agent there is working, the message waits until it is done.
- {{"type":"new","prompt":"<text>"}} – open one more chat window and start a chat there.
- {{"type":"stop","to":"<chat window>"}} – stop the agent in that window.
- {{"type":"focus","to":"<window>"}} – zoom to that window; {{"type":"unfocus"}} – show all windows again.
- {{"type":"move","to":"<window>","target":"left|right|up|down|center|<other window>"}} – move a window (in a direction, to the center of the view, or next to another window).
- {{"type":"arrange"}} – arrange all windows neatly side by side and center the view.
- {{"type":"workspace","target":"<workspace>"}} – switch to another workspace (project).
- {{"type":"open","target":"terminal"}} – open a terminal window; {{"type":"open","target":"browser","url":"<url>"}} – open a browser window (for local servers use e.g. http://localhost:3000).
- {{"type":"navigate","to":"<browser window>","url":"<url>"}} – show another address in a browser window.
- {{"type":"close","to":"<window>"}} – close a window (a chat stays available in the chat list).
- {{"type":"note","prompt":"<text>"}} – pin a sticky note with that text on the canvas.

How to write the prompt for a chat window:
- Write it in the user's language, directly to the coding agent (imperative), as a clear and complete task: the goal, every requirement the user mentioned, sensible defaults for missing details (say they are assumptions the agent may adapt) and what counts as done.
- If the user refers to the drawing/sketch/notes on the canvas, put the relevant content of the drawing into the prompt (the agent can also read the canvas itself with its canvas_read tool).
- Keep the user's intent; do not add requirements that change it. Never mention Dieter, voice or speech recognition.
- Usually 3–12 lines of plain text; short lists are fine.

Rules:
- Map misheard names to the most similar window or workspace name. "sag Lena, …", "Lena soll …", "an Lena: …" are tasks for Lena. One sentence can contain several actions.
- If it is unclear which window is meant or what should be done, ask one short question in "reply" and take no action.
- Questions about the state ("Status?", "Was macht Benni?"): answer in "reply" from the window information above – spoken style, short, no lists, no markdown.
- "reply" is read aloud: short and natural (usually one sentence confirming what you did), in the user's language, no markdown, no code, no emojis.
- If the words are not meant for you (noise, a fragment, the user talking to someone else), answer {{"reply":"","actions":[]}}.

Conversation so far:
{history}
The user just said: «{said}»

Answer with only the JSON object: {{"reply":"…","actions":[…]}}"#
    )
}

/// Eine Zeile (oder wenige) über den Thread in einem Fenster.
async fn describe(state: &AppState, t: &ThreadRow, name: &str, big: &str) -> AppResult<String> {
    let status = match t.status() {
        ThreadStatus::Working => "working",
        ThreadStatus::Waiting => "waiting (for an answer from the user or for subagents)",
        ThreadStatus::Done => "finished, not looked at yet",
        ThreadStatus::Error => "stopped with an error",
        ThreadStatus::Idle | ThreadStatus::Settled => "idle",
    };
    let project: Option<String> = match t.project_id {
        Some(pid) => sqlx::query_scalar("SELECT name FROM projects WHERE id = ?").bind(pid).fetch_optional(&state.db).await?,
        None => None,
    };
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT role, content, data FROM messages WHERE thread_id = ? ORDER BY id DESC LIMIT 60").bind(t.id).fetch_all(&state.db).await?;
    let (mut last_user, mut last_agent, mut question, mut error) = (None, None, None, None);
    let mut steps: Vec<String> = Vec::new();
    for (role, content, data) in &rows {
        let c = content.trim();
        match role.as_str() {
            "user" if last_user.is_none() => last_user = Some(clip(c, 400)),
            "assistant" if last_agent.is_none() && !c.is_empty() => last_agent = Some(clip(c, 600)),
            "question" if question.is_none() && last_agent.is_none() => question = Some(clip(c, 300)),
            "error" if error.is_none() && last_agent.is_none() => error = Some(clip(c, 300)),
            "tool" if steps.len() < 3 && last_agent.is_none() => {
                let call: ToolCall = serde_json::from_str(data).unwrap_or_default();
                if !matches!(call.kind.as_str(), "think" | "todo") {
                    let s = if call.summary.is_empty() { call.name.clone() } else { call.summary.clone() };
                    steps.push(clip(&s, 120));
                }
            }
            _ => {}
        }
    }
    let mut out = format!("- {name}{big}: {} ({}), state: {status}", t.kind().name(), if t.model.is_empty() { "default model" } else { t.model.as_str() });
    if let Some(p) = project {
        out.push_str(&format!(", project \"{p}\""));
    }
    out.push_str(&format!(", chat title \"{}\"\n", clip(&t.title, 120)));
    if let Some(u) = last_user {
        out.push_str(&format!("  last task from the user: {}\n", one_line(&u)));
    }
    if !steps.is_empty() {
        steps.reverse();
        out.push_str(&format!("  latest steps: {}\n", steps.join(" · ")));
    }
    if let Some(a) = last_agent {
        out.push_str(&format!("  last answer of the agent: {}\n", one_line(&a)));
    }
    if let (Some(q), ThreadStatus::Waiting) = (question, t.status()) {
        out.push_str(&format!("  open question to the user: {}\n", one_line(&q)));
    }
    if let (Some(e), ThreadStatus::Error) = (error, t.status()) {
        out.push_str(&format!("  error: {}\n", one_line(&e)));
    }
    Ok(out)
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// JSON aus der Antwort holen (auch mit ```json-Zaun oder Text drumherum); passt Namen an die
/// Fenster an und verwirft Unbekanntes.
pub fn parse(raw: &str, req: &DieterRequest) -> DieterReply {
    let json = match (raw.find('{'), raw.rfind('}')) {
        (Some(a), Some(b)) if b > a => &raw[a..=b],
        _ => "",
    };
    let Ok(mut reply) = serde_json::from_str::<DieterReply>(json) else {
        // Kein JSON: als gesprochene Antwort nehmen (ohne Markdown-Reste)
        let text: String = raw.replace(['*', '#', '`'], "").split_whitespace().collect::<Vec<_>>().join(" ");
        return DieterReply { reply: clip(&text, 400), actions: Vec::new() };
    };
    let names: Vec<&str> = req.panes.iter().map(|p| p.name.as_str()).collect();
    let canvas = req.mode == "canvas";
    let kind_of = |name: &str| req.panes.iter().find(|p| p.name == name).map(|p| p.kind.as_str()).unwrap_or("chat");
    let spaces: Vec<&str> = req.workspaces.iter().map(String::as_str).collect();
    reply.reply = clip(reply.reply.trim(), 1200);
    reply.actions = reply
        .actions
        .into_iter()
        .filter_map(|mut a| {
            a.kind = a.kind.trim().to_lowercase();
            a.prompt = a.prompt.trim().chars().take(12_000).collect();
            a.target = a.target.trim().chars().take(200).collect();
            a.url = a.url.trim().chars().take(2000).collect();
            match a.kind.as_str() {
                "send" | "stop" => {
                    a.to = closest(&a.to, &names)?;
                    // Aufträge nur an Chat-Fenster
                    if canvas && matches!(kind_of(&a.to), "terminal" | "browser") {
                        return None;
                    }
                    (a.kind != "send" || !a.prompt.is_empty()).then_some(DieterAction { target: String::new(), url: String::new(), ..a })
                }
                "focus" => {
                    a.to = closest(&a.to, &names)?;
                    Some(DieterAction { kind: a.kind, to: a.to, ..Default::default() })
                }
                "unfocus" => Some(DieterAction { kind: a.kind, ..Default::default() }),
                "new" => (req.can_add && !a.prompt.is_empty()).then_some(DieterAction { kind: a.kind, prompt: a.prompt, ..Default::default() }),
                "arrange" if canvas => Some(DieterAction { kind: a.kind, ..Default::default() }),
                "close" if canvas => {
                    a.to = closest(&a.to, &names)?;
                    Some(DieterAction { kind: a.kind, to: a.to, ..Default::default() })
                }
                "move" if canvas => {
                    a.to = closest(&a.to, &names)?;
                    let t = a.target.to_lowercase();
                    let target = match t.as_str() {
                        "left" | "right" | "up" | "down" | "center" => t,
                        _ => closest(&a.target, &names).filter(|n| *n != a.to)?,
                    };
                    Some(DieterAction { kind: a.kind, to: a.to, target, ..Default::default() })
                }
                "workspace" if canvas => {
                    let target = closest_space(&a.target, &spaces)?;
                    Some(DieterAction { kind: a.kind, target, ..Default::default() })
                }
                "open" if canvas => {
                    let target = a.target.to_lowercase();
                    match target.as_str() {
                        "terminal" => Some(DieterAction { kind: a.kind, target, ..Default::default() }),
                        "browser" => Some(DieterAction { kind: a.kind, target, url: a.url, ..Default::default() }),
                        "chat" if !a.prompt.is_empty() => Some(DieterAction { kind: "new".into(), prompt: a.prompt, ..Default::default() }),
                        _ => None,
                    }
                }
                "navigate" if canvas && !a.url.is_empty() => {
                    let to = closest(&a.to, &names).filter(|n| kind_of(n) == "browser")?;
                    Some(DieterAction { kind: a.kind, to, url: a.url, ..Default::default() })
                }
                "note" if canvas && !a.prompt.is_empty() => Some(DieterAction { kind: a.kind, prompt: a.prompt.chars().take(2000).collect(), ..Default::default() }),
                _ => None,
            }
        })
        .take(12)
        .collect();
    reply
}

/// Passender Fenstername (gleich, ohne Gross/Klein, oder höchstens zwei Buchstaben anders).
fn closest(name: &str, names: &[&str]) -> Option<String> {
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    if let Some(x) = names.iter().find(|x| x.to_lowercase() == n) {
        return Some(x.to_string());
    }
    names
        .iter()
        .map(|x| (distance(&x.to_lowercase(), &n), *x))
        .filter(|(d, _)| *d <= 2)
        .min_by_key(|(d, _)| *d)
        .map(|(_, x)| x.to_string())
}

/// Passender Arbeitsbereich: gleich, enthalten oder höchstens drei Buchstaben anders.
fn closest_space(name: &str, spaces: &[&str]) -> Option<String> {
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    if let Some(x) = spaces.iter().find(|x| x.to_lowercase() == n) {
        return Some(x.to_string());
    }
    if let Some(x) = spaces.iter().find(|x| x.to_lowercase().contains(&n) || n.contains(&x.to_lowercase())) {
        return Some(x.to_string());
    }
    spaces
        .iter()
        .map(|x| (distance(&x.to_lowercase(), &n), *x))
        .filter(|(d, _)| *d <= 3)
        .min_by_key(|(d, _)| *d)
        .map(|(_, x)| x.to_string())
}

/// Levenshtein-Abstand.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use nrzcode_shared::voice::DieterPane;

    fn req() -> DieterRequest {
        DieterRequest {
            text: "sag Lena sie soll eine Webseite bauen".into(),
            panes: vec![DieterPane { name: "Lena".into(), thread_id: None, ..Default::default() }, DieterPane { name: "Bernd".into(), thread_id: Some(4), ..Default::default() }],
            can_add: false,
            ..Default::default()
        }
    }

    #[test]
    fn parses_fenced_json_and_fixes_names() {
        let raw = "Klar!\n```json\n{\"reply\":\"Mach ich.\",\"actions\":[{\"type\":\"send\",\"to\":\"lehna\",\"prompt\":\"Baue eine Webseite.\"},{\"type\":\"stop\",\"to\":\"Berndt\"},{\"type\":\"new\",\"prompt\":\"x\"},{\"type\":\"send\",\"to\":\"Otto\",\"prompt\":\"y\"}]}\n```";
        let r = parse(raw, &req());
        assert_eq!(r.reply, "Mach ich.");
        assert_eq!(r.actions.len(), 2);
        assert_eq!(r.actions[0].to, "Lena");
        assert_eq!(r.actions[1].kind, "stop");
        assert_eq!(r.actions[1].to, "Bernd");
    }

    #[test]
    fn plain_text_becomes_reply() {
        let r = parse("**Lena** ist fertig.", &req());
        assert_eq!(r.reply, "Lena ist fertig.");
        assert!(r.actions.is_empty());
    }

    #[test]
    fn canvas_actions() {
        let req = DieterRequest {
            text: "x".into(),
            mode: "canvas".into(),
            panes: vec![
                DieterPane { name: "Lena".into(), thread_id: Some(3), kind: "chat".into(), ..Default::default() },
                DieterPane { name: "Max".into(), kind: "terminal".into(), ..Default::default() },
                DieterPane { name: "Web".into(), kind: "browser".into(), ..Default::default() },
            ],
            workspaces: vec!["nrzCode".into(), "Webshop".into()],
            can_add: true,
            ..Default::default()
        };
        let raw = r#"{"reply":"ok","actions":[
            {"type":"move","to":"lena","target":"Left"},
            {"type":"move","to":"Lena","target":"Maks"},
            {"type":"workspace","target":"web shop"},
            {"type":"open","target":"browser","url":"http://localhost:3000"},
            {"type":"navigate","to":"Lena","url":"http://x"},
            {"type":"navigate","to":"Web","url":"http://y"},
            {"type":"send","to":"Max","prompt":"ls"},
            {"type":"note","prompt":"Login bauen"},
            {"type":"arrange"}
        ]}"#;
        let r = parse(raw, &req);
        let kinds: Vec<&str> = r.actions.iter().map(|a| a.kind.as_str()).collect();
        assert_eq!(kinds, vec!["move", "move", "workspace", "open", "navigate", "note", "arrange"]);
        assert_eq!(r.actions[0].target, "left");
        assert_eq!(r.actions[1].target, "Max");
        assert_eq!(r.actions[2].target, "Webshop");
        assert_eq!(r.actions[4].to, "Web");
        // Im Raster gibt es die Canvas-Aktionen nicht
        let grid = DieterRequest { mode: String::new(), ..req };
        let kinds: Vec<String> = parse(raw, &grid).actions.into_iter().map(|a| a.kind).collect();
        assert_eq!(kinds, vec!["send"]);
    }

    #[test]
    fn levenshtein() {
        assert_eq!(distance("lena", "lehna"), 1);
        assert_eq!(distance("bernd", "bernd"), 0);
        assert_eq!(closest("Benni", &["Lena"]), None);
    }
}
