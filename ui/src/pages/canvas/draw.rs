//! Zeichenebene wie in Excalidraw: Stift, Linie, Pfeil, Rechteck, Ellipse, Raute, Text und
//! Notizzettel. Formen liegen in Flächenkoordinaten (SVG unter den Fenstern, Text und Notizen als
//! HTML darüber); Treffer werden geometrisch bestimmt (Auswählen, Radieren).

use super::store::*;
use crate::i18n::tr;
use dioxus::prelude::*;
use nrzcode_shared::canvas::CanvasShape;

/// Farben der Werkzeugleiste (`""` = Vordergrund, passt zu hell und dunkel).
pub const COLORS: [&str; 6] = ["", "#e03131", "#2f9e44", "#1971c2", "#f08c00", "#9c36b5"];
/// Farben der Notizzettel.
pub const NOTE_COLORS: [&str; 4] = ["#f5c542", "#9be7a8", "#a5d8ff", "#ffc9e3"];
pub const STROKES: [f64; 3] = [2.0, 4.0, 7.0];

/// CSS-Farbe einer Form.
pub fn css_color(c: &str) -> String {
    if c.is_empty() {
        "var(--foreground)".into()
    } else {
        c.to_string()
    }
}

/// Stand einer Form während des Ziehens ([`LIVE_SHAPE`]) – nur die gezogene Form zeichnet sich neu.
fn use_live_shape(id: &str) -> Memo<Option<CanvasShape>> {
    let id = id.to_string();
    use_memo(move || LIVE_SHAPE.read().as_ref().filter(|x| x.id == id).cloned())
}

/// Rahmen einer Form: (x0, y0, x1, y1).
pub fn bbox(s: &CanvasShape) -> (f64, f64, f64, f64) {
    if matches!(s.kind.as_str(), "pen" | "line" | "arrow") && !s.points.is_empty() {
        let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for p in &s.points {
            x0 = x0.min(p[0]);
            y0 = y0.min(p[1]);
            x1 = x1.max(p[0]);
            y1 = y1.max(p[1]);
        }
        return (x0, y0, x1, y1);
    }
    let (x0, x1) = if s.w < 0.0 { (s.x + s.w, s.x) } else { (s.x, s.x + s.w) };
    let (y0, y1) = if s.h < 0.0 { (s.y + s.h, s.y) } else { (s.y, s.y + s.h) };
    (x0, y0, x1, y1)
}

/// Rahmen mit positiver Breite/Höhe.
pub fn normalized(mut s: CanvasShape) -> CanvasShape {
    if !matches!(s.kind.as_str(), "pen" | "line" | "arrow") {
        let (x0, y0, x1, y1) = bbox(&s);
        s.x = x0;
        s.y = y0;
        s.w = x1 - x0;
        s.h = y1 - y0;
    }
    s
}

fn seg_dist(p: (f64, f64), a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((p.0 - a[0]) * dx + (p.1 - a[1]) * dy) / len2).clamp(0.0, 1.0) };
    let (qx, qy) = (a[0] + t * dx, a[1] + t * dy);
    ((p.0 - qx).powi(2) + (p.1 - qy).powi(2)).sqrt()
}

fn poly_dist(p: (f64, f64), pts: &[[f64; 2]], closed: bool) -> f64 {
    let mut d = f64::MAX;
    for w in pts.windows(2) {
        d = d.min(seg_dist(p, w[0], w[1]));
    }
    if closed && pts.len() > 2 {
        d = d.min(seg_dist(p, pts[pts.len() - 1], pts[0]));
    }
    if pts.len() == 1 {
        d = d.min(((p.0 - pts[0][0]).powi(2) + (p.1 - pts[0][1]).powi(2)).sqrt());
    }
    d
}

/// Umriss geschlossener Formen.
fn outline(s: &CanvasShape) -> Vec<[f64; 2]> {
    let (x0, y0, x1, y1) = bbox(s);
    match s.kind.as_str() {
        "diamond" => {
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            vec![[cx, y0], [x1, cy], [cx, y1], [x0, cy]]
        }
        "ellipse" => {
            let (cx, cy, rx, ry) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0, (x1 - x0) / 2.0, (y1 - y0) / 2.0);
            (0..40).map(|i| std::f64::consts::TAU * i as f64 / 40.0).map(|a| [cx + rx * a.cos(), cy + ry * a.sin()]).collect()
        }
        _ => vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]],
    }
}

