// M9-C (t_dc01f34b) — Beste-Orte-Optimierung für Mining-Listen.
//
// Ziel (LISTEN-MODE-PLAN Welle 1 C): alle Materialien einer Mining-Liste an
// MÖGLICHST WENIGEN Orten sammeln. Greedy-Set-Cover über die Strata-
// `ore-locations`-Daten (M7-Backend): wiederholt den Ort wählen, der die
// meisten noch offenen Materialien abdeckt.
//
// Qualitäts-Bänder (SCMDB qualityBands, M8-R2-Snap-Daten) als Filter:
// „Ort liefert die geforderte Qualität?" — die Strata-API hat KEINE Spawn-
// Qualität (MINING-MODE-PLAN „Quality-800-Falle"), also wird die
// Concentration-Tier-Skala (1–6) der Ortszeile über die SCMDB-Bänder des
// Materials auf eine erreichbare Qualität abgebildet:
//
//   maxQuality(tier) = bands[max(1, round(tier / 6 * bands.length))] - 1
//
// (proportional, monotone Abbildung: Tier 1 → niedrigstes Band, Tier 6
// „Pure" → oberstes Band = 1000). Kein Band-Datenbestand für das Material
// (leere Liste, M8-R2-Vertrag „kein Raten") → der Band-Filter greift NICHT,
// nur die Tier-Schwelle. Material ohne ore_id (Nicht-Erz, `items`) ist ortlos
// → bleibt offen.

/// Aggregierte Material-Zeile (aus `list_detail`, AggMat) + ore_id.
export type PlaceMat = {
	material: string;
	unit: string;
	quantity: number;
	min_quality: number;
	oreId: string | null;
};

/// Eine Ortszeile aus `ore-locations/{ore}` (alle Gruppen vereinheitlicht).
export type PlaceLocRow = {
	locationId: string;
	location: string;
	system: string | null;
	group: string; // space | surface | cave | exploration
	tier: number;
	concentration: number;
};

/// Ein gewählter Ort mit den dort abgedeckten Materialien.
export type PlacePick = {
	locationId: string;
	location: string;
	system: string | null;
	group: string;
	tier: number;
	covered: { material: string; unit: string; quantity: number; min_quality: number; tier: number }[];
};

export type PlacesPlan = {
	places: PlacePick[];
	/// Materialien ohne passenden Ort (kein ore_id / kein qualifizierender Ort).
	uncovered: { material: string; unit: string; quantity: number; min_quality: number }[];
};

/// Maximale erreichbare Qualität eines Materials an einem Ort der
/// Concentration-Stufe `tier` (1–6) — proportional über die Band-Grenzen.
/// Ohne Bänder (leer) → null (Filter nicht anwendbar).
export function maxQualityForTier(bands: number[], tier: number): number | null {
	if (!bands.length) return null;
	const t = Math.min(6, Math.max(1, Math.round(tier)));
	const idx = Math.max(1, Math.round((t / 6) * bands.length));
	return bands[idx - 1];
}

/// Zählt ein Ort der Stufe `tier` für ein Material mit Mindest-Qualität?
/// Band-Filter nur, wenn Bänder vorhanden („kein Raten", M8-R2-Präzedenz).
export function locationQualifies(
	tier: number,
	bands: number[],
	minQuality: number,
	tierThreshold: number
): boolean {
	if (Math.round(tier) < tierThreshold) return false;
	const maxQ = maxQualityForTier(bands, tier);
	return maxQ === null || maxQ >= minQuality;
}

/**
 * Greedy-Set-Cover: möglichst wenige Orte, die alle (ortbaren) Materialien
 * abdecken. Kriterien je Schritt: (1) meisten offenen Materialien, Tie
 (2) höchste Summe der Tier-Stufen (bessere Konzentrations-Lage zuerst).
 * Vollständig erledigte Materialien (done >= quantity) werden per
 * `isDone`-Prädikat ausgeschlossen (nicht mitgraben).
 */
