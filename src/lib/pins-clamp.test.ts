// t_01f1a9ed (Punkt 5): Unit-Test für die Box-Clamp-Logik des Pin-Fensters —
// lauffähig mit `node --test src/lib/pins-clamp.test.ts` (Node type-stripping,
// kein Framework, Muster wie window-breakpoints.test.ts). Eine geclampte Box
// ist immer mindestens randberührt sichtbar, egal welche verwahrten
// Koordinaten aus der Persistenz kommen.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { clampBoxToViewport } from './pins.ts';

test('innerhalb des Viewports bleibt unverändert', () => {
	assert.deepEqual(clampBoxToViewport(500, 300, 356, 120, 3440, 1440), { x: 500, y: 300 });
});

test('vollständig rechts/unten außerhalb → randberührt zurückgezogen', () => {
	// x=3400 bei w=356 auf 3440: Box ragt hinaus → x = 3440-356.
	assert.deepEqual(clampBoxToViewport(3400, 0, 356, 120, 3440, 1440), {
		x: 3440 - 356,
		y: 0
	});
	assert.deepEqual(clampBoxToViewport(0, 1400, 356, 120, 3440, 1440), {
		x: 0,
		y: 1440 - 120
	});
});

test('negative Koordinaten → 0 (Box bleibt sichtbar)', () => {
	assert.deepEqual(clampBoxToViewport(-500, -50, 356, 120, 3440, 1440), { x: 0, y: 0 });
});

test('Box größer als der Viewport → Ursprung (nie vollständig draußen)', () => {
	assert.deepEqual(clampBoxToViewport(100, 100, 4000, 2000, 3440, 1440), { x: 0, y: 0 });
});

test('Sebo-Befund: 3440er-Koordinaten im 800×600-Viewport werden auf sichtbar geclampt', () => {
	// Lokaltest: Boxen mit screen-relativen x/y für 3440×1440 landeten im
	// 800×600-Default-Viewport → geclippt. Nach dem Clamp bleibt die Box
	// randberührt im Viewport statt im Nichts.
	const c = clampBoxToViewport(3000, 700, 356, 120, 800, 600);
	assert.equal(c.x, 800 - 356);
	assert.equal(c.y, 600 - 120);
});