/// Trifft der Punkt die Form? (`tol` in Flächeneinheiten)
pub fn hit(s: &CanvasShape, p: (f64, f64), tol: f64) -> bool {
    let (x0, y0, x1, y1) = bbox(s);
    let inside = p.0 >= x0 && p.0 <= x1 && p.1 >= y0 && p.1 <= y1;
    match s.kind.as_str() {
        "text" | "note" => inside,
        "pen" | "line" | "arrow" => poly_dist(p, &s.points, false) <= tol + s.size / 2.0,
        _ => (s.fill && inside) || poly_dist(p, &outline(s), true) <= tol + s.size / 2.0,
    }
}

/// Oberste Form am Punkt.
pub fn shape_at(p: (f64, f64), tol: f64) -> Option<CanvasShape> {
    BOARD.peek().as_ref().and_then(|b| b.shapes.iter().rev().find(|s| hit(s, p, tol)).cloned())
}

/// Schriftgrösse eines Textes nach Strichstärke.
pub fn font_size(s: &CanvasShape) -> f64 {
    match s.size {
        x if x >= 6.0 => 36.0,
        x if x >= 3.5 => 26.0,
        _ => 19.0,
    }
}

/// Text-Formen wachsen mit ihrem Inhalt.
pub fn fit_text(s: &mut CanvasShape) {
    if s.kind != "text" {
        return;
    }
    let fs = font_size(s);
    let lines: Vec<&str> = s.text.split('\n').collect();
    let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0).max(4) as f64;
    s.w = longest * fs * 0.56 + 12.0;
    s.h = lines.len().max(1) as f64 * fs * 1.3 + 8.0;
}

/// Weicher Pfad durch die Punkte eines Strichs.
fn pen_path(pts: &[[f64; 2]]) -> String {
    let mut d = String::new();
    if pts.is_empty() {
        return d;
    }
    d.push_str(&format!("M{:.1} {:.1}", pts[0][0], pts[0][1]));
    if pts.len() < 3 {
        for p in &pts[1..] {
            d.push_str(&format!(" L{:.1} {:.1}", p[0], p[1]));
        }
        return d;
    }
    for i in 1..pts.len() - 1 {
        let (p, n) = (pts[i], pts[i + 1]);
        d.push_str(&format!(" Q{:.1} {:.1} {:.1} {:.1}", p[0], p[1], (p[0] + n[0]) / 2.0, (p[1] + n[1]) / 2.0));
    }
    let last = pts[pts.len() - 1];
    d.push_str(&format!(" L{:.1} {:.1}", last[0], last[1]));
    d
}

/// Pfeilspitze am Ende einer Linie.
fn arrow_head(a: [f64; 2], b: [f64; 2], size: f64) -> String {
    let ang = (b[1] - a[1]).atan2(b[0] - a[0]);
    let len = 14.0 + size * 2.2;
    let spread = 0.45;
    let p1 = [b[0] - len * (ang - spread).cos(), b[1] - len * (ang - spread).sin()];
    let p2 = [b[0] - len * (ang + spread).cos(), b[1] - len * (ang + spread).sin()];
    format!("M{:.1} {:.1} L{:.1} {:.1} L{:.1} {:.1}", p1[0], p1[1], b[0], b[1], p2[0], p2[1])
}

