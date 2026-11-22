// M9-C (t_dc01f34b) — Unit-Tests für die Beste-Orte-Optimierung (Set-Cover
// + Qualitäts-Band-Filter). Laufbar mit `node --test src/lib/mining-places.test.ts`
// (Node type-stripping, Muster ore-abbrev.test.ts).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
	isExcludedLocation,
	locationQualifies,
	maxQualityForTier,
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

test('ohne ore_id (items) oder ohne qualifizierenden Ort → uncovered', () => {
	const mats = [mat('Titanium', null, 0, 'items'), mat('Seltenes', 'x', 950)];
	const bands = { Seltenes: [400, 800] };
	const plan = planBestPlaces(mats, { x: [row('L1', 'Ort', 6)] }, bands, 1);
	assert.equal(plan.places.length, 0);
	assert.deepEqual(
		plan.uncovered.map((u) => u.material).sort(),
		['Seltenes', 'Titanium']
	);
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
});
