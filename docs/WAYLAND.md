# Wayland-Bedienung (Hyprland)

Global Shortcuts (`tauri-plugin-global-shortcut`) sind X11-basiert und
funktionieren unter reinem Wayland nicht zuverlässig. Ebenso achten einige
Compositoren die von der App gesetzten Attribute (`ignore cursor events`,
`always-on-top`) nur eingeschränkt. Deshalb gibt es für Wayland-Sessions den
Weg über den **Control Socket**.

## Control Socket

Die App legt bei jedem Start einen Socket an:

```
$XDG_RUNTIME_DIR/architects-overlay.sock   (mode 0700, nur gleicher User)
```

Kommandos (eine Zeile): `toggle`, `show`, `hide`, `clickthrough`, `state`.
Antwort: `OK <state>` oder `ERR <reason>`.

## CLI-Client

Mit dem App-Build wird `arch-overlay` mitgebaut (src-tauri/src/bin/):

```sh
arch-overlay toggle        # Spotlight ein-/ausblenden
arch-overlay clickthrough  # Click-through <-> Fokus-Modus
arch-overlay state         # aktuellen Zustand anzeigen
```

## Bind-Sync über OmaSettings (Omarchy-Pfad, C2b)

In den Einstellungen (F12 → Tastenkürzel) gibt es den Consent-Chip
**„Tastenkürzel mit System (Omarchy) synchronisieren“** (Default aus).
Ist er aktiv und läuft Hyprland mit dem OmaSettings-Plugin
(`~/.config/omarchy/plugins/io.github.twiking.omasettings/bin/omasettings`),
führt die App genau zwei Binds des Users:

| Kürzel (Settings) | Bind-Kommando | OmaSettings-Beschreibung |
| --- | --- | --- |
| Overlay-Toggle | `arch-overlay toggle` | Architects Overlay: Palette ein-/ausblenden |
| Click-through | `arch-overlay clickthrough` | Architects Overlay: Click-through umschalten |

Ablauf bei Änderung oder Start-Drift (Store ≠ Settings):

1. Backup `~/.config/omarchy/omasettings.json.bak-<YYYY-MM-DD>` (einmal pro Tag, vor dem ersten Schreibvorgang).
2. Änderungen laufen **nur** über das Plugin-CLI (positionale Argumente, nie Flags, nie direktes JSON-Editing):
   `omasettings keys remove "<ALT>"` → (nur falls der alte Bind schon deaktiviert war) `omasettings keys disable "<ALT>"` → `omasettings keys add "<NEU>" "<Beschreibung>" "<Kommando>"`.
   Das Plugin aktualisiert dadurch Store + `~/.config/hypr/bindings.lua` (Marker-Block `-- >>> omasettings bindings`) und ruft selbst `hyprctl reload` auf.
3. Verifikation: `hyprctl binds -j` muss den Bind mit korrekter Taste + Modmaske zeigen (SHIFT=1, CTRL=4, ALT=8, SUPER=64) und `assert(loadfile(bindings.lua))` muss durchgehen; Fehltreffer melden die App direkt in der Settings-Note.

Das Kürzelformat der Settings (`Ctrl+Shift+U`) wird 1:1 in die
Hyprland-Keysym-Schreibweise übersetzt (`CTRL + SHIFT + U`; `Enter`→`RETURN`,
`PageUp`→`PAGE_UP`, `ArrowUp`→`UP`, `Quote`→`APOSTROPHE` … — live gegen
Hyprland 0.56 mit `hyprctl eval 'hl.bind(...)'` validiert).

Fremde Binds bleiben unangetastet: Nur die zwei `arch-overlay …`-Einträge
werden ersetzt; ein alter Combo, den OmaSettings bereits deaktiviert hatte,
bleibt deaktiviert (= altes Kürzel tot), ein vorher nicht deaktivierter wird
nicht nachträglich stummgeschaltet.

## Manueller Weg ohne OmaSettings-Plugin (generisches Hyprland)

Findet die App das Plugin nicht, schreibt sie **nichts** in fremde
Konfigurationen. Die Binds dann selbst anlegen (ini-Syntax):

