// M4 (t_7bf507e6): Dauerhafte Shellboxen „Pins" — Frontend-Typen + Helpers.
//
// Spiegel der Serde-DTOs aus src-tauri/src/pins.rs (PinDto/PinRowDto).
// Die Pin-Verwaltung ist rein mechanisch ({view_id, screen, box_index});
// der Anzeige-Snapshot (title/kpi/rows) wird von der View-Komponente des
// Overlay-Frontends erzeugt — generische DISPLAY-Daten ohne Plugin-Semantik
// (Schema-U-Disziplin, Sonde §6).

export type PinRow = {
	idx: string;
	name: string;
	sub: string | null;
	value: string;
	side: string | null;
};

export type PinEntry = {
	view_id: string;
	screen: string;
	box_index: number;
	opacity: number;
	pinned: boolean;
	x: number;
	y: number;
	title: string;
	kpi: string | null;
	rows: PinRow[];
};

export type PinsState = {
	enabled: boolean;
	game_running: boolean;
	overlay_visible: boolean;
	pins: PinEntry[];
};

/// Label-Präfix der Pin-Fenster (MUST match pins.rs PINS_LABEL_PREFIX).
export const PINS_LABEL_PREFIX = 'pins-';

/// Backend-Event: Pin-Set geändert (pins.rs PINS_CHANGED_EVENT).
export const PINS_CHANGED_EVENT = 'ao:pins-changed';

/// Pin-Fenster-Erkennung am Label (`pins-0`, `pins-1`, …).
export function isPinsWindow(label: string): boolean {
	return label.startsWith(PINS_LABEL_PREFIX) && /^pins-\d+$/.test(label);
}

/// Box-Kopfhöhe + Zeilenhöhe in px — MIRROR der Platzierungs-Schätzung in
/// pins.rs pin_add (44 + 34/Zeile). Nur für die Default-Stapelhöhe; die
/// gerenderte Box ist content-driven (Mockup-Vertrag).
export const PIN_BOX_W = 356;

export function pinBoxEstHeight(rows: number): number {
	return 44 + 34 * Math.max(rows, 1);
}
