/* Smoke-harness Tauri-stub: real Svelte palette JS logic, canned IPC data.
   Only for screenshots / DOM probes — real IPC search/math/market matching is
   covered by cargo tests (crates/uex-client apply_local u.a.).

   NB: 'get_marketplace' mirrors the PROVIDER contract in JS (q = jeder Term
   trifft mindestens ein Feld Name/Typ/Terminal/Verkäufer, operation-Regel,
   Sortierung, Limit) — der echte Pfad ist durch cargo tests belegt, dieser
   Mirror dient nur der Svelte-Logik-Verifikation (Live-Feedback, t_594c213c). */

/* Hangar (t_317f19c5): IPC-Stub für UI-Smoke. Die Daten in
   window.__HANGAR_DATA__ stammen aus dem ECHTEN Import (cargo test
   dump_real_import_for_smoke mit ARCH_HANGAR_DUMP, echter Parser+Merge
   gegen /tmp/HangarExport) — hier wird nur die IPC-Grenze gestubbt. */
window.__HANGAR_DATA__ = window.__HANGAR_DATA__ || [];
function hangarSave() { /* In-Memory; kein Persistenz-Nachweis nötig */ }
function hangarNextId() {
  return window.__HANGAR_DATA__.reduce((m, i) => Math.max(m, i.id || 0), 0) + 1;
}
function hangarUpsertStub(item) {
  const now = Math.floor(Date.now() / 1000);
  if (item.id != null) {
    const idx = window.__HANGAR_DATA__.findIndex((i) => i.id === item.id);
    if (idx >= 0) {
      window.__HANGAR_DATA__[idx] = { ...item, updated_at: now };
      return { id: item.id, inserted: false };
    }
  }
  const id = hangarNextId();
  window.__HANGAR_DATA__.push({ ...item, id, created_at: now, updated_at: now });
  return { id, inserted: true };
}

