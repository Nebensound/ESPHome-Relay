# Copilot Instructions – ESPHome Firmware Relay

Siehe [README.md](../../README.md) für Projektbeschreibung, Konfiguration, API-Endpunkte und Funktionsweise.

**Device-Repo**: [Nebensound/ESPHome-WagnerHof](https://github.com/Nebensound/ESPHome-WagnerHof) (privat) – enthält Device-YAMLs, Common-Packages und CI-Workflows.

## Technologie

- **Sprache**: Rust
- **Framework**: Axum
- **HTTP-Client**: reqwest (für GitHub API)
- **Deployment**: HA-Addon (Docker-basiert), Image per GitHub Actions nach `ghcr.io/nebensound/esphome-relay`

## CI/CD

3 Workflows:

1. **`ci.yml`** – Läuft auf `develop`-Pushes und PRs gegen `main`. Tests, `cargo fmt --check`, `cargo clippy`.
2. **`release.yml`** – Wenn PR nach `main` gemerged → SemVer-Tag via `paulhatch/semantic-version`, GitHub Release erstellen.
3. **`build-addon.yml`** – Auf Tag `v*` → Docker Multi-Arch Build (amd64, arm64, armv7) → Push nach `ghcr.io/nebensound/esphome-relay`.

## Coding-Konventionen

- **Sprache**: Englisch für Code, Deutsch für User-facing Strings (Addon-Beschreibung)
- **Git-Workflow**: GitHub Desktop + VS Code UI bevorzugt, kein CLI
- **Branches**: `develop` = Entwicklung, `main` = stable. PRs von `develop` → `main`
- **Versionierung**: SemVer via `paulhatch/semantic-version`. Commit-Message-Keywords steuern Bump:
  - `(MAJOR)` → Major-Bump (Breaking Changes)
  - `(MINOR)` → Minor-Bump (neue Features)
  - Sonst → Patch-Bump (Bugfixes)
- **Commit-Messages**: Conventional Commits
- **Rust**: Ein Modul pro Verantwortungsbereich (`github.rs`, `cache.rs`, `server.rs`, `webhook.rs`, `status.rs`, `config.rs`), API-Calls in dedizierten Modulen mit Rate-Limit-Respekt

## Design-Guidelines

- **Externe Crates bevorzugen**: Lieber ein etabliertes, häufig genutztes Crate nutzen als Logik selbst schreiben – auch wenn es etwas Overhead erzeugt. Macht Code lesbarer und besser wartbar. Beispiele: `semver` für Versionsvergleiche, `hmac`/`sha2` für Kryptografie, `chrono` für Zeitoperationen etc.

## Sicherheit

- HTTPS für GitHub-Kommunikation (Relay → GitHub API)
- HTTP im LAN akzeptabel (Relay → ESPs), da internes Netzwerk
- GitHub PAT nur als Addon-Option in der HA UI, niemals im Code

## Architektur-Entscheidungen

- **Caching**: Nur lokales Filesystem (`/data/firmware-cache`), kein externer Object Storage
- **Sync-Delete**: Bei jedem Refresh werden Remote-Assets mit lokalem Cache verglichen. Nur Deltas werden geladen, entfernte Firmware wird lokal gelöscht
- **Ingress-Panel**: Einfaches HTML-Dashboard über HA Ingress, zeigt Geräte-Status (lokal/remote Versionen, Sync-Status)

## Entwicklung

### Voraussetzungen

- Rust ≥ 1.87 (Edition 2024)
- cargo

### Build & Test

```bash
cd esphome-relay

# Tests ausführen (87 Tests)
cargo test

# Nur Tests eines Moduls
cargo test config
cargo test cache
cargo test github
cargo test status
cargo test webhook
cargo test server

# Release-Build
cargo build --release

# Lokal starten (braucht /data/options.json – siehe unten)
cargo run
```

### Lokales Testen

Der Relay liest seine Config aus `/data/options.json` (HA-Addon-Konvention). Für lokales Testen:

```bash
sudo mkdir -p /data
cat > /tmp/options.json << 'EOF'
{
  "github_token": "ghp_...",
  "github_repo": "Nebensound/wagnerhof-esphome",
  "poll_interval_minutes": 5,
  "cache_dir": "/tmp/firmware-cache",
  "log_level": "debug"
}
EOF
sudo cp /tmp/options.json /data/options.json
cargo run
```

### Docker-Build (wie CI)

```bash
cd esphome-relay
docker build -t esphome-relay .
```

### Modul-Übersicht

| Modul | Verantwortung | Testbar |
|---|---|---|
| `config.rs` | `/data/options.json` parsen + validieren | Unit-Tests mit tempfile |
| `cache.rs` | Firmware lesen/schreiben, Manifest-Rewrite, Sync-Delete | Unit-Tests mit tempfile |
| `github.rs` | GitHub API (Releases, Downloads), Asset-Parsing | Unit-Tests (Parsing), HTTP nur live |
| `status.rs` | Geräte-Status-Tracking (Thread-safe), Versionsvergleich via `semver` | Unit-Tests |
| `webhook.rs` | HMAC-SHA256 Signatur-Validierung, Event-Typ-Prüfung | Unit-Tests |
| `server.rs` | Axum-Router, alle Endpoints, Ingress-HTML-Panel | Integration-Tests via `tower::ServiceExt` |
| `main.rs` | Verdrahtung, Background-Sync-Task, Server-Start | – |
