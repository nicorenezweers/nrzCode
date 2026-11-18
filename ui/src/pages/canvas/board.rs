//! Die Fläche: unendlich, mit Punktraster. Verschieben mit gedrückter Maus (Hand bzw. auf freier
//! Fläche), zwei Fingern oder Mausrad; Zoomen mit ⌘/Strg + Rad oder Pinch. Darauf die Zeichnung
//! (SVG) und die Fenster. Während einer Zieh-Aktion fängt eine Ebene über allem die Bewegungen
//! (auch über Browser-Fenstern und Terminals); der Zeiger ist am Element gefangen und die
//! Bewegungen kommen über `window` – höchstens einmal je Bild (`requestAnimationFrame`).
//! Verschieben der Fläche und Ziehen eines Fensters gehen direkt ins DOM, ohne dass Dioxus neu
//! zeichnet; übernommen wird beim Loslassen.

use super::draw::*;
use super::store::*;
use super::window::WindowView;
use crate::components::common::Loading;
use crate::i18n::tr;
use crate::state::{self, THREADS};
use dioxus::core::spawn_forever;
use dioxus::prelude::*;
use nrzcode_shared::canvas::CanvasShape;
use std::cell::{Cell, RefCell};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

thread_local! {
    static INSTALLED: Cell<bool> = const { Cell::new(false) };
    /// Pinch in Safari/WebKit: Zoom beim Beginn der Geste.
    static GESTURE_Z: Cell<f64> = const { Cell::new(1.0) };
    /// Lage des gezogenen Fensters (nur im DOM, in die Fläche beim Loslassen).
    static LIVE: RefCell<Option<LiveWin>> = const { RefCell::new(None) };
    /// Letzte Zeigerbewegung, die noch aufs nächste Bild wartet: (x, y, Shift).
    static PENDING_MOVE: Cell<Option<(f64, f64, bool)>> = const { Cell::new(None) };
    static FRAME: Cell<bool> = const { Cell::new(false) };
    static FRAME_CB: RefCell<Option<Closure<dyn FnMut(f64)>>> = const { RefCell::new(None) };
}

/// Zeiger am Ziel des Ereignisses fangen: Bewegungen kommen weiter an, auch wenn er über ein
/// Browser-Fenster (iframe) oder aus dem Fenster gleitet.
pub fn capture_pointer(e: &PointerEvent) {
    let id = e.pointer_id();
    let target = e.data().downcast::<web_sys::PointerEvent>().and_then(|ev| ev.target()).or_else(|| e.data().downcast::<web_sys::Event>().and_then(|ev| ev.target()));
    if let Some(el) = target.and_then(|t| t.dyn_into::<web_sys::Element>().ok()) {
        let _ = el.set_pointer_capture(id);
    }
}

pub const VIEW_ID: &str = "nrz-canvas";

