# ESPHome Firmware Relay (HA-Addon)

Lokaler Proxy/Cache für ESPHome OTA-Updates aus privaten GitHub Repos. Läuft als Home Assistant Addon.

ESPs können keine privaten GitHub-Repos erreichen (kein Auth-Header auf Mikrocontrollern praktikabel). Der Relay authentifiziert sich bei GitHub, cached Firmware lokal und stellt sie per einfachem HTTP im LAN bereit.

## Features

- **Proxy**: Authentifiziert sich bei GitHub API mit Personal Access Token
- **Cache**: Firmware wird lokal gecacht (nur Filesystem, kein externer Storage). Bleibt verfügbar, auch wenn GitHub oder Internet ausfällt
- **Sync-Delete**: Bei jedem Refresh werden nur veränderte Assets heruntergeladen. Lokal vorhandene Firmware, die remote nicht mehr existiert, wird automatisch gelöscht
- **Manifest-Rewrite**: Schreibt URLs im Manifest von GitHub auf lokale Relay-URLs um
- **Status-Dashboard**: HA-Ingress-Panel zeigt pro Gerät lokale/remote Firmware-Versionen und aktuellen Sync-Status

## Installation

1. In Home Assistant: **Einstellungen → Add-ons → Add-on Store → ⋮ → Repositories**
2. URL hinzufügen: `https://github.com/Nebensound/esphome-relay`
3. "ESPHome Firmware Relay" installieren
4. In der Addon-Konfiguration den GitHub PAT und Repo-Namen eintragen

## Konfiguration

| Option                  | Typ          | Standard                       | Beschreibung                              |
| ----------------------- | ------------ | ------------------------------ | ----------------------------------------- |
| `github_token`          | string       | –                              | GitHub PAT mit `Contents: read` Scope     |
| `github_repo`           | string       | `Nebensound/wagnerhof-esphome` | Privates Repo mit Firmware-Releases       |
| `poll_interval_minutes` | int (5–1440) | `30`                           | Intervall für Release-Checks              |
| `webhook_secret`        | string       | –                              | Shared Secret für GitHub Webhook (HMAC-SHA256 Signatur-Validierung) |
| `cache_dir`             | string       | `/data/firmware-cache`         | Lokaler Cache-Pfad                        |
| `log_level`             | enum         | `info`                         | `trace`, `debug`, `info`, `warn`, `error` |

## API-Endpunkte

```
GET  /devices/{name}/manifest.json     Gecachtes Manifest für das Gerät
GET  /devices/{name}/firmware.ota.bin   Gecachte Firmware-Binary
GET  /devices                           Liste aller verfügbaren Geräte + Versionen
GET  /status                            Status aller Geräte (lokal/remote Versionen, Sync-Status)
GET  /health                            Health-Check
POST /refresh                           Manueller Cache-Refresh (LAN, ohne Auth)
POST /webhook/github                    GitHub Webhook Endpoint (HMAC-SHA256 validiert)
```

## Funktionsweise

1. **Polling**: Prüft regelmäßig via GitHub API ob ein neues Release existiert
2. **GitHub Webhook** (optional): GitHub sendet bei neuem Release ein Event an `POST /webhook/github` → sofortiger Cache-Refresh ohne Warten auf nächsten Poll
3. **Sync**: Vergleicht Remote-Assets mit lokalem Cache. Nur veränderte Dateien werden heruntergeladen, lokal vorhandene Firmware ohne Remote-Gegenstück wird gelöscht
4. **Manifest-Rewrite**: Schreibt `path` im Manifest auf lokale Relay-URL um (`http://<host>:8099/devices/<name>/firmware.ota.bin`)
5. **Serving**: ESPHome-Geräte holen sich Manifest und Firmware per HTTP vom Relay
6. **Manueller Refresh**: `POST /refresh` für sofortigen Cache-Refresh per HA-Automation (LAN, ohne Auth)

### Status-Dashboard (HA-Ingress)

Das Addon stellt ein einfaches Web-Panel über Home Assistant Ingress bereit. Es zeigt:

| Spalte          | Beschreibung                                                        |
| --------------- | ------------------------------------------------------------------- |
| **Gerät**       | Name des ESP-Geräts                                                 |
| **Lokal**       | Firmware-Version im lokalen Cache                                   |
| **Remote**      | Firmware-Version im GitHub Release                                  |
| **Status**      | Aktueller Sync-Status (`aktuell`, `lade Firmware…`, `veraltet`, …)  |

### Webhook-Setup (GitHub → Relay)

1. Im GitHub Device-Repo → **Settings → Webhooks → Add webhook**
2. **Payload URL**: `http://<relay-host>:8099/webhook/github` (bzw. über Cloudflare Tunnel / Reverse-Proxy mit HTTPS falls aus dem Internet erreichbar)
3. **Content type**: `application/json`
4. **Secret**: Gleicher Wert wie `webhook_secret` in der Addon-Konfiguration
5. **Events**: Nur `Releases` auswählen

Der Relay validiert jede eingehende Webhook-Anfrage per HMAC-SHA256 (`X-Hub-Signature-256` Header) gegen das konfigurierte `webhook_secret`. Ungültige oder fehlende Signaturen werden mit `401` abgelehnt. Nur Events vom Typ `release` mit Action `published` lösen einen Cache-Refresh aus.

## Repo-Struktur

```
esphome-relay/
├── repository.yaml              # HA Addon-Repository Manifest
└── esphome-relay/               # Das eigentliche Addon
    ├── config.yaml              # HA-Addon Manifest
    ├── Dockerfile               # Multi-stage: Rust build → minimal Runtime
    ├── run.sh                   # Entrypoint
    ├── Cargo.toml
    └── src/
        ├── main.rs
        ├── github.rs            # GitHub API Client
        ├── cache.rs             # Lokaler Firmware-Cache + Sync-Delete
        ├── server.rs            # Axum HTTP-Server + Ingress-Panel
        ├── webhook.rs           # GitHub Webhook Handler + HMAC-Validierung
        ├── status.rs            # Geräte-Status (lokal/remote/device Versionen)
        └── config.rs            # Addon-Options parsen
```
