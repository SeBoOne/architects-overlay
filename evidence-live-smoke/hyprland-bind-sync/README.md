# C2b Live-Evidenz (Sebos Hyprland-Session, 2026-10-27)

- binds_before.json        : Ausgangslage U(5)+O(5), modmask CTRL=4+SHIFT=1
- binds_arch_before.json   : Arch-Subset gefiltert
- binds_after_drift.json   : nach Startup-Drift (config Y vs store U) -> Y(12)=CTRL+ALT, U entfernt
- binds_after_ui_change.json: nach UI-Zyklus Y->U -> U(5) zurueck, Y entfernt
- binds_final.json         : Endzustand U(5)+O(5) (Sebo-Original wiederhergestellt)
- store_before.json / store_final.json / store_diff.txt : omasettings.json vor/nach + Diff
  (diff identisch: alle U<->Y-Zwischenschritte enden im Ausgangszustand; Zwischenzustaende
   sind in evidence/instance_run1.log und instance_run2.log dokumentiert)
- instance_run1_deadlock.log : Live-Nachweis des sync_slot-Mutex-Selbstdeadlocks
  ('TOGGLE-CMD rufe sync_slot' ohne 'sync_slot ein' — Store blieb ungeschrieben)
- instance_run2.log : fixierte Instanz: 'sync_slot ein: ... consent=true' -> 'slot OK' (beide Richtungen),
  drift no-op + drift OK (Y), jeweils mit Backup-Verweis
- bindings_before.lua / bindings_final.lua : Marker-Block vor/nach (identisch)
- live_evidence/*.png : Settings-View, armed-Capture, Commit, Drift/Sync-Screenshots