#[component]
pub fn Board() -> Element {
    // Arbeitsbereich wählen bzw. Fläche laden
    use_effect(|| {
        let _ = state::PROJECTS.read();
        ensure_ws();
    });
    use_effect(|| {
        let key = WS();
        if key >= 0 {
            spawn_forever(load(key));
        }
    });
    // Fenster passend zu den Threads (auch während des Chat-Modus entstandene)
    use_effect(sync_threads);
    use_hook(install_listeners);
    // Der Rahmen hängt nur an der Kamera und an wenigen Merkmalen (als Memo): Er zeichnet sich nicht
    // bei jeder Änderung der Zeichnung oder jeder Zeigerbewegung neu, der Inhalt nicht beim
    // Verschieben und Zoomen ([`WorldContent`]).
    let loaded = use_memo(|| BOARD.read().is_some());
    let empty = use_memo(|| BOARD.read().as_ref().is_some_and(|b| b.windows.is_empty() && b.shapes.is_empty()));
    let drag_cursor_now = use_memo(|| DRAG.read().as_ref().map(drag_cursor));
    let panning = use_memo(|| matches!(DRAG.read().as_ref(), Some(Drag::Pan { .. })));

    // Während des Verschiebens steht die Kamera schon im DOM ([`preview_cam`])
    let _ = CAM();
    let cam = cam_now();
    if !loaded() {
        return rsx! {
            div { class: "absolute inset-0 grid place-items-center", Loading {} }
        };
    }
    let tool = TOOL();
    let grid = grid_size(cam.z);
    let anim = if ANIM() { "transition: transform 380ms cubic-bezier(.2,.8,.2,1), background-position 380ms cubic-bezier(.2,.8,.2,1), background-size 380ms;" } else { "" };
    // Beim Ziehen eine eigene Ebene (nur verschieben, nicht neu malen); danach wieder scharf
    let will = if panning() { "will-change: transform;" } else { "" };
    let cursor = match tool {
        Tool::Hand => "cursor-grab",
        Tool::Select => "cursor-default",
        Tool::Text => "cursor-text",
        Tool::Eraser => "cursor-cell",
        _ => "cursor-crosshair",
    };
    let empty = empty();

    rsx! {
        div {
            id: VIEW_ID,
            class: "absolute inset-0 touch-none select-none overflow-hidden {cursor}",
            style: "background-color: var(--background); background-image: radial-gradient(circle at 1px 1px, color-mix(in oklch, var(--foreground) 16%, transparent) 1px, transparent 0); background-size: {grid}px {grid}px; background-position: {cam.x}px {cam.y}px; {anim}",
            onpointerdown: on_down,
            // Text/Notiz: Der Editor ist nach `pointerdown` schon da und fokussiert – das folgende
            // `mousedown` desselben Klicks würde ihm den Fokus nehmen (leere Notiz = verworfen).
            onmousedown: move |e: MouseEvent| {
                let target = e.data().downcast::<web_sys::Event>().and_then(|ev| ev.target());
                if EDITING.peek().is_some() && !in_window(&e) && !typing(target) {
                    e.prevent_default();
                }
            },
            ondoubleclick: move |e: MouseEvent| {
                if !in_window(&e) {
                    on_double(e);
                }
            },
            oncontextmenu: move |e: MouseEvent| {
                // In Fenstern bleibt das normale Menü (Kopieren, Einfügen …)
                if in_window(&e) {
                    return;
                }
                e.prevent_default();
                // Rechte Maustaste: Ziehen verschiebt die Fläche, das Menü kommt beim Loslassen
                // ohne Bewegung (on_up). Sonst (z. B. Ctrl-Klick am Mac) gleich hier.
                if matches!(DRAG.peek().as_ref(), Some(Drag::Pan { right: true, .. })) {
                    return;
                }
                end_drag();
                let p = e.client_coordinates();
                *MENU.write() = Some((p.x, p.y));
            },
            // Welt: Zeichnung unter den Fenstern, Text und Notizen darüber
            div {
                id: WORLD_ID,
                class: "absolute top-0 left-0 origin-top-left",
                style: "transform: translate3d({cam.x}px, {cam.y}px, 0) scale({cam.z}); {anim} {will}",
                WorldContent { compact: cam.z < super::window::CARD_BELOW }
            }
            if empty {
                div { class: "pointer-events-none absolute inset-0 grid place-items-center",
                    div { class: "max-w-sm text-center text-muted-foreground",
                        icons::Frame { class: "mx-auto mb-3 size-8 opacity-60" }
                        p { class: "text-base font-medium text-foreground", {tr("Leere Canvas")} }
                        p { class: "mt-1 text-sm", {tr("Links einen Chat, ein Terminal oder einen Browser öffnen – oder einfach Dieter sagen, was zu tun ist.")} }
                    }
                }
            }
        }
        if let Some(cursor) = drag_cursor_now() {
            div {
                class: "fixed inset-0 z-[90] touch-none select-none {cursor}",
                oncontextmenu: move |e: MouseEvent| e.prevent_default(),
            }
        }
        ContextMenu {}
    }
}

