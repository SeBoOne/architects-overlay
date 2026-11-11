// M5 (t_d10a0d84): Box-/Popover-Geometrie für die Pin-Boxen im Spotlight —
// bewusst als reine Funktionen ausgelagert (testbar mit `node --test`,
// Muster pins-clamp.test.ts). Der Spotlight-Viewport deckt seinen Monitor
// 1:1 ab; die Pin-Koordinaten x/y sind dieselbe Viewport-Basis, die auch
// die Pin-Boxen nutzen (window.innerWidth/Height — CSS-px
// des Vollbild-Viewports; DPR wirkt auf beide Seiten identisch, daher kein
// separater Skalierungsschritt).
import { PIN_BOX_W, pinBoxEstHeight, snapBoxToScreen, type PinEntry } from './pins.ts';

/// Popover-Maße (CSS-px) — Mini-Controller neben der Box.
export const POPOVER_W = 260;
export const POPOVER_H = 160;

/// Patch-Position: Box an den Spotlight-Viewport clamplen — MIRROR der
/// Persistenz-Mathe (pins.rs snap_to_edge, t_88fb1585): identische
/// EDGE_MARGIN-Grenzen wie das Backend beim pin_move, damit Anzeige ==
/// Persistenz (der Drop-Ort bleibt nach dem Load exakt erhalten). Keine
/// Snap-Zonen der eigenen Art mehr — nur noch der eine, geteilte Clamp.
export function patchPos(pin: PinEntry, vw: number, vh: number): { x: number; y: number } {
	return snapBoxToScreen(pin.x, pin.y, PIN_BOX_W, pinBoxEstHeight(pin.rows.length), vw, vh);
}

/// Popover-Position: unter der Box, horizontal an der Box-Mitte orientiert,
/// an den Viewport-Rand geclampt (Modal-Regeln — kein position:fixed).
export function popoverPos(pin: PinEntry, vw: number, vh: number): { x: number; y: number } {
	const pos = patchPos(pin, vw, vh);
	const h = pinBoxEstHeight(pin.rows.length);
	const x = Math.max(0, Math.min(pos.x + PIN_BOX_W / 2, Math.max(0, vw - POPOVER_W)));
	const y = Math.max(0, Math.min(pos.y + h, Math.max(0, vh - POPOVER_H)));
	return { x, y };
}
