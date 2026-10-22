<script lang="ts">
	// SC Architects Overlay — Suchpalette (Arkanis-UX-Rework, Phase t_1a0b0acb).
	//
	// Beide Tauri-Fenster rendern dieselbe Palette ("main" = Entwicklungs-Shell,
	// "spotlight" = Overlay über Star Citizen). Der Spotlight ist click-through-
	// fähig und zeigt den Spiel-/Fokus-Status; Esc blendet ihn aus.
	//
	// Bedienung orientiert an ArkanisOverlay:
	//   - Kategorie-Präfixe  :c :i :l  filtern je Term der Suchzeile
	//     (case-insensitive — :C/:I/:L parsen identisch, UI zeigt lowercase)
	//   - Inline-Rechner     =2*250+15%  → Ergebnis-Chip statt Ergebnisliste
	//   - Zeile anklicken/Enter → Detail-Seite mit Preis-Tabelle (Kauf/Verkauf)
	//     (keine Inline-Expansion mehr; Rückkehr behält Scroll-Position +
	//      Selektion der geöffneten Zeile)
	//   - Alt+1..0 fokussiert den n-ten Treffer, erneut → zurück ins Suchfeld
	//   - Esc: Suchfeld-Fokus ⇒ Overlay ausblenden; sonst Fokus ins Suchfeld
	//   - Pfeil-Navigation scrollt die Liste auf die fokussierte Zeile (auto)
	// Market-Modus (Tab/M): DAS Haupt-Suchfeld durchsucht Offers (Name/Typ/
	// Terminal/Verkäufer); Min/Max-Qualität-Stepper + Sortierung, Alt+Zahl-
	// Badges auf den ersten 10 Offers, Esc zurück zur Suche.
	//   - Ortssperre (+/L): sperrt einen Location-/Terminal-Treffer; die
	//     Commodity-Detail-Seite filtert Preise auf diesen Ort (< 43 Zeilen).
	//   - Detail-Tastatur: ↑↓ Zeile, ↵ Auswahl, Esc zurück, L sperren.
	//   - Marketplace: ≥15 Offers pro Seite + "Weitere laden" (Pagination).
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
	type PriceDetail = {
	commodity: string;
	source: string;
	fetched_at: number;
	prices: PricePoint[];
	// Location-Aggregat (kind:'location'): Gruppen je Terminal + flach
	// gefaltene Preise in `prices`. Null auf dem Commodity-/Item-Pfad.
	location: LocationPrices | null;
	};
	type LocationPrices = { location: string; terminal: string; groups: TerminalPriceGroup[] };
	type TerminalPriceGroup = { terminal_id: number; terminal_name: string; prices: PricePoint[] };
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

	// Ortssperre (Location / Terminal): Commodity-Detail filtert Preise darauf.
	type LocationLock = { id: number; name: string; kind: 'location' | 'terminal' };

	const OVERLAY_STATE_EVENT = 'overlay-state-changed';
	const SYNC_EVENT = 'trade-sync';
	const M_PAGE = 15; // Min. 15 Offers pro Seite (statt 1)

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

	// Detail-Seite: nur EINE Zeile ist gleichzeitig geöffnet (Detail-View statt
	// Inline-Expansion). `expanded` merkt die geöffnete Zeile für Rückkehr.
	type RowDetail = { busy: boolean; detail: PriceDetail | null; error: string | null };
	let xpan = $state<RowDetail | null>(null);
	// Stale-Response-Guard: eine spät eintreffende get_prices-Antwort darf eine
	// neuer geöffnete Zeile (oder eine geleerte Liste) nicht überschreiben.
	let xpanToken = 0;
	let expanded = $state<number | null>(null);
	let focusIdx = $state<number | null>(null);
	let searchEl = $state<HTMLInputElement | null>(null);

	// Detail-Ansicht (statt Inline-Expansion): blendet die Liste aus, zeigt die
	// Preis-Tabelle. Keyboard-Navigation über die `.pr`-Zeilen.
	let inDetail = $state(false);
	let detailHit = $state<SearchHit | null>(null);
	// `dSel` = fokussierte Detail-Zeile, `dChosen` = per Enter ausgewählt.
	let dSel = $state(0);
	let dChosen = $state<number | null>(null);
	// Scroll-Position der Ergebnisliste für den Zurück-Blick.
	let listScroll = $state(0);
	// Status-Toast nach Auswahl einer Detail-Zeile.
	let dToast = $state('');
	// Reactive Zeilen-Liste der Detail-Ansicht (Kauf dann Verkauf, jeweils
	// absteigend sortiert, ggf. ortssperren-gefiltert). Leer bei
	// Location-Aggregaten (keine Zeilen-Navigation dort).
	// Point 2: Terminal-Detail zeigt Warenname statt Terminal-Name.
	let dRows = $derived<{ price: number; loc: string; kind: 'buy' | 'sell' }[]>(xpan?.detail && !xpan.detail.location
		? (() => {
			const d = xpan?.detail!;
			const isTerminal = detailHit?.kind === 'terminal';
			const b = lockFilter(buyPoints(d));
			const s = lockFilter(sellPoints(d));
			return [
				...b.map((p) => ({ price: p.buy_price, loc: isTerminal ? p.commodity : p.location, kind: 'buy' as const })),
				...s.map((p) => ({ price: p.sell_price, loc: isTerminal ? p.commodity : p.location, kind: 'sell' as const }))
			];
		})()
		: []);

	// ---- Ortssperre (Lock) --------------------------------------------------
	// Setzt/entfernt die Ortssperre für die Commodity-Detail-Ansicht. Die
	// Commodity-Filterung erfolgt clientseitig über terminal_id (Spec: Ort
	// primär im Angebot/der Preise). Das Suchfeld bleibt aktiv.
	let locationLock = $state<LocationLock | null>(null);
	function lockLocation(h: SearchHit) {
		if (h.kind !== 'location' && h.kind !== 'terminal') return;
		locationLock = { id: h.id, name: h.name, kind: h.kind as 'location' | 'terminal' };
		dSel = 0;
	}
	function unlockLocation() {
		locationLock = null;
		dSel = 0;
	}
	// Clientseitiger Filter: nur Preise dieses Terminals/Ortes (Spec Ortssperre).
	function lockFilter(prices: PricePoint[]): PricePoint[] {
		const lock = locationLock;
		if (!lock) return prices;
		return prices.filter((p) => p.terminal_id === lock.id);
	}

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
	// Qualitäts-Filter (clientseitig — die UEX-API bietet keinen Qualitäts-
	// Query-Parameter, s. uex-api-reference.md §4b; leerer Wert = keine Grenze).
	let mqMin = $state('');
	let mqMax = $state('');
	let marketRegions = $state<string[]>([]);

	// Progressive Market-Pagination: erst die in-Memory-Snapshot-Seite (15)
	// zeigen, "Weitere laden" deckt Mehr aus dem Snapshot; ist auch das 100-
	// Angebot-Limit des Snapshots erschöpft, wird progressiv nachgefetcht
	// (Limit 200 → 400 → 800 → 1000) — bis das Backend nichts mehr liefert.
	let mMshown = $state(M_PAGE);
	let mLimit = $state(100);
	let mAllLoaded = $state(false);
	let mLoadingMore = $state(false);

	// Sichtbare Offers = API-/Provider-Antwort + clientseitiger Qualitätsfilter
	// (mit Sortierung kombinierbar — die Sortierung passiert im Provider, der
	// Filter danach verändert die Reihenfolge nicht).
	let visOffers = $derived(offers.filter((o) => qualityPasses(o, mqMin, mqMax)));
	let mVew = $derived(visOffers.slice(0, mMshown));
	// Qualitätsfilter neu setzen → immer ab Seite 1 beginnen (kein abgeschnittenes
	// Ergebnis, wenn der Filter die Liste verkürzt).
	$effect(() => {
		mqMin; mqMax;
		mMshown = Math.min(M_PAGE, visOffers.length);
	});
	$effect(() => {
		// mSel auf die gefilterte Liste klemmen (Qualitätsfilter ändert Länge).
		const top = Math.max(visOffers.length - 1, 0);
		if (mSel > top || (visOffers.length === 0 && mSel !== 0)) mSel = top;
	});

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
	//
	// Präfixe sind case-insensitive (Live-Feedback Punkt 1): `:c`, `:C`, `:C `,
	// `' ':i` u.ä. parsen alle wie `:c`. Das UI zeigt Kategorien lowercase.
	function parseQuery(): { text: string; cats: string[]; isCalc: boolean } {
		const raw = input.trim();
		if (!raw) return { text: '', cats: [], isCalc: false };
		if (raw.startsWith('=')) {
			return { text: raw.slice(1).trim(), cats: [], isCalc: true };
		}
		const cats: string[] = [];
		const terms: string[] = [];
		for (const tok of raw.split(/\s+/)) {
			const low = tok.toLowerCase();
			if (low === ':c') cats.push('commodity');
			else if (low === ':i') cats.push('item');
			else if (low === ':l') cats.push('location');
			else if (low.startsWith(':')) terms.push(tok); // unbekanntes Präfix → Fuzzy-Term (Spec §2)
			else terms.push(tok);
		}
		return { text: terms.join(' '), cats: [...new Set(cats)], isCalc: false };
	}

	// ---- Eingabe (debounced): Suche im Such-Modus, Marktplatz im Market-Modus
	// Das HAUPT-Suchfeld treibt beide Modi — im Market-Modus durchsucht es die
	// Offers (Name/Typ/Terminal/Verkäufer), der Platzhalter wechselt mit.
	function onInput() {
		if (debounce) clearTimeout(debounce);
		debounce = setTimeout(() => void (phase === 'market' ? loadMarket() : runSearch()), 180);
	}

	async function runSearch() {
		const q = parseQuery();
		calcResult = null;
		calcError = null;
		expanded = null;
		focusIdx = null;
		xpan = null;
		xpanToken++;
		inDetail = false;
		detailHit = null;
		dSel = 0;
		dChosen = null;
		dToast = '';

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
		scrollRowIntoView(sel);
	}

	// Pfeiltasten-Navigation über die sichtbare Fensterhöhe hinaus scrollt die
	// Liste automatisch auf die fokussierte Zeile (Live-Feedback Punkt 6).
	// `.rwrap` umschließt auch die Inline-Expansion → auch aufgeklappt wird die
	// volle Zeilenhöhe berücksichtigt; `block: 'nearest'` scrollt minimal.
	function scrollRowIntoView(idx: number) {
		document.querySelectorAll<HTMLElement>('.rwrap')[idx]?.scrollIntoView({ block: 'nearest' });
	}
	function scrollOfferIntoView(idx: number) {
		document.querySelectorAll<HTMLElement>('.mrow')[idx]?.scrollIntoView({ block: 'nearest' });
	}

	// Clientseitiger Qualitätsfilter (UEX-Skala 0–1000; leerer Wert = offen).
	function qualityPasses(o: Offer, minRaw: string, maxRaw: string): boolean {
		const min = parseInt(minRaw, 10);
		const max = parseInt(maxRaw, 10);
		if (Number.isFinite(min) && o.quality < min) return false;
		if (Number.isFinite(max) && o.quality > max) return false;
		return true;
	}

	// ---- Detail-Seite (statt Inline-Expansion) ------------------------------
	// Öffnet die Detail-Seite für eine Ergebnis-Zeile: lazy-Laden der Preise,
	// Scroll-Position der Liste merken, Rückkehr behält Selektion + Position.
	function openRow(idx: number) {
		if (!flat[idx]) return;
		sel = idx;
		expanded = idx;
		xpan = { busy: true, detail: null, error: null };
		// Scroll-Position der Ergebnisliste merken, damit "zurück" genau dort
		// hinläuft, wo der User aufgehört hat (statt zum Listen-Top).
		const list = document.querySelector('.list');
		if (list) listScroll = list.scrollTop;
		detailHit = flat[idx];
		inDetail = true;
		dSel = 0;
		dChosen = null;
		dToast = '';
		focusRow(idx);
		void loadDetail(flat[idx]);
	}

	function closeDetail() {
		inDetail = false;
		detailHit = null;
		dSel = 0;
		dChosen = null;
		dToast = '';
		// Scroll-Position der Liste wiederherstellen (kein Scroll-Back to top).
		const list = document.querySelector('.list');
		if (list) list.scrollTop = listScroll;
		// Fokus auf die Zeile, die geöffnet worden war → seamless.
		focusRow(expanded ?? sel);
	}

	// Öffnet bzw. schließt (Toggle) die Detail-Ansicht für `idx`.
	function toggleOpen(idx: number) {
		if (inDetail && expanded === idx) {
			closeDetail();
		} else {
			openRow(idx);
		}
	}

	// Alt+Zahl nur fokussieren: Badge ausgegraut, aber NICHT öffnen.
	// (NB-1: das Öffnen übernimmt Enter auf der fokussierten Zeile.)
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
		inDetail = false;
		detailHit = null;
		dSel = 0;
		dChosen = null;
		dToast = '';
	}

	async function loadDetail(h: SearchHit) {
		const myToken = ++xpanToken;
		// get_prices nach `h.kind` routen (Backend-Kontrakt trade.rs):
		// - terminal (FTS-Hit im Location-Gruppen-Kind): Preise DIESES Terminals
		//   (eigener Datenbestand — Fix: NICHT das Standort-Aggregat).
		// - location (die Standort-Entity selbst): das Terminal-Aggregat in
		//   `location.groups` (je Terminal eine Gruppe).
		// - item: nie Preise, leere Tabelle → "nicht kaufbar"-Zustand unten.
		// - commodity/Default: Commodity-Preisliste.
		// id gehört nur an die kind-Abhängigen Pfade (Reviewer NB-1).
		const kindArg =
			h.kind === 'terminal' ? 'terminal' : h.kind === 'location' ? 'location' : h.kind === 'item' ? 'item' : null;
		try {
			const d = await invoke<PriceDetail>('get_prices', {
				commodity: h.name,
				...(kindArg && h.id ? { id: h.id } : {}),
				...(kindArg ? { kind: kindArg } : {})
			});
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
			inDetail = false;
			detailHit = null;
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
	// Esc-Zustände (Spec §3/§4): 1) Detail-Ansicht offen → schließen + Fokus zurück;
	// 2) Ergebnis fokussiert (Alt+Zahl) oder eine Zeile geöffnet → zuklappen +
	//    Fokus ins Suchfeld; 3) Suchfeld fokussiert → Overlay ausblenden (Spotlight)
	//    bzw. leeren; 4) Fokus woanders → Fokus ins Suchfeld (Select-All).
	// Nie ausblenden, wenn der Fokus woanders liegt (Arkanis-Verhalten).
	function escSearch() {
		if (inDetail) {
			closeDetail();
			return;
		}
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
			mMshown = M_PAGE;
			mAllLoaded = false;
			// Wieder im Such-Modus: Ergebnisliste an den Suchtext des HAUPT-
			// Suchfelds angleichen (im Market-Modus wurde dort der Offer-Query
			// getippt — keine veralteten Treffer stehen lassen).
			void runSearch();
		}
	}

	// Kern-Loader für den Marketplace (cache-first, progressive Limits).
	async function fetchMarket(limit: number, showAll = false) {
		if (showAll) mLoadingMore = true; else mLoading = true;
		mNote = null;
		const filterObj = {
			location: '',
			q: input.trim(),
			item_id: null,
			operation: mOp === 'all' ? null : mOp,
			region: mRegion === 'all' ? '' : mRegion,
			sort: mSort,
			limit
		};
		try {
			const resp = await invoke<MarketResponse>('get_marketplace', { filters: filterObj });
			offers = resp.offers;
			mSource = resp.source;
			mNote = resp.note ?? null;
			marketRegions = [...new Set(resp.offers.map((o) => o.region).filter(Boolean))].sort();
			// Nach einem frischen Fetch: neue Seite 1, oder alles anzeigen (bei
			// progressiver Nachladung) — bis das Snapshot-Limit erreicht ist.
			mMshown = showAll ? offers.length : Math.min(M_PAGE, offers.length);
			mAllLoaded = offers.length < limit; // Backend lieferte weniger als gefordert
		} catch (e) {
			offers = [];
			mNote = String(e);
			mSource = 'offline';
			mAllLoaded = true;
		}
		mLoading = false;
		mLoadingMore = false;
	}

	async function loadMarket() {
		mLimit = 100;
		mAllLoaded = false;
		await fetchMarket(100, false);
	}

	// Pfeil-Navigation durch die Offers (sichtbare = qualitätsgefilterte Liste).
	function mMove(dir: number) {
		if (!visOffers.length) return;
		mSel = Math.min(Math.max(mSel + dir, 0), visOffers.length - 1);
		scrollOfferIntoView(mSel);
	}

	// "Weitere laden": zuerst aus dem Snapshot nachladen, bei Bedarf progressiv
	// (Limit 200/400/800/1000) nachfetchen — bis das Backend nichts mehr liefert.
	function loadMore() {
		if (mMshown < visOffers.length) {
			mMshown = Math.min(mMshown + M_PAGE, visOffers.length);
			return;
		}
		if (mLimit < 1000 && !mAllLoaded) {
			mLimit = Math.min(mLimit * 2, 1000);
			void fetchMarket(mLimit, true);
		}
	}

	function openOfferDetail() {
		if (!visOffers.length) return;
		offerDetail = visOffers[mSel] ?? null;
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
		mMshown = M_PAGE;
		void loadMarket();
	}

	function setOp(o: 'all' | MarketOp) {
		mOp = o;
		mMshown = M_PAGE;
		void loadMarket();
	}

	function setRegion(r: string) {
		mRegion = r;
		mMshown = M_PAGE;
		void loadMarket();
	}

	// ---- Detail-Seiten-Tastatur (über .pane-detail .pr) ---------------------
	// ↑↓ scrollt die Preis-Zeile in den Fokus (per Effekt gekennzeichnet),
	// ↵ wählt sie aus (Highlight + Toast), Esc verlässt die Detail-Ansicht.
	function dMove(dir: number) {
		if (!dRows.length) return;
		dSel = Math.min(Math.max(dSel + dir, 0), dRows.length - 1);
		document.querySelectorAll<HTMLElement>('.pane-detail .pr')[dSel]?.scrollIntoView({ block: 'nearest' });
	}
	function dSelect() {
		if (!dRows.length) return;
		dChosen = dSel;
		const r = dRows[dSel];
		dToast = `${r.kind === 'buy' ? 'Kauf' : 'Verkauf'} · ${fmt(r.price)} aUEC · ${r.loc}`;
	}
	// Sync highlight-Klassen (is-sel/is-chosen) über die DOM-Reihenfolge der
	// `.pr`-Zeilen — funktioniert für Kauf- und Verkaufstabelle gleichermaßen,
	// weil dRows genau diese Reihenfolge (Kauf dann Verkauf) widerspiegelt.
	$effect(() => {
		const rows = document.querySelectorAll<HTMLElement>('.pane-detail .pr');
		rows.forEach((r, i) => {
			r.classList.toggle('is-sel', dRows.length > 0 && i === dSel);
			r.classList.toggle('is-chosen', dRows.length > 0 && i === dChosen);
		});
	});

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
		const ages = ds.map((d) => d.age_secs).filter((i) => i != null);
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

	// ---- Formatierung (Deutsch, Mono — Spalten bleiben stehen) ---------------
	function fmt(n: number, d = 2): string {
		return n.toLocaleString('de-DE', { maximumFractionDigits: d, minimumFractionDigits: 0 });
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
		// Marktplatz-Modus: Angebot-Navigation + Detail.
		if (phase === 'market') {
			const t = e.target;
			const inStepper = t instanceof HTMLInputElement && t.type === 'number';
			const onChip = t instanceof HTMLButtonElement;
			if (k === 'ArrowDown' && !offerDetail && !inStepper) {
				mMove(1);
				e.preventDefault();
				return;
			}
			if (k === 'ArrowUp' && !offerDetail && !inStepper) {
				mMove(-1);
				e.preventDefault();
				return;
			}
			if (k === 'ArrowRight' && !offerDetail) {
				openOfferDetail();
				e.preventDefault();
				return;
			}
			if (k === 'ArrowLeft' && offerDetail) {
				offerDetail = null;
				e.preventDefault();
				return;
			}
			if (k === 'Enter' && offerDetail && !onChip) {
				void openOfferDeepLink();
				e.preventDefault();
				return;
			}
			if (k === 'Enter' && !offerDetail && !onChip && !inStepper) {
				openOfferDetail();
				e.preventDefault();
				return;
			}
			if (k === 'Escape') {
				if (offerDetail) {
					offerDetail = null;
				} else {
					// Esc verlässt den Market-Modus zurück zur Suche — blendet
					// das Overlay NICHT aus (Fokus liegt in der Ergebnisliste).
					toggleMarket();
				}
				e.preventDefault();
				return;
			}
			if (k === 'Tab' && mMshown < visOffers.length && !offerDetail && !inStepper) {
				// Tab erreicht den "Weitere laden"-Button (nächste Seite).
				const btn = document.querySelector<HTMLButtonElement>('.btn-more');
				if (btn) {
					btn.focus();
					e.preventDefault();
					return;
				}
			}
			// Alt+Zahl-Badges wie im Such-Modus: ersten 10 Offers fokussieren.
			if (e.altKey && !offerDetail && /^[0-9]$/.test(k)) {
				if (!visOffers.length) return;
				const n = k === '0' ? 9 : Number(k) - 1;
				if (n >= visOffers.length) return;
				if (mSel === n) {
					focusSearch(true);
					e.preventDefault();
					return;
				}
				mSel = n;
				scrollOfferIntoView(n);
				e.preventDefault();
				return;
			}
			return;
		}

		// Ortssperre per Tastatur (L) — wie m/M: nicht schlucken, wenn ein
		// Eingabefeld fokussiert ist. Funktioniert in Listen- und Detail-Ansicht.
		if ((k === 'l' || k === 'L') && !isFieldFocused(e)) {
			const cur = inDetail ? detailHit : flat[sel];
			if (!cur) return;
			if (!locationLock && (cur.kind === 'location' || cur.kind === 'terminal')) lockLocation(cur);
			else if (locationLock) unlockLocation();
			e.preventDefault();
			return;
		}

		// Detail-Ansicht: ↑↓/↵/Esc (Ortssperre wird oben behandelt).
		if (inDetail) {
			if (k === 'ArrowDown') {
				dMove(1);
				e.preventDefault();
				return;
			}
			if (k === 'ArrowUp') {
				dMove(-1);
				e.preventDefault();
				return;
			}
			if (k === 'Enter') {
				dSelect();
				e.preventDefault();
				return;
			}
			if (k === 'Escape') {
				closeDetail();
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
			toggleOpen(focusIdx ?? sel);
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
			// Suche/Liste ist der Hauptpfad; Tab navigiert natürlich weiter
			// (kein Filter-Cycle — Arkanis nutzt Präfixe für Kategorien).
			return;
		}
	}

	// ---- Preis-Tabellen für die Detail-Ansicht (Kauf | Verkauf) -------------
	// Arkanis GameEntitySearchDetails/PriceTagTable: zwei Spalten nebeneinander.
	function buyPoints(d: PriceDetail): PricePoint[] {
		return (d.prices ?? [])
			.filter((p) => p.buy_price > 0)
			.sort((a, b) => b.buy_price - a.buy_price);
	}

	function sellPoints(d: PriceDetail): PricePoint[] {
		return (d.prices ?? [])
			.filter((p) => p.sell_price > 0)
			.sort((a, b) => b.sell_price - a.buy_price);
	}

	// Per-Terminal-Gruppen des Location-Aggregats (QA-Auflage A): gleiche
	// Kauf/Verkauf-Logik wie buyPoints/sellPoints, nur auf einer Gruppe.
	function groupBuys(g: TerminalPriceGroup): PricePoint[] {
		return (g.prices ?? [])
			.filter((p) => p.buy_price > 0)
			.sort((a, b) => b.buy_price - a.buy_price);
	}

	function groupSells(g: TerminalPriceGroup): PricePoint[] {
		return (g.prices ?? [])
			.filter((p) => p.sell_price > 0)
			.sort((a, b) => b.sell_price - a.buy_price);
	}

	function fmtStock(p: PricePoint): string {
		return fmtQty(Math.max(p.sell_stock, p.buy_stock));
	}

	// Beste Preise für die Detail-Kopfzeile (aus der ortsgefilterten Menge).
	function bestBuy(d: PriceDetail | null): number {
		const xs = (lockFilter(d?.prices ?? []).map((p) => p.buy_price).filter((v) => v > 0));
		return xs.length ? Math.max(...xs) : 0;
	}

	function bestSell(d: PriceDetail | null): number {
		const xs = (lockFilter(d?.prices ?? []).map((p) => p.sell_price).filter((v) => v > 0));
		return xs.length ? Math.max(...xs) : 0;
	}

	// Kategorie-Icons = Präfixe lowercase im UI (Live-Feedback Punkt 1: das UI
	// zeigt Präfixe wie die Eingabe, nicht `:L`/`:C`-Großbuchstaben).
	const ICONS: Record<string, string> = { commodity: 'c', item: 'i', location: 'l' };
</script>

<svelte:window onkeydown={onKey} />

<main class="pal-root">
	<div class="shell" class:in-detail={inDetail}>
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
				placeholder={phase === 'market' ? 'Marktplatz durchsuchen — Name / Typ / Terminal / Verkäufer …' : 'Commodity, Item oder Terminal suchen …'}
				spellcheck="false"
				autocomplete="off"
			/>
			<span class="smode">{phase === 'market' ? 'MARKETPLACE' : 'SUCHE'}</span>
			<span class="esc">ESC</span>
		</div>

		<!-- Helper-Zeile: Shortcuts -->
		<div class="helper">
			{#if phase === 'market'}
				<span><kbd>↑↓</kbd> offers</span>
				<span class="sep">·</span>
				<span><kbd>Alt+1..0</kbd> offer</span>
				<span class="sep">·</span>
				<span><kbd>↵</kbd> detail</span>
				<span class="sep">·</span>
				<span><kbd>Tab</kbd> → Weitere laden</span>
				<span class="sep">·</span>
				<span><kbd>Esc</kbd> zurück zur Suche</span>
				<span class="sep">·</span>
				<span>Haupt-Suchfeld = Offer-Suche</span>
			{:else}
				<span><kbd>:c</kbd> commodities</span>
				<span class="sep">·</span>
				<span><kbd>:i</kbd> items</span>
				<span class="sep">·</span>
				<span><kbd>:l</kbd> locations</span>
				<span class="sep">·</span>
				<span><kbd>=</kbd> math</span>
				<span class="sep">·</span>
				<span><kbd>%</kbd> ÷100</span>
				<span class="sep">·</span>
				<span><kbd>↵</kbd> öffnet Detail</span>
				<span class="sep">·</span>
				<span><kbd>L</kbd> Ort sperren</span>
			{/if}
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
						<button type="button" class="chip" class:is-on={mOp === 'all'} onclick={() => setOp('all')}>Alle</button>
						<button type="button" class="chip" class:is-on={mOp === 'sell'} onclick={() => setOp('sell')}><span class="dot"></span>Verkauf</button>
						<button type="button" class="chip" class:is-on={mOp === 'buy'} onclick={() => setOp('buy')}><span class="dot"></span>Kauf</button>
						<span class="m-spacer"></span>
						<span class="m-lbl">Region</span>
						<button type="button" class="chip" class:is-on={mRegion === 'all'} onclick={() => setRegion('all')}>Alle</button>
						{#each marketRegions as r}
							<button type="button" class="chip" class:is-on={mRegion === r} onclick={() => setRegion(r)}>{r}</button>
						{/each}
					</div>
					<div class="mbar2">
						<span class="m-lbl">Qualität</span>
						<input class="qnum" type="number" min="0" max="1000" step="10" placeholder="min" aria-label="Min-Qualität (0–1000)" title="Min-Qualität (0–1000)" bind:value={mqMin} />
						<span class="qnum-dash">–</span>
						<input class="qnum" type="number" min="0" max="1000" step="10" placeholder="max" aria-label="Max-Qualität (0–1000)" title="Max-Qualität (0–1000)" bind:value={mqMax} />
						<button type="button" class="chip" onclick={() => { mqMin = ''; mqMax = ''; }}>Q ×</button>
						<span class="m-lbl">Sort</span>
						<button type="button" class="chip" class:is-on={mSort === 'price_asc'} onclick={() => setSort('price_asc')}>Preis ↑</button>
						<button type="button" class="chip" class:is-on={mSort === 'price_desc'} onclick={() => setSort('price_desc')}>Preis ↓</button>
						<button type="button" class="chip" class:is-on={mSort === 'newest'} onclick={() => setSort('newest')}>Neueste</button>
						<span class="m-spacer"></span>
						{#if mSource === 'offline'}
							<span class="chip warn"><span class="dot"></span>Offline-Snapshot</span>
						{:else if mSource === 'cache'}
							<span class="chip"><span class="dot"></span>Cache</span>
						{/if}
					</div>

					{#if visOffers.length}
						<div class="mlist">
							{#each mVew as o, i}
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
									<span class="mbadge">{badgeOf(i)}</span>
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
							<span>{mVew.length} / {visOffers.length} Angebote · Seite {Math.min(M_PAGE, mMshown) === 0 ? 0 : Math.ceil(mMshown / M_PAGE)}</span>
							<div class="m-more">
								{#if mLoadingMore}
									<span class="spin"></span><span class="sm">lädt …</span>
								{:else if mMshown < visOffers.length}
									<button class="btn-more" onclick={loadMore}>Weitere laden</button>
								{:else if !mAllLoaded}
									<button class="btn-more" onclick={loadMore}>(noch mehr laden)</button>
								{:else}
									<span class="sm">Alle geladen</span>
								{/if}
								<span class="src">UEX Corp</span>
							</div>
						</div>
					{:else}
						<div class="row-empty" style="padding: 28px 16px">Keine Angebote für diese Filter.</div>
					{/if}
				{/if}
			</div>

		{:else}
			<div class="pane-list" class:in-detail={inDetail}>
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
					<!-- Found-Chip über der Liste + Ortssperre -->
					<div class="found">
						<span class="c">Found <b>{searchTotal}</b> results in <b>{searchMs}</b> ms</span>
						{#if !locationLock}
							{@const cur = flat[sel]}
							{#if cur && (cur.kind === 'location' || cur.kind === 'terminal')}
								<button class="lock-btn" aria-label="Ort sperren (L)" onclick={() => lockLocation(cur)}>+</button>
							{/if}
						{:else}
							<span class="lock-badge">📌 {locationLock.name} <button class="lock-x" aria-label="sperren aufheben" onclick={unlockLocation}>×</button></span>
						{/if}
					</div>

					{#if locationLock && detailHit?.kind === 'commodity'}
						<div class="lock-hint"><span class="dot"></span>Preise gefiltert auf <b>{locationLock.name}</b> · <span class="sm">{dRows.length} Preiszeilen</span> · <button class="btn-ghost sm" onclick={unlockLocation}>× sperren aufheben</button></div>
					{/if}

					<div class="list">
						{#each groups as g, gi}
							{@const show = filters.length ? filters.includes(g.kind) : true}
							{#if show && g.hits.length}
								<div class="vgroup">{g.label} <span class="n">{g.hits.length}</span></div>
								{#each g.hits as h, hi}
									{@const idx = groupOffset(gi) + hi}
									<div class="rwrap" class:in-detail={inDetail && expanded === idx}>
										<div
											class="row"
											tabindex="-1"
											class:is-sel={idx === sel}
											class:is-focus={focusIdx === idx}
											role="option"
											aria-selected={idx === sel}
											onclick={() => toggleOpen(idx)}
										>
											<span class="rbadge">{badgeOf(idx)}</span>
											<span class="ic">{ICONS[g.kind] ?? '?'}{locationLock && (h.kind === 'location' || h.kind === 'terminal') && locationLock.id === h.id ? ' 🔒' : ''}</span>
											<div>
												<div class="name">{h.name}</div>
												<div class="sub">{h.subtitle}</div>
											</div>
											<div class="meta">
												<div class="mf"><span class="k">UEX-ID</span><span class="v">{h.code ?? h.id}</span></div>
											</div>
										</div>
									</div>
								{/each}
							{:else if show && !g.hits.length}
								<div class="vgroup">{g.label} <span class="n">0</span></div>
								<div class="row-empty">Keine Treffer</div>
							{/if}
						{/each}
					</div>

					<!-- Detail-Seite (Overlay über der Liste) -->
					{#if inDetail}
						<div class="pane-detail" role="dialog" aria-modal="true" aria-label="Detail">
							{#if xpan?.busy && !xpan?.detail}
								<div class="d-ghost">
									<div class="cap">Lade Preise …</div>
								</div>
							{:else if xpan?.error}
								<div class="d-ghost">
									<div class="cap">Fehler</div>
									<div class="sm">{xpan.error}</div>
								</div>
							{:else if xpan?.detail}
								{@const d = xpan.detail}
								<div class="d-head">
									<button class="d-back" aria-label="zurück zur Liste" onclick={closeDetail}>←</button>
									<div class="d-title">
										<h2>{detailHit?.name ?? d.commodity}</h2>
										<span class="c">{detailHit?.subtitle ?? ''}</span>
									</div>
									<div class="d-price">
										<span class="buy">↗ {fmt(bestBuy(d))}</span>
										<span class="sell">↘ {fmt(bestSell(d))}</span>
									</div>
								</div>

								{#if locationLock && detailHit?.kind === 'commodity'}
									<div class="lock-hint"><span class="dot"></span>↳ gefiltert auf <b>{locationLock.name}</b> · {dRows.length} Preiszeilen · <button class="btn-ghost sm" onclick={unlockLocation}>×</button></div>
								{/if}

								{#if dToast}
									<div class="d-toast" role="status">{dToast} <b>↵ Auswahl</b></div>
								{/if}

								<div class="d-body">
									{#if d.location}
										<!-- Location-Aggregat: je Terminal eine Gruppe -->
										{@const loc = d.location}
										{#each loc.groups as grp, gi}
											{@const gbuys = groupBuys(grp)}
											{@const gsells = groupSells(grp)}
											<div class="ptab">
												<div class="cap">{grp.terminal_name}</div>
												{#if gbuys.length || gsells.length}
													<div class="ph"><span>Item</span><span>Preis</span><span>SCU</span></div>
													{#if gbuys.length}
														<div class="psub buy">Kauf</div>
														{#each gbuys as p, bi}
															<div class="pr" class:best={bi === 0}>
																<span class="loc">{p.commodity}</span>
																<span class="val buy">{fmt(p.buy_price)}</span><span class="unit">aUEC</span>
														</div>
														{/each}
													{:else}
														<div class="psub buy empty">Kauf — kein Terminal</div>
													{/if}
													{#if gsells.length}
														<div class="psub sell">Verkauf</div>
														{#each gsells as p, si}
															<div class="pr" class:best={si === 0}>
																<span class="loc">{p.commodity}</span>
																<span class="val sell">{fmt(p.sell_price)}</span><span class="unit">aUEC</span>
															</div>
														{/each}
													{:else}
														<div class="psub sell empty">Verkauf — kein Terminal</div>
													{/if}
												{:else}
													<div class="pr"><span class="loc" style="color: var(--fgfaint)">keine Preisdaten</span></div>
												{/if}
											</div>
										{/each}
									{:else if d.prices.length === 0}
										<!-- QA-Auflage B: Item ohne Preise → "nicht kaufbar" -->
										<div class="np-state">nicht kaufbar · keine Preisdaten</div>
									{:else}
										<!-- Commodity / Terminal mit Preisen -->
										{@const dIsTerminal = detailHit?.kind === 'terminal'}
										{@const buys = lockFilter(buyPoints(d))}
										{@const sells = lockFilter(sellPoints(d))}
										<div class="ptab">
											<div class="cap">Alle <b class="buy">Kauf</b>-Preise · {buys.length} Terminal</div>
											<div class="ph"><span>{dIsTerminal ? 'Item' : 'Terminal'}</span><span>Preis</span><span>SCU</span></div>
											{#if buys.length}
												{#each buys as p, bi}
													<div class="pr" class:best={bi === 0}>
														<span class="loc">{dIsTerminal ? p.commodity : p.location}</span>
														<span class="val buy">{fmt(p.buy_price)}</span><span class="unit">aUEC</span>
													</div>
												{/each}
											{:else}
												<div class="pr"><span class="loc" style="color: var(--fgfaint)">{dIsTerminal ? 'Kein Artikel zum Kauf' : 'Kein Terminal kauft'}</span></div>
											{/if}
										</div>
										<div class="ptab">
											<div class="cap">Alle <b class="sell">Verkauf</b>-Preise · {sells.length} Terminal</div>
											<div class="ph"><span>{dIsTerminal ? 'Item' : 'Terminal'}</span><span>Preis</span><span>SCU</span></div>
											{#if sells.length}
												{#each sells as p, si}
													<div class="pr" class:best={si === 0}>
														<span class="loc">{dIsTerminal ? p.commodity : p.location}</span>
														<span class="val sell">{fmt(p.sell_price)}</span><span class="unit">aUEC</span>
													</div>
												{/each}
											{:else}
												<div class="pr"><span class="loc" style="color: var(--fgfaint)">{dIsTerminal ? 'Kein Artikel zum Verkauf' : 'Kein Terminal verkauft'}</span></div>
											{/if}
										</div>
									{/if}
								</div>

								<div class="d-foot">
									{#if dRows.length}
										<span><kbd>↑↓</kbd> Zeile ({dSel + 1}/{dRows.length})</span>
										<span class="sep">·</span>
										<span><kbd>↵</kbd> Auswahl</span>
										<span class="sep">·</span>
										<span><kbd>Esc</kbd> zurück</span>
										<span class="sep">·</span>
										<span><kbd>L</kbd> sperren</span>
									{:else}
										<span><kbd>Esc</kbd> zurück</span>
									{/if}
								</div>
							{/if}
						</div>
					{/if}
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
					<span><kbd>Alt+1..0</kbd> Offer</span>
					<span><kbd>↵</kbd> Detail</span>
					<span><kbd>Tab</kbd> Filter</span>
					{#if offerDetail}
						<span><kbd>↵</kbd> im Browser öffnen</span>
					{/if}
					<span><kbd>Esc</kbd> zurück zur Suche</span>
				{:else}
					<span><kbd>↑↓</kbd> Nav</span>
					<span><kbd>Alt+1..0</kbd> Treffer</span>
					<span><kbd>↵</kbd> Detail</span>
					<span><kbd>L</kbd> Ort sperren</span>
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
