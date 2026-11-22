// M9-P (t_fb405e8d, P7+P8+P9) — Tests für die geteilte Formular-Logik
// (Vorbefüllung Restbedarf + Auto-Reserve-Cap). Lauf:
//   node --test src/lib/list-actions.test.ts
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { openRest, reserveCap, type AggLite } from './list-actions.ts';

const agg = (quantity: number, done_qty: number, reserved_qty: number): AggLite => ({
	quantity,
	done_qty,
	reserved_qty
});

// ---------- openRest: Bedarf − erledigt − reserviert ------------------------

test('openRest: voll offen ohne done/reserved', () => {
	assert.equal(openRest(agg(10, 0, 0)), 10);
});

test('openRest: done zählt gegen den Bedarf', () => {
	assert.equal(openRest(agg(10, 3, 0)), 7);
});

test('openRest: reserved zählt gegen den Bedarf (M9-M)', () => {
	assert.equal(openRest(agg(10, 3, 4)), 3);
});

test('openRest: done + reserved über Bedarf → 0', () => {
	assert.equal(openRest(agg(7, 5, 4)), 0);
});

test('openRest: Bruchteile auf 2 Dezimalen', () => {
	assert.equal(openRest(agg(0.3, 0.1, 0.05)), 0.15);
});

// ---------- reserveCap: nie über den Listenbedarf (M9-I-Cap) ---------------

test('reserveCap: 10 hinzugefügt, 7 benötigt → 7 reserviert (Ziel Done)', () => {
	assert.equal(reserveCap(10, 7, 0), 7);
});

test('reserveCap: nur 4 von 7 → 4 reserviert (Teil)', () => {
	assert.equal(reserveCap(4, 7, 0), 4);
});

test('reserveCap: bereits reservierte Menge bleibt unberührt', () => {
	assert.equal(reserveCap(10, 7, 3), 4);
});

test('reserveCap: Bedarf schon voll → nichts mehr', () => {
	assert.equal(reserveCap(10, 7, 7), 0);
	assert.equal(reserveCap(5, 7, 9), 0);
});

test('reserveCap: Bedarf 0 → nichts', () => {
	assert.equal(reserveCap(10, 0, 0), 0);
});

test('reserveCap: Bruchteile auf 2 Dezimalen', () => {
	assert.equal(reserveCap(0.375, 0.25, 0), 0.25);
});