/// Eine Form im SVG (alles ausser Text und Notizen).
#[component]
pub fn ShapeSvg(shape: CanvasShape, #[props(default)] ghost: bool) -> Element {
    let live = use_live_shape(&shape.id);
    let s = if ghost { shape } else { live().unwrap_or(shape) };
    let color = css_color(&s.color);
    let width = s.size;
    let opacity = if ghost { "0.7" } else { "1" };
    let fill = if s.fill { format!("fill: {color}; fill-opacity: 0.14;") } else { "fill: none;".to_string() };
    let style = format!("stroke: {color}; stroke-width: {width}; stroke-linecap: round; stroke-linejoin: round; opacity: {opacity}; {fill}");
    let (x0, y0, x1, y1) = bbox(&s);
    match s.kind.as_str() {
        "pen" => rsx! { path { d: pen_path(&s.points), style: "{style} fill: none;" } },
        "line" | "arrow" if s.points.len() >= 2 => {
            let a = s.points[0];
            let b = s.points[s.points.len() - 1];
            let head = if s.kind == "arrow" { arrow_head(a, b, width) } else { String::new() };
            let label = s.text.trim().to_string();
            rsx! {
                g {
                    line { x1: "{a[0]}", y1: "{a[1]}", x2: "{b[0]}", y2: "{b[1]}", style: "{style}" }
                    if !head.is_empty() {
                        path { d: head, style: "{style} fill: none;" }
                    }
                    if !label.is_empty() {
                        text { x: "{(a[0] + b[0]) / 2.0}", y: "{(a[1] + b[1]) / 2.0 - 8.0}", "text-anchor": "middle", style: "fill: {color}; stroke: none; font-size: 15px; font-family: var(--font-sans);", "{label}" }
                    }
                }
            }
        }
        "rect" => rsx! { rect { x: "{x0}", y: "{y0}", width: "{x1 - x0}", height: "{y1 - y0}", rx: "10", style: "{style}" } },
        "ellipse" => rsx! { ellipse { cx: "{(x0 + x1) / 2.0}", cy: "{(y0 + y1) / 2.0}", rx: "{(x1 - x0) / 2.0}", ry: "{(y1 - y0) / 2.0}", style: "{style}" } },
        "diamond" => {
            let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
            rsx! { polygon { points: "{cx},{y0} {x1},{cy} {cx},{y1} {x0},{cy}", style: "{style}" } }
        }
        _ => rsx! {},
    }
}

/// Lage, Farben und Schrift eines Textes bzw. Notizzettels (CSS).
fn note_frame(s: &CanvasShape) -> (String, f64, f64) {
    let (x0, y0, x1, y1) = bbox(s);
    let (w, h) = ((x1 - x0).max(40.0), (y1 - y0).max(24.0));
    let note = s.kind == "note";
    let fs = if note { 16.0 } else { font_size(s) };
    let frame = if note {
        let bg = if s.color.is_empty() { NOTE_COLORS[0].to_string() } else { s.color.clone() };
        format!("left:{x0}px; top:{y0}px; width:{w}px; min-height:{h}px; background:{bg}; color:#1f1f1f; font-size:{fs}px; line-height:1.35; padding:12px 14px; border-radius:6px; box-shadow: 0 6px 18px rgb(0 0 0 / 0.22);")
    } else {
        format!("left:{x0}px; top:{y0}px; min-width:{w}px; color:{}; font-size:{fs}px; line-height:1.3; padding:2px 4px;", css_color(&s.color))
    };
    (frame, h, fs)
}

/// Text oder Notizzettel (HTML über der Zeichnung; beim Schreiben ein Eingabefeld).
#[component]
pub fn ShapeNote(shape: CanvasShape, editing: bool) -> Element {
    let live = use_live_shape(&shape.id);
    if editing {
        return rsx! { NoteEditor { shape } };
    }
    let s = live().unwrap_or(shape);
    let note = s.kind == "note";
    let (frame, _, _) = note_frame(&s);
    rsx! {
        div {
            class: "pointer-events-none absolute whitespace-pre-wrap break-words font-sans",
            style: "{frame}",
            if s.text.is_empty() {
                span { class: "opacity-50", if note { {tr("Notiz")} } else { {tr("Text")} } }
            } else {
                "{s.text}"
            }
            if note && !s.author.is_empty() && s.author != "user" {
                div { class: "mt-2 text-[11px] opacity-60", "— {s.author}" }
            }
        }
    }
}

