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
  pin   = true,
})
```

(Entsprechend ini-AltSyntax: `windowrulev2 = size monitor_w monitor_h,
class:^(architects-overlay)$` — unter Hyprland ≥0.55 ist Lua die
unterstützte Konfiguration.)

Ohne jede `size`-Regel entscheidet der GTK-Default-Size-Hint beim ersten Map,
Danach die App — das ist der empfohlene Zustand: die Regeln oben (float/pin/
stay_focused) reichen, die Geometrie kommt von der App.

### hyprland.conf (ini)

```ini
windowrulev2 = float, class:^(architects-overlay)$
windowrulev2 = pin, class:^(architects-overlay)$
windowrulev2 = nodecorate, class:^(architects-overlay)$
# Das Spiel (Wine/Proton, fullscreen) fordert den Fokus ständig zurück —
# stay_focused hält die Palette im Fokus, solange sie sichtbar ist.
windowrulev2 = stay_focused, class:^(architects-overlay)$
# Input-Transparenz erzwingen, solange die App nicht in den Fokus-Modus schaltet:
# windowrulev2 = noinput, class:^(architects-overlay)$
```

### hyprland.lua (Omarchy, wie in `~/.config/hypr/hyprland.lua`)

```lua
hl.window_rule({
  match = { class = "architects-overlay" },
  float = true,
  pin = true,
  decorate = false,
  rounding = 0,
  no_blur = true,
  stay_focused = true,
})
```

Verifikation nach dem Setzen: `hyprctl clients -j` muss für das Spotlight-Fenster
`floating=true`, `pinned=true` und Vollmonitor-Größe zeigen.

Hinweis: `noinput` macht das Fenster pauschal klick-durchlässig. Wer den
Fokus-Modus (Klicken im Overlay) nutzen will, lässt die Regel weg und steuert
den Modus über `arch-overlay clickthrough`.

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
