// PB (t_1c9bc240, B1): Mode-Hotkeys — única fonte de verdade para Defaults,
// Validação e Lookup. Backend (Settings-Crate) persiste o Map validado; o
// Frontend cai nos Defaults quando o snapshot ainda não tem o campo
// (SSR/Probe/Config antiga = Migration automática para a nova Belegung).

export type Mode =
	| 'search'
	| 'market'
	| 'routes'
	| 'inventory'
	| 'hangar'
	| 'settings'
	| 'mining'
	| 'crafting';

export type ModeHotkeys = Record<Mode, string>;

// M7-K2 (t_bb3303d0) / M7-F1: F5 gehört ALLEIN dem Mining-Modus — der
// kontextsensitive Resync (B2) läuft auf STRG+F5 (Kontexttrennung im
// F5-Pfad von +page.svelte).

/// Nova Belegung (B1): Hangar → F11 (raramente usado), Handelsrouten → F3
/// (pertinho do F2/Marktplatz), Inventar → F4. F2 Marktplatz e F12 Settings
/// permanecem. F1 Suche permanece.
export const DEFAULT_MODE_HOTKEYS: ModeHotkeys = {
	search: 'F1',
	market: 'F2',
	routes: 'F3',
	inventory: 'F4',
	hangar: 'F11',
	settings: 'F12',
	mining: 'F5',
	// M8-K2 (t_f95f9d97): Crafting-Modus — F6 ist die erste freie F-Taste
	// nach F1–F5 (F1–F4 Modi, F5 Mining).
	crafting: 'F6'
};

const FKEY_RE = /^F([1-9]|1[0-2])$/;

/** Canonicaliza um F-Key (" f3 " → "F3"). Retorna null quando inválido. */
export function normalizeFkey(raw: string): string | null {
	const key = raw.trim().toUpperCase();
	return FKEY_RE.test(key) ? key : null;
}

/** Valididade de um Map inteiro: F-Keys válidos, sem duplicatas (M7-F1: F5 é o Default de Mining — KEINE Reservierung in der Validierung). */
export function validateModeHotkeys(hk: ModeHotkeys): string | null {
	const seen: string[] = [];
	for (const mode of Object.keys(hk) as Mode[]) {
		const key = normalizeFkey(hk[mode] ?? '');
		if (!key) return `${mode}: apenas F1–F12`;
		if (seen.includes(key)) return `${mode}: ${key} já está em uso`;
		seen.push(key);
	}
	return null;
}

/** Hotkey efetivo de um modo: snapshot do backend ou Defaults. */
export function hotkeyFor(cfg: { mode_hotkeys?: ModeHotkeys | null } | null, mode: Mode): string {
	const hk = cfg?.mode_hotkeys;
	return (hk && normalizeFkey(hk[mode] ?? '')) || DEFAULT_MODE_HOTKEYS[mode];
}

/** Map reverso key → mode a partir do snapshot (ou Defaults). */
export function modeKeyMap(cfg: { mode_hotkeys?: ModeHotkeys | null } | null): Map<string, Mode> {
	const map = new Map<string, Mode>();
	for (const mode of Object.keys(DEFAULT_MODE_HOTKEYS) as Mode[]) {
		map.set(hotkeyFor(cfg, mode), mode);
	}
	return map;
}