```ini
# ~/.config/hypr/hyprland.conf (bzw. bindings.conf)
bind = CTRL SHIFT, U, exec, arch-overlay toggle
bind = CTRL SHIFT, O, exec, arch-overlay clickthrough
```

Lua-Syntax (Hyprland ≥ 0.55, ohne OmaSettings-Helfer):

```lua
hl.bind("CTRL + SHIFT + U", hl.dsp.exec_cmd("arch-overlay toggle"),
        { description = "Architects Overlay: Palette ein-/ausblenden" })
hl.bind("CTRL + SHIFT + O", hl.dsp.exec_cmd("arch-overlay clickthrough"),
        { description = "Architects Overlay: Click-through umschalten" })
```

Nach `hyprctl reload` gegentesten: `hyprctl binds -j | grep arch-overlay` —
der Eintrag muss die neue Taste/modmask zeigen; `arch-overlay state` antwortet
auf `$XDG_RUNTIME_DIR/architects-overlay.sock`.

## Hyprland-Keybinds

```ini
# ~/.config/hypr/hyprland.conf
bind = CTRL SHIFT, U, exec, arch-overlay toggle
bind = CTRL SHIFT, O, exec, arch-overlay clickthrough
```

## Fensterregeln (Hyprland)

Seit 0.3.2 pinnt die App ihre WM-Klasse selbst auf exakt `architects-overlay`
(kleingeschrieben): `lib.rs::pin_wm_class` wird in `setup` (nach GTK-Init,
vor dem ersten Fenster-Map) aufgerufen und setzt GDK-Programmklasse und
prgname explizit. Grund: tao erzeugt Linux-Fenster über GTK, und GDK schreibt
die WM_CLASS beim Display-Open aus dem prgname groß (`Architects-overlay`) —
erst das Window-Realize liest die (dann gepinnte) globale Programmklasse.
Hyprland matcht `windowrulev2`-Matcher case-sensitiv — alle Regeln unten
passen deshalb zur realen app_id, wie sie `hyprctl clients -j` zeigt.

Die Overlay-Geometrie verwaltet die App selbst: jedes Show spannt das Fenster
auf den kompletten Monitor unter dem Pointer (`place_on_pointer_monitor`,
nach dem Map zusätzlich per `reassert_place_after_map` nachgesetzt), die CSS
zeichnet die sichtbare Shell kompakt darin (feste Breite
`min(max(0.70·vw, 640), 0.94·vw)`, Max-Höhe 84 vh, innerer Scroll). Eine
feste `size`-Regel ist deshalb überflüssig bis kontraproduktiv — früheres
`size 660 600` ist obsolet.

### `size`-Regeln und Multi-Monitor (der Fall t_5133f3e9)

Hyprlands `size` und `move` sind *statische* Effekte: sie werden einmal pro
Fenster-Öffnen ausgewertet — und Hide/Show der App ist unmap/re-map, also ein
neues Öffnen: die Regel greift bei **jedem** Show (gemessen: nach jedem Re-Map
steht das Fenster wieder auf der pinnten Größe). Der Client kann dagegen
protokollseitig nicht ankommen: `xdg_toplevel` kennt kein Resize-Gesuch nach
dem Map (wire-belegt via `WAYLAND_DEBUG`, t_5133f3e9; `set_maximized`/
`set_fullscreen` erreichen Hyprland zwar, werden für ein pinned floating
Fenster aber kommentarlos verworfen). Die App setzt die Geometrie deshalb mit
Hyprlands eigenen Dispatchern (`hl.dsp.window.resize` + `hl.dsp.window.move`,
`window = "pid:<eigene PID>"`) — nach dem Map gestaffelt in
`reassert_place_after_map`. Belegmatrix: `evidence-live-smoke/overlay-size-monitor/`.

Die dangeröse Variante ist eine fixierte Global-Regel wie
`size = { 3440, 1440 }` auf einem Multi-Monitor-Setup: auf einem kleineren
oder rotierten Monitor (z. B. 1080×1920, transform 3) bleibt das Fenster
breiter als der Monitor und die zentrierte CSS-Shell ragt aus dem Screen.

Falls überhaupt eine `size`-Regel gewünscht ist, dann monitor-adaptiv über
die Monitor-Variablen der Regel-Ausdrücke (`monitor_w`/`monitor_h` sind
monitor-lokal und werden pro Map aufgelöst):

