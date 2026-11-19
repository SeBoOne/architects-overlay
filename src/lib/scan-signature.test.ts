// M7-K3 (t_cfea5bea): Unit-Tests für die reine Scan-Signatur-Logik —
// lauffähig mit `node --test src/lib/scan-signature.test.ts` (Node type-
// stripping, Muster pin-content.test.ts). Die Renderer-Komponente ist
// svelte-check/Build-Gate; die Vertragslogik (null→„—", Balken-Clamp,
// Tier-Badge) wird hier abgedeckt.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
	sigBarPct,
	sigEntries,
	sigMultiples,
	sigRowsFor,
	sigTierBadge,
	sigToggleEntry,
	sigValueDisplay
} from './scan-signature.ts';

test('sigValueDisplay: fehlende Daten → „—" (Karten-Spec: pinnt trotzdem)', () => {
	assert.equal(sigValueDisplay(null), '—');
	assert.equal(sigValueDisplay(undefined), '—');
	assert.equal(sigValueDisplay(Number.NaN), '—');
});

test('sigValueDisplay: Zahl bleibt Zahl', () => {
	assert.equal(sigValueDisplay(0), '0');
	assert.equal(sigValueDisplay(42.5), '42.5');
});

test('sigBarPct: relativ zum Chart-Max, geclampt auf 0..100', () => {
	assert.equal(sigBarPct(50, 100), 50);
	assert.equal(sigBarPct(100, 100), 100);
	assert.equal(sigBarPct(0, 100), 0);
	assert.equal(sigBarPct(150, 100), 100); // Über-Max clampt
	assert.equal(sigBarPct(-5, 100), 0); // Negativ clampt
});

test('sigBarPct: kein Balken ohne Wert/Max/ungültigem Max', () => {
	assert.equal(sigBarPct(null, 100), null);
	assert.equal(sigBarPct(undefined, 100), null);
	assert.equal(sigBarPct(50, null), null);
	assert.equal(sigBarPct(50, undefined), null);
	assert.equal(sigBarPct(50, 0), null); // Max <= 0
	assert.equal(sigBarPct(50, Number.NaN), null);
});

test('sigBarPct: 0.0 ist gültige Daten (M7-K1-Disziplin: null ≠ 0)', () => {
	assert.equal(sigBarPct(0, 100), 0);
	assert.equal(sigValueDisplay(0), '0');
});

test('sigTierBadge: nur bei vorhandener Stufe', () => {
	assert.equal(sigTierBadge(1), 'T1');
	assert.equal(sigTierBadge(6), 'T6');
	assert.equal(sigTierBadge(4.4), 'T4');
	assert.equal(sigTierBadge(null), null);
	assert.equal(sigTierBadge(undefined), null);
});

// ---- M7-F4 (t_c3367dfb): Sammel-Signatur-Pin (P1 Dedupe + P2 Payload) ----
test('sigEntries: dedupe nach oreId — jeder Wert nur EINMAL (P1)', () => {
	const entries = sigEntries({
		entries: [
			{ oreId: 'a', label: 'Quantum', value: 4400, rarity: 'rare' },
			{ oreId: 'a', label: 'Quantum', value: 4400, rarity: 'rare' },
			{ oreId: 'b', label: 'Copper', value: 120, rarity: 'common' }
		]
	});
	assert.equal(entries.length, 2);
	assert.deepEqual(entries.map((e) => e.oreId), ['a', 'b']);
});

test('sigEntries: nicht-objektige/ohne-oreId Einträge raus (F3-Contract: oreId ist Key)', () => {
	const entries = sigEntries({
		entries: [null, 42, { label: 'Laranite', value: null, rarity: null }, { oreId: '', label: '' }]
	} as never);
	assert.equal(entries.length, 0);
});

test('sigEntries: fehlende/ungültige Payload → leere Liste (kein Throw)', () => {
	assert.deepEqual(sigEntries(null), []);
	assert.deepEqual(sigEntries({ entries: undefined }), []);
	assert.deepEqual(sigEntries({ entries: null }), []);
});

test('sigRowsFor: eine Row pro Entry, idx als String (pin_add-Contract)', () => {
	const rows = sigRowsFor([
		{ oreId: 'a', label: 'Quantum', value: 4400, rarity: 'rare' },
		{ oreId: 'b', label: 'Copper', value: null, rarity: 'common' }
	]);
	assert.equal(rows.length, 2);
	assert.deepEqual(rows[0], { idx: '0', name: 'Quantum', value: '4400' });
	assert.deepEqual(rows[1], { idx: '1', name: 'Copper', value: '—' });
});

test('sigToggleEntry: add und remove (dedupe nach oreId, Reihenfolge stabil)', () => {
	let e: { oreId: string; label: string; value: number | null; rarity?: string | null }[] = [
		{ oreId: 'a', label: 'Quantum', value: 4400 },
		{ oreId: 'b', label: 'Copper', value: 120 }
	];
	e = sigToggleEntry(e, { oreId: 'c', label: 'Titanium', value: 9 });
	assert.deepEqual(e.map((x) => x.oreId), ['a', 'b', 'c']);
	e = sigToggleEntry(e, { oreId: 'b', label: 'Copper', value: 120 });
	assert.deepEqual(e.map((x) => x.oreId), ['a', 'c']); // remove, nicht add
	e = sigToggleEntry(e, { oreId: 'c', label: 'Titanium', value: 9 });
	assert.deepEqual(e.map((x) => x.oreId), ['a']); // Toggle remove
});

test('sigToggleEntry: Rarity wird im Entry mitgefuehrt (P2 Payload)', () => {
	let e = sigToggleEntry([], { oreId: 'a', label: 'Quantum', value: 4400, rarity: 'rare' });
	assert.equal(e[0].rarity, 'rare');
});

test('sigMultiples: beginnt bei x2 — die Basis IST x1 (M7-F5d, kein Doppel)', () => {
	const rows = sigMultiples(3000, 5);
	assert.deepEqual(
		rows.map((r) => r.display),
		['6000', '9000', '12000', '15000']
	);
	assert.deepEqual(
		rows.map((r) => r.n),
		[2, 3, 4, 5]
	);
	// slots=2 → nur x2; slots=1 → keine Stufen-Zellen (Basis allein)
	assert.deepEqual(sigMultiples(3000, 2).map((r) => r.display), ['6000']);
	assert.deepEqual(sigMultiples(3000, 1), []);
	// null-Wert → „—"-Zellen ab x2
	assert.deepEqual(sigMultiples(null, 3).map((r) => r.display), ['—', '—']);
});