/// Eingabefeld für Text und Notizen; leer → Form wird gelöscht.
#[component]
fn NoteEditor(shape: CanvasShape) -> Element {
    let s = shape;
    let note = s.kind == "note";
    let (frame, h, fs) = note_frame(&s);
    let back = if note { "" } else { "background: color-mix(in oklch, var(--background) 85%, transparent);" };
    let height = h.max(fs * 1.6);
    let mut draft = use_signal(|| s.text.clone());
    let id = s.id.clone();
    let orig = s.clone();
    let finish = move || {
        if EDITING.peek().as_deref() != Some(id.as_str()) {
            return;
        }
        *EDITING.write() = None;
        let text = draft.peek().trim_end().to_string();
        if text.trim().is_empty() {
            delete_shapes(vec![id.clone()]);
            return;
        }
        if text == orig.text {
            return;
        }
        let mut n = orig.clone();
        n.text = text;
        fit_text(&mut n);
        remember_shapes();
        put_shape(n);
    };
    let finish2 = finish.clone();
    rsx! {
        textarea {
            class: "absolute resize-none overflow-hidden border-0 font-sans outline-none ring-2 ring-primary/50",
            style: "{frame} height:{height}px; {back}",
            value: "{draft}",
            placeholder: if note { tr("Notiz …") } else { tr("Text …") },
            onmounted: move |e| async move {
                let _ = e.set_focus(true).await;
            },
            onpointerdown: move |e| e.stop_propagation(),
            oninput: move |e| draft.set(e.value()),
            onkeydown: move |e: KeyboardEvent| {
                e.stop_propagation();
                if e.key() == Key::Escape || (e.key() == Key::Enter && (e.modifiers().ctrl() || e.modifiers().meta())) {
                    finish2();
                }
            },
            onblur: move |_| finish(),
        }
    }
}

/// Rahmen um die ausgewählte Form mit Griff (Grösse) und Löschen.
#[component]
pub fn Selection(shape: CanvasShape) -> Element {
    // Nur der Zoom zählt (Griffe gleich gross), nicht das Verschieben der Kamera
    let z = use_memo(|| CAM.read().z);
    let live = use_live_shape(&shape.id);
    let zoom = z();
    let shape = live().unwrap_or(shape);
    let (x0, y0, x1, y1) = bbox(&shape);
    let pad = 6.0 / zoom;
    let k = 1.0 / zoom;
    let boxed = matches!(shape.kind.as_str(), "rect" | "ellipse" | "diamond" | "note" | "text");
    let id = shape.id.clone();
    let orig = shape.clone();
    rsx! {
        div {
            class: "pointer-events-none absolute rounded-sm border border-dashed border-primary/70",
            style: "left:{x0 - pad}px; top:{y0 - pad}px; width:{x1 - x0 + 2.0 * pad}px; height:{y1 - y0 + 2.0 * pad}px;",
        }
        button {
            class: "absolute grid size-7 place-items-center rounded-full border bg-popover text-muted-foreground shadow-md hover:text-destructive",
            style: "left:{x1 + pad}px; top:{y0 - pad}px; transform: translate(-50%, -50%) scale({k}); transform-origin: center;",
            title: tr("Form löschen (Entf)"),
            onpointerdown: move |e| e.stop_propagation(),
            onclick: move |_| delete_shapes(vec![id.clone()]),
            icons::Trash2 { class: "size-3.5" }
        }
        if boxed {
            div {
                class: "absolute size-3.5 cursor-nwse-resize rounded-sm border-2 border-primary bg-background",
                style: "left:{x1 + pad}px; top:{y1 + pad}px; transform: translate(-50%, -50%) scale({k});",
                onpointerdown: move |e: PointerEvent| {
                    e.stop_propagation();
                    let p = e.client_coordinates();
                    *DRAG.write() = Some(Drag::ShapeResize { sx: p.x, sy: p.y, orig: orig.clone() });
                },
            }
        }
    }
}