window.isTauri = true;
window.__TAURI__ = {};
const stubState = {
	palette: 'amber', detail_variant: 'c',
	uex_api_key_set: false, commodity_source: 'wiki',
	// K2 (t_5fe8eb4c, P12): font/icon scale in % — mirrors Settings.scale in the
	// Rust backend so the probe can A/B the scale live without a Tauri app.
	scale: 100
};
function stubSettings() {
	// K2: letzter Snapshot als Spiegel — Probes prüfen damit, ob ein Setter
	// IPC wirklich gefeuert hat (settings_set_scale etc.).
	globalThis.__lastSettings = { ...stubState };
	return { ...stubState };
}
window.__TAURI_INTERNALS__ = {
	transformCallback: (cb) => cb,
	metadata: { currentWindow: { label: 'spotlight' } },
	invoke: async (cmd, args) => {
		const a = args || {};
		if (cmd === 'plugin:event|listen' || cmd === 'plugin:event|unlisten') return 0;
		if (cmd === 'plugin:event|emit') return undefined;
		switch (cmd) {
			case 'overlay_get_state':
				return { visible: true, game_running: true, click_through: false };
			case 'overlay_set_visible':
				return true;
			case 'settings_get':
				return stubSettings();
			case 'settings_set_palette':
			case 'settings_set_detail_variant':
			case 'settings_set_uex_api_key':
			case 'settings_set_commodity_source':
				if (cmd === 'settings_set_commodity_source') stubState.commodity_source = a.name || stubState.commodity_source;
				return stubSettings();
			case 'settings_set_scale':
				if (Number.isInteger(a.pct) && a.pct >= 50 && a.pct <= 200) stubState.scale = a.pct;
				return stubSettings();
			case 'sync_status':
				return {
					provider: 'uex',
					syncing: false,
					online: true,
					last_error: null,
					datasets: [
						{ kind: 'commodity', label: 'Commodities', fresh: true, age_secs: 300, count: 205 },
						{ kind: 'item', label: 'Items', fresh: true, age_secs: 600, count: 7767 },
						{ kind: 'location', label: 'Locations', fresh: true, age_secs: 400, count: 826 }
					]
				};
			case 'sync_now':
				return { provider: 'uex', syncing: true, online: true, last_error: null, datasets: [] };
			case 'hangar_list':
				return JSON.parse(JSON.stringify(window.__HANGAR_DATA__));
			case 'hangar_add': {
				const r = hangarUpsertStub(a.item);
				return window.__HANGAR_DATA__.find((i) => i.id === r.id);
			}
			case 'hangar_update': {
				hangarUpsertStub(a.item);
				return window.__HANGAR_DATA__.find((i) => i.id === a.item.id);
			}
			case 'hangar_delete':
				window.__HANGAR_DATA__ = window.__HANGAR_DATA__.filter((i) => i.id !== a.id);
				return undefined;
			case 'hangar_import': {
				// Parser-Pfad ist durch cargo tests gegen die Realdateien belegt;
				// hier genügt die Zähl-Antwort für die UI-Note.
				const n = (window.__HANGAR_DATA__ || []).length;
				return { imported: n, updated: 0, skipped: 0, format: a.format };
			}
			case 'hangar_export':
				return JSON.stringify(window.__HANGAR_DATA__, null, 2);
			case 'get_prices': {
				const name = String(a.commodity || '');
				const now = Math.floor(Date.now() / 1000);
				const price = (loc, tid, buy, sell, q, extra, cname) => Object.assign({
					commodity: cname || name, commodity_id: 1, location: loc, terminal_id: tid,
					sell_price: sell, buy_price: buy, sell_stock: 0, buy_stock: 0,
					sell_status: sell > 0 ? 1 : 0, buy_status: buy > 0 ? 1 : 0,
					quality: q, updated: now - 240
				}, extra || {});
				// QA-Auflage C: Kontrakt-treuer Mirror — Location → `location.groups`
				// (je Terminal eine Gruppe, leere Gruppe = "keine Preisdaten"),
				// Terminal → EIGENE Preise, Item → prices:[] (niemals Fehler),
				// Commodity → Marktliste (unverändert).
				if (a.kind === 'terminal') {
					const tid = Number(a.id || 436);
					const byTerminal = {
						428: [
							{ commodity: 'Agricultural Supplies', buy: 1108, sell: 0, q: 0, code: 'AGRS', commodity_id: 3 },
							{ commodity: 'Distilled Spirits', buy: 1427, sell: 0, q: 0, code: 'DIST', commodity_id: 27 },
							{ commodity: 'DynaFlex', buy: 1235, sell: 0, q: 0, code: 'DYNF', commodity_id: 93 },
							{ commodity: 'Helium', buy: 727, sell: 0, q: 0, code: 'HELI', commodity_id: 38 }
						],
						437: [
							{ commodity: 'Ship Ammunition - Size 1', buy: 0, sell: 2320, q: 0, code: 'SHPA1', commodity_id: 201 },
							{ commodity: 'Ship Ammunition - Size 2', buy: 0, sell: 2420, q: 0, code: 'SHPA2', commodity_id: 202 },
							{ commodity: 'Ship Decoy Countermeasures', buy: 0, sell: 4293, q: 0, code: 'SHPDC', commodity_id: 208 }
						],
						436: [
							{ commodity: 'Agricultural Supplies', buy: 1108, sell: 0, q: 0, code: 'AGRS', commodity_id: 3 },
							{ commodity: 'Carbon', buy: 235, sell: 0, q: 0, code: 'CARB', commodity_id: 102 },
							{ commodity: 'Distilled Spirits', buy: 1427, sell: 0, q: 0, code: 'DIST', commodity_id: 27 },
							{ commodity: 'DynaFlex', buy: 1235, sell: 0, q: 0, code: 'DYNF', commodity_id: 93 },
							{ commodity: 'Helium', buy: 727, sell: 0, q: 0, code: 'HELI', commodity_id: 38 },
							{ commodity: 'Mercury', buy: 715, sell: 0, q: 0, code: 'MERC', commodity_id: 123 },
							{ commodity: 'Waste', buy: 229, sell: 0, q: 0, code: 'WAST', commodity_id: 79 },
							{ commodity: 'Altruciatoxin', buy: 0, sell: 6200, q: 0, code: 'AUTR', commodity_id: 4 },
							{ commodity: 'Revenant Tree Pollen', buy: 0, sell: 1200, q: 0, code: 'REVE', commodity_id: 66 },
							{ commodity: 'Argon', buy: 0, sell: 420, q: 0, code: 'ARGO', commodity_id: 111 },
							{ commodity: 'Beryl', buy: 0, sell: 19000, q: 0, code: 'BERY', commodity_id: 11 },
							{ commodity: 'Bioplastic', buy: 0, sell: 8600, q: 0, code: 'BIOPL', commodity_id: 112 },
							{ commodity: 'Borase', buy: 0, sell: 27000, q: 0, code: 'BORA', commodity_id: 15 },
							{ commodity: 'Construction Materials', buy: 0, sell: 12000, q: 0, code: 'CMAT', commodity_id: 19 },
							{ commodity: 'Corundum', buy: 0, sell: 3500, q: 0, code: 'CORU', commodity_id: 22 },
							{ commodity: 'Fluorine', buy: 0, sell: 1200, q: 0, code: 'FLUO', commodity_id: 31 },
							{ commodity: 'Hephaestanite', buy: 0, sell: 4600, q: 0, code: 'HEPH', commodity_id: 39 },
							{ commodity: 'Human Food Bars', buy: 0, sell: 470, q: 0, code: 'HFBA', commodity_id: 97 },
							{ commodity: 'Hydrogen', buy: 0, sell: 1100, q: 0, code: 'HYDR', commodity_id: 41 },
							{ commodity: 'Neon', buy: 0, sell: 17000, q: 0, code: 'NEON', commodity_id: 52 },
							{ commodity: 'Agricium', buy: 0, sell: 9200, q: 0, code: 'AGRI', commodity_id: 1 },
							{ commodity: 'Nitrogen', buy: 0, sell: 3000, q: 0, code: 'NITR', commodity_id: 106 },
							{ commodity: 'Omnapoxy', buy: 0, sell: 4300, q: 0, code: 'OMPO', commodity_id: 115 },
							{ commodity: 'Pressurized Ice', buy: 0, sell: 5200, q: 0, code: 'PICE', commodity_id: 101 },
							{ commodity: 'Processed Food', buy: 0, sell: 1400, q: 0, code: 'PFOO', commodity_id: 56 },
							{ commodity: 'Quartz', buy: 0, sell: 4200, q: 0, code: 'QUAR', commodity_id: 60 },
							{ commodity: 'Scrap', buy: 0, sell: 3500, q: 0, code: 'SCRA', commodity_id: 67 },
							{ commodity: 'Steel', buy: 0, sell: 2000, q: 0, code: 'STEE', commodity_id: 109 },
							{ commodity: 'Stims', buy: 0, sell: 5000, q: 0, code: 'STIM', commodity_id: 70 },
							{ commodity: 'Tin', buy: 0, sell: 3000, q: 0, code: 'TIN', commodity_id: 103 },
							{ commodity: 'Titanium', buy: 0, sell: 8300, q: 0, code: 'TITA', commodity_id: 75 },
							{ commodity: 'Recycled Material Composite', buy: 7587, sell: 0, q: 0, code: 'RMC', commodity_id: 63 },
							{ commodity: 'Ship Ammunition - Size 7', buy: 8671, sell: 0, q: 0, code: 'SHPA7', commodity_id: 207 },
							{ commodity: 'Ship Ammunition - Size 1', buy: 6868, sell: 0, q: 0, code: 'SHPA1', commodity_id: 201 },
							{ commodity: 'Ship Ammunition - Size 2', buy: 7126, sell: 0, q: 0, code: 'SHPA2', commodity_id: 202 },
							{ commodity: 'Ship Ammunition - Size 3', buy: 7384, sell: 0, q: 0, code: 'SHPA3', commodity_id: 203 },
							{ commodity: 'Ship Ammunition - Size 4', buy: 7641, sell: 0, q: 0, code: 'SHPA4', commodity_id: 204 },
							{ commodity: 'Ship Ammunition - Size 5', buy: 7985, sell: 0, q: 0, code: 'SHPA5', commodity_id: 205 },
							{ commodity: 'Ship Ammunition - Size 6', buy: 8328, sell: 0, q: 0, code: 'SHPA6', commodity_id: 206 },
							{ commodity: 'Ship Decoy Countermeasures', buy: 4293, sell: 0, q: 0, code: 'SHPDC', commodity_id: 208 },
							{ commodity: 'Hydrogen Fuel', buy: 782, sell: 0, q: 0, code: 'HYDF', commodity_id: 104 },
							{ commodity: 'Quantum Fuel', buy: 1760, sell: 0, q: 0, code: 'QFUE', commodity_id: 87 },
													{ commodity: 'Ship Noise Countermeasures', buy: 8585, sell: 0, q: 0, code: 'SHPNC', commodity_id: 209 }
											],
											435: [
												{ commodity: 'MedPen (Hemozal)', buy: 295, sell: 0, q: 0, code: 'MEDH', commodity_id: 77 },
												{ commodity: 'MedPen (Roxzadolith)', buy: 120, sell: 0, q: 0, code: 'MEDR', commodity_id: 78 },
												{ commodity: 'MedPen (Demicide)', buy: 55, sell: 0, q: 0, code: 'MEDI', commodity_id: 79 }
											]
											};
					const rows = byTerminal[tid] || [];
					const prices = rows.map((r) => ({
						commodity: r.commodity, commodity_id: r.commodity_id || 1, location: 'Checkmate — ' + tid,
						terminal_id: tid, sell_price: r.sell, buy_price: r.buy,
						sell_stock: 0, buy_stock: 0, sell_status: r.sell > 0 ? 1 : 0,
						buy_status: r.buy > 0 ? 1 : 0, quality: r.q, updated: now - 240
					}));
					return {
						commodity: name, source: 'uex', fetched_at: now,
						location: null, prices
					};
				}
				if (a.kind === 'location') {
					const groups = [
						{
							terminal_id: 428,
							terminal_name: 'Landing Services - Checkmate',
							prices: []
						},
						{
							terminal_id: 431,
							terminal_name: 'Refinement Center - Checkmate',
							prices: []
						},
						{
							terminal_id: 435,
							terminal_name: 'Medical Shop - Checkmate',
							prices: [
								price('Checkmate — Medical', 435, 295, 0, 0, { code: 'MEDH', commodity_id: 77 }, 'MedPen (Hemozal)'),
								price('Checkmate — Medical', 435, 120, 0, 0, { code: 'MEDR', commodity_id: 78 }, 'MedPen (Roxzadolith)'),
								price('Checkmate — Medical', 435, 55, 0, 0, { code: 'MEDI', commodity_id: 79 }, 'MedPen (Demicide)')
							]
						},
						{
							terminal_id: 429,
							terminal_name: 'Lama Terminal - Checkmate',
							prices: [
								price('Checkmate — Lama Terminal', 429, 999, 0, 0, { code: 'LAMA', commodity_id: 88 }, 'Lama Feed'),
								price('Checkmate — Lama Terminal', 429, 0, 5400, 0, { code: 'LAMX', commodity_id: 89 }, 'Lama Extract')
							]
						},
						{
							terminal_id: 436,
							terminal_name: 'Admin - Checkmate',
							prices: [
																price('Checkmate — Admin Terminal', 436, 1108, 0, 0, { code: 'AGRS', commodity_id: 3 }),
																price('Checkmate — Admin Terminal', 436, 235, 0, 0, { code: 'CARB', commodity_id: 102 }),
																price('Checkmate — Admin Terminal', 436, 1427, 0, 0, { code: 'DIST', commodity_id: 27 }),
																price('Checkmate — Admin Terminal', 436, 1235, 0, 0, { code: 'DYNF', commodity_id: 93 }),
																price('Checkmate — Admin Terminal', 436, 727, 0, 0, { code: 'HELI', commodity_id: 38 }),
																price('Checkmate — Admin Terminal', 436, 715, 0, 0, { code: 'MERC', commodity_id: 123 }),
																price('Checkmate — Admin Terminal', 436, 229, 0, 0, { code: 'WAST', commodity_id: 79 }),
																price('Checkmate — Admin Terminal', 436, 0, 6200, 0, { code: 'AUTR', commodity_id: 4 }),
																price('Checkmate — Admin Terminal', 436, 0, 1200, 0, { code: 'REVE', commodity_id: 66 }),
																price('Checkmate — Admin Terminal', 436, 0, 420, 0, { code: 'ARGO', commodity_id: 111 }),
																price('Checkmate — Admin Terminal', 436, 0, 19000, 0, { code: 'BERY', commodity_id: 11 }),
																price('Checkmate — Admin Terminal', 436, 0, 8600, 0, { code: 'BIOPL', commodity_id: 112 }),
																price('Checkmate — Admin Terminal', 436, 0, 27000, 0, { code: 'BORA', commodity_id: 15 }),
																price('Checkmate — Admin Terminal', 436, 0, 12000, 0, { code: 'CMAT', commodity_id: 19 }),
																price('Checkmate — Admin Terminal', 436, 0, 3500, 0, { code: 'CORU', commodity_id: 22 }),
																price('Checkmate — Admin Terminal', 436, 0, 1200, 0, { code: 'FLUO', commodity_id: 31 }),
																price('Checkmate — Admin Terminal', 436, 0, 4600, 0, { code: 'HEPH', commodity_id: 39 }),
																price('Checkmate — Admin Terminal', 436, 0, 470, 0, { code: 'HFBA', commodity_id: 97 }),
																price('Checkmate — Admin Terminal', 436, 0, 1100, 0, { code: 'HYDR', commodity_id: 41 }),
																price('Checkmate — Admin Terminal', 436, 0, 17000, 0, { code: 'NEON', commodity_id: 52 }),
																price('Checkmate — Admin Terminal', 436, 0, 9200, 0, { code: 'AGRI', commodity_id: 1 }),
																price('Checkmate — Admin Terminal', 436, 0, 3000, 0, { code: 'NITR', commodity_id: 106 }),
																price('Checkmate — Admin Terminal', 436, 0, 4300, 0, { code: 'OMPO', commodity_id: 115 }),
																price('Checkmate — Admin Terminal', 436, 0, 5200, 0, { code: 'PICE', commodity_id: 101 }),
																price('Checkmate — Admin Terminal', 436, 0, 1400, 0, { code: 'PFOO', commodity_id: 56 }),
																price('Checkmate — Admin Terminal', 436, 0, 4200, 0, { code: 'QUAR', commodity_id: 60 }),
																price('Checkmate — Admin Terminal', 436, 0, 3500, 0, { code: 'SCRA', commodity_id: 67 }),
																price('Checkmate — Admin Terminal', 436, 0, 2000, 0, { code: 'STEE', commodity_id: 109 }),
																price('Checkmate — Admin Terminal', 436, 0, 5000, 0, { code: 'STIM', commodity_id: 70 }),
																price('Checkmate — Admin Terminal', 436, 0, 3000, 0, { code: 'TIN', commodity_id: 103 }),
																price('Checkmate — Admin Terminal', 436, 0, 8300, 0, { code: 'TITA', commodity_id: 75 }),
																price('Checkmate — Admin Terminal', 436, 7587, 0, 0, { code: 'RMC', commodity_id: 63 }),
																price('Checkmate — Admin Terminal', 436, 8671, 0, 0, { code: 'SHPA7', commodity_id: 207 }),
																price('Checkmate — Admin Terminal', 436, 6868, 0, 0, { code: 'SHPA1', commodity_id: 201 }),
																price('Checkmate — Admin Terminal', 436, 7126, 0, 0, { code: 'SHPA2', commodity_id: 202 }),
																price('Checkmate — Admin Terminal', 436, 7384, 0, 0, { code: 'SHPA3', commodity_id: 203 }),
																price('Checkmate — Admin Terminal', 436, 7641, 0, 0, { code: 'SHPA4', commodity_id: 204 }),
																price('Checkmate — Admin Terminal', 436, 7985, 0, 0, { code: 'SHPA5', commodity_id: 205 }),
																price('Checkmate — Admin Terminal', 436, 8328, 0, 0, { code: 'SHPA6', commodity_id: 206 }),
																price('Checkmate — Admin Terminal', 436, 4293, 0, 0, { code: 'SHPDC', commodity_id: 208 }),
																price('Checkmate — Admin Terminal', 436, 782, 0, 0, { code: 'HYDF', commodity_id: 104 }),
																price('Checkmate — Admin Terminal', 436, 1760, 0, 0, { code: 'QFUE', commodity_id: 87 }),
																price('Checkmate — Admin Terminal', 436, 8585, 0, 0, { code: 'SHPNC', commodity_id: 209 })
							]
						},
						{
							terminal_id: 437,
							terminal_name: 'Guns - Checkmate',
							prices: []
						},
						{
							terminal_id: 438,
							terminal_name: 'Sharp Shooters - Checkmate',
							prices: []
						},
						{
							terminal_id: 440,
							terminal_name: 'Ship Parts - Checkmate',
							prices: []
						},
						{
							terminal_id: 441,
							terminal_name: 'Refinery Shop - Checkmate',
							prices: []
						},
						{
							terminal_id: 442,
							terminal_name: 'Refinery Ore Sales - Checkmate',
							prices: []
						},
						{
							terminal_id: 490,
							terminal_name: 'Chuan - Checkmate',
							prices: []
						},
						{
							terminal_id: 491,
							terminal_name: 'Filtered Water - Checkmate',
							prices: []
						},
						{
							terminal_id: 492,
							terminal_name: 'Station Convenience - Checkmate',
							prices: []
						},
						{
							terminal_id: 493,
							terminal_name: 'Cargo Services - Checkmate',
							prices: []
						},
						{
							terminal_id: 494,
							terminal_name: 'Buy and Fly - Checkmate',
							prices: []
						},
						{
							terminal_id: 596,
							terminal_name: 'Gear Up - Checkmate',
							prices: []
						},
						{
							terminal_id: 597,
							terminal_name: 'Ready To Trade - Checkmate',
							prices: []
						},
						{
							terminal_id: 600,
							terminal_name: 'Unique Quality - Checkmate',
							prices: []
						},
						{
							terminal_id: 689,
							terminal_name: 'Equipment Contested Zone Checkmate',
							prices: []
						}
					];
					const prices = [].concat(...groups.map((g) => g.prices));
					return {
						commodity: name, source: 'uex', fetched_at: now, prices,
						location: {
							location: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station',
							terminal: 'Checkmate Station', groups
						}
					};
				}
				if (a.kind === 'item') {
					// Point 8: Item detail returns real shop prices from /items_prices,
					// not an empty table. (Stub: CSP-68H backpack @ CenterMass.)
					//
					// QA Runde 2 (Punkt C): die echte API sendet auf /items_prices auch
					// `"price_sell": null` (shops kaufen nichts zurück — measured 1/16 @
					// id_item=465). `sell_price` ist auf der SEAM-Ebene (PricePoint) immer
					// eine Zahl: der Backend-DTO mappt wire-null → 0.0 (cargo fixture
					// item_prices_465_null.json, siehe dto.rs de_f64_null_to_zero + Import
					// in lib.rs map_item_price_point). Dieser Stub bildet das exakt nach —
					// `sel0()` sourct aus einem *nullbaren* Kanalwert → 0, damit der Mirror
					// den echten Pfad nicht länger verdeckt (Regression trennen: Cargo-Test
					// ist die rote Assertion, der Stub der grüne UI-Mirror).
					const sel0 = (v) => (v == null ? 0 : v);
					const itemRows = {
						465: [
							{ commodity: 'CSP-68H Backpack', buy: 295, sell: null, q: 0, code: null, commodity_id: 465, loc: 'CenterMass - ArcCorp', tid: 107, term: 'CenterMass - ArcCorp' },
							{ commodity: 'CSP-68H Backpack', buy: 295, sell: 0, q: 0, code: null, commodity_id: 465, loc: 'CenterMass - Hurston', tid: 108, term: 'CenterMass - Hurston' }
						]
					};
					const iid = Number(a.id || 0);
					const rows = itemRows[iid] || [];
					const prices = rows.map((r) => ({
						commodity: r.commodity, commodity_id: r.commodity_id, location: r.loc, terminal_id: r.tid,
						sell_price: sel0(r.sell), buy_price: r.buy, sell_stock: 0, buy_stock: 0,
						sell_status: sel0(r.sell) > 0 ? 1 : 0, buy_status: r.buy > 0 ? 1 : 0,
						quality: r.q, updated: now - 240
					}));
					return { commodity: name, source: 'uex', fetched_at: now, location: null, prices };
				}
				// Commodity default: 43 price rows (30 @ Checkmate Admin 436 + 13 across
				// ARC-L1/Area18/Orison/Levski/Lorille) — reicht für "43 Preiszeilen" Smoke
				// und lässt sich per Ortssperre (436) auf 30 Zeilen eingrenzen.
				const adminBuy = [1108,235,1427,1235,727,715,229,7587,8671,6868,7126,7384,7641,7985,8328,4293,782,1760,8585,9200,12000,8600,27000,19000,3500,4600,1100,17000,8300,4300];
				const oT=[11,12,13,14,15], oL=['ARC-L1 Wide Forest Station','Area18 — TDD','Orison — Providence','Levski — Grand Barter','Lorille — Tammany & Sons'];
				const prices = Array.from({ length: 43 }, (_, i) => {
					if (i < 30) return price('Checkmate — Admin Terminal', 436, adminBuy[i], 0, 0, { code: 'AGRI' + (i % 9), commodity_id: i + 1 });
					const t = oT[i % 5], loc = oL[i % 5];
					const buy = (i % 2 === 0) ? 800 + i * 20 : 0;
					const sell = (i % 2 === 0) ? 0 : 2000 + i * 20;
					return price(loc, t, buy, sell, 0, { code: 'OT' + (i - 30), commodity_id: i + 300 });
				});
				return {
					commodity: name,
					source: 'uex',
					fetched_at: now,
					location: null,
					prices
				};
			}
			case 'search': {
				const q = String(a.query || '').toLowerCase();
				const f = a.filters || null;
				const kinds = [];
				const groups = [];
				// >20 Gold-Varianten, damit die Liste über den sichtbaren Bereich
				// hinausragt (Auto-Scroll-Prüfung im Smoke).
				const goldPool = Array.from({ length: 22 }, (_, i) => ({
					id: 1000 + i,
					name: 'Gold ' + String(i + 1).padStart(2, '0'),
					kind: 'commodity',
					subtitle: 'Commodity · börsenfähig (Stub)',
					code: 'AU' + (i + 1)
				}));
				const pool = {
					commodity: [
						...goldPool,
						{ id: 3, name: 'Gold', kind: 'commodity', subtitle: 'Commodity · börsenfähig', code: 'AU' },
						{ id: 57, name: 'Bexalite (Raw)', kind: 'commodity', subtitle: 'Commodity · Raffinerie nötig', code: 'BEX' },
						{ id: 48, name: 'Quantanium (Raw)', kind: 'commodity', subtitle: 'Commodity · instabil', code: 'QNT' },
						{ id: 22, name: 'Titanium', kind: 'commodity', subtitle: 'Commodity · Metall', code: 'TIT' },
						{ id: 9, name: 'Laranite', kind: 'commodity', subtitle: 'Commodity · Kristalle', code: 'LRN' }
					],
					item: [
						{ id: 2914, name: 'Gold Beret', kind: 'item', subtitle: 'Item · Bekleidung', code: null },
						{ id: 3105, name: 'Goldschnalle — Helmvisier', kind: 'item', subtitle: 'Item · Rüstungsteil', code: null },
						{ id: 80, name: 'MedPen', kind: 'item', subtitle: 'Item · Verbrauchsgut', code: 'MED' },
						{ id: 465, name: 'CSP-68H Backpack', kind: 'item', subtitle: 'Item · Rucksack', code: null }
					],
					location: [
						// Reale Wiki-Location-Id (> 2^53, aus der echten Cache-DB):
						// beweist im Component-Flow, dass die Id als String
						// unangetastet durch den Picker geht (t_1bab1fea Blocker 2).
						{ id: '9218937313948769813', name: 'Mining Claim #QR4-25Q', kind: 'location', subtitle: 'Stanton > Crusader > Yela > Bashak Fields', code: null },
						{ id: 436, name: 'Checkmate Station', kind: 'location', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: null },
							{ id: 428, name: 'Landing Services - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'LSCHE' },
							{ id: 431, name: 'Refinement Center - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'RCCHE' },
							{ id: 435, name: 'Medical Shop - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'MECHE' },
							{ id: 436, name: 'Admin - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'CHECK' },
							{ id: 437, name: 'Guns - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'GNCHE' },
							{ id: 438, name: 'Sharp Shooters - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'SSCHE' },
							{ id: 440, name: 'Ship Parts - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'SPCHE' },
							{ id: 441, name: 'Refinery Shop - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'REFCHE' },
							{ id: 442, name: 'Refinery Ore Sales - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'ROCHE' },
							{ id: 490, name: 'Chuan - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'CHCHE' },
							{ id: 491, name: 'Filtered Water - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'FWCHE' },
							{ id: 492, name: 'Station Convenience - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'STCCH' },
							{ id: 493, name: 'Cargo Services - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'CTCHE' },
							{ id: 494, name: 'Buy and Fly - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'BFCHE' },
							{ id: 596, name: 'Gear Up - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'GUCHE' },
							{ id: 597, name: 'Ready To Trade - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'RTCHE' },
							{ id: 600, name: 'Unique Quality - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'UQCHE' },
							{ id: 689, name: 'Equipment Contested Zone Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: 'CZCMS' }
					]
				};
				const want = f && f.length ? f : Object.keys(pool);
				let total = 0;
				for (const kind of ['commodity', 'item', 'location']) {
					if (!want.includes(kind)) continue;
					const hits = (pool[kind] || []).filter(
						(h) => !q || h.name.toLowerCase().includes(q) || (h.code || '').toLowerCase().includes(q) || h.subtitle.toLowerCase().includes(q)
					);
					total += hits.length;
					if (hits.length) {
						groups.push({
							kind,
							label: kind === 'commodity' ? 'Commodities' : kind === 'item' ? 'Items' : 'Locations',
							hits
						});
					}
				}
				var res = { groups, total };
				globalThis.__lastSearch = { query: a.query, filters: f };
				return res;
			}
			case 'math_eval': {
				// JS mirror of the Rust evaluator for screenshots only.
				function ev(src) {
					function skip(p){while(p.i<p.c.length && (p.c[p.i]===' '||p.c[p.i]==='\t'))p.i++;}
					const P={i:0,c:Array.from(src)};
					function n(){skip(P);let s='';while(P.i<P.c.length&&(/[0-9.]/.test(P.c[P.i]))&&s.length<20){s+=P.c[P.i];P.i++;}return parseFloat(s)||0;}
					function p(){skip(P);if(P.i>=P.c.length)throw Error('vollständig');let ch=P.c[P.i];if(ch==='-'){P.i++;return -p();}if(ch==='('){P.i++;const v=e();skip(P);if(P.c[P.i]===')')P.i++;return v;}let v=n();skip(P);if(P.c[P.i]==='%'){P.i++;v/=100;}return v;}
					function t(){skip(P);let v=p();while(P.i<P.c.length&&(P.c[P.i]==='*'||P.c[P.i]==='/')){const op=P.c[P.i];P.i++;const r=p();v=op==='*'?v*r:v/r;}return v;}
					function e(){skip(P);let v=t();while(P.i<P.c.length&&(P.c[P.i]==='+'||P.c[P.i]==='-')){const op=P.c[P.i];P.i++;const r=t();v=op==='+'?v+r:v-r;}return v;}
					return e();
				}
				try {
					const v2 = ev(String((a.expr||'').replace(/^=+/,'')).trim());
					if (!isFinite(v2)) throw Error('nicht endlich');
					const s = String(v2); const body=s.includes('.')?String(v2.toFixed(10)).replace(/0+$/,'').replace(/\\.$/,''):s;
					return body;
				} catch (e) { throw Error(String(e.message||e)); }
			}
			case 'get_marketplace': {
				const filter = a.filters || {};
				// Offer-Pool (Qualität 0–1000, UEX-Skala) — deckt Name/Typ/
				// Terminal/Verkäufer und Qualitäts-Spreizung für den Smoke ab.
				const now = Math.floor(Date.now() / 1000);
				const O = (id, title, location, seller, quality, price, extra) => Object.assign({
					id, title, description: '', item_id: 0, category_id: 0,
					operation: 'sell', listing_type: 'item', price,
					currency: 'UEC', unit: 'scu', quantity: 1200,
					location, region: 'Stanton', seller, quality,
					updated: now - id * 60, expires: null, slug: 'slug-' + id
				}, extra || {});
				const pool = [
					O(1, 'Gold Ore (Raw)', 'Port Tressler', 'HaulerCo', 950, 4812),
					O(2, 'Gold Ore (Refined)', 'Everus Harbor', 'HaulerCo', 880, 962),
					O(3, 'Iron Ore (Raw)', 'Port Tressler', 'OrbitalMining', 720, 512),
					O(4, 'MedPen', 'Orison — Providence', 'RedCrossShop', 640, 45),
					O(5, 'Hull-C Charter', 'Everus Harbor', 'RentABoat', 300, 25000, { listing_type: 'service' }),
					O(6, 'Kartographie-Service', 'Lorville — TDD', 'ServiceGmbH', 500, 120, { listing_type: 'service' }),
					O(7, 'Bexalite (Raw)', 'Port Tressler', 'HaulerCo', 990, 6200),
					O(8, 'Quantanium (Raw)', 'ARC-L1 Wide Forest Station', 'InstaMiner', 300, 8100),
					O(9, 'Titanium', 'Lorville — TDD', 'BulkHaul', 600, 3150),
					O(10, 'Gold Beret', 'Area18 — TDD', 'FashionCo', 400, 2500),
					O(11, 'Goldschnalle — Helmvisier', 'Area18 — TDD', 'FashionCo', 350, 1800),
					O(12, 'Laranite', 'Port Tressler', 'HaulerCo', 980, 7300),
					O(13, 'Laranite (Ankauf)', 'Port Tressler', 'BuyWant', 450, 7100, { operation: 'buy' }),
					O(14, 'Artifex Ore', 'Levski — Grand Barter', 'NyxMiner', 800, 880, { region: 'Nyx' })
				];
				// 30 Parallax-Angebote (Qualität 900–1000, ausschließlich Port Tressler) —
				// reicht für "≥15 anzeigen + Weitere-laden"-Smoke (erste Seite 15, dann 30).
				const plocations = ['Port Tressler','Everus Harbor','ARC-L1 Wide Forest Station','Area18 — TDD','Orison — Providence'];
				const ppool = Array.from({ length: 30 }, (_, i) => O(100 + i, 'Parallax ' + (i + 1), plocations[i % 5], 'ParallaxCo', 900 + (i % 11) * 10, 1000 + i * 50));
				pool.push(...ppool);
				// Provider-Mirror (Market-Fixes-Kontrakt, t_3402ae7e): der echte
				// Provider verengt serverseitig (id_item für bekannte Items,
				// sonst Verkauf+Kauf-Fenster gemerged) und wendet DANACH
				// q/Region-Filter + Sortierung auf das komplette Fenster an
				// (provider::apply_marketplace_filters — cargo-test-belegt).
				// Der `limit`-Parameter wird von der API ignoriert und hier
				// ebenfalls nicht mehr angewendet; die Seiten blättert die
				// Svelte-Pagination im Snapshot.
				const terms = (filter.q || '').trim().toLowerCase().split(/\s+/).filter(Boolean);
				let out = pool.filter((o) => {
					if (filter.operation && o.operation !== filter.operation) return false;
					if (!terms.length) return true;
					const hay = [
						o.title, o.description, o.seller, o.location,
						o.listing_type, o.operation, o.region
					].map((s) => String(s).toLowerCase());
					return terms.every((t) => hay.some((h) => h.includes(t)));
				});
				if (filter.region) out = out.filter((o) => o.region === filter.region);
				const sort = filter.sort || 'newest';
				if (sort === 'price_asc') out.sort((x, y) => x.price - y.price);
				else if (sort === 'price_desc') out.sort((x, y) => y.price - x.price);
				else out.sort((x, y) => y.updated - x.updated);
				globalThis.__lastMarket = { filters: filter, returned: out.length };
				return { source: 'cache', fetched_at: now, offers: out, note: null };
			}
		case 'inventory_list': {
			return invRows();
		}
		case 'inventory_locations': {
			const seen = new Set();
			const names = [];
			for (const r of __inv.rows) {
				const k = r.location_name.toLowerCase();
				if (!seen.has(k)) { seen.add(k); names.push(r.location_name); }
			}
			return names;
		}
		case 'inventory_add': {
			const e = a.entry || {};
			const now = Math.floor(Date.now() / 1000);
			const row = {
				// Reale Ids kommen als Strings über die IPC (t_1bab1fea Blocker 2:
				// wiki ids > 2^53 runden als JS-Number still — Konvention
				// t_925a577a). Der Stub spiegelt das Verhalten.
				id: String(++__inv.nextId),
				entity_kind: e.entity_kind ?? null,
				entity_id: e.entity_id ?? null,
				name: e.name, quality: e.quality,
				location_id: e.location_id ?? null,
				location_name: e.location_name,
				quantity: e.quantity, price: e.price ?? null,
				created_at: now, updated_at: now
			};
			__inv.rows.push(row);
			globalThis.__lastInv = { op: 'add', row };
			return row;
		}
		case 'inventory_update': {
			const e = a.entry || {};
			const row = __inv.rows.find((r) => String(r.id) === String(a.id));
			if (!row) throw Error('Position ' + a.id + ' existiert nicht');
			Object.assign(row, e, { entity_id: e.entity_id ?? null, location_id: e.location_id ?? null, price: e.price ?? null, updated_at: Math.floor(Date.now() / 1000) });
			globalThis.__lastInv = { op: 'update', row };
			return null;
		}
		case 'inventory_delete': {
			const i = __inv.rows.findIndex((r) => String(r.id) === String(a.id));
			if (i < 0) throw Error('Position ' + a.id + ' existiert nicht');
			__inv.rows.splice(i, 1);
			globalThis.__lastInv = { op: 'delete', id: a.id };
			return null;
		}
		case 'vehicles_scu': {
				// SCU-Kapazitäten (lowercase name → SCU) — Smoke-Daten.
				return { 'carrack': 62, 'nomad': 24, 'cutlass black': 46 };
			}
			case 'get_routes': {
				// Provider-Kontrakt-Spiegel: RouteFilters → gefilterte/sortierte
				// Routen (Berechnung selbst ist cargo-test-belegt in
				// provider::routes — hier genügt die Kontrakt-Struktur).
				const f = (a && a.filters) || {};
				const now = Math.floor(Date.now() / 1000);
				const R = (cid, cname, illegal, bt, bchain, buy, bauto, bcargo, bground, st, schain, sell, sauto, scargo, sground, stock, dist) => ({
					commodity_id: cid, commodity: cname, illegal,
					buy_terminal_id: bt[0], buy_terminal: bt[1], buy_chain: bchain, buy_price: buy,
					buy_terminal_autoload: bauto, buy_terminal_cargo_deck: bcargo, buy_terminal_ground: bground,
					sell_terminal_id: st[0], sell_terminal: st[1], sell_chain: schain, sell_price: sell,
					sell_terminal_autoload: sauto, sell_terminal_cargo_deck: scargo, sell_terminal_ground: sground,
					sell_stock: stock, profit_per_scu: sell - buy, profit_total: (sell - buy) * (f.scu || 1),
					cost_total: buy * (f.scu || 1), margin: Math.round((sell - buy) / buy * 10000) / 10000, distance: dist
				});
				let routes = [
					R(27, 'Distilled Spirits', false, [436, 'Checkmate Admin'], 'Stanton > Crusader > Yela > Daymar', 1427, true, true, true, [428, 'Bai Hang'], 'Stanton > Crusader > Yela > Yela Prime', 2320, true, true, false, 900, 2),
					R(3, 'Agricultural Supplies', false, [436, 'Checkmate Admin'], 'Stanton > Crusader > Yela > Daymar', 1108, true, true, true, [437, 'Port Olisare'], 'Stanton > Crusader > Yela > Yela Prime', 1780, false, true, false, 4000, 2),
					R(4, 'Altruciatoxin', true, [436, 'Checkmate Admin'], 'Stanton > Crusader > Yela > Daymar', 3200, false, false, true, [428, 'Bai Hang'], 'Stanton > Crusader > Yela > Yela Prime', 6200, true, true, false, 120, 2)
				];
				if (f.commodity) routes = routes.filter((r) => r.commodity.toLowerCase().includes(String(f.commodity).toLowerCase()));
				if (f.from) routes = routes.filter((r) => (r.buy_terminal + ' ' + r.buy_chain).toLowerCase().includes(String(f.from).toLowerCase()));
				if (f.to) routes = routes.filter((r) => (r.sell_terminal + ' ' + r.sell_chain).toLowerCase().includes(String(f.to).toLowerCase()));
				if (f.legality === 'legal') routes = routes.filter((r) => !r.illegal);
				if (f.legality === 'illegal') routes = routes.filter((r) => r.illegal);
				if (f.autoload !== null && f.autoload !== undefined) routes = routes.filter((r) => r.buy_terminal_autoload === f.autoload && r.sell_terminal_autoload === f.autoload);
				if (f.cargo_deck !== null && f.cargo_deck !== undefined) routes = routes.filter((r) => r.buy_terminal_cargo_deck === f.cargo_deck && r.sell_terminal_cargo_deck === f.cargo_deck);
				if (f.ground_only) routes = routes.filter((r) => r.buy_terminal_ground && r.sell_terminal_ground);
				if (f.space_only) routes = routes.filter((r) => !r.buy_terminal_ground && !r.sell_terminal_ground);
				const sort = f.sort || 'profit_desc';
				if (sort === 'profit_asc') routes.sort((x, y) => x.profit_per_scu - y.profit_per_scu);
				else if (sort === 'total_profit_desc') routes.sort((x, y) => y.profit_total - x.profit_total);
				else if (sort === 'margin_desc') routes.sort((x, y) => y.margin - x.margin);
				else if (sort === 'distance_asc') routes.sort((x, y) => x.distance - y.distance);
				else if (sort === 'natural') { /* Originalliste (Provider-Reihenfolge) */ }
				else routes.sort((x, y) => y.profit_per_scu - x.profit_per_scu);
				return { source: 'stub', swept_at: now, routes };
			}
			default:
				return null;
		}
	}
};

/* Inventar-Datensatz (Karte t_1bab1fea): Gold mit Qualitäts-Spreizung
   (Q900 vs Q364) über zwei Standorte + Freitext-Position. */
const __inv = {
	nextId: 4,
	rows: [
		{ id: '1', entity_kind: 'commodity', entity_id: '1000', name: 'Gold', quality: 900, location_id: null, location_name: 'Heim-Hangar', quantity: 12, price: 4500, created_at: 0, updated_at: 0 },
		{ id: '2', entity_kind: 'commodity', entity_id: '1001', name: 'Gold', quality: 364, location_id: null, location_name: 'Meine Carrack', quantity: 3, price: null, created_at: 0, updated_at: 0 },
		{ id: '3', entity_kind: 'commodity', entity_id: '22', name: 'Titanium', quality: 720, location_id: null, location_name: 'Org-Depot', quantity: 40, price: 300, created_at: 0, updated_at: 0 }
	]
};
function invRows() { return __inv.rows.map((r) => ({ ...r })); }


window.addEventListener('error', function (e) { globalThis.__pageErr = String((e.error && e.error.stack) || e.message); });
window.addEventListener('unhandledrejection', function (e) { globalThis.__pageErr = 'PROMISE:' + String(e.reason && (e.reason.stack || e.reason)); });
