// M9-C (t_dc01f34b) — Beste-Orte-Optimierung für Mining-Listen.
//
// Ziel (LISTEN-MODE-PLAN Welle 1 C): alle Materialien einer Mining-Liste an
// MÖGLICHST WENIGEN Orten sammeln. Greedy-Set-Cover über die Strata-
// `ore-locations`-Daten (M7-Backend): wiederholt den Ort wählen, der die
// meisten noch offenen Materialien abdeckt.
//
// Qualitätsregel (M9-O R2, t_a39041f8 — Sebos Domänen-Korrektur): die Strata-
// Tier-Stufen (1–6) sind KONZENTRATION am Ort, NICHT Qualität. Erreichbare
// Qualität richtet sich nach dem PRIMARY-Status des Materials am Ort:
//
//   PRIMARY  (rank === 1 bzw. rankPct === 100 je Ort-Zeile, Live-Beleg
//            fixtures-live/ore-locations.gold_ore.live.json) → 501–1000
//            erreichbar → erfüllt jede min_quality ≤ 1000.
//   SEKUNDÄR → bis zur 4. Band-Grenze des Materials (SCMDB qualityBands,
//            bands[3]) — die unteren vier Band-Grenzen, kein 1000er.
//
// Kein Band-Datenbestand für das Material (leere Liste, M8-R2-Vertrag „kein
// Raten") → der Sekundär-Filter greift NICHT, nur die Konzentrations-Schwelle
// (tierThreshold). Material ohne ore_id (Nicht-Erz, `items`) ist ortlos →
// bleibt offen.

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
	/// M9-O R2: Wettbewerbs-Rang in der Gruppe (1 = best) + Anteil der Proben
	/// (0–100) — PRIMARY-Bestimmung (Sebos Regel).
	rank: number;
	rankPct: number;
};

/// Ein gewählter Ort mit den dort abgedeckten Materialien.
export type PlaceCovered = {
	material: string;
	unit: string;
	quantity: number;
	min_quality: number;
	tier: number;
	/// M9-O (t_a39041f8): max erreichbare Qualität — nur gesetzt, wenn der Ort
	/// als Qualitäts-Fallback gewählt wurde (KEIN Ort erfüllt min_quality).
	/// Die View zeigt dafür den Warn-Hinweis place_below_quality.
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

/// M9-O R2: Ist das Material an diesem Ort PRIMARY (Hauptmaterial)?
/// Live-Beleg (gold_ore.live.json): rank 1 ⟺ rankPct 100 je Gruppenzeile.
export function isPrimary(row: Pick<PlaceLocRow, 'rank' | 'rankPct'>): boolean {
	return row.rank === 1 || row.rankPct === 100;
}

/// M9-O R2: Max erreichbare Qualität am Ort — PRIMARY → 1000 (501–1000
/// machbar), SEKUNDÄR → 4. Band-Grenze (bands[3]). Ohne Bänder → null
/// (Filter nicht anwendbar, kein Raten).
export function maxQualityForLoc(
	row: Pick<PlaceLocRow, 'rank' | 'rankPct'>,
	bands: number[]
): number | null {
	if (isPrimary(row)) return 1000;
	return bands.length >= 4 ? bands[3] : null;
}

/// M9-O (t_a39041f8, Ursache A): Name für ore_id-Namens-Match normalisieren —
/// lowercase, Suffix „ (Raw)"/„ (Ore)" weg, Whitespace getrimmt. „Glacosite"
/// matcht so den Strata-Katalognamen „Glacosite (Raw)". NUR für EXAKTEN
/// Gleichstand nach Normalisierung (kein fuzzy/includes — sonst falsche Erze).
export function normalizeOreName(s: string): string {
	return s.toLowerCase().replace(/\s*\((raw|ore)\)\s*/g, '').trim();
}

/// Zählt ein Ort für ein Material mit Mindest-Qualität? (M9-O R2, Sebos
/// Regel): PRIMARY erfüllt jede min_quality (501–1000), SEKUNDÄR nur bis
/// bands[3]; ohne Bänder gilt der Sekundär-Filter nicht (kein Raten).
/// Die Konzentrations-Schwelle (tier) greift weiterhin vor dem Qualitätsfilter.
export function locationQualifies(
	row: Pick<PlaceLocRow, 'tier' | 'rank' | 'rankPct'>,
	bands: number[],
	minQuality: number,
	tierThreshold: number
): boolean {
	if (Math.round(row.tier) < tierThreshold) return false;
	if (isPrimary(row)) return true;
	if (!bands.length) return true;
	const cap = bands.length >= 4 ? bands[3] : null;
	return cap === null || minQuality <= cap;
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
 *   Materialien werden zu einem Ort zusammengefasst. Der Qualitätsfilter
 *   (PRIMARY/SEKUNDÄR + Bänder) greift in BEIDEN Modi.
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
		const rows = locRows[m.oreId] ?? [];
		const qualified = rows.filter((r) =>
			locationQualifies(r, bands, m.min_quality, tierThreshold)
		);
		if (!qualified.length) {
			// M9-O (t_a39041f8, Feedback P5): qualifiziert KEIN Ort die
			// min_quality (Primär-/Sekundär-Filter oder Tier-Schwelle schneidet
			// alle ab), aber der Erz hat Orte → bester verfügbarer Ort statt
			// uncovered: max erreichbare Qualität (Sebos Regel) DESC, dann Tier
			// DESC, dann Name. Der covered-Eintrag trägt maxQuality → View
			// warnt deutlich.
			if (!rows.length) {
				noLoc.push(m);
				continue;
			}
			const best = [...rows].sort((a, b) => {
				const qa = maxQualityForLoc(a, bands) ?? -1;
				const qb = maxQualityForLoc(b, bands) ?? -1;
				return qb - qa || b.tier - a.tier || a.location.localeCompare(b.location);
			})[0];
			cands.push({
				mat: m,
				locs: new Map([[best.locationId, best]]),
				belowQuality: maxQualityForLoc(best, bands) ?? undefined
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
