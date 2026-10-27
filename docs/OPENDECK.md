# Overlay-Taste auf Makro-Pad / Stream Deck / Tensor Deck

Jede belegbare Taste einer Stream-Deck-artigen Tastatur (OpenDeck, Elgato
Stream Deck, Tensor Deck, Logitech G Hub, Tastatur-Makros …) steuert das
Overlay über **dieselben zwei Wege**:

| Weg | Funktioniert auf | Aktionstyp auf dem Deck |
|---|---|---|
| **CLI ausführen** `arch-overlay …` | Linux (Wayland/X11) | „Open Application“ / „Run Command“ / Shell-Makro |
| **Hotkey drücken** `STRG+UMSCHALT+U` | Windows, X11 (global; unter Wayland unreliable → CLI-Weg) | „Press a key“ / Key-Press-Makro |

Hotkeys in der App: **STRG+UMSCHALT+U** = Spotlight ein/aus,
**STRG+UMSCHALT+O** = Click-through ↔ Fokus-Modus
(`src-tauri/src/overlay.rs`, `register_global_shortcuts`).

## 1. Der CLI-Befehl (Linux)

```bash
arch-overlay toggle        # Spotlight ein-/ausblenden
arch-overlay clickthrough  # Click-through <-> Fokus-Modus
arch-overlay show / hide   # eindeutig setzen
arch-overlay state         # Zustand abfragen (OK visible=… game_running=…)
```

Der Befehl spricht den Control-Socket der laufenden App an
(`$XDG_RUNTIME_DIR/architects-overlay.sock`); Details in `docs/WAYLAND.md`.

**In Deck-Aktionen immer den absoluten Pfad angeben**, z. B.
`/home/<user>/.local/bin/arch-overlay` — Deck-Werkzeuge starten oft ohne
Login-Shell und kennen dann kein `~/.local/bin` im PATH.

### Game-Gate (wichtig)

`toggle` ist ein **No-op, solange Star Citizen nicht läuft**: Das Overlay ist
ein Game-Begleiter, ein Fehldruck kann es also nicht still für die nächste
Spiel-Sitzung abschalten (`toggle_visible`, mit Unit-Test
`toggle_requires_running_game` abgesichert). Ein manuell gesetztes „aus“
überlebt Spiel-Neustarts, bis es wieder eingeschaltet wird.
Ist der Zustand unklar: `arch-overlay state` zeigt `game_running`.

## 2. OpenDeck (Linux) — auch Mirabox HSV293S & Co.

OpenDeck (`https://opendeck.app`) behandelt auch Nicht-Elgato-Geräte wie
Makro-Pads; das Mirabox HSV293S läuft über das Community-Plugin
`st.lynx.plugins.opendeck-akp153.sdPlugin`.

### Taste belegen (GUI)

1. OpenDeck öffnen, Zielgerät und Profil wählen.
2. Eine **freie** Taste wählen — nichts Bestehendes überschreiben.
3. Aktion **„Run Command“** (Plugin *Starter Pack*) wählen.
4. Feld **Key down**: `/home/<user>/.local/bin/arch-overlay toggle`
   (Label auf der Taste z. B. `AO`).
5. Taste testen: `arch-overlay state` zeigt danach den umgeschalteten Zustand
   (nur mit laufendem Star Citizen wirksam — Game-Gate, siehe oben).

Für Click-through eine zweite freie Taste mit
`…/arch-overlay clickthrough` belegen.

### Manuell in der Profil-JSON

OpenDeck speichert Profile **nicht** in einer `profiles.json`, sondern pro
Gerät: `~/.config/opendeck/profiles/<gerät-id>/<Profil>.json`
(z. B. `99-355499441494-293S/Default.json`). Der `keys`-Eintrag für die
Run-Command-Aktion:

