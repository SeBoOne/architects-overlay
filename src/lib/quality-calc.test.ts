// M8-K6 (t_f7a29367): Unit-Tests für die Modifier-Interpolation.
// Laufbar mit `node --test src/lib/quality-calc.test.ts` (Node type-stripping).
//
// Die Beispiel-Modifier stammen aus dem Live-Beleg gegen die SC-Wiki-API
// (Omnisky III Cannon, Blueprint BP_CRAFT_AMRS_LaserCannon_S1,
// 4.10.3-LIVE.13165946, 2026-11-19): Integrity 0.9→1.1, Impact Force
// 0.95→1.05, quality_range 0..1000, value_range_type "linear".

import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
	interpolateModifier,
	modifierPercent,
	statChanges,
	qualityReachable,
	scaledStat,
	type Aspect,
	type AspectModifier,
	type SummaryProperty
} from './quality-calc.ts';

const integrity: AspectModifier = {
	property_key: 'health_maxhealth',
	label: 'Integrity',
	better_when: 'higher',
	quality_range: { min: 0, max: 1000 },
	modifier_range: { at_min_quality: 0.9, at_max_quality: 1.1 },
	value_range_type: 'linear'
};

const impact: AspectModifier = {
	property_key: 'weapon_damage',
	label: 'Impact Force',
	better_when: 'higher',
	quality_range: { min: 0, max: 1000 },
	modifier_range: { at_min_quality: 0.95, at_max_quality: 1.05 },
	value_range_type: 'linear'
};

test('interpolate: Endpunkte exakt (Q0 → at_min, Q1000 → at_max)', () => {
	assert.equal(interpolateModifier(integrity, 0), 0.9);
	assert.equal(interpolateModifier(integrity, 1000), 1.1);
});

test('interpolate: linear zwischendrin (Q500 → Mittelwert)', () => {
	assert.equal(interpolateModifier(integrity, 500), 1.0);
	assert.ok(Math.abs((interpolateModifier(impact, 250) ?? 0) - 0.975) < 1e-9);
});

test('interpolate: Ziel-Qualität außerhalb wird geklemmt', () => {
	assert.equal(interpolateModifier(integrity, -50), 0.9);
	assert.equal(interpolateModifier(integrity, 2000), 1.1);
});

test('interpolate: Randfall quality_range min>max bleibt linear-konsistent', () => {
	const flipped: AspectModifier = {
		...integrity,
		quality_range: { min: 1000, max: 0 }
	};
	// at_min_quality gehört zu range.min (=1000): Q1000 → 0.9, Q0 → 1.1.
	assert.equal(interpolateModifier(flipped, 1000), 0.9);
	assert.equal(interpolateModifier(flipped, 0), 1.1);
	assert.equal(interpolateModifier(flipped, 500), 1.0);
});

test('interpolate: fehlende/unvollständige Ranges → null (ehrlich degradieren)', () => {
	assert.equal(interpolateModifier({ ...integrity, quality_range: null }, 500), null);
	assert.equal(interpolateModifier({ ...integrity, modifier_range: null }, 500), null);
	assert.equal(interpolateModifier({ ...integrity, value_range_type: 'step' }, 500), null);
	assert.equal(
		interpolateModifier({ ...integrity, modifier_range: { at_min_quality: NaN, at_max_quality: 1.1 } }, 500),
		null
	);
});

test('interpolate: degenerate Range (min==max) → at_min_quality', () => {
	const deg: AspectModifier = {
		...integrity,
		quality_range: { min: 500, max: 500 }
	};
	assert.equal(interpolateModifier(deg, 100), 0.9);
	assert.equal(interpolateModifier(deg, 900), 0.9);
});

test('modifierPercent: Faktor → %-Abweichung', () => {
	assert.equal(modifierPercent(1.1), 10);
	assert.equal(modifierPercent(0.9), -10);
	assert.equal(modifierPercent(1.05), 5);
	assert.equal(modifierPercent(null), null);
});

const SUMMARY: SummaryProperty[] = [
	{ property_key: 'health_maxhealth', label: 'Integrity', better_when: 'higher' },
	{ property_key: 'weapon_damage', label: 'Impact Force', better_when: 'higher' }
];