/// Inhalt der Fläche: Zeichnung unter den Fenstern, Text und Notizen darüber, die Fenster und die
/// Auswahl. Einzige Eigenschaft ist `compact` (weit herausgezoomt): Verschieben und Zoomen der
/// Kamera zeichnen den Inhalt nicht neu, Zeigerbewegungen beim Zeichnen und Ziehen ebenso wenig
/// ([`DrawPreview`], [`LIVE_SHAPE`]; Fenster beim Ziehen direkt im DOM).
#[component]
fn WorldContent(compact: bool) -> Element {
    let selected = SELECTED();
    let shape_sel = SHAPE_SEL();
    let editing = EDITING();
    let (project_id, windows, strokes, notes, sel_shape) = {
        let guard = BOARD.read();
        let Some(board) = guard.as_ref() else { return rsx! {} };
        let mut windows = board.windows.clone();
        windows.sort_by_key(|w| w.z);
        let (strokes, notes): (Vec<CanvasShape>, Vec<CanvasShape>) = board.shapes.iter().cloned().partition(|s| !matches!(s.kind.as_str(), "text" | "note"));
        let sel_shape = shape_sel.and_then(|id| board.shapes.iter().find(|s| s.id == id).cloned()).filter(|s| editing.as_deref() != Some(s.id.as_str()));
        (board.project_id, windows, strokes, notes, sel_shape)
    };
    let animate = GLIDE();
    rsx! {
        svg { class: "pointer-events-none absolute top-0 left-0 overflow-visible", width: "1", height: "1",
            for s in strokes {
                ShapeSvg { key: "{s.id}", shape: s }
            }
            DrawPreview {}
        }
        for s in notes {
            ShapeNote { key: "{s.id}", editing: editing.as_deref() == Some(s.id.as_str()), shape: s.clone() }
        }
        for w in windows {
            WindowView { key: "{w.id}", selected: selected.as_deref() == Some(w.id.as_str()), compact, project_id, animate, win: w.clone() }
        }
        if let Some(s) = sel_shape {
            Selection { shape: s }
        }
    }
}

/// Vorschau der Form, die gerade gezeichnet wird – nur sie zeichnet sich bei jeder Bewegung neu.
#[component]
fn DrawPreview() -> Element {
    let shape = match DRAG.read().as_ref() {
        Some(Drag::Draw(s)) if !matches!(s.kind.as_str(), "text" | "note") => Some(s.clone()),
        _ => None,
    };
    match shape {
        Some(s) => rsx! { ShapeSvg { shape: s, ghost: true } },
        None => rsx! {},
    }
}

/// Kommt das Ereignis aus einem Fenster (Chat, Terminal, Browser)?
fn in_window(e: &MouseEvent) -> bool {
    e.data()
        .downcast::<web_sys::Event>()
        .and_then(|ev| ev.target())
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .is_some_and(|el| el.closest("[data-cw]").ok().flatten().is_some())
}

fn drag_cursor(d: &Drag) -> &'static str {
    match d {
        Drag::Pan { .. } | Drag::Window { .. } | Drag::Shape { .. } => "cursor-grabbing",
        Drag::Resize { .. } | Drag::ShapeResize { .. } => "cursor-nwse-resize",
        Drag::Erase(_) => "cursor-cell",
        Drag::Draw(_) => "cursor-crosshair",
    }
}

fn world(sx: f64, sy: f64) -> (f64, f64) {
    cam_now().to_world(sx, sy)
}

/// Trefferweite in Flächeneinheiten (etwa 6 Bildschirmpunkte).
fn tol() -> f64 {
    6.0 / cam_now().z
}

/// Zieh-Aktion beenden, ohne etwas zu übernehmen (z. B. Kontextmenü per Ctrl-Klick).
fn end_drag() {
    if DRAG.peek().is_some() {
        *DRAG.write() = None;
    }
    LIVE.with(|l| l.borrow_mut().take());
    PENDING_MOVE.with(|p| p.set(None));
    commit_cam();
    if LIVE_SHAPE.peek().is_some() {
        *LIVE_SHAPE.write() = None;
    }
}

fn on_down(e: PointerEvent) {
    *MENU.write() = None;
    stop_glide();
    // Noch nicht übernommene Kamera (Rad) zuerst übernehmen, sonst springt die Fläche
    commit_cam();
    down(&e);
    if DRAG.peek().is_some() {
        capture_pointer(&e);
    }
}

