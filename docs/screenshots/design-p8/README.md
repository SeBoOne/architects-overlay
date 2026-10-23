# P8 — Preis-Detail-Redesign · Design-Varianten (Kanban t_b503888b)

Feedback v0.1.7, Punkt 8: Die Preis-Detailansicht (Commodity geöffnet → Kauf-/Verkauf-
Terminals) ist unübersichtlich — viele Zeilen untereinander, relevante Info (Preis +
Terminal) muss man erst suchen, leere Sektionen stehen mit vollem Spaltenkopf da.

**Status: Entwurf, kein Produktionscode.** Der Builder implementiert nach Sebos Wahl.

## Mockups (interaktiv, lokal im Browser öffnen)

| Datei | Variante |
|---|---|
| `mockups/A.html` | A — Arkanis-artige Zeilenbox |
| `mockups/B.html` | B — kompakte Preis-Matrix |
| `mockups/C.html` | C — Bestpreis-Hero + Terminal-Liste |

Öffnen: `xdg-open docs/screenshots/design-p8/mockups/A.html` (A/B/C verlinken sich
gegenseitig). Die Leiste am unteren Rand schaltet **live** um:

* **Palette** — Amber (Default) / MFCD / Bloom; identische Token-Blöcke wie `src/app.css`,
  Wechsel ist genau `data-palette` auf `<html>`.
* **Datensatz** — echte UEX-Cache-Daten (s. u.).
* **Pane-Höhe** — „Höhe real (640 px)" = das bestehende Budget `.pane-detail`
  `min(58vh, 640px)`, oder „Höhe frei" zum Draufschauen.
* **Tastatur** — ↑↓ bewegt die Markierung, ↵ setzt die Auswahl; die Fußzeile zählt mit.

## Screenshots (3440 × 1440 = Sebos Monitor, Vollbild)

```
A-levski-amber.png              A-laranite-amber.png   A-laranite-verkauf-amber.png
B-levski-amber.png              B-laranite-amber.png   B-cm-mfcd.png
C-levski-amber.png              C-laranite-amber.png   C-laranite-bloom.png
```

`*-levski-*` = der Referenzfall aus dem Feedback (1 Kauf-Zeile, 0 Verkauf).
`*-laranite-*` = breiter Fall (6 Kauf-, 20 Verkauf-Terminals).
`*-cm-*` = voller Datensatz Construction Materials (6 Kauf-, 31 Verkauf-Terminals).
`*-mfcd/-bloom` belegen den Palettenumschalter.

## Daten (echt, aus dem UEX-Cache)

`~/.local/share/com.sebo.architects-overlay/cache.sqlite3`, Tabelle `prices`, roh
übernommen (kein Mock-Inhalt):

* **Construction Materials / Levski** — Kauf 8.640, Bestand 42 SCU; 0 Verkaufsterminals.
* **Laranite** — 6 Kauf-Terminals (7.047 … 7.541), 20 Verkaufsterminals (6.500 … 9.700).
* **Construction Materials** — 6 Kauf-Terminals (8.640 … 10.023), 31 Verkaufsterminals
  (9.700 … 13.000).

## Die drei Varianten

