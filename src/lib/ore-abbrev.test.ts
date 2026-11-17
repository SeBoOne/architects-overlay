// M7-F3 (t_cf1224c5, Punkt 6): Unit-Test für oreAbbrev gegen die ECHTE
// Erzliste (Strata-Live-Fixtures). Laufbar mit
// `node --test src/lib/ore-abbrev.test.ts` (Node type-stripping).
//
// Hinweis: „Carinite (Raw)“ existiert doppelt in der API-Liste (fps + 
// ground_vehicle, verschiedene ids) — die Kürzel sind pro NAME eindeutig,
// die Duplikate teilen sich zwangsläufig „Ca“ (gleicher Basisname).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';
import { oreAbbrev } from './ore-abbrev.ts';

const FIXTURE = fileURLToPath(
	new URL('../../crates/strata-client/tests/fixtures-live/ores.live.json', import.meta.url)
);

// fileURLToPath statt .pathname — Leerzeichen im Pfad („Architects Overlay“)
// brechen als %20 (ENOENT).
const REPO_ROOT = dirname(dirname(FIXTURE));

test('oreAbbrev: Beispiele aus der Spec', () => {
	assert.equal(oreAbbrev('Agricium (Ore)'), 'Ag');
	assert.equal(oreAbbrev('Beryl (Raw)'), 'Be');
	assert.equal(oreAbbrev('Quartz (Raw)'), 'Qu');
	assert.equal(oreAbbrev('Gold (Ore)'), 'Go');
});

test('oreAbbrev: Klammerzusätze fließen nicht ein', () => {
	// Befund User: „Agricium (Ore) → aktuell AO, richtig wäre Ag“
	assert.notEqual(oreAbbrev('Agricium (Ore)'), 'AO');
	assert.equal(oreAbbrev('Hephaestanite (R'), 'He'); // abgeschnittener Klammerzusatz der Live-API
	assert.equal(oreAbbrev('Tungsten (Ore)'), 'Tu');
});

test('oreAbbrev: Randfälle', () => {
	assert.equal(oreAbbrev('Quartz'), 'Qu');
	assert.equal(oreAbbrev('(Raw)'), 'Ra'); // nur Klammer → Fallback voller Name
	assert.equal(oreAbbrev('!!!'), '??'); // gar keine Buchstaben
	assert.equal(oreAbbrev(''), '??');
});

test('oreAbbrev: alle 38 Live-Erze erzeugen erwartete Kürzel', () => {
	const doc = JSON.parse(readFileSync(FIXTURE, 'utf8'));
	const ores = doc.ores ?? doc;
	assert.equal(ores.length, 38);
	const byName = new Map<string, string>();
	for (const o of ores) {
		const abbr = oreAbbrev(o.name);
		assert.match(abbr, /^[A-Z][a-z]?$/, `${o.name} → ${abbr}`);
		// Kürzel stammt aus dem Basisnamen, nie aus dem Klammerzusatz:
		const base = o.name.replace(/\s*\(.*$/, '').trim();
		assert.ok(abbr.toLowerCase().startsWith(base[0].toLowerCase()), `${o.name}: ${abbr}`);
		const prev = byName.get(o.name);
		if (prev !== undefined) assert.equal(abbr, prev, 'gleicher Name → gleiches Kürzel');
		byName.set(o.name, abbr);
	}
	// Eindeutigkeit pro Name (die Carinite-Duplikate der API teilen „Ca“):
	assert.equal(byName.size, 37);
});
