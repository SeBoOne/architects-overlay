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

## Welle 2 (Feedback-Runde nach Welle 1, 2026-11-21 — 8 Punkte)

**Live-Test der Welle 1 ergab:**

1. **Tab-Navigation:** Mining-Modus → Reiter „Listen" hat KEINEN Weg zurück zu
   den anderen Reitern (nur Modus-Wechsel). → Tab-Header in allen Modi
   konsistent, Rückweg immer da.
2. **Sync Mining→Craft fehlt:** Teil-Material hinzufügen oder „erledigt" in
   einer AUS CRAFTING entstammten Mining-Liste ist in der Crafting-Liste
   NICHT sichtbar. → Abhaken muss durchschlagen.
3. **MODELL-UMKEHR (Inventar-first):** NICHT über die Crafting-Liste Material
   ins Inventar schreiben, sondern umgekehrt: **Material zuerst ins Inventar,
   dann aus dem Inventar die benötigte Menge reservieren** — aus allen
   verfügbaren Quellen und Qualitäten. Reservierung im Inventar sichtbar
   (pro Material: gesamt / reserviert / frei, und wofür reserviert).
   Crafting-Liste zeigt **Durchschnittsqualität der reservierten Materialien**.
   Plus: Button in der Crafting-Liste, der die **Inventar-Eingabemaske** öffnet
   (Material direkt aus der Liste anlegen).
4. **Autocomplete-Doppel:** Freitext-Ort im Inventar erscheint als eigener
   Vorschlag und verdeckt teils die SC-Location-Vorschläge. → Freitext
   entweder nicht vorschlagen ODER in denselben Autocomplete integrieren.
5. **Mining-Liste → Inventar bleibt:** Material direkt aus (unverknüpften)
   Mining-Listen ins Inventar — mit Vorbefüllung: „als erledigt"-Flow öffnet
   die Inventar-Eingabemaske mit dem fehlenden Restbetrag voreingetragen
   (nur Qualität + Lagerort eintragen); Teil-Eintrag → Teilmenge voreingetragen.
   **Nur bei verknüpften Listen Reservierung** — unverknüpfte Mining-Listen
   reservieren NICHTS.
6. **Scroll + Padding:** Beide Listen (Craft + Mining) haben Inhalt außerhalb
   des sichtbaren Bereichs, zu dem man nicht scrollen kann; alles klebt am
   Rand (Zier-Rahmen teils abgeschnitten). → Scrollbar für alles + Padding.
   Gleiche Padding-Problematik in den Blueprint-Details des Crafting-Modus.
7. **Craft→Mining-Sync:** Änderungen an der Crafting-Liste (Mengen, Material)
   spiegeln sich automatisch in der verknüpften Mining-Liste. Verknüpfte
   Mining-Listen können Materialien/Mengen NICHT selbst ändern (abhängig).
8. **Reservierungen allgemein (größer als Listen):** Reservieren ist NICHT
   nur über Crafting-Listen — direkt im Inventar für beliebige Zwecke
   (unabhängige Projekte, Verkauf). Partielle Reservierung (auch Teil-Mengen
   reservierbar), Anzeige gesamt/reserviert/frei + je Zweck.

**Konsequenz fürs Datenmodell:** Reservierungen werden zum Inventar-first-
Primtiv (`reservations`: inventory_id, purpose/list_ref, qty, note) statt
Liste-an-Inventar-Schreibrichtung. Listen-Reservierung = Zweck „Liste X".
Qualitäts-Mix: Durchschnitt über reservierte Positionen (gewichtet nach Menge).

## Offen

- Modus-Taste für die Listen-Ansicht (F2? F3? — belegt-Prüfung nötig).
- Menge-Einheiten: SCU/gems/items-Mischlisten (Einheiten-Regel beachten).
- Inventar-Datenmodell: wie hängt es am bestehenden Hangar/Inventar-Modus?
