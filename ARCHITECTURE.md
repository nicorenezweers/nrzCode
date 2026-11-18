# Architecture

nrzCode is one Rust program: a web client for coding agents. It starts the agents' command line
programs – Claude Code, Codex, Gemini CLI, OpenCode, Kilo Code and Grok CLI – in workspaces that
are checked out from nrzGit, streams their output into a chat, and lets several agents (and several
subscriptions of the same agent) work side by side. Agents get nrzGit as an MCP server, so they can
create repositories, push, open pull requests and releases, build and push images, and check which
ports are taken.

```
┌──────────────────────────── Browser ─────────────────────────────┐
│ Dioxus app (WASM, ui/) – sidebar · chat · composer · right panel │
└──────────────┬──────────────────────────────┬────────────────────┘
               │ /api/v1 + /api/v1/events (SSE)│ :8411 preview proxy
┌──────────────▼──────────────────────────────▼────────────────────┐
│ Rocket 0.5 (src/)                                                 │
│  core: config · db · auth · hub (live events) · git · markdown    │
│  agents/     accounts · tools · threads · runs · subagents ·      │
│              MCP server for subagents · terminals (PTY)           │
│  workspace/  nrzGit connection · projects · worktrees · git ·     │
│              files · preview proxy · users · settings             │
│  voice/      Whisper speech recognition (candle) · Dieter         │
└───────┬───────────────────────┬───────────────────────┬──────────┘
        │ spawn (own HOME each) │ git / HTTP + token     │ MCP (Bearer token)
┌───────▼─────────┐     ┌───────▼──────────────────────────────────▼───┐
│ claude · codex  │     │ nrzGit: repos, issues, PRs, releases, ports,  │
│ gemini · opencode│    │ packages (container registry), MCP at /mcp    │
│ kilocode · grok │     └───────────────────────────────────────────────┘
└─────────────────┘
```

## Layout

| Path | What |
|---|---|
| `shared/` | Crate `nrzcode-shared`: `common.rs` (agent kinds, thread/project summaries, messages, live events, diffs) plus `agents.rs`, `workspace.rs`, `voice.rs` and `canvas.rs` |
| `src/` | Server. Core modules at the top, one folder per area (`agents`, `workspace`, `voice`, `canvas`) |
| `migrations/` | `0001_init.sql` is the whole schema; areas may add `0002_agents.sql`, `0003_workspace.sql` |
| `ui/` | Dioxus 0.7 web app with rust-ui components; pages per area in `ui/src/pages/<area>/` |
| `locales/<lang>/<area>.json` | UI translations (German source text → translation), merged by `build.rs` |
| `tests/` | Integration tests; `tests/common/mod.rs` has helpers (users, projects with a git repo, threads) |

## Data folder

| Path | What |
|---|---|
| `nrzcode.db` | SQLite: users, sessions, nrzGit connections, agent accounts, projects, threads, runs, messages, usage log (`usage_log`, kept when threads are deleted), canvas boards (`canvas_boards`) |
| `accounts/<id>/home` | HOME of one agent account – `HOME`, `XDG_*`, `CLAUDE_CONFIG_DIR`, `CODEX_HOME` … point here, so two Claude subscriptions never share a login |
| `shims/security` | macOS only: first in the `PATH` of account processes. An account HOME has no keychain, so writes to the default keychain (Claude Code saving its login) would make macOS show “Keychain Not Found”; the shim fails them quietly (exit 37, as without a keychain) and the CLIs keep using their files in the account HOME. Everything else goes to `/usr/bin/security` |
| `workspaces/<project>/repo` | main checkout of a project |
| `workspaces/<project>/worktrees/<thread>` | git worktree of a thread (branch `nrz/<slug>`) |
| `scratch/<thread>` | working folder of threads without a project |
| `bots/<bot>` | working folder (“computer”) of a bot, shared by all its conversations |
| `tools/` | agent CLIs installed by nrzCode (`tools/npm`, npm prefix) |
| `attachments/` | files attached in the composer |
| `models/whisper-<size>/` | Whisper models for voice control (`config.json`, `tokenizer.json`, `model.safetensors` from `openai/whisper-<size>`), downloaded on demand |

