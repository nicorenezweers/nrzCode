//! nrzCode – Oberfläche (Dioxus 0.7, WebAssembly) mit Komponenten von rust-ui.
//! Aufbau wie ein Coding-Agent-Client: links Projekte und Threads, in der Mitte die
//! Unterhaltung mit dem Agenten und der Composer, rechts Umgebung, Git, Vorschau und Subagenten.
//!
//! Seiten je Bereich: `pages::workspace` (Rahmen, Anmeldung, Projekte, Thread-Seite mit rechter
//! Spalte, Einstellungen), `pages::agents` (Unterhaltung, Composer, Subagenten, Agent-Konten,
//! Werkzeuge, Terminal).

mod api;
mod components;
mod events;
mod i18n;
mod mic;
mod notify;
mod popup;
mod pages;
mod state;
mod ui;
mod util;

use components::overlay::{ConfirmHost, ImageViewerHost, Toaster};
use dioxus::prelude::*;
use pages::canvas::Canvas;
use pages::workspace::*;

static CSS: Asset = asset!("/assets/tailwind.css");
static ICON: Asset = asset!("/assets/icon.png");
static FAVICON: Asset = asset!("/assets/favicon-32.png");

#[derive(Routable, Clone, PartialEq, Debug)]
#[rustfmt::skip]
pub enum Route {
    #[route("/user/login?:next")]
    Login { next: String },
    #[route("/install")]
    Setup {},
    // Fenster der Menüleiste (macOS-App), ohne Rahmen der App
    #[route("/tray")]
    Tray {},
    #[layout(AppLayout)]
        // Startseite: Projekt wählen und neuen Thread beginnen
        #[route("/")]
        Home {},
        // Thread: Unterhaltung (agents::ChatView) und rechte Spalte (Umgebung, Git, Vorschau,
        // Subagenten über agents::Lineage)
        #[route("/t/:id")]
        ThreadPage { id: i64 },
        // Raster: mehrere Threads nebeneinander (IDs mit Komma getrennt)
        #[route("/grid/:ids")]
        Grid { ids: String },
        // Canvas-Modus: alle Fenster eines Arbeitsbereichs auf einer Fläche
        #[route("/canvas")]
        Canvas {},
        // Bot-Modus: Übersicht der Bots und Gespräch mit einem Bot
        #[route("/bots")]
        Bots {},
        #[route("/b/:id")]
        BotPage { id: i64 },
        #[route("/projects")]
        Projects {},
        #[route("/projects/:id")]
        ProjectPage { id: i64 },
        // section: profile | usage | nrzgit | agents | chains | tools | dieter | appearance | app |
        // users | instance
        #[route("/settings/:section")]
        Settings { section: String },
    #[end_layout]
    #[route("/:..segments")]
    NotFound { segments: Vec<String> },
}

fn main() {
    install_panic_hook();
    dioxus::launch(App);
}

/// Hinweis mit „Neu laden“, falls die Oberfläche kurz nach einem Neuladen wieder abstürzt
/// (reines JavaScript – Dioxus reagiert nach einem Absturz nicht mehr).
const CRASH_BANNER: &str = "(function(){if(document.getElementById('nrz-crash'))return;var de=(document.documentElement.lang||navigator.language||'').indexOf('de')===0;\
var d=document.createElement('div');d.id='nrz-crash';d.style.cssText='position:fixed;left:50%;top:16px;transform:translateX(-50%);z-index:2147483647;display:flex;gap:12px;align-items:center;background:#18181b;color:#fafafa;border:1px solid #3f3f46;border-radius:10px;padding:10px 14px;font:13px system-ui,sans-serif;box-shadow:0 8px 30px rgba(0,0,0,.45)';\
d.textContent=de?'nrzCode ist auf einen Fehler gestossen.':'nrzCode ran into an error.';var b=document.createElement('button');b.textContent=de?'Neu laden':'Reload';\
b.style.cssText='font:inherit;cursor:pointer;border-radius:7px;padding:4px 10px;border:1px solid #52525b;background:#27272a;color:#fafafa';b.onclick=function(){location.reload()};d.appendChild(b);document.body.appendChild(d)})()";

/// Nach einem Absturz (Rust-Panic) reagiert die Oberfläche nicht mehr – sie wirkte „aufgehängt“.
/// Darum: Meldung in die Konsole und die Seite neu laden (der Stand liegt auf dem Server);
/// höchstens einmal je Minute, sonst ein Hinweis mit Knopf statt einer Schleife aus Neuladen.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        web_sys::console::error_1(&format!("nrzCode: {info}").into());
        let Some(w) = web_sys::window() else { return };
        let now = js_sys::Date::now();
        let store = w.session_storage().ok().flatten();
        let last = store.as_ref().and_then(|s| s.get_item("nrz-crash-reload").ok().flatten()).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
        let script = if now - last > 60_000.0 {
            if let Some(s) = &store {
                let _ = s.set_item("nrz-crash-reload", &now.to_string());
            }
            "location.reload()"
        } else {
            CRASH_BANNER
        };
        // Reines JavaScript und erst nach dem Absturz: nichts davon braucht den (kaputten) Zustand
        let f = js_sys::Function::new_no_args(script);
        let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(&f, 30);
    }));
}

#[component]
fn App() -> Element {
    let mut ready = use_signal(|| false);
    use_hook(notify::install);
    use_hook(util::install_file_drop);
    use_hook(util::install_spin_sync);
    use_hook(popup::install);
    use_hook(components::overlay::install_image_clicks);
    // Milchglas der App, ⌘, / Strg+, für die Einstellungen, Thema „wie das System“ live,
    // Update-Stand der App für „Nach Updates suchen“
    use_hook(state::apply_glass);
    use_hook(state::install_settings_shortcut);
    use_hook(state::watch_system_theme);
    use_hook(pages::workspace::update::install);
    use_future(move || async move {
        if let Some(t) = util::storage_get("nrz-theme") {
            state::apply_theme(&t);
        }
        i18n::init().await;
        ready.set(true);
    });
    rsx! {
        document::Stylesheet { href: CSS }
        document::Stylesheet { href: concat!("/static/highlight.css?v=", env!("CARGO_PKG_VERSION")) }
        document::Link { rel: "icon", r#type: "image/png", href: FAVICON }
        document::Link { rel: "apple-touch-icon", href: ICON }
        document::Meta { name: "theme-color", content: "#1f1f1f" }
        if ready() {
            Router::<Route> {}
        }
        ConfirmHost {}
        ImageViewerHost {}
        Toaster {}
    }
}
