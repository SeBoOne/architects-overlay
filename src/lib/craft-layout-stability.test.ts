// t_e261c7f4: Layout-Stabilität beim Qualitätsregeln — Regressions-Probe.
//
// User-Befund (0.5.81): beim Bewegen eines Material-Qualitätsreglers wuchsen
// Stat-Zeilen / Effektlabels und schoben die Materialliste aus dem
// sichtbaren Bereich. Root-Causes (behoben):
//  1. aspectQualities wurde NUR aus matQual (leer bis zum ersten Move)
//     gebaut → QualityStats-Modifier-Zeilen + combined kamen erst beim
//     ersten Regler-Move ins Layout. Fix: Seed über ALLE Kosten-Zeilen
//     (unberührt = 0, wie rowQuality).
//  2. qs-prop-val/def ohne nowrap → längere „neu“-Werte brachen um und
//     vergrößerten Zeilenhöhen. Fix: nowrap + ellipsis.
//  3. cmq-changes wuchsen mit Label-Anzahl → Regler-Zeile höher. Fix:
//     einzeilig, ellipsis, max-width.
//  4. scrollTop sprang bei $state-Updates. Fix: scrollTop vor dem Update
//     sichern, nach dem Flush (rAF) wiederherstellen.
//
// Der Probe belegt die vier Struktur-Verträge am AST/CSS der Komponenten
// (Muster wie pin-patches-render.test.ts). Laufbar mit
// `node --test src/lib/craft-layout-stability.test.ts`.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { parse } from 'svelte/compiler';

const here = dirname(fileURLToPath(import.meta.url));
const crafting = readFileSync(join(here, 'Crafting.svelte'), 'utf8');
const qualityStats = readFileSync(join(here, 'QualityStats.svelte'), 'utf8');

parse(crafting, { modern: true }); // Syntax-Check des Markup (wirft bei Fehlern)
const scriptSrc = crafting.slice(
	crafting.indexOf('<script'),
	crafting.indexOf('</script>')
);

function cssBlock(css: string, selector: string): string {
	const idx = css.indexOf(selector + ' {') >= 0 ? css.indexOf(selector + ' {') : css.indexOf(selector + '{');
	assert.ok(idx >= 0, `CSS-Selektor fehlt: ${selector}`);
	const open = css.indexOf('{', idx);
	let depth = 1;
	let i = open + 1;
	while (depth > 0 && i < css.length) {
		if (css[i] === '{') depth++;
		else if (css[i] === '}') depth--;
		i++;
	}
	return css.slice(open + 1, i - 1);
}

test('1. .cdetail trägt bind:this (Scroll-Container-Anker für Scroll-Erhalt)', () => {
	assert.ok(
		/class="cdetail" bind:this=\{detailEl\}/.test(crafting),
		'.cdetail muss bind:this={detailEl} haben'
	);
});