## Conventions

Same as nrzGit: Rust only (no JavaScript/TypeScript/Python files; browser APIs through `web-sys`),
German source strings with English catalogs, camelCase JSON, Unix seconds, `AppResult`/`AppError`,
`sqlx::AssertSqlSafe` for dynamic SQL, Markdown rendered on the server (`markdown::render_plain`).
Everything the UI shows live goes through `hub::publish(state, user_id, HubEvent::…)`; the UI listens
on `GET /api/v1/events` (`ui/src/events.rs` keeps `state::THREADS`, `PROJECTS`, `MESSAGES`,
`CHANGES`, `PREVIEW` current). Spinners: `util::install_spin_sync` gives every spinner that enters the DOM a negative `animation-delay` matching the page clock (re-inserted ones – e.g. after the sidebar re-sorts – keep spinning instead of restarting), and they get `will-change: transform`. Do not tick a whole view every second: only leaf components read `parts::NOW` (e.g. the chat's status line), never `ChatBody`. These handlers run outside any component: use `spawn_forever`,
never `spawn` (no current scope – it panics and the UI reloads).

New thread from the home page: the composer sets `state::PENDING_THREAD` before `POST /threads`;
home shows `agents::PendingChat` and the sidebar a placeholder row until the response, then the
history is loaded and the route switches to the thread. File drops: `util::install_file_drop`
accepts files anywhere in the window (the WebView never opens them itself) and hands drops outside
a drop zone to the nearest visible composer (`data-composer`, `util::FILE_DROP`).

## Contracts between the areas

Server functions (signatures fixed):

| Function | Owner | Used by |
|---|---|---|
| `workspace::nrzgit_access(state, user) -> Option<NrzGitAccess{url, username, token}>` | workspace | agents (MCP config of runs) |
| `workspace::thread_workdir(state, thread) -> PathBuf` | workspace | agents (cwd of runs and terminals) |
| `workspace::agent_env(state, user) -> Vec<(String, String)>` | workspace | agents (git auth, docker registry) |
| `workspace::after_run(state, thread)` | workspace | agents (after each run) |
| `workspace::forget_thread(state, thread)` | workspace | agents (before deleting a thread: remove its worktree, keep the branch) |
| `agents::stop_thread(state, thread)` | agents | workspace (delete thread/project) |
| `agents::publish_thread(state, thread)` | agents | workspace (branch/worktree changed) |

UI components (names and props fixed): `agents::{ChatView{thread_id}, Composer{project_id, thread_id},
Lineage{thread_id}, AccountsSettings, ToolsSettings, TerminalView{id}, AgentIcon{kind, size}}` are
embedded by `workspace` (`AppLayout`, `Home`, `ThreadPage`, `Settings`).

API paths (under `/api/v1`):

| Area | Paths |
|---|---|
| core | `GET /health`, `POST /markdown`, `GET /events` (SSE) |
| agents | `/agents…` (kinds, models), `/accounts…`, `/tools…`, `/threads…` (incl. messages, runs, stop, subagents), `/attachments…`, `/terminals…`, `/bots…` (conversation, routines) |
| team | `/projects/<id>/team` (view, switch on/off), `/projects/<id>/team/messages`, `/projects/<id>/team/sync` |
| workspace | `/auth/…`, `/me…`, `/users/…`, `/settings/…`, `/nrzgit…`, `/projects…`, `/workspace/<thread>/…` (worktree, branch, changes, diff, commit, push, pull request, files), `/previews…` |
| voice | `/voice/settings`, `/voice/models…` (list, download, delete), `/voice/transcribe` (WAV → text), `/voice/command` (sentence + grid windows → reply and actions) |

Outside the API: `/mcp` (MCP server for subagents, agents), `/attachments/<uuid>` (agents), the
preview proxy on `preview_port` (workspace).

## nrzGit

The user creates an access token in nrzGit (Settings → Applications, scope `mcp` or `all`) and pastes
URL and token into nrzCode (Settings → nrzGit). nrzCode then

- lists and creates repositories through the nrzGit REST API (`/api/v1/me/repos`, `POST /api/v1/repos`),
- clones and pushes with the token (HTTP Basic, user name + token),
- gives every agent run the MCP server `<nrzgit>/mcp` (Streamable HTTP, `Authorization: Bearer <token>`)
  next to its own MCP server (`/mcp` of nrzCode, a token per run) for subagents and previews.

A luGit can be connected next to (or instead of) nrzGit: `nrzgit_connections` holds one row per user
and platform (`kind` = `nrzgit` | `lugit`), projects remember their platform (`projects.forge`), and
`workspace::forge_accesses` / `forge_access` / `project_access` pick the right one. From luGit 1.1.0
on (`has_mcp`, detected via `POST <lugit>/mcp` → 401) agents get it as the MCP server `lugit`; the
environment carries `NRZGIT_*` and `LUGIT_*`, git credentials and the Docker config cover both.

GitHub is the third platform (`kind` = `github`): github.com (REST API at `https://api.github.com`)
or a GitHub Enterprise Server (`<url>/api/v3`, detected by that). `nrzgit::Client` translates GitHub's
shapes into the same `Remote…` types, so import, create, clone, push and pull requests share one
path. For github.com agents get GitHub's own MCP server (`https://api.githubcopilot.com/mcp/`) as
`github`; the environment carries `GITHUB_*` and `GH_TOKEN` (for the `gh` CLI), the registry is
`ghcr.io`.

## Building

```bash
scripts/ui.sh            # dx bundle → ui/dist/public
cargo build --release    # embeds UI, static files, locales
```

## Voice control (Dieter)

The grid (`/grid/…`) names every window (Lena, Benni, …) and has Dieter at the bottom center. The UI
records with `getUserMedia` + Web Audio (`ui/src/mic.rs`; microphone chosen per device in Settings → Dieter,
kept in `localStorage`, falling back to the system default when it is unplugged), cuts sentences at speech pauses, resamples
to 16 kHz and posts each sentence as WAV to `/voice/transcribe`. The server recognizes it with Whisper
in pure Rust (`src/voice/whisper.rs`, candle; on macOS through Accelerate), with the window names as
prompt so they are spelled right. "Dieter, schalte dich ab/an" is handled in the UI (asleep he only
reacts to waking up); everything else goes to `/voice/command`: a fast agent (`runs::oneshot_user`,
read-only, empty folder; Claude: Haiku unless chosen in Settings → Dieter) gets the state of every
window and answers `{reply, actions}` – `send` (with a complete prompt it writes), `stop`, `focus`,
`unfocus`, `new`. The UI executes the actions and reads the reply aloud (`speechSynthesis`).

Dictation in the composer (microphone next to Send) uses the same recording and sentence cutting
(`mic::dictate_start/stop`, independent of Dieter) and appends every recognized sentence to the
text field; a missing Whisper model is downloaded on the first try.

## Bot mode

The switch “Code | Bot” above the sidebar search (`state::WORK`, kept in `localStorage`) changes
the sidebar between threads in projects and **bots** – persistent AI teammates in the spirit of
Grok Bot (`src/agents/bots.rs`, `ui/src/pages/workspace/bots.rs`, routes `/bots` and `/b/<id>`).
A bot has a name, emoji, role, instructions, an agent/account/model and a **memory** (Markdown in
`bots.memory`). A conversation is an ordinary thread without project with `threads.bot_id`;
`bots.thread_id` is the current one (“New conversation” starts another, the memory stays).
Runs of bot threads work in `bots/<bot>` and get a different system hint (`bots::hint`:
personality, instructions, memory) instead of the coding hint; the nrzCode MCP server adds
`remember` (append one fact) and `rewrite_memory` for them. **Routines** (`bot_routines`) send a
task into the current conversation at a local time on chosen weekdays (`time_min`, `days` bit mask
Mo=1 … Su=64, `tz_offset` minutes east of UTC from the browser); `bots::spawn_loop` checks every
30 s and catches up at most an hour late. Bot threads stay out of the Code sidebar, home and canvas.

## Referencing other repositories (`@@`)

Typing `@@` in the composer lists the user's other projects (and, below them, repositories of
the connected platforms that are not projects yet – choosing one imports it, cloning in the
background). The message keeps the plain token, e.g. `@@payment-api`. When a run starts,
`workspace::referenced_repos` resolves every `@@name` (project name with spaces as `-`, `owner/name`
or just the repository name; only the user's own projects, not the thread's own one, at most 8,
waiting briefly for a clone) to the project's main checkout. The CLI gets a short "Referenced
repositories" section with path and branch after the prompt – read for reference, e.g. to integrate
two projects, not to modify – and access to the folders (`--add-dir` for Claude Code,
`--include-directories` for Gemini; the other CLIs read absolute paths anyway).

