// M7-K3 (t_cfea5bea): Unit-Tests für die reine Scan-Signatur-Logik —
// lauffähig mit `node --test src/lib/scan-signature.test.ts` (Node type-
// stripping, Muster pin-content.test.ts). Die Renderer-Komponente ist
// svelte-check/Build-Gate; die Vertragslogik (null→„—", Balken-Clamp,
// Tier-Badge) wird hier abgedeckt.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { sigBarPct, sigTierBadge, sigValueDisplay } from './scan-signature.ts';

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
