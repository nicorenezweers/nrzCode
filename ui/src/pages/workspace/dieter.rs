//! Dieter – Sprachsteuerung des Rasters, unten in der Mitte.
//!
//! * Mikrofon an: Sätze werden an Sprachpausen getrennt (`mic`), auf dem Server mit Whisper erkannt
//!   (`POST /voice/transcribe`, mit den Fensternamen als Hilfe) und an Dieter gegeben
//!   (`POST /voice/command`), der daraus Aufträge macht: „Sag Lena, sie soll eine Webseite
//!   programmieren, und Bernd soll die Tests reparieren“ → zwei ausführliche Prompts.
//! * „Dieter, schalte dich ab“: Dieter schläft – er hört weiter zu, reagiert aber nur noch auf
//!   „Dieter, schalte dich an“. Beides erkennt die Oberfläche selbst (ohne Agent, sofort).
//! * Antworten stehen in der Sprechblase und werden vorgelesen (abschaltbar).
//! * Einstellungen (Modell der Erkennung, Sprache, Dieters Agent/Konto/Modell): [`DieterSettings`].

use super::grid::{pane_created, update_panes, FOCUS, MAX, PANES, PANE_NAMES};
use crate::api;
use crate::components::common::*;
use crate::components::overlay::toast_error;
use crate::i18n::{tr, trf};
use crate::mic;
use crate::state::{self, ME};
use crate::util;
use crate::Route;
use dioxus::prelude::*;
use futures_util::StreamExt;
use nrzcode_shared::agents::PostMessage;
use nrzcode_shared::voice::{DieterAction, DieterPane, DieterReply, DieterRequest, DieterTurn, SttModel, Transcription, VoiceSettings};
use nrzcode_shared::{AgentKind, ThreadSummary};

const AWAKE_KEY: &str = "nrz-dieter-awake";

#[derive(Clone, Copy, PartialEq, Debug)]
enum Phase {
    Idle,
    /// Satz wird erkannt
    Hearing,
    /// Dieter überlegt (Agent)
    Thinking,
}

/// Was im Satz an Dieter selbst geht.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Switch {
    On,
    Off,
    None,
}

/// Klingt das Wort wie „Dieter“? (Whisper schreibt den Namen manchmal anders.)
fn is_dieter(w: &str) -> bool {
    matches!(w, "dieter" | "dieta" | "dita" | "diter" | "dieder" | "diether" | "dietr" | "deeter" | "dietter" | "dietar" | "dieterle" | "ditter")
        || (w.chars().count() >= 5 && w.starts_with("di") && distance(w, "dieter") <= 2)
}

fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// „Dieter, schalte dich ab/an“ (und Varianten) erkennen – nur in kurzen Sätzen.
pub fn switch_of(text: &str) -> Switch {
    let norm: String = text.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    let words: Vec<&str> = norm.split_whitespace().collect();
    let Some(pos) = words.iter().position(|w| is_dieter(w)) else { return Switch::None };
    let rest = &words[pos + 1..];
    if rest.len() > 6 || words.len() > 9 {
        return Switch::None;
    }
    let has = |list: &[&str]| rest.iter().any(|w| list.contains(w));
    let off = has(&["ab", "aus", "schlafen", "schlaf", "still", "ruhe", "pause", "off", "stopp", "sleep"]);
    let on = has(&["an", "ein", "auf", "wach", "aufwachen", "on", "up", "da"]);
    let verb = rest.len() <= 2 || has(&["schalte", "schalt", "schaltet", "schalten", "geh", "gehe", "sei", "mach", "turn", "wake", "wach", "wache", "aufwachen", "bist", "go"]);
    match (verb, on, off) {
        (true, true, false) => Switch::On,
        (true, false, true) => Switch::Off,
        _ => Switch::None,
    }
}

/// Sprache für das Vorlesen (`de-DE` …).
fn speech_lang(language: &str) -> String {
    match language {
        "de" => "de-DE".into(),
        "en" => "en-US".into(),
        "" => match crate::i18n::LANG.read().as_str() {
            "en" => "en-US".into(),
            _ => "de-DE".into(),
        },
        l => l.to_string(),
    }
}

/// Fenster des Rasters mit Namen (für Dieter und Whisper).
/// Fehler des Mikrofons als Satz. Fehlt die Erlaubnis, zeigt die App gleich die Rechte von
/// nrzCode (dort führt „Einstellungen“ direkt zu Datenschutz & Sicherheit → Mikrofon).
fn mic_error(e: &str) -> String {
    if e == "denied" {
        util::native("perm:hint");
    }
    match e {
        "denied" if util::in_app() => tr("Kein Zugriff auf das Mikrofon. Erlaube es in den Rechten von nrzCode (öffnet sich gleich) oder unter Systemeinstellungen → Datenschutz & Sicherheit → Mikrofon."),
        "denied" => tr("Kein Zugriff auf das Mikrofon – bitte im Browser erlauben."),
        "nodevice" => tr("Kein Mikrofon gefunden."),
        "unsupported" => tr("Mikrofon ist hier nicht verfügbar (nur über https oder in der App)."),
        other => trf("Mikrofon: {0}", &[&other]),
    }
}

fn panes() -> Vec<(i64, String)> {
    let names = PANE_NAMES.read();
    PANES.read().iter().map(|k| (*k, names.get(k).cloned().unwrap_or_default())).filter(|(_, n)| !n.is_empty()).collect()
}

fn pane_by_name(name: &str) -> Option<i64> {
    panes().into_iter().find(|(_, n)| n.eq_ignore_ascii_case(name.trim())).map(|(k, _)| k)
}