## Interjections

"Interject" (⌘/Ctrl+Enter, or "send now" on a queued message) puts the message into the queue with
`interrupt: true` and signals a soft stop (`runs::signal_soft_stop`). The run ends at the next
suitable moment – a tool or an answer just finished and no tool is open – or after
`runs::SOFT_STOP_GRACE` (20 s), as `Outcome::Interrupted`: no "Stopped" line, subagents keep
working, and the queue goes out right away. The CLI gets `messages::INTERJECTION_NOTE` in front of
the text; the chat shows only the text (`UserMessageData.interjection`, older messages are stripped
when loaded). "MCP servers not connected" is reported only when the list changes in a thread.

## Agent steps in the chat

Tool steps of a run are shown as they happen, as quiet lines between the answers (no "worked for"
header): reads, searches and thoughts collapse into one line ("2 files read · 1 search",
"Thought"), consecutive commands into "Ran 3 commands" – a click lists every `$ cmd` with its
output –, edits become code blocks with line numbers, red/green lines and syntax colours
(`ui/src/components/highlight.rs`). For edits the run
writer remembers the file when the tool is announced and cuts the changed sections with three lines of
context once it is done (`src/agents/edits.rs`, Myers diff in `shared/src/diff.rs`); agents that only
report afterwards (Codex) fall back to the old/new text, the last content seen in the run or `HEAD`.