/// Kurzbeschreibung der Zeichnung als Text (für „Skizze an einen Chat geben“).
pub fn describe(shapes: &[CanvasShape]) -> String {
    let mut out = String::new();
    let mut strokes = 0;
    let label = |s: &CanvasShape| -> String {
        if !s.text.trim().is_empty() {
            return s.text.split_whitespace().collect::<Vec<_>>().join(" ");
        }
        let (x0, y0, x1, y1) = bbox(s);
        shapes
            .iter()
            .filter(|t| t.kind == "text" && !t.text.trim().is_empty())
            .filter(|t| {
                let b = bbox(t);
                let (cx, cy) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
                cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1
            })
            .map(|t| t.text.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join(" / ")
    };
    let at = |p: [f64; 2]| -> String {
        shapes
            .iter()
            .filter(|s| !matches!(s.kind.as_str(), "pen" | "line" | "arrow"))
            .find(|s| {
                let b = bbox(s);
                p[0] >= b.0 - 32.0 && p[0] <= b.2 + 32.0 && p[1] >= b.1 - 32.0 && p[1] <= b.3 + 32.0
            })
            .map(|s| {
                let l = label(s);
                if l.is_empty() { s.kind.clone() } else { format!("„{l}“") }
            })
            .or_else(|| {
                BOARD.peek().as_ref().and_then(|b| {
                    b.windows.iter().find(|w| p[0] >= w.x - 48.0 && p[0] <= w.x + w.w + 48.0 && p[1] >= w.y - 48.0 && p[1] <= w.y + w.h + 48.0).map(|w| w.name.clone())
                })
            })
            .unwrap_or_else(|| format!("({:.0}, {:.0})", p[0], p[1]))
    };
    for s in shapes {
        let line = match s.kind.as_str() {
            "pen" => {
                strokes += 1;
                continue;
            }
            "note" => format!("- Notiz: „{}“", label(s)),
            "text" => {
                // Text in einem Kasten gehört zum Kasten
                let inside = shapes.iter().any(|o| {
                    matches!(o.kind.as_str(), "rect" | "ellipse" | "diamond") && {
                        let (x0, y0, x1, y1) = bbox(o);
                        let b = bbox(s);
                        let (cx, cy) = ((b.0 + b.2) / 2.0, (b.1 + b.3) / 2.0);
                        cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1
                    }
                });
                if inside {
                    continue;
                }
                format!("- Text: „{}“", label(s))
            }
            "rect" | "ellipse" | "diamond" => {
                let l = label(s);
                let what = match s.kind.as_str() {
                    "rect" => "Kasten",
                    "ellipse" => "Ellipse",
                    _ => "Raute",
                };
                if l.is_empty() { format!("- {what}") } else { format!("- {what} „{l}“") }
            }
            "arrow" | "line" if s.points.len() >= 2 => {
                let what = if s.kind == "arrow" { "Pfeil" } else { "Linie" };
                let extra = if s.text.trim().is_empty() { String::new() } else { format!(" („{}“)", s.text.trim()) };
                format!("- {what} von {} nach {}{extra}", at(s.points[0]), at(s.points[s.points.len() - 1]))
            }
            _ => continue,
        };
        out.push_str(&line);
        out.push('\n');
    }
    if strokes > 0 {
        out.push_str(&format!("- {strokes} Freihand-Striche\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(kind: &str, x: f64, y: f64, w: f64, h: f64) -> CanvasShape {
        CanvasShape { id: kind.into(), kind: kind.into(), x, y, w, h, size: 2.0, ..Default::default() }
    }

    #[test]
    fn hits_outlines_and_strokes() {
        let r = shape("rect", 0.0, 0.0, 100.0, 50.0);
        assert!(hit(&r, (50.0, 1.0), 4.0));
        assert!(!hit(&r, (50.0, 25.0), 4.0), "innen nur gefüllt");
        let mut f = r.clone();
        f.fill = true;
        assert!(hit(&f, (50.0, 25.0), 4.0));
        let mut p = shape("pen", 0.0, 0.0, 0.0, 0.0);
        p.points = vec![[0.0, 0.0], [10.0, 10.0], [20.0, 0.0]];
        assert!(hit(&p, (10.0, 9.0), 3.0));
        assert!(!hit(&p, (10.0, 30.0), 3.0));
        let n = normalized(shape("ellipse", 100.0, 100.0, -40.0, -20.0));
        assert_eq!((n.x, n.y, n.w, n.h), (60.0, 80.0, 40.0, 20.0));
    }
}