/// Eine Aktion ausführen; Ergebnis als kurzer Text für die Sprechblase.
async fn run_action(a: DieterAction, project_id: Option<i64>) -> Result<String, String> {
    match a.kind.as_str() {
        "send" => {
            let key = pane_by_name(&a.to).ok_or_else(|| trf("Kein Fenster „{0}“.", &[&a.to]))?;
            if key > 0 {
                let t = api::post::<ThreadSummary, _>(&format!("/threads/{key}/messages"), &PostMessage { text: a.prompt.clone(), attachments: Vec::new(), interrupt: false })
                    .await
                    .map_err(|e| e.message)?;
                let queued = t.status.is_active();
                state::upsert_thread(t);
                Ok(if queued { trf("{0}: wartet, bis der Agent fertig ist", &[&a.to]) } else { trf("{0}: Auftrag übergeben", &[&a.to]) })
            } else {
                let t = crate::pages::agents::start_thread(project_id, a.prompt.clone()).await.map_err(|e| e.message)?;
                pane_created(key, t.id);
                Ok(trf("{0}: neuer Chat gestartet", &[&a.to]))
            }
        }
        "stop" => {
            let key = pane_by_name(&a.to).filter(|k| *k > 0).ok_or_else(|| trf("In „{0}“ läuft nichts.", &[&a.to]))?;
            let t = api::action::<ThreadSummary>(&format!("/threads/{key}/stop")).await.map_err(|e| e.message)?;
            state::upsert_thread(t);
            Ok(trf("{0}: gestoppt", &[&a.to]))
        }
        "focus" => {
            let key = pane_by_name(&a.to).ok_or_else(|| trf("Kein Fenster „{0}“.", &[&a.to]))?;
            *FOCUS.write() = Some(key);
            Ok(trf("{0}: gross", &[&a.to]))
        }
        "unfocus" => {
            *FOCUS.write() = None;
            Ok(tr("Alle gleich gross"))
        }
        "new" => {
            if PANES.peek().len() >= MAX {
                return Err(tr("Das Raster ist voll."));
            }
            let t = crate::pages::agents::start_thread(project_id, a.prompt.clone()).await.map_err(|e| e.message)?;
            let id = t.id;
            update_panes(move |l| l.push(id));
            Ok(tr("Neues Fenster gestartet"))
        }
        _ => Err(String::new()),
    }
}

#[derive(Clone, PartialEq, Default)]
struct Bubble {
    heard: String,
    reply: String,
    done: Vec<String>,
    failed: Vec<String>,
    /// Hinweis (z. B. Dieter schläft, Fehler)
    note: String,
}

