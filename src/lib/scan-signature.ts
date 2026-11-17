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
};
export type ScanSignaturesPayload = {
	entries?: ScanSignatureEntry[];
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
	const rows: { n: number; display: string }[] = [];
	for (let i = 1; i <= n; i++) {
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
