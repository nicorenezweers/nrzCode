// Migrationen sind ins Programm eingebettet (sqlx::migrate!) – bei Änderungen neu übersetzen.
//
// Die Oberfläche (Dioxus, `ui/`) wird mit `scripts/ui.sh` nach `ui/dist/public` gebaut und hier
// ins Programm eingebettet: Für jede Datei entsteht ein Eintrag in `$OUT_DIR/spa.rs`, Text und
// WebAssembly zusätzlich gzip-komprimiert. Fehlt der Ordner, bleibt die Liste leer und der Server
// zeigt einen Hinweis statt der Oberfläche.
//
// Übersetzungen: `locales/<sprache>/*.json` (deutscher Text → Übersetzung, je Bereich eine Datei)
// werden je Sprache zu einem Katalog zusammengeführt (`$OUT_DIR/locales.rs`).
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=ui/dist/public");
    println!("cargo:rerun-if-changed=locales");
    check_migrations();

    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/dist/public");
    let mut files = Vec::new();
    if root.join("index.html").is_file() {
        collect(&root, &root, &mut files);
    }
    files.sort();

    let mut code = String::from("pub static FILES: &[SpaFile] = &[\n");
    for (i, rel) in files.iter().enumerate() {
        let src = root.join(rel);
        println!("cargo:rerun-if-changed={}", src.display());
        let data = std::fs::read(&src).unwrap();
        let gz = if compressible(rel) {
            let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
            enc.write_all(&data).unwrap();
            let gz = enc.finish().unwrap();
            let path = out.join(format!("spa-{i}.gz"));
            std::fs::write(&path, gz).unwrap();
            format!("Some(include_bytes!({:?}))", path.display().to_string())
        } else {
            "None".into()
        };
        code += &format!(
            "    SpaFile {{ path: {rel:?}, body: include_bytes!({:?}), gzip: {gz} }},\n",
            src.display().to_string()
        );
    }
    code += "];\n";
    std::fs::write(out.join("spa.rs"), code).unwrap();

    // Übersetzungen je Sprache zusammenführen
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    let mut langs: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    langs.sort();
    let mut code = String::from("pub static LOCALES: &[(&str, &[u8])] = &[\n");
    for lang_dir in langs {
        let lang = lang_dir.file_name().unwrap().to_string_lossy().to_string();
        let mut parts: Vec<PathBuf> = std::fs::read_dir(&lang_dir)
            .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect())
            .unwrap_or_default();
        parts.sort();
        let mut catalog: BTreeMap<String, String> = BTreeMap::new();
        for part in parts {
            println!("cargo:rerun-if-changed={}", part.display());
            let text = std::fs::read_to_string(&part).unwrap();
            let map: BTreeMap<String, String> = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", part.display()));
            catalog.extend(map);
        }
        let path = out.join(format!("locale-{lang}.json"));
        std::fs::write(&path, serde_json::to_vec(&catalog).unwrap()).unwrap();
        code += &format!("    ({lang:?}, include_bytes!({:?})),\n", path.display().to_string());
    }
    code += "];\n";
    std::fs::write(out.join("locales.rs"), code).unwrap();
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, files);
        } else {
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            files.push(rel);
        }
    }
}

fn compressible(path: &str) -> bool {
    [".html", ".js", ".css", ".wasm", ".svg", ".json"].iter().any(|e| path.ends_with(e))
}

/// Migrationen werden eingebettet und über ihre Prüfsumme wiedererkannt. Mit Windows-Zeilenenden
/// (git `core.autocrlf`) ausgecheckt hätten sie eine andere – die App hielte jede schon
/// angewandte Migration für verändert und startete nicht mehr ("migration 1 was previously
/// applied but has been modified"). So ein Programm darf gar nicht erst entstehen.
fn check_migrations() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for path in entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "sql")) {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        if bytes.contains(&b'\r') {
            panic!(
                "{} hat Windows-Zeilenenden (CRLF). Die Migrationen müssen mit LF ausgecheckt sein, sonst startet \
                 die App bei bestehenden Datenbanken nicht: `git config core.autocrlf false` und neu auschecken \
                 (siehe .gitattributes).",
                path.display()
            );
        }
    }
}
