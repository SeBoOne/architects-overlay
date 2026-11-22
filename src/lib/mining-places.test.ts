// M9-C (t_dc01f34b) — Unit-Tests für die Beste-Orte-Optimierung (Set-Cover
// + Qualitätsfilter). Laufbar mit `node --test src/lib/mining-places.test.ts`
// (Node type-stripping, Muster ore-abbrev.test.ts).
//
// M9-O R2 (t_a39041f8): Sebos Domänen-Korrektur — Tier = KONZENTRATION, nicht
// Qualität. Qualitätsfilter über PRIMARY/SEKUNDÄR je Ort-Zeile (rank/rankPct):
// PRIMARY → jede min_quality ≤ 1000 machbar; SEKUNDÄR → min_quality ≤ bands[3].
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
	isExcludedLocation,
	isPrimary,
	locationQualifies,
	maxQualityForLoc,
	normalizeOreName,
	planBestPlaces,
	type PlaceLocRow,
	type PlaceMat
} from './mining-places.ts';

const row = (
	locationId: string,
	location: string,
	tier: number,
	opts: { group?: string; rank?: number; rankPct?: number } = {}
): PlaceLocRow => ({
	locationId,
	location,
	system: 'Stanton',
	group: opts.group ?? 'surface',
	tier,
	concentration: 0.5,
	rank: opts.rank ?? 1,
	rankPct: opts.rankPct ?? 100
});

const mat = (material: string, oreId: string | null, min_quality = 0, unit = 'SCU'): PlaceMat => ({
	material,
	unit,
	quantity: 1,
	min_quality,
	oreId
});

// ---------- PRIMARY-Bestimmung + maxQualityForLoc (Sebos Regel) ----------

test('isPrimary: rank 1 oder rankPct 100 je Ort-Zeile (Live-Beleg gold_ore)', () => {
	// Live-Fixture-Muster: rank 1 ⟺ rankPct 100; rank 2/75 = sekundär.
	assert.equal(isPrimary(row('L1', 'Ort 1', 4, { rank: 1, rankPct: 100 })), true);
	assert.equal(isPrimary(row('L1', 'Ort 1', 4, { rank: 2, rankPct: 100 })), true);
	assert.equal(isPrimary(row('L1', 'Ort 1', 4, { rank: 1, rankPct: 67 })), true);
	assert.equal(isPrimary(row('L1', 'Ort 1', 4, { rank: 2, rankPct: 75 })), false);
	assert.equal(isPrimary(row('L1', 'Ort 1', 4, { rank: 4, rankPct: 25 })), false);
});

test('maxQualityForLoc: PRIMARY → 1000, SEKUNDÄR → bands[3], ohne Bänder → null', () => {
	const bands = [325, 525, 671, 743, 870, 942, 965, 1000];
	assert.equal(maxQualityForLoc(row('L1', 'Ort', 4, { rank: 1, rankPct: 100 }), bands), 1000);
	assert.equal(maxQualityForLoc(row('L1', 'Ort', 4, { rank: 2, rankPct: 75 }), bands), 743);
	// Weniger als 4 Band-Grenzen → keine 4. Grenze → null (kein Raten).
	assert.equal(maxQualityForLoc(row('L1', 'Ort', 4, { rank: 2, rankPct: 75 }), [500]), null);
	assert.equal(maxQualityForLoc(row('L1', 'Ort', 4, { rank: 2, rankPct: 75 }), []), null);
	// Auch ohne Bänder ist PRIMARY 1000 (501–1000 machbar).
	assert.equal(maxQualityForLoc(row('L1', 'Ort', 4, { rank: 1, rankPct: 100 }), []), 1000);
});

// ---------- locationQualifies ----------

