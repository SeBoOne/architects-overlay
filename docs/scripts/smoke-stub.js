/* Smoke-harness Tauri-stub: real Svelte palette JS logic, canned IPC data.
   Only for screenshots / DOM probes — real IPC search/math/market matching is
   covered by cargo tests (crates/uex-client apply_local u.a.).

   NB: 'get_marketplace' mirrors the PROVIDER contract in JS (q = jeder Term
   trifft mindestens ein Feld Name/Typ/Terminal/Verkäufer, operation-Regel,
   Sortierung, Limit) — der echte Pfad ist durch cargo tests belegt, dieser
   Mirror dient nur der Svelte-Logik-Verifikation (Live-Feedback, t_594c213c). */
window.isTauri = true;
window.__TAURI__ = {};
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
			case 'get_prices': {
				const name = String(a.commodity || '');
				const now = Math.floor(Date.now() / 1000);
				const price = (loc, tid, buy, sell, q, extra) => Object.assign({
					commodity: name, commodity_id: 1, location: loc, terminal_id: tid,
					sell_price: sell, buy_price: buy, sell_stock: 0, buy_stock: 0,
					sell_status: sell > 0 ? 1 : 0, buy_status: buy > 0 ? 1 : 0,
					quality: q, updated: now - 240
				}, extra || {});
				// QA-Auflage C: Kontrakt-treuer Mirror — Location → `location.groups`
				// (je Terminal eine Gruppe, leere Gruppe = "keine Preisdaten"),
				// Item → `prices: []` (nie ein Fehler), sonst Commodity wie bisher.
				if (a.kind === 'location') {
					const groups = [
						{
							terminal_id: 436, terminal_name: name,
							prices: [
								price('Checkmate — Admin Terminal', 436, 1108, 0, 680),
								price('Checkmate — Admin Terminal', 436, 0, 4812, 710)
							]
						},
						{
							terminal_id: 438, terminal_name: 'Commodity Console - Checkmate Station',
							prices: []
						}
					];
					const prices = [].concat(...groups.map((g) => g.prices));
					return {
						commodity: name, source: 'uex', fetched_at: now, prices,
						location: {
							location: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station',
							terminal: name, groups
						}
					};
				}
				if (a.kind === 'item') {
					return { commodity: name, source: 'uex', fetched_at: now, location: null, prices: [] };
				}
				return {
					commodity: name,
					source: 'uex',
					fetched_at: now,
					location: null,
					prices: [
						{ commodity: name, commodity_id: 1, location: 'ARC-L1 Wide Forest Station', terminal_id: 11, sell_price: 0, buy_price: 4812, sell_stock: 0, buy_stock: 1200, sell_status: 1, buy_status: 1, quality: 3, updated: now - 240 },
						{ commodity: name, commodity_id: 1, location: 'Area18 — TDD', terminal_id: 12, sell_price: 0, buy_price: 4930, sell_stock: 0, buy_stock: 900, sell_status: 1, buy_status: 1, quality: 3, updated: now - 300 },
						{ commodity: name, commodity_id: 1, location: 'Orison — Providence', terminal_id: 13, sell_price: 6305, buy_price: 0, sell_stock: 2200, buy_stock: 0, sell_status: 1, buy_status: 1, quality: 3, updated: now - 180 },
						{ commodity: name, commodity_id: 1, location: 'Levski — Grand Barter', terminal_id: 14, sell_price: 6118, buy_price: 0, sell_stock: 1800, buy_stock: 0, sell_status: 1, buy_status: 1, quality: 3, updated: now - 360 },
						{ commodity: name, commodity_id: 1, location: 'Lorville — Tammany & Sons', terminal_id: 15, sell_price: 5980, buy_price: 0, sell_stock: 1500, buy_stock: 0, sell_status: 1, buy_status: 1, quality: 3, updated: now - 540 }
					]
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
						{ id: 5001, name: 'Commodity Shop - Admin - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: null },
						{ id: 5002, name: 'Orison — Providence', kind: 'terminal', subtitle: 'Terminal · Kauf/Verkauf', code: 'ORS' }
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
				// Provider-Mirror: q = jeder Term trifft mindestens ein Feld
				// (Name/Typ/Terminal/Verkäufer/Beschreibung); operation-Regel;
				// Sortierung; dann Limit.
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
				out = out.slice(0, filter.limit || 100);
				globalThis.__lastMarket = { filters: filter, returned: out.length };
				return { source: 'cache', fetched_at: now, offers: out, note: null };
			}
			default:
				return null;
		}
	}
};
