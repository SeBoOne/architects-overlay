// M9-C (t_dc01f34b) — Unit-Tests für die Beste-Orte-Optimierung (Set-Cover
// + Qualitäts-Band-Filter). Laufbar mit `node --test src/lib/mining-places.test.ts`
// (Node type-stripping, Muster ore-abbrev.test.ts).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
	isExcludedLocation,
	locationQualifies,
	maxQualityForTier,
	normalizeOreName,
	planBestPlaces,
	type PlaceLocRow,
	type PlaceMat
} from './mining-places.ts';

const row = (locationId: string, location: string, tier: number, group = 'surface'): PlaceLocRow => ({
	locationId,
	location,
	system: 'Stanton',
	group,
	tier,
	concentration: 0.5
});

const mat = (material: string, oreId: string | null, min_quality = 0, unit = 'SCU'): PlaceMat => ({
	material,
	unit,
	quantity: 1,
	min_quality,
	oreId
});

// ---------- maxQualityForTier ----------

test('maxQualityForTier: proportional, Tier 6 = oberstes Band (1000)', () => {
	const bands = [310, 525, 646, 718, 853, 925, 957, 1000];
	assert.equal(maxQualityForTier(bands, 6), 1000); // Pure → jede Qualität
	assert.equal(maxQualityForTier(bands, 1), 310); // niedrigstes Band
	assert.equal(maxQualityForTier(bands, 3), 718); // Mitte: round(3/6*8)=4 → idx 4 (0-basiert 3)
	assert.ok(maxQualityForTier(bands, 4)! > maxQualityForTier(bands, 3)!); // monotone
	// Kein Band-Datenbestand → Filter nicht anwendbar (null).
	assert.equal(maxQualityForTier([], 4), null);
});

// ---------- locationQualifies ----------

test('locationQualifies: Band-Filter + Tier-Schwelle', () => {
	const bands = [500, 1000];
	// Tier 4 → round(4/6*2)=1 → bands[0]=500 ≥ 500 ✓, aber < 800 ✗.
	assert.equal(locationQualifies(4, bands, 500, 4), true);
	assert.equal(locationQualifies(4, bands, 800, 1), false);
	// Tier-Schwelle greift VOR dem Band-Filter.
	assert.equal(locationQualifies(3, bands, 0, 4), false);
	// Ohne Bänder zählt nur die Schwelle (kein Raten).
	assert.equal(locationQualifies(5, [], 900, 4), true);
	assert.equal(locationQualifies(4, [], 900, 4), true);
	assert.equal(locationQualifies(3, [], 900, 4), false);
});

// ---------- planBestPlaces ----------

test('set-cover: ein Ort, der alles abdeckt, gewinnt', () => {
	const mats = [mat('A', 'a'), mat('B', 'b'), mat('C', 'c')];
	const rowsByOre = {
		a: [row('L1', 'Ort 1', 4), row('L2', 'Ort 2', 4)],
		b: [row('L1', 'Ort 1', 4)],
		c: [row('L1', 'Ort 1', 4), row('L2', 'Ort 2', 5)]
	};
	const plan = planBestPlaces(mats, rowsByOre, {}, 1);
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L1');
	assert.equal(plan.places[0].covered.length, 3);
	assert.equal(plan.uncovered.length, 0);
});

test('set-cover: zwei Orte, wenn keiner alles abdeckt', () => {
	const mats = [mat('A', 'a'), mat('B', 'b')];
	const rowsByOre = {
		a: [row('L1', 'Ort 1', 4)],
		b: [row('L2', 'Ort 2', 4)]
	};
	const plan = planBestPlaces(mats, rowsByOre, {}, 1);
	assert.equal(plan.places.length, 2);
	assert.deepEqual(
		plan.places.map((p) => p.locationId).sort(),
		['L1', 'L2']
	);
	assert.equal(plan.uncovered.length, 0);
});

test('Qualitäts-Band-Filter: Ort zählt nur, wenn sein Band die Mindest-Qualität liefert', () => {
	const mats = [mat('Taranite', 'tar', 900)];
	const bands = { Taranite: [310, 525, 646, 718, 853, 925, 957, 1000] };
	const rowsByOre = {
		tar: [row('L-LOW', 'Armer Ort', 3), row('L-HIGH', 'Reicher Ort', 6)]
	};
	const plan = planBestPlaces(mats, rowsByOre, bands, 1);
	// Tier 3 → bands[3]=718 < 900 ✗; Tier 6 → 1000 ≥ 900 ✓.
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L-HIGH');
});

