#![cfg_attr(windows, windows_subsystem = "windows")]
//! nrzCode als native App für macOS und Windows – komplett in Rust.
//! (Windows: WebView2 statt WKWebView, Menü im Fenster, Rechte-Abschnitt und Ampel nur unter macOS.)
//!
//! * Der nrzCode-Server läuft im selben Prozess auf `127.0.0.1` (Daten in
//!   `~/Library/Application Support/nrzCode`, Protokoll in `~/Library/Logs/nrzCode`).
//! * Kein Login: Die App meldet sich mit einem Token an, das nur sie kennt (`/__desktop/login`).
//! * Optionale Code-Sperre (6 Ziffern) nach Inaktivität und beim Start (siehe `lock`),
//!   einstellbar unter nrzCode → Einstellungen … (⌘,).
//! * Dort lassen sich auch die Rechte von macOS (Ordner, Mikrofon, Automation, Bedienungshilfen …)
//!   vorab erteilen, damit kein Dialog mitten in einem Agenten-Lauf aufgeht (siehe `permissions`).
//!   Beim Start fragt die App fehlende Ordnerrechte selbst an; verweigert macOS später einen Zugriff
//!   (Server meldet „Kein Zugriff: macOS …“), öffnen sich die Rechte (`perm:hint`).
//! * Die Oberfläche läuft in einem nativen Fenster (WKWebView über `wry`, Fenster über `tao`),
//!   mit macOS-Menüleiste (`muda`): Bearbeiten (Kopieren/Einfügen), Ansicht, Fenster, Hilfe.
//! * Updates: Die App fragt die Releases in nrzGit ab (`macos-<version>`), zeigt einen Hinweis mit
//!   Knopf, lädt die DMG, ersetzt das Bundle und startet neu (siehe `updater`).

mod lock;
#[cfg(windows)]
mod glass;
mod overlay;
#[cfg(target_os = "macos")]
mod permissions;
mod server;
#[cfg(target_os = "macos")]
mod tray;
mod updater;
#[cfg(windows)]
mod webview2_loader;

use muda::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::HashMap;
use std::path::PathBuf;
#[cfg_attr(not(target_os = "macos"), allow(unused_imports))]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder, WindowId};
use updater::{Installed, Update};
use wry::{NewWindowResponse, PageLoadEvent, WebView, WebViewBuilder};

const REPO_PAGE: &str = "http://100.78.107.162:8390/nicorenezweers/nrzCode";

#[derive(Debug)]
enum UserEvent {
    Menu(MenuEvent),
    ServerReady(Result<u16, String>),
    /// Ergebnis einer Update-Prüfung; `manual` = über das Menü bzw. die Oberfläche angestossen,
    /// `quiet` = ohne Dialog (Knopf in der Seitenleiste – das Ergebnis zeigt die Oberfläche).
    UpdateChecked { result: Result<Option<Update>, String>, manual: bool, quiet: bool },
    Progress(f64),
    Downloaded(Result<PathBuf, String>),
    /// Nachricht aus einer Seite; `trusted` = von der eigenen Oberfläche im Hauptfenster.
    Ipc { msg: String, trusted: bool },
    PageLoaded(WindowId),
    OpenWindow(String),
    /// Das Hauptfenster hat eine fremde Seite geladen (Link ohne `target`, Weiterleitung …):
    /// im Browser des Systems öffnen und die Oberfläche zurückholen.
    Escaped(String),
    /// Regelmässiger Takt für die Sperre nach Inaktivität.
    Tick,
    /// Klick auf das Symbol in der Menüleiste.
    #[cfg(target_os = "macos")]
    Tray(tray_icon::TrayIconEvent),
    /// Nachricht aus dem Fenster der Menüleiste (`tray:open:<id>`, `tray:new`, `tray:main`).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    TrayIpc(String),
    /// Zustand der Rechte von macOS hat sich (vielleicht) geändert – an die Einstellungen schicken.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Permissions,
    /// Rechte fehlen (beim Start verweigerte Ordner oder ein verweigerter Zugriff): Einstellungen
    /// mit den Rechten zeigen.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    PermissionHint,
}

/// Zustand des Updaters.
#[derive(Default)]
struct UpdateState {
    available: Option<Update>,
    /// Hinweis vom Benutzer weggeklickt (bis zur nächsten neueren Version).
    dismissed: Option<semver::Version>,
    busy: bool,
}

struct MenuIds {
    settings: MenuItem,
    lock: MenuItem,
    check: MenuItem,
    install: MenuItem,
    quit: MenuItem,
    reload: MenuItem,
    data: MenuItem,
    logs: MenuItem,
    repo: MenuItem,
    close: MenuItem,
}

#[derive(serde::Deserialize)]
struct NotifyMsg {
    title: String,
    #[serde(default)]
    body: String,
}

#[derive(serde::Deserialize)]
struct SettingsSave {
    current: String,
    pin: Option<String>,
    minutes: u32,
}

/// Mitte der Ampel (Abstand vom oberen Fensterrand in Punkten), so wie macOS sie tatsächlich
/// zeichnet – hängt von der macOS-Version ab, darum gemessen statt geschätzt.
#[cfg(target_os = "macos")]
fn traffic_light_center(window: &Window) -> Option<f64> {
    use objc2_app_kit::{NSWindow, NSWindowButton};
    use tao::platform::macos::WindowExtMacOS;
    // SAFETY: Zeiger auf das NSWindow des lebenden tao-Fensters, Aufruf im Haupt-Thread.
    let ns = unsafe { (window.ns_window() as *const NSWindow).as_ref()? };
    let close = ns.standardWindowButton(NSWindowButton::CloseButton)?;
    if close.isHidden() {
        return None;
    }
    let r = close.convertRect_toView(close.bounds(), None);
    let y = ns.frame().size.height - (r.origin.y + r.size.height / 2.0);
    (y > 0.0 && y < 80.0).then_some(y)
}

/// Gemessene Ampel-Mitte als CSS-Variable `--nrz-tl-y` an die Oberfläche geben.
#[cfg(target_os = "macos")]
fn send_traffic_light(window: &Window, wv: &WebView) {
    if let Some(y) = traffic_light_center(window) {
        let _ = wv.evaluate_script(&format!("document.documentElement.style.setProperty('--nrz-tl-y','{y:.1}px')"));
    }
}

