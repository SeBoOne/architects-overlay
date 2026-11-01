# Connector-System („Dienste") — Vorausplanung

**Status:** Gegrillt & abgestimmt (2026-11-01), Umsetzungstart WARTET auf die AAWA-Platform-API (persönliche API-Keys in Arbeit).
**Ziel:** Externe Plattformen/Dienste (z. B. AAWA Platform, evtl. CCUGame) werden über deklarative JSON-Manifeste angebunden — kein fremder Code im Overlay. Jeder Nutzer kann sein eigenes Manifest erstellen und hinzufügen.

---

## 1. Entscheidungen (gegrillt, Sebo bestätigt)

| Bereich | Entscheidung |
|---|---|
| Mechanik | **Manifest-Connector** (JSON deklariert Endpoints/Auth/Mappings/Views) — KEINE Code-Plugins (WASM/JS) in v1 |
| Umfang v1 | **L1 Lesen** (Listen + Detail-Views) + **L2 Sync** (z. B. Hangar, Richtung + Strategie pro Connector) |
| L3 (später) | Interaktiv: Resourcenlager-Anfragen, Lieferketten, Textchat pro Anfrage, Events/OC-Interaktion |
| Verbreitung | **Beispieldatei** im Repo als Vorlage; Import per **Plugin-Abschnitt in den Einstellungen** (Hinzufügen-Button, lokale Datei). KEIN zentrales Remote-Verteilsystem in v1 |
| Master-Richtung | Pro Connector konfigurierbar (`sync.master`: `platform`/`local`, `sync.strategy`: `replace`/`merge`). AAWA: Platform = Master, Replace. Keine feste Norm |
| Feld-Mapping | **Dot-Paths** pro Feld (`"name": "data[].ship_name"`) |
| Auth | Mechanik wie UEX-Secret-Key (maskiert, lokal); **Felder erscheinen nur, wenn das Manifest sie deklariert** |
| Labels | de/en im Manifest, Fallback en |
| Aktualisierung | Polling-Intervall pro View aus dem Manifest + manueller Button + bestehender TTL-Cache |
| Writes | Über die API-Keys des Nutzers; keine Extra-Bestätigungsschleife (Verantwortung beim Manifest-Ersteller + Key-Berechtigungen) |
| UI | **Top-Level-Bereich „Dienste"**, Tab pro aktiviertem Connector; **generische Komponenten** (Tabelle, Karten, Detail-View, Kalender-View) per Manifest-Typ |
| Import-Sicherheit | **Streng**: JSON-Schema-Validierung, nur https-Endpoints, keine file://-Pfade, Größenlimit — mit bewusster „Warnung ignorieren"-Option (Verantwortung des Nutzers) |
| Timing | **Framework + Mock-Connector** zuerst (demonstriert alle generischen Views gegen einen Test-Endpoint); AAWA-Connector sobald API + Keys da sind |

## 2. Gesicherte Schutzmaßnahmen

- Replace-Sync: **Bestätigungsdialog beim ersten Sync + lokaler Backup-Snapshot** des Hangars (Schutz vor Verwechslung Master-Richtung; kein Böse-Manifest-Schutz — das regeln die Keys).
- Import-Validierung blockiert file:// und http:// (nur https), Größenlimit für Manifeste.

## 3. Manifest-Schema (Entwurf, v1)

```jsonc
{
  "manifest_version": 1,
  "id": "aawa-platform",                 // eindeutig
  "name": { "de": "AAWA Platform", "en": "AAWA Platform" },
  "base_url": "https://platform.example.org/api",
  "auth": {                              // optional; Einstellungs-Felder erscheinen nur bei Deklaration
    "type": "api_key",                   // api_key | bearer | none
    "key_label": { "de": "API-Key", "en": "API key" },
    "header": "Authorization",           // Header-Name
    "prefix": "Bearer"                   // optionaler Prefix
  },
  "views": [
    {
      "id": "hangar",
      "type": "table",                   // table | cards | detail | calendar
      "title": { "de": "Hangar", "en": "Hangar" },
      "endpoint": "/hangar",
      "mapping": { "name": "data[].ship_name", "modules": "data[].modules" },
      "poll_seconds": 60
    },
    {
      "id": "events",
      "type": "calendar",
      "endpoint": "/events",
      "mapping": { "start": "data[].start_at", "title": "data[].title" },
      "poll_seconds": 300
    }
  ],
  "sync": {                              // optional (L2)
    "master": "platform",                // platform | local
    "strategy": "replace",               // replace | merge
    "pull": { "endpoint": "/hangar", "mapping": { "..." : "..." } },
    "push": { "endpoint": "/hangar", "method": "PUT", "body_template": { "..." : "..." } }
  }
}
```

## 4. Karten-Breakdown (für den Start, wenn die API bereit ist)

Seriell, wie gehabt — jede Karte mit Review-Child:

1. **C1 Manifest-Schema + Validator** — Schema-Typen in Rust (`crates/connector/`), strenge Validierung (https-only, file://-Block, Größenlimit), „Warnung ignorieren"-Flag, Dot-Path-Mapping-Engine mit Unit-Tests.
2. **C2 Settings-Plugin-Abschnitt** — Hinzufügen/Löschen/Aktivieren-Toggle, Auth-Felder nur bei Deklaration (UEX-Secret-Mechanik), Beispiel-Datei im Repo (`examples/connector-manifest.example.json`).
3. **C3 Generische Views** — Tabelle/Karten/Detail/Kalender-Komponenten, die aus Connector-Antworten + Manifest-Mapping rendern; Top-Level-„Dienste"-Bereich mit dynamischen Tabs.
4. **C4 Sync-Engine (L2)** — Pull/Push mit Master/Strategy, Replace-Confirm + Backup-Snapshot, Merge-Strategie, Polling + Cache-Anbindung.
5. **C5 Mock-Connector** — zeigt alle View-Typen gegen einen statischen Test-Endpoint (z. B. GitHub-Raw oder lokaler Fixtures-Server); E2E-Verifizierung.
6. **C6 AAWA-Connector** — erst wenn Platform-API + Keys existieren; Mapping auf die realen Endpoints (Hangar, Resourcenlager-L1-Sicht, Markt-L1, Events, Operation Center).

## 5. Bewusst out of scope (v1)

- L3-Funktionen (Anfragen, Chat, Lieferketten)
- Zentrale Plugin-Distribution (Remote-Repo-Pull wie module-overrides)
- Bidirektionaler Sync mit Konfliktauflösung (v1: ein Master, klare Richtung)
- iframe/HTML-Fremdrendering
