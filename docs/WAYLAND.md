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

## Fensterregeln (optional)

Damit Hyprland das Overlay wie gewünscht behandelt (floating, always on top,
ggf. input-transparent per Regel statt per App-API):

```ini
windowrulev2 = float, class:^(architects-overlay)$
windowrulev2 = pin, class:^(architects-overlay)$
# Input-Transparenz erzwingen, solange die App nicht in den Fokus-Modus schaltet:
windowrulev2 = noinput, class:^(architects-overlay)$
```

Hinweis: `noinput` macht das Fenster pauschal klick-durchlässig. Wer den
Fokus-Modus (Klicken im Overlay) nutzen will, lässt die Regel weg und steuert
den Modus über `arch-overlay clickthrough`.

## Beenden-Verhalten

- Erkennt die App, dass Star Citizen beendet wurde, versteckt sie das
  Spotlight-Fenster wieder (der letzte `user_visible`-Zustand bleibt für die
  nächste Session erhalten).
- Beim App-Exit wird der Socket-Datei-Eintrag entfernt; nach einem Crash
  entfernt der nächste Start die alte Socket-Datei automatisch.
- Sauber beenden: Fenster-Close des Hauptfensters oder `kill` des Prozesses;
  es gibt keinen Daemon-Modus außerhalb der App.