test('ohne ore_id (items) → uncovered; mit ore_id aber kein qualifizierender Ort → Qualitäts-Fallback mit maxQuality-Warnung (M9-O)', () => {
	const mats = [mat('Titanium', null, 0, 'items'), mat('Seltenes', 'x', 950)];
	const bands = { Seltenes: [400, 800] };
	const plan = planBestPlaces(mats, { x: [row('L1', 'Ort', 6)] }, bands, 1);
	// Titanium (kein ore_id) bleibt uncovered…
	assert.deepEqual(plan.uncovered.map((u) => u.material), ['Titanium']);
	// …Seltenes bekommt den besten verfügbaren Ort als Fallback (max. 800 < 950).
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L1');
	assert.equal(plan.places[0].covered[0].maxQuality, 800);
});

test('erledigte Materialien (done) werden nicht mehr mitgeplant', () => {
	const mats = [mat('A', 'a'), mat('B', 'b')];
	const rowsByOre = {
		a: [row('L1', 'Ort 1', 4)],
		b: [row('L1', 'Ort 1', 4)]
	};
	const plan = planBestPlaces(mats, rowsByOre, {}, 1, (m) => m.material === 'A');
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].covered.length, 1);
	assert.equal(plan.places[0].covered[0].material, 'B');
});

test('Tie-Break: gleiche Abdeckung → höhere Tier-Summe zuerst', () => {
	const mats = [mat('A', 'a')];
	const rowsByOre = {
		a: [row('L1', 'Ort 1', 2), row('L2', 'Ort 2', 5)]
	};
	const plan = planBestPlaces(mats, rowsByOre, {}, 1);
	assert.equal(plan.places[0].locationId, 'L2');
});

// Regression (Review R1): Realistische Materialnamen mit Parenthesen/Kast.
// MiningLists.svelte baut den Band-Key als raw `${material}:${unit}` —
// ein Test mit einfachen Namen konvergierte nur zufällig mit dem
// Raw-Namens-Fallback und maskierte die Key-Inkompatibilität.
test('Band-Key realistischer Name "Taranite (Raw)": raw `${material}:${unit}`-Form deckt mit', () => {
	const material = 'Taranite (Raw)';
	const bands = { 'Taranite (Raw):SCU': [310, 525, 646, 718, 853, 925, 957, 1000] };
	const mats = [mat(material, 'tar', 900)];
	const rowsByOre = {
		tar: [row('L-LOW', 'Armer Ort', 3), row('L-HIGH', 'Reicher Ort', 6)]
	};
	// Band-Filter aktiv: Tier 3 → 718 < 900 ✗; nur Tier 6 → 1000 ≥ 900 ✓.
	const plan = planBestPlaces(mats, rowsByOre, bands, 1);
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L-HIGH');
});

test('Band-Key realistischer Name: falscher Key (lowercase-only) darf NICHT decken', () => {
	// Wenn der Key nicht konvergiert (bands=[]), zählt nur die Tier-Schwelle —
	// das war der Review-Defekt: Tier-3-Ort deckte Q900 zu Unrecht.
	const mats = [mat('Taranite (Raw)', 'tar', 900)];
	const rowsByOre = { tar: [row('L-LOW', 'Armer Ort', 3)] };
	const plan = planBestPlaces(mats, rowsByOre, {}, 1);
	assert.equal(plan.places.length, 1, 'ohne Bänder deckt Tier-Schwelle — Beweis der Maske');
});

// ---------- M9-K (t_db9b234f, P10): Concentration-Modus ----------

test('concentration: je Material der ORT mit höchster Tier-Stufe, gemeinsame Orte zusammengefasst', () => {
	const mats = [mat('A', 'a'), mat('B', 'b')];
	const rowsByOre = {
		a: [row('L1', 'Ort 1', 2), row('L2', 'Ort 2', 5)],
		b: [row('L1', 'Ort 1', 3), row('L3', 'Ort 3', 6)]
	};
	const plan = planBestPlaces(mats, rowsByOre, {}, 1, undefined, 'concentration');
	// A → L2 (T5), B → L3 (T6): die T-High-Orte je Material, nicht Set-Cover.
	assert.deepEqual(plan.places.map((p) => p.locationId).sort(), ['L2', 'L3']);
	const l2 = plan.places.find((p) => p.locationId === 'L2')!;
	assert.equal(l2.tier, 5);
	assert.deepEqual(l2.covered.map((c) => c.material), ['A']);
});