```json
{
  "action": {
    "plugin": "com.amansprojects.starterpack.sdPlugin",
    "uuid": "com.amansprojects.starterpack.runcommand",
    "name": "Run Command",
    "controllers": ["Keypad", "Encoder"],
    "icon": "plugins/com.amansprojects.starterpack.sdPlugin/icons/runCommand.png",
    "property_inspector": "plugins/com.amansprojects.starterpack.sdPlugin/propertyInspector/runCommand.html",
    "supported_in_multi_actions": true,
    "visible_in_action_list": true,
    "disable_automatic_states": false,
    "encoder": null,
    "states": [ { "alignment": "middle", "background_colour": "#000000",
      "colour": "#FFFFFF", "family": "Liberation Sans",
      "image": "plugins/com.amansprojects.starterpack.sdPlugin/icons/runCommand.png",
      "image_scale": 100, "name": "", "show": true, "size": 16,
      "stroke_colour": "#000000", "stroke_size": 3, "style": "Regular",
      "text": "AO", "underline": false } ],
    "tooltip": "Architects Overlay umschalten"
  },
  "children": null,
  "context": "Keypad.<N>.0",
  "current_state": 0,
  "settings": { "down": "/home/<user>/.local/bin/arch-overlay toggle", "up": "" },
  "states": [ … identisch zu action.states, mit "text": "AO" … ]
}
```

Regeln für Hand-edit:

- **Vorher Backup**: `Default.json` → `Default.json.bak-<JJJJ-MM-TT>` im
  selben Ordner.
- Nur in einen **leeren** Slot (`null`-Eintrag im `keys`-Array) schreiben;
  `context` muss `Keypad.<index>.0` passend zum Array-Index sein.
- Editieren, wenn OpenDeck **nicht** läuft — OpenDeck schreibt Profile beim
  Beexit zurück; nach manuellem Edit OpenDeck neu starten.

## 3. Andere Decks / Makro-Werkzeuge

| Werkzeug | Empfohlene Aktion |
|---|---|
| Elgato Stream Deck App (Linux) | OpenDeck nutzen (deckt auch Elgato-Geräte ab) |
| Elgato Stream Deck App (Windows/macOS) | „Press a key“: `STRG+UMSCHALT+U` |
| Tensor Deck | Exec/Shell-Aktion (Linux) bzw. Key-Press (Windows): wie oben |
| Logitech G Hub | Makro „Ausführen“ (Linux) bzw. Tastatur-Makro `Ctrl+Shift+U` (Windows) |
| Tastatur-Makros (AutoHotkey …) | Windows: `^+u` → Overlay-Toggle; Linux: Exec des CLI |

### Windows-Hinweis (ehrlich)

Der `arch-overlay`-Client ist **Unix-only** (Control-Socket =
Unix-Domain-Socket, Wayland-Fallback-Konzept; `src-tauri/src/bin/arch-overlay.rs`).
Er wird in der Windows-Build-Pipeline zwar kompiliert, gibt dort aber nur
einen Hinweis aus und ist **nicht** im NSIS/MSI-Installer enthalten.
Auf Windows läuft das Overlay trotzdem und wird über die **globalen Hotkeys**
gesteuert — auf Decks also die Aktion als Key-Press `STRG+UMSCHALT+U`
(Toggle) bzw. `STRG+UMSCHALT+O` (Click-through) anlegen.

## 4. Fehlerbilder

| Symptom | Ursache / Abhilfe |
|---|---|
| Taste tut nichts, App läuft | Star Citizen läuft nicht → Game-Gate, normal |
| `Overlay nicht erreichbar` / `cannot reach overlay socket` | Overlay-App läuft nicht, oder Deck-Aktion ohne absoluten Pfad |
| Taste zeigt nichts / Aktion fehlt nach Hand-edit | OpenDeck nicht neu gestartet, oder `context` ≠ Array-Index |
| CLI-Aktion schlägt nur im Deck fehl | Relativer Befehl → absoluten Pfad `/home/<user>/.local/bin/arch-overlay` verwenden |