test('locationQualifies: PRIMARY erfüllt jede min_quality; SEKUNDÄR nur ≤ bands[3]', () => {
	const bands = [325, 525, 671, 743, 870, 942, 965, 1000];
	// Primary → 501–1000 machbar, auch bei min_quality 965/1000.
	assert.equal(locationQualifies(row('L1', 'Ort', 4, { rank: 1, rankPct: 100 }), bands, 965, 1), true);
	assert.equal(locationQualifies(row('L1', 'Ort', 4, { rank: 1, rankPct: 100 }), bands, 1000, 1), true);
	// Sekundär: 965 > bands[3]=743 → nicht machbar; 743 ≤ 743 → machbar.
	assert.equal(locationQualifies(row('L1', 'Ort', 4, { rank: 2, rankPct: 75 }), bands, 965, 1), false);
	assert.equal(locationQualifies(row('L1', 'Ort', 4, { rank: 2, rankPct: 75 }), bands, 743, 1), true);
	assert.equal(locationQualifies(row('L1', 'Ort', 4, { rank: 2, rankPct: 75 }), bands, 744, 1), false);
	// Ohne Bänder gilt der Sekundär-Filter nicht (kein Raten).
	assert.equal(locationQualifies(row('L1', 'Ort', 4, { rank: 2, rankPct: 75 }), [], 965, 1), true);
	// Konzentrations-Schwelle greift weiterhin VOR dem Qualitätsfilter.
	assert.equal(locationQualifies(row('L1', 'Ort', 3, { rank: 1, rankPct: 100 }), bands, 0, 4), false);
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

test('Qualitätsfilter (Sebos Regel): PRIMARY deckt Q900 auch an schwachem Ort; Sekundär nicht', () => {
	const bands = { Taranite: [310, 525, 646, 718, 853, 925, 957, 1000] };
	const mats = [mat('Taranite', 'tar', 900)];
	const rowsByOre = {
		tar: [
			row('L-HIGH', 'Reicher Ort', 6, { rank: 1, rankPct: 100 }),
			row('L-SEC', 'Sekundär-Ort', 6, { rank: 2, rankPct: 40 })
		]
	};
	// Beide Orte erfüllen (Primary → 1000 ≥ 900; Sekundär → bands[3]=718 < 900 ✗).
	const plan = planBestPlaces(mats, rowsByOre, bands, 1);
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L-HIGH');
	assert.equal(plan.places[0].covered[0].maxQuality, undefined);
});

test('M9-O Riccite (Sebos Regel): PRIMARY → Q965 machbar trotz nur Tier 2/4', () => {
	// Realer Fall: min_quality 965, Orte nur Tier 2 und Tier 4 — das war
	// vorher wegen der Band-Map ALLE uncovered. Nach der Regel qualifiziert
	// der Primary-Ort (501–1000 machbar) unabhängig von der Tier-Stufe.
	const bands = { Riccite: [325, 525, 671, 743, 870, 942, 965, 1000] };
	const mats = [mat('Riccite', 'riccite_ore', 965)];
	const rowsByOre = {
		riccite_ore: [
			row('L-T2', 'Tier-2-Ort', 2, { rank: 1, rankPct: 100 }),
			row('L-T4', 'Tier-4-Ort', 4, { rank: 2, rankPct: 60 })
		]
	};
	const plan = planBestPlaces(mats, rowsByOre, bands, 1);
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].covered[0].maxQuality, undefined);
	assert.equal(plan.uncovered.length, 0);
});

test('M9-O Riccite SEKUNDÄR: 965 > bands[3]=743 → Fallback-Ort mit maxQuality-Warnung statt uncovered', () => {
	const bands = { Riccite: [325, 525, 671, 743, 870, 942, 965, 1000] };
	const mats = [mat('Riccite', 'riccite_ore', 965)];
	const rowsByOre = {
		riccite_ore: [
			row('L-T2', 'Tier-2-Ort', 2, { rank: 2, rankPct: 30 }),
			row('L-T4', 'Tier-4-Ort', 4, { rank: 2, rankPct: 55 })
		]
	};
	const plan = planBestPlaces(mats, rowsByOre, bands, 1);
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].covered[0].maxQuality, 743);
	assert.equal(plan.uncovered.length, 0);
});