## Canvas mode

Besides the chat mode (sidebar, grid) the UI has a canvas mode (`/canvas`, switch at the top): one
infinite board per user and workspace (project, `0` = none) with every window of that workspace –
chats, empty chats, terminals, browsers – free to move, resize and zoom, plus a drawing layer (pen,
boxes, ellipses, diamonds, arrows, lines, text, sticky notes). The chats are the same threads as in
chat mode; `canvas_boards` only stores where each window is (`layout`, owned by the UI) and what is
drawn (`shapes`, shared with the agents). Agents read and draw through the MCP tools `canvas_read` /
`canvas_draw` (`src/canvas/mod.rs`, author = window name); every change bumps `rev` and sends
`CanvasUpdated`, so other tabs and agent drawings appear at once. Browser windows use the preview
proxy without a thread (`/previews?projectId=`, token with `threadId: 0`). On the canvas Dieter knows
more actions (`move`, `arrange`, `workspace`, `open`, `navigate`, `close`, `note`) and the position
of every window. API: [docs/api-canvas.md](docs/api-canvas.md).

## Team

Several people – each with their own nrzCode on any platform – work on the same repository and
their agents talk to each other (`src/team/mod.rs`, `shared/src/team.rs`, `ui/src/pages/workspace/team.rs`,
tab "Team" in the right column). There is no server for it: the shared state (`TeamDoc`: members with
their agents – thread number, title, status, branch – and the messages) is `team.json` in the ref
`refs/nrzcode/team` of the shared repository. Every instance polls it with `git ls-remote` (5 s while
agents work or the tab is open, else 40 s), fetches, merges and pushes a new commit on top; a
rejected push (somebody else wrote) is retried with the new state. Access = access to the repository;
the member key is the user name on the git platform. `team_projects` keeps the last known state per
project, `team_outbox` unsent messages, `team_seen` what each thread already got. Agents of main
threads get `team_status` / `team_send` / `team_read` and a `TEAM` section in the system hint (their
address `name#thread`, teammates, new messages). With "agents answer on their own" (`auto_reply`, off
by default) a message to `name#thread` starts that thread (at most 8 times per hour); team messages
are always framed as coming from collaborators, not from the user. Works for local-only users of one
server too (two projects on the same remote). On the phone the team API passes the nrzCodeMobile
proxy (`/projects/…`). API: [docs/api-team.md](docs/api-team.md).