fn down(e: &PointerEvent) {
    let p = e.client_coordinates();
    let (sx, sy) = (p.x, p.y);
    let button = e.trigger_button();
    use dioxus::html::input_data::MouseButton;
    // Rechte Taste: Fläche verschieben – mit jedem Werkzeug (z. B. beim Zeichnen schnell weiter
    // nach rechts); ohne Bewegung öffnet das Loslassen das Kontextmenü
    if button == Some(MouseButton::Secondary) {
        *DRAG.write() = Some(Drag::Pan { sx, sy, cam: *CAM.peek(), right: true });
        return;
    }
    let tool = *TOOL.peek();
    // Mittlere Taste, Hand oder Leertaste: verschieben
    if button == Some(MouseButton::Auxiliary) || tool == Tool::Hand || SPACE.with(|s| s.get()) {
        *DRAG.write() = Some(Drag::Pan { sx, sy, cam: *CAM.peek(), right: false });
        return;
    }
    if EDITING.peek().is_some() {
        // Erst das Schreiben beenden (blur speichert)
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.active_element()) {
            if let Ok(h) = el.dyn_into::<web_sys::HtmlElement>() {
                let _ = h.blur();
            }
        }
        return;
    }
    let (wx, wy) = world(sx, sy);
    match tool {
        Tool::Select => {
            *SELECTED.write() = None;
            match shape_at((wx, wy), tol()) {
                Some(s) => {
                    *SHAPE_SEL.write() = Some(s.id.clone());
                    *DRAG.write() = Some(Drag::Shape { sx, sy, orig: s, moved: false });
                }
                None => {
                    *SHAPE_SEL.write() = None;
                    *DRAG.write() = Some(Drag::Pan { sx, sy, cam: *CAM.peek(), right: false });
                }
            }
        }
        Tool::Eraser => {
            let mut hit = Vec::new();
            erase_at((wx, wy), &mut hit);
            *DRAG.write() = Some(Drag::Erase(hit));
        }
        Tool::Text | Tool::Note => {
            let note = tool == Tool::Note;
            let mut s = CanvasShape {
                id: new_shape_id(),
                kind: tool.shape_kind().into(),
                x: wx,
                y: wy,
                w: if note { 240.0 } else { 160.0 },
                h: if note { 150.0 } else { 34.0 },
                color: if note { NOTE_COLORS[0].to_string() } else { COLOR.peek().clone() },
                size: *STROKE.peek(),
                author: "user".into(),
                ..Default::default()
            };
            if note {
                s.x -= 20.0;
                s.y -= 20.0;
            } else {
                s.y -= font_size(&s) * 0.7;
            }
            // Erst beim Speichern auf den Server (leer = verworfen)
            with_board(|b| b.shapes.push(s.clone()));
            *SHAPE_SEL.write() = None;
            *EDITING.write() = Some(s.id.clone());
            *TOOL.write() = Tool::Select;
        }
        _ => {
            let kind = tool.shape_kind();
            let points = match kind {
                "pen" => vec![[wx, wy]],
                "line" | "arrow" => vec![[wx, wy], [wx, wy]],
                _ => Vec::new(),
            };
            let s = CanvasShape {
                id: new_shape_id(),
                kind: kind.into(),
                points,
                x: wx,
                y: wy,
                color: COLOR.peek().clone(),
                size: *STROKE.peek(),
                author: "user".into(),
                ..Default::default()
            };
            *SHAPE_SEL.write() = None;
            *DRAG.write() = Some(Drag::Draw(s));
        }
    }
}

/// Doppelklick auf freie Fläche: Text schreiben; auf Text/Notiz: bearbeiten.
fn on_double(e: MouseEvent) {
    if *TOOL.peek() != Tool::Select {
        return;
    }
    let p = e.client_coordinates();
    let (wx, wy) = world(p.x, p.y);
    if let Some(s) = shape_at((wx, wy), tol()) {
        if matches!(s.kind.as_str(), "text" | "note") {
            *SHAPE_SEL.write() = None;
            *EDITING.write() = Some(s.id.clone());
        }
        return;
    }
    let mut s = CanvasShape { id: new_shape_id(), kind: "text".into(), x: wx, y: wy, w: 160.0, h: 34.0, color: COLOR.peek().clone(), size: *STROKE.peek(), author: "user".into(), ..Default::default() };
    s.y -= font_size(&s) * 0.7;
    with_board(|b| b.shapes.push(s.clone()));
    *EDITING.write() = Some(s.id);
}

fn erase_at(p: (f64, f64), hit: &mut Vec<String>) {
    let t = tol() * 1.5;
    let found: Vec<String> = BOARD.peek().as_ref().map(|b| b.shapes.iter().filter(|s| !hit.contains(&s.id) && super::draw::hit(s, p, t)).map(|s| s.id.clone()).collect()).unwrap_or_default();
    if found.is_empty() {
        return;
    }
    if hit.is_empty() {
        remember_shapes();
    }
    with_board(|b| b.shapes.retain(|s| !found.contains(&s.id)));
    hit.extend(found);
}

