# Live-Beleg: Overlay-Geometrie auf transformiertem Monitor gegen pinnte size-Regel (t_5133f3e9)

Setup (unveraendert!): Sebos Live-Hyprland 0.56.2 (Lua-Config) mit
`hl.window_rule({ match={class="architects-overlay"}, size={3440,1440}, move={0,0}, float, pin, stay_focused })`,
Monitore DP-3 `[0,0] 3440x1440 transform 0` und HDMI-A-2 `[1825,-1920] 1080x1920 transform 3`.
Build: `pnpm tauri build --no-bundle` auf feature/overlay-size-monitor (pointer-monitor-coords d731a4d
+ windowrule-class-fix 28189af cherry-pick + Compositor-Geometrie-Hebel).
Instanz isoliert (eigene XDG-Dirs, RT /tmp/ao51), Fake-Game via `cp /usr/bin/sleep`,
Trigger per Socket `arch-overlay show/hide`, Cursor per `hl.dsp.cursor.move`,
Messung per `hyprctl -j clients` (PID-Filter, Feld `at`).

## These / Fund t_317b9d15 bestaetigt

Zeitleiste nach `show` auf HDMI-A-2 (Cursor-Warp-Matrix, Fall HDMI-Ecke):

    +0.26s at=[1825,-1920] size=[3440,1440]   <- size-Regel feuert beim Map (statischer Effekt)
    +0.51s at=[1825,-1920] size=[1080,1920]   <- Reassert (Compositor-Dispatch, +400ms) korrigiert exakt
    +0.77..+2.03s bleibt [1080,1920] @ [1825,-1920]

Versteckter Fund aus der hide/show-Analyse: Hide/Show ist Re-Map, und GTK3 destroyed
dabei den xdg_toplevel komplett und baut ihn neu auf (WAYLAND_DEBUG: #42 -> #49). Die
pinnte 3440x1440-Regel greift daher bei JEDEM Show erneut — ein einmaliger Fix beim
ersten Map reicht nie.

## Warum client-seitiches set_size/maximize/fullscreen verliert (wire-Beleg, WAYLAND_DEBUG=1)

    [18:39:17.290714]  -> xdg_toplevel#49.set_maximized()      # tao/tauri sendet wirklich
    [18:39:17.594242] xdg_toplevel#49.configure(3440, 1440, ...) # Hyprland konfiguriert weiter Regel-Groesse
    [18:39:18.285487]  -> xdg_toplevel#49.set_maximized()
    [18:39:19.486918]  -> xdg_toplevel#49.set_fullscreen(nil)  # Request geht raus
    ... fullscreenClient bleibt 0 -> kommentarlos verworfen (pinned floating Fenster)

`xdg_toplevel` kennt per Design kein client->compositor Resize-Gesuch nach dem Map;
maximize/fullscreen erreichen den Compositor, werden fuer dieses Fensterfensterprofil
aber ignoriert. Die Fenster-Groesse kann auf Wayland nur der Compositor setzen.

## Der funktionierende Hebel: Hyprlands eigene Dispatcher auf die eigene PID

    hyprctl dispatch 'hl.dsp.window.resize({ x = 1080, y = 1920, window = "pid:<APP-PID>" })'
    hyprctl dispatch 'hl.dsp.window.move({ x = 1825, y = -1920, window = "pid:<APP-PID>" })'

- `resize` wirkt (configure(1080,1920) auf dem Wire), verlagert aber den Anker
  (live beobachtet: +1180,-240 Shift) -> deshalb resize DANN move.
- `move` ist absolut im globalen Layout-Raum; Monitorposition == Fensterziel.
- `window="pid:..."` Selector ohne gemapptes Fenster ist ein verifizierter No-op
  (getestet gegen aktives Fenster: keine Kollateralschaeden).
- Reihenfolge/Staffelung ueber `reassert_place_after_map` (+400ms, +900ms), da der
  Wayland-Map async ist (bekannte Race aus der compositor_focus-Fix).

## Matrix mit finaler App (gemessen, hyprctl -j clients PID-Filter)

| Fall | Cursor (Layout-px) | at | size | Mon |
|---|---|---|---|---|
| HDMI-Mitte | (2365,-960) | [1825,-1920] | 1080x1920 | 1 |
| DP-3-Mitte (Referenz) | (1720,720) | [0,0] | 3440x1440 | 0 |
| HDMI-Ecke | (1900,-1850) | [1825,-1920] | 1080x1920 | 1 |

Nach Re-Map ohne Dispatch steht das Fenster wieder auf 3440x1440 (Regel) und wird
vom naechsten Reassert-Zyklus wieder korrigiert — Verhalten ist pro-Show stabil.

## Visuelle Verifikation

Screenshots grim -o HDMI-A-2 (1080x1920). OCR-Band (magick -negate + tesseract,
TSV/2 = Monitor-px): Panel-Tabs COMMODITIES 208..382, ITEMS 508..583, LOCATIONS
709..849 -> Panelband-Mitte ~528 ~ Monitor-Mitte 540, alle Elemente innerhalb
0..1080, `esc`-Badge rechts bis x=983 vollstaendig im Bild. (vision_analyze lief
gegen Provider-Timeout; OCR+Geometrie als Ground-Truth.)

Dateien: matrix-final.txt (Rohdump), wire-excerpt.txt (WAYLAND_DEBUG-Auszug),
1-hdmi-center-hdmi.png / 3-hdmi-corner-hdmi.png (HDMI-Crop nach Fix),
2-dp3-center.png (DP-3 Referenz, Vollbild).
