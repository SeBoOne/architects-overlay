# Module-Overrides — Zuweisungs-Datei pflegen

Quelle der Modul-Zuordnung (S4): `src-tauri/resources/module-overrides.json` im Repo.
Schiff mit Eintrag in der Datei → Modul-Feld mit genau den Datei-Modulen;
Schiff ohne Eintrag → gar kein Modul-Feld.

## Ablauf: neue Zuweisung einpflegen

1. **Dev-Repo (privat)**: `src-tauri/resources/module-overrides.json` auf
   `main` bearbeiten — Format:

   ```json
   { "ships": [ { "ship": "Aegis Eclipse", "modules": ["S10 Bomb Rack Weapon Kit", "..."] } ] }
   ```

2. **Öffentliches Repo**: nach dem Merge `git push public main` — die Datei
   liegt damit auf dem `main` von `github.com/SeBoOne/architects-overlay`.
   Vor dem Merge auf `main` auf grün laufende Gates achten.
3. **Nutzer ziehen die Änderung automatisch**: Jede Installation ladet die
   Datei stündlich per Remote-Fetch von

   `https://raw.githubusercontent.com/SeBoOne/architects-overlay/main/src-tauri/resources/module-overrides.json`

   (öffentlich, kein Login nötig) und cacht sie; der Settings-Button
   „Modul-Zuweisungen aktualisieren" (`modul_compat_refresh`) erzwingt den
   Fetch sofort. Neue Releases liefern die Datei zusätzlich als
   Repo-Baseline mit.

## Fallback-Kette

Remote-Fetch (GitHub-Raw) → Remote-Cache (letzte erfolgreiche Datei,
Cache-DB) → Repo-Baseline (in der Binary eingebettete `module-overrides.json`).
Fehler beim Fetch sind nie fatal — die App läuft mit dem letzten bekannten
Stand weiter.