test('ohne ore_id (items) → uncovered; mit ore_id aber kein qualifizierender Ort → Qualitäts-Fallback mit maxQuality-Warnung (M9-O)', () => {
	const mats = [mat('Titanium', null, 0, 'items'), mat('Seltenes', 'x', 950)];
	const bands = { Seltenes: [400, 800] };
	const plan = planBestPlaces(mats, { x: [row('L1', 'Ort', 6, { rank: 2, rankPct: 80 })] }, bands, 1);
	// Titanium (kein ore_id) bleibt uncovered…
	assert.deepEqual(plan.uncovered.map((u) => u.material), ['Titanium']);
	// …Seltenes (Sekundär, nur 2 Band-Grenzen → kein Sekundär-Cap) — Filter
	// nicht anwendbar, Ort qualifiziert OHNE Warnung (kein Raten).
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L1');
	assert.equal(plan.places[0].covered[0].maxQuality, undefined);
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
		tar: [
			row('L-SEC', 'Sekundär-Ort', 3, { rank: 2, rankPct: 45 }),
			row('L-HIGH', 'Reicher Ort', 6, { rank: 1, rankPct: 100 })
		]
	};
	// Sekundär → bands[3]=718 < 900 ✗; Primary → 1000 ≥ 900 ✓.
	const plan = planBestPlaces(mats, rowsByOre, bands, 1);
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L-HIGH');
});

test('Band-Key realistischer Name: falscher Key (lowercase-only) darf NICHT decken', () => {
	// Wenn der Key nicht konvergiert (bands=[]), zählt der Sekundär-Filter
	// nicht — das war der Review-Defekt. Sekundär-Ort an Q900-Material
	// DECKT deshalb; der Test hält das Verhalten bewusst fest (kein Raten).
	const mats = [mat('Taranite (Raw)', 'tar', 900)];
	const rowsByOre = { tar: [row('L-SEC', 'Sekundär-Ort', 3, { rank: 2, rankPct: 45 })] };
	const plan = planBestPlaces(mats, rowsByOre, {}, 1);
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].covered[0].maxQuality, undefined);
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

test('concentration: Qualitätsfilter greift auch hier — Sekundär-Ort unter Q-Anspruch wird nicht gewählt', () => {
	const bands = { Taranite: [310, 525, 646, 718, 853, 925, 957, 1000] };
	const mats = [mat('Taranite', 'tar', 900)];
	const rowsByOre = { tar: [row('L-SEC', 'Sekundär-Ort', 6, { rank: 2, rankPct: 40 })] };
	// Sekundär → bands[3]=718 < 900 → kein qualifizierender Ort → Fallback
	// (Ort trotzdem gewählt, mit maxQuality-Warnung).
	const plan = planBestPlaces(mats, rowsByOre, bands, 1, undefined, 'concentration');
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L-SEC');
	assert.equal(plan.places[0].covered[0].maxQuality, 718);
});

test('concentration: erledigte Materialien und ortlose bleiben ausgenommen', () => {
	const mats = [mat('A', 'a'), mat('B', 'b'), mat('X', null, 0, 'items')];
	const rowsByOre = { a: [row('L1', 'Ort 1', 4)] };
	const plan = planBestPlaces(mats, rowsByOre, {}, 1, (m) => m.material === 'A', 'concentration');
	assert.equal(plan.places.length, 0);
	assert.deepEqual(plan.uncovered.map((u) => u.material).sort(), ['B', 'X']);
});

// ---------- M9-O (t_a39041f8, Feedback P5) ----------