```lua
hl.window_rule({
  match = { class = "architects-overlay" },
  -- Größe auf den Monitor, auf dem das Fenster gemappt wird:
  size  = { "monitor_w", "monitor_h" },
  move  = { 0, 0 },
  float = true,
  -- KEIN pin (v2, t_6d26985e): das Overlay bleibt im Workspace, in dem
  -- es geöffnet wurde, und verschwindet mit ihm beim Wechsel.
})
```

(Entsprechend ini-AltSyntax: `windowrulev2 = size monitor_w monitor_h,
class:^(architects-overlay)$` — unter Hyprland ≥0.55 ist Lua die
unterstützte Konfiguration.)

Ohne jede `size`-Regel entscheidet der GTK-Default-Size-Hint beim ersten Map,
Danach die App — das ist der empfohlene Zustand: die Regel `float` reicht, die
Geometrie kommt von der App.

### Verhalten seit v0.5.x/v2: Workspace-Kohärenz PASSIV, keine aktiven Dispatches (t_6d26985e)

**Geschichte (warum):** v0.5.x (PA, t_d340f851) hat das Overlay AKTIV an den
SC-Workspace gebunden: jeder Show capturte den SC-Workspace, ein
socket2-`workspace>>`-Listener versteckte das Overlay, sobald der Nutzer den
Workspace verließ — und jeder Hide dispatchte den Fokus zurück ans Spiel. Der
Nutzer wurde bei JEDEM Workspace-Wechsel-Versuch in den SC-Workspace
zurückgeworfen (Discord/Browser im Spiel unmöglich, 2. Monitor
unerreichbar). In v0.4.6 reverted; aktive Workspace/Focus-Dispatches aus dem
Overlay-Prozess sind dauerhaft VERBOTEN (`workspace`, `focuswindow` als
Workspace-Korrektur, `movetoworkspacesilent` — kein Polling, das den
Workspace „korrigiert").

**v2 ist strikt passiv:**

- **Kein `pin`** — weder statisch (hyprland.lua) noch zur Laufzeit. Das
  Overlay ist ein normales float-Fenster: es mapped auf dem Workspace, auf
  dem der Nutzer sich BEIM EINBLENDEM befindet, und verschwindet mit diesem
  Workspace beim Wechsel (Standard-Hyprland). Das Overlay sichtbar machen
  heißt NICHT: zum SC-Workspace wechseln — dorthin zu wechseln ist Sache des
  Nutzers. Wer das Overlay sehen will, blendet es im aktuellen Workspace ein.
- **Keine Workspace-Reaktion:** Die App lauscht NICHT auf
  `workspace>>`/`focusedmon…`-Events und pollt den Workspace nicht. Es gibt
  keinen Code-Pfad mehr, der Sichtbarkeit von Workspaces abhängig macht
  (Regression-Test `effective_visible_ignores_workspaces` +
  `no_pin_dispatches_exist` in `overlay.rs`).
- **stay_focused-Brücke bleibt:** Solange das Overlay sichtbar ist, hält
  `stay_focused` (gesetzt per Lua-Dispatcher nach dem Map auf die eigene
  Fenster-Adresse) den Tastaturfokus gegen den Fokus-Raub des Spiels. Das
  ist das Gegenteil des PA-Problems und war nie Ursache der globalen
  Eingabe-Blockierung.
- **Dialog-Suspend:** Öffnet die App einen eigenen Dialog (Bild-File-Chooser,
  gleiche WM-Class + pid), suspendiert sie `stay_focused` auf dem
  Hauptfenster, solange der Dialog offen ist, und stellt es danach wieder
  her. Ohne `pin` ist der Picker ohne jeden keep-above-Krieg normal
  bedienbar (P3).
- **Öffnen = Fokus-Modus:** Beim Einblenden schaltet das Overlay strikt in
  den Fokus-Modus (auch wenn vorher Click-through aktiv war); der
  Status-Button unten rechts zeigt konsistent `FOKUS` (P4, aus PA
  übernommen).
- **Eingabe-Blockierung (P2):** Wayland/Hyprland bietet KEINE Möglichkeit,
  Pointer-Eingabe-Blockierung eines Wayland-Fensters auf einen Monitor zu
  begrenzen — die Eingabe-Form folgt dem Fenster (und das Overlay spannt
  bereits genau EINEN Monitor, den unter dem Pointer). Beleg: Hyprland
  windowrule-Vokabular kennt nur fenster-/klassen-globale Input-Regeln
  (`noinput`), keine monitor-gestreuten; `hyprctl monitors`/`clients` expose
  keine per-Monitor-Input-Zonen (Probe `pz-probe`, Check p2). Pragmatische
  Lösung (Standard): das Fenster liegt nur auf EINEM Monitor, beim Öffnen
  Fokus-Modus nur auf diesem Monitor; alle anderen Monitore bleiben frei.
  Click-through ist weiterhin per `arch-overlay clickthrough` / Kürzel
  schaltbar; ein Fokus-Wechsel passiert nur durch Öffnen (Nutzer-Intent) oder
  expliziten Nutzerklick.

Empfohlene Regeln (hyprland.conf, ini):

```ini
windowrulev2 = float, class:^(architects-overlay)$
windowrulev2 = nodecorate, class:^(architects-overlay)$
# KEIN pin, KEIN stay_focused statisch nötig — die App setzt stay_focused
# dynamisch nach dem Map (Dialog-Suspend inklusive).
# ACHTUNG (t_1042447d): stay_focused darf NIE statisch auf die Klasse gesetzt
# werden. In-Process-GTK-Dialoge (WebKITGTK-Filechooser der Plugin-/Bild-
# Importe) teilen pid + WM_CLASS mit dem Overlay und erben die Rule bei jedem
# Map — zwei stay_focused-Fenster duellieren sich um den Fokus bei jedem
# Klick (Filepicker/Overlay-Flackern). Die App disarmt beim Dialog-Suspend
# alle Fenster ihrer Klasse, aber die statische Rule bleibt trotzdem
# kontraproduktiv und wird nicht empfohlen.
# Optional, wenn das Overlay pauschal klick-durchlässig sein soll:
# windowrulev2 = noinput, class:^(architects-overlay)$
```

Pins (M5, t_d10a0d84): Pin-Boxen sind DOM im Spotlight-Webview — es gibt
KEINE eigenen `pins-<idx>`-Fenster mehr (M4-Ansatz verworfen, siehe
FENSTERMODUS-PLAN.md §6-Revision). Obige class-basierten Regeln treffen
nur `spotlight` und `app-window`; nichts weiter nötig. (Historisch, M4:
Pin-Fenster teilten dieselbe WM_CLASS — der Abschnitt ist obsolet.)

hyprland.lua (Omarchy, wie in `~/.config/hypr/hyprland.lua`):

```lua
hl.window_rule({
  match = { class = "architects-overlay" },
  float = true,
  decorate = false,
  rounding = 0,
  no_blur = true,
  -- pin entfallen (v2): das Overlay bleibt im Workspace, in dem es geöffnet
  -- wurde. stay_focused setzt die App dynamisch (Brücke + Dialog-Suspend).
  -- stay_focused hier NICHT setzen (t_1042447d): die Rule matcht auch unsere
  -- in-process GTK-Filechooser (gleiche Klasse) — Fokus-Duell, siehe oben.
})
```

Verifikation nach dem Setzen: `hyprctl clients -j` muss für das
Spotlight-Fenster `floating=true` und Vollmonitor-Größe zeigen und — nach dem
Map — `pinned=false` (v2: niemals gepinnt).

## Beenden-Verhalten

- Erkennt die App, dass Star Citizen beendet wurde, versteckt sie das
  Spotlight-Fenster wieder (der letzte `user_visible`-Zustand bleibt für die
  nächste Session erhalten).
- Die Socket-Datei wird beim App-Exit **nicht** entfernt — weder bei
  Fenster-Close noch bei `kill` (SIGTERM). Sie bleibt an Ort und Stelle und
  wird erst beim **nächsten App-Start** beim Binden automatisch weggeräumt
  (`remove_file` vor `bind`, gleiches Verhalten nach einem Crash).
- Sauber beenden: `kill`/Beenden des Prozesses genügt; es gibt keinen
  Daemon-Modus außerhalb der App.
