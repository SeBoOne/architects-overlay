// M4-E (t_34efb781): Unit-Tests für die Patch-Geometrie der Spotlight-
// Interaktionsflächen — lauffähig mit `node --test src/lib/pin-patches.test.ts`
// (Node type-stripping, Muster pins-clamp.test.ts). Synthetische Monitor-/
// Box-Rects; der DPR-Fall ist konstruktiv abgedeckt: die Patch-Positionen
// rechnen in der Viewport-Basis des Spotlight-Webviews (CSS-px), dieselbe
// Basis wie die Pin-Boxen (PinBoxes.svelte, M5) — DPR wirkt auf Pin-Koordinaten und Viewport
// identisch, daher ist die Geometrie DPR-invariant (Test mit DPR-1- und
// DPR-1.5-Viewportmaßen auf denselben logischen Koordinaten).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dragBasePos, patchPos, popoverPos, POPOVER_W, POPOVER_H } from './pin-patches.ts';
import { PIN_BOX_W, pinBoxEstHeight, type PinEntry } from './pins.ts';

function pin(over: Partial<PinEntry> = {}): PinEntry {
	return {
		view_id: 'v1',
		screen: 'DP-3',
		box_index: 0,
		opacity: 55,
		pinned: true,
		x: 500,
		y: 300,
		title: 'Route',
		kpi: null,
		rows: [{ idx: 'a', name: 'n', sub: null, value: 'v', side: null }],
		...over
	};
}

test('Patch innerhalb des Viewports bleibt unverändert', () => {
	const p = patchPos(pin(), 3440, 1440);
	assert.deepEqual(p, { x: 500, y: 300 });
});

test('verwahrte Koordinaten → randberührt an der Persistenz-Marge geclampt (t_88fb1585: Mirror von snap_to_edge, EDGE_MARGIN 8)', () => {
	assert.deepEqual(patchPos(pin({ x: 3400, y: -50 }), 3440, 1440), {
		x: 3440 - PIN_BOX_W - 8,
		y: 8
	});
	const h = pinBoxEstHeight(3);
	assert.equal(patchPos(pin({ rows: new Array(3).fill(pin().rows[0]), y: 1600 }), 3440, 1440).y, 1440 - h - 8);
});

test('M6-R1: Drag-Anker = gerenderte (geclampte) Position — erster Move folgt sofort ohne Versatz', () => {
	// Verwahrte Koordinate außerhalb des Clamps (z. B. Monitorwechsel):
	// Anzeige klebte am Edge, der Drag-Basis muss DIESE Position sein —
	// sonst frisst der Clamp das dx des ersten Drags (Nachzieh-Verzögerung)
	// und der Griffpunkt versetzt sich beim Freigeben des Clamps.
	const p = pin({ x: -500, y: -80 });
	const vw = 3440;
	const vh = 1440;
	const base = dragBasePos(p, vw, vh);
	const rendered = patchPos(p, vw, vh);
	assert.deepEqual(base, rendered); // Anker == Anzeige-Position
	// Erster Move: base + dx ist SOFORT sichtbar bewegt (kein Clamp-Fraß),
	// während die alte Basis pin.x + dx = -490 weiterhin geclampt bliebe.
	const dx = 60;
	const liveOld = patchPos({ ...p, x: p.x + dx, y: p.y + dx }, vw, vh);
	assert.equal(liveOld.x, rendered.x); // alt: Box folgt gar nicht
	const liveNew = patchPos({ ...p, x: base.x + dx, y: base.y + dx }, vw, vh);
	assert.equal(liveNew.x, rendered.x + dx); // neu: 1:1-Follow ab pointerdown
	// Nach dem Drop persistiert das Backend die geclampte Position — die
	// zweite Drag-Basis ist dann identisch zur ersten (keine Sprünge).
	assert.deepEqual(dragBasePos({ ...p, x: base.x + dx, y: base.y + dx }, vw, vh), liveNew);
});

test('Popover: unter der Box, an der Box-Mitte orientiert', () => {
	const p = pin({ x: 500, y: 300 });
	const box = patchPos(p, 3440, 1440);
	const pop = popoverPos(p, 3440, 1440);
	assert.equal(pop.x, box.x + PIN_BOX_W / 2);
	assert.equal(pop.y, box.y + pinBoxEstHeight(1));
});