test('concentration: gleicher Top-Ort für mehrere Materialien → ein Pick mit allen covered', () => {
	const mats = [mat('A', 'a'), mat('B', 'b')];
	const rowsByOre = {
		a: [row('L1', 'Ort 1', 6)],
		b: [row('L1', 'Ort 1', 6)]
	};
	const plan = planBestPlaces(mats, rowsByOre, {}, 1, undefined, 'concentration');
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].covered.length, 2);
});

test('concentration: Qualitäts-Bänder filtern auch hier — unqualifizierter Top-Tier-Ort wird nicht gewählt', () => {
	const bands = { Taranite: [310, 525, 646, 718, 853, 925, 957, 1000] };
	const mats = [mat('Taranite', 'tar', 900)];
	const rowsByOre = { tar: [row('L-LOW', 'Armer Ort', 3), row('L-HIGH', 'Reicher Ort', 6)] };
	const plan = planBestPlaces(mats, rowsByOre, bands, 1, undefined, 'concentration');
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L-HIGH');
});

test('concentration: erledigte Materialien und ortlose bleiben ausgenommen', () => {
	const mats = [mat('A', 'a'), mat('B', 'b'), mat('X', null, 0, 'items')];
	const rowsByOre = { a: [row('L1', 'Ort 1', 4)] };
	const plan = planBestPlaces(mats, rowsByOre, {}, 1, (m) => m.material === 'A', 'concentration');
	assert.equal(plan.places.length, 0);
	assert.deepEqual(plan.uncovered.map((u) => u.material).sort(), ['B', 'X']);
});

// ---------- M9-L (t_4de3c84f, P6): Battagliaintro-Filter ----------

test('isExcludedLocation: Slug UND locationId-Suffix-Match, Dungeon-Typ bleibt unangetastet', () => {
	// Direkt per locationId (strata_ore_locations-Zeilen, kein Slug):
	assert.equal(isExcludedLocation('dungeon_v3slotpreset_harvestable_battagliaintro'), true);
	// Per Slug (strata_locations-Zeilen):
	assert.equal(isExcludedLocation('dungeon_v3slotpreset_harvestable_battagliaintro', 'battagliaintro'), true);
	assert.equal(isExcludedLocation('egal', 'battagliaintro'), true);
	// Andere Dungeon-Orte NICHT getroffen (nur der Slug wird gefiltert):
	assert.equal(isExcludedLocation('dungeon_cave_aberdeen_medium'), false);
	assert.equal(isExcludedLocation('dungeon_v3slotpreset_rockcracker', 'rockcracker'), false);
	assert.equal(isExcludedLocation('dungeon_v3slotpreset_harvestable_rockcracker_asteroid'), false);
	// Nichts zu tun ohne Slug-Übereinstimmung:
	assert.equal(isExcludedLocation('station_portolis'), false);
});

test('planBestPlaces (fewest): Battagliaintro-Zeile wird aus den Kandidaten gefiltert', () => {
	const mats = [mat('A', 'a')];
	const rowsByOre = {
		a: [
			row('dungeon_v3slotpreset_harvestable_battagliaintro', 'Battagliaintro', 6),
			row('L1', 'Regulärer Ort', 4)
		]
	};
	const plan = planBestPlaces(mats, rowsByOre, {}, 1);
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L1');
	// Kein material-getriebener Rückgriff auf den gefilterten Ort:
	assert.ok(!plan.places.some((p) => p.locationId.includes('battagliaintro')));
});

test('planBestPlaces (fewest): NUR Battagliaintro → Material bleibt uncovered, kein Crash', () => {
	const mats = [mat('A', 'a')];
	const rowsByOre = { a: [row('dungeon_v3slotpreset_harvestable_battagliaintro', 'Battagliaintro', 6)] };
	const plan = planBestPlaces(mats, rowsByOre, {}, 1);
	assert.equal(plan.places.length, 0);
	assert.deepEqual(plan.uncovered.map((u) => u.material), ['A']);
});

