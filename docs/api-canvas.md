# API – Bereich canvas

Alle Pfade unter `/api/v1`, JSON in camelCase, Zeiten in Unix-Sekunden. Formen in `shared/src/canvas.rs`.
Eine Fläche je Benutzer und Arbeitsbereich: `<project>` ist die Projekt-ID, `0` = ohne Projekt.
Fremde Projekte gibt es nicht (404). Wird ein Projekt gelöscht, verschwindet auch seine Fläche.

| Methode | Pfad | Inhalt → Antwort |
|---|---|---|
| GET | `/canvas/<project>` | → `CanvasBoard` (`windows`, `closed`, `shapes`, `rev`, `projectId`); noch nie gespeichert = leer mit `rev` 0 |
| PUT | `/canvas/<project>/layout` | `CanvasLayout` (`windows`, `closed`) → `CanvasRev`. Gehört der Oberfläche. Fenster: `chat` (mit `threadId`), `new` (leerer Chat), `terminal` (`terminalId`), `browser` (`url`), jeweils mit `name`, `x`, `y`, `w`, `h`, `z`; ungültige fallen weg, Grössen werden begrenzt, höchstens 48 Fenster. `closed`: Threads, deren Fenster geschlossen wurde (sie öffnen sich nicht von selbst wieder) |
| POST | `/canvas/<project>/shapes` | `CanvasShapesChange` (`upsert`, `delete`, `clear`) → `CanvasRev`. Arten `pen`, `rect`, `ellipse`, `diamond`, `arrow`, `line`, `text`, `note`; gleiche ID = ändern, unbekannte Arten fallen weg; höchstens 3000 Formen und 4000 Punkte je Strich |

Jede Änderung erhöht `rev` und sendet `CanvasUpdated {projectId, rev}` – andere Tabs und
Zeichnungen von Agenten erscheinen sofort.

## Agenten (MCP)

`canvas_read` und `canvas_draw` (siehe [api-agents.md](api-agents.md)) arbeiten auf der Fläche des
Arbeitsbereichs, zu dem der Thread gehört. Formen von Agenten tragen als `author` den Fensternamen
(bei Subagenten den des Hauptthreads, ohne Fenster Agent und Titel); löschen dürfen Agenten nur
eigene Formen. Ohne `x`/`y` landen neue Formen neben dem Fenster des Agenten. Pfeile verbinden über
`from`/`to` Fenster (Name) und Formen (ID oder Text); unbekannte Ziele sind ein Fehler, es wird dann
nichts gezeichnet.

## Browser-Fenster

Browser-Fenster nutzen die Vorschau (siehe [api-workspace.md](api-workspace.md)) auch ohne Thread:
`GET /previews?projectId=` und `POST /previews/token` mit `threadId: 0`.
