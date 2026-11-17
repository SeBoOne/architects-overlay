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
