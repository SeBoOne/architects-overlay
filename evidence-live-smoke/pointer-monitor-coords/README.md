# Live-Beleg: place_on_pointer_monitor — X11-Canvas vs. Hyprland-Layout (t_317b9d15)

Setup: Omarchy/Hyprland (Lua-Config), XWayland :0. Monitore (hyprctl monitors):
- DP-3 `AOC CU34G2XP` [0,0] 3440x1440 transform 0
- HDMI-A-2 `PHL 243V5` [1825,-1920] 1080x1920 **transform 3** (rotiert)

X11-Canvas (xrandr gegen XWayland): Screen 4520x1920, `DP-3 3440x1440+0+0`,
`HDMI-A-2 1080x1920+3440+0 right` — XWayland ordnet also ENTGEGEN dem Hyprland-Layout
(Nebenordnung statt "HDMI oben"), negative Layout-Koordinaten existieren auf dem Canvas nicht.

Messmatrix (identisch für beide Läufe): isolierte Instanz (eigene XDG-Dirs), Fake-Game
(cp /usr/bin/sleep /tmp/StarCitizen.exe), Cursor-Warp per `hl.dsp.cursor.move`, Trigger per
`arch-overlay show` (Unix-Socket), Fenster-Geometrie per `hyprctl -j clients` (PID-Filter).

## run1 — Altbestand (Cursorquelle x11rb query_pointer), instrumentiert
Alle 5 Proben (DP-3-Mitte, HDMI-Mitte, HDMI-links-oben, DP-3-unten-rechts, DP-3-links-oben):
X11 meldete STets `cursor_x11=(3440,1919)` (veralteter XWayland-Canvas-Zustand — X-Pointer
updated nur beim Überqueren von X-Client-Flächen), keine Containment, 5x primary-Fallback.
=> Mismatch real, reproduzierbar.

## run2 — Fix (Cursorquelle hyprctl -j cursorpos, Layout-Raum)
`place:` wählt 5/5 korrekt (inkl. transform-3-Monitor mit negativen y):
cursor (1720,720)->CU34G2XP@0,0 | (2365,-960)->PHL 243V5@1825,-1920 | (1900,-1850)->PHL 243V5
| (3430,1430)->CU34G2XP | (100,50)->CU34G2XP. Cargo-Test `contains_point_matches_measured_hyprland_layout` ok.

## Nebenbefund (Review-Entscheidung, eigene Karte t_...)
Hyprland-Live-Config pinnt per window_rule size {3440,1440} + move {0,0} (monitor-relativ)
für class architects-overlay -> client-seitiges set_size/set_position wird uebersteuert;
auf HDMI-A-2 (1080x1920) bleibt das Fenster 3440x1440 zu gross. Monitor-Zuordnung selbst
stimmt (monitor=1, at=[1825,-1920]). Repo-docs (WAYLAND.md) nennen nur die alte 660x600-Regel.
Follow-up: per-Monitor-Regel oder App-seitige Groessen-Umsetzung noetig.