## AI chains

Settings → AI chains: an order of accounts, e.g. Claude subscription 1 → Claude subscription 2 →
Codex (`src/agents/chains.rs`, stored per user in `settings`). When a run stops at a rate limit the
thread switches right away to the next step that is not at its limit itself and continues with the
history (a new session, since sessions belong to CLI and account); `preflight` switches before a run
when the account is known to be at its limit. Without a matching chain the thread waits for the reset
as before.

## Native CLI

`</>` in the top bar (`ui/src/pages/workspace/cli.rs`) swaps a thread's chat for the agent's own
interactive CLI: `POST /threads/<id>/cli` (`src/agents/terminals.rs`) starts the program directly in a
PTY (no shell; Windows npm shims resolved by `platform::resolve_program`) in the thread's worktree, with
the account's environment, the thread's model and `--resume <session>` (`codex resume`, `-s` …). The
view is a `TerminalPane` with `themed: true` – app colours, `--term-*` palette per theme, translucent
over the glass. Switching is instant: chat and CLI are stacked layers of the thread page (the hidden
one `invisible`, still laid out, so the terminal keeps its size) and the CLI keeps running while the
chat shows. It is started ahead in the background – on hovering `</>`, when opening a thread whose CLI
was used (`localStorage` `nrz-cli-threads`) and after every run. Only one program writes a session at
a time: a run starting closes the thread's CLI (`terminals::close_cli` in `runs.rs`, exit code `-2`, the
UI restarts it after the run with the new session state), opening it while a run works returns `409`;
at most four CLIs run per user (the oldest is closed). The next run resumes the same session and so
sees what happened in the CLI.

## App window

The native app (`macos/`, also built for Windows) shows the interface in a transparent window with the
system's frosted glass behind it – blurred, not just see-through (`window-vibrancy`, `apply_glass`):
macOS vibrancy `UnderWindowBackground`, applied after the webview because wry replaces tao's content
view with its own (the view `Window` still reports is detached); Windows 11 22H2+ its own acrylic
(`macos/src/glass.rs`: `SetWindowCompositionAttribute`, tinted like the theme – the system acrylic
`DWMSBT_TRANSIENTWINDOW` follows the system's light/dark and turns into a flat colour whenever the
window is not in front), older Windows 10/11 blur behind the window (their acrylic lags when
dragging). Without it the window stays opaque. `overlay::GLASS_SCRIPT` sets
`window.__nrzGlass` and `html.nrz-glass` before the first paint, so the interface only tints its
surfaces (`ui/tailwind.css`; off under Settings → Appearance, `localStorage` `nrz-glass`). The
interface talks to the app through `window.ipc` (`util::native`): `theme:<dark|light|system>` sets the
window appearance and with it the glass, `update:check:quiet` and `update:state` drive "Check for
updates" in the sidebar (answer: `window.__nrzUpdateState(status, text)`), and ⌘, (app menu) or
Ctrl+, call `window.__nrzOpenSettings()`.

Links never open a foreign page inside the app: `overlay::NATIVE_SCRIPT` catches clicks on links to
another origin (also `localhost` of agent servers, `mailto:`) and sends them through `window.open` to
the new-window handler, which opens everything but the built-in server (`is_own`: attachments → app
window) in the system browser. Should the main window still load a foreign page (link without
handler, redirect), the page-load handler – main frame only, so the preview and the canvas browser
(iframes) stay – opens it in the system browser and loads the interface again (`UserEvent::Escaped`).

Adding a repository that already is a project never fails: `POST /projects/import` and
`POST /projects/create` return the existing project (200) and the interface selects it for the new
thread; creating a name that already exists on the platform imports that repository instead. Project
pickers (home, sidebar filter) have a search field (`overlay::MenuSearch`, Enter takes the first hit).