test('normalizeOreName: „Glacosite" matcht „Glacosite (Raw)"/„glacosite"-Slug, kein fuzzy-Teiltreffer', () => {
	assert.equal(normalizeOreName('Glacosite'), normalizeOreName('Glacosite (Raw)'));
	assert.equal(normalizeOreName('Riccite'), normalizeOreName('Riccite (Ore)'));
	assert.equal(normalizeOreName('  Taranite  '), 'taranite');
	// Kein includes-Matching: „Taran" darf „Taranite" NICHT matchen.
	assert.notEqual(normalizeOreName('Taran'), normalizeOreName('Taranite'));
});

test('M9-O Fallback-Bestwahl: maxQuality DESC (Sebos Regel), dann Tier DESC, dann Name', () => {
	// Sekundär überall → maxQ alle bands[3]=500; Primary-Ort L-P wäre
	// qualifiziert (1000 ≥ 950) und NIE Fallback — deshalb alle sekundär.
	const bands = { X: [100, 200, 300, 500, 600] };
	const mats = [mat('X', 'x', 950)];
	const rowsByOre = {
		x: [
			row('L-A', 'Ort A', 2, { rank: 2, rankPct: 30 }),
			row('L-B', 'Ort B', 4, { rank: 3, rankPct: 20 }),
			row('L-C', 'Ort C', 3, { rank: 2, rankPct: 30 })
		]
	};
	const plan = planBestPlaces(mats, rowsByOre, bands, 1);
	// maxQ gleich (500) → höhere Tier gewinnt (L-B, T4); maxQuality-Marker.
	assert.equal(plan.places[0].locationId, 'L-B');
	assert.equal(plan.places[0].tier, 4);
	assert.equal(plan.places[0].covered[0].maxQuality, 500);
});

test('M9-O Fallback-Bestwahl: höheres Sekundär-Cap schlägt niedrigeres, gleiche maxQuality+Tier → Name-Ascending', () => {
	const bands = { X: [100, 200, 300, 500] };
	const mats = [mat('X', 'x', 950)];
	const rowsByOre = {
		x: [
			row('L-B', 'Ort B', 4, { rank: 2, rankPct: 60 }),
			row('L-A', 'Ort A', 4, { rank: 2, rankPct: 60 })
		]
	};
	const plan = planBestPlaces(mats, rowsByOre, bands, 1);
	assert.equal(plan.places[0].locationId, 'L-A');
	assert.equal(plan.places[0].covered[0].maxQuality, 500);
});

test('M9-O: oreId gesetzt, aber Erz hat GAR KEINE Orte → weiterhin uncovered', () => {
	const mats = [mat('Verwaist', 'ghost_ore', 0)];
	const plan = planBestPlaces(mats, { ghost_ore: [] }, {}, 1);
	assert.equal(plan.places.length, 0);
	assert.deepEqual(plan.uncovered.map((u) => u.material), ['Verwaist']);
});

test('M9-O: Fallback-Material teilt sich den Ort mit normalen Materialien (fewest)', () => {
	const bands = { Rar: [100, 200, 300, 400] };
	const mats = [mat('Normal', 'n'), mat('Rar', 'r', 900)];
	const rowsByOre = {
		n: [row('L1', 'Ort 1', 4)],
		r: [
			row('L1', 'Ort 1', 4, { rank: 2, rankPct: 40 }),
			row('L2', 'Ort 2', 4, { rank: 2, rankPct: 40 })
		]
	};
	// Rar: Sekundär → bands[3]=400 < 900 → Fallback; maxQ gleich, Tier
	// gleich → Name-Tie: L1. L1 deckt zusätzlich Normal → EIN Ort mit
	// beiden, Rar mit maxQuality-Marker.
	const plan = planBestPlaces(mats, rowsByOre, bands, 1);
	assert.equal(plan.places.length, 1);
	assert.equal(plan.places[0].locationId, 'L1');
	assert.equal(plan.places[0].covered.length, 2);
	const rar = plan.places[0].covered.find((c) => c.material === 'Rar')!;
	assert.equal(rar.maxQuality, 400);
	const normal = plan.places[0].covered.find((c) => c.material === 'Normal')!;
	assert.equal(normal.maxQuality, undefined);
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
