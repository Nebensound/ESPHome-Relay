# ESPHome Firmware Relay

Lokaler Proxy/Cache für ESPHome OTA-Updates aus privaten GitHub Repos.

ESPs können keine privaten GitHub-Repos erreichen (kein Auth-Header auf Mikrocontrollern praktikabel). Der Relay authentifiziert sich bei GitHub, cached Firmware lokal und stellt sie per einfachem HTTP im LAN bereit.

## Einrichtung

1. In der **Konfiguration** den GitHub Personal Access Token (PAT) und Repo-Namen eintragen
2. Addon starten – der Relay lädt automatisch alle Firmware-Releases aus dem konfigurierten Repo
3. Im **Ingress-Panel** (Seitenleiste) den Sync-Status aller Geräte überprüfen

## Konfiguration

| Option                    | Beschreibung                                                                                                            |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| **github_token**          | GitHub PAT mit `Contents: read` Scope. Erstellen unter: GitHub → Settings → Developer settings → Personal access tokens |
| **github_repo**           | Privates Repo mit Firmware-Releases (Format: `Owner/Repo`)                                                              |
| **poll_interval_minutes** | Intervall in Minuten, wie oft der Relay nach neuen Releases sucht (5–1440)                                              |
| **webhook_secret**        | Optional: Shared Secret für GitHub Webhook. Ermöglicht sofortige Updates bei neuem Release                              |
| **cache_dir**             | Lokaler Cache-Pfad (Standard: `/data/firmware-cache`)                                                                   |
| **log_level**             | Log-Stufe: `trace`, `debug`, `info`, `warn` oder `error`                                                                |

## ESPHome-Geräte konfigurieren

In der ESPHome-Device-Konfiguration die OTA-URL auf den Relay zeigen:

```yaml
ota:
  platform: http_request
  url: http://<homeassistant-ip>:8099/devices/<gerätename>/manifest.json
```

## Webhook einrichten (optional)

Für sofortige Updates ohne auf den nächsten Poll zu warten:

1. Im GitHub Device-Repo → **Settings → Webhooks → Add webhook**
2. **Payload URL**: `http://<relay-host>:8099/webhook/github`
3. **Content type**: `application/json`
4. **Secret**: Gleicher Wert wie `webhook_secret` in der Addon-Konfiguration
5. **Events**: Nur **Releases** auswählen

## Status-Dashboard

Das Ingress-Panel in der HA-Seitenleiste zeigt pro Gerät:

- **Lokal**: Firmware-Version im Cache
- **Remote**: Firmware-Version im GitHub Release
- **Status**: Sync-Status (aktuell, lade Firmware…, veraltet)

## Manueller Refresh

Ein Cache-Refresh kann jederzeit manuell ausgelöst werden – z.B. per HA-Automation:

```yaml
service: rest_command.esphome_relay_refresh
```

Oder direkt: `POST http://<homeassistant-ip>:8099/refresh`
