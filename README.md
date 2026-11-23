# nrzCode – Umbrel-Release

Programme für den nrzStore (App `nrz-code`): `nrzcode-<arch>.gz` (Linux amd64/arm64, Oberfläche
eingebettet), `Rocket.toml` und `SHA256SUMS`. Der `pre-start`-Hook der App lädt sie für die
Architektur des Geräts über `http://localhost:8390/nicorenezweers/nrzCode/raw/tag/umbrel-<version>/`.
Stand: nrzCode 1.0.1, gebaut aus `main` (d2e2e7d) mit `cargo zigbuild --release --target <arch>-unknown-linux-gnu.2.31`.