/// Gezogenes Fenster direkt im DOM verschieben bzw. in der Grösse ändern (ohne Neuzeichnen).
fn show_live(l: &LiveWin, orig: (f64, f64)) {
    let el = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.query_selector(&format!("[data-cw-id=\"{}\"]", l.id)).ok().flatten())
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
    if let Some(el) = el {
        let st = el.style();
        let _ = st.set_property("transition", "none");
        let _ = st.set_property("transform", &format!("translate3d({}px, {}px, 0)", l.x - orig.0, l.y - orig.1));
        let _ = st.set_property("width", &format!("{}px", l.w));
        let _ = st.set_property("height", &format!("{}px", l.h));
    }
}

fn move_to(sx: f64, sy: f64, shift: bool) {
    let Some(d) = DRAG.peek().clone() else { return };
    let z = cam_now().z;
    match d {
        Drag::Pan { sx: x0, sy: y0, cam, .. } => {
            preview_cam(Cam { x: cam.x + sx - x0, y: cam.y + sy - y0, z: cam.z });
        }
        // Fenster: nur das gezogene Fenster folgt dem Zeiger (ohne Runden – gerundet wird beim
        // Loslassen), die Fläche bleibt unberührt
        Drag::Window { id, sx: x0, sy: y0, x, y, w, h } => {
            let l = LiveWin { id, x: x + (sx - x0) / z, y: y + (sy - y0) / z, w, h };
            show_live(&l, (x, y));
            LIVE.with(|v| *v.borrow_mut() = Some(l));
        }
        Drag::Resize { id, sx: x0, sy: y0, x, y, w, h } => {
            let l = LiveWin { id, x, y, w: (w + (sx - x0) / z).clamp(260.0, 4000.0), h: (h + (sy - y0) / z).clamp(160.0, 4000.0) };
            show_live(&l, (x, y));
            LIVE.with(|v| *v.borrow_mut() = Some(l));
        }
        Drag::Draw(mut s) => {
            let (wx, wy) = world(sx, sy);
            match s.kind.as_str() {
                "pen" => {
                    let last = *s.points.last().unwrap_or(&[wx, wy]);
                    if ((last[0] - wx).powi(2) + (last[1] - wy).powi(2)).sqrt() * z >= 2.0 {
                        s.points.push([wx, wy]);
                    }
                }
                "line" | "arrow" => {
                    let a = s.points[0];
                    let (mut bx, mut by) = (wx, wy);
                    if shift {
                        // auf 45° einrasten
                        let ang = (by - a[1]).atan2(bx - a[0]);
                        let snap = (ang / std::f64::consts::FRAC_PI_4).round() * std::f64::consts::FRAC_PI_4;
                        let len = ((bx - a[0]).powi(2) + (by - a[1]).powi(2)).sqrt();
                        bx = a[0] + len * snap.cos();
                        by = a[1] + len * snap.sin();
                    }
                    s.points = vec![a, [bx, by]];
                }
                _ => {
                    let (mut w, mut h) = (wx - s.x, wy - s.y);
                    if shift {
                        let m = w.abs().max(h.abs());
                        w = m * w.signum();
                        h = m * h.signum();
                    }
                    s.w = w;
                    s.h = h;
                }
            }
            *DRAG.write() = Some(Drag::Draw(s));
        }
        Drag::Shape { sx: x0, sy: y0, orig, moved } => {
            let (dx, dy) = ((sx - x0) / z, (sy - y0) / z);
            let mut s = orig.clone();
            s.x += dx;
            s.y += dy;
            for pt in s.points.iter_mut() {
                pt[0] += dx;
                pt[1] += dy;
            }
            *LIVE_SHAPE.write() = Some(s);
            let now = dx.abs() + dy.abs() > 0.5;
            if now != moved {
                *DRAG.write() = Some(Drag::Shape { sx: x0, sy: y0, orig, moved: now });
            }
        }
        Drag::ShapeResize { sx: x0, sy: y0, orig } => {
            let o = normalized(orig.clone());
            let mut s = o.clone();
            s.w = (o.w + (sx - x0) / z).max(24.0);
            s.h = (o.h + (sy - y0) / z).max(18.0);
            *LIVE_SHAPE.write() = Some(s);
        }
        Drag::Erase(mut hit) => {
            erase_at(world(sx, sy), &mut hit);
            *DRAG.write() = Some(Drag::Erase(hit));
        }
    }
}

