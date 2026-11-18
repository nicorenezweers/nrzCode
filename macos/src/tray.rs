//! macOS: Symbol in der Menüleiste. Ein Klick öffnet ein kleines, rahmenloses Fenster direkt
//! unter dem Symbol mit der Seite `/tray` der Oberfläche (letzte Threads, Nutzung mit Kosten,
//! Tokens und Limits). Es verschwindet, sobald es den Fokus verliert – wie ein Popover.

use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event_loop::EventLoopWindowTarget;
use tao::window::{Window, WindowBuilder};
use tray_icon::{Icon, Rect, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// Grösse des Fensters (Punkte).
pub const WIDTH: f64 = 360.0;
pub const HEIGHT: f64 = 600.0;

/// Symbol als Vorlage (schwarz mit Alphakanal – macOS färbt es hell oder dunkel):
/// eine Eingabeaufforderung „>_“, 44×44 Pixel für die Retina-Menüleiste.
fn icon() -> Icon {
    const N: usize = 44;
    // Strecken (x1, y1, x2, y2) und Strichstärke
    let segs = [(10.0, 12.0, 20.0, 22.0), (20.0, 22.0, 10.0, 32.0), (24.0, 32.5, 35.0, 32.5)];
    let half = 2.4_f64;
    let mut rgba = vec![0u8; N * N * 4];
    for py in 0..N {
        for px in 0..N {
            let (x, y) = (px as f64 + 0.5, py as f64 + 0.5);
            let d = segs
                .iter()
                .map(|&(x1, y1, x2, y2)| {
                    let (dx, dy) = (x2 - x1, y2 - y1);
                    let t = (((x - x1) * dx + (y - y1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                    ((x - x1 - t * dx).powi(2) + (y - y1 - t * dy).powi(2)).sqrt()
                })
                .fold(f64::MAX, f64::min);
            // Weiche Kante über einen Pixel
            let a = (half + 0.5 - d).clamp(0.0, 1.0);
            rgba[(py * N + px) * 4 + 3] = (a * 255.0).round() as u8;
        }
    }
    Icon::from_rgba(rgba, N as u32, N as u32).expect("Symbol")
}

/// Symbol anlegen (im Haupt-Thread, nachdem die Ereignisschleife läuft). Klicks gehen über
/// `on_event` an die App.
pub fn create(on_event: impl Fn(TrayIconEvent) + Send + Sync + 'static) -> Option<TrayIcon> {
    TrayIconEvent::set_event_handler(Some(on_event));
    match TrayIconBuilder::new().with_icon_templated(icon()).with_tooltip("nrzCode").build() {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!("Menüleiste: {e}");
            None
        }
    }
}

/// Rahmenloses, durchsichtiges Fenster (die Seite zeichnet runde Ecken), unsichtbar bis zum Klick.
pub fn window<T: 'static>(target: &EventLoopWindowTarget<T>) -> Option<Window> {
    use tao::platform::macos::WindowBuilderExtMacOS;
    WindowBuilder::new()
        .with_title("nrzCode")
        .with_decorations(false)
        .with_resizable(false)
        .with_visible(false)
        .with_transparent(true)
        .with_always_on_top(true)
        .with_has_shadow(true)
        .with_inner_size(LogicalSize::new(WIDTH, HEIGHT))
        .build(target)
        .map_err(|e| tracing::warn!("Menüleisten-Fenster: {e}"))
        .ok()
}

/// Fenster mittig unter das Symbol setzen (innerhalb des Bildschirms).
pub fn place(window: &Window, rect: Rect) {
    let scale = window.scale_factor();
    let w = WIDTH * scale;
    let cx = rect.position.x + rect.size.width as f64 / 2.0;
    let mut x = cx - w / 2.0;
    let y = rect.position.y + rect.size.height as f64 + 4.0 * scale;
    if let Some(m) = window.current_monitor().or_else(|| window.primary_monitor()) {
        let (mx, mw) = (m.position().x as f64, m.size().width as f64);
        x = x.clamp(mx + 8.0 * scale, (mx + mw - w - 8.0 * scale).max(mx));
    }
    window.set_outer_position(PhysicalPosition::new(x, y));
}