const ASPECTS: Aspect[] = [
	{ key: 'FRAME', name: 'Frame', modifiers: [integrity] },
	{ key: 'EMITTER', name: 'Emitter', modifiers: [impact] }
];

test('statChanges: Reihenfolge folgt summary_properties', () => {
	const changes = statChanges(ASPECTS, SUMMARY, 1000);
	assert.deepEqual(
		changes.map((c) => c.property_key),
		['health_maxhealth', 'weapon_damage']
	);
	assert.equal(changes[0].pct, 10);
	assert.equal(changes[0].better, true);
});

test('statChanges: aggregiert multiplikativ über Aspekte mit gleicher Eigenschaft', () => {
	const both: Aspect[] = [
		{ key: 'A', name: 'A', modifiers: [impact] },
		{ key: 'B', name: 'B', modifiers: [impact] }
	];
	const changes = statChanges(both, SUMMARY, 1000);
	assert.equal(changes[0].pct, 10.3); // 1.05 × 1.05 = 1.1025 → +10.25 % → 10.3
	assert.equal(changes[0].better, true);
});

test('statChanges: better_when "lower" kehrt die Ampel um', () => {
	const lower: SummaryProperty[] = [
		{ property_key: 'heat', label: 'Heat', better_when: 'lower' }
	];
	const aspects: Aspect[] = [
		{ key: 'A', name: 'A', modifiers: [{ ...integrity, property_key: 'heat' }] }
	];
	const changes = statChanges(aspects, lower, 1000);
	assert.equal(changes[0].better, false); // +10 % bei better_when=lower → schlecht
});

test('statChanges: neutrale/null-Fälle', () => {
	// Q500 → Faktor 1.0 → pct 0 → neutral
	const changes = statChanges(ASPECTS, SUMMARY, 500);
	assert.equal(changes[0].pct, 0);
	assert.equal(changes[0].better, null);
	// better_when unbekannt → Ampel neutral trotz Änderung
	const unknown: SummaryProperty[] = [
		{ property_key: 'health_maxhealth', label: 'Integrity', better_when: 'banana' }
	];
	const c2 = statChanges([ASPECTS[0]], unknown, 1000);
	assert.equal(c2[0].pct, 10);
	assert.equal(c2[0].better, null);
	// Gar keine berechenbaren Modifier → leere Liste
	assert.deepEqual(statChanges([{ key: 'X', name: 'X', modifiers: null }], SUMMARY, 500), []);
});

test('statChanges: unbekannte Keys hängen hinter summary_properties', () => {
	const aspects: Aspect[] = [
		{
			key: 'A',
			name: 'A',
			modifiers: [
				integrity,
				{ ...impact, property_key: 'extra_prop', label: 'Extra' }
			]
		}
	];
	const changes = statChanges(aspects, SUMMARY, 1000);
	assert.deepEqual(
		changes.map((c) => c.property_key),
		['health_maxhealth', 'extra_prop']
	);
});

test('qualityReachable: null/undefined = keine Grenze, sonst Vergleich', () => {
	assert.equal(qualityReachable(500, null), true);
	assert.equal(qualityReachable(500, undefined), true);
	assert.equal(qualityReachable(500, 400), true);
	assert.equal(qualityReachable(400, 500), false);
	assert.equal(qualityReachable(500, 500), true);
});

// ---------- M8-R1-A: individuelle Regler + Kombination ----------

import {
	aspectQuality,
	combinedQuality,
	materialKey,
	nearestSnap,
	statChangesPerAspect,
	weightedMean
} from './quality-calc.ts';

test('materialKey: trim + lowercase (exakt, wie Rust normalize_name)', () => {
	assert.equal(materialKey('  Iron  '), 'iron');
	assert.equal(materialKey(null), '');
});

test('weightedMean: mengengewichtet, fehlende Gewichte = 1.0', () => {
	assert.equal(weightedMean([]), null);
	assert.equal(
		weightedMean([
			{ weight: 0.64, quality: 200 },
			{ weight: 0.32, quality: 800 }
		]),
		(0.64 * 200 + 0.32 * 800) / 0.96
	);
	// Ohne Gewicht → einfaches Mittel
	assert.equal(
		weightedMean([
			{ weight: null, quality: 100 },
			{ weight: null, quality: 300 }
		]),
		200
	);
	// Nur ungültige Qualitäten → null
	assert.equal(weightedMean([{ weight: 1, quality: null }]), null);
});

