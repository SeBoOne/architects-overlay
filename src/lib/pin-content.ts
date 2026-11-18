// M6-F1 (t_58d46923): Kontext-adaptive Pin-Inhalte — generische Renderer-
// Registry. Die Pin-Ansicht (PinBoxes.svelte) rendert standardmäßig den
// DISPLAY-Snapshot (title/kpi/rows); hat ein Pin einen Content-Deskriptor
// `{ type, ...payload }`, wählt diese Registry den registrierten Renderer
// (Svelte-Komponente) dafür. Kein Plugin-Vokabular hier: Content-Typen
// registrieren sich SEITENSTARK (Plugins/Views), die Verwaltung und das
// Backend kennen nur das generische JSON-Format (Schema-U).
//
// Der Renderer erhält `pin` (volles PinEntry) + `interactive` (Overlay-
// aktiv-Gate, M4-E/M5-Vertrag: Interaktion nur bei interaktivem Spotlight)
// und rendert IM Box-Körper der Pin-Box (das Spotlight-Webview ist der
// Interaktionskanal — das frühere M4-Patch-Fenster existiert seit M5 nicht
// mehr, der Vertrag „Anzeige + unsichtbare Interaktionsebene darüber" ist
// strukturell erfüllt: die Box selbst liegt im interaktiven Spotlight).
import type { Component } from 'svelte';
import type { PinContent } from './pins';

type PinContentProps = {
	pin: import('./pins').PinEntry;
	interactive: boolean;
};

const registry = new Map<string, Component<PinContentProps>>();

/// Registriert einen Content-Renderer für einen Deskriptor-Typ. Später
/// gesetzte Registrierungen (kein Doppeltyp) überschreiben deterministisch —
/// letzte Registrierung gewinnt (Plugin-Load-Reihenfolge).
export function registerPinContent(type: string, renderer: Component<PinContentProps>): void {
	registry.set(type, renderer);
}

/// Renderer für einen Content-Typ oder null → rows-Snapshot-Fallback.
export function pinContentFor(type: string | null | undefined): Component<PinContentProps> | null {
	if (!type) return null;
	return registry.get(type) ?? null;
}

/// Für Tests/Reset (HMR, isolierte Testläufe).
export function clearPinContentRegistry(): void {
	registry.clear();
}

/// M7-F4 (t_c3367dfb) Punkt 1 (Dedupe): Content-Renderer, die ihren Wert
/// selbst UND NUR EINMAL im Körper zeigen — der Header-KPI der Pin-Box
/// würde denselben Wert duplizieren („2× die Signatur für EIN Vorkommen").
/// Die Pin-Ansicht blendet den KPI für diese Typen aus; DealChat u. a.
/// bleiben unverändert (deren KPI ist Information, kein Duplikat).
const HIDE_HEADER_KPI = new Set(['scan-signature', 'scan-signatures']);

export function hideHeaderKpi(type: string | null | undefined): boolean {
	return type != null && HIDE_HEADER_KPI.has(type);
}
