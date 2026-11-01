# Konfigurierbare globale Tastenkürzel (C2a — t_6565fe8b)

Zwei globale Kürzel sind seit dieser Karte konfigurierbar:

| Slot | Wirkung | Default |
|---|---|---|
| Overlay-Toggle | Spotlight ein/aus (nur mit laufendem Star Citizen) | `Ctrl+Shift+U` |
| Click-through | Maus durchs Overlay ⇄ Fokus-Modus | `Ctrl+Shift+O` |

## Kanonische Quelle

`<config-dir>/architects-overlay/config.toml`:

```toml
[shortcuts]
toggle = "Ctrl+Shift+U"
click_through = "Ctrl+Shift+O"

[shortcuts.mode_hotkeys]
search = "F1"
market = "F2"
routes = "F3"
inventory = "F4"
hangar = "F11"
settings = "F12"
```

Die Strings sind **kanonisch normalisiert**: Mod-Reihenfolge fix `Ctrl` → `Alt`
→ `Shift` → `Super`, Haupttaste zuletzt, Großbuchstaben-Tokens. Sie sind
1:1 lesbar und direkt im `global_hotkey`-Format parsebar (derselbe Parser,
den `tauri-plugin-global-shortcut` unter Windows/X11 nutzt). **Diese Datei
ist die einzige kanonische Quelle** — auch für Compositor-Binder (unten).

## Settings-View (F12)

- Zwei Felder mit **Key-Capture**: „Ändern“ → Kombination drücken → sofortige
  Prüfung und Übernahme; „Abbrechen“ oder **Esc** bricht die Capture ab.
- **Aktiv-Zeile zeigt den Backend-Wert (Soll-Ist)**, nicht den Entwurf:
  fehlgeschlagene Änderungen ändern den gezeigten aktiven Wert nicht.
- Registrierungszustand wird ausgewiesen: unter Wayland registriert das OS
  keine globalen Hotkeys (`global_shortcuts_active = false` — ehrlicher
  Hinweis im View), die gespeicherte Kombination bleibt gültig.

## Validierung (Backend, Fehler erscheinen im Feld)

1. ≥1 Mod-Taste **und** genau 1 Haupttaste (Strg/Alt/Shift/Win + Taste).
2. **Escape als Kombination abgelehnt** (Esc = Capture-Abbruch / UI-Zurück).
3. Keine Doppelvergabe: beide Felder dürfen nicht dieselbe Kombination haben.
4. Für globale Kürzel gesperrt (Konfliktliste in der Meldung): die F-Tasten
   sind die Mode-Hotkeys (siehe unten, konfigurierbar), `F5` führt den
   Resync aus; weitere interne Belegungen: `Esc`, `Enter`, `Tab`,
   Pfeiltasten, `m`, `Alt+Zahl` — die In-App-Handler prüfen keine
   Mod-Tasten, dieselbe Taste global zu binden würde immer beide Aktionen
   auslösen.

## Mode-Hotkeys (PB — t_1c9bc240)

Die In-App-Modi wechseln per F-Taste (nur bei sichtbarem/fokussiertem
Overlay — keine System-Binds). Belegung konfigurierbar in den Settings
(Abschnitt „Mode-Hotkeys", Capture wie bei den globalen Kürzeln):

| Modus | Default | änderbar |
|---|---|---|
| Suche | `F1` (fix) | nein |
| Marktplatz | `F2` | ja |
| Handelsrouten | `F3` | ja |
| Inventar | `F4` | ja |
| Hangar | `F11` | ja |
| Einstellungen | `F12` (fix) | nein |

Änderbare Slots: `hangar`, `routes`, `inventory`, `market` — geändert wird
in den Settings per Key-Capture (`settings_set_mode_hotkey`), Zurücksetzen
über `settings_reset_mode_hotkeys`. F1 und F12 sind fix.

- **F5 ist fest reserviert** für den kontextsensitiven **Resync** und wirkt
  NUR, wo Resync relevant ist: Marktplatz-Angebote (echter Live-Fetch,
  `force_refresh`), Mein Shop (User-scoped Pull) und Routen-Sweep
  (`sync_now`). Überall sonst macht F5 nichts und der Hinweis fehlt.
- **Migration:** Configs ohne `[shortcuts.mode_hotkeys]` (Nutzer ohne eigene
  Anpassung) landen automatisch auf der neuen Belegung; ungültige Werte
  (Doppelbelegung, F5, kein F-Key) fallen beim Laden auf die Defaults
  zurück.
5. Unbekannte/Ziffernblock-Tasten abgelehnt; ungültige Hand-Edit-Werte in der
   config fallen beim Laden auf die Defaults zurück (App bootet trotzdem).

## Registrierung (Windows/X11) — Re-Registrierung ohne Neustart

Beim Speichern: **alte Kombination deregistrieren → neue registrieren →
Bindings tauschen**. Scheitert die Registrierung (Belegung durch eine andere
App — typischer Windows-Fall), wird die **alte Kombination re-registriert**,
der Fehler sichtbar im Settings-Feld angezeigt und **nichts persistiert**:
alter Kürzel bleibt aktiv. Handler-Dispatch läuft über die Live-
`ShortcutBindings`, nicht über closures — ein Rebind braucht keinen
Plugin-Neubau. Beim Start werden die persistierten Werte registriert; eine
Kollision beim Start ist im Log sichtbar und beim nächsten Rebind im UI.

## Wayland (Omarchy) — Hinweis für C2b

Unter Wayland initialisiert das Plugin nicht (X11-only); toggles laufen dort
über Hyprland-Binds auf `arch-overlay toggle|clickthrough` (Socket-CLI,
docs/WAYLAND.md). C2b liest die `[shortcuts]`-Werte aus der config und
erzeugt daraus die Binder. Mapping-Tabelle Token → Hyprland:

| config-Token | Hyprland |
|---|---|
| `Ctrl` | `CTRL` |
| `Alt` | `ALT` |
| `Shift` | `SHIFT` |
| `Super` | `SUPER` |
| `U`/`F9`/`Space` … | Taste direkt (`U`, `F9`, `Space`) |

Beispiel bei Defaults: `bind = CTRL SHIFT, U, exec, /home/sebo/.local/bin/arch-overlay toggle`.

**Wichtig:** Solange die Binder auf fest `CTRL SHIFT, U` stehen, muss ein
geänderter Settings-Wert in Sebo's Hyprland-Konfig nachgezogen werden
(C2b-Aufgabe) — die config-Werte sind ab jetzt maßgeblich.
