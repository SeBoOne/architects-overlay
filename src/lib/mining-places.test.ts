// M9-C (t_dc01f34b) — Unit-Tests für die Beste-Orte-Optimierung (Set-Cover
// + Qualitäts-Band-Filter). Laufbar mit `node --test src/lib/mining-places.test.ts`
// (Node type-stripping, Muster ore-abbrev.test.ts).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
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