Frame of the interface (`ui/src/pages/workspace/layout.rs`): a narrow rail on the far left
(`Rail`: Code, Bots, projects, canvas, sidebar; at the bottom "check for updates" in the app
(`update::UpdateRailButton`), the usage ring, settings and the profile), the sidebar with search, "New thread" and the threads grouped by day (`day_groups`:
pinned, today, yesterday, this week, older – by their place in the list, so drag and drop matches),
each as two lines (agent, title, state; project · branch, changes). The right column of a thread
is one panel with quiet section labels (`SectionLabel`): environment (changes, worktree, branch,
commit & push), repository, pull request, editor, actions (local servers) and usage
(`agents::UsageSection`: per account the remaining percent of the 5 h, weekly and special limits).
It is on by default (`nrz-feature-right`) and always starts collapsed. Running agents float as a
sticky card at the bottom of the thread list. The chat and the terminal follow new output only
while at the very bottom; scrolling up (wheel, touch) stops following at once. `overscroll-behavior:
none` sits only on the page and the large scroll areas – on every element it would stop the wheel
at each clipped (`overflow: hidden`) child, e.g. truncated thread titles.

## macOS permissions

The app is signed with a stable local certificate (`scripts/macos.sh`, "nrzCode Local Signing" in its
own keychain) so macOS keeps granted permissions across updates (ad-hoc signatures change with every
build). At start the app asks for missing folder permissions (Desktop, Documents, Downloads); the
microphone is asked for on first use of Dieter. A denied file access returns `403` with "Kein Zugriff:
macOS …" – the app then opens its permission list (`perm:hint`).


## nrzCodeMobile

`mobile/` is a separate small program for the Umbrel (app `nrz-code-mobile` in the nrzStore): an
axum server with an embedded Dioxus UI for phones (`mobile/ui`). The PC never opens a port: after
pairing (Settings → Mobile: address and code from nrzCodeMobile, `POST <addr>/pair/claim` → token)
nrzCode keeps an outgoing WebSocket to `<addr>/connect` (`src/mobile.rs`, reconnects with back-off).
nrzCodeMobile sends `{"t":"req",…}` for `/api/pc/<id>/v1/…` from the phone; nrzCode runs it against
its own API on `127.0.0.1` with a session of the paired user (`Authorization: Bearer`, accepted only
from loopback; only threads, messages, usage, projects, agents and `me` are allowed) and answers
`{"t":"res",…}`. All hub events of the user go up as `{"t":"event",…}` and reach the phone as
Server-Sent Events. Accounts (`mobile/src/auth.rs`): username + password (Argon2id), the first
account is created on first visit and is admin (it can add more); sessions are random tokens in an
HttpOnly cookie `nrzm_session` (only the SHA-256 is stored in `store.json`, mode 0600, 30 days
sliding). Every PC has an `owner`; pairing codes belong to the account that created them, and
`/api/pcs`, `/events` and the proxy answer 404 for other accounts' PCs (admins included). Changing
requests need `X-Nrzm: 1` and a same-origin `Origin`; logins lock out after 5 failures per name
(20 per address); responses carry CSP/nosniff/DENY. In the UI, loads triggered from a view that
is about to unmount (opening a chat from the list) use `spawn_forever`, and the effect that opens the event stream and loads the chat list depends on both the sign-in state and the PC (reopening with a remembered PC and a valid session must load too). The phone UI is always dark. Updates from inside the app (`mobile/src/update.rs`, admins, Settings → App): latest tag `umbrel-X.Y.Z` of nrzCodeMobile on nrzGit (`NRZMOBILE_UPDATE_URL`, else the host the app is opened on, else the container gateway, port 8390), `SHA256SUMS` checked, the binary must run and report its version (`nrzcode-mobile version`), it goes to `<data>/update/` and the running process `exec`s into it (same PID, no container restart); on start the store binary `exec`s into `<data>/update/` while that one is newer. The UI polls `/api/info` and offers „Neu laden“ when the server runs a newer version than the page. Build: `scripts/mobile.sh` (local) or `scripts/mobile.sh linux` (static
amd64/arm64), publish for Umbrel with `scripts/mobile-umbrel.sh` (repo nrzCodeMobile, branch
`umbrel`, tag `umbrel-<version>`).