fn release(px: f64, py: f64) {
    // Letzte Bewegung noch zeigen, dann übernehmen
    if let Some((x, y, shift)) = PENDING_MOVE.with(|p| p.take()) {
        move_to(x, y, shift);
    }
    let Some(d) = DRAG.write().take() else { return };
    match d {
        Drag::Pan { sx, sy, right, .. } => {
            if right && (px - sx).abs() + (py - sy).abs() < 5.0 {
                *MENU.write() = Some((sx, sy));
            }
            commit_cam();
            save_cam();
        }
        Drag::Window { .. } | Drag::Resize { .. } => {
            // Erst jetzt in die Fläche (gerundet) und auf den Server
            let live = LIVE.with(|l| l.borrow_mut().take());
            if let Some(l) = live {
                update_window(&l.id, |w| {
                    w.x = l.x.round();
                    w.y = l.y.round();
                    w.w = l.w.round();
                    w.h = l.h.round();
                });
                schedule_save();
            }
        }
        Drag::Draw(s) => {
            let s = normalized(s);
            let (x0, y0, x1, y1) = bbox(&s);
            let small = (x1 - x0).max(y1 - y0) * CAM.peek().z < 4.0;
            match s.kind.as_str() {
                "pen" if s.points.len() >= 2 => add_shape(s),
                "pen" => {
                    // Ein Punkt: kleiner Strich, damit er sichtbar bleibt
                    let mut s = s;
                    let p = s.points[0];
                    s.points.push([p[0] + 0.5, p[1] + 0.5]);
                    add_shape(s);
                }
                _ if small => {}
                _ => {
                    let id = s.id.clone();
                    add_shape(s);
                    *SHAPE_SEL.write() = Some(id);
                    *TOOL.write() = Tool::Select;
                }
            }
        }
        // Formen: Die Fläche hat noch den Stand vor dem Ziehen (für Rückgängig), jetzt übernehmen
        Drag::Shape { moved, .. } => {
            let live = LIVE_SHAPE.write().take();
            if let (true, Some(s)) = (moved, live) {
                remember_shapes();
                put_shape(s);
            }
        }
        Drag::ShapeResize { .. } => {
            let live = LIVE_SHAPE.write().take();
            if let Some(s) = live {
                remember_shapes();
                put_shape(s);
            }
        }
        Drag::Erase(hit) => {
            if !hit.is_empty() {
                send_shapes(nrzcode_shared::canvas::CanvasShapesChange { delete: hit, ..Default::default() });
            }
        }
    }
}

/// Rechtsklick auf die Fläche: Fenster oder Notiz genau dort.
#[component]
fn ContextMenu() -> Element {
    let Some((sx, sy)) = MENU() else { return rsx! {} };
    let at = world(sx, sy);
    let item = "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-accent [&_svg]:size-4 [&_svg]:text-muted-foreground";
    let open = move |kind: &'static str| {
        *MENU.write() = None;
        if let Some(id) = add_window(kind, None, "", Some(at)) {
            *SELECTED.write() = Some(id);
        }
    };
    rsx! {
        div { class: "fixed inset-0 z-[95]", onpointerdown: move |_| *MENU.write() = None, oncontextmenu: move |e| { e.prevent_default(); *MENU.write() = None; } }
        div {
            class: "fixed z-[96] w-56 rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg animate-in",
            style: "left: {sx}px; top: {sy}px;",
            button { class: item, onclick: move |_| open("new"), icons::MessageSquarePlus {} {tr("Neuer Chat hier")} }
            button { class: item, onclick: move |_| open("terminal"), icons::SquareTerminal {} {tr("Terminal hier")} }
            button { class: item, onclick: move |_| open("browser"), icons::Globe {} {tr("Browser hier")} }
            div { class: "-mx-1 my-1 h-px bg-border" }
            button {
                class: item,
                onclick: move |_| {
                    *MENU.write() = None;
                    let s = CanvasShape { id: new_shape_id(), kind: "note".into(), x: at.0, y: at.1, w: 240.0, h: 150.0, color: NOTE_COLORS[0].into(), size: 2.0, author: "user".into(), ..Default::default() };
                    with_board(|b| b.shapes.push(s.clone()));
                    *EDITING.write() = Some(s.id);
                },
                icons::StickyNote {}
                {tr("Notiz hier")}
            }
            button { class: item, onclick: move |_| { *MENU.write() = None; arrange(); }, icons::LayoutGrid {} {tr("Fenster anordnen")} }
        }
    }
}

