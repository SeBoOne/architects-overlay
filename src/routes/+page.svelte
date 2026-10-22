<script lang="ts">
	// SC Architects Overlay — Suchpalette (Arkanis-UX-Rework, Phase t_1a0b0acb).
	//
	// Beide Tauri-Fenster rendern dieselbe Palette ("main" = Entwicklungs-Shell,
	// "spotlight" = Overlay über Star Citizen). Der Spotlight ist click-through-
	// fähig und zeigt den Spiel-/Fokus-Status; Esc blendet ihn aus.
	//
	// Bedienung orientiert an ArkanisOverlay:
	//   - Kategorie-Präfixe  :c :i :l  filtern je Term der Suchzeile
	//   - Inline-Rechner     =2*250+15%  → Ergebnis-Chip statt Ergebnisliste
	//   - Zeile anklicken/Enter → Inline-Expansion mit Preis-Tabelle (Kauf/Verkauf)
	//   - Alt+1..0 fokussiert den n-ten Treffer, erneut → zurück ins Suchfeld
	//   - Esc: Suchfeld-Fokus ⇒ Overlay ausblenden; sonst Fokus ins Suchfeld
	//
	// Zustände: idle · typing · loading · error · offline · market
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { invoke, isTauri } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';

	// ---- IPC-Typen (Mirror der Serde-DTOs in src-tauri/src/trade.rs) ---------
	type OverlayState = { visible: boolean; game_running: boolean; click_through: boolean };
	type SearchHit = { id: number; name: string; kind: string; subtitle: string; code: string | null };
	type GroupHits = { kind: string; label: string; hits: SearchHit[] };
	type SearchResponse = { groups: GroupHits[]; total: number };
	type PricePoint = {
		commodity: string;
		commodity_id: number;
		location: string;
		terminal_id: number;
		sell_price: number;
		buy_price: number;
		sell_stock: number;
		buy_stock: number;
		sell_status: number;
		buy_status: number;
		quality: number;
		updated: number;
	};
	type PriceDetail = { commodity: string; source: string; fetched_at: number; prices: PricePoint[] };
	type CalcResult = { value: string };
	type DatasetStatus = { kind: string; label: string; fresh: boolean; age_secs: number | null; count: number };
	type SyncStatus = {
		provider: string;
		syncing: boolean;
		online: boolean;
		last_error: string | null;
		datasets: DatasetStatus[];
	};

	// Mirrors provider::MarketOperation / ListingType / OfferSort / Offer.
	type MarketOp = 'buy' | 'sell';
	type ListingType = 'item' | 'service' | 'contract';
	type OfferSort = 'newest' | 'price_asc' | 'price_desc';
	type Offer = {
		id: number;
		title: string;
		description: string;
		item_id: number;
		category_id: number;
		operation: MarketOp;
		listing_type: ListingType;
		price: number;
		currency: string;
		unit: string;
		quantity: number;
		location: string;
		region: string;
		seller: string;
		quality: number;
		updated: number;
		expires: number | null;
		slug: string;
	};
	type MarketResponse = {
		source: 'cache' | 'live' | 'offline';
		fetched_at: number;
		offers: Offer[];
		note: string | null;
	};

	const OVERLAY_STATE_EVENT = 'overlay-state-changed';
	const SYNC_EVENT = 'trade-sync';

	type Phase = 'idle' | 'typing' | 'loading' | 'error' | 'offline' | 'market';

	// ---- Zustand -------------------------------------------------------------
	let isSpotlight = $state(false);
	let overlay = $state<OverlayState | null>(null);
	let phase = $state<Phase>('loading');
	let input = $state('');
	// Vom Präfix-Parser abgeleitete Kategorien (aus `:c :i :l`-Termen).
	let filters = $state<string[]>([]);
	let groups = $state<GroupHits[]>([]);
	let sel = $state(0);
	let status = $state<SyncStatus | null>(null);
	let errorMsg = $state<string | null>(null);

	// Rechner-Modus (`=…`) + Found-Chip.
	let calcResult = $state<CalcResult | null>(null);
	let calcError = $state<string | null>(null);
	let searchMs = $state(0);
	let searchTotal = $state(0);

	// Inline-Expansion: nur EINE Zeile ist gleichzeitig aufgeklappt, also reicht
	// ein reaktives Skalar (Map-Mutationen würden Svelte 5 NICHT neu rendern —
	// die X-Pan würde ewig „Lade Preise …" zeigen; ein reaktiver Vergleich
	// `expanded === idx` plus dieses Skalar triggert jedes Mal).
	type RowDetail = { busy: boolean; detail: PriceDetail | null; error: string | null };
	let xpan = $state<RowDetail | null>(null);
	// Stale-Response-Guard: eine spät eintreffende get_prices-Antwort darf eine
	// neuer geöffnete Zeile (oder eine geleerte Liste) nicht überschreiben.
	let xpanToken = 0;
	// `expanded` = welche Zeile auf-/zugeklappt ist; `focusIdx` = per Alt+Zahl
	// fokussierte Zeile (Badge ausgegraut).
	let expanded = $state<number | null>(null);
	let focusIdx = $state<number | null>(null);
	let searchEl = $state<HTMLInputElement | null>(null);

	// ---- Market-State (Phase 2) --------------------------------------------
	let offers = $state<Offer[]>([]);
	let mLoading = $state(false);
	let mSource = $state<'cache' | 'live' | 'offline'>('live');
	let mNote = $state<string | null>(null);
	let mSel = $state(0);
	let offerDetail = $state<Offer | null>(null);
	let mSort = $state<OfferSort>('newest');
	let mOp = $state<'all' | MarketOp>('all');
	let mRegion = $state('all');
	let mQuery = $state('');
	let marketRegions = $state<string[]>([]);
	let mDebounce: ReturnType<typeof setTimeout> | null = null;

	// Flaches Navigationsmodell: alle Zeilen in Reihenfolge + Offsets je Gruppe.
	let flat = $state<SearchHit[]>([]);
	let groupOffsets = $state<number[]>([]);
	let debounce: ReturnType<typeof setTimeout> | null = null;

	function hasData(s?: SyncStatus | null): boolean {
		return (s ?? status)?.datasets.some((d) => d.count > 0) ?? false;
	}

	// ---- Initialisierung -----------------------------------------------------
	async function boot() {
		// Nur in der Tauri-Webview booten: ohne Tauri (SSR, plain-vite-dev)
		// rejected jedes invoke — früher abbrechen.
		if (!isTauri()) {
			return;
		}
		try {
			const label = getCurrentWindow().label;
			if (typeof document !== 'undefined') document.documentElement.dataset.window = label;
			if (label === 'spotlight') {
				isSpotlight = true;
				overlay = await invoke<OverlayState>('overlay_get_state');
				await listen<OverlayState>(OVERLAY_STATE_EVENT, (e) => {
					overlay = e.payload;
				});
				// Arkanis: sobald das Overlay-Fenster Fokus bekommt, springt der
				// Cursor ins Suchfeld (window-focus handler).
				if (typeof window !== 'undefined') {
					window.addEventListener('focus', onWindowFocus);
				}
			}
		} catch {
			// Ohne Tauri-Webview (plain vite dev) → unkritisch, Palette läuft.
		}

		try {
			status = await invoke<SyncStatus>('sync_status');
		} catch (e) {
			status = { provider: 'uex', syncing: false, online: false, last_error: String(e), datasets: [] };
		}

		try {
			await listen<SyncStatus>(SYNC_EVENT, (e) => onSyncEvent(e.payload));
			if (!hasData()) {
				// Erster Start: lokalen Cache füllen (einmaliger Sync-Spinner).
				phase = status && status.online ? 'loading' : 'error';
				await invoke('sync_now');
			} else {
				phase = status && status.online ? 'idle' : 'offline';
			}
		} catch (e) {
			status = { provider: 'uex', syncing: false, online: false, last_error: String(e), datasets: [] };
			phase = 'error';
		}
	}
	void boot();

	function onWindowFocus() {
		if (isSpotlight) focusSearch(true);
	}

	function onSyncEvent(s: SyncStatus) {
		status = s;
		if (phase === 'typing') return; // laufende Aktion nicht stören
		if (!hasData(s)) {
			phase = s.last_error ? 'error' : s.syncing ? 'loading' : phase;
		} else {
			phase = s.online ? (input ? 'typing' : 'idle') : 'offline';
		}
	}

	// ---- Fokus ins Suchfeld (Select-All) --------------------------------------
	function focusSearch(selectAll: boolean) {
		if (searchEl) {
			searchEl.focus();
			if (selectAll) {
				searchEl.select();
			}
		}
	}

	// ---- Suchbegriff → (freie Terme, Kategorie-Filter, Rechner) ---------------
	// Jeder Leerzeichen-getrennte Term einzeln: `:c/:i/:l` → Kategorie-Filter,
	// `=…` (erster Term) → Rechnermodus, sonst Fuzzy-Suchtext. Freie Terme werden
	// mit Leerzeichen wieder zu einem Suchtext verbunden.
	function parseQuery(): { text: string; cats: string[]; isCalc: boolean } {
		const raw = input.trim();
		if (!raw) return { text: '', cats: [], isCalc: false };
		if (raw.startsWith('=')) {
			return { text: raw.slice(1).trim(), cats: [], isCalc: true };
		}
		const cats: string[] = [];
		const terms: string[] = [];
		for (const tok of raw.split(/\s+/)) {
			if (tok === ':c') cats.push('commodity');
			else if (tok === ':i') cats.push('item');
			else if (tok === ':l') cats.push('location');
			else if (tok.startsWith(':')) terms.push(tok); // unbekanntes Präfix → Fuzzy-Term (Spec §2)
			else terms.push(tok);
		}
		return { text: terms.join(' '), cats: [...new Set(cats)], isCalc: false };
	}

	// ---- Suche (debounced 180 ms, cache-only) ---------------------------------
	function onInput() {
		if (debounce) clearTimeout(debounce);
		debounce = setTimeout(() => void runSearch(), 180);
	}

	async function runSearch() {
		const q = parseQuery();
		calcResult = null;
		calcError = null;
		expanded = null;
		focusIdx = null;
		xpan = null;
		xpanToken++;

		if (q.isCalc) {
			// Inline-Rechner: Ergebnis-Chip anstelle der Ergebnisliste.
			if (!q.text) {
				groups = [];
				flat = [];
				groupOffsets = [];
				sel = 0;
				phase = 'idle';
				return;
			}
			const t0 = performance.now();
			try {
				const v = await invoke<string>('math_eval', { expr: q.text });
				calcResult = { value: v };
				calcError = null;
				searchMs = Math.max(1, Math.round(performance.now() - t0));
				searchTotal = 1;
				groups = [];
				flat = [];
				groupOffsets = [];
				phase = 'typing';
			} catch (e) {
				calcError = String(e);
				calcResult = null;
				phase = 'typing';
			}
			return;
		}

		if (!q.text && !q.cats.length) {
			groups = [];
			flat = [];
			groupOffsets = [];
			sel = 0;
			phase = 'idle';
			return;
		}
		filters = q.cats;
		// Präfix-only (z. B. `:c`) hat keinen Freitext → dann alle Treffer der
		// Kategorie listen (Suchtext leer = Breitensuche).
		const query = q.text;
		const t0 = performance.now();
		try {
			const resp = await invoke<SearchResponse>('search', {
				query,
				filters: filters.length ? filters : null,
			});
			searchMs = Math.max(1, Math.round(performance.now() - t0));
			searchTotal = resp.total;
			groups = resp.groups;
			rebuildFromGroups();
			sel = 0;
			phase = 'typing';
		} catch (e) {
			errorMsg = String(e);
			phase = 'error';
		}
	}

	function rebuildFromGroups() {
		const out: SearchHit[] = [];
		const offs: number[] = [];
		let acc = 0;
		for (const g of groups) {
			// Bei Kategorie-Filter nur die passende Gruppe zeigen.
			if (filters.length && !filters.includes(g.kind)) continue;
			offs.push(acc);
			out.push(...g.hits);
			acc += g.hits.length;
		}
		groupOffsets = offs;
		flat = out;
	}

	function groupOffset(gi: number): number {
		return groupOffsets[gi] ?? 0;
	}

	// ---- Navigation ----------------------------------------------------------
	function move(dir: number) {
		if (!flat.length) return;
		sel = Math.min(Math.max(sel + dir, 0), flat.length - 1);
		// Pfeil-Navigation übernimmt die Fokus-Zeile — sonst bliebe ein per
		// Alt+Zahl gesetzter Fokus als Enter-Ziel stehen (NB-2).
		focusIdx = sel;
	}

	// ---- Inline-Expansion (Preis-Tabelle) statt separates Detail-Panel --------
	function openRow(idx: number) {
		if (!flat[idx]) return;
		// Taggleiche Zeile → zuklappen.
		if (expanded === idx) {
			clampIdx();
			return;
		}
		sel = idx;
		expanded = idx;
		// Preise lazy laden — reaktiv in `xpan` (Svelte-5-sicher).
		xpan = { busy: true, detail: null, error: null };
		void loadDetail(flat[idx]);
		focusRow(idx);
	}

	// Alt+Zahl nur fokussieren: Badge ausgegraut, aber NICHT aufklappen.
	// (NB-1: Verhalten entspricht jetzt dem Kommentar/Spec §4 — das Aufklappen
	// übernimmt Enter auf der fokussierten Zeile.)
	function focusOnly(idx: number) {
		focusRow(idx);
	}

	// Echten DOM-Fokus auf die Ergebniszeile legen (Spec §4 „Alt+Zahl fokussiert
	// das Ergebnis") — der Fokus-Zustand darf nicht nur CSS sein. `.row` ist über
	// `tabindex="-1"` fokussierbar, aber nicht im Tab-Order (kein Formular-Tab).
	function focusRow(idx: number) {
		document.querySelectorAll<HTMLElement>('.row')[idx]?.focus();
	}

	function clampIdx() {
		expanded = null;
		focusIdx = null;
		xpan = null;
		xpanToken++;
	}

	async function loadDetail(h: SearchHit) {
		const myToken = ++xpanToken;
		try {
			const d = await invoke<PriceDetail>('get_prices', { commodity: h.name });
			if (myToken === xpanToken) xpan = { busy: false, detail: d, error: null };
		} catch (e) {
			if (myToken === xpanToken) xpan = { busy: false, detail: null, error: String(e) };
		}
	}

	// ---- Alt+Zahl: Treffer fokussieren / zurück ins Suchfeld -----------------
	function onAltNumber(digit: string) {
		if (!flat.length) return;
		const n = digit === '0' ? 9 : Number(digit) - 1;
		if (n >= flat.length) return;
		if (focusIdx === n) {
			// erneut → Fokus zurück ins Suchfeld (Select-All).
			focusIdx = null;
			expanded = null;
			xpan = null;
			focusSearch(true);
			return;
		}
		focusIdx = n;
		focusOnly(n);
	}

	function badgeOf(idx: number): string {
		if (idx >= 10) return '';
		return idx < 9 ? String(idx + 1) : '0';
	}

	// ---- Esc-Toggle (Kernfeature) --------------------------------------------
	// Esc-Zustände (Spec §3/§4): 1) Ergebnis fokussiert (Alt+Zahl) oder eine
	// Expansion offen → erst zuklappen + Fokus zurück ins Suchfeld; der nächste
	// Esc blendet aus. 2) Suchfeld fokussiert → Overlay ausblenden (hide) bzw.
	// leeren. 3) Fokus woanders → Fokus zurück ins Suchfeld (Select-All).
	// Nie ausblenden, wenn der Fokus woanders liegt (Arkanis-Verhalten).
	function escSearch() {
		if (focusIdx !== null || expanded !== null) {
			clampIdx();
			focusSearch(true);
			return;
		}
		const inSearch = searchEl ? document.activeElement === searchEl : phase !== 'typing';
		if (inSearch) {
			// Suchfeld fokussiert → Overlay ausblenden (Spotlight) bzw. leeren.
			if (isSpotlight) {
				void invoke<boolean>('overlay_set_visible', { visible: false });
			} else {
				input = '';
				groups = [];
				flat = [];
				groupOffsets = [];
				expanded = null;
				focusIdx = null;
				xpan = null;
				calcResult = null;
				calcError = null;
				phase = 'idle';
			}
		} else {
			// Fokus woanders (Ergebnis/Expansion/…) → Suchfeld (Select-All).
			focusSearch(true);
		}
	}

	function toggleMarket() {
		phase = phase === 'market' ? 'typing' : 'market';
		if (phase === 'market') void loadMarket();
		else {
			offerDetail = null;
			mSel = 0;
		}
	}

	// Load / refresh the marketplace via the provider (cache-first with short
	// TTL; live when a filter forces it; offline snapshot on failure).
	async function loadMarket() {
		mLoading = true;
		mNote = null;
		const filterObj = {
			location: '',
			q: mQuery.trim(),
			item_id: null,
			operation: mOp === 'all' ? null : mOp,
			region: mRegion === 'all' ? '' : mRegion,
			sort: mSort,
			limit: 100
		};
		try {
			const resp = await invoke<MarketResponse>('get_marketplace', { filters: filterObj });
			offers = resp.offers;
			mSource = resp.source;
			mNote = resp.note;
			mSel = Math.min(mSel, Math.max(offers.length - 1, 0));
			marketRegions = [...new Set(resp.offers.map((o) => o.region).filter(Boolean))].sort();
		} catch (e) {
			offers = [];
			mNote = String(e);
			mSource = 'offline';
		}
		mLoading = false;
	}

	function onMarketQuery() {
		if (mDebounce) clearTimeout(mDebounce);
		mDebounce = setTimeout(() => void loadMarket(), 200);
	}

	function openOfferDetail() {
		if (!offers.length) return;
		offerDetail = offers[mSel] ?? null;
	}

	async function openOfferDeepLink() {
		if (!offerDetail) return;
		try {
			await invoke<string>('open_marketplace_listing', { slug: offerDetail.slug });
		} catch (e) {
			errorMsg = String(e);
		}
	}

	function setSort(s: OfferSort) {
		mSort = s;
		void loadMarket();
	}

	function setOp(o: 'all' | MarketOp) {
		mOp = o;
		void loadMarket();
	}

	function setRegion(r: string) {
		mRegion = r;
		void loadMarket();
	}

	function fmtPrice(p: number): string {
		if (!p) return '—';
		return new Intl.NumberFormat('de-DE', { maximumFractionDigits: 0 }).format(p);
	}

	function fmtQty(n: number): string {
		return new Intl.NumberFormat('de-DE', { maximumFractionDigits: 0 }).format(n);
	}

	async function toggleClickThrough() {
		if (!overlay) return;
		const ignore = !overlay.click_through;
		await invoke<boolean>('overlay_set_click_through', { ignore });
		overlay = await invoke<OverlayState>('overlay_get_state');
	}

	async function retrySync() {
		errorMsg = null;
		phase = 'loading';
		await invoke('sync_now');
	}

	// 'm' und 'o' sind druckbare Buchstaben — die Shortcuts dürfen sie NICHT
	// schlucken, während der User in einem Eingabefeld tippt.
	function isFieldFocused(e: KeyboardEvent): boolean {
		const t = e.target;
		if (t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement || t instanceof HTMLSelectElement) {
			return true;
		}
		return (
			typeof (t as HTMLElement | null)?.isContentEditable === 'boolean' &&
			(t as HTMLElement).isContentEditable
		);
	}

	function onKey(e: KeyboardEvent) {
		const k = e.key;
		// Market phase: offer list navigation + detail.
		if (phase === 'market') {
			if (k === 'ArrowDown' && !offerDetail) {
				mSel = Math.min(offers.length - 1, mSel + 1);
				e.preventDefault();
				return;
			}
			if (k === 'ArrowUp' && !offerDetail) {
				mSel = Math.max(0, mSel - 1);
				e.preventDefault();
				return;
			}
			if (k === 'ArrowRight' || k === 'Tab') {
				if (offerDetail) return;
				openOfferDetail();
				e.preventDefault();
				return;
			}
			if (k === 'ArrowLeft' && offerDetail) {
				offerDetail = null;
				e.preventDefault();
				return;
			}
			if (k === 'Enter' && offerDetail) {
				void openOfferDeepLink();
				e.preventDefault();
				return;
			}
			if (k === 'Enter' && !offerDetail) {
				openOfferDetail();
				e.preventDefault();
				return;
			}
			if (k === 'Escape') {
				if (offerDetail) {
					offerDetail = null;
				} else {
					toggleMarket();
				}
				e.preventDefault();
				return;
			}
			return;
		}

		// Alt+Zahl: Treffer direkt fokussieren (Badges 1..0).
		if (e.altKey && /^[0-9]$/.test(k)) {
			onAltNumber(k);
			e.preventDefault();
			return;
		}

		if (k === 'ArrowDown') {
			move(1);
			e.preventDefault();
		} else if (k === 'ArrowUp') {
			move(-1);
			e.preventDefault();
		} else if (k === 'Enter') {
			// NB-2: Enter trifft die fokussierte Zeile (Alt+Zahl), sonst die
			// per Pfeil selektierte.
			openRow(focusIdx ?? sel);
			e.preventDefault();
		} else if (k === 'Escape') {
			escSearch();
			e.preventDefault();
		} else if (k === 'm' || k === 'M') {
			if (isFieldFocused(e) && !e.ctrlKey && !e.metaKey) {
				return;
			}
			toggleMarket();
			e.preventDefault();
		} else if (k === 'Tab') {
			// Inline-Expansion ist der Hauptpfad; Tab navigiert weiterhin durch
			// das Formular nativer — kein Filter-Cycle mehr (Arkanis nutzt Präfixe).
			return;
		}
	}

	// ---- Formatierung (Deutsch, Mono — Spalten bleiben stehen) ---------------
	function fmt(n: number, d = 2): string {
		return n.toLocaleString('de-DE', { maximumFractionDigits: d, minimumFractionDigits: 0 });
	}

	function nowSec(): number {
		return Math.floor(Date.now() / 1000);
	}

	function rel(ts: number): string {
		if (!ts) return '—';
		const s = Math.max(0, nowSec() - ts);
		if (s < 60) return `vor ${Math.max(1, s)} s`;
		if (s < 3600) return `vor ${Math.floor(s / 60)} min`;
		if (s < 86400) return `vor ${Math.floor(s / 3600)} h`;
		return `vor ${Math.floor(s / 86400)} T`;
	}

	function cacheAge(s?: SyncStatus | null): string {
		const ds = (s ?? status)?.datasets ?? [];
		const ages = ds.map((d) => d.age_secs).filter((a) => a != null);
		if (!ages.length) return '—';
		return rel(nowSec() - Math.max(...(ages as number[])));
	}

	function syncLabel(): { text: string; cls: string } {
		const s = status;
		if (!s) return { text: 'SYNC …', cls: 'load' };
		if (s.syncing) return { text: 'SYNC LÄUFT', cls: 'load' };
		if (!hasData(s)) return s.last_error ? { text: 'SYNC FEHLER', cls: 'alert' } : { text: 'SYNC LÄUFT', cls: 'load' };
		if (!s.online) return { text: `OFFLINE · CACHE ${cacheAge(s)}`, cls: 'off' };
		return { text: `SYNC ${cacheAge(s)} · ${s.provider.toUpperCase()}`, cls: '' };
	}

	// ---- Preis-Tabellen für die Inline-Expansion (Kauf | Verkauf) ------------
	// Arkanis GameEntitySearchDetails/PriceTagTable: zwei Spalten nebeneinander.
	function buyPoints(d: PriceDetail): PricePoint[] {
		return (d.prices ?? [])
			.filter((p) => p.buy_price > 0)
			.sort((a, b) => b.buy_price - a.buy_price);
	}

	function sellPoints(d: PriceDetail): PricePoint[] {
		return (d.prices ?? [])
			.filter((p) => p.sell_price > 0)
			.sort((a, b) => b.sell_price - a.sell_price);
	}

	function fmtStock(p: PricePoint): string {
		return fmtQty(Math.max(p.sell_stock, p.buy_stock));
	}

	// Beste Preise für die Inline-Meta-Felder (der Expansion entnommen).
	function bestBuy(d: PriceDetail | null): number {
		const xs = (d?.prices ?? []).map((p) => p.buy_price).filter((v) => v > 0);
		return xs.length ? Math.max(...xs) : 0;
	}

	function bestSell(d: PriceDetail | null): number {
		const xs = (d?.prices ?? []).map((p) => p.sell_price).filter((v) => v > 0);
		return xs.length ? Math.max(...xs) : 0;
	}

	const ICONS: Record<string, string> = { commodity: 'C', item: 'I', location: 'L' };
