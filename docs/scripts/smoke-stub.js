/* Smoke-harness Tauri-stub: real Svelte palette JS logic, canned IPC data.
   Only for screenshots — real IPC math/search is covered by cargo tests. */
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
						{ kind: 'commodity', label: 'Commodities', fresh: true, age_secs: 300, count: 120 },
						{ kind: 'item', label: 'Items', fresh: true, age_secs: 600, count: 340 },
						{ kind: 'location', label: 'Locations', fresh: true, age_secs: 400, count: 80 }
					]
				};
			case 'sync_now':
				return { provider: 'uex', syncing: true, online: true, last_error: null, datasets: [] };
			case 'get_prices': {
				const name = String(a.commodity || '');
				return {
					commodity: name,
					source: 'uex',
					fetched_at: Math.floor(Date.now() / 1000),
					prices: [
						{ commodity: name, commodity_id: 1, location: 'ARC-L1 Wide Forest Station', terminal_id: 11, sell_price: 0, buy_price: 4812, sell_stock: 0, buy_stock: 1200, sell_status: 1, buy_status: 1, quality: 3, updated: Math.floor(Date.now() / 1000) - 240 },
						{ commodity: name, commodity_id: 1, location: 'Area18 — TDD', terminal_id: 12, sell_price: 0, buy_price: 4930, sell_stock: 0, buy_stock: 900, sell_status: 1, buy_status: 1, quality: 3, updated: Math.floor(Date.now() / 1000) - 300 },
						{ commodity: name, commodity_id: 1, location: 'Orison — Providence', terminal_id: 13, sell_price: 6305, buy_price: 0, sell_stock: 2200, buy_stock: 0, sell_status: 1, buy_status: 1, quality: 3, updated: Math.floor(Date.now() / 1000) - 180 },
						{ commodity: name, commodity_id: 1, location: 'Levski — Grand Barter', terminal_id: 14, sell_price: 6118, buy_price: 0, sell_stock: 1800, buy_stock: 0, sell_status: 1, buy_status: 1, quality: 3, updated: Math.floor(Date.now() / 1000) - 360 },
						{ commodity: name, commodity_id: 1, location: 'Lorville — Tammany & Sons', terminal_id: 15, sell_price: 5980, buy_price: 0, sell_stock: 1500, buy_stock: 0, sell_status: 1, buy_status: 1, quality: 3, updated: Math.floor(Date.now() / 1000) - 540 }
					]
				};
			}
			case 'search': {
				const q = String(a.query || '').toLowerCase();
				const f = a.filters || null;
				const kinds = [];
				const groups = [];
				const pool = {
					commodity: [
						{ id: 3, name: 'Gold', kind: 'commodity', subtitle: 'Commodity · börsenfähig', code: 'AU' },
						{ id: 57, name: 'Bexalite (Raw)', kind: 'commodity', subtitle: 'Commodity · Raffinerie nötig', code: 'BEX' },
						{ id: 48, name: 'Quantanium (Raw)', kind: 'commodity', subtitle: 'Commodity · instabil', code: 'QNT' },
						{ id: 22, name: 'Titanium', kind: 'commodity', subtitle: 'Commodity · Metall', code: 'TIT' },
						{ id: 9, name: 'Laranite', kind: 'commodity', subtitle: 'Commodity · Kristalle', code: 'LRN' }
					],
					item: [
						{ id: 2914, name: 'Gold Beret', kind: 'item', subtitle: 'Item · Bekleidung', code: null },
						{ id: 3105, name: 'Goldschnalle — Helmvisier', kind: 'item', subtitle: 'Item · Rüstungsteil', code: null },
						{ id: 80, name: 'MedPen', kind: 'item', subtitle: 'Item · Verbrauchsgut', code: 'MED' }
					],
					location: [
						{ id: 5001, name: 'Area18 — TDD', kind: 'location', subtitle: 'Terminal · Kauf/Verkauf', code: 'A18' },
						{ id: 5002, name: 'Orison — Providence', kind: 'location', subtitle: 'Terminal · Kauf/Verkauf', code: 'ORS' }
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
				globalThis.__resp = res;
				return res;
			}
			case 'math_eval': {
				// JS mirror of the Rust evaluator for screenshots only.
				function ev(src) {
					function skip(p){while(p.i<p.c.length && (p.c[p.i]===' '||p.c[p.i]==='	'))p.i++;}
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
					const s = String(v2); const body=s.includes('.')?String(v2.toFixed(10)).replace(/0+$/,'').replace(/\.$/,''):s;
					return body;
				} catch (e) { throw Error(String(e.message||e)); }
			}
			case 'get_marketplace':
				return { source: 'cache', fetched_at: Math.floor(Date.now() / 1000), offers: [], note: null };
			default:
				return null;
		}
	}
};