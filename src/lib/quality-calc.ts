// M8-K6 (t_f7a29367): Echter Qualitäts-Stat-Rechner.
//
// Rechenmodell — durch Live-Beleg gegen die SC-Wiki-API verifiziert
// (Blueprint "BP_CRAFT_AMRS_LaserCannon_S1", Omnisky III Cannon,
// game_version 4.10.3-LIVE.13165946, abgerufen 2026-11-19):
//
//   aspects[].modifiers[] = {
//     property_key, label, better_when,
//     quality_range:  { min: 0, max: 1000 },
//     modifier_range: { at_min_quality: 0.9, at_max_quality: 1.1 },
//     value_range_type: "linear" }
//
// Der Modifier ist ein FAKTOR auf die Eigenschaft (SC-Wiki-Interpretation):
// bei Qualität 0 wirkt 0.9×, bei Qualität 1000 wirkt 1.1×. Wir zeigen den
// Effekt ehrlich als %-Abweichung gegenüber 1.0 („Integrity +10 % bei
// Q1000“) statt absoluter Stats — die Basis-Stat-Werte pro Item liegen
// hier nicht vor und werden nicht erfunden.
//
// Aggregation über Aspekte: mehrere Aspekte können dieselbe Eigenschaft
// modifizieren — interpretiert als multiplikative Kette (Faktor-Produkt).
// Diese Interpretation ist dokumentiert, nicht als Spielwahrheit belegt.

export type QualityRange = { min: number; max: number };
export type ModifierRange = { at_min_quality: number; at_max_quality: number };

export type AspectModifier = {
	property_key: string;
	label: string;
	better_when?: string | null; // "higher" | "lower" | …
	quality_range?: QualityRange | null;
	modifier_range?: ModifierRange | null;
	value_range_type?: string | null;
};

export type AspectInput = {
	name: string;
	min_quality?: number | null;
	quantity?: number | null;
	quantity_scu?: number | null;
};

export type Aspect = {
	key: string;
	name: string;
	input?: AspectInput | null;
	modifiers?: AspectModifier[] | null;
	required_count?: number | null;
};

export type SummaryProperty = {
	property_key: string;
	label: string;
	better_when?: string | null;
};

/** Linearer Modifier-Faktor an einer Ziel-Qualität, oder null bei fehlenden/
 *  nicht-linearen Range-Daten (ehrlich degradieren, nichts erfinden). */
export function interpolateModifier(
	mod: AspectModifier,
	quality: number
): number | null {
	if (!mod || mod.value_range_type !== 'linear') return null;
	const qr = mod.quality_range;
	const mr = mod.modifier_range;
	if (!qr || !mr) return null;
	const { min: qMin, max: qMax } = qr;
	if (
		!Number.isFinite(qMin) ||
		!Number.isFinite(qMax) ||
		!Number.isFinite(mr.at_min_quality) ||
		!Number.isFinite(mr.at_max_quality)
	) {
		return null;
	}
	if (qMin === qMax) {
		// Degenerate Range: ein einziger Punkt — at_min ist der einzige
		// definierte Wert.
		return mr.at_min_quality;
	}
	// Ziel-Qualität auf die Range klemmen (auch bei min>max konsistent:
	// t läuft von 0 an qMin bis 1 an qMax, egal welche Richtung).
	const t = (clamp(quality, Math.min(qMin, qMax), Math.max(qMin, qMax)) - qMin) / (qMax - qMin);
	return mr.at_min_quality + t * (mr.at_max_quality - mr.at_min_quality);
}

function clamp(v: number, lo: number, hi: number): number {
	return Math.min(hi, Math.max(lo, v));
}

/** Faktor → %-Abweichung gegenüber 1.0, auf 1 Nachkommastelle. */
export function modifierPercent(factor: number | null): number | null {
	if (factor === null || !Number.isFinite(factor)) return null;
	return Math.round((factor - 1) * 1000) / 10;
}

export type StatChange = {
	property_key: string;
	label: string;
	/** Aggregierter Faktor über alle Aspekte (Produkt) — null wenn keine
	 *  Daten. */
	factor: number | null;
	/** %-Abweichung (factor−1)·100 — null wenn keine Daten. */
	pct: number | null;
	/** true = Verbesserung, false = Verschlechterung, null = neutral/unbekannt
	 *  (better_when unbekannt oder pct === 0). */
	better: boolean | null;
};

/** Änderungen je Eigenschaft bei Ziel-Qualität, aggregiert über alle
 *  Aspekte (multiplikativ — siehe Header-Kommentar). Reihenfolge folgt
 *  summary_properties (unbekannte Keys hinten, in Erstauftretens-Reihenfolge). */
export function statChanges(
	aspects: Aspect[],
	summary: SummaryProperty[] | null | undefined,
	quality: number
): StatChange[] {
	const factors = new Map<string, number>();
	let any = false;
	for (const a of aspects ?? []) {
		for (const m of a.modifiers ?? []) {
			const f = interpolateModifier(m, quality);
			if (f === null) continue;
			any = true;
			factors.set(m.property_key, (factors.get(m.property_key) ?? 1) * f);
		}
	}
	if (!any) return [];

	const order: string[] = [];
	for (const s of summary ?? []) {
		if (factors.has(s.property_key) && !order.includes(s.property_key)) {
			order.push(s.property_key);
		}
	}
	for (const k of factors.keys()) {
		if (!order.includes(k)) order.push(k);
	}

	const summaryByKey = new Map<string, SummaryProperty>();
	for (const s of summary ?? []) summaryByKey.set(s.property_key, s);

	return order.map((key) => {
		const factor = factors.get(key) ?? null;
		const pct = modifierPercent(factor);
		const betterWhen = summaryByKey.get(key)?.better_when ?? null;
		let better: boolean | null = null;
		if (pct !== null && pct !== 0) {
			if (betterWhen === 'higher') better = pct > 0;
			else if (betterWhen === 'lower') better = pct < 0;
		}
		const label = summaryByKey.get(key)?.label ?? key;
		return { property_key: key, label, factor, pct, better };
	});
}

/** Ist die Ziel-Qualität für ein Material erreichbar (min_quality)? */
export function qualityReachable(target: number, minQuality: number | null | undefined): boolean {
	if (minQuality === null || minQuality === undefined) return true; // keine Grenze
	return target >= minQuality;
}