fn main() {
    let log_guard = init_logging();

    // Nur eine Instanz: zwei Server auf derselben Datenbank vertragen sich nicht.
    let _lock_file = match single_instance_lock() {
        Ok(l) => l,
        Err(e) => {
            message("nrzCode", &e, rfd::MessageLevel::Info);
            return;
        }
    };

    updater::cleanup();

    // WebView2Loader.dll steckt im Programm: vor dem ersten WebView auspacken und laden
    #[cfg(windows)]
    if let Err(e) = webview2_loader::load() {
        tracing::error!("WebView2-Lader: {e}");
        message("nrzCode", &format!("WebView2 kann nicht gestartet werden:\n{e}"), rfd::MessageLevel::Error);
        return;
    }

    // Umgebung (PATH, Ports, Datenordner, Desktop-Token) vor allen weiteren Threads setzen
    let ports = server::prepare_env();
    let mut lock = lock::Lock::load();

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    {
        let p = proxy.clone();
        MenuEvent::set_event_handler(Some(move |e| {
            let _ = p.send_event(UserEvent::Menu(e));
        }));
    }

    let (menu, ids) = build_menu();

    // Titelleiste in die Oberfläche integriert: transparent, Inhalt bis oben, Ampel oben wie in
    // nativen Apps. Die Oberfläche richtet „nrzCode“ und das Einklapp-Symbol an der gemessenen
    // Ampel-Mitte aus (`--nrz-tl-y`), lässt links Platz und meldet „drag“ zum Ziehen.
    let builder = WindowBuilder::new()
        .with_title("nrzCode")
        .with_inner_size(tao::dpi::LogicalSize::new(1440.0, 920.0))
        .with_min_inner_size(tao::dpi::LogicalSize::new(760.0, 480.0))
        // Durchsichtig: dahinter liegt das Milchglas des Systems (`apply_glass`)
        .with_transparent(true);
    #[cfg(target_os = "macos")]
    let builder = {
        use tao::platform::macos::WindowBuilderExtMacOS;
        builder
            .with_titlebar_transparent(true)
            .with_title_hidden(true)
            .with_fullsize_content_view(true)
            .with_traffic_light_inset(tao::dpi::LogicalPosition::new(16.0, 18.0))
    };
    // Windows: wie am Mac ohne Titelleiste und Menü – die Oberfläche reicht bis oben, rechts oben
    // zeichnet sie Minimieren/Maximieren/Schliessen (`overlay::WIN_SCRIPT`). Der Schatten des
    // Systems bleibt (und mit ihm die Ränder zum Ziehen links, rechts und unten).
    #[cfg(windows)]
    let builder = {
        use tao::platform::windows::WindowBuilderExtWindows;
        builder.with_decorations(false).with_undecorated_shadow(true)
    };
    let window = builder.build(&event_loop).expect("Fenster");
    let main_id = window.id();
    // Milchglas hinter der Oberfläche (davon hängen Durchsichtigkeit und `GLASS_SCRIPT` des WebViews
    // ab); ohne zeichnet die Oberfläche deckend. Windows: am Fenster, schon vor dem WebView.
    #[cfg(windows)]
    let glass = apply_glass(&window);
    #[cfg(not(windows))]
    let glass = cfg!(target_os = "macos");
    GLASS.store(glass, Ordering::Relaxed);

    let server_port: Arc<Mutex<Option<u16>>> = Arc::new(Mutex::new(None));
    let webview = build_webview(&window, &proxy, server_port.clone(), true, false, None).expect("WebView");
    // macOS: erst jetzt – wry setzt eine eigene Inhaltsansicht ein, das Milchglas gehört unter den
    // WebView darin (sonst wäre das Fenster nur durchsichtig, nicht weichgezeichnet)
    #[cfg(target_os = "macos")]
    if glass && !apply_glass(&window) {
        // Dann deckend: die Oberfläche meldet beim Laden ihr Thema (`theme:`), die Antwort schaltet
        // das Milchglas dort ab
        GLASS.store(false, Ordering::Relaxed);
    }

    // Server starten (blockiert nicht das Fenster)
    let server: Arc<Mutex<Option<server::Server>>> = Arc::new(Mutex::new(None));
    match ports {
        Ok((port, preview)) => {
            tracing::info!("Server auf 127.0.0.1:{port}, Vorschau auf {preview}");
            let p = proxy.clone();
            let slot = server.clone();
            std::thread::spawn(move || {
                let r = server::start(port).map(|s| {
                    let port = s.port;
                    *slot.lock().unwrap() = Some(s);
                    port
                });
                let _ = p.send_event(UserEvent::ServerReady(r));
            });
        }
        Err(e) => {
            let _ = proxy.send_event(UserEvent::ServerReady(Err(e)));
        }
    }

    // Updater und Takt für die Sperre: eigene kleine Laufzeit
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("Tokio");
    {
        let p = proxy.clone();
        rt.spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            loop {
                let result = updater::check(&updater::current_version()).await;
                if let Err(e) = &result {
                    tracing::warn!("Update-Prüfung: {e}");
                }
                let _ = p.send_event(UserEvent::UpdateChecked { result, manual: false, quiet: false });
                tokio::time::sleep(updater::CHECK_INTERVAL).await;
            }
        });
        let p = proxy.clone();
        rt.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(15)).await;
                if p.send_event(UserEvent::Tick).is_err() {
                    break;
                }
            }
        });
    }

    let mut updates = UpdateState::default();
    // Läuft gerade eine Abfrage der Rechte? (Dialoge nacheinander, nicht übereinander)
    let perm_busy = Arc::new(AtomicBool::new(false));
    // Letzter automatisch gezeigter Hinweis auf fehlende Rechte (nicht öfter als einmal je Minute)
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut, unused_variables))]
    let mut perm_hint_at: Option<std::time::Instant> = None;
    let mut extra: HashMap<WindowId, (Window, WebView)> = HashMap::new();
    let mut main_window = Some((window, webview));
    // Oberfläche von nrzCode geladen? Beim Start mit Sperre erst nach dem Entsperren.
    let mut ui_loaded = false;
    // Menüleiste (macOS): Symbol, Fenster und wann es zuletzt verschwand (ein Klick auf das
    // Symbol bei offenem Fenster soll es schliessen, nicht gleich wieder öffnen)
    #[cfg(target_os = "macos")]
    let mut tray_icon: Option<tray_icon::TrayIcon> = None;
    #[cfg(target_os = "macos")]
    let mut tray_win: Option<(Window, WebView)> = None;
    #[cfg(target_os = "macos")]
    let mut tray_hidden_at = std::time::Instant::now() - Duration::from_secs(10);
    let _log_guard = log_guard;

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;
        let quit = |control_flow: &mut ControlFlow| {
            if let Some(s) = server.lock().unwrap().as_mut() {
                s.stop(Duration::from_secs(4));
            }
            *control_flow = ControlFlow::Exit;
        };
        let port = *server_port.lock().unwrap();
        match event {
            Event::NewEvents(StartCause::Init) => {
                #[cfg(target_os = "macos")]
                menu.init_for_nsapp();
                #[cfg(target_os = "macos")]
                {
                    let p = proxy.clone();
                    tray_icon = tray::create(move |ev| {
                        let _ = p.send_event(UserEvent::Tray(ev));
                    });
                }
                // Windows: kein Menü (Updates, Sperre und Ordner stehen in Einstellungen → App)
                #[cfg(windows)]
                let _ = &menu;
            }
            Event::Reopen { .. } => {
                if let Some((w, _)) = &main_window {
                    w.set_visible(true);
                    w.set_focus();
                }
            }
            #[cfg(target_os = "macos")]
            Event::WindowEvent { window_id, event: WindowEvent::Resized(_) | WindowEvent::Focused(true), .. } if window_id == main_id => {
                if let Some((w, wv)) = &main_window {
                    send_traffic_light(w, wv);
                }
            }
            #[cfg(windows)]
            Event::WindowEvent { window_id, event: WindowEvent::Resized(_), .. } if window_id == main_id => {
                if let Some((w, wv)) = &main_window {
                    send_window_state(w, wv);
                }
            }
            #[cfg(target_os = "macos")]
            Event::WindowEvent { window_id, event: WindowEvent::Focused(false) | WindowEvent::CloseRequested, .. }
                if tray_win.as_ref().is_some_and(|(w, _)| w.id() == window_id) =>
            {
                if let Some((w, _)) = &tray_win {
                    if w.is_visible() {
                        w.set_visible(false);
                        tray_hidden_at = std::time::Instant::now();
                    }
                }
            }
            Event::WindowEvent { window_id, event: WindowEvent::CloseRequested, .. } => {
                if window_id == main_id {
                    if cfg!(target_os = "macos") {
                        // Wie bei macOS üblich: Fenster schliessen, App (und Agenten) laufen weiter
                        if let Some((w, _)) = &main_window {
                            w.set_visible(false);
                        }
                    } else {
                        // Windows: ohne Dock – Fenster schliessen beendet die App
                        quit(control_flow);
                    }
                } else {
                    extra.remove(&window_id);
                }
            }
            Event::UserEvent(ev) => match ev {
                #[cfg(target_os = "macos")]
                UserEvent::Tray(tray_icon::TrayIconEvent::Click {
                    rect,
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                }) => {
                    let _ = &tray_icon;
                    let open = tray_win.as_ref().is_some_and(|(w, _)| w.is_visible());
                    if open {
                        if let Some((w, _)) = &tray_win {
                            w.set_visible(false);
                        }
                    } else if tray_hidden_at.elapsed() < Duration::from_millis(350) {
                        // Gerade durch den Klick auf das Symbol verschwunden – zu lassen
                    } else if lock.locked || port.is_none() || !ui_loaded {
                        // Gesperrt oder noch nicht bereit: Hauptfenster (mit Sperre) zeigen
                        if let Some((w, _)) = &main_window {
                            w.set_visible(true);
                            w.set_focus();
                        }
                    } else {
                        if tray_win.is_none() {
                            if let (Some(w), Some(p)) = (tray::window(target), port) {
                                let url = format!("http://127.0.0.1:{p}/tray");
                                match build_webview(&w, &proxy, server_port.clone(), false, true, Some(&url)) {
                                    Ok(wv) => tray_win = Some((w, wv)),
                                    Err(e) => tracing::warn!("Menüleisten-Fenster: {e}"),
                                }
                            }
                        }
                        if let Some((w, wv)) = &tray_win {
                            tray::place(w, rect);
                            w.set_visible(true);
                            w.set_focus();
                            let _ = wv.evaluate_script("window.__nrzTrayShown && window.__nrzTrayShown()");
                        }
                    }
                }
                #[cfg(target_os = "macos")]
                UserEvent::Tray(_) => {}
                UserEvent::TrayIpc(msg) => {
                    #[cfg(target_os = "macos")]
                    if let Some((w, _)) = &tray_win {
                        w.set_visible(false);
                        tray_hidden_at = std::time::Instant::now();
                    }
                    // Thread (oder neuen Thread) im Hauptfenster öffnen
                    let target_id = match msg.as_str() {
                        "tray:new" => Some(-1),
                        "tray:main" => None,
                        m => m.strip_prefix("tray:open:").and_then(|id| id.parse::<i64>().ok()),
                    };
                    if let Some((w, wv)) = &main_window {
                        w.set_visible(true);
                        w.set_minimized(false);
                        w.set_focus();
                        if let Some(id) = target_id {
                            let _ = wv.evaluate_script(&format!("window.__nrzOpenThread && window.__nrzOpenThread({id})"));
                        }
                    }
                }
                UserEvent::ServerReady(Ok(p)) => {
                    *server_port.lock().unwrap() = Some(p);
                    // Fehlende Ordnerrechte jetzt anfragen (Dialoge von macOS), nicht erst mitten in
                    // einem Agenten-Lauf; verweigerte zeigt die App einmal je Version
                    #[cfg(target_os = "macos")]
                    {
                        let p = proxy.clone();
                        std::thread::spawn(move || {
                            let denied = permissions::ask_missing_folders();
                            let _ = p.send_event(UserEvent::Permissions);
                            if !denied.is_empty() && permissions::hint_due() {
                                let _ = p.send_event(UserEvent::PermissionHint);
                            }
                        });
                    }
                    if let Some((w, wv)) = &main_window {
                        if lock.locked {
                            // Oberfläche erst nach dem Entsperren laden
                            show_lock(wv);
                        } else {
                            let _ = wv.load_url(&server::login_url(p));
                            ui_loaded = true;
                        }
                        w.set_focus();
                    }
                }
                UserEvent::ServerReady(Err(e)) => {
                    tracing::error!("Server: {e}");
                    if let Some((_, wv)) = &main_window {
                        let _ = wv.load_html(&overlay::loading_page(Some(&e)));
                    }
                }
                UserEvent::PageLoaded(id) => {
                    if id == main_id {
                        if let Some((_w, wv)) = &main_window {
                            #[cfg(target_os = "macos")]
                            send_traffic_light(_w, wv);
                            #[cfg(windows)]
                            send_window_state(_w, wv);
                            if lock.locked {
                                show_lock(wv);
                            } else if let Some(u) = &updates.available {
                                if !updates.busy && updates.dismissed.as_ref() != Some(&u.version) {
                                    show_update(wv, u);
                                }
                            }
                        }
                    }
                }
                UserEvent::PermissionHint => {
                    #[cfg(target_os = "macos")]
                    if let Some((w, wv)) = &main_window {
                        if !lock.locked && perm_hint_at.map(|t: std::time::Instant| t.elapsed() > Duration::from_secs(60)).unwrap_or(true) {
                            perm_hint_at = Some(std::time::Instant::now());
                            w.set_visible(true);
                            show_settings(wv, lock.settings.enabled(), lock.settings.lock_minutes);
                        }
                    }
                }
                UserEvent::Permissions => {
                    if let Some((_, wv)) = &main_window {
                        #[cfg(target_os = "macos")]
                        send_permissions(wv, perm_busy.load(Ordering::SeqCst));
                        #[cfg(not(target_os = "macos"))]
                        let _ = (wv, &perm_busy);
                    }
                }
                UserEvent::Tick => {
                    if lock.due() && lock.lock_now() {
                        tracing::info!("Code-Sperre nach Inaktivität");
                        apply_lock(&main_window, &mut extra);
                    }
                }
                UserEvent::UpdateChecked { result, manual, quiet } => {
                    // Knopf „Nach Updates suchen“ in der Seitenleiste: Ergebnis melden (nach einer
                    // automatischen Prüfung nur ein gefundenes Update)
                    if let Some((_, wv)) = &main_window {
                        let state = match &result {
                            Ok(Some(u)) => Some(("available", u.version.to_string())),
                            Ok(None) if manual => Some(("current", updater::current_version().to_string())),
                            Err(e) if manual => Some(("error", e.clone())),
                            _ => None,
                        };
                        if let Some((status, text)) = state {
                            update_ui(wv, status, &text);
                        }
                    }
                    match result {
                        Ok(Some(u)) => {
                            ids.install.set_text(format!("Update auf {} installieren …", u.version));
                            ids.install.set_enabled(true);
                            let fresh = updates.available.as_ref().map(|a| a.version != u.version).unwrap_or(true);
                            let show = manual || (fresh && updates.dismissed.as_ref() != Some(&u.version));
                            if show && !updates.busy && !lock.locked {
                                if let Some((w, wv)) = &main_window {
                                    if manual {
                                        w.set_visible(true);
                                        w.set_focus();
                                    }
                                    show_update(wv, &u);
                                }
                            }
                            updates.available = Some(u);
                            // NRZCODE_AUTO_UPDATE=1: ohne Rückfrage laden und installieren
                            if std::env::var("NRZCODE_AUTO_UPDATE").is_ok_and(|v| v == "1") {
                                start_download(&rt, &proxy, &mut updates, &main_window);
                            }
                        }
                        Ok(None) => {
                            if manual && !quiet {
                                message(
                                    "Keine Updates",
                                    &format!("nrzCode {} ist die neueste Version.", updater::current_version()),
                                    rfd::MessageLevel::Info,
                                );
                            }
                        }
                        Err(e) => {
                            if manual && !quiet {
                                message("Update-Prüfung fehlgeschlagen", &e, rfd::MessageLevel::Warning);
                            }
                        }
                    }
                }
                UserEvent::Ipc { msg, trusted } => {
                    if msg == "activity" {
                        lock.activity();
                    } else if !trusted {
                        tracing::warn!("Nachricht aus fremder Seite ignoriert");
                    } else if let Some(code) = msg.strip_prefix("lock:unlock:") {
                        if let Some((_, wv)) = &main_window {
                            match lock.unlock(code) {
                                Ok(()) => {
                                    if !ui_loaded {
                                        if let Some(p) = port {
                                            let _ = wv.load_url(&server::login_url(p));
                                            ui_loaded = true;
                                        }
                                    } else {
                                        let _ = wv.evaluate_script("window.__nrzLock && window.__nrzLock.hide()");
                                        if let Some(u) = &updates.available {
                                            if updates.dismissed.as_ref() != Some(&u.version) && !updates.busy {
                                                show_update(wv, u);
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = wv.evaluate_script(&format!(
                                        "window.__nrzLock && window.__nrzLock.error({})",
                                        overlay::js_str(&e)
                                    ));
                                }
                            }
                        }
                    } else if let Some(json) = msg.strip_prefix("settings:save:") {
                        let result = serde_json::from_str::<SettingsSave>(json)
                            .map_err(|e| e.to_string())
                            .and_then(|s| lock.update(&s.current, s.pin.as_deref(), s.minutes));
                        if let Some((_, wv)) = &main_window {
                            let (ok, text) = match &result {
                                Ok(()) => (true, String::new()),
                                Err(e) => (false, e.clone()),
                            };
                            let _ = wv.evaluate_script(&format!(
                                "window.__nrzSettings && window.__nrzSettings.result({ok}, {})",
                                overlay::js_str(&text)
                            ));
                        }
                        if result.is_ok() {
                            tracing::info!(
                                "Code-Sperre: {}, {} Minuten",
                                if lock.settings.enabled() { "ein" } else { "aus" },
                                lock.settings.lock_minutes
                            );
                        }
                    } else if msg == "window:drag" {
                        if let Some((w, _)) = &main_window {
                            let _ = w.drag_window();
                        }
                    } else if msg == "window:zoom" {
                        if let Some((w, _)) = &main_window {
                            w.set_maximized(!w.is_maximized());
                        }
                    } else if msg == "window:fullscreen" {
                        // Canvas-Modus: Vollbild-Knopf (nur noch die Canvas)
                        if let Some((w, _wv)) = &main_window {
                            let full = w.fullscreen().is_some();
                            w.set_fullscreen(if full { None } else { Some(tao::window::Fullscreen::Borderless(None)) });
                            #[cfg(windows)]
                            send_window_state(w, _wv);
                        }
                    } else if msg == "window:minimize" {
                        if let Some((w, _)) = &main_window {
                            w.set_minimized(true);
                        }
                    } else if msg == "window:close" {
                        // Wie der Schliessen-Knopf des Systems
                        if cfg!(target_os = "macos") {
                            if let Some((w, _)) = &main_window {
                                w.set_visible(false);
                            }
                        } else {
                            quit(control_flow);
                        }
                    } else if let Some(dir) = msg.strip_prefix("window:resize:") {
                        // Rahmenloses Fenster (Windows): am oberen Rand ziehen ändert die Grösse
                        if let (Some((w, _)), Some(d)) = (&main_window, resize_direction(dir)) {
                            let _ = w.drag_resize_window(d);
                        }
                    } else if msg == "update:check" {
                        let p = proxy.clone();
                        rt.spawn(async move {
                            let result = updater::check(&updater::current_version()).await;
                            let _ = p.send_event(UserEvent::UpdateChecked { result, manual: true, quiet: false });
                        });
                    } else if msg == "lock:now" {
                        if lock.lock_now() {
                            apply_lock(&main_window, &mut extra);
                        } else if let Some((_, wv)) = &main_window {
                            // Noch kein Code: Einstellungen öffnen
                            let _ = wv.evaluate_script(&format!(
                                "window.__nrzSettings && window.__nrzSettings.show(false, {}, {})",
                                lock.settings.lock_minutes,
                                cfg!(target_os = "macos")
                            ));
                        }
                    } else if msg == "app:data" {
                        open_external(&server::data_dir().to_string_lossy());
                    } else if msg == "app:logs" {
                        open_external(&server::log_dir().join("nrzcode.log").to_string_lossy());
                    } else if let Some(json) = msg.strip_prefix("notify:") {
                        if let Ok(n) = serde_json::from_str::<NotifyMsg>(json) {
                            native_notify(&n.title, &n.body);
                        }
                    } else if msg == "perm:hint" {
                        let _ = proxy.send_event(UserEvent::PermissionHint);
                    } else if let Some(theme) = msg.strip_prefix("theme:") {
                        set_window_theme(&main_window, theme);
                        if !GLASS.load(Ordering::Relaxed) {
                            if let Some((_, wv)) = &main_window {
                                let _ = wv.evaluate_script(overlay::NO_GLASS_SCRIPT);
                            }
                        }
                    } else if msg == "settings:open" {
                        if let Some((w, wv)) = &main_window {
                            if !lock.locked {
                                w.set_visible(true);
                                w.set_focus();
                                show_settings(wv, lock.settings.enabled(), lock.settings.lock_minutes);
                            }
                        }
                    } else if let Some(cmd) = msg.strip_prefix("perm:") {
                        #[cfg(target_os = "macos")]
                        if !lock.locked {
                            permission_command(cmd, &proxy, &perm_busy);
                        }
                        #[cfg(not(target_os = "macos"))]
                        let _ = cmd;
                    } else {
                        match msg.as_str() {
                            "update:install" => start_download(&rt, &proxy, &mut updates, &main_window),
                            "update:check:quiet" => {
                                let p = proxy.clone();
                                rt.spawn(async move {
                                    let result = updater::check(&updater::current_version()).await;
                                    let _ = p.send_event(UserEvent::UpdateChecked { result, manual: true, quiet: true });
                                });
                            }
                            // Oberfläche (neu) geladen: ein schon gefundenes Update zeigen
                            "update:state" => {
                                if let (Some((_, wv)), Some(u)) = (&main_window, &updates.available) {
                                    update_ui(wv, "available", &u.version.to_string());
                                }
                            }
                            "update:later" => {
                                updates.dismissed = updates.available.as_ref().map(|u| u.version.clone());
                                if let Some((_, wv)) = &main_window {
                                    let _ = wv.evaluate_script("window.__nrzUpdate && window.__nrzUpdate.hide()");
                                }
                            }
                            "update:notes" => {
                                if let Some(u) = &updates.available {
                                    open_external(&u.page_url);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                UserEvent::Progress(p) => {
                    if let Some((_, wv)) = &main_window {
                        let _ = wv.evaluate_script(&format!("window.__nrzUpdate && window.__nrzUpdate.progress({p})"));
                    }
                }
                UserEvent::Downloaded(Err(e)) => {
                    updates.busy = false;
                    tracing::error!("Update: {e}");
                    status(&main_window, "Update fehlgeschlagen", &e, true);
                }
                UserEvent::Downloaded(Ok(dmg)) => {
                    status(&main_window, "Update wird installiert …", "nrzCode startet gleich neu.", false);
                    match updater::install(&dmg) {
                        Ok(Installed::Replaced(bundle)) => {
                            tracing::info!("Update installiert, Neustart");
                            updater::relaunch_after_exit(&bundle);
                            quit(control_flow);
                        }
                        Ok(Installed::OpenedDmg(text)) => {
                            updates.busy = false;
                            status(&main_window, "Update bereit", &text, false);
                        }
                        Err(e) => {
                            updates.busy = false;
                            status(&main_window, "Update fehlgeschlagen", &e, true);
                        }
                    }
                }
                UserEvent::Escaped(url) => {
                    tracing::warn!("Hauptfenster verliess die App: {url} – im Browser geöffnet");
                    open_external(&url);
                    if let (Some(p), Some((_, wv))) = (port, &main_window) {
                        if lock.locked {
                            // Ladeseite mit Sperre (PageLoaded); die Oberfläche kommt nach dem Entsperren
                            let _ = wv.load_html(&overlay::loading_page(None));
                            ui_loaded = false;
                        } else {
                            let _ = wv.load_url(&server::login_url(p));
                            ui_loaded = true;
                        }
                    }
                }
                UserEvent::OpenWindow(url) => {
                    if !lock.locked {
                        open_window(target, &proxy, &server_port, &mut extra, &url);
                    }
                }
                UserEvent::Menu(e) => {
                    let id = e.id();
                    if id == ids.quit.id() {
                        quit(control_flow);
                    } else if id == ids.settings.id() {
                        if let Some((w, wv)) = &main_window {
                            w.set_visible(true);
                            w.set_focus();
                            if !lock.locked {
                                // Einstellungen der Oberfläche; ohne sie (Start, Fehlerseite) die der App
                                let _ = wv.evaluate_script(&format!(
                                    "if (window.__nrzOpenSettings) window.__nrzOpenSettings(); else if (window.__nrzSettings) window.__nrzSettings.show({}, {}, {})",
                                    lock.settings.enabled(),
                                    lock.settings.lock_minutes,
                                    cfg!(target_os = "macos")
                                ));
                            }
                        }
                    } else if id == ids.lock.id() {
                        if lock.lock_now() {
                            apply_lock(&main_window, &mut extra);
                        } else if let Some((_, wv)) = &main_window {
                            // Noch kein Code: Einstellungen öffnen
                            let _ = wv.evaluate_script(&format!(
                                "window.__nrzSettings && window.__nrzSettings.show(false, {}, {})",
                                lock.settings.lock_minutes,
                                cfg!(target_os = "macos")
                            ));
                        }
                    } else if id == ids.check.id() {
                        let p = proxy.clone();
                        rt.spawn(async move {
                            let result = updater::check(&updater::current_version()).await;
                            let _ = p.send_event(UserEvent::UpdateChecked { result, manual: true, quiet: false });
                        });
                    } else if id == ids.install.id() {
                        start_download(&rt, &proxy, &mut updates, &main_window);
                    } else if id == ids.reload.id() {
                        // Gesperrt: nicht neu laden (die Oberfläche bliebe sonst kurz sichtbar)
                        if let (Some(p), false) = (port, lock.locked) {
                            if let Some((_, wv)) = &main_window {
                                let _ = wv.load_url(&server::login_url(p));
                                ui_loaded = true;
                            }
                        }
                    } else if id == ids.data.id() {
                        open_external(&server::data_dir().to_string_lossy());
                    } else if id == ids.logs.id() {
                        open_external(&server::log_dir().join("nrzcode.log").to_string_lossy());
                    } else if id == ids.repo.id() {
                        open_external(REPO_PAGE);
                    } else if id == ids.close.id() {
                        // Fenster mit Fokus schliessen
                        if let Some(k) = extra.iter().find(|(_, (w, _))| w.is_focused()).map(|(k, _)| *k) {
                            extra.remove(&k);
                        } else if let Some((w, _)) = &main_window {
                            w.set_visible(false);
                        }
                    }
                }
            },
            Event::LoopDestroyed => {
                main_window.take();
                if let Some(s) = server.lock().unwrap().as_mut() {
                    s.stop(Duration::from_secs(4));
                }
            }
            _ => {}
        }
    });
}

fn build_webview(
    window: &Window,
    proxy: &EventLoopProxy<UserEvent>,
    server_port: Arc<Mutex<Option<u16>>>,
    is_main: bool,
    tray: bool,
    url: Option<&str>,
) -> wry::Result<WebView> {
    let id = window.id();
    let p_ipc = proxy.clone();
    let p_load = proxy.clone();
    let p_new = proxy.clone();
    let port_ipc = server_port.clone();
    let port_load = server_port.clone();
    // Entwicklerwerkzeuge nur im Debug-Build oder mit NRZCODE_DEVTOOLS=1 – sonst liesse sich die
    // Code-Sperre über „Element untersuchen" entfernen.
    let devtools = cfg!(debug_assertions) || std::env::var("NRZCODE_DEVTOOLS").is_ok_and(|v| v == "1");
    let glass = GLASS.load(Ordering::Relaxed);
    let builder = WebViewBuilder::new()
        .with_initialization_script_for_main_only(overlay::INIT_SCRIPT, true)
        .with_initialization_script_for_main_only(overlay::LOCK_SCRIPT, true)
        .with_initialization_script_for_main_only(if is_main && cfg!(target_os = "macos") { overlay::WINDOW_SCRIPT } else { "" }, true)
        .with_initialization_script_for_main_only(if is_main && cfg!(windows) { overlay::WIN_SCRIPT } else { "" }, true)
        .with_initialization_script_for_main_only(if is_main || tray { overlay::NATIVE_SCRIPT } else { "" }, true)
        .with_initialization_script_for_main_only(if is_main && glass { overlay::GLASS_SCRIPT } else { "" }, true)
        // Fenster der Menüleiste: durchsichtig, die Seite zeichnet runde Ecken; Hauptfenster mit
        // Milchglas: durchsichtig, die Oberfläche tönt es nur
        .with_transparent(tray || (is_main && glass))
        .with_ipc_handler(move |req| {
            // Nur das Hauptfenster mit der eigenen Oberfläche (oder der Startseite der App) darf
            // Sperre, Einstellungen und Updates steuern – nicht Vorschauen oder fremde Seiten.
            let uri = req.uri();
            let own = match (uri.host(), uri.port_u16(), *port_ipc.lock().unwrap()) {
                (Some("127.0.0.1"), Some(p), Some(sp)) => p == sp,
                (None, None, _) => true,
                _ => uri.scheme_str() == Some("about") || uri.to_string().starts_with("about:"),
            };
            // Fenster der Menüleiste: nur seine eigenen Nachrichten (Thread öffnen usw.)
            if tray && own && req.body().starts_with("tray:") {
                let _ = p_ipc.send_event(UserEvent::TrayIpc(req.body().clone()));
                return;
            }
            let trusted = is_main && own;
            let _ = p_ipc.send_event(UserEvent::Ipc { msg: req.body().clone(), trusted });
        })
        .with_on_page_load_handler(move |ev, url| {
            // Das Hauptfenster zeigt nur die eigene Oberfläche: eine fremde Seite (nur Hauptrahmen –
            // Vorschau und Browser im Canvas sind iframes) geht in den Browser des Systems, sonst
            // bliebe die App darin hängen (ohne Zurück)
            if is_main && is_web(&url) && !is_own(&url, *port_load.lock().unwrap()) {
                let _ = p_load.send_event(UserEvent::Escaped(url));
                return;
            }
            if matches!(ev, PageLoadEvent::Finished) {
                let _ = p_load.send_event(UserEvent::PageLoaded(id));
            }
        })
        .with_new_window_req_handler(move |url, _features| {
            // Seiten des eingebauten Servers (Anhänge) in einem App-Fenster, alles andere – auch
            // localhost anderer Ports (Server der Agenten) – im Browser des Systems
            if is_own(&url, *server_port.lock().unwrap()) {
                let _ = p_new.send_event(UserEvent::OpenWindow(url));
            } else {
                open_external(&url);
            }
            NewWindowResponse::Deny
        })
        .with_download_started_handler(|url, path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .filter(|n| !n.is_empty())
                .or_else(|| url.rsplit('/').next().map(|s| s.split('?').next().unwrap_or(s).to_string()))
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| "download".into());
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            *path = unique_path(home.join("Downloads").join(name));
            true
        })
        .with_download_completed_handler(|_url, path, ok| {
            if let (Some(p), true) = (path, ok) {
                reveal(&p);
            }
        })
        // Mikrofon für Dieter: die eigene Oberfläche darf (macOS fragt beim ersten Mal selbst),
        // fremde Seiten in Zusatzfenstern fragen wie im Browser
        .with_permission_handler(move |kind| match kind {
            wry::PermissionKind::Microphone if is_main => wry::PermissionResponse::Allow,
            _ => wry::PermissionResponse::Default,
        })
        .with_clipboard(true)
        // Merkmal der App: nur so antwortet der eingebaute Server (nicht im Browser)
        .with_user_agent(server::user_agent())
        .with_devtools(devtools)
        .with_back_forward_navigation_gestures(false);
    // Windows: Browser-Kürzel von WebView2 (F5, Strg+R, Alt+←, Strg+P …) aus; Kopieren/Einfügen bleibt
    #[cfg(windows)]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        builder.with_browser_accelerator_keys(!is_main)
    };
    let builder = match url {
        Some(u) => builder.with_url(u),
        None => builder.with_html(overlay::loading_page(None)),
    };
    builder.build(window)
}

fn open_window(
    target: &EventLoopWindowTarget<UserEvent>,
    proxy: &EventLoopProxy<UserEvent>,
    server_port: &Arc<Mutex<Option<u16>>>,
    extra: &mut HashMap<WindowId, (Window, WebView)>,
    url: &str,
) {
    let Ok(window) = WindowBuilder::new()
        .with_title("nrzCode")
        .with_inner_size(tao::dpi::LogicalSize::new(1100.0, 780.0))
        .build(target)
    else {
        return;
    };
    // Nachrichten aus Zusatzfenstern gelten nie als vertrauenswürdig
    match build_webview(&window, proxy, server_port.clone(), false, false, Some(url)) {
        Ok(wv) => {
            extra.insert(window.id(), (window, wv));
        }
        Err(e) => tracing::error!("Fenster: {e}"),
    }
}

/// Rand für `window:resize:<n|ne|nw|…>` (rahmenloses Fenster unter Windows).
fn resize_direction(d: &str) -> Option<tao::window::ResizeDirection> {
    use tao::window::ResizeDirection as R;
    Some(match d {
        "n" => R::North,
        "s" => R::South,
        "e" => R::East,
        "w" => R::West,
        "ne" => R::NorthEast,
        "nw" => R::NorthWest,
        "se" => R::SouthEast,
        "sw" => R::SouthWest,
        _ => return None,
    })
}

/// Windows: maximiert/Vollbild an die Oberfläche (Symbol des Maximieren-Knopfs, Ränder).
#[cfg(windows)]
fn send_window_state(window: &Window, wv: &WebView) {
    let _ = wv.evaluate_script(&format!(
        "window.__nrzWinState && window.__nrzWinState({}, {})",
        window.is_maximized(),
        window.fullscreen().is_some()
    ));
}

/// Sperre zeigen und Zusatzfenster (Anhänge, Vorschau) schliessen.
fn apply_lock(main: &Option<(Window, WebView)>, extra: &mut HashMap<WindowId, (Window, WebView)>) {
    extra.clear();
    if let Some((_, wv)) = main {
        let _ = wv.evaluate_script("window.__nrzSettings && document.getElementById('nrz-settings') && document.getElementById('nrz-settings').remove()");
        show_lock(wv);
    }
}

/// `perm:list`, `perm:request:<id>`, `perm:all`, `perm:open:<id>` aus den Einstellungen.
#[cfg(target_os = "macos")]
fn permission_command(cmd: &str, proxy: &EventLoopProxy<UserEvent>, busy: &Arc<AtomicBool>) {
    if cmd == "list" {
        let _ = proxy.send_event(UserEvent::Permissions);
        return;
    }
    if let Some(kind) = cmd.strip_prefix("open:").and_then(permissions::Kind::from_id) {
        permissions::open_settings(kind);
        return;
    }
    let job: Box<dyn FnOnce() + Send> = if cmd == "all" {
        Box::new(permissions::request_all)
    } else if let Some(kind) = cmd.strip_prefix("request:").and_then(permissions::Kind::from_id) {
        Box::new(move || {
            let state = permissions::request(kind);
            tracing::info!("Recht {}: {state:?}", kind.id());
        })
    } else {
        return;
    };
    if busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = proxy.send_event(UserEvent::Permissions);
    let p = proxy.clone();
    let busy = busy.clone();
    // Die Abfragen blockieren, bis der Dialog von macOS beantwortet ist
    std::thread::spawn(move || {
        job();
        busy.store(false, Ordering::SeqCst);
        let _ = p.send_event(UserEvent::Permissions);
    });
}

/// Einstellungen der App (Code-Sperre, unter macOS mit den Rechten) zeigen.
fn show_settings(wv: &WebView, enabled: bool, minutes: u32) {
    let _ = wv.evaluate_script(&format!("window.__nrzSettings && window.__nrzSettings.show({enabled}, {minutes}, {})", cfg!(target_os = "macos")));
}

#[cfg(target_os = "macos")]
fn send_permissions(wv: &WebView, busy: bool) {
    let list = serde_json::to_string(&permissions::list()).unwrap_or_else(|_| "[]".into());
    let _ = wv.evaluate_script(&format!("window.__nrzSettings && window.__nrzSettings.perms({list}, {busy})"));
}

fn show_lock(wv: &WebView) {
    let _ = wv.evaluate_script("window.__nrzLock && window.__nrzLock.show()");
}

/// Seite des eingebauten Servers (`http://127.0.0.1:<port>` bzw. `localhost`)?
fn is_own(url: &str, port: Option<u16>) -> bool {
    let Some(p) = port else { return false };
    ["127.0.0.1", "localhost"].iter().any(|h| {
        let base = format!("http://{h}:{p}");
        url.strip_prefix(&base).is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']))
    })
}

/// Adresse im Netz (nicht `about:`, `data:` oder die Ladeseite)?
fn is_web(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://")
}

/// Liegt Milchglas hinter dem Hauptfenster? (Gesetzt vor dem ersten WebView.)
static GLASS: AtomicBool = AtomicBool::new(false);

/// Milchglas hinter dem Hauptfenster – der Schreibtisch und Fenster dahinter scheinen
/// weichgezeichnet durch, nicht nur durchsichtig:
/// - macOS: Vibrancy (`NSVisualEffectView`, wie die Seitenleisten nativer Apps) unter dem WebView,
///   nach dessen Bau (siehe [`content_view`]).
/// - Windows 11 (ab 22H2): eigenes Acryl, getönt nach dem Thema der Oberfläche – bleibt auch,
///   wenn das Fenster nicht vorn ist. Windows 10 und 11 21H2: Weichzeichnen hinter dem Fenster –
///   deren Acryl ruckelt beim Ziehen und Vergrössern. Siehe [`glass`].
///
/// `false`, wenn das System es nicht kann – dann zeichnet die Oberfläche deckend.
fn apply_glass(window: &Window) -> bool {
    #[cfg(target_os = "macos")]
    let result = match content_view(window) {
        Some(view) => window_vibrancy::apply_vibrancy(
            view,
            window_vibrancy::NSVisualEffectMaterial::UnderWindowBackground,
            Some(window_vibrancy::NSVisualEffectState::Active),
            None,
        )
        .map_err(|e| format!("{e:?}")),
        None => Err("keine Inhaltsansicht".to_string()),
    };
    #[cfg(windows)]
    let result = {
        use tao::platform::windows::WindowExtWindows;
        // Tönung vorerst dunkel – die Oberfläche meldet ihr Thema beim Laden (`theme:`)
        glass::apply(window.hwnd(), glass::Tint::Dark)
    };
    #[cfg(not(any(target_os = "macos", windows)))]
    let result: Result<(), String> = {
        let _ = window;
        Err("nicht unterstützt".into())
    };
    match result {
        Ok(()) => true,
        Err(e) => {
            tracing::info!("Kein Milchglas: {e}");
            false
        }
    }
}

/// Aktuelle Inhaltsansicht des Fensters als Handle für window-vibrancy. wry ersetzt die Ansicht
/// von tao durch eine eigene mit dem WebView darin; `Window` meldet aber weiter die alte, die
/// nicht mehr im Fenster hängt – Vibrancy dort bliebe unsichtbar.
#[cfg(target_os = "macos")]
struct ContentView(std::ptr::NonNull<std::ffi::c_void>);

#[cfg(target_os = "macos")]
impl tao::rwh_06::HasWindowHandle for ContentView {
    fn window_handle(&self) -> Result<tao::rwh_06::WindowHandle<'_>, tao::rwh_06::HandleError> {
        let raw = tao::rwh_06::RawWindowHandle::AppKit(tao::rwh_06::AppKitWindowHandle::new(self.0));
        // SAFETY: Das Fenster hält seine Inhaltsansicht, solange es offen ist; das Handle wird nur
        // sofort (im Hauptthread) benutzt.
        Ok(unsafe { tao::rwh_06::WindowHandle::borrow_raw(raw) })
    }
}

#[cfg(target_os = "macos")]
fn content_view(window: &Window) -> Option<ContentView> {
    use objc2_app_kit::NSWindow;
    use tao::platform::macos::WindowExtMacOS;
    let ns = unsafe { (window.ns_window() as *const NSWindow).as_ref()? };
    let view = ns.contentView()?;
    std::ptr::NonNull::new(objc2::rc::Retained::as_ptr(&view) as *mut std::ffi::c_void).map(ContentView)
}

/// Erscheinungsbild des Fensters wie das Thema der Oberfläche (`dark`, `light`, `mondnacht`,
/// sonst wie das System) – das Milchglas dahinter (Vibrancy bzw. Acryl) wird damit dunkel bzw.
/// hell, unter Windows zusätzlich in der Farbe des Themas getönt.
fn set_window_theme(main: &Option<(Window, WebView)>, theme: &str) {
    let Some((w, _)) = main else { return };
    let native = match theme {
        "dark" | "mondnacht" => Some(tao::window::Theme::Dark),
        "light" => Some(tao::window::Theme::Light),
        _ => None,
    };
    w.set_theme(native);
    #[cfg(windows)]
    if GLASS.load(Ordering::Relaxed) {
        use tao::platform::windows::WindowExtWindows;
        let tint = match theme {
            "mondnacht" => glass::Tint::Mondnacht,
            "light" => glass::Tint::Light,
            "dark" => glass::Tint::Dark,
            // wie das System
            _ if w.theme() == tao::window::Theme::Light => glass::Tint::Light,
            _ => glass::Tint::Dark,
        };
        glass::retint(w.hwnd(), tint);
    }
}

/// Ergebnis einer Update-Prüfung an die Oberfläche: `available` (mit Version), `current` (mit der
/// eigenen Version) oder `error` (mit Text).
fn update_ui(wv: &WebView, status: &str, text: &str) {
    let _ = wv.evaluate_script(&format!(
        "window.__nrzUpdateState && window.__nrzUpdateState({}, {})",
        overlay::js_str(status),
        overlay::js_str(text)
    ));
}

fn show_update(wv: &WebView, u: &Update) {
    let js = format!(
        "window.__nrzUpdate && window.__nrzUpdate.show({}, {})",
        overlay::js_str(&u.version.to_string()),
        overlay::js_str(&u.title)
    );
    let _ = wv.evaluate_script(&js);
}

fn status(main: &Option<(Window, WebView)>, title: &str, text: &str, error: bool) {
    if let Some((w, wv)) = main {
        w.set_visible(true);
        let js = format!(
            "window.__nrzUpdate && window.__nrzUpdate.status({}, {}, {error})",
            overlay::js_str(title),
            overlay::js_str(text)
        );
        let _ = wv.evaluate_script(&js);
    }
}

fn start_download(
    rt: &tokio::runtime::Runtime,
    proxy: &EventLoopProxy<UserEvent>,
    updates: &mut UpdateState,
    main: &Option<(Window, WebView)>,
) {
    let Some(u) = updates.available.clone() else {
        return;
    };
    if updates.busy {
        return;
    }
    updates.busy = true;
    if let Some((w, wv)) = main {
        w.set_visible(true);
        let _ = wv.evaluate_script("window.__nrzUpdate && window.__nrzUpdate.progress(0)");
    }
    let p = proxy.clone();
    rt.spawn(async move {
        let pp = p.clone();
        let last = Mutex::new(-1i64);
        let r = updater::download(&u, move |f| {
            // Nur bei ganzen Prozenten melden
            let step = if f < 0.0 { -1 } else { (f * 100.0) as i64 };
            let mut l = last.lock().unwrap();
            if *l != step {
                *l = step;
                let _ = pp.send_event(UserEvent::Progress(f));
            }
        })
        .await;
        let _ = p.send_event(UserEvent::Downloaded(r));
    });
}

fn build_menu() -> (Menu, MenuIds) {
    let version = updater::current_version().to_string();
    let ids = MenuIds {
        settings: MenuItem::new("Einstellungen …", true, Some("CmdOrCtrl+Comma".parse().unwrap())),
        lock: MenuItem::new("Jetzt sperren", true, Some("CmdOrCtrl+L".parse().unwrap())),
        check: MenuItem::new("Nach Updates suchen …", true, None),
        install: MenuItem::new("Update installieren …", false, None),
        quit: MenuItem::new("nrzCode beenden", true, Some("CmdOrCtrl+Q".parse().unwrap())),
        // Ohne Tastenkürzel: ⌘R soll die App nicht wie eine Webseite neu laden
        reload: MenuItem::new("Oberfläche neu laden", true, None),
        data: MenuItem::new("Datenordner im Finder zeigen", true, None),
        logs: MenuItem::new("Protokoll öffnen", true, None),
        repo: MenuItem::new("nrzCode in nrzGit", true, None),
        close: MenuItem::new("Fenster schliessen", true, Some("CmdOrCtrl+W".parse().unwrap())),
    };
    let about = AboutMetadata {
        name: Some("nrzCode".into()),
        version: Some(version),
        comments: Some("Coding-Agenten, verbunden mit nrzGit – komplett in Rust.".into()),
        license: Some("AGPL-3.0-or-later".into()),
        ..Default::default()
    };
    let app = Submenu::with_items(
        "nrzCode",
        true,
        &[
            &PredefinedMenuItem::about(Some("Über nrzCode"), Some(about)),
            &PredefinedMenuItem::separator(),
            &ids.settings,
            &ids.lock,
            &PredefinedMenuItem::separator(),
            &ids.check,
            &ids.install,
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &ids.quit,
        ],
    )
    .unwrap();
    let edit = Submenu::with_items(
        "Bearbeiten",
        true,
        &[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::select_all(None),
        ],
    )
    .unwrap();
    let view = Submenu::with_items(
        "Ansicht",
        true,
        &[&ids.reload, &PredefinedMenuItem::separator(), &PredefinedMenuItem::fullscreen(None)],
    )
    .unwrap();
    let window = Submenu::with_items(
        "Fenster",
        true,
        &[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(None),
            &PredefinedMenuItem::separator(),
            &ids.close,
        ],
    )
    .unwrap();
    let help = Submenu::with_items(
        "Hilfe",
        true,
        &[&ids.data, &ids.logs, &PredefinedMenuItem::separator(), &ids.repo],
    )
    .unwrap();
    let menu = Menu::with_items(&[&app, &edit, &view, &window, &help]).unwrap();
    #[cfg(target_os = "macos")]
    {
        window.set_as_windows_menu_for_nsapp();
        help.set_as_help_menu_for_nsapp();
    }
    (menu, ids)
}

/// Mitteilung von macOS (im Mitteilungszentrum, mit Ton).
fn native_notify(title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    #[allow(deprecated)]
    {
        use objc2_foundation::{NSString, NSUserNotification, NSUserNotificationCenter, NSUserNotificationDefaultSoundName};
        let title: String = title.chars().take(120).collect();
        let body: String = body.chars().take(400).collect();
        // SAFETY: Aufruf im Hauptthread (Event-Loop); deprecated, aber ohne Signatur nutzbar.
        #[allow(deprecated)]
        unsafe {
            let n = NSUserNotification::new();
            n.setTitle(Some(&NSString::from_str(&title)));
            n.setInformativeText(Some(&NSString::from_str(&body)));
            n.setSoundName(Some(NSUserNotificationDefaultSoundName));
            NSUserNotificationCenter::defaultUserNotificationCenter().deliverNotification(&n);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (title, body);
}

fn open_external(target: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // start "" <ziel> öffnet Adressen im Browser und Dateien/Ordner im Explorer
        let _ = std::process::Command::new("cmd").args(["/C", "start", ""]).arg(target).creation_flags(0x0800_0000).spawn();
    }
    #[cfg(not(windows))]
    let _ = std::process::Command::new("/usr/bin/open").arg(target).spawn();
}

/// Datei im Finder bzw. Explorer zeigen.
fn reveal(path: &std::path::Path) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("/usr/bin/open").arg("-R").arg(path).spawn();
}

fn message(title: &str, text: &str, level: rfd::MessageLevel) {
    rfd::MessageDialog::new().set_title(title).set_description(text).set_level(level).set_buttons(rfd::MessageButtons::Ok).show();
}

fn unique_path(p: PathBuf) -> PathBuf {
    if !p.exists() {
        return p;
    }
    let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let dir = p.parent().map(PathBuf::from).unwrap_or_default();
    (2..1000).map(|i| dir.join(format!("{stem} {i}{ext}"))).find(|c| !c.exists()).unwrap_or(p)
}

fn single_instance_lock() -> Result<std::fs::File, String> {
    // Nach einem Update startet die neue Version, während die alte noch beendet: kurz warten
    let relaunched = std::env::args().any(|a| a == "--relaunched");
    let deadline = std::time::Instant::now() + Duration::from_secs(if relaunched { 20 } else { 0 });
    loop {
        match try_instance_lock() {
            Ok(f) => return Ok(f),
            Err(e) if std::time::Instant::now() >= deadline => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

fn try_instance_lock() -> Result<std::fs::File, String> {
    let dir = server::data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("app.lock"))
        .map_err(|e| e.to_string())?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(_) => Err("nrzCode läuft bereits.".into()),
    }
}

/// Protokoll nach `~/Library/Logs/nrzCode/nrzcode.log`.
fn init_logging() -> Option<()> {
    use tracing_subscriber::EnvFilter;
    let dir = server::log_dir();
    let _ = std::fs::create_dir_all(&dir);
    let filter = || {
        EnvFilter::try_from_env("NRZCODE_LOG")
            .unwrap_or_else(|_| EnvFilter::new("info,rocket::server=warn,rocket::launch::_=warn,rocket::shield=warn"))
    };
    let path = dir.join("nrzcode.log");
    // Altes Protokoll ab 10 MB beiseitelegen
    if std::fs::metadata(&path).map(|m| m.len() > 10 * 1024 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("nrzcode.old.log"));
    }
    match std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => {
            tracing_subscriber::fmt().with_ansi(false).with_env_filter(filter()).with_writer(Mutex::new(file)).init();
        }
        Err(_) => {
            tracing_subscriber::fmt().with_env_filter(filter()).init();
        }
    }
    Some(())
}
