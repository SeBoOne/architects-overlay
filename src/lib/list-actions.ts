// M9-P (t_fb405e8d, P7+P8+P9) — geteilte Formular-Logik der Listen-Buttons.
//
// Pro Material-Zeile bleibt genau EIN Button; das Formular dahinter hat
// seine eigenen Qty-Felder. Diese kleine Rein-Funktionen kapseln die
// Mengen-Logik (Vorbefüllung + Auto-Reserve-Cap), damit sie node-test-bar
// ist — dieselbe Trennung wie list-bridge.ts (M9-D).

/** Die Mengen-Sicht eines aggregierten Materials (Teilmenge von AggMat). */
export type AggLite = {
	quantity: number;
	done_qty: number;
	reserved_qty: number;
};

/// Offener Restbedarf: Bedarf − erledigt − reserviert. M9-M zählt
/// done + reserved als Fortschritt, also zählen BEIDE gegen den Bedarf —
/// die Vorbefüllung des Formulars darf sonst doppeltes Abhaken erzeugen.
export function openRest(m: AggLite): number {
	// ?? 0: ältere Payloads (Probe-Stubs vor M9-M) ohne reserved/done-Felder.
	return Math.max(
		0,
		Math.round((m.quantity - (m.done_qty ?? 0) - (m.reserved_qty ?? 0)) * 100) / 100
	);
}

/// Auto-Reserve-Menge beim Inventar-Add (M9-I-Cap): reserviert wird nie
/// über den Listenbedarf hinaus. `add` = neu hinzugefügte Menge,
/// `demand` = Listenbedarf, `already` = bereits reserviert. Die
/// serverseitige Klemmung (cache::reservations, harter Cap) bleibt die
/// Quelle der Wahrheit — hier dieselbe Formel für Vorbefüllung/Hinweise.
export function reserveCap(add: number, demand: number, already: number): number {
	return Math.max(0, Math.round(Math.min(add, demand - already) * 100) / 100);
}
