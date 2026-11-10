// M4-Fix (t_df166209, Befund 1): Unit-Test für den i18n-FLAT-Vertrag
// (src/lib/i18n.ts): `strings` ist eine flache Map mit Punktnamen
// ("pins.pin_detail": …) — KEIN verschachtelter Block (weder an der
// Wurzel noch unter `strings`). Lauffähig mit
// `node --test src/lib/locales-flat.test.ts` (Node type-stripping,
// kein Framework nötig — Muster window-breakpoints.test.ts).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const LOCALES_DIR = new URL('../../resources/locales/', import.meta.url).pathname;

function loadStrings(loc: string): Record<string, string> {
	const doc = JSON.parse(readFileSync(join(LOCALES_DIR, `${loc}.json`), 'utf8'));
	return doc.strings;
}

function extractTKeys(): string[] {
	const keys = new Set<string>();
	for (const dir of ['src', 'src/lib', 'src/routes']) {
		for (const f of readdirSync(join(LOCALES_DIR, '../../', dir)).filter((f) => f.endsWith('.svelte') || f.endsWith('.ts'))) {
			const text = readFileSync(join(LOCALES_DIR, '../../', dir, f), 'utf8');
			for (const m of text.matchAll(/\$t\('([^']+)'\)|tr\('([^']+)'\)/g)) {
				const key = m[1] ?? m[2];
				if (key) keys.add(key);
			}
		}
	}
	return [...keys];
}

const PIN_KEYS = [
	'pins.pin_hint',
	'pins.pin_detail',
	'pins.pin_short',
	'pins.pin_detail_title',
	'pins.added',
	'pins.already_pinned',
	'pins.drag_title',
	'pins.opacity_title',
	'pins.opacity',
	'pins.unpin_title',
	'pins.close_title',
	'pins.empty'
];

test('kein verschachtelter pins-Block an der Wurzel', () => {
	for (const loc of ['de', 'en']) {
		const doc = JSON.parse(readFileSync(join(LOCALES_DIR, `${loc}.json`), 'utf8'));
		assert.equal('pins' in doc, false, `${loc}: Root-"pins"-Block muss weg`);
	}
});

test('kein verschachteltes Objekt unter strings', () => {
	for (const loc of ['de', 'en']) {
		for (const [k, v] of Object.entries(loadStrings(loc))) {
			assert.equal(typeof v, 'string', `${loc}: strings["${k}"] ist kein Leaf-String`);
		}
	}
});

test('alle pins.*-Keys flach in strings beider Dateien', () => {
	for (const loc of ['de', 'en']) {
		const st = loadStrings(loc);
		for (const key of PIN_KEYS) {
			assert.equal(typeof st[key], 'string', `${loc}: strings["${key}"] fehlt`);
		}
	}
});

test('pins.*-Inhalte nicht vertauscht (de=deutsch, en=englisch)', () => {
	const de = loadStrings('de')['pins.pin_detail'];
	const en = loadStrings('en')['pins.pin_detail'];
	assert.equal(de, 'Als Pin anheften');
	assert.equal(en.toLowerCase(), 'pin as box');
});

test('alle $t()-Literal-Keys des Frontends resolvbar', () => {
	const st = loadStrings('en');
	const missing = extractTKeys().filter((k) => !(k in st));
	assert.deepEqual(missing, [], 'Fehlende Keys in en.json');
});