</script>

<svelte:window onkeydown={onKey} />

<main class="pal-root">
	<div class="shell">
		<i class="deco t1"></i><i class="deco t2"></i>

		<!-- Suchzeile -->
		<div class="srow">
			<span class="ico">
				<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round">
					<circle cx="10.5" cy="10.5" r="6.5" />
					<path d="M15.4 15.4 21 21" />
				</svg>
			</span>
			<input
				class="q"
				type="text"
				autofocus
				bind:this={searchEl}
				bind:value={input}
				oninput={onInput}
				placeholder="Commodity, Item oder Terminal suchen …"
				spellcheck="false"
				autocomplete="off"
			/>
			<span class="smode">{phase === 'market' ? 'MARKETPLACE' : 'SUCHE'}</span>
			<span class="esc">ESC</span>
		</div>

		<!-- Helper-Zeile: Shortcuts (Arkanis - Overline/Typo secondary) -->
		<div class="helper">
			<span><kbd>:c</kbd> commodities</span>
			<span class="sep">·</span>
			<span><kbd>:i</kbd> items</span>
			<span class="sep">·</span>
			<span><kbd>:l</kbd> locations</span>
			<span class="sep">·</span>
			<span><kbd>=</kbd> math</span>
			<span class="sep">·</span>
			<span><kbd>%</kbd> ÷100</span>
		</div>

		<!-- Inhalt je Zustand -->
		{#if phase === 'loading'}
			<div class="pane-loading" role="status">
				<div class="center">
					<span class="spin"></span>
					<div class="big" style="margin-top: 12px">Erster Sync läuft</div>
					<div class="sm">Commodities · Terminals · Preise — danach wird sofort lokal gesucht.</div>
				</div>
			</div>

		{:else if phase === 'error'}
			<div class="pane-error">
				<div class="center">
					<div class="big" style="color: var(--alert)">UEX nicht erreichbar</div>
					<div class="sm">
						{errorMsg ?? status?.last_error ?? 'Keine Verbindung'}<br />
						Cache-Stand: {cacheAge()}
					</div>
					<span class="btn" onclick={retrySync}>Erneut versuchen</span>
					<div class="sm" style="margin-top: 10px">
						<b style="color: var(--fgdim)">O</b> Offline mit Cache fortsetzen
					</div>
				</div>
			</div>

		{:else if phase === 'market'}
			<div class="pane-market">
				{#if offerDetail}
					<div class="mdetail">
						<div class="md-head">
							<div class="md-t">
								<h2>{offerDetail.title}</h2>
								<span class="tag" style="color: var(--fgdim)">
									{offerDetail.operation === 'sell' ? 'WTS' : 'WTB'} · {offerDetail.listing_type.toUpperCase()}{offerDetail.region ? ' · ' + offerDetail.region : ''}
								</span>
							</div>
							<div class="md-price">
								<div class="v {offerDetail.operation === 'sell' ? 'sell' : 'buy'}">{fmtPrice(offerDetail.price)}</div>
								<div class="u">{offerDetail.currency}{offerDetail.unit ? ' / ' + offerDetail.unit : ''}</div>
							</div>
						</div>
						<div class="md-meta">
							<span><b>Anbieter</b> {offerDetail.seller || '—'}</span>
							<span><b>Menge</b> {fmtQty(offerDetail.quantity)}</span>
							<span><b>Ort</b> {offerDetail.location || '—'}</span>
							<span><b>Alter</b> {rel(offerDetail.updated)}</span>
							{#if offerDetail.expires}
								<span><b>Ende</b> {rel(offerDetail.expires)}</span>
							{/if}
							{#if offerDetail.quality}
								<span><b>Qualität</b> {offerDetail.quality}</span>
							{/if}
						</div>
						{#if offerDetail.description}
							<div class="md-desc">{offerDetail.description}</div>
						{:else}
							<div class="md-desc empty">Keine Beschreibung.</div>
						{/if}
						<div class="md-actions">
							<span class="btn" onclick={openOfferDeepLink}>AUF UEXCORP.SPACE ÖFFNEN ↗</span>
							<span class="btn-ghost" onclick={() => (offerDetail = null)}>Zurück</span>
						</div>
						<div class="md-note">
							Öffnet <span class="src">uexcorp.space/marketplace/item/info/{offerDetail.slug}</span> im Browser — nur Lesen, keine Änderungen.
						</div>
					</div>
				{:else if mLoading && !offers.length}
					<div class="center">
						<span class="spin"></span>
						<div class="big" style="margin-top: 12px">Lade Marketplace …</div>
					</div>
				{:else if mNote && !offers.length}
					<div class="pane-error">
						<div class="center">
							<div class="big" style="color: var(--alert)">Marketplace nicht erreichbar</div>
							<div class="sm">{mNote}</div>
							<span class="btn" onclick={() => loadMarket()}>Erneut versuchen</span>
						</div>
					</div>
				{:else}
					<div class="mbar">
						<span
							class="chip"
							class:is-on={mOp === 'all'}
							onclick={() => setOp('all')}>Alle</span>
						<span
							class="chip"
							class:is-on={mOp === 'sell'}
							onclick={() => setOp('sell')}><span class="dot"></span>Verkauf</span>
						<span
							class="chip"
							class:is-on={mOp === 'buy'}
							onclick={() => setOp('buy')}><span class="dot"></span>Kauf</span>
						<span class="m-spacer"></span>
						<span class="m-lbl">Region</span>
						<span
							class="chip"
							class:is-on={mRegion === 'all'}
							onclick={() => setRegion('all')}>Alle</span>
						{#each marketRegions as r}
							<span class="chip" class:is-on={mRegion === r} onclick={() => setRegion(r)}>{r}</span>
						{/each}
					</div>
					<div class="mbar2">
						<input
							class="mq"
							type="text"
							bind:value={mQuery}
							oninput={onMarketQuery}
							placeholder="Kommodity / Item / Anbieter filtern …"
							spellcheck="false"
							autocomplete="off"
						/>
						<span class="m-lbl">Sort</span>
						<span class="chip" class:is-on={mSort === 'price_asc'} onclick={() => setSort('price_asc')}>Preis ↑</span>
						<span class="chip" class:is-on={mSort === 'price_desc'} onclick={() => setSort('price_desc')}>Preis ↓</span>
						<span class="chip" class:is-on={mSort === 'newest'} onclick={() => setSort('newest')}>Neueste</span>
						<span class="m-spacer"></span>
						{#if mSource === 'offline'}
							<span class="chip warn"><span class="dot"></span>Offline-Snapshot</span>
						{:else if mSource === 'cache'}
							<span class="chip"><span class="dot"></span>Cache</span>
						{/if}
					</div>

					{#if offers.length}
						<div class="mlist">
							{#each offers as o, i}
								<div
									class="mrow"
									role="option"
									aria-selected={i === mSel}
									class:is-sel={i === mSel}
									onclick={() => {
										mSel = i;
										openOfferDetail();
									}}
								>
									<div class="m-op {o.operation}">{o.operation === 'sell' ? 'WTS' : 'WTB'}</div>
									<div class="m-main">
										<div class="m-title">{o.title}</div>
										<div class="m-sub">
											{o.location || '—'}{o.region ? ' · ' + o.region : ''} · {o.seller}
										</div>
									</div>
									<div class="m-right">
										<div class="m-price {o.operation === 'sell' ? 'sell' : 'buy'}">{fmtPrice(o.price)}</div>
										<div class="m-meta">{fmtQty(o.quantity)} Einheiten · {rel(o.updated)}</div>
									</div>
								</div>
							{/each}
						</div>
						<div class="m-foot">
							<span>{offers.length} Angebote</span>
							<span class="src">UEX Corp</span>
						</div>
					{:else}
						<div class="row-empty" style="padding: 28px 16px">Keine Angebote für diese Filter.</div>
					{/if}
				{/if}
			</div>

		{:else if phase === 'typing' || phase === 'offline' || phase === 'idle'}
			<div class="pane-list">
				{#if status?.online === false}
					<div class="offlineband">
						<span class="dot"></span>Offline · Cache von {cacheAge()} · Preise ggf. veraltet
					</div>
				{/if}

				{#if calcResult}
					<!-- Inline-Rechner-Ergebnis statt Ergebnisliste -->
					<div class="found">
						<span class="c">= {calcResult.value}</span>
					</div>
				{:else if calcError}
					<div class="found">
						<span class="c" style="color: var(--alert)">{calcError}</span>
					</div>
				{:else if (input as string).trim() && flat.length}
					<!-- Found-Chip über der Liste -->
					<div class="found">
						<span class="c">Found <b>{searchTotal}</b> results in <b>{searchMs}</b> ms</span>
					</div>

					<div class="list">
						{#each groups as g, gi}
							{@const show = filters.length ? filters.includes(g.kind) : true}
							{#if show && g.hits.length}
								<div class="vgroup">{g.label} <span class="n">{g.hits.length}</span></div>
								{#each g.hits as h, hi}
									{@const idx = groupOffset(gi) + hi}
									<div class="rwrap" class:is-open={expanded === idx}>
										<div
											class="row"
											tabindex="-1"
											class:is-sel={idx === sel}
											class:is-focus={focusIdx === idx}
											role="option"
											aria-selected={idx === sel}
											onclick={() => {
												sel = idx;
												if (expanded === idx) {
													clampIdx();
												} else {
													openRow(idx);
												}
											}}
										>
											<span class="rbadge">{badgeOf(idx)}</span>
											<span class="ic">{ICONS[g.kind] ?? '?'}</span>
											<div>
												<div class="name">{h.name}</div>
												<div class="sub">{h.subtitle}</div>
											</div>
											<div class="meta">
												<div class="mf"><span class="k">UEX-ID</span><span class="v">{h.code ?? h.id}</span></div>
												{#if expanded === idx && xpan?.detail}
													<div class="mf">
														<span class="k">Kauf ab</span>
														<span class="v buy">{fmt(bestBuy(xpan.detail))} aUEC</span>
													</div>
													<div class="mf">
														<span class="k">Verkauf bis</span>
														<span class="v sell">{fmt(bestSell(xpan.detail))} aUEC</span>
													</div>
												{/if}
											</div>
											<div class="ctl">
												{#if expanded === idx}
													<span class="btn-x primary">{xpan?.busy ? '…' : '▼ PREISE'}</span>
												{:else}
													<span class="btn-x">↵ Preise</span>
												{/if}
											</div>
										</div>

										{#if expanded === idx && xpan}
											<div class="xpan is-open">
												<div class="inner">
													{#if xpan.busy && !xpan.detail}
														<div class="ptab">
															<div class="cap">Lade Preise …</div>
														</div>
													{:else if xpan.error}
														<div class="ptab">
															<div class="cap">Fehler</div>
															<div class="ph"><span>Preise nicht geladen</span></div>
														</div>
													{:else if xpan.detail}
														{@const buys = buyPoints(xpan.detail)}
														{@const sells = sellPoints(xpan.detail)}
														<div class="ptab">
															<div class="cap">Alle verfügbaren <b class="buy">Kauf</b>-Preise</div>
															<div class="ph"><span>Terminal</span><span>Preis</span><span>SCU</span></div>
															{#if buys.length}
																{#each buys as p, bi}
																	<div class="pr" class:best={bi === 0}>
																		<span class="loc">{p.location}</span>
																		<span class="val buy">{fmt(p.buy_price)}</span><span class="unit">aUEC</span>
																	</div>
																{/each}
															{:else}
																<div class="pr"><span class="loc" style="color: var(--fgfaint)">Kein Terminal kauft</span></div>
															{/if}
														</div>
														<div class="ptab">
															<div class="cap">Alle verfügbaren <b class="sell">Verkauf</b>-Preise</div>
															<div class="ph"><span>Terminal</span><span>Preis</span><span>SCU</span></div>
															{#if sells.length}
																{#each sells as p, si}
																	<div class="pr" class:best={si === 0}>
																		<span class="loc">{p.location}</span>
																		<span class="val sell">{fmt(p.sell_price)}</span><span class="unit">aUEC</span>
																	</div>
																{/each}
															{:else}
																<div class="pr"><span class="loc" style="color: var(--fgfaint)">Kein Terminal verkauft</span></div>
															{/if}
														</div>
													{:else}
														<div class="ptab">
															<div class="cap">Lade Preise …</div>
														</div>
													{/if}
												</div>
											</div>
										{/if}
									</div>
								{/each}
							{:else if show && !g.hits.length}
								<div class="vgroup">{g.label} <span class="n">0</span></div>
								<div class="row-empty">Keine Treffer</div>
							{/if}
						{/each}
					</div>
				{:else}
					<!-- idle -->
					<div class="pane-idle">
						<div class="sect"><div class="h">Zuletzt gesucht</div></div>
						<div class="recent"><span class="r">Keine — tippe, um zu suchen</span></div>
						<div class="sect"><div class="h">Schnellzugriff</div></div>
						<div class="recent">
							<span class="r"><b>Marketplace</b> · Ctrl+M</span>
							<span class="r"><b>Rechner</b> · =2*250+15%</span>
							<span class="r"><b>Kategorie</b> · :c gold</span>
						</div>
					</div>
				{/if}
			</div>
		{/if}

		<!-- Fußzeile (bleibt in allen Zuständen stehen → kein Layoutsprung) -->
		<div class="foot">
			<div class="hints">
				{#if phase === 'market'}
					<span><kbd>↑↓</kbd> Angebote</span>
					<span><kbd>↹</kbd> Detail</span>
					{#if offerDetail}
						<span><kbd>↵</kbd> im Browser öffnen</span>
					{/if}
					<span><kbd>Esc</kbd> zurück</span>
				{:else}
					<span><kbd>↑↓</kbd> Nav</span>
					<span><kbd>Alt+1..0</kbd> Treffer</span>
					<span><kbd>↵</kbd> aufklappen</span>
					<span><kbd>Esc</kbd> {isSpotlight ? 'ausblenden' : 'leeren'}</span>
					<span><kbd>Ctrl+M</kbd> Market</span>
				{/if}
			</div>
			<span class="sync {syncLabel().cls}"><span class="dot"></span>{syncLabel().text}</span>
			{#if isSpotlight}
				<div class="spot-status">
					<span class="dot" class:live={overlay?.game_running}></span>
					<button onclick={toggleClickThrough}>
						{overlay?.click_through ? 'KLICK' : 'FOKUS'}
					</button>
				</div>
			{/if}
		</div>
	</div>
</main>