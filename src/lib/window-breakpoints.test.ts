// M2 (t_a931d1bb): Unit-Test für die Breakpoint-Klassenlogik des
// Fenstermodus — lauffähig mit `node --test src/lib/window-breakpoints.test.ts`
// (Node type-stripping, kein Framework nötig). Das Frontend selbst hat nur
// `pnpm run check` als Gate; dieser Test dokumentiert die Schwellwerte
// maschinell und läuft bewusst ohne neue Dev-Dependency (vitest etc.).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { appCompact, hintMore, APP_WINDOW_COMPACT_PX, APP_WINDOW_WIDE_PX } from './window-breakpoints.ts';

test('appCompact: nur im Fenstermodus', () => {
	assert.equal(appCompact(800, false), false);
	assert.equal(appCompact(800, true), true);
});

test('appCompact: ungemessene Breite (0) nie kompakt', () => {
	assert.equal(appCompact(0, true), false);
});

test('appCompact: Schwelle 900 — darunter kompakt, ab 900 breit', () => {
	assert.equal(appCompact(APP_WINDOW_COMPACT_PX, true), false);
	assert.equal(appCompact(APP_WINDOW_COMPACT_PX - 1, true), true);
	assert.equal(appCompact(1400, true), false);
	assert.equal(appCompact(APP_WINDOW_WIDE_PX, true), false);
});

test('hintMore: breit nie gekürzt', () => {
	assert.equal(hintMore(5, false), 0);
});

test('hintMore: kompakt — erste drei bleiben, Rest als +N', () => {
	assert.equal(hintMore(5, true), 2);
	assert.equal(hintMore(3, true), 0);
	assert.equal(hintMore(2, true), 0);
});