test('aspectQuality: mengengewichtetes Mittel der gematchten Materialien', () => {
	const aspects: Aspect[] = [
		{
			key: 'FRAME',
			name: 'Frame',
			input: { name: 'Iron', quantity_scu: 0.64 },
			modifiers: [integrity]
		},
		{
			key: 'BARREL',
			name: 'Barrel',
			input: { name: 'Titanium', quantity_scu: 0.32 },
			modifiers: [impact]
		}
	];
	const mq = new Map([
		['iron', 200],
		['titanium', 800]
	]);
	assert.equal(aspectQuality(aspects[0], mq), 200);
	assert.equal(aspectQuality(aspects[1], mq), 800);
	// Kein gematchtes Material → null
	assert.equal(aspectQuality(aspects[0], new Map()), null);
});

test('combinedQuality: min über die Aspekt-Qualitäten', () => {
	const aspects: Aspect[] = [
		{ key: 'A', name: 'A', input: { name: 'Iron', quantity_scu: 1 }, modifiers: [integrity] },
		{ key: 'B', name: 'B', input: { name: 'Titanium', quantity_scu: 1 }, modifiers: [impact] }
	];
	const mq = new Map([
		['iron', 700],
		['titanium', 300]
	]);
	assert.equal(combinedQuality(aspects, mq), 300);
	// Nichts berechenbar → null
	assert.equal(combinedQuality(aspects, new Map()), null);
});

test('statChangesPerAspect: jede Aspekt-Qualität einzeln interpoliert, Faktor-Produkt', () => {
	// Zwei Aspekte auf dieselbe Eigenschaft (weapon_damage) mit
	// unterschiedlichen Qualitäten — multiplikativ (K6-Modell).
	const aspects: Aspect[] = [
		{
			key: 'CYCLER',
			name: 'Cycler',
			input: { name: 'Riccite' },
			modifiers: [{ ...impact, quality_range: { min: 0, max: 1000 }, modifier_range: { at_min_quality: 0.9, at_max_quality: 1.1 } }]
		},
		{
			key: 'BARREL',
			name: 'Barrel',
			input: { name: 'Titanium' },
			modifiers: [{ ...impact, quality_range: { min: 0, max: 1000 }, modifier_range: { at_min_quality: 0.9, at_max_quality: 1.1 } }]
		}
	];
	// Cycler 1000 → 1.1, Barrel 0 → 0.9 → Produkt 0.99 = −1 %
	const aq = new Map([
		['CYCLER', 1000],
		['BARREL', 0]
	]);
	const ch = statChangesPerAspect(aspects, SUMMARY, aq);
	assert.equal(ch.length, 1);
	assert.equal(ch[0].property_key, 'weapon_damage');
	assert.equal(ch[0].pct, -1);
	// Aspekt ohne Qualität fällt still raus
	const ch2 = statChangesPerAspect(aspects, SUMMARY, new Map([['CYCLER', 1000]]));
	assert.equal(ch2[0].pct, 10);
});

test('nearestSnap: nächster Punkt, null bei leerer Liste', () => {
	const pts = [325, 521, 664, 1000];
	assert.equal(nearestSnap(pts, 330), 325);
	assert.equal(nearestSnap(pts, 500), 521);
	assert.equal(nearestSnap(pts, 999), 1000);
	assert.equal(nearestSnap([], 500), null);
	});

// M8-R1-C (t_b6ee9f14): scaledStat — API-Basiswert × Modifier-Faktor für
// die Stats-Vollanzeige (default | neu).
test('scaledStat: Basiswert × Faktor, auf 1 Nachkommastelle gerundet', () => {
	assert.equal(scaledStat(73, 1.05), 76.7); // 76.65 → 76.7 (round half up)
	assert.equal(scaledStat(73, 1.1), 80.3);
	assert.equal(scaledStat(100, 1.0), 100);
	assert.equal(scaledStat(492, 0.95), 467.4);
});

test('scaledStat: null/undefined-Basis oder -Faktor → null (nichts erfinden)', () => {
	assert.equal(scaledStat(null, 1.05), null);
	assert.equal(scaledStat(undefined, 1.05), null);
	assert.equal(scaledStat(73, null), null);
	assert.equal(scaledStat(73, undefined), null);
});
