// M6-F1 (t_58d46923): Unit-Tests für die generische Pin-Content-Registry —
// lauffähig mit `node --test src/lib/pin-content.test.ts` (Node type-
// stripping, Muster pin-patches.test.ts). Die Registry ist der Universality-
// Kern: Content-Typen registrieren sich seitenstark, die Pin-Ansicht wählt
// nur per Deskriptor-Typ — kein SCLM-/DealChat-Vokabular im Kern.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
	registerPinContent,
	pinContentFor,
	clearPinContentRegistry
} from './pin-content.ts';
import type { PinEntry } from './pins.ts';

// Ersatzweise zwei Dummy-Komponenten (Registry speichert nur Referenzen —
// für die Registry-Logik sind echte Svelte-Komponenten nicht nötig).
const A = (() => null) as unknown as import('svelte').Component<{ pin: PinEntry; interactive: boolean }>;
const B = (() => null) as unknown as import('svelte').Component<{ pin: PinEntry; interactive: boolean }>;

test('unbekannter/leerer Typ → null (rows-Snapshot-Fallback)', () => {
	clearPinContentRegistry();
	assert.equal(pinContentFor(undefined), null);
	assert.equal(pinContentFor(null), null);
	assert.equal(pinContentFor('ghost-type'), null);
});

test('registrierter Typ liefert den Renderer', () => {
	clearPinContentRegistry();
	registerPinContent('deal-chat', A);
	assert.equal(pinContentFor('deal-chat'), A);
	assert.notEqual(pinContentFor('other'), A);
});

test('letzte Registrierung gewinnt (Plugin-Load-Reihenfolge deterministisch)', () => {
	clearPinContentRegistry();
	registerPinContent('deal-chat', A);
	registerPinContent('deal-chat', B);
	assert.equal(pinContentFor('deal-chat'), B);
});