/// Leiste unten in der Mitte des Rasters bzw. der Canvas: Mikrofon, Zustand, Sprechblase.
/// Auf der Canvas (`canvas`) kennt Dieter alle Fenster, Arbeitsbereiche und die Zeichnung.
#[component]
pub fn DieterDock(project_id: Option<i64>, can_add: bool, #[props(default)] canvas: bool) -> Element {
    let mut listening = use_signal(|| false);
    let mut starting = use_signal(|| false);
    let awake = use_signal(|| util::storage_get(AWAKE_KEY).as_deref() != Some("0"));
    let mut phase = use_signal(|| Phase::Idle);
    let mut talking = use_signal(|| false);
    let mut level = use_signal(|| 0f32);
    let mut bubble = use_signal(|| None::<Bubble>);
    let mut history = use_signal(Vec::<DieterTurn>::new);
    let mut settings = use_signal(|| None::<VoiceSettings>);
    let mut models = use_signal(|| None::<Vec<SttModel>>);
    let mut speaking = use_signal(|| false);
    // Aktuelle Werte für die Verarbeitung (die Aufgabe lebt länger als ein Rendern)
    let mut project = use_signal(|| project_id);
    let mut add_ok = use_signal(|| can_add);
    let mut on_canvas = use_signal(|| canvas);
    use_effect(use_reactive!(|(project_id, can_add, canvas)| {
        project.set(project_id);
        add_ok.set(can_add);
        on_canvas.set(canvas);
    }));

    use_drop(mic::stop);

    let load = move || async move {
        if let Ok(s) = api::get::<VoiceSettings>("/voice/settings").await {
            settings.set(Some(s));
        }
        if let Ok(m) = api::get::<Vec<SttModel>>("/voice/models").await {
            models.set(Some(m));
        }
    };
    use_future(move || async move { load().await });

    let set_awake = move |v: bool| {
        let mut awake = awake;
        awake.set(v);
        util::storage_set(AWAKE_KEY, if v { "1" } else { "0" });
    };

    // Vorlesen (wenn eingeschaltet); hört währenddessen nicht zu
    let say = move |text: String| {
        let Some(s) = settings.peek().clone() else { return };
        if !s.speak || text.trim().is_empty() {
            return;
        }
        let mut speaking = speaking;
        speaking.set(true);
        mic::speak(&text, &speech_lang(&s.language), &s.voice, move || speaking.set(false));
    };

    // Einen erkannten Satz verarbeiten
    let handle = move |pcm: Vec<f32>| async move {
        phase.set(Phase::Hearing);
        let canvas = *on_canvas.peek();
        let names: Vec<String> = if canvas { crate::pages::canvas::dieter_names() } else { panes().into_iter().map(|(_, n)| n).collect() };
        let path = format!("/voice/transcribe?names={}", api::enc(&names.join(",")));
        let text = match api::post_bytes::<Transcription>(&path, &mic::wav(&pcm), "audio/wav").await {
            Ok(t) => t.text.trim().to_string(),
            Err(e) => {
                bubble.set(Some(Bubble { note: e.message, ..Default::default() }));
                phase.set(Phase::Idle);
                return;
            }
        };
        if text.is_empty() {
            phase.set(Phase::Idle);
            return;
        }
        let sw = switch_of(&text);
        if !*awake.peek() {
            if sw == Switch::On {
                set_awake(true);
                let r = tr("Ich bin da. Was soll ich tun?");
                bubble.set(Some(Bubble { heard: text, reply: r.clone(), ..Default::default() }));
                say(r);
            } else {
                bubble.set(Some(Bubble { heard: text, note: tr("Dieter schläft – sag „Dieter, schalte dich an“."), ..Default::default() }));
            }
            phase.set(Phase::Idle);
            return;
        }
        match sw {
            Switch::Off => {
                set_awake(false);
                let r = tr("Okay, ich bin still. Sag „Dieter, schalte dich an“, wenn du mich brauchst.");
                bubble.set(Some(Bubble { heard: text, reply: r.clone(), ..Default::default() }));
                say(r);
                phase.set(Phase::Idle);
                return;
            }
            Switch::On => {
                let r = tr("Ich bin schon da.");
                bubble.set(Some(Bubble { heard: text, reply: r.clone(), ..Default::default() }));
                say(r);
                phase.set(Phase::Idle);
                return;
            }
            Switch::None => {}
        }
        bubble.set(Some(Bubble { heard: text.clone(), ..Default::default() }));
        phase.set(Phase::Thinking);
        let req = if canvas {
            crate::pages::canvas::dieter_request(text.clone(), history.peek().clone())
        } else {
            let list = panes();
            let focused = FOCUS.peek().and_then(|f| list.iter().find(|(k, _)| *k == f).map(|(_, n)| n.clone()));
            DieterRequest {
                text: text.clone(),
                panes: list.iter().map(|(k, n)| DieterPane { name: n.clone(), thread_id: (*k > 0).then_some(*k), ..Default::default() }).collect(),
                focused,
                can_add: *add_ok.peek(),
                history: history.peek().clone(),
                ..Default::default()
            }
        };
        match api::post::<DieterReply, _>("/voice/command", &req).await {
            Ok(r) => {
                let (mut done, mut failed) = (Vec::new(), Vec::new());
                for a in r.actions {
                    let res = if canvas { crate::pages::canvas::dieter_action(a).await } else { run_action(a, *project.peek()).await };
                    match res {
                        Ok(s) => done.push(s),
                        Err(e) if !e.is_empty() => failed.push(e),
                        Err(_) => {}
                    }
                }
                {
                    let mut h = history.write();
                    h.push(DieterTurn { role: "user".into(), text: text.clone() });
                    if !r.reply.is_empty() {
                        h.push(DieterTurn { role: "dieter".into(), text: r.reply.clone() });
                    }
                    let len = h.len();
                    if len > 12 {
                        h.drain(..len - 12);
                    }
                }
                let empty = r.reply.is_empty() && done.is_empty() && failed.is_empty();
                bubble.set(if empty { None } else { Some(Bubble { heard: text, reply: r.reply.clone(), done, failed, note: String::new() }) });
                say(r.reply);
            }
            Err(e) => bubble.set(Some(Bubble { heard: text, note: e.message, ..Default::default() })),
        }
        phase.set(Phase::Idle);
    };

    // Mikrofon an: Ereignisse lesen, Sätze nacheinander verarbeiten
    let mut start = move || {
        if starting() {
            return;
        }
        starting.set(true);
        spawn(async move {
            if settings.peek().is_none() || models.peek().is_none() {
                load().await;
            }
            let chosen = settings.peek().as_ref().map(|s| s.stt_model.clone()).unwrap_or_else(|| "base".into());
            let ready = models.peek().as_ref().is_some_and(|l| l.iter().any(|m| m.id == chosen && m.installed));
            if !ready {
                bubble.set(Some(Bubble { note: "model".into(), ..Default::default() }));
                starting.set(false);
                return;
            }
            let (tx, mut rx) = futures_channel::mpsc::unbounded::<mic::Event>();
            let (utx, mut urx) = futures_channel::mpsc::unbounded::<Vec<f32>>();
            match mic::start(tx).await {
                Ok(()) => {
                    listening.set(true);
                    if bubble.peek().as_ref().is_some_and(|b| !b.note.is_empty()) {
                        bubble.set(None);
                    }
                }
                Err(e) => {
                    bubble.set(Some(Bubble { note: mic_error(&e), ..Default::default() }));
                    starting.set(false);
                    return;
                }
            }
            starting.set(false);
            // Sätze der Reihe nach (während Dieter überlegt, kann man schon weiterreden)
            spawn(async move {
                while let Some(pcm) = urx.next().await {
                    handle(pcm).await;
                }
            });
            let mut smooth = 0f32;
            while let Some(ev) = rx.next().await {
                match ev {
                    mic::Event::Level(x) => {
                        smooth = smooth * 0.6 + x * 0.4;
                        let v = (smooth * 12.0).min(1.0);
                        if (v - *level.peek()).abs() > 0.04 {
                            level.set(v);
                        }
                    }
                    mic::Event::Speaking(b) => talking.set(b),
                    mic::Event::Utterance(pcm) => {
                        let _ = utx.unbounded_send(pcm);
                    }
                }
            }
            listening.set(false);
            talking.set(false);
            level.set(0.0);
        });
    };

    let mut stop = move || {
        mic::stop();
        listening.set(false);
        talking.set(false);
        level.set(0.0);
        speaking.set(false);
    };

    // Modell laden (aus der Sprechblase)
    let download = move |id: String| {
        spawn(async move {
            match api::action::<Vec<SttModel>>(&format!("/voice/models/{id}/download")).await {
                Ok(m) => models.set(Some(m)),
                Err(e) => {
                    toast_error(e);
                    return;
                }
            }
            loop {
                gloo_timers::future::TimeoutFuture::new(1000).await;
                let Ok(m) = api::get::<Vec<SttModel>>("/voice/models").await else { break };
                let entry = m.iter().find(|x| x.id == id).cloned();
                models.set(Some(m));
                match entry {
                    Some(e) if e.installed => {
                        bubble.set(None);
                        start();
                        break;
                    }
                    Some(e) if !e.downloading => {
                        if !e.error.is_empty() {
                            toast_error(e.error);
                        }
                        break;
                    }
                    _ => {}
                }
            }
        });
    };

    let on = listening();
    let ph = phase();
    let is_awake = awake();
    let lvl = level();
    let status = if starting() {
        tr("startet …")
    } else if !on {
        tr("Mikrofon aus")
    } else if speaking() {
        tr("spricht …")
    } else if ph == Phase::Thinking {
        tr("denkt nach …")
    } else if ph == Phase::Hearing {
        tr("versteht …")
    } else if !is_awake {
        tr("schläft – „Dieter, schalte dich an“")
    } else if talking() {
        tr("hört zu …")
    } else {
        tr("hört zu")
    };
    let mic_class = if !on {
        "grid size-8 shrink-0 place-items-center rounded-full bg-muted text-muted-foreground hover:bg-accent hover:text-foreground"
    } else if !is_awake {
        "grid size-8 shrink-0 place-items-center rounded-full bg-muted-foreground/30 text-foreground"
    } else {
        "relative grid size-8 shrink-0 place-items-center rounded-full bg-primary text-primary-foreground"
    };
    let chosen_model = settings().map(|s| s.stt_model).unwrap_or_else(|| "base".into());
    let model_entry = models().and_then(|l| l.into_iter().find(|m| m.id == chosen_model));

    rsx! {
        div { class: "relative flex shrink-0 justify-center pt-0.5",
            if let Some(b) = bubble() {
                div { class: "nrz-pane absolute bottom-full z-30 mb-2 w-[min(34rem,calc(100%-1rem))] rounded-2xl border bg-popover/95 p-3 text-sm text-popover-foreground shadow-xl backdrop-blur",
                    button {
                        class: "absolute top-2 right-2 grid size-6 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-foreground",
                        title: tr("Schliessen"),
                        onclick: move |_| bubble.set(None),
                        icons::X { class: "size-3.5" }
                    }
                    if b.note == "model" {
                        div { class: "flex flex-col gap-2 pr-6",
                            p { class: "font-medium", {tr("Dieter braucht einmalig ein Sprachmodell")} }
                            if let Some(m) = model_entry.clone() {
                                p { class: "text-xs text-muted-foreground",
                                    {trf("{0} ({1}) erkennt deine Sprache lokal auf diesem Rechner – nichts geht dafür ins Internet.", &[&m.label, &util::size(m.size)])}
                                }
                                if m.downloading {
                                    div { class: "h-1.5 overflow-hidden rounded-full bg-muted",
                                        div { class: "h-full rounded-full bg-primary transition-all", style: "width: {(m.progress * 100.0) as u32}%" }
                                    }
                                    span { class: "text-xs text-muted-foreground", {trf("Wird geladen … {0} %", &[&((m.progress * 100.0) as u32)])} }
                                } else {
                                    div { class: "flex items-center gap-2",
                                        button {
                                            class: "inline-flex h-8 items-center gap-1.5 rounded-md bg-primary px-3 text-xs font-medium text-primary-foreground hover:bg-primary/90",
                                            onclick: {
                                                let id = m.id.clone();
                                                move |_| download(id.clone())
                                            },
                                            icons::Download { class: "size-3.5" }
                                            {tr("Jetzt laden")}
                                        }
                                        Link { to: Route::Settings { section: "dieter".into() }, class: "text-xs text-muted-foreground underline-offset-2 hover:underline", {tr("Anderes Modell wählen")} }
                                    }
                                    if !m.error.is_empty() {
                                        span { class: "text-xs text-destructive", "{m.error}" }
                                    }
                                }
                            }
                        }
                    } else {
                        div { class: "flex flex-col gap-1.5 pr-6",
                            if !b.heard.is_empty() {
                                p { class: "text-xs text-muted-foreground", span { class: "font-medium", {tr("Du:")} } " „{b.heard}“" }
                            }
                            if !b.reply.is_empty() {
                                p { span { class: "font-semibold", "Dieter: " } "{b.reply}" }
                            }
                            if !b.note.is_empty() {
                                p { class: "text-xs text-muted-foreground", "{b.note}" }
                            }
                            if !b.done.is_empty() || !b.failed.is_empty() {
                                div { class: "flex flex-wrap gap-1",
                                    for d in b.done.iter() {
                                        span { class: "inline-flex items-center gap-1 rounded-full bg-success/10 px-2 py-0.5 text-[11px] text-success", icons::Check { class: "size-3" } "{d}" }
                                    }
                                    for f in b.failed.iter() {
                                        span { class: "inline-flex items-center gap-1 rounded-full bg-destructive/10 px-2 py-0.5 text-[11px] text-destructive", icons::X { class: "size-3" } "{f}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "flex h-11 items-center gap-2 rounded-full border bg-card/90 py-1 pr-2 pl-1.5 shadow-sm",
                button {
                    class: mic_class,
                    title: if on { tr("Mikrofon aus") } else { tr("Mikrofon an – Dieter hört zu") },
                    onclick: move |_| if listening() { stop() } else { start() },
                    if on && is_awake && talking() {
                        span { class: "absolute inset-0 animate-ping rounded-full bg-primary/40" }
                    }
                    if starting() {
                        icons::LoaderCircle { class: "size-4 animate-spin" }
                    } else if on {
                        icons::Mic { class: "relative size-4" }
                    } else {
                        icons::MicOff { class: "size-4" }
                    }
                }
                div { class: "flex min-w-0 flex-col leading-tight",
                    span { class: "text-sm font-semibold", "Dieter" }
                    span { class: "max-w-[16rem] truncate text-[11px] text-muted-foreground", "{status}" }
                }
                // Pegel: drei Balken
                if on {
                    div { class: "flex h-5 items-end gap-0.5", title: tr("Pegel des Mikrofons"),
                        for (i , f) in [0.6f32, 1.0, 0.75].iter().enumerate() {
                            span {
                                key: "{i}",
                                class: if is_awake { "w-1 rounded-full bg-primary/80 transition-all" } else { "w-1 rounded-full bg-muted-foreground/50 transition-all" },
                                style: "height: {(4.0 + lvl * f * 16.0) as u32}px",
                            }
                        }
                    }
                }
                if ph != Phase::Idle {
                    icons::LoaderCircle { class: "size-3.5 animate-spin text-muted-foreground" }
                }
                button {
                    class: "grid size-7 place-items-center rounded-full text-muted-foreground hover:bg-accent hover:text-foreground",
                    title: if is_awake { tr("Dieter schlafen legen (oder sag „Dieter, schalte dich ab“)") } else { tr("Dieter aufwecken (oder sag „Dieter, schalte dich an“)") },
                    onclick: move |_| set_awake(!awake()),
                    if is_awake {
                        icons::Sun { class: "size-3.5" }
                    } else {
                        icons::Moon { class: "size-3.5" }
                    }
                }
                Link {
                    to: Route::Settings { section: "dieter".into() },
                    class: "grid size-7 place-items-center rounded-full text-muted-foreground hover:bg-accent hover:text-foreground",
                    title: tr("Einstellungen von Dieter"),
                    icons::Settings2 { class: "size-3.5" }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Einstellungen

/// Einstellungen → Dieter: Spracherkennung (Whisper-Modell, Sprache), Dieters Agent/Konto/Modell,
/// Vorlesen.
#[component]
pub fn DieterSettings() -> Element {
    let agents = crate::pages::agents::use_agents();
    let mut settings = use_signal(|| None::<VoiceSettings>);
    let mut models = use_signal(|| None::<Vec<SttModel>>);
    let admin = ME.read().as_ref().is_some_and(|m| m.is_admin);
    use_future(move || async move {
        match api::get::<VoiceSettings>("/voice/settings").await {
            Ok(s) => settings.set(Some(s)),
            Err(e) => toast_error(e),
        }
        // Liste laufend halten, solange ein Modell geladen wird
        loop {
            if let Ok(m) = api::get::<Vec<SttModel>>("/voice/models").await {
                let busy = m.iter().any(|x| x.downloading);
                models.set(Some(m));
                if !busy {
                    gloo_timers::future::TimeoutFuture::new(5000).await;
                    continue;
                }
            }
            gloo_timers::future::TimeoutFuture::new(1000).await;
        }
    });
    let mut save = move |s: VoiceSettings| {
        settings.set(Some(s.clone()));
        spawn(async move {
            match api::put::<VoiceSettings, _>("/voice/settings", &s).await {
                Ok(s) => settings.set(Some(s)),
                Err(e) => toast_error(e),
            }
        });
    };
    let Some(s) = settings() else {
        return rsx! { Loading {} };
    };
    let agents = agents.unwrap_or_default();
    let usable: Vec<_> = agents.iter().filter(|a| a.installed && !a.accounts.is_empty()).cloned().collect();
    let current = s.agent.and_then(|k| agents.iter().find(|a| a.kind == k).cloned());
    rsx! {
        Section { title: tr("Dieter – Sprachsteuerung"), description: tr("Im Raster unten in der Mitte: Mikrofon an und sprechen. Dieter verteilt Aufträge an die Fenster (Lena, Benni …) und schreibt daraus gute Prompts."),
            div { class: "grid max-w-2xl gap-2 text-sm",
                Example { text: tr("„Sag Lena bitte, sie soll eine Webseite programmieren, und Bernd soll die Tests reparieren.“") }
                Example { text: tr("„Wie ist der Stand?“ · „Was macht Benni gerade?“ · „Stopp Bernd.“ · „Zeig Lena gross.“") }
                Example { text: tr("„Dieter, schalte dich ab“ – er schläft und reagiert nur noch auf „Dieter, schalte dich an“.") }
            }
        }
        Section { title: tr("Mikrofon"), description: tr("Welches Mikrofon Dieter benutzt – gespeichert für dieses Gerät. Mit „Testen“ siehst du, ob es dich hört."),
            MicPicker {}
        }
        Section { title: tr("Spracherkennung"), description: tr("Whisper läuft lokal auf diesem Rechner (einmal laden, danach ohne Internet). Kleinere Modelle sind schneller, grössere genauer."),
            div { class: "grid max-w-2xl gap-2",
                for m in models().unwrap_or_default() {
                    div {
                        key: "{m.id}",
                        class: if m.id == s.stt_model { "flex items-center gap-3 rounded-lg border border-ring bg-accent/60 px-4 py-2.5" } else { "flex items-center gap-3 rounded-lg border px-4 py-2.5" },
                        button {
                            class: "flex min-w-0 flex-1 items-start gap-2.5 text-left",
                            onclick: {
                                let mut n = s.clone();
                                let id = m.id.clone();
                                move |_| {
                                    n.stt_model = id.clone();
                                    save(n.clone());
                                }
                            },
                            span { class: if m.id == s.stt_model { "mt-1 size-3 shrink-0 rounded-full border-4 border-primary" } else { "mt-1 size-3 shrink-0 rounded-full border" } }
                            span { class: "min-w-0",
                                span { class: "block text-sm font-medium", "{m.label} · " {util::size(m.size)} }
                                span { class: "block text-xs text-muted-foreground", {tr(&m.detail)} }
                                if !m.error.is_empty() {
                                    span { class: "block text-xs text-destructive", "{m.error}" }
                                }
                            }
                        }
                        if m.installed {
                            span { class: "inline-flex items-center gap-1 text-xs text-success", icons::Check { class: "size-3.5" } {tr("Geladen")} }
                            if admin {
                                button {
                                    class: "grid size-7 place-items-center rounded text-muted-foreground hover:bg-accent hover:text-destructive",
                                    title: tr("Modell löschen"),
                                    onclick: {
                                        let id = m.id.clone();
                                        move |_| {
                                            let id = id.clone();
                                            spawn(async move {
                                                match api::send::<Vec<SttModel>, _>("DELETE", &format!("/voice/models/{id}"), &()).await {
                                                    Ok(l) => models.set(Some(l)),
                                                    Err(e) => toast_error(e),
                                                }
                                            });
                                        }
                                    },
                                    icons::Trash2 { class: "size-3.5" }
                                }
                            }
                        } else if m.downloading {
                            div { class: "flex w-32 flex-col gap-1",
                                div { class: "h-1.5 overflow-hidden rounded-full bg-muted",
                                    div { class: "h-full rounded-full bg-primary transition-all", style: "width: {(m.progress * 100.0) as u32}%" }
                                }
                                span { class: "text-right text-[11px] text-muted-foreground", "{(m.progress * 100.0) as u32} %" }
                            }
                        } else {
                            button {
                                class: "inline-flex h-8 items-center gap-1.5 rounded-md border px-2.5 text-xs hover:bg-accent",
                                onclick: {
                                    let id = m.id.clone();
                                    move |_| {
                                        let id = id.clone();
                                        spawn(async move {
                                            match api::action::<Vec<SttModel>>(&format!("/voice/models/{id}/download")).await {
                                                Ok(l) => models.set(Some(l)),
                                                Err(e) => toast_error(e),
                                            }
                                        });
                                    }
                                },
                                icons::Download { class: "size-3.5" }
                                {tr("Laden")}
                            }
                        }
                    }
                }
            }
            div { class: "mt-4 flex max-w-2xl flex-wrap items-center gap-2",
                span { class: "mr-1 text-sm text-muted-foreground", {tr("Sprache:")} }
                for (code , label) in [("de", tr("Deutsch")), ("en", tr("Englisch")), ("", tr("Automatisch"))] {
                    button {
                        key: "{code}",
                        class: if s.language == code { "rounded-full bg-primary px-3 py-1 text-xs text-primary-foreground" } else { "rounded-full border px-3 py-1 text-xs hover:bg-accent" },
                        onclick: {
                            let mut n = s.clone();
                            move |_| {
                                n.language = code.to_string();
                                save(n.clone());
                            }
                        },
                        "{label}"
                    }
                }
            }
        }
        Section { title: tr("Dieters Modell"), description: tr("Dieser Agent versteht deine Sätze und schreibt die Prompts für die Fenster. Ein schnelles Modell antwortet am zügigsten (bei Claude: Haiku)."),
            div { class: "grid max-w-2xl gap-3 sm:grid-cols-3",
                label { class: "grid gap-1 text-xs text-muted-foreground",
                    {tr("Agent")}
                    select {
                        class: "{INPUT} h-9 text-sm text-foreground",
                        onchange: {
                            let s = s.clone();
                            move |e: FormEvent| {
                                let mut n = s.clone();
                                n.agent = AgentKind::parse(&e.value());
                                n.account_id = None;
                                n.model = String::new();
                                save(n);
                            }
                        },
                        option { value: "", selected: s.agent.is_none(), {tr("Automatisch (schnellster verfügbarer)")} }
                        for a in usable.iter() {
                            option { key: "{a.kind.key()}", value: "{a.kind.key()}", selected: s.agent == Some(a.kind), "{a.name}" }
                        }
                    }
                }
                label { class: "grid gap-1 text-xs text-muted-foreground",
                    {tr("Konto")}
                    select {
                        class: "{INPUT} h-9 text-sm text-foreground",
                        disabled: current.is_none(),
                        onchange: {
                            let s = s.clone();
                            move |e: FormEvent| {
                                let mut n = s.clone();
                                n.account_id = e.value().parse::<i64>().ok();
                                save(n);
                            }
                        },
                        option { value: "", selected: s.account_id.is_none(), {tr("Standardkonto")} }
                        if let Some(c) = current.clone() {
                            for acc in c.accounts.iter() {
                                option { key: "{acc.id}", value: "{acc.id}", selected: s.account_id == Some(acc.id), "{acc.label}" }
                            }
                        }
                    }
                }
                label { class: "grid gap-1 text-xs text-muted-foreground",
                    {tr("Modell")}
                    select {
                        class: "{INPUT} h-9 text-sm text-foreground",
                        disabled: current.is_none(),
                        onchange: {
                            let s = s.clone();
                            move |e: FormEvent| {
                                let mut n = s.clone();
                                n.model = e.value();
                                save(n);
                            }
                        },
                        option { value: "", selected: s.model.is_empty(), {tr("Schnell (Vorgabe)")} }
                        if let Some(c) = current.clone() {
                            for m in c.models.iter() {
                                option { key: "{m.id}", value: "{m.id}", selected: s.model == m.id, {tr(&m.label)} }
                            }
                            if !s.model.is_empty() && !c.models.iter().any(|m| m.id == s.model) {
                                option { value: "{s.model}", selected: true, "{s.model}" }
                            }
                        }
                    }
                }
            }
            if usable.is_empty() {
                div { class: "mt-3 max-w-2xl",
                    Callout { text: tr("Noch kein Agent mit Konto – unter Einstellungen → Agenten einrichten.") }
                }
            }
        }
        Section { title: tr("Stimme"), description: tr("Dieter liest seine Antworten mit der Sprachausgabe des Systems vor – mit einer männlichen Stimme. Solange er spricht, hört er nicht zu."),
            div { class: "grid max-w-2xl gap-3",
                Toggle {
                    checked: s.speak,
                    onchange: {
                        let s = s.clone();
                        move |v: bool| {
                            let mut n = s.clone();
                            n.speak = v;
                            save(n);
                        }
                    },
                    label: tr("Antworten vorlesen"),
                }
                VoicePicker { settings: s.clone(), onsave: move |n: VoiceSettings| save(n) }
            }
        }
    }
}

/// Mikrofon für Dieter (je Gerät): Standard des Systems oder ein bestimmtes; Test mit Pegel.
/// An- und abgesteckte Geräte erscheinen von selbst.
#[component]
fn MicPicker() -> Element {
    // None = lädt noch
    let mut list = use_signal(|| None::<Vec<mic::Input>>);
    let mut chosen = use_signal(mic::chosen);
    let mut note = use_signal(String::new);
    let mut testing = use_signal(|| false);
    let mut level = use_signal(|| 0f32);
    let mut device = use_signal(String::new);
    let refresh = move |unlock: bool| async move {
        match mic::inputs(unlock).await {
            Ok(l) => {
                list.set(Some(l));
                note.set(String::new());
            }
            Err(e) => {
                list.set(Some(Vec::new()));
                note.set(mic_error(&e));
            }
        }
    };
    use_future(move || async move {
        let watch = mic::watch_devices();
        refresh(false).await;
        if let Some((_watch, mut changes)) = watch {
            while changes.next().await.is_some() {
                refresh(false).await;
            }
        }
    });
    use_drop(mic::probe_stop);
    // Test: gewähltes Mikrofon öffnen und den Pegel zeigen, bis „Test beenden“
    let start_test = move || {
        spawn(async move {
            match mic::probe_start().await {
                Ok(name) => {
                    device.set(name);
                    note.set(String::new());
                    if !*testing.peek() {
                        testing.set(true);
                        // Erst jetzt kennt der Browser die Namen
                        refresh(false).await;
                        let mut smooth = 0f32;
                        while *testing.peek() {
                            smooth = smooth * 0.6 + mic::probe_level() * 0.4;
                            let v = (smooth * 12.0).min(1.0);
                            if (v - *level.peek()).abs() > 0.02 {
                                level.set(v);
                            }
                            gloo_timers::future::TimeoutFuture::new(60).await;
                        }
                    }
                }
                Err(e) => {
                    mic::probe_stop();
                    testing.set(false);
                    note.set(mic_error(&e));
                }
            }
        });
    };
    let toggle = move |_| {
        if testing() {
            mic::probe_stop();
            testing.set(false);
            level.set(0.0);
        } else {
            start_test();
        }
    };
    let pick = move |e: FormEvent| {
        let id = e.value();
        let label = list.peek().as_ref().and_then(|l| l.iter().find(|d| d.id == id).map(|d| d.label.clone()));
        let label = label.unwrap_or_else(|| if chosen.peek().0 == id { chosen.peek().1.clone() } else { String::new() });
        chosen.set((id.clone(), label.clone()));
        spawn(async move {
            // Hört Dieter gerade zu, wechselt er sofort
            if let Err(e) = mic::choose(&id, &label).await {
                note.set(mic_error(&e));
            }
            if *testing.peek() {
                start_test();
            }
        });
    };
    let (cur, cur_label) = chosen();
    let loading = list().is_none();
    let devices = list().unwrap_or_default();
    // Gewähltes Gerät nicht in der Liste: abgesteckt (Liste da) oder Namen noch unbekannt
    let extra = !cur.is_empty() && !devices.iter().any(|d| d.id == cur);
    let missing = extra && !devices.is_empty();
    let extra_label = if cur_label.is_empty() { tr("Gewähltes Mikrofon") } else { cur_label.clone() };
    let unnamed = !loading && (devices.is_empty() || devices.iter().any(|d| d.label.is_empty()));
    let bar = format!("width: {:.0}%", level() * 100.0);
    rsx! {
        div { class: "grid max-w-2xl gap-2",
            div { class: "flex flex-wrap items-end gap-2",
                label { class: "grid min-w-64 flex-1 gap-1 text-xs text-muted-foreground",
                    {tr("Mikrofon")}
                    select { class: "{INPUT} h-9 text-sm text-foreground", disabled: loading, onchange: pick,
                        option { value: "", selected: cur.is_empty(), {tr("Standard des Systems")} }
                        for (i , d) in devices.iter().enumerate() {
                            option { key: "{d.id}", value: "{d.id}", selected: d.id == cur,
                                if d.label.is_empty() {
                                    {trf("Mikrofon {0}", &[&(i + 1)])}
                                } else {
                                    "{d.label}"
                                }
                            }
                        }
                        if extra {
                            option { value: "{cur}", selected: true,
                                if missing {
                                    {trf("{0} (nicht verbunden)", &[&extra_label])}
                                } else {
                                    "{extra_label}"
                                }
                            }
                        }
                    }
                }
                button { class: "inline-flex h-9 items-center gap-1.5 rounded-md border px-3 text-sm hover:bg-accent", onclick: toggle,
                    if testing() {
                        icons::MicOff { class: "size-4" }
                        {tr("Test beenden")}
                    } else {
                        icons::Mic { class: "size-4" }
                        {tr("Testen")}
                    }
                }
            }
            if testing() {
                div { class: "grid gap-1.5",
                    div { class: "h-2 overflow-hidden rounded-full bg-muted",
                        div { class: "h-full rounded-full bg-success transition-[width] duration-75", style: "{bar}" }
                    }
                    p { class: "text-xs text-muted-foreground",
                        if device().is_empty() {
                            {tr("Sprich etwas – der Balken sollte ausschlagen.")}
                        } else {
                            {trf("Sprich etwas – der Balken sollte ausschlagen. Gerät: {0}", &[&device()])}
                        }
                    }
                }
            }
            if missing {
                p { class: "text-xs text-muted-foreground", {tr("Nicht verbunden – solange nimmt Dieter den Standard des Systems.")} }
            }
            if unnamed {
                div { class: "flex flex-wrap items-center gap-2 text-xs text-muted-foreground",
                    {tr("Die Namen der Mikrofone zeigt der Browser erst nach der Erlaubnis.")}
                    button {
                        class: "rounded-md border px-2 py-1 text-foreground hover:bg-accent",
                        onclick: move |_| {
                            spawn(refresh(true));
                        },
                        {tr("Mikrofone anzeigen")}
                    }
                }
            }
            if !note().is_empty() {
                p { class: "text-xs text-destructive", "{note}" }
            }
        }
    }
}

/// Stimme fürs Vorlesen: automatisch eine männliche oder eine bestimmte Systemstimme; Probe.
#[component]
fn VoicePicker(settings: VoiceSettings, onsave: EventHandler<VoiceSettings>) -> Element {
    let lang = speech_lang(&settings.language);
    let base = lang.split('-').next().unwrap_or("de").to_string();
    // Stimmen lädt der Browser manchmal erst nach – kurz nachfragen
    let mut list = use_signal(Vec::<(String, String, bool)>::new);
    {
        let base = base.clone();
        use_future(move || {
            let base = base.clone();
            async move {
                for _ in 0..20 {
                    let l = mic::voices(&base);
                    if !l.is_empty() {
                        list.set(l);
                        break;
                    }
                    gloo_timers::future::TimeoutFuture::new(250).await;
                }
            }
        });
    }
    let auto = mic::auto_voice(&lang);
    let current = settings.voice.clone();
    let l = list();
    let probe = {
        let lang = lang.clone();
        let current = current.clone();
        move |_| {
            let text = if lang.starts_with("de") { "Hallo, ich bin Dieter. Sag mir, was die Fenster tun sollen." } else { "Hi, I am Dieter. Tell me what the windows should do." };
            mic::speak(text, &lang, &current, || {});
        }
    };
    rsx! {
        div { class: "flex flex-wrap items-end gap-2",
            label { class: "grid min-w-64 flex-1 gap-1 text-xs text-muted-foreground",
                {tr("Stimme")}
                select {
                    class: "{INPUT} h-9 text-sm text-foreground",
                    onchange: {
                        let s = settings.clone();
                        move |e: FormEvent| {
                            let mut n = s.clone();
                            n.voice = e.value();
                            onsave.call(n);
                        }
                    },
                    option { value: "", selected: current.is_empty(),
                        match auto.clone() {
                            Some(v) => trf("Automatisch – männlich ({0})", &[&v]),
                            None => tr("Automatisch – tiefer gestimmt (keine männliche Stimme installiert)"),
                        }
                    }
                    for (name , code , male) in l.iter().cloned() {
                        option { key: "{name}", value: "{name}", selected: current == name,
                            if male { "♂ {name} · {code}" } else { "{name} · {code}" }
                        }
                    }
                }
            }
            button { class: "inline-flex h-9 items-center gap-1.5 rounded-md border px-3 text-sm hover:bg-accent", onclick: probe,
                icons::Volume2 { class: "size-4" }
                {tr("Probe hören")}
            }
        }
        if auto.is_none() && current.is_empty() && !l.is_empty() {
            p { class: "mt-2 max-w-2xl text-xs text-muted-foreground",
                {tr("Tipp (macOS): Unter Systemeinstellungen → Bedienungshilfen → Gesprochene Inhalte → Systemstimme → Stimmen verwalten eine männliche Stimme wie „Markus“ oder „Yannick“ laden.")}
            }
        }
    }
}

#[component]
fn Example(text: String) -> Element {
    rsx! {
        div { class: "flex items-start gap-2 rounded-lg border bg-card/50 px-3 py-2",
            icons::AudioLines { class: "mt-0.5 size-4 shrink-0 text-muted-foreground" }
            span { "{text}" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_switches() {
        assert_eq!(switch_of("Dieter, schalte dich ab."), Switch::Off);
        assert_eq!(switch_of("Dieter schaltet dich ab"), Switch::Off);
        assert_eq!(switch_of("Dieter, schalte dich an!"), Switch::On);
        assert_eq!(switch_of("Dieta, wach auf"), Switch::On);
        assert_eq!(switch_of("Dieter aus"), Switch::Off);
        assert_eq!(switch_of("Dieter, bist du da?"), Switch::On);
        assert_eq!(switch_of("Sag Lena, sie soll die Seite an die API anbinden"), Switch::None);
        assert_eq!(switch_of("Dieter, sag Lena sie soll eine Webseite mit Login und Datenbank bauen und das Design an"), Switch::None);
        assert_eq!(switch_of("schalte dich ab"), Switch::None);
    }
}
