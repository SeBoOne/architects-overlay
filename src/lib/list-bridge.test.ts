// list-bridge (M9-D, t_36f3966c) — Tests für die Craft→Mining-Brücke.
import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { bridgeItems } from './list-bridge.ts';

describe('list-bridge', () => {
	it('builds one mining item per aggregated material — qty as copies, MAX-quality as requirement', () => {
		const items = bridgeItems({
			items: [
				{
					mats: [
						{ material: 'Hadanite', ore_id: 'ore-had', unit: 'gems', quantity: 250, quality: 500 },
						{ material: 'Quantum Core', ore_id: null, unit: 'items', quantity: 1, quality: 300 }
					]
				},
				{
					mats: [{ material: 'Hadanite', ore_id: 'ore-had', unit: 'gems', quantity: 300, quality: 800 }]
				}
			],
			aggregated: [
				{ material: 'Hadanite', unit: 'gems', quantity: 800, min_quality: 800 },
				{ material: 'Quantum Core', unit: 'items', quantity: 1, min_quality: 300 }
			]
		});
		assert.equal(items.length, 2);
		const had = items[0];
		assert.equal(had.item_id, 'ore-had'); // ore_id aus dem Snapshot
		assert.equal(had.item_name, 'Hadanite');
		assert.equal(had.craft_time, 0);
		assert.equal(had.copies, 800); // aggregierte Menge
		assert.deepEqual(had.mats, [
			{ material: 'Hadanite', ore_id: 'ore-had', unit: 'gems', quantity: 1, quality: 800 }
		]);
		const qc = items[1];
		assert.equal(qc.item_id, 'Quantum Core'); // kein ore_id → Materialname als item_id
		assert.equal(qc.mats[0].ore_id, null);
		assert.equal(qc.mats[0].quality, 300);
	});

	it('splits by unit — same material in two units stays two entries', () => {
		const items = bridgeItems({
			items: [{ mats: [{ material: 'Hadanite', ore_id: 'ore-had', unit: 'SCU', quantity: 1, quality: 600 }] }],
			aggregated: [
				{ material: 'Hadanite', unit: 'gems', quantity: 300, min_quality: 500 },
				{ material: 'Hadanite', unit: 'SCU', quantity: 0.3, min_quality: 600 }
			]
		});
		assert.equal(items.length, 2);
		// ore_id je (material, unit) — die gems-Zeile hat KEINEN Snapshot-Eintrag.
		assert.equal(items[0].mats[0].ore_id, null);
		assert.equal(items[0].mats[0].unit, 'gems');
		assert.equal(items[1].mats[0].ore_id, 'ore-had');
		assert.equal(items[1].mats[0].unit, 'SCU');
	});

	it('drops zero-quantity rows (engine copies > 0 validation) and empty aggregation', () => {
		const items = bridgeItems({
			items: [{ mats: [{ material: 'X', ore_id: null, unit: 'items', quantity: 0, quality: 500 }] }],
			aggregated: [{ material: 'X', unit: 'items', quantity: 0, min_quality: 500 }]
		});
		assert.deepEqual(items, []);
		assert.deepEqual(
			bridgeItems({ items: [], aggregated: [] }),
			[]
		);
	});
});