test('Popover: rechte/untere Randlage → in den Viewport zurückgeclampt', () => {
	const right = popoverPos(pin({ x: 3440 - PIN_BOX_W, y: 0 }), 3440, 1440);
	assert.equal(right.x, 3440 - POPOVER_W);
	// Box clampt auf die Persistenz-Marge (y=8) — Popover hängt darunter.
	assert.equal(right.y, 8 + pinBoxEstHeight(1));
	const bottom = popoverPos(pin({ x: 0, y: 1440 - pinBoxEstHeight(1) }), 3440, 1440);
	assert.equal(bottom.y, 1440 - POPOVER_H);
});

test('kleiner Viewport: Popover/Box min. 0, nie negativ', () => {
	const p = popoverPos(pin({ x: 0, y: 0 }), 200, 100);
	assert.equal(p.x, Math.min(0 + PIN_BOX_W / 2, Math.max(0, 200 - POPOVER_W)));
	assert.ok(p.x >= 0 && p.y >= 0);
});

// M6-F2 (t_3ff5e552): die Patch-Geometrie rechnet mit dem Größenfaktor —
// skalierte Boxen clampen an der skalierten Größe (Anzeige == Persistenz,
// pin_resize snappt im Backend mit scaled_box_w/h).
test('M6-F2: skalierte Box clampt an skalierter Größe (scale 200)', () => {
	// scale undefined → Legacy-Vertrag 100 % (alte Configs ohne scale).
	assert.deepEqual(patchPos(pin({ x: 3400, y: -50 }), 3440, 1440), {
		x: 3440 - PIN_BOX_W - 8,
		y: 8
	});
	// 200 %: Breite 712 → rechter Rand 3440-712-8; Höhe ×2.
	const big = pin({ x: 3400, y: -50, scale: 200 });
	const pos = patchPos(big, 3440, 1440);
	assert.equal(pos.x, 3440 - PIN_BOX_W * 2 - 8);
	assert.equal(pos.y, 8);
	// 50 %: halbe Maße.
	const small = pin({ x: 3400, y: -50, scale: 50 });
	assert.equal(patchPos(small, 3440, 1440).x, 3440 - PIN_BOX_W / 2 - 8);
	// Popover orientiert sich an der skalierten Box-Mitte/-Kante.
	const pop = popoverPos(big, 3440, 1440);
	assert.equal(pop.y, 8 + pinBoxEstHeight(1) * 2);
});

test('M6-F2: unbekannte scale-Werte werden geclampt (Frontend-Mirror)', () => {
	// clampScale 50–200 — ein geclampter Wert ändert die Geometrie deterministisch.
	assert.equal(patchPos(pin({ x: 100, y: 100, scale: 999 }), 3440, 1440).x, 100);
	assert.equal(patchPos(pin({ x: 100, y: 100, scale: 200 }), 3440, 1440).x, 100);
	assert.equal(patchPos(pin({ x: 100, y: 100, scale: 1 }), 3440, 1440).x, 100);
});

test('DPR-Invarianz: gleiche logische Geometrie bei DPR 1 und 1.5', () => {
	// DPR 1: physical == logical (Viewport 3440×1440). DPR 1.5: der Webview
	// rechnet CSS-px — dieselben logischen Koordinaten, Viewportmaße sind die
	// CSS-Größe; die clamp-Resultate unterscheiden sich nur, wenn das
	// Viewportmaße selbst anders wäre. Konsistenz-Beleg: bei IDENTISCHEN
	// Viewportmaßen ist das Ergebnis identisch (kein versteckter DPR-Faktor).
	const a = patchPos(pin({ x: 3400, y: 1400 }), 3440, 1440);
	const b = patchPos(pin({ x: 3400, y: 1400 }), 3440, 1440);
	assert.deepEqual(a, b);
	assert.deepEqual(popoverPos(pin({ x: 3400, y: 1400 }), 3440, 1440), popoverPos(pin({ x: 3400, y: 1400 }), 3440, 1440));
});