thread_local! {
    /// Leertaste gedrückt (verschieben wie mit der Hand).
    static SPACE: Cell<bool> = const { Cell::new(false) };
}

/// Ist das Ziel ein Eingabefeld (dann gehören Tasten ihm)?
fn typing(target: Option<web_sys::EventTarget>) -> bool {
    let Some(el) = target.and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return false };
    let tag = el.tag_name().to_lowercase();
    matches!(tag.as_str(), "input" | "textarea" | "select") || el.closest("[contenteditable=true],[role=dialog]").ok().flatten().is_some() || el.closest("[data-cw] pre").ok().flatten().is_some()
}

fn on_canvas() -> bool {
    *state::MODE.peek() == state::UiMode::Canvas && web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id(VIEW_ID)).is_some()
}

/// Rad, Pinch (WebKit), Tasten und Vollbild-Wechsel – einmal für die ganze App.
fn install_listeners() {
    if INSTALLED.with(|i| i.replace(true)) {
        return;
    }
    let Some(win) = web_sys::window() else { return };
    let opts = web_sys::AddEventListenerOptions::new();
    opts.set_passive(false);

    // Zieh-Aktionen: Bewegungen gesammelt einmal je Bild, Loslassen sofort
    FRAME_CB.with(|f| {
        *f.borrow_mut() = Some(Closure::<dyn FnMut(f64)>::new(move |_t: f64| {
            FRAME.with(|f| f.set(false));
            if let Some((x, y, shift)) = PENDING_MOVE.with(|p| p.take()) {
                move_to(x, y, shift);
            }
        }));
    });
    let pmove = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |e: web_sys::PointerEvent| {
        if DRAG.peek().is_none() {
            return;
        }
        PENDING_MOVE.with(|p| p.set(Some((e.client_x() as f64, e.client_y() as f64, e.shift_key()))));
        if !FRAME.with(|f| f.replace(true)) {
            let ok = FRAME_CB.with(|f| f.borrow().as_ref().and_then(|cb| web_sys::window().and_then(|w| w.request_animation_frame(cb.as_ref().unchecked_ref()).ok())).is_some());
            if !ok {
                FRAME.with(|f| f.set(false));
            }
        }
    });
    let _ = win.add_event_listener_with_callback("pointermove", pmove.as_ref().unchecked_ref());
    pmove.forget();
    let pup = Closure::<dyn FnMut(web_sys::PointerEvent)>::new(move |e: web_sys::PointerEvent| {
        if DRAG.peek().is_some() {
            release(e.client_x() as f64, e.client_y() as f64);
        }
    });
    let _ = win.add_event_listener_with_callback("pointerup", pup.as_ref().unchecked_ref());
    let _ = win.add_event_listener_with_callback("pointercancel", pup.as_ref().unchecked_ref());
    pup.forget();

    let wheel = Closure::<dyn FnMut(web_sys::WheelEvent)>::new(move |e: web_sys::WheelEvent| {
        if !on_canvas() {
            return;
        }
        let Some(target) = e.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else { return };
        if target.closest(&format!("#{VIEW_ID}")).ok().flatten().is_none() {
            return;
        }
        let in_window = target.closest("[data-cw]").ok().flatten().is_some();
        if e.ctrl_key() || e.meta_key() || !in_window {
            stop_glide();
        }
        if e.ctrl_key() || e.meta_key() {
            e.prevent_default();
            let factor = (-e.delta_y() * if e.delta_mode() == 1 { 0.05 } else { 0.0025 }).exp();
            zoom_at(e.client_x() as f64, e.client_y() as f64, factor);
        } else if !in_window {
            e.prevent_default();
            let k = if e.delta_mode() == 1 { 32.0 } else { 1.0 };
            let (dx, dy) = if e.shift_key() && e.delta_x() == 0.0 { (e.delta_y(), 0.0) } else { (e.delta_x(), e.delta_y()) };
            let c = cam_now();
            preview_cam(Cam { x: c.x - dx * k, y: c.y - dy * k, z: c.z });
            commit_cam_soon();
        }
    });
    let _ = win.add_event_listener_with_callback_and_add_event_listener_options("wheel", wheel.as_ref().unchecked_ref(), &opts);
    wheel.forget();

    // Safari/WebKit (App unter macOS): Pinch kommt als „gesture“-Ereignis
    let gstart = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        if !on_canvas() {
            return;
        }
        e.prevent_default();
        stop_glide();
        GESTURE_Z.with(|g| g.set(cam_now().z));
    });
    let _ = win.add_event_listener_with_callback_and_add_event_listener_options("gesturestart", gstart.as_ref().unchecked_ref(), &opts);
    gstart.forget();
    let gchange = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        if !on_canvas() {
            return;
        }
        e.prevent_default();
        let get = |k: &str| js_sys::Reflect::get(&e, &k.into()).ok().and_then(|v| v.as_f64());
        let (Some(scale), Some(x), Some(y)) = (get("scale"), get("clientX"), get("clientY")) else { return };
        let target = GESTURE_Z.with(|g| g.get()) * scale;
        let cur = cam_now().z;
        if cur > 0.0 {
            zoom_at(x, y, target / cur);
        }
    });
    let _ = win.add_event_listener_with_callback_and_add_event_listener_options("gesturechange", gchange.as_ref().unchecked_ref(), &opts);
    gchange.forget();

    let keydown = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
        if !on_canvas() || typing(e.target()) {
            return;
        }
        let key = e.key();
        let cmd = e.ctrl_key() || e.meta_key();
        if key == " " {
            SPACE.with(|s| s.set(true));
            e.prevent_default();
            return;
        }
        if cmd {
            match key.as_str() {
                "z" | "Z" => {
                    e.prevent_default();
                    undo(e.shift_key());
                }
                "y" => {
                    e.prevent_default();
                    undo(true);
                }
                "0" => {
                    e.prevent_default();
                    zoom_step(1.0 / CAM.peek().z);
                }
                "=" | "+" => {
                    e.prevent_default();
                    zoom_step(1.25);
                }
                "-" => {
                    e.prevent_default();
                    zoom_step(0.8);
                }
                _ => {}
            }
            return;
        }
        match key.as_str() {
            "Escape" => {
                *SHAPE_SEL.write() = None;
                *SELECTED.write() = None;
                *MENU.write() = None;
                *TOOL.write() = Tool::Select;
            }
            "Delete" | "Backspace" => {
                // Erst lesen, dann löschen (`delete_shapes` schreibt SHAPE_SEL)
                let selected = SHAPE_SEL.peek().clone();
                if let Some(id) = selected {
                    e.prevent_default();
                    delete_shapes(vec![id]);
                }
            }
            "!" => fit_all(),
            k => {
                if e.alt_key() {
                    return;
                }
                if let Some(t) = Tool::ALL.iter().find(|t| t.key() == k.to_lowercase()) {
                    *TOOL.write() = *t;
                }
            }
        }
    });
    let _ = win.add_event_listener_with_callback("keydown", keydown.as_ref().unchecked_ref());
    keydown.forget();
    let keyup = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |e: web_sys::KeyboardEvent| {
        if e.key() == " " {
            SPACE.with(|s| s.set(false));
        }
    });
    let _ = win.add_event_listener_with_callback("keyup", keyup.as_ref().unchecked_ref());
    keyup.forget();

    let fs = Closure::<dyn FnMut(web_sys::Event)>::new(move |_e: web_sys::Event| {
        let on = web_sys::window().and_then(|w| w.document()).and_then(|d| d.fullscreen_element()).is_some();
        if *FULLSCREEN.peek() != on {
            *FULLSCREEN.write() = on;
        }
    });
    let _ = win.add_event_listener_with_callback("fullscreenchange", fs.as_ref().unchecked_ref());
    fs.forget();
}

/// Fenster, die gerade arbeiten (für die obere Leiste).
pub fn working_in(key: i64) -> usize {
    THREADS.read().values().filter(|t| t.parent_id.is_none() && t.project_id.unwrap_or(0) == key && t.status.is_active()).count()
}
