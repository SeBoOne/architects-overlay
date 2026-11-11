// t_01f1a9ed (Punkt 5) / t_88fb1585 (M5-R2): Unit-Test für den
// Screen-Clamp der Pin-Boxen — lauffähig mit
// `node --test src/lib/pins-clamp.test.ts` (Node type-stripping).
// snapBoxToScreen ist der MIRROR von pins.rs snap_to_edge: die Erwartungen
// unten sind dieselben Fälle wie die Rust-Tests `snapping_pulls_box_back_
// inside` — Anzeige und Persistenz müssen identisch rechnen, sonst springt
// die Box nach dem Drop (Sebo-Befund M5-R2).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { snapBoxToScreen, EDGE_MARGIN } from './pins.ts';

test('innerhalb des Screens bleibt unverändert (wie snap_to_edge 500,500)', () => {
	assert.deepEqual(snapBoxToScreen(500, 300, 356, 120, 3440, 1440), { x: 500, y: 300 });
});

test('links raus → EDGE_MARGIN (Mirror: snap_to_edge(-50,100,…)=EDGE_MARGIN)', () => {
	assert.deepEqual(snapBoxToScreen(-50, 100, 356, 120, 1920, 1080), {
		x: EDGE_MARGIN,
		y: 100
	});
});

test('rechts raus → screen-w-MARGIN (Mirror: 1920-356-8)', () => {
	assert.deepEqual(snapBoxToScreen(1900, 100, 356, 120, 1920, 1080), {
		x: 1920 - 356 - EDGE_MARGIN,
		y: 100
	});
});

test('oben raus → EDGE_MARGIN (Mirror: snap_to_edge(100,-5,…))', () => {
	assert.deepEqual(snapBoxToScreen(100, -5, 356, 120, 1920, 1080), {
		x: 100,
		y: EDGE_MARGIN
	});
});

test('unten raus → screen-h-MARGIN (Mirror: 1080-120-8)', () => {
	assert.deepEqual(snapBoxToScreen(100, 1100, 356, 120, 1920, 1080), {
		x: 100,
		y: 1080 - 120 - EDGE_MARGIN
	});
});

test('Box breiter als der Screen → Ursprung nach Margin (Mirror-Test)', () => {
	assert.deepEqual(snapBoxToScreen(50, 50, 2000, 120, 1920, 1080), {
		x: EDGE_MARGIN,
		y: 50
	});
});

test('verwahrte negative Koordinaten → randberührt an der Margin', () => {
	assert.deepEqual(snapBoxToScreen(-500, -50, 356, 120, 3440, 1440), {
		x: EDGE_MARGIN,
		y: EDGE_MARGIN
	});
});

// Konsistenz-Beleg M5-R2: patchPos über pin-patches.ts muss DASSELBE
// Ergebnis liefern wie dieser Mirror (eine Positions-Quelle, keine
// PatchPos-Snap-Divergenz mehr).
test('patchPos rechnet identisch zu snapBoxToScreen (Anzeige=Persistenz)', async () => {
	const { patchPos } = await import('./pin-patches.ts');
	const pin = {
		view_id: 'v1',
		screen: 'DP-3',
		box_index: 0,
		opacity: 55,
		pinned: true,
		x: 3076,
		y: 100,
		title: 'Route',
		kpi: null,
		rows: [{ idx: 'a', name: 'n', sub: null, value: 'v', side: null }]
	};
	// Drag simuliert: 3076-dx (nach-links-Ziehen über den rechten Rand hinaus)
	for (const x of [3076, 2400, 0, -20, 3440, 5000]) {
		const direct = snapBoxToScreen(x, 100, 356, 78, 3440, 1440);
		const viaPatch = patchPos({ ...pin, x }, 3440, 1440);
		assert.deepEqual(
			{ x: viaPatch.x, y: viaPatch.y },
			direct,
			`patchPos(${x}) weicht vom Persistenz-Mirror ab`
		);
	}
});