export function planBestPlaces(
	mats: PlaceMat[],
	rowsByOre: Record<string, PlaceLocRow[]>,
	bandsByMaterial: Record<string, number[]>,
	tierThreshold: number,
	isDone?: (m: PlaceMat) => boolean
): PlacesPlan {
	// Kandidaten: ortbare, nicht erledigte Materialien mit mindestens einem
	// qualifizierenden Ort.
	type Cand = {
		mat: PlaceMat;
		locs: Map<string, PlaceLocRow>;
	};
	const materialKey = (m: { material: string; unit: string }) => `${m.material}:${m.unit}`;
	const done = isDone ?? (() => false);
	const cands: Cand[] = [];
	const noLoc: PlaceMat[] = [];
	for (const m of mats) {
		if (done(m)) continue;
		if (!m.oreId) {
			noLoc.push(m);
			continue;
		}
		const bands = bandsByMaterial[materialKey(m)] ?? bandsByMaterial[m.material] ?? [];
		const qualified = (rowsByOre[m.oreId] ?? []).filter((r) =>
			locationQualifies(r.tier, bands, m.min_quality, tierThreshold)
		);
		if (!qualified.length) {
			noLoc.push(m);
			continue;
		}
		cands.push({ mat: m, locs: new Map(qualified.map((r) => [r.locationId, r])) });
	}

	const places: PlacePick[] = [];
	const placed = new Set<string>(); // materialKey der abgedeckten
	// Rest-Materialmenge > 0 als Abbruch-Anker:
	while (cands.some((c) => !placed.has(materialKey(c.mat)))) {
		let best: { loc: PlaceLocRow; gain: number; tierSum: number } | null = null;
		// Gain-Map je Ort einmal berechnen (nicht je Kandidat).
		const gain = new Map<string, { n: number; tierSum: number; row: PlaceLocRow }>();
		for (const c of cands) {
			if (placed.has(materialKey(c.mat))) continue;
			for (const r of c.locs.values()) {
				const g = gain.get(r.locationId);
				if (g) {
					g.n += 1;
					g.tierSum += r.tier;
				} else {
					gain.set(r.locationId, { n: 1, tierSum: r.tier, row: r });
				}
			}
		}
		for (const g of gain.values()) {
			if (
				!best ||
				g.n > best.gain ||
				(g.n === best.gain &&
					(g.tierSum > best.tierSum ||
						(g.tierSum === best.tierSum &&
							g.row.location.localeCompare(best.loc.location) < 0)))
			) {
				best = { loc: g.row, gain: g.n, tierSum: g.tierSum };
			}
		}
		if (!best) break; // kein Ort mehr → Rest bleibt offen
		const pick: PlacePick = {
			locationId: best.loc.locationId,
			location: best.loc.location,
			system: best.loc.system,
			group: best.loc.group,
			tier: best.loc.tier,
			covered: []
		};
		for (const c of cands) {
			const key = materialKey(c.mat);
			if (placed.has(key)) continue;
			const r = c.locs.get(best.loc.locationId);
			if (r) {
				placed.add(key);
				pick.covered.push({
					material: c.mat.material,
					unit: c.mat.unit,
					quantity: c.mat.quantity,
					min_quality: c.mat.min_quality,
					tier: r.tier
				});
			}
		}
		places.push(pick);
	}

	const uncovered = noLoc
		.filter((m) => !placed.has(materialKey(m)))
		.map((m) => ({
			material: m.material,
			unit: m.unit,
			quantity: m.quantity,
			min_quality: m.min_quality
		}))
		.sort((a, b) => a.material.toLowerCase().localeCompare(b.material.toLowerCase()));
	// Orte nach Größe absteigend (größte Abdeckung zuerst), Tie: Name.
	places.sort(
		(a, b) =>
			b.covered.length - a.covered.length ||
			a.location.localeCompare(b.location)
	);
	return { places, uncovered };
}
