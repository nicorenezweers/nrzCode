# nrzCode – Umbrel-Release

Programme für den nrzStore (App `nrz-code`): `nrzcode-<arch>.gz` (Linux amd64/arm64, Oberfläche
eingebettet), `Rocket.toml` und `SHA256SUMS`. Der `pre-start`-Hook der App lädt sie für die
Architektur des Geräts über `http://localhost:8390/nicorenezweers/nrzCode/raw/tag/umbrel-<version>/`.
Gebaut aus `main` mit `docker buildx build --platform linux/amd64,linux/arm64 --target dist`.
