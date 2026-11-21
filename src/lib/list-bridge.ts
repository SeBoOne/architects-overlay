// list-bridge (M9-D, t_36f3966c) — Craft→Mining-Brücke: Crafting-Liste →
// Mining-Liste (Material + Qualitäten übernommen).
//
// Die Listen-Engine ist generisch (kind 'craft' | 'mining', M9-B) — die
// Brücke braucht also KEINE Backend-Änderung: aus der Aggregation der
// Crafting-Liste werden Mining-Listen-Einträge gebaut (Einträge sind dort
// DIREKT Materialien: copies = Menge, eine mat-Zeile mit quantity 1 und der
// MAX-Qualität der Anforderung). ore_id kommt aus den Eintrags-Snapshots
// (material:unit-Key, identisch zur oreIdOf-Logik der besten Orte in
// MiningLists) — damit funktioniert die „Beste Orte“-Optimierung der
// entstehenden Mining-Liste sofort.
//
// Pure Funktion (kein invoke) — testbar in list-bridge.test.ts.

export type BridgeDetail = {
	items: {
		mats: {
			material: string;
			ore_id: string | null;
			unit: string;
			quantity: number;
			quality: number;
		}[];
	}[];
	aggregated: {
		material: string;
		unit: string;
		quantity: number;
		min_quality: number;
	}[];
};

/// Mining-Listen-Eintrag (NewListItem-Shape für `list_add_item`).
export type BridgeItem = {
	item_id: string;
	item_name: string;
	craft_time: 0;
	copies: number;
	mats: [
		{
			material: string;
			ore_id: string | null;
			unit: string;
			quantity: 1;
			quality: number;
		}
	];
};

/// Aggregation der Crafting-Liste → Mining-Listen-Einträge.
/// - Menge = aggregierte Menge (copies), Qualität = MAX-Anforderung.
/// - ore_id aus dem Eintrags-Snapshot (material:unit), sonst null.
/// - Zeilen mit Menge ≤ 0 fliegen raus (Engine-Validierung: copies > 0).
export function bridgeItems(detail: BridgeDetail): BridgeItem[] {
	const oreOf = new Map<string, string | null>();
	for (const it of detail.items) {
		for (const mm of it.mats) {
			oreOf.set(`${mm.material}:${mm.unit}`, mm.ore_id);
		}
	}
	return detail.aggregated
		.filter((m) => m.quantity > 0)
		.map((m) => {
			const ore_id = oreOf.get(`${m.material}:${m.unit}`) ?? null;
			return {
				item_id: ore_id ?? m.material,
				item_name: m.material,
				craft_time: 0 as const,
				copies: m.quantity,
				mats: [
					{
						material: m.material,
						ore_id,
						unit: m.unit,
						quantity: 1 as const,
						quality: m.min_quality
					}
				]
			};
		});
}
