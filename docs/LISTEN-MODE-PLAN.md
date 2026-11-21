# Listen-System (M9) — Plan

> ZieL: Blueprint Pool + Crafting-/Mining-Listen + Inventar-Anbindung.
> Schwester-Pläne: [MINING-MODE-PLAN.md](MINING-MODE-PLAN.md), [CRAFTING-MODE-PLAN.md](CRAFTING-MODE-PLAN.md).

## Komponenten (User-Vision, 2026-11-21)

1. **Blueprint Pool (Crafting):** Haken in der Blueprint-Liste + Haken oben rechts
   im Detail — markiert „im Besitz". Zusätzlicher Filter (an/aus) neben den
   Kategorie-Filtern: direkt verfügbare / noch nicht verfügbare Blueprints.
2. **Crafting-Listen:** benannte Listen von Blueprints (z. B. „FPS-Set",
   „Schiffskomponenten-Set") — auf einen Blick alle geplanten Gegenstände +
   **aggregierte Materialmengen über alle Listen-Items** (+ optional Qualität).
   Qualität wird im Calculator beim Hinzufügen übernommen (Default 500 →
   optionaler Wert bleibt leer). **Abhakbar:** Material ganz erledigen ODER
   teilweise (z. B. 250/300 Gems Hadanite) — Stand nachverfolgen.
3. **Mining-Listen:** aus allen Materialien Listen zusammenstellen (Material +
   Menge + optional Qualität), Abhaken wie oben. Plus: **beste Orte**-Anzeige —
   alles an möglichst wenigen Orten sammeln, Qualitäts-Anforderungen beachten
   (Band-Orte die das Mindest-Qualität-Band liefern).
4. **Craft→Mining-Brücke:** aus einer Crafting-Liste heraus das benötigte
   Material (+ Qualitäten) als Mining-Liste anlegen.
5. **Inventar-Anbindung:** Material über Listen hinzufügen → mit **Ort + Qualität**
   abfragen → landet direkt im Inventar mit Hinweis „reserviert für Liste X".
   Verbundene Listen (aus Crafting-Liste entstanden) verweisen auf die
   Crafting-Liste. **Höhere Qualität als gefordert → Anforderung wird
   angepasst** (höher ist immer besser).

## Architektur-Grundsätze

- **Lokale Persistenz** (Restart-fest): Listen/Besitz-Status/Abhaken landen im
  bestehenden lokalen Store (Cache-DB/settings — builder wählt passend, alles
  client-lokal, KEIN Cloud-Zwang).
- **Generisch:** Listen-System ist Modi-unabhängig gebaut (Pin-Content-Muster) —
  Craft- und Mining-Listen teilen die Listen-Engine (Items, Mengen, Qualität,
  Abhaken, Reservierung); die Modi sind Views darauf.
- **Qualität als Anforderung:** Anforderung = Mindest-Qualität; gelieferte
  Qualität ≥ Anforderung erfüllt und **hebt** die Anforderung nicht, aber
  eingetragene bessere Qualität wird übernommen (User: „höhere Qualität ist
  immer besser").
- **Ort-Optimierung (Mining-Listen):** Gruppierung nach Locations, bevorzugt
  Orte die mehrere Ziele abdecken; Qualitätsbänder (SCMDB) als Filter für
  „Ort liefert die geforderte Qualität?".

## Welle 1 (Karten)

- **A Blueprint Pool:** Besitz-Haken (Liste + Detail) + Filter, lokal persistiert.
- **B Listen-Engine + Crafting-Listen:** CRUD Listen, Blueprints hinzufügen
  (mit übernommener Qualität aus den Reglern), Material-Aggregation, Abhaken
  (ganz/teilweise), UI als eigener Modus-Abschnitt (F-Taste? — Omar-Entscheid
  im Spec-Followup oder Settings-Hotkey).
- **C Mining-Listen:** gleiche Engine, Mining-seitig + beste-Orte-Optimierung.
- **D Brücke:** Crafting-Liste → Mining-Liste (Material + Qualitäten).
- **E Inventar:** Material hinzufügen (Ort + Qualität + Reservierungs-Hinweis),
  Anforderungs-Anpassung bei höherer Qualität.

## Offen

- Modus-Taste für die Listen-Ansicht (F2? F3? — belegt-Prüfung nötig).
- Menge-Einheiten: SCU/gems/items-Mischlisten (Einheiten-Regel beachten).
- Inventar-Datenmodell: wie hängt es am bestehenden Hangar/Inventar-Modus?
