// M7-K3 (t_cfea5bea): generischer Pin-Content-Typ „scan-signature".
// Payload-Vertrag (Karten-Spec, M6-F1-Disziplin — KEIN Strata-Vokabular):
//   { type: 'scan-signature', label: string, value: number | null,
//     tier?: number | null, max?: number | null }
// - value ist die Scan-Signatur-Zahl; null/fehlend = fehlende Daten → „—"
//   (Pin bleibt trotzdem — Orientierung am Label/Title).
// - max ist das Chart-Maximum der Quelle (Orientierungsbalken relativ dazu);
//   ohne max (oder max <= 0) rendert der Renderer keinen Balken.
// - tier ist die Tier-Stufe der gemappten Quelle (1..=6) — Badge nur bei
//   vorhandenem Wert.
// Die reine Logik steht hier (unit-getestbar mit node --test), die Komponente
// (PinScanSignature.svelte) rendert nur.

export type ScanSignaturePayload = {
	label?: string;
	value?: number | null;
	tier?: number | null;
	max?: number | null;
};

/// Anzeige-Wert: fehlende Daten → „—" (Karten-Spec), sonst die Zahl.
export function sigValueDisplay(value: number | null | undefined): string {
	if (value === null || value === undefined || !Number.isFinite(value)) return '—';
	return String(value);
}

/// Balkenbreite in Prozent (0..100) relativ zum Chart-Max — oder null, wenn
/// kein sinnvoller Balken möglich ist (fehlender Wert, fehlendes/ungültiges
/// Max, Max <= 0). Negative Werte clampt auf 0 (Orientierung, keine Achse).
export function sigBarPct(
	value: number | null | undefined,
	max: number | null | undefined
): number | null {
	if (
		value === null ||
		value === undefined ||
		!Number.isFinite(value) ||
		max === null ||
		max === undefined ||
		!Number.isFinite(max) ||
		max <= 0
	) {
		return null;
	}
	return Math.max(0, Math.min(100, (value / max) * 100));
}

/// Tier-Badge-Text: nur bei ganzzahliger Tier-Stufe (1..=6-Fenster der
/// Quelle; der Renderer clampt die Anzeige nicht — er zeigt was da ist).
export function sigTierBadge(tier: number | null | undefined): string | null {
	if (tier === null || tier === undefined || !Number.isFinite(tier)) return null;
	return `T${Math.round(tier)}`;
}

// ---- M7-F3 (t_cf1224c5, Punkt 2): Sammel-Signatur-Pin ----------------------
// EIN Pin-Content „scan-signatures“ (Plural, Liste) sammelt ALLE angepinnten
// Signaturen (dedupe nach oreId, Pin-Button am Erz togglet). Payload bewusst
// generisch (Universality-Regel — KEIN Strata-Vokabular):
//   { type: 'scan-signatures',
//     entries: [{ oreId, label, value }],
//     visibleSlots?: number }
// `visibleSlots` ist NACHRÜSTBAR (Pin-Resize-Welle, bis zu ×10) — der Renderer
// liest es mit Default 5; alte Snapshots ohne das Feld bleiben verträglich.
export type ScanSignatureEntry = {
	oreId: string;
	label: string;
	value: number | null;
	/** M7-F4 (t_c3367dfb): Rarity-String der Quelle (ores[].tier) — Chip-Farbe. */
	rarity?: string | null;
};
export type ScanSignaturesPayload = {
	entries?: ScanSignatureEntry[] | null;
	/** Nachrüstbar (Pin-Resize-Welle): angezeigte Multiplikator-Slots, Default 5. */
	visibleSlots?: number;
};

const DEFAULT_SLOTS = 5;

/// Multiplikator-Zeilen ×1..×N (Basis·n) — Strata-Signatur-Chart-Konvention
/// (2 Vorkommen = 2× Basis-Signatur). fehlender Wert → „—“ je Slot.
export function sigMultiples(
	value: number | null | undefined,
	slots: number | null | undefined = DEFAULT_SLOTS
): { n: number; display: string }[] {
	const n = Math.max(1, Math.min(10, Math.round(slots ?? DEFAULT_SLOTS) || DEFAULT_SLOTS));
	const base = value === null || value === undefined || !Number.isFinite(value) ? null : value;
	// M7-F5d (User 2026-11-19): die Basis-Signatur IST der ×1-Wert und steht
	// im Chip — die Multiplikator-Zellen beginnen deshalb beim DOPPELTEN
	// (i = 2..n; kein ×1-Doppelrender mehr).
	const rows: { n: number; display: string }[] = [];
	for (let i = 2; i <= n; i++) {
		rows.push({
			n: i,
			display: base === null ? '—' : String(Math.round(base * i))
		});
	}
	return rows;
}

/// Sammel-Liste: bestehende Einträge + ein neuer (dedupe nach oreId — der
/// Pin-Button am Erz togglet, siehe Mining.svelte toggleSignaturePin).
export function sigToggleEntry(
	entries: ScanSignatureEntry[] | undefined,
	entry: ScanSignatureEntry
): ScanSignatureEntry[] {
	const list = entries ?? [];
	return list.some((e) => e.oreId === entry.oreId)
		? list.filter((e) => e.oreId !== entry.oreId)
		: [...list, entry];
}

// ---- M7-F4 (t_c3367dfb): Normalisierung + Rows-Snapshot --------------------
/// Normalisiert die Payload-Entries: nicht-objektige Einträge raus,
/// Dedupe nach oreId — jeder Wert bleibt EINMAL im Pin (Spec P1).
export function sigEntries(
	payload: Partial<ScanSignaturesPayload> | null | undefined
): ScanSignatureEntry[] {
	const raw = Array.isArray(payload?.entries) ? payload!.entries! : [];
	const seen = new Set<string>();
	const out: ScanSignatureEntry[] = [];
	for (const e of raw as unknown[]) {
		if (!e || typeof e !== 'object') continue;
		const rec = e as Record<string, unknown>;
		const oreId = typeof rec.oreId === 'string' ? rec.oreId : null;
		if (!oreId || seen.has(oreId)) continue;
		seen.add(oreId);
		out.push({
			oreId,
			label: typeof rec.label === 'string' ? rec.label : '—',
			value: typeof rec.value === 'number' && Number.isFinite(rec.value) ? rec.value : null,
			rarity: typeof rec.rarity === 'string' ? rec.rarity : null
		});
	}
	return out;
}

/// Rows-Snapshot für pin_add/pin_update (Geometrie pinBoxEstHeight: eine
/// Zeile pro Entry) — Anzeige-Wert über sigValueDisplay („—“ bei null).
export function sigRowsFor(entries: ScanSignatureEntry[]): { idx: string; name: string; value: string }[] {
	return entries.map((e, i) => ({
		idx: String(i),
		name: e.label,
		value: sigValueDisplay(e.value)
	}));
}
