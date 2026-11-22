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
export type PlaceCovered = {
	material: string;
	unit: string;
	quantity: number;
	min_quality: number;
	tier: number;
	/// M9-O (t_a39041f8): max erreichbare Qualität laut Bändern — nur gesetzt,
	/// wenn der Ort als Qualitäts-Fallback gewählt wurde (KEIN Ort erfüllt
	/// min_quality). Die View zeigt dafür den Warn-Hinweis place_below_quality.
	maxQuality?: number;
};

export type PlacePick = {
	locationId: string;
	location: string;
	system: string | null;
	group: string;
	tier: number;
	covered: PlaceCovered[];
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

/// M9-O (t_a39041f8, Ursache A): Name für ore_id-Namens-Match normalisieren —
/// lowercase, Suffix „ (Raw)"/„ (Ore)" weg, Whitespace getrimmt. „Glacosite"
/// matcht so den Strata-Katalognamen „Glacosite (Raw)". NUR für EXAKTEN
/// Gleichstand nach Normalisierung (kein fuzzy/includes — sonst falsche Erze).
export function normalizeOreName(s: string): string {
	return s.toLowerCase().replace(/\s*\((raw|ore)\)\s*/g, '').trim();
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
 *
 * M9-K (t_db9b234f, P10): `mode` wählt den Optimierungs-Modus pro Liste:
 * - 'fewest' (Default): Set-Cover-Greedy wie oben.
 * - 'concentration': je Material EIN Ort — der mit der höchsten
 *   erreichbaren Konzentrations-Stufe (tier) unter den qualifizierenden
 *   Orten (Tie: concentration, dann Name). Gleiche Orts-Treffer mehrerer
 *   Materialien werden zu einem Ort zusammengefasst. Qualitäts-Bänder
 *   filtern in BEIDEN Modi (immer Orte, die die Qualitätsansprüche erfüllen).
 */
export type PlaceMode = 'fewest' | 'concentration';

/// M9-L (t_4de3c84f, P6): Von der Planung/Orts-Ansicht ausgeschlossene
/// Orts-Slugs. `battagliaintro` (locationId `dungeon_v3slotpreset_harvestable_-
/// battagliaintro`, Typ `dungeon`) ist an eine EINMALIGE Mission gekoppelt und
/// taucht daher als regulärer Mining-Ort nicht auf. Der Dungeon-Typ bleibt
/// unangetastet (26 weitere Dungeon-Orte sind reguläre Caves/Rockcracker) —
/// es wird NUR dieser Slug gefiltert.
export const EXCLUDED_LOCATION_SLUGS: readonly string[] = ['battagliaintro'];

/// Erkennt einen ausgeschlossenen Ort: entweder direkt per Slug (Ort-Liste,
/// `strata_locations`) oder über das locationId-Suffix (`strata_ore_locations`-
/// Zeilen tragen keinen Slug — `…_harvestable_battagliaintro` endet auf dem
/// Slug mit `_`-Trenner).
export function isExcludedLocation(locationId: string, slug?: string | null): boolean {
	if (slug !== undefined && slug !== null && EXCLUDED_LOCATION_SLUGS.includes(slug)) return true;
	return EXCLUDED_LOCATION_SLUGS.some((s) => locationId === s || locationId.endsWith(`_${s}`));
}

export function planBestPlaces(
	mats: PlaceMat[],
	rowsByOre: Record<string, PlaceLocRow[]>,
	bandsByMaterial: Record<string, number[]>,
	tierThreshold: number,
	isDone?: (m: PlaceMat) => boolean,
	mode: PlaceMode = 'fewest'
): PlacesPlan {
	// M9-L (P6): ausgeschlossene Orte (Battagliaintro) schon aus den
	// Kandidaten-Zeilen entfernen — greift in BEIDEN Modi (fewest +
	// concentration), weil beide aus `locRows` qualifizieren.
	const locRows: Record<string, PlaceLocRow[]> = {};
	for (const [oreId, rows] of Object.entries(rowsByOre)) {
		locRows[oreId] = rows.filter((r) => !isExcludedLocation(r.locationId));
	}
	// Kandidaten: ortbare, nicht erledigte Materialien mit mindestens einem
	// qualifizierenden Ort.
	type Cand = {
		mat: PlaceMat;
		locs: Map<string, PlaceLocRow>;
		/// M9-O: gesetzt → Material ging als Qualitäts-Fallback in den Plan
		/// (kein Ort erfüllt min_quality); Wert = max erreichbare Qualität
		/// am Fallback-Ort.
		belowQuality?: number;
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
		const qualified = (locRows[m.oreId] ?? []).filter((r) =>
		const rows = rowsByOre[m.oreId] ?? [];
		const qualified = rows.filter((r) =>			locationQualifies(r.tier, bands, m.min_quality, tierThreshold)
		);
		if (!qualified.length) {
			// M9-O (t_a39041f8, Feedback P5): qualifiziert KEIN Ort die
			// min_quality (Band-Filter oder Tier-Schwelle schneidet alle ab),
			// aber der Erz hat Orte → bester verfügbarer Ort statt uncovered:
			// max erreichbare Qualität (Bänder) DESC, dann Tier DESC, dann Name.
			// Der covered-Eintrag trägt maxQuality → View warnt deutlich.
			if (!rows.length) {
				noLoc.push(m);
				continue;
			}
			const best = [...rows].sort((a, b) => {
				const qa = maxQualityForTier(bands, a.tier) ?? -1;
				const qb = maxQualityForTier(bands, b.tier) ?? -1;
				return qb - qa || b.tier - a.tier || a.location.localeCompare(b.location);
			})[0];
			cands.push({
				mat: m,
				locs: new Map([[best.locationId, best]]),
				belowQuality: maxQualityForTier(bands, best.tier) ?? undefined
			});
			continue;
		}
		cands.push({ mat: m, locs: new Map(qualified.map((r) => [r.locationId, r])) });
	}

	const places: PlacePick[] = [];
	const placed = new Set<string>(); // materialKey der abgedeckten
	// M9-K P10: Konzentrations-Modus — je Material der qualifizierende Ort mit
	// der höchsten Tier-Stufe (Tie: concentration, dann Name); gleiche Orte
	// mehrerer Materialien werden zu einem Pick zusammengefasst.
	if (mode === 'concentration') {
		const byLoc = new Map<string, PlacePick>();
		for (const c of cands) {
			let best: PlaceLocRow | null = null;
			for (const r of c.locs.values()) {
				if (
					!best ||
					r.tier > best.tier ||
					(r.tier === best.tier &&
						(r.concentration > best.concentration ||
							(r.concentration === best.concentration &&
								r.location.localeCompare(best.location) < 0)))
				) {
					best = r;
				}
			}
			if (!best) continue; // kann nicht passieren (qualified.length > 0)
			placed.add(materialKey(c.mat));
			let pick = byLoc.get(best.locationId);
			if (!pick) {
				pick = {
					locationId: best.locationId,
					location: best.location,
					system: best.system,
					group: best.group,
					tier: best.tier,
					covered: []
				};
				byLoc.set(best.locationId, pick);
			}
			pick.covered.push({
				material: c.mat.material,
				unit: c.mat.unit,
				quantity: c.mat.quantity,
				min_quality: c.mat.min_quality,
				tier: best.tier,
				maxQuality: c.belowQuality
			});
		}
		places.push(...[...byLoc.values()]);
	} else {
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
					tier: r.tier,
					maxQuality: c.belowQuality
				});
			}
		}
		places.push(pick);
	}
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
