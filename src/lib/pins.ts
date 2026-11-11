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
	/// M4-E (t_34efb781): Label des Spotlight-Screens (pins.rs
	/// spotlight_screen_of) — Patch-Flächen nur für Pins dieses Screens.
	spotlight_screen?: string;
};

/// Backend-Event: Pin-Set geändert (pins.rs PINS_CHANGED_EVENT). M5: das
/// Spotlight-Frontend (PinBoxes.svelte + pinsMeta in +page.svelte) zieht
/// damit live nach — Pin-Fenster existieren nicht mehr (§6-Revision).
export const PINS_CHANGED_EVENT = 'ao:pins-changed';

/// Box-Kopfhöhe + Zeilenhöhe in px — MIRROR der Platzierungs-Schätzung in
/// pins.rs pin_add (44 + 34/Zeile). Nur für die Default-Stapelhöhe; die
/// gerenderte Box ist content-driven (Mockup-Vertrag).
export const PIN_BOX_W = 356;

export function pinBoxEstHeight(rows: number): number {
	return 44 + 34 * Math.max(rows, 1);
}

/// Edge-Margin — MIRROR von pins.rs EDGE_MARGIN (8): die Anzeige clampt
/// exakt mit denselben Regeln wie die Persistenz (snap_to_edge beim
/// pin_move), damit Drop-Ort == nach dem Load gerenderte Position
/// (t_88fb1585 M5-R2: kein PatchPos/Persistenz-Divergenz mehr).
export const EDGE_MARGIN = 8;

/// Box-Clamp — MIRROR von pins.rs snap_to_edge: identische Margin-Grenzen,
/// identische Reihenfolge (erst L/R, dann T/B), identisches Verhalten für
/// Boxen breiter/höher als der Screen. Die Anzeige rendert dadurch exakt
/// die Position, die das Backend beim pin_move persistieren würde — der
/// Drop-Ort bleibt nach dem Load sichtbar erhalten.
export function snapBoxToScreen(
	x: number,
	y: number,
	w: number,
	h: number,
	screen_w: number,
	screen_h: number
): { x: number; y: number } {
	const cw = Math.min(Math.max(w, 0), Math.max(screen_w, 0));
	const ch = Math.min(Math.max(h, 0), Math.max(screen_h, 0));
	const max_x = screen_w - cw - EDGE_MARGIN;
	const max_y = screen_h - ch - EDGE_MARGIN;
	const x2 =
		x < EDGE_MARGIN ? EDGE_MARGIN : x > Math.max(max_x, EDGE_MARGIN) ? Math.max(max_x, EDGE_MARGIN) : x;
	const y2 =
		y < EDGE_MARGIN ? EDGE_MARGIN : y > Math.max(max_y, EDGE_MARGIN) ? Math.max(max_y, EDGE_MARGIN) : y;
	return { x: x2, y: y2 };
}

