// M7-F7 (t_84008140): geteilter Entfernen-Pfad für Einträge im Sammel-
// Signatur-Pin — genutzt von ZWEI Oberflächen:
//   1. PinScanSignatures.svelte — Direktklick auf den Chip im INTERAKTIVEN
//      Spotlight-Kontext (Overlay offen, Palette sichtbar; User-Klarstellung
//      2026-11-19: „Entfernen bei offenem Overlay — der Pin interagierbar").
//   2. PinBoxes.svelte — ✕-Button in der Popover-Eintragsliste (auch für den
//      pins-only-gezeigten Weg über Patch→Klick→Popover).
// EINE Invoke-Sequenz, keine Kopie (Spec: „NICHT kopieren"). Die reine
// Listen-Logik bleibt unit-getestbar in scan-signature.ts (node --test ohne
// Tauri-Import — deshalb ist dieser dünne IPC-Wrapper eine eigene Datei).
// Payload-Vertrag unverändert ({entries, visibleSlots}) — das Entfernen ist
// eine Payload-Aktualisierung über den bestehenden pin_update-Pfad; erst bei
// leerer Liste wird der Pin selbst entfernt (pin_remove), wie beim
// Mining-Mode-Toggle-Verhalten.
import { invoke } from '@tauri-apps/api/core';
import type { PinEntry } from './pins';
import { sigCollectRows, sigEntries, sigRemoveEntry } from './scan-signature';

/// Entfernt GENAU den einen Entry (dedupe-Key oreId) aus dem Sammel-Pin.
/// `title` = lokalisiertes pins.scan_collect_title (der Caller resolved
/// $t — dieses Modul kennt kein i18n). true = Invoke geglückt; nach dem
/// Aufruf emittiert das Backend PINS_CHANGED (live Refresh der Liste).
export async function removeCollectEntry(
	pin: PinEntry,
	oreId: string,
	title: string
): Promise<boolean> {
	const entries = sigRemoveEntry(
		sigEntries(pin.content as Parameters<typeof sigEntries>[0]),
		oreId
	);
	try {
		if (entries.length === 0) {
			await invoke('pin_remove', {
				view_id: pin.view_id,
				screen: pin.screen,
				box_index: pin.box_index
			});
		} else {
			await invoke('pin_update', {
				view_id: pin.view_id,
				box_index: pin.box_index,
				title,
				kpi: null,
				rows: sigCollectRows(entries),
				content: { type: 'scan-signatures', entries },
				key: pin.key ?? null
			});
		}
		return true;
	} catch (err) {
		console.debug('[scan-signature-pin] removeCollectEntry FAILED', err);
		return false;
	}
}
