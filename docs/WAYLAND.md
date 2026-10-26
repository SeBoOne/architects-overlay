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
auf den kompletten Monitor unter dem Pointer (`place_on_pointer_monitor`), die
CSS zeichnet die sichtbare Shell kompakt darin (feste Breite
`min(max(0.70·vw, 640), 0.94·vw)`, Max-Höhe 84 vh, innerer Scroll). Eine
feste `size`-Regel ist deshalb überflüssig bis kontraproduktiv — früheres
`size 660 600` ist obsolet.

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
