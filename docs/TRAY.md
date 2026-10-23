# Tray-Icon (Overlay-Toggle / Config öffnen / Beenden)

Das Overlay registriert ein System-Tray (StatusNotifier, libayatana) mit drei
Menüpunkten. Implementiert in `src-tauri/src/tray.rs`, Icon
`src-tauri/icons/tray-amber-32.png` (Amber-Diamond, Palette-Token `--accent`
`#f2a33c`, per `include_bytes!` eingebettet).

## Menü

| Punkt | Wirkung |
|---|---|
| Overlay ein-/ausblenden | `overlay::toggle_visible` — identisch zum Hotkey (No-op ohne laufendes Spiel). |
| Config öffnen | `overlay::open_config_from_tray` — zeigt das Overlay **auch ohne laufendes Spiel** (`config_open`-Flag in `OverlayState`, siehe unten) und sendet `tray-open-config`; das Frontend öffnet dieselbe Config-View wie beim Zahnrad-Button. |
| Beenden | `app.exit(0)` — sauberer Exit mit Code 0. |

Fehlt ein Tray-Daemon (kein StatusNotifier-Provider), schlägt die Registrierung
fehl **soft**: die App läuft ohne Tray weiter (`log::warn!`), alle Funktionen
bleiben über Hotkeys und den `arch-overlay`-Socket erreichbar.

## Beenden vs. systemd-user-unit (Kernpunkt der Triage t_ee57b8d6)

Das Overlay läuft normalerweise als transiente systemd-User-Unit
(`architects-overlay.service`, gestartet via `systemd-run --user`). Verifiziert
in der Triage: die Unit hat **`Restart=no`** (`systemctl --user show`).

Daraus folgt für Tray-„Beenden":

- `app.exit(0)` beendet den Main-Process mit **Exit-Code 0**.
- systemd sieht einen sauberen Exit → die Unit deaktiviert sich normal.
- **Kein Restart-Loop** (restart-policy greift nicht bei Exit 0, und `Restart=no`
  startet ohnehin nie neu). Kein `systemctl`-Subprocess nötig.
- Live-Test (feature/tray-icon): dbusmenu-Click auf „Beenden" → Process weg,
  Exit-Code 0, keine Neustart-Aktivität; parallel laufende zweite Instanz
  (Prod-Unit) blieb unberührt — Tray-Quit beendet nur die eigene Instanz.

**Upgrade-Pfad** (falls später `Restart=on-failure` gewünscht wird, z. B. für
Crash-Restart): dann MUSS der Tray-Exit zwingend Code 0 bleiben (wie oben), und
Signal-Exits (SIGTERM bei `systemctl stop`) dürfen nicht als Failure zählen
(`RestartForceExitStatus`/`SuccessExitStatus` ergänzen). Tray-„Beenden" als
`systemctl --user stop`-Subprocess (Triage-Option B) ist nur nötig, wenn die
App selbst die Unit-Verwaltung übernehmen soll — für v1 bewusst nicht umgesetzt.

## Config-ohne-Spiel (Fenster-Sichtbarkeit)

`OverlayState` hat ein neues Flag `config_open`:

- Effektive Sichtbarkeit: `config_open || (user_visible && game_running)`.
- Tray-„Config öffnen" setzt `config_open`, zeigt das Fenster und emittet
  `tray-open-config` → Frontend öffnet `pane-config`.
- Schließt der User die Config (Esc / Back-Pfeil), ruft das Frontend
  `overlay_config_closed` auf → `config_open` wird geräumt; war das Fenster
  nur wegen der Config offen (kein Spiel), wird es wieder verborgen.
  Läuft das Spiel, übernimmt die normale `user_visible && game_running`-Logik.

## Tests

- `cargo fmt / check / test` (93 Tests grün), `pnpm check / build`.
- Live: StatusNotifierWatcher-Registrierung (`tray_icon_tray_app_architects_overlay_tray`),
  dbusmenu-Layout gelesen, Icon-Pixmap 100 % identisch zum eingebetteten PNG,
  dbusmenu-Clicks „Config öffnen" (Fenster gezeigt + Fokus) und „Beenden"
  (Exit 0) ausgeführt.
- Frontend: `docs/scripts/tray-icon-probe.py` (T1–T4: Listener-Draht,
  Tray-Event öffnet Config, ESC → `overlay_config_closed`, Zahnrad-Regression).

## Windows 11 (spätere Pipeline)

Tray-Feature identisch; die systemd-Semantik entfällt. Icon muss vom User in
den sichtbaren Bereich gepinnt werden (Overflow-Area). Autostart später via
`tauri-plugin-autostart` (bewusst nicht diese Runde).