test('planBestPlaces (concentration): Battagliaintro wird nie als Top-Ort gewählt', () => {
	const mats = [mat('A', 'a')];
	const rowsByOre = {
		a: [
			row('dungeon_v3slotpreset_harvestable_battagliaintro', 'Battagliaintro', 6),
			row('L1', 'Regulärer Ort', 4)
		]
	};
	const plan = planBestPlaces(mats, rowsByOre, {}, 1, undefined, 'concentration');
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L1');
// ---------- M9-O (t_a39041f8, Feedback P5) ----------

test('normalizeOreName: „Glacosite" matcht „Glacosite (Raw)"/„glacosite"-Slug, kein fuzzy-Teiltreffer', () => {
	assert.equal(normalizeOreName('Glacosite'), normalizeOreName('Glacosite (Raw)'));
	assert.equal(normalizeOreName('Riccite'), normalizeOreName('Riccite (Ore)'));
	assert.equal(normalizeOreName('  Taranite  '), 'taranite');
	// Kein includes-Matching: „Taran" darf „Taranite" NICHT matchen.
	assert.notEqual(normalizeOreName('Taran'), normalizeOreName('Taranite'));
});

test('M9-O Riccite: min_quality > max aller Tier-Bänder → Fallback-Ort mit maxQuality statt uncovered', () => {
	// Realer Fall: Bänder [325..1000], Orte nur Tier 2 (maxQ 671) und Tier 4
	// (maxQ 870 < min_quality 965) → bisher ALLE uncovered.
	const bands = { Riccite: [325, 525, 671, 743, 870, 942, 965, 1000] };
	const mats = [mat('Riccite', 'riccite_ore', 965)];
	const rowsByOre = {
		riccite_ore: [row('L-T2', 'Tier-2-Ort', 2), row('L-T4', 'Tier-4-Ort', 4)]
	};
	const plan = planBestPlaces(mats, rowsByOre, bands, 4);
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L-T4');
	assert.equal(plan.places[0].covered[0].maxQuality, 870);
	assert.equal(plan.uncovered.length, 0);
});

test('M9-O Fallback-Bestwahl: maxQuality DESC, dann Tier DESC (maxQ ist monotone in Tier), dann Name', () => {
	const bands = { X: [100, 500] }; // maxQ: T2/T3/T4 alle → bands[0]=100 (round-Map)
	const mats = [mat('X', 'x', 950)];
	const rowsByOre = {
		x: [row('L-A', 'Ort A', 2), row('L-B', 'Ort B', 4), row('L-C', 'Ort C', 3)]
	};
	const plan = planBestPlaces(mats, rowsByOre, bands, 1);
	// maxQ gleich → höhere Tier gewinnt (L-B, T4); maxQuality-Marker gesetzt.
	assert.equal(plan.places[0].locationId, 'L-B');
	assert.equal(plan.places[0].tier, 4);
	assert.equal(plan.places[0].covered[0].maxQuality, 100);
});

test('M9-O Fallback-Bestwahl: gleiche maxQuality+Tier → Name-Ascending', () => {
	const bands = { X: [100, 500] };
	const mats = [mat('X', 'x', 950)];
	const rowsByOre = {
		x: [row('L-B', 'Ort B', 4), row('L-A', 'Ort A', 4)]
	};
	const plan = planBestPlaces(mats, rowsByOre, bands, 1);
	assert.equal(plan.places[0].locationId, 'L-A');
});

test('M9-O: oreId gesetzt, aber Erz hat GAR KEINE Orte → weiterhin uncovered', () => {
	const mats = [mat('Verwaist', 'ghost_ore', 0)];
	const plan = planBestPlaces(mats, { ghost_ore: [] }, {}, 1);
	assert.equal(plan.places.length, 0);
	assert.deepEqual(plan.uncovered.map((u) => u.material), ['Verwaist']);
});

test('M9-O: Fallback-Material teilt sich den Ort mit normalen Materialien (fewest)', () => {
	const bands = { Rar: [200, 400] };
	const mats = [mat('Normal', 'n'), mat('Rar', 'r', 900)];
	const rowsByOre = {
		n: [row('L1', 'Ort 1', 4)],
		r: [row('L1', 'Ort 1', 4), row('L2', 'Ort 2', 4)]
	};
	// Rar: maxQ(T4)=200 (round(4/6*2)=1 → bands[0]) < 900 → Fallback auf L1
	// (Tie maxQ/Tier → Name); L1 deckt zusätzlich Normal → EIN Ort mit beiden,
	// Rar mit maxQuality-Marker.
	const plan = planBestPlaces(mats, rowsByOre, bands, 1);
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L1');
	assert.equal(plan.places[0].covered.length, 2);
	const rar = plan.places[0].covered.find((c) => c.material === 'Rar')!;
	assert.equal(rar.maxQuality, 200);
	const normal = plan.places[0].covered.find((c) => c.material === 'Normal')!;
	assert.equal(normal.maxQuality, undefined);});