test('2. setRowQuality sichert scrollTop und stellt ihn nach dem Flush (rAF) wieder her', () => {
	assert.ok(/const top = scroller === null \? null : scroller\.scrollTop;/.test(scriptSrc));
	assert.ok(/requestAnimationFrame\(\(\) => \{/.test(scriptSrc));
	assert.ok(/scroller\.scrollTop = top;/.test(scriptSrc));
	// Capture VOR dem $state-Update (Reihenfolge im Funktionskörper).
	const fnIdx = scriptSrc.indexOf('function setRowQuality');
	const body = scriptSrc.slice(fnIdx, scriptSrc.indexOf('\n\t}', fnIdx));
	assert.ok(body.indexOf('scrollTop') < body.indexOf('matQual ='));
});

test('3. aspectQualities ist ges seeded: alle Kosten-Zeilen tragen eine Qualität (0 = gültig)', () => {
	const fnIdx = scriptSrc.indexOf('const aspectQualities = $derived.by');
	const body = scriptSrc.slice(fnIdx, scriptSrc.indexOf('\n\t});', fnIdx));
	// Seed läuft über detail.costs (ALLE Zeilen), nicht über Object.entries(matQual).
	assert.ok(/detail\.costs\.entries\(\)/.test(body), 'Seed muss über detail.costs laufen');
	assert.ok(!body.includes('Object.entries(matQual)'), 'matQual-only-Seed ist die Root-Cause');
	// Unberührte Regler zählen 0 (rowQuality-Default).
	assert.ok(/\?\? 0/.test(scriptSrc), 'rowQuality-Default 0 muss existieren');
});

test('4. QualityStats: Stat-Zeilen-Werte einzeilig (nowrap + ellipsis), keine Höhen-Änderung', () => {
	const val = cssBlock(qualityStats, '.qs-prop-val');
	assert.ok(val.includes('white-space: nowrap'), 'qs-prop-val: nowrap');
	assert.ok(val.includes('text-overflow: ellipsis'), 'qs-prop-val: ellipsis');
	const def = cssBlock(qualityStats, '.qs-prop-def');
	assert.ok(def.includes('white-space: nowrap'), 'qs-prop-def: nowrap');
});

test('5. M8-R4: Effektzeile EIGENE Zeile unter der Regler-Zeile — voll ausgeschrieben, mehrzeilig', () => {
	// .cmq-changes ist NICHT mehr Teil der Regler-Zeile (kein Kind von .cmq-slider).
	assert.ok(
		!/<span class="cmq-changes">/.test(crafting),
		'cmq-changes muss ein eigenes Block-Element sein (kein span in der Regler-Zeile)'
	);
	const changes = cssBlock(crafting, '.cmq-changes');
	assert.ok(changes.includes('flex-wrap'), 'cmq-changes: mehrzeilig erlaubt');
	assert.ok(!changes.includes('text-overflow: ellipsis'), 'cmq-changes: KEIN Ellipsis-Cut');
	assert.ok(!changes.includes('white-space: nowrap'), 'cmq-changes: kein Nowrap-Clamp');
	assert.ok(!changes.includes('max-width'), 'cmq-changes: Breite nicht gekappt');
	// Regler-Zeile trägt nur Balken + Wert + Toggle.
	const sliderRow = crafting.slice(crafting.indexOf('class="cmq-slider"'));
	const rowEnd = sliderRow.indexOf('</div>');
	assert.ok(sliderRow.includes('CraftQualitySlider'));
	assert.ok(sliderRow.includes('cmq-val'));
	assert.ok(sliderRow.includes('cmq-snap'));
	assert.ok(sliderRow.slice(0, rowEnd).includes('CraftQualitySlider'));
	assert.ok(
		!sliderRow.slice(0, rowEnd).includes('cmq-changes'),
		'Effekttext gehört NICHT in die Regler-Zeile'
	);
});

test('6. M8-R4: minQuality-Anzeige + Grünt/Rot-Logik komplett entfernt (tote Referenzen)', () => {
	assert.ok(!/class="cmq"/.test(crafting), 'minQuality-Badge .cmq muss weg sein');
	assert.ok(!/min_quality/.test(scriptSrc), 'i18n min_quality darf nicht mehr referenziert sein');
	assert.ok(!/rowMeets/.test(scriptSrc), 'rowMeets muss weg sein');
	assert.ok(!/requiredQuality/.test(scriptSrc), 'requiredQuality muss weg sein');
	assert.ok(!/is-ok/.test(crafting), 'kein Grün/Rot mehr an Material-/Reglerwerten');
	assert.ok(
		!/cp-result|cp-note|cp-legend|planner_title|planner_note/.test(crafting),
		'Planner-Extras weg (cp-head bleibt: Missions-Blöcke nutzen die Klasse)'
	);
	assert.ok(!/q_ok|q_fail|required_quality/.test(scriptSrc), '✓/✗-Legende + required-Zeile weg');
	assert.ok(/cmat-legend/.test(crafting), 'Einheiten-Legende hängt am Materiallisten-Kopf');
});

test('7. M8-R4: Gesamt-Qualität = MITTELWERT über alle Kosten-Zeilen (nicht min)', () => {
	const idx = scriptSrc.indexOf('const combined = $derived.by');
	const body = scriptSrc.slice(idx, scriptSrc.indexOf('\n	});', idx));
	assert.ok(body.includes('meanQuality'), 'combined muss über meanQuality mitteln');
	assert.ok(body.includes('detail.costs'), 'combined muss über detail.costs mitteln');
	assert.ok(body.includes('rowQuality'), 'combined muss die Reglerwerte (rowQuality) mitteln');
	assert.ok(!body.includes('Math.min'), 'kein min über Aspekte mehr');
});