### A — Arkanis-artige Zeilenbox
Jede Preiszeile ist eine eigene Box (Icon + Terminal + Bestand links, Preis rechtsbündig
farbcodiert), Kauf und Verkauf als gruppierte Abschnitte mit Akzentband. Eine leere
Sektion wird komplett weggeblendet und durch **eine** gestrichelte Statuszeile ersetzt
(„Verkauf · 0 Terminal · kein Terminal kauft diese Ware") — kein Spaltenkopf, keine
Leerzeilen. Größte visuelle Nähe zum Arkanis-Vorbild und die klarsten Klick-/↑↓-Ziele,
aber auch die höchste Liste (26 Zeilen = 2.607 px Scrollfläche gegenüber 2.287 px bei B) —
stark bei kurzen Listen wie Levski, scroll-lastig bei vollen Datensätzen.

### B — kompakte Preis-Matrix
Eine Tabelle, Spalten `Terminal | Kauf | Verkauf`; jedes Terminal genau **eine** Zeile,
nicht gehandelte Richtung als grauer durchgestrichener Strich („–" = wird hier nicht
gehandelt, in der Legende erklärt). Sortierung „beste Gelegenheit oben": Kaufpreis
aufsteigend, danach reine Verkaufsterminals absteigend. Minimal-invasiv — nutzt exakt das
bestehende Zeilenraster (`.ptab`-Logik) und ist mit 2.287 px die kürzeste der drei
Varianten; dafür zeigt sie pro Terminal nur eine Zeile, also weniger Kontext pro Zeile.

### C — Bestpreis-Hero + Terminal-Liste
Drei KPI-Karten oben beantworten die Handelsfrage direkt: **günstigster Kauf**, **höchster
Verkauf**, **Marge/SCU** (inkl. Beispiel „265.300 je 100 SCU" und der Route). Darunter
dieselbe kompakte Terminal-Liste wie in B als ↑↓-Ziel. Beste Entscheidungshilfe, weil die
Antwort vor der Liste steht statt in ihr; kostet ~130 px Höhe, wodurch bei 640 px Pane
zwei Zeilen weniger sichtbar sind als in B.

## Design-Entscheidungen (für alle Varianten)

* **Bestpreis-Semantik korrigiert:** „Best Kauf" = **niedrigster** `buy_price`,
  „Best Verkauf" = **höchster** `sell_price`. Die App nimmt heute für beide `max()`
  (`dBest`, +page.svelte:209) — der Kauf-Chip zeigt damit den *teuersten* Einkauf. Das
  gehört bei der P8-Umsetzung mitkorrigiert (deckt sich mit dem im Builder-Task
  genannten „dBest/dRows"-Punkt).
* **Sortierung Kauf aufsteigend** (günstigster zuerst), Verkauf absteigend. Die App
  sortiert heute beide Richtungen absteigend.
* **0-Terminal-Regel:** kein Spaltenkopf, keine leere Tabelle — eine Zeile genügt
  (A: gestrichelte Zeile; B: Segment in der Legende; C: KPI-Karte „kein Verkauf-Terminal").
* **Bestand 0 ≠ „0 SCU":** UEX meldet 0 als „keine Meldung" — deshalb Klartext
  („keine Nachfrage gemeldet") statt eines Strichs in der Unterzeile.
* **Token-Vertrag eingehalten:** Farben ausschließlich über die CSS-Variablen aus
  `src/app.css` (Amber/MFCD/Bloom-Blöcke wörtlich übernommen), kein Literalfarbwert in den
  Varianten-Styles. Einzige Ausnahme ist der als solcher markierte Mockup-Bühnen-Block
  (`--mock-*`, simuliert das Gameplay hinter dem Overlay).
* **Fußzeile:** Die Mockups zeigen noch die zwei Zeilen (Detail-Fußzeile + App-Fußzeile) wie
  im Referenzscreenshot. Laut P6 wandern die Detail-Hinweise in die *eine* dynamische
  Statuszeile — das Layout der Varianten ändert sich dadurch nicht.

## Offene Punkte für die Umsetzung

1. `dBest` auf min(Kauf)/max(Verkauf) drehen und die Kopf-Chips entsprechend labeln.
2. Pane-Höhe prüfen: bei 3440 × 1440 sind 640 px knapp für 26+ Zeilen — ggf.
   `min(64vh, 760px)`, sonst bleibt jede Variante scroll-lastig.
3. Variante A und B/C mischen ist möglich: A-Boxen für kurze Listen (≤ ~8 Zeilen),
   B-Matrix ab ~10 Zeilen.

## Verifikation (Selbstprüfung vor Abgabe)

Headless-Chromium 3440 × 1440, 9 Kombinationen (3 Varianten × 3 Datensätze):

* Palette exakt **2.064 px = 60 vw** in allen 9 Fällen (vorher lief Variante C bei
  Laranite auf 2.391 px auf — Ursache war die nowrap-Kontextzeile der Marge-Karte, behoben).
* **0 Überläufe** (kein `scrollWidth > clientWidth` an Namen/Preisen/Kopfzeilen).
* **0 Konsolenfehler**; ↑↓ ändert die Markierung, die markierte Zeile liegt immer
  vollständig im sichtbaren Pane; Found-Chip über, Fußzeile unter dem Pane.
* Screenshots zusätzlich per Sichtprüfung gelesen (Werte, Labels, Farbcodierung,
  Leerzustände) und danach korrigiert: doppelte Leer-Hinweiszeile in B entfernt,
  „Nachfrage — SCU"/doppelter Strich in C ersetzt, Matrix-Sortierung gedreht.

Erzeugende Skripte (nicht Teil des Repos): `~/.hermes/profiles/designer/cache/scratch/p8/`
(`mkdata.py`, `mkhtml.py`, `shots.py`, `verify.py`, `verify.json`).
