<script lang="ts">
	// SC Architects Overlay — Suchpalette (Arkanis-UX-Rework, Phase t_1a0b0acb).
	//
	// Beide Tauri-Fenster rendern dieselbe Palette ("main" = Entwicklungs-Shell,
	// "spotlight" = Overlay über Star Citizen). Der Spotlight ist click-through-
	// fähig und zeigt den Spiel-/Fokus-Status; das Ausblenden übernimmt der
	// globale Toggle (CTRL+SHIFT+U), NICHT Esc.
	//
	// Bedienung orientiert an ArkanisOverlay:
	//   - Kategorie-Präfixe  :c :i :l  filtern je Term der Suchzeile
	//     (case-insensitive — :C/:I/:L parsen identisch, UI zeigt lowercase)
	//   - Inline-Rechner     =2*250+15%  → Ergebnis-Chip statt Ergebnisliste
	//   - Zeile anklicken/Enter → Detail-Seite mit Preis-Tabelle (Kauf/Verkauf)
	//     (keine Inline-Expansion mehr; Rückkehr behält Scroll-Position +
	//      Selektion der geöffneten Zeile)
	//   - Alt+1..0 fokussiert den n-ten Treffer, erneut → zurück ins Suchfeld
	//   - Esc: reine interne Zurück-Navigation (Detail → Liste → Suche); im Root
	//     tut Esc NICHTS — Ausblenden ausschließlich per globalem Toggle
	//     (Hyprland-Bind CTRL+SHIFT+U → arch-overlay toggle, KEIN In-App-Esc)
	//   - Pfeil-Navigation scrollt die Liste auf die fokussierte Zeile (auto)
	// Modi (Karte t_315a26cc): F1 Suche · F2 Marktplatz · F3 Inventar ·
	// F4 Hangar · F12 Einstellungen — In-App-Keyhandler, die nur greifen, wenn
	// das Overlay sichtbar+fokussiert ist (compositor_focus beim Show); die Rail
	// links zeigt die Modi mit F-Key-Label, aktiver Modus in Akzentfarbe.
	// Market-Modus (F2 / M / Strg+M): DAS Haupt-Suchfeld durchsucht Offers
	// (Name/Typ/Terminal/Verkäufer); Min/Max-Qualität-Stepper + Sortierung,
	// Alt+Zahl-Badges auf den ersten 10 Offers, Esc/F2 zurück zur Suche.
	//   - Detail-Tastatur: ↑↓ Zeile, ↵ Auswahl, Esc zurück.
	//   - N2 Enter-Detail-Filter: das Suchfeld ist im Detail der In-Context-Filter
	//     (Location → Waren-/Terminalnamen, Commodity/Item → Orts-/Terminalnamen).
	//   - Marketplace: ≥15 Offers pro Seite + "Weitere laden" (Pagination).
	//
	// Zustände: idle · typing · loading · error · offline · market
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { invoke, isTauri } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	// Relatives $lib-Import: dieses Repo nutzt kein svelte.config.js, der
	// `$lib`-Alias ist für svelte-check nicht aufgelöst.
	import Rail from '../lib/Rail.svelte';
	import Inventory from '../lib/Inventory.svelte';

	// ---- IPC-Typen (Mirror der Serde-DTOs in src-tauri/src/trade.rs) ---------
	type OverlayState = { visible: boolean; game_running: boolean; click_through: boolean };
	// t_925a577a: id kommt als STRING (Wiki-IDs > 2^53 runden als JS-Number
// still — 404707429735935130 → …35100 — und brechen den Detail-Fetch).
type SearchHit = { id: string; name: string; kind: string; subtitle: string; code: string | null };
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

	const OVERLAY_STATE_EVENT = 'overlay-state-changed';
	// Tray "Config öffnen" → Backend-Event (siehe overlay::TRAY_OPEN_CONFIG_EVENT).
	const overlayTrayOpenConfig = 'tray-open-config';
	const SYNC_EVENT = 'trade-sync';
	const M_PAGE = 15; // Min. 15 Offers pro Seite (statt 1)
const MARKET_STALE_SECS = 300; // P3: Market-Daten älter als 5 min → Hintergrund-Refresh

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

	// ---- Config-View (t_163074f3) --------------------------------------------
	// Eigenes View in der Palette (wie die Detail-"Seite"): Esc/Zurück-Pfeil,
	// kein neues Fenster. Einstellungen wirken SOFORT + persistieren im Backend.
	type SettingsSnapshot = {
		palette: string;
		detail_variant: string;
		uex_api_key_set: boolean;
		commodity_source: string;
	};
	let inConfig = $state(false);
	let cfg = $state<SettingsSnapshot | null>(null);
	let apiKeyInput = $state('');
	let apiKeyNote = $state('');

	// P8-Detail-Variante: Backend-Wert ist Quelle der Wahrheit, aber die Umschal-
	// tung wirkt SOFORT auch ohne Backend (statischer Build/Probe) über einen
	// lokalen Fallback, bis settings_set_detail_variant antwortet.
	let localVariant = $state<'a' | 'c' | null>(null);

	// ---- Modi (Karte t_315a26cc) --------------------------------------------
	// Zentraler Mode-State: F1 Suche · F2 Marktplatz · F6 Routen · F3 Inventar · F4 Hangar ·
	// F12 Einstellungen. Der bestehende Market-Modus (M/Strg+M) ist derselbe
	// State — `phase === 'market'` bleibt die Render-Quelle, Config bleibt
	// `inConfig`; `railMode` trägt nur die Platzhalter-Modi (eigene Karten).
	// P1 t_a39f6811: F6 Handelsrouten — eigener Mode für die Rail/Keyhandler;
	// intern bleibt es der Market-Phase mit mTab='routes'.
	type Mode = 'search' | 'market' | 'routes' | 'inventory' | 'hangar' | 'settings';
	let mTab = $state<'offers' | 'routes'>('offers');
	let railMode = $state<'inventory' | 'hangar' | null>(null);
	let mode = $derived<Mode>(
		railMode ??
			(inConfig
				? 'settings'
				: phase === 'market'
					? mTab === 'routes'
						? 'routes'
						: 'market'
					: 'search')
	);
	// Palletten-Breite (bind:clientWidth) → Rail-Kollaps auf reine Icons, wenn schmal.
	let palW = $state(0);

	// ---- Hangar (Karte t_317f19c5) ------------------------------------------
	// Schiffe/Fahrzeuge/Module mit Versicherung — Backend-Tabelle `hangar`
	// (Schema v4). Import: Hangar-XPLORer JSON (Primärformat) + CCUgame CSV;
	// Export: eigenes sauberes JSON. Merge über pledge_id / (ship_code, name).
	type HangarItem = {
		id: number | null;
		name: string;
		origin: string;
		insurance: string | null;
		custom_name: string;
		entity_type: string;
		base_ship: string;
		ship_code: string;
		manufacturer: string;
		purchase_price: number | null;
		purchase_date: string;
		pledge_id: string;
		pledge_name: string;
		warbond: boolean;
		ccud: boolean;
		created_at: number | null;
		updated_at: number | null;
	};
	type HangarImportResult = { imported: number; updated: number; skipped: number; format: string };

	let hangarItems = $state<HangarItem[]>([]);
	let hangarLoaded = $state(false);
	let hangarBusy = $state(false);
	let hangarNote = $state('');
	let hangarFilter = $state<'all' | 'pledge' | 'ingame' | 'lti' | 'module'>('all');
	let hangarSel = $state(0);
	let hangarView = $state<'list' | 'detail'>('list');
	let hangarDetail = $state<HangarItem | null>(null);

	const hangarFiltered = $derived.by(() => {
		let items = hangarItems;
		if (hangarFilter === 'pledge') items = items.filter((i) => i.origin === 'pledge');
		else if (hangarFilter === 'ingame') items = items.filter((i) => i.origin === 'ingame');
		else if (hangarFilter === 'lti') items = items.filter((i) => i.insurance === 'LTI');
		else if (hangarFilter === 'module') items = items.filter((i) => i.entity_type === 'module');
		return items;
	});

	async function loadHangar(force = false) {
		if (hangarLoaded && !force) return;
		hangarBusy = true;
		hangarNote = '';
		try {
			hangarItems = await invoke<HangarItem[]>('hangar_list');
			hangarLoaded = true;
		} catch (e) {
			hangarNote = String(e);
		} finally {
			hangarBusy = false;
		}
	}

	function hangarDisplayName(i: HangarItem) {
		return i.custom_name || i.name;
	}

	function hangarModulesOf(shipName: string) {
		return hangarItems.filter((i) => i.entity_type === 'module' && i.base_ship === shipName);
	}

	async function hangarImportFile(format: 'xplorer' | 'ccugame', file: File) {
		hangarBusy = true;
		hangarNote = '';
		try {
			const text = await file.text();
			const res = await invoke<HangarImportResult>('hangar_import', { format, text });
			hangarNote = `Import (${res.format}): ${res.imported} neu · ${res.updated} aktualisiert · ${res.skipped} übersprungen`;
			await loadHangar(true);
		} catch (e) {
			hangarNote = String(e);
		} finally {
			hangarBusy = false;
		}
	}

	function hangarTriggerImport(format: 'xplorer' | 'ccugame') {
		const input = document.createElement('input');
		input.type = 'file';
		input.accept = format === 'xplorer' ? '.json,application/json' : '.csv,text/csv';
		input.onchange = () => {
			const f = input.files?.[0];
			if (f) hangarImportFile(format, f);
		};
		input.click();
	}

	async function hangarExport() {
		hangarBusy = true;
		try {
			const json = await invoke<string>('hangar_export');
			const blob = new Blob([json], { type: 'application/json' });
			const a = document.createElement('a');
			a.href = URL.createObjectURL(blob);
			a.download = 'architects-overlay-hangar.json';
			a.click();
			URL.revokeObjectURL(a.href);
			hangarNote = 'Export: architects-overlay-hangar.json';
		} catch (e) {
			hangarNote = String(e);
		} finally {
			hangarBusy = false;
		}
	}

	async function hangarDelete(item: HangarItem) {
		if (item.id == null) return;
		hangarBusy = true;
		try {
			await invoke('hangar_delete', { id: item.id });
			await loadHangar(true);
			hangarNote = `Gelöscht: ${hangarDisplayName(item)}`;
		} catch (e) {
			hangarNote = String(e);
		} finally {
			hangarBusy = false;
		}
	}

	function hangarBlank(): HangarItem {
		return {
			id: null,
			name: '',
			origin: 'ingame',
			insurance: null,
			custom_name: '',
			entity_type: 'ship',
			base_ship: '',
			ship_code: '',
			manufacturer: '',
			purchase_price: null,
			purchase_date: '',
			pledge_id: '',
			pledge_name: '',
			warbond: false,
			ccud: false,
			created_at: null,
			updated_at: null
		};
	}
	let hangarFormOpen = $state(false);
	let hangarEdit = $state(false);
	let hangarForm = $state<HangarItem>(hangarBlank());
	let hangarFormPrice = $state('');

	function hangarOpenAdd() {
		hangarForm = hangarBlank();
		hangarFormPrice = '';
		hangarFormOpen = true;
		hangarEdit = false;
		hangarView = 'list';
		hangarDetail = null;
	}

	function hangarStartEdit(it: HangarItem) {
		hangarForm = { ...it };
		hangarFormPrice = it.purchase_price != null ? String(it.purchase_price) : '';
		hangarEdit = true;
		hangarFormOpen = false;
	}

	function hangarCancelEdit() {
		hangarFormOpen = false;
		hangarEdit = false;
	}

	async function hangarSaveForm() {
		hangarBusy = true;
		hangarNote = '';
		try {
			const name = hangarForm.name.trim();
			if (!name) throw new Error('Name fehlt');
			const price = hangarFormPrice.trim();
			const item: HangarItem = {
				...hangarForm,
				name,
				purchase_price: price === '' ? null : Number(price.replace(',', '.'))
			};
			let msg;
			if (item.id != null) {
				await invoke<HangarItem>('hangar_update', { item });
				msg = `Aktualisiert: ${name}`;
			} else {
				const saved = await invoke<HangarItem>('hangar_add', { item });
				msg = `Angelegt: ${hangarDisplayName(saved)}`;
			}
			hangarFormOpen = false;
			hangarEdit = false;
			await loadHangar(true);
			// Review-Finding: nach dem Speichern zeigte die Detailansicht die
			// alten Werte — hangarDetail aus der frischen Liste nachziehen.
			if (hangarDetail?.id != null) {
				const fresh = hangarItems.find((i) => i.id === hangarDetail!.id);
				if (fresh) hangarDetail = fresh;
			}
			hangarNote = msg;
		} catch (e) {
			hangarNote = String(e);
		} finally {
			hangarBusy = false;
		}
	}

	function hangarOpenDetail(item: HangarItem) {
		hangarDetail = item;
		hangarView = 'detail';
	}

	function hangarBack() {
		if (hangarFormOpen || hangarEdit) {
			// Formular zuerst schließen, dann View-Ebene.
			hangarFormOpen = false;
			hangarEdit = false;
			return;
		}
		if (hangarView === 'detail') {
			hangarView = 'list';
			hangarDetail = null;
		} else {
			// Hangar-Root: zurück zur Suche (analog Platzhalter-Esc).
			railMode = null;
			focusSearch(true);
		}
	}

	let detailVariant = $derived<'a' | 'c'>((cfg?.detail_variant ?? localVariant ?? 'c') === 'a' ? 'a' : 'c');

	// data-palette + data-detail-variant am html — CSS-Scoping-Kontaktstelle
	// zum parallelen Frontend-Task (t_733aa11d).
	function applySettings(snapshot: SettingsSnapshot | null) {
		if (typeof document === 'undefined' || !snapshot) return;
		document.documentElement.dataset.palette = snapshot.palette;
		document.documentElement.dataset.detailVariant = snapshot.detail_variant;
	}

	async function loadSettings() {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_get');
			applySettings(cfg);
		} catch {
			// Ohne Tauri (SSR/vite-dev) → Palette bleibt am app.html-Default.
		}
	}

	function openConfig() {
		// Kein Enter-Filter-Zustand darf bestehen, während der Config-View offen
		// ist: Detail + In-Context-Filter + Auswahl werden vorab geräumt.
		inDetail = false;
		detailHit = null;
		expanded = null;
		focusIdx = null;
		xpan = null;
		xpanToken++;
		dSel = 0;
		dChosen = null;
		dToast = '';
		apiKeyInput = '';
		apiKeyNote = '';
		inConfig = true;
		if (searchEl) searchEl.blur();
	}

	function closeConfig() {
		inConfig = false;
		apiKeyInput = '';
		apiKeyNote = '';
		// Der Tray kann das Overlay (ohne laufendes Spiel) rein für die Config
		// gezeigt haben — das Backend darf das Fenster dann wieder verbergen.
		if (isTauri()) {
			invoke('overlay_config_closed').catch(() => {});
		}
		if (searchEl) searchEl.focus();
	}

	async function choosePalette(name: string) {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_palette', { name });
			applySettings(cfg);
		} catch (e) {
			apiKeyNote = String(e);
		}
	}

	// Commodity-Quelle (A/B): 'wiki' = Wiki-Detail-Preise primär, 'uex' =
	// UEX-Preistabelle primär. Wirkt SOFORT (SourceSwitch wird live geflippt),
	// ohne App-Neustart.
	async function chooseSource(name: string) {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_commodity_source', { name });
		} catch (e) {
			apiKeyNote = String(e);
		}
	}

	async function chooseVariant(name: string) {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_detail_variant', { name });
			applySettings(cfg);
		} catch (e) {
			apiKeyNote = String(e);
		}
	}

	async function saveApiKey() {
		const key = apiKeyInput.trim();
		if (!key) return;
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_uex_api_key', { key });
			apiKeyInput = '';
			apiKeyNote = 'API-Key gespeichert (lokal, 0600).';
		} catch (e) {
			apiKeyNote = String(e);
		}
	}

	async function removeApiKey() {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_uex_api_key', { key: null });
			apiKeyInput = '';
			apiKeyNote = 'API-Key entfernt.';
		} catch (e) {
			apiKeyNote = String(e);
		}
	}

	// Scroll-Position der Ergebnisliste für den Zurück-Blick.
	let listScroll = $state(0);
	// Status-Toast nach Auswahl einer Detail-Zeile.
	let dToast = $state('');

	// ---- In-Context-Filter (N2, ersetzt die alte Ortssperre) -----------------
	// Ist eine Detail-Ansicht offen, wird das HAUPT-Suchfeld zum Live-Filter
	// dieser Ansicht —
	//   - Location-Detail  → filtert die Inhalte (Waren-/Terminalnamen),
	//   - Commodity/Item   → filtert die Preisliste nach Orts-/Terminalnamen,
	//   - Terminal-Detail  → filtert nach Warennamen.
	// Leer = alles zeigen. Kein Badge, kein L-Shortcut, kein Backend-Parameter.
	let dFilter = $derived(input.trim().toLowerCase());
	type DRow = { p: PricePoint; kind: 'buy' | 'sell'; fi: number; name: string };
	type LocGroupView = { terminal_id: number; terminal_name: string; buys: DRow[]; sells: DRow[] };

	// Filtert + sortiert Preiszeilen einer Richtung; `c` zählt den flachen Index
	// über Kauf→Verkauf (Tastatur-Navigation bleibt über den Filter hinweg).
	function dRowsFor(prices: PricePoint[], kind: 'buy' | 'sell', byLoc: boolean, c: { i: number }): DRow[] {
		const val = (p: PricePoint) => (kind === 'buy' ? p.buy_price : p.sell_price);
		return prices
			.filter((p) => val(p) > 0)
			.filter((p) => !dFilter || (byLoc ? p.location : p.commodity).toLowerCase().includes(dFilter))
			// P8-Sortierung: Kauf günstigster zuerst (aufsteigend), Verkauf
			// höchster zuerst (absteigend) — "beste Gelegenheit" steht oben.
			.sort((a, b) => (kind === 'buy' ? val(a) - val(b) : val(b) - val(a)))
			.map((p) => ({ p, kind, fi: c.i++, name: byLoc ? p.location : p.commodity }));
	}

	// Location-Aggregat: je Terminal eine Gruppe. Filter trifft Terminalname
	// (ganze Gruppe bleibt) ODER Warenname (nur passende Zeilen bleiben); Gruppen
	// ohne Treffer verschwinden.
	let dLoc = $derived<LocGroupView[]>(
		xpan?.detail?.location
			? (() => {
					const c = { i: 0 };
					return (xpan?.detail!.location!.groups ?? [])
						// t_dc152764: Gruppen alphabetisch nach Terminal-Name — auch wenn
						// ein (älterer) Cache noch nach terminal_id sortierte Daten liefert.
						.slice()
						.sort((a, b) => a.terminal_name.toLowerCase().localeCompare(b.terminal_name.toLowerCase()))
						.flatMap((grp) => {
					const buys = dRowsFor(grp.prices ?? [], 'buy', false, c);
					const sells = dRowsFor(grp.prices ?? [], 'sell', false, c);
					const termHit = !!dFilter && grp.terminal_name.toLowerCase().includes(dFilter);
					if (!dFilter || termHit || buys.length || sells.length) {
						return [{ terminal_id: grp.terminal_id, terminal_name: grp.terminal_name, buys, sells }];
					}
					return [];
				});
			})()
			: []);

	// Flache Zeilen-Liste der Detail-Ansicht (Kauf dann Verkauf) — für Location-
	// Aggregate aus dLoc, sonst aus den Punktwolken (Commodity → Ortsname,
	// Terminal → Warenname).
	let dRows = $derived<DRow[]>(
		xpan?.detail?.location
			? dLoc.flatMap((g) => [...g.buys, ...g.sells])
			: (() => {
				const d = xpan?.detail;
				if (!d) return [];
				const c = { i: 0 };
				const byLoc = detailHit?.kind !== 'terminal';
				return [...dRowsFor(d.prices ?? [], 'buy', byLoc, c), ...dRowsFor(d.prices ?? [], 'sell', byLoc, c)];
			})());
	// Filter schrumpft die Liste → Selektion auf die verbleibenden Zeilen klemmen.
	$effect(() => {
		const top = Math.max(dRows.length - 1, 0);
		if (dSel > top) dSel = top;
	});
	// Beste Preise der (gefilterten) Detail-Menge für die Kopfzeile (P8):
	// Best Kauf = günstigster Kaufpreis (min), Best Verkauf = höchster
	// Verkaufspreis (max). Leere Richtung → 0 (Chip zeigt „—").
	let dBest = $derived({
		buy: Math.min(Infinity, ...dRows.filter((r) => r.kind === 'buy').map((r) => r.p.buy_price)),
		sell: Math.max(0, ...dRows.filter((r) => r.kind === 'sell').map((r) => r.p.sell_price))
	});

	// Marge (Best Kauf → Best Verkauf) für den P8-Hero; nur bei beiden Seiten
	// vorhanden, sonst 0 (Karte zeigt „—"). Folgt dem Filter (dBest aus dRows).
	let dMargin = $derived(dBest.buy !== Infinity && dBest.sell > 0 ? dBest.sell - dBest.buy : 0);

	// Bestands-/Nachfrage-Unterzeile (P8-Entscheid): 0 heißt bei UEX „keine
	// Meldung", nicht „0 SCU" — deshalb Klartext statt eines Strichs.
	function stockTxt(p: PricePoint, kind: 'buy' | 'sell'): string {
		const n = Math.round(kind === 'buy' ? p.buy_stock : p.sell_stock);
		if (kind === 'buy') return n > 0 ? `Bestand ${fmt(n)} SCU` : 'kein Bestand gemeldet';
		return n > 0 ? `Nachfrage ${fmt(n)} SCU` : 'keine Nachfrage gemeldet';
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
	// Qualitäts-Filter (clientseitig — die UEX-API bietet keinen Qualitäts-
	// Query-Parameter, s. uex-api-reference.md §4b; leerer Wert = keine Grenze).
	let mqMin = $state('');
	let mqMax = $state('');

	// Market-Pagination: der Provider liefert je Fetch sein komplettes
	// abrufbares Fenster (serverseitig nach item/operation verengt, sonst
	// Verkauf+Kauf-Fenster gemerged) — "Weitere laden" blättert nur noch im
	// In-Memory-Snapshot (15 pro Seite), es gibt kein progressives Nachfetchen.
	let mMshown = $state(M_PAGE);
	let mAllLoaded = $state(false);
	let mLoadingMore = $state(false);
	// P3/P4: Frische-Tracking + app-lokales F5-Feedback (Statuszeile, nicht modal).
	// fetched_at des letzten erfolgreichen Market-Fetch — PERSISTIERT (localStorage),
	// damit der Stale-Check beim Wiedereinstieg auch über App-Starts hinweg greift.
	function loadMarketFetchedAt(): number {
		try {
			const v = localStorage.getItem('architects-overlay.market.fetchedAt');
			const n = v === null ? 0 : Number(v);
			return Number.isFinite(n) && n > 0 ? n : 0;
		} catch {
			return 0;
		}
	}
	function persistMarketFetchedAt(v: number) {
		try {
			if (v > 0) localStorage.setItem('architects-overlay.market.fetchedAt', String(v));
		} catch {
			/* localStorage unvollständig — In-Memory-Tracking bleibt aktiv. */
		}
	}
	let mFetchedAt = $state(loadMarketFetchedAt());
	let f5Msg = $state(''); // kurzer Status-Text aus dem F5-Sync
	let f5Clear: ReturnType<typeof setTimeout> | null = null;

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

	// ---- Handelsrouten (Karte t_128b7547) ------------------------------------
	// Separater Tab im Marktmodus (Design-Empfehlung der Karte). Datenquelle:
	// UEX-Sweep über alle Commodity-Terminals (Backend `get_routes`), Filte-
	// rung/Sortierung serverseitig im Backend. BEWUSST ohne Mindest-/Maximal-
	// bestand-Filter (Unterschied zu Arkanis).
	type RouteSort = 'profit_desc' | 'profit_asc' | 'distance_asc' | 'total_profit_desc' | 'margin_desc';
	type TradeRoute = {
		commodity_id: number;
		commodity: string;
		illegal: boolean;
		buy_terminal_id: number;
		buy_terminal: string;
		buy_chain: string;
		buy_price: number;
		buy_terminal_autoload: boolean;
		buy_terminal_cargo_deck: boolean;
		buy_terminal_ground: boolean;
		sell_terminal_id: number;
		sell_terminal: string;
		sell_chain: string;
		sell_price: number;
		sell_terminal_autoload: boolean;
		sell_terminal_cargo_deck: boolean;
		sell_terminal_ground: boolean;
		sell_stock: number;
		profit_per_scu: number;
		profit_total: number;
		cost_total: number;
		margin: number;
		distance: number;
	};
	type RoutesResponse = { source: string; swept_at: number; refreshing: boolean; routes: TradeRoute[] };

	let routes_ = $state<TradeRoute[]>([]);
	let rLoading = $state(false);
	let rNote = $state<string | null>(null);
	let rSweptAt = $state(0);
	let rRefreshing = $state(false);
	let rSel = $state(0);
	let rDetail = $state<TradeRoute | null>(null);
	// Filter (Kartenvorgabe).
	let rCommodity = $state('');
	let rFrom = $state('');
	let rTo = $state('');
	let rLegal = $state<'all' | 'legal' | 'illegal'>('all');
	let rAutoload = $state<'any' | 'yes' | 'no'>('any');
	let rCargo = $state<'any' | 'yes' | 'no'>('any');
	let rPlace = $state<'any' | 'ground' | 'space'>('any');
	let rSort = $state<RouteSort>('profit_desc');
	let rScu = $state(0);
	// Schiff aus Hangar → SCU automatisch (UEX /vehicles.scu), oder manuell.
	let rShipName = $state('');
	let vehicleScuMap = $state<Record<string, number> | null>(null);

	/** Ship → SCU via UEX /vehicles (case-insensitive über name/full/slug). */
	async function shipScu(name: string): Promise<number | null> {
		if (!name.trim()) return null;
		if (vehicleScuMap === null) {
			try {
				vehicleScuMap = await invoke<Record<string, number>>('vehicles_scu');
			} catch {
				vehicleScuMap = {};
			}
		}
		const q = name.trim().toLowerCase();
		const v = vehicleScuMap[q];
		return typeof v === 'number' && v > 0 ? v : null;
	}

	async function applyShipScu(name: string) {
		rShipName = name;
		if (!name) {
			rScu = 0;
			return;
		}
		const scu = await shipScu(name);
		if (scu !== null) rScu = scu;
	}

	let rLoadingRef: boolean = false;
	async function loadRoutes() {
		if (rLoadingRef) return;
		rLoadingRef = true;
		rLoading = true;
		rNote = null;
		try {
			const resp = await invoke<RoutesResponse>('get_routes', {
				filters: {
					commodity: rCommodity.trim() || null,
					from: rFrom.trim() || null,
					to: rTo.trim() || null,
					legality: rLegal,
					autoload: rAutoload === 'any' ? null : rAutoload === 'yes',
					cargo_deck: rCargo === 'any' ? null : rCargo === 'yes',
					ground_only: rPlace === 'ground',
					space_only: rPlace === 'space',
					scu: Number(rScu) || 0,
					sort: rSort
				}
			});
			routes_ = resp.routes;
			rSweptAt = resp.swept_at;
			rRefreshing = resp.refreshing;
			rSel = 0;
		} catch (e) {
			routes_ = [];
			rRefreshing = false;
			rNote = String(e);
		} finally {
			rLoadingRef = false;
			rLoading = false;
		}
	}

	// Filter/Sort-Änderung → neu laden (debounced, Text-Filter tippt sich).
	let rDebounce: ReturnType<typeof setTimeout> | null = null;
	function queueRoutes() {
		if (rDebounce) clearTimeout(rDebounce);
		rDebounce = setTimeout(() => void loadRoutes(), 250);
	}
	$effect(() => {
		// Reaktive Abhängigkeiten: alle Filter + Sort; mTab steuert den Start.
		rCommodity; rFrom; rTo; rLegal; rAutoload; rCargo; rPlace; rSort; rScu;
		if (phase === 'market' && mTab === 'routes') {
			void loadHangar(); // Schiff-Auswahl aus dem Hangar (SCU auto)
			queueRoutes();
		}
	});

	/** Hangar-Schiffe (keine Module) für die Schiff-Auswahl. */
	let hangarShips = $derived(hangarItems.filter((i) => i.entity_type !== 'module'));
	const DIST_LABELS = ['gleiches Terminal', 'gleicher Mond', 'gleicher Planet', 'gleiches System', 'anderes System'];

	function rMove(dir: number) {
		if (!routes_.length) return;
		rSel = Math.min(Math.max(rSel + dir, 0), routes_.length - 1);
		scrollRouteIntoView(rSel);
	}
	function scrollRouteIntoView(i: number) {
		const el = document.querySelector(`[data-route-idx="${i}"]`);
		el?.scrollIntoView({ block: 'nearest' });
	}
	function openRouteDetail() {
		if (routes_.length) rDetail = routes_[rSel] ?? null;
	}
	/** Gewinn = Einheit * Preisdifferenz * SCU — Plausibilitäts-Check im Smoke. */
	function routeMath(r: TradeRoute) {
		const scu = rScu || 1; // ohne Schiff/SCU-Angabe: Einzel-SCU rechnen
		const cost = r.buy_price * scu;
		const gross = r.sell_price * scu;
		return { cost, gross, profit: gross - cost };
	}

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
			// Persistierte Einstellungen (Palette + Detail-Variante) vor dem
			// ersten Paint anwenden — verhindert einen Palette-Flash.
			await loadSettings();
			if (label === 'spotlight') {
				isSpotlight = true;
				overlay = await invoke<OverlayState>('overlay_get_state');
				await listen<OverlayState>(OVERLAY_STATE_EVENT, (e) => {
					overlay = e.payload;
				});
				// Tray "Config öffnen": Fenster zeigen macht das Backend — hier
				// nur der Config-View (gleicher Pfad wie der Zahnrad-Button).
				await listen(overlayTrayOpenConfig, () => openConfig());
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
		// Platzhalter-Modi (Inventar/Hangar): das Haupt-Suchfeld ist dort inert —
		// erst zurück im Suche/Market-Modus treibt es wieder Suche/Offers.
		if (railMode) return;
		// N2: Im Detail ist das Suchfeld der In-Context-Filter — `input` fließt
		// über das dFilter-Derived direkt in die gefilterten Zeilen (kein
		// debounce, kein neuer search-Call). Erst zurück in der Hauptsuche
		// treibt das Feld wieder die Volltextsuche.
		if (inDetail) {
			dSel = 0;
			dChosen = null;
			dToast = '';
			return;
		}
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
		// N2: Suchfeld = In-Context-Filter der Detail-Ansicht → mit leerem Filter
		// starten (alles zeigen); Fokus bleibt im Suchfeld zum sofortigen Filtern.
		if (debounce) clearTimeout(debounce);
		input = '';
		focusSearch(false);
		void loadDetail(flat[idx]);
	}

	function closeDetail() {
		inDetail = false;
		detailHit = null;
		dSel = 0;
		dChosen = null;
		dToast = '';
		// N2: Filter zurücksetzen — das Feld ist in der Hauptsuche wieder die
		// Volltextsuche (leer, Ergebnisliste der letzten Suche bleibt stehen).
		if (debounce) clearTimeout(debounce);
		input = '';
		focusSearch(true);
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

	// Alt+Zahl fokussiert nur (Badge is-focus, NICHT aufklappen). Das Öffnen
	// übernimmt Enter auf der fokussierten Zeile (toggleOpen(focusIdx ?? sel))
	// bzw. Klick — NB-1 + QA-Kommentar 56.
	function focusOnly(idx: number) {
		focusRow(idx);
	}

	// Echten DOM-Fokus auf die Ergebniszeile legen (Spec §4 „Alt+Zahl fokussiert
	// das Ergebnis") — der Fokus-Zustand darf nicht nur CSS sein. `.row` ist über
	// `tabindex="-1"` fokussierbar, aber nicht im Tab-Order (kein Formular-Tab).
	function focusRow(idx: number) {
		focusIdx = idx;
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

	// ---- Esc-Toggle (Karte t_315a26cc: reine interne Zurück-Navigation) ------
	// Esc-Zustände: 1) Detail-Ansicht offen → zurück zur Liste + Fokus zurück;
	// 2) Ergebnis fokussiert (Alt+Zahl) oder eine Zeile geöffnet → zuklappen +
	//    Fokus ins Suchfeld; 3) Platzhalter-Modus → zurück zur Suche;
	// 4) Fokus woanders → Fokus ins Suchfeld (Select-All).
	// Im Root (Suchfeld fokussiert, Suche-Modus) tut Esc NICHTS mehr: das
	// Ausblenden des Overlays bleibt ausschließlich dem globalen Toggle
	// (Hyprland-Bind CTRL+SHIFT+U → arch-overlay toggle) vorbehalten — SC
	// verliert so nie Tasten an Esc-Restarbeit.
	function escSearch() {
		if (inDetail) {
			closeDetail();
			return;
		}
		if (railMode) {
			railMode = null;
			focusSearch(true);
			return;
		}
		if (focusIdx !== null || expanded !== null) {
			clampIdx();
			focusSearch(true);
			return;
		}
		const inSearch = searchEl ? document.activeElement === searchEl : phase !== 'typing';
		if (!inSearch) {
			// Fokus woanders (Ergebnis/Expansion/…) → Suchfeld (Select-All).
			focusSearch(true);
		}
		// Root + Suchfeld fokussiert: bewusst NICHTS — kein Overlay-hide
		// (isSpotlight-Zweig entfernt), kein Leeren. Global-Toggle only.
	}

	function toggleMarket() {
		if (phase === 'market') exitMarket();
		else enterMarket();
	}

	// Market-Modus betreten (F2 / M / Strg+M / Rail): P3 — wenn die Market-Daten
	// älter als 5 Minuten sind, sofortiger Sync im Hintergrund (kein Blocken).
	function enterMarket() {
		phase = 'market';
		const stale = mFetchedAt > 0 && nowSec() - mFetchedAt > MARKET_STALE_SECS;
		void loadMarket(stale ? 'bg' : 'normal');
	}

	// Market-Modus verlassen (Esc/F2/Tab/M/Rail): P5 — Rückweg hinterlässt
	// NICHTS: Eingabefeld leeren (im Market-Modus stand dort der Offer-Query),
	// Sortierung/Operation/Qualität auf Defaults, damit der nächste
	// Market-Besuch unbelastet startet.
	function exitMarket() {
		phase = 'typing';
		input = '';
		offerDetail = null;
		// Routen-Tab sauber verlassen (Kartenvorgabe: Market-Exit hinterlässt
		// nichts — nächster Besuch startet unbelastet).
		mTab = 'offers';
		routes_ = [];
		rDetail = null;
		rSel = 0;
		rNote = null;
		mSel = 0;
		mMshown = M_PAGE;
		mAllLoaded = false;
		mSort = 'newest';
		mOp = 'all';
		mqMin = '';
		mqMax = '';
		offers = [];
		mNote = null;
		groups = [];
		flat = [];
		groupOffsets = [];
		sel = 0;
		expanded = null;
		focusIdx = null;
		xpan = null;
		xpanToken++;
		calcResult = null;
		calcError = null;
		inDetail = false;
		detailHit = null;
		dSel = 0;
		dChosen = null;
		dToast = '';
		// Wieder im Such-Modus: Ergebnisliste an den Suchtext des HAUPT-
		// Suchfelds angleichen (im Market-Modus wurde dort der Offer-Query
		// getippt — keine veralteten Treffer stehen lassen).
		void runSearch();
	}

	// ---- Modi-Wechsel (F1/F2/F3/F4/F6/F12 + Rail-Klicks) ------------------------
	// Ein zentraler Einstieg: räumt den jeweiligen Alt-Zustand auf (Market-
	// Reset, Config schließen, Detail schließen) und behält den Tastaturfokus
	// im Suchfeld — wie bisher nach Market-Exit/Detail-Schließen.
	function setMode(m: Mode) {
		if (m === 'search') {
			railMode = null;
			if (inConfig) closeConfig();
			else if (phase === 'market') exitMarket();
			else if (inDetail) closeDetail();
			if (searchEl) searchEl.focus();
		} else if (m === 'market') {
			railMode = null;
			if (inConfig) closeConfig();
			if (phase !== 'market') {
				if (inDetail) closeDetail();
				enterMarket();
				if (searchEl) searchEl.focus();
			}
			mTab = 'offers'; // F2 = Angebots-Tab (F6 trägt Routen)
		} else if (m === 'routes') {
			// F6/Rail: Handelsrouten-Tab im Marktmodus (t_a39f6811 P1).
			// railMode lösen (t_04e3d8c6): sonst bleibt aus Hangar/Inventar
			// railMode gesetzt und mode $derived wechselt nie auf 'routes'.
			railMode = null;
			if (inConfig) closeConfig();
			if (phase !== 'market') {
				if (inDetail) closeDetail();
				enterMarket();
				if (searchEl) searchEl.focus();
			}
			mTab = 'routes';
		} else if (m === 'inventory' || m === 'hangar') {
			// Platzhalter-Modus: Overlay-States räumen (Detail schließen,
			// Market sauber verlassen), dann Platzhalter zeigen.
			if (inConfig) closeConfig();
			else if (phase === 'market') exitMarket();
			else if (inDetail) closeDetail();
			railMode = m;
			if (m === 'hangar') loadHangar();
			if (debounce) clearTimeout(debounce);
		} else {
			// settings → bestehender Config-View (Zahnrad-Pfad, settings_set_*-IPC).
			if (phase === 'market') exitMarket();
			else if (inDetail) closeDetail();
			railMode = null;
			openConfig();
		}
	}

	// Kern-Loader für den Marketplace (cache-first, progressive Limits).
	// `bg = true`: Hintergrund-Refresh (P3) — kein Spinner, UI bleibt bedienbar.
	async function fetchMarket(limit: number, showAll = false, bg = false) {
		if (showAll) mLoadingMore = true; else if (!bg) mLoading = true;
		mNote = null;
		const filterObj = {
			location: '',
			q: input.trim(),
			item_id: null,
			operation: mOp === 'all' ? null : mOp,
			region: '',
			sort: mSort,
			limit
		};
		try {
			const resp = await invoke<MarketResponse>('get_marketplace', { filters: filterObj });
			offers = resp.offers;
			mSource = resp.source;
			mNote = resp.note ?? null;
			mFetchedAt = resp.fetched_at || nowSec();
			persistMarketFetchedAt(mFetchedAt);
			// Der Provider liefert sein komplettes abrufbares Fenster — mehr
			// als diesen Snapshot gibt es vom Backend nicht nachzuholen.
			mMshown = showAll ? offers.length : Math.min(M_PAGE, offers.length);
			mAllLoaded = true;
		} catch (e) {
			offers = [];
			mNote = String(e);
			mSource = 'offline';
			mAllLoaded = true;
		}
		mLoading = false;
		mLoadingMore = false;
	}

	async function loadMarket(mode: 'normal' | 'bg' = 'normal') {
		mAllLoaded = false;
		await fetchMarket(100, false, mode === 'bg');
	}

	// P4: F5 (app-lokal) — kontextsensitiver Sync.
	//   Market-Modus → Market-Daten manuell syncen (Normal-Load mit Spinner,
	//   kein Hintergrund-Refresh — der User hat explizit angefragt).
	//   Normal/Detail-Modus → Commodity-Preise syncen (Backend `sync_now`).
	// Feedback NUR in der Statuszeile (f5Msg), nie modal.
	async function onF5() {
		flashStatus('Sync läuft …');
		if (phase === 'market') {
			await loadMarket();
			flashStatus(mNote ? `Market-Sync fehlgeschlagen · ${mNote}` : `Aktualisiert vor ${rel(mFetchedAt || nowSec())}`);
		} else {
			try {
				status = await invoke<SyncStatus>('sync_now');
				flashStatus(`Preise aktualisiert · Cache ${cacheAge(status)}`);
			} catch (e) {
				flashStatus(`Sync fehlgeschlagen · ${String(e)}`);
			}
		}
	}

	function flashStatus(t: string) {
		f5Msg = t;
		if (f5Clear) clearTimeout(f5Clear);
		f5Clear = setTimeout(() => (f5Msg = ''), 5000);
	}

	// Pfeil-Navigation durch die Offers (sichtbare = qualitätsgefilterte Liste).
	function mMove(dir: number) {
		if (!visOffers.length) return;
		mSel = Math.min(Math.max(mSel + dir, 0), visOffers.length - 1);
		scrollOfferIntoView(mSel);
	}

	// "Weitere laden": blättert im In-Memory-Snapshot (15 pro Seite) — der
	// Provider liefert je Fetch sein komplettes abrufbares Fenster, es gibt
	// nichts Progressives mehr nachzuholen.
	function loadMore() {
		if (mMshown < visOffers.length) {
			mMshown = Math.min(mMshown + M_PAGE, visOffers.length);
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
		dToast = `${r.kind === 'buy' ? 'Kauf' : 'Verkauf'} · ${fmt(r.kind === 'buy' ? r.p.buy_price : r.p.sell_price)} aUEC · ${r.name}`;
	}
	// Sync highlight-Klassen (is-sel/is-chosen) — deklarativ per class: im
	// Template gebunden (siehe .pr in der Detail-Tabelle), NICHT per DOM-Query:
	// manuell gesetzte Klassen überleben den Svelte-Flush nicht.
	// → Die Zeilen binden class:is-sel={i === dSel} / class:is-chosen={i === dChosen}
	//   über den flachen Index (Kauf-Zeilen zuerst, dann Verkauf).

	// QA Runde 2 (N3): rohen serde-/Rust-Fehlertext nicht als deutsche UI zeigen.
	// Bekannte Muster auf freundliche Meldungen mappen; Unbekanntes bleibt lesbar,
	// aber mit deutschem Rahmen statt technischem Fehler-String.
	function friendlyErr(e: unknown): string {
		const s = String(e ?? '');
		const lo = s.toLowerCase();
		if (!s) return 'Unbekanntes Problem beim Laden der Preise.';
		if (lo.includes('invalid type: null') || lo.includes('expected f64') || lo.includes('expected u'))
			return 'Preisdaten unvollständig — der Anbieter lieferte leere Werte. Bitte erneut versuchen.';
		if (lo.includes('requests limit reached') || lo.includes('rate limit'))
			return 'Anfragenlimit erreicht — bitte kurz warten und erneut versuchen.';
		if (lo.includes('not found'))
			return 'Keine Preisdaten für diesen Eintrag gefunden.';
		if (lo.includes('timeout') || lo.includes('timed out'))
			return 'Zeitüberschreitung beim Laden — bitte erneut versuchen.';
		if (lo.includes('connection failed') || lo.includes('network') || lo.includes('offline'))
			return 'Keine Verbindung zum Preisdienst. Bitte Internet prüfen.';
		// Fallback: bekannte Fehlermeldung, aber mit deutschem Kontext.
		return `Preise konnten nicht geladen werden (${s}).`;
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
		// ---- F-Tasten: In-App-Moduswechsel (Karte t_315a26cc) ----------------
		// F1 Suche · F2 Marktplatz · F6 Routen · F3 Inventar · F4 Hangar · F12 Einstellungen.
		// KEINE Hyprland-Binds — die Tasten greifen nur, solange das Overlay
		// sichtbar+fokussiert ist (compositor_focus beim Show): SC verliert nie
		// Tasten. F-Tasten erzeugen keine Zeichen, sie können also auch bei
		// Fokus im Suchfeld/Stepper nicht in Eingaben geraten; einzig gegen
		// contentEditable-Editoren (die F-Tasten selbst belegen können) wird
		// mit demselben Check geschützt wie die bestehenden m/o-Shortcuts.
		if (/^F(1|2|3|4|6|12)$/.test(k)) {
			if (
				typeof (e.target as HTMLElement | null)?.isContentEditable === 'boolean' &&
				(e.target as HTMLElement).isContentEditable
			) {
				return; // Rich-Text-Editor behält seine F-Tasten
			}
			if (k === 'F1') setMode('search');
			else if (k === 'F2') setMode('market');
			else if (k === 'F3') setMode('inventory');
			else if (k === 'F4') setMode('hangar');
			else if (k === 'F6') setMode('routes');
			else setMode('settings');
			e.preventDefault();
			return;
		}
		// Platzhalter-Modi (Inventar/Hangar): nur Esc/F-Tasten wirken — keine
		// Ergebnis-Navigation, kein Enter im Leeren.
		if (railMode) {
			// Inventar (t_1bab1fea) und Hangar (t_317f19c5) besitzen eigene
			// Key-Handler-Ketten: Esc geht an die jeweilige Komponente.
			if (railMode === 'hangar') {
				if (k === 'Escape') {
					hangarBack();
					e.preventDefault();
					return;
				}
				if (hangarView === 'list') {
					if (k === 'ArrowDown') {
						hangarSel = Math.min(hangarSel + 1, Math.max(hangarFiltered.length - 1, 0));
						e.preventDefault();
						return;
					}
					if (k === 'ArrowUp') {
						hangarSel = Math.max(hangarSel - 1, 0);
						e.preventDefault();
						return;
					}
					if (k === 'Enter') {
						const it = hangarFiltered[hangarSel];
						if (it) hangarOpenDetail(it);
						e.preventDefault();
						return;
					}
				}
			} else if (k === 'Escape') {
				railMode = null;
				focusSearch(true);
				e.preventDefault();
			}
			return;
		}
		// Config-View: Esc → zurück zur Suche (NICHT Overlay-hide). Alle anderen
		// Shortcuts (Nav/Alt-Zahlen/Ctrl+M/F5) sind in dieser Ansicht gesperrt —
		// Eingabe im API-Key-Feld bleibt natürlich erlaubt.
		if (inConfig) {
			if (k === 'Escape') {
				closeConfig();
				e.preventDefault();
			}
			return;
		}
		// P4: F5 ist app-lokal — nie Browser-Reload. Kontextsensitiv: Market-Modus
		// syncen Market-Daten, sonst Commodity-Preise (Feedback in der Fußzeile).
		if (k === 'F5') {
			void onF5();
			e.preventDefault();
			return;
		}
		// Marktplatz-Modus: Angebot-Navigation + Detail.
		if (phase === 'market') {
			const t = e.target;
			const inStepper = t instanceof HTMLInputElement && t.type === 'number';
			const onChip = t instanceof HTMLButtonElement;
			// Tab: ANGEBOTE ⇄ ROUTEN (separater Routen-Tab, Karte t_128b7547).
			if (k === 'Tab') {
				mTab = mTab === 'offers' ? 'routes' : 'offers';
				e.preventDefault();
				return;
			}
			if (mTab === 'routes') {
				if (k === 'Escape') {
					if (rDetail) rDetail = null;
					else toggleMarket();
					e.preventDefault();
					return;
				}
				if (rDetail) return; // Detail: keine Listen-Navigation
				if (k === 'ArrowDown' && !inStepper) {
					rMove(1);
					e.preventDefault();
					return;
				}
				if (k === 'ArrowUp' && !inStepper) {
					rMove(-1);
					e.preventDefault();
					return;
				}
				if (k === 'Enter' && !onChip && !inStepper) {
					openRouteDetail();
					e.preventDefault();
					return;
				}
				return; // Routen-Tab: keine Offer-Shortcuts
			}
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
			if ((k === 'm' || k === 'M') && !inStepper) {
				// P5: 'm' verlässt den Market-Modus symmetrisch zum Eintritt —
				// 'm' im Suchfeld tippt weiterhin normal. CTRL+M entfernt
				// (t_a39f6811 P2): Rail + F2 decken den Wechsel ab.
				if (isFieldFocused(e)) {
					return;
				}
				toggleMarket();
				e.preventDefault();
				return;
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

		// N2: Der alte 'L'-Ortssperren-Shortcut ist entfallen — im Detail filtert
		// das Suchfeld (siehe onInput/dFilter), 'l' tippt dort einfach mit.

		// Detail-Ansicht: ↑↓/↵/Esc.
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
			// CTRL+M entfernt (t_a39f6811 P2) — nur der Fokus-Check bleibt:
			// 'm' im Suchfeld tippt normal, außerhalb toggelt der Market.
			if (isFieldFocused(e)) {
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
	// Aufbau/Sortierung/Filterung läuft über dRowsFor/dLoc/dRows (oben, N2).

	// Kategorie-Icons = Präfixe lowercase im UI (Live-Feedback Punkt 1: das UI
	// zeigt Präfixe wie die Eingabe, nicht `:L`/`:C`-Großbuchstaben).
	const ICONS: Record<string, string> = { commodity: 'c', item: 'i', location: 'l' };
</script>

<svelte:window onkeydown={onKey} />

<main class="pal-root" bind:clientWidth={palW}>
	<div class="shell" class:in-detail={inDetail}>
		<i class="deco t1"></i><i class="deco t2"></i>

		<!-- Rail: linke Modus-Leiste (F1/F2/F3/F4/F6/F12) — gehört nur zum Overlay -->
		<Rail mode={mode} compact={palW > 0 && palW < 520} onselect={setMode} />

		<div class="shell-main">
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
				placeholder={railMode === 'inventory'
				? 'Inventar filtern — Standort oder Material …'
				: phase === 'market'
					? 'Marktplatz durchsuchen — Name / Typ / Terminal / Verkäufer …'
					: inDetail
					? detailHit?.kind === 'location' || detailHit?.kind === 'terminal'
						? `Filter: Waren in ${detailHit?.name} …`
						: `Filter: Orte mit Preisen für ${detailHit?.name} …`
					: 'Commodity, Item oder Terminal suchen …'}
				spellcheck="false"
				autocomplete="off"
			/>
			<span class="smode">{mode === 'market' ? 'MARKETPLACE' : mode === 'routes' ? 'HANDELSROUTEN' : mode === 'inventory' ? 'INVENTAR' : mode === 'hangar' ? 'HANGAR' : mode === 'settings' ? 'EINSTELLUNGEN' : 'SUCHE'}</span>
			<span class="esc">ESC</span>
		</div>

		<!-- Helper-Zeile (P7): nur Kategorie-Präfixe + math kurz. Alles andere
		     (Navigation, Filter, Refresh) lebt in der dynamischen Fußzeile (P6). -->
		{#if phase !== 'market' && !inDetail && !railMode}
			<div class="helper">
				<span><kbd>:c</kbd> commodities</span>
				<span class="sep">·</span>
				<span><kbd>:i</kbd> items</span>
				<span class="sep">·</span>
				<span><kbd>:l</kbd> locations</span>
				<span class="sep">·</span>
				<span><kbd>=</kbd> math</span>
			</div>
		{/if}

		<!-- Inhalt je Zustand -->
{#if railMode === 'inventory'}
			<!-- Inventar-Modus (t_1bab1fea): eigene SPA-Komponente; das HAUPT-
			     Suchfeld dient als In-Context-Filter (Standort/Material). -->
			<Inventory onexit={() => setMode('search')} filter={input} />
{:else if railMode === 'hangar'}
			<!-- Hangar (Karte t_317f19c5): Schiffe/Fahrzeuge/Module mit Versicherung.
			     Import: Hangar-XPLORer JSON + CCUgame CSV · Export: eigenes JSON.
			     Tastatur: ↑/↓ Auswahl · Enter Detail · Esc zurück/zur Suche. -->
			<div class="hangar" role="region" aria-label="Hangar">
				<header class="hangar-head">
					<button class="hangar-back" onclick={hangarBack} title="Zurück (Esc)" aria-label="Zurück">←</button>
					<span class="hangar-title">HANGAR</span>
					<div class="hangar-filters">
						{#each [['all', 'ALLE'], ['pledge', 'PLEDGE'], ['ingame', 'INGAME'], ['lti', 'LTI'], ['module', 'MODULE']] as [f, lbl] (f)}
							<button
								type="button"
								class="hchip"
								class:is-on={hangarFilter === f}
								onclick={() => { hangarFilter = f as typeof hangarFilter; hangarSel = 0; hangarView = 'list'; hangarDetail = null; }}
							>{lbl}</button>
						{/each}
					</div>
					<div class="hangar-actions">
						<button class="hbtn" disabled={hangarBusy} onclick={() => hangarTriggerImport('xplorer')}>Import XPLorer</button>
						<button class="hbtn" disabled={hangarBusy} onclick={() => hangarTriggerImport('ccugame')}>Import CCUgame</button>
						<button class="hbtn" disabled={hangarBusy} onclick={hangarExport}>Export</button>
						<button class="hbtn" disabled={hangarBusy} onclick={hangarOpenAdd} title="Eintrag manuell anlegen">+ Anlegen</button>
					</div>
				</header>
				{#if hangarNote}<div class="hangar-note">{hangarNote}</div>{/if}
				{#if hangarFormOpen && hangarView === 'list'}
					<form class="hangar-form" onsubmit={(e) => { e.preventDefault(); hangarSaveForm(); }}>
						<div class="hf-title">EINTRAG ANLEGEN</div>
						<div class="hf-grid">
							<label>Name<input bind:value={hangarForm.name} placeholder="z. B. Cutlass Black" required /></label>
							<label>Typ
								<select bind:value={hangarForm.entity_type}>
									<option value="ship">Schiff</option>
									<option value="vehicle">Fahrzeug</option>
									<option value="module">Modul</option>
								</select>
							</label>
							<label>Herkunft
								<select bind:value={hangarForm.origin}>
									<option value="ingame">Ingame</option>
									<option value="pledge">Pledge</option>
								</select>
							</label>
							<label>Versicherung<input bind:value={hangarForm.insurance} placeholder="LTI / 120 Monate / leer" /></label>
							<label>Custom-Name<input bind:value={hangarForm.custom_name} /></label>
							<label>Basisschiff (Modul)<input bind:value={hangarForm.base_ship} /></label>
							<label>Hersteller<input bind:value={hangarForm.manufacturer} /></label>
							<label>Preis (USD)<input bind:value={hangarFormPrice} inputmode="decimal" placeholder="z. B. 180" /></label>
							<label>Datum<input bind:value={hangarForm.purchase_date} placeholder="2025-01-31" /></label>
						</div>
						<div class="hf-actions">
							<button type="submit" class="hbtn" disabled={hangarBusy}>Speichern</button>
							<button type="button" class="hbtn" onclick={hangarCancelEdit}>Abbrechen</button>
						</div>
					</form>
				{/if}
				{#if hangarView === 'list' && !hangarFormOpen}
					<div class="hangar-list" role="listbox" aria-label="Hangar-Einträge">
						{#each hangarFiltered as it, i (it.id ?? `${it.name}#${i}`)}
							<button
								type="button"
								class="hrow"
								class:is-sel={i === hangarSel}
								onclick={() => { hangarSel = i; hangarOpenDetail(it); }}
							>
								<span class="hname">
									{hangarDisplayName(it)}
									{#if it.custom_name && it.custom_name !== it.name}<span class="hmodel">· {it.name}</span>{/if}
								</span>
								<span class="hchip o-{it.origin}">{it.origin === 'pledge' ? 'PLEDGE' : 'INGAME'}</span>
								<span class="hchip ins">{it.insurance ?? '—'}</span>
								<span class="hprice">{it.purchase_price != null ? `$${it.purchase_price.toFixed(2)}` : ''}</span>
							</button>
						{:else}
							<div class="hangar-empty">Keine Einträge{hangarFilter !== 'all' ? ` (Filter: ${hangarFilter.toUpperCase()})` : ''} — Import oben.</div>
						{/each}
					</div>
				{:else if hangarView === 'detail' && hangarDetail}
					<div class="hangar-detail">
						<div class="hd-head">
							<span class="hd-name">{hangarDisplayName(hangarDetail)}</span>
							<span class="hchip o-{hangarDetail.origin}">{hangarDetail.origin === 'pledge' ? 'PLEDGE' : 'INGAME'}</span>
							<span class="hchip ins">{hangarDetail.insurance ?? '—'}</span>
							{#if !hangarEdit}
								<button class="hbtn" disabled={hangarBusy} onclick={() => hangarStartEdit(hangarDetail!)}>Bearbeiten</button>
							{/if}
							<button class="hbtn danger" disabled={hangarBusy} onclick={() => hangarDelete(hangarDetail!)}>Löschen</button>
						</div>
						{#if hangarEdit}
							<form class="hangar-form" onsubmit={(e) => { e.preventDefault(); hangarSaveForm(); }}>
								<div class="hf-grid">
									<label>Name<input bind:value={hangarForm.name} required /></label>
									<label>Versicherung<input bind:value={hangarForm.insurance} placeholder="LTI / 120 Monate / leer" /></label>
									<label>Custom-Name<input bind:value={hangarForm.custom_name} /></label>
									<label>Basisschiff (Modul)<input bind:value={hangarForm.base_ship} /></label>
									<label>Preis (USD)<input bind:value={hangarFormPrice} inputmode="decimal" /></label>
									<label>Datum<input bind:value={hangarForm.purchase_date} /></label>
								</div>
								<div class="hf-actions">
									<button type="submit" class="hbtn" disabled={hangarBusy}>Speichern</button>
									<button type="button" class="hbtn" onclick={hangarCancelEdit}>Abbrechen</button>
								</div>
							</form>
						{:else}
						<dl class="hd-grid">
							<div><dt>Modell</dt><dd>{hangarDetail.name}</dd></div>
							<div><dt>Hersteller</dt><dd>{hangarDetail.manufacturer || '—'}</dd></div>
							<div><dt>Ship-Code</dt><dd>{hangarDetail.ship_code || '—'}</dd></div>
							<div><dt>Typ</dt><dd>{hangarDetail.entity_type}</dd></div>
							<div><dt>Pledge</dt><dd>{hangarDetail.pledge_name || '—'}</dd></div>
							<div><dt>Pledge-ID</dt><dd>{hangarDetail.pledge_id || '—'}</dd></div>
							<div><dt>Datum</dt><dd>{hangarDetail.purchase_date || '—'}</dd></div>
							<div><dt>Preis</dt><dd>{hangarDetail.purchase_price != null ? `$${hangarDetail.purchase_price.toFixed(2)}` : '—'}</dd></div>
							<div><dt>Warbond</dt><dd>{hangarDetail.warbond ? 'Ja' : 'Nein'}</dd></div>
							<div><dt>CCUD</dt><dd>{hangarDetail.ccud ? 'Ja' : 'Nein'}</dd></div>
						</dl>
						{#if hangarDetail.entity_type !== 'module'}
							<div class="hd-mods">
								<div class="h">Module ({hangarModulesOf(hangarDetail.name).length})</div>
								{#each hangarModulesOf(hangarDetail.name) as m (m.id ?? m.name)}
									<div class="hmod">
										<span>{hangarDisplayName(m)}</span>
										<span class="hchip ins">{m.insurance ?? '—'}</span>
									</div>
								{:else}
									<div class="hmodempty">Keine Module zugeordnet.</div>
								{/each}
							</div>
						{/if}
						{/if}
					</div>
				{/if}
			</div>
		{:else if inConfig}
			<!-- Config-View: eigenes View in der Palette (wie die Detail-"Seite").
			     Esc/zurück-Pfeil → zurück zur Suche; kein neues Fenster. -->
			<div class="pane-config" role="dialog" aria-label="Konfiguration">
				<div class="cfg-head">
					<button
						type="button"
						class="cfg-back"
						onclick={closeConfig}
						title="Zurück zur Suche (Esc)"
						aria-label="Zurück zur Suche"
					>←</button>
					<div class="cfg-title">KONFIGURATION</div>
					<span class="cfg-live">wirkt sofort</span>
				</div>
				<div class="cfg-body">
					<section class="cfg-sect">
						<div class="h">Farbpalette</div>
						<div class="cfg-chips">
							<button
								type="button"
								class="chip"
								class:is-on={cfg?.palette === 'amber'}
								onclick={() => choosePalette('amber')}
							>Amber</button>
							<button
								type="button"
								class="chip"
								class:is-on={cfg?.palette === 'mfcd'}
								onclick={() => choosePalette('mfcd')}
							>MFCD</button>
							<button
								type="button"
								class="chip"
								class:is-on={cfg?.palette === 'bloom'}
								onclick={() => choosePalette('bloom')}
							>Bloom</button>
						</div>
					</section>
					<section class="cfg-sect">
						<div class="h">Preis-Detail-Variante</div>
						<div class="cfg-chips">
							<button
								type="button"
								class="chip"
								class:is-on={cfg?.detail_variant === 'c'}
								onclick={() => chooseVariant('c')}
							>C · Standard</button>
							<button
								type="button"
								class="chip"
								class:is-on={cfg?.detail_variant === 'a'}
								onclick={() => chooseVariant('a')}
							>A</button>
						</div>
					</section>
					<section class="cfg-sect">
						<div class="h">Preis-Quelle (A/B)</div>
						<div class="cfg-chips">
							<button
								type="button"
								class="chip"
								class:is-on={cfg?.commodity_source === 'wiki'}
								onclick={() => chooseSource('wiki')}
							>Wiki</button>
							<button
								type="button"
								class="chip"
								class:is-on={cfg?.commodity_source === 'uex'}
								onclick={() => chooseSource('uex')}
							>UEX</button>
						</div>
					</section>
					<section class="cfg-sect">
						<div class="h">Account-Verknüpfung (bald)</div>
						<div class="cfg-locked" aria-disabled="true">
							Kommt später — UEX-Account-Verknüpfung ist noch nicht verfügbar.
						</div>
					</section>
					<section class="cfg-sect">
						<div class="h">UEX API-Key</div>
						<div class="cfg-key-status">
							Status:
							<b class:ok={cfg?.uex_api_key_set} class:off={!cfg?.uex_api_key_set}>
								{cfg?.uex_api_key_set ? 'gesetzt' : 'nicht gesetzt'}
							</b>
						</div>
						<div class="cfg-key-row">
							<input
								type="password"
								autocomplete="off"
								placeholder="Key einfügen … (maskiert)"
								bind:value={apiKeyInput}
								onkeydown={(e) => e.stopPropagation()}
							/>
							<button type="button" class="btn" onclick={saveApiKey} disabled={!apiKeyInput.trim()}>
								Setzen
							</button>
							{#if cfg?.uex_api_key_set}
								<button type="button" class="btn-ghost" onclick={removeApiKey}>Entfernen</button>
							{/if}
						</div>
						{#if apiKeyNote}
							<div class="cfg-note">{apiKeyNote}</div>
						{/if}
					</section>
				</div>
			</div>
		{:else if phase === 'loading'}
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
			{#if !offerDetail}
				<div class="mtabs" role="tablist">
					<button type="button" role="tab" aria-selected={mTab === 'offers'} class="chip" class:is-on={mTab === 'offers'} onclick={() => (mTab = 'offers')}>Angebote</button>
					<button type="button" role="tab" aria-selected={mTab === 'routes'} class="chip" class:is-on={mTab === 'routes'} onclick={() => (mTab = 'routes')}>Handelsrouten</button>
				</div>
			{/if}
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
				{:else if mTab === 'routes'}
					{#if rDetail}
						<div class="rdetail" role="dialog" aria-modal="true" aria-label="Routen-Detail">
							<div class="md-head">
								<div class="md-t">
									<h2>{rDetail.commodity} {rDetail.illegal ? '⚠ illegal' : ''}</h2>
									<span class="tag" style="color: var(--fgdim)">Distanz: {DIST_LABELS[rDetail.distance] ?? rDetail.distance}</span>
								</div>
								<div class="md-price">
									<div class="v sell">+{fmtPrice(rDetail.profit_total)}</div>
									<div class="u">aUEC Gewinn bei {rScu || 1} SCU</div>
								</div>
							</div>
							<div class="rd-leg">
								<div class="rd-stop">
									<div class="rd-cap">KAUF</div>
									<div class="rd-term">{rDetail.buy_terminal}</div>
									<div class="rd-chain">{rDetail.buy_chain}</div>
									<div class="rd-price">{fmtPrice(rDetail.buy_price)} aUEC / SCU</div>
									<div class="rd-flags">
										{#if rDetail.buy_terminal_autoload}<span class="chip">Autoload</span>{/if}
										{#if rDetail.buy_terminal_cargo_deck}<span class="chip">Cargodeck</span>{/if}
										{#if rDetail.buy_terminal_ground}<span class="chip">Boden</span>{:else}<span class="chip">Space</span>{/if}
									</div>
								</div>
								<div class="rd-arrow">→</div>
								<div class="rd-stop">
									<div class="rd-cap">VERKAUF</div>
									<div class="rd-term">{rDetail.sell_terminal}</div>
									<div class="rd-chain">{rDetail.sell_chain}</div>
									<div class="rd-price">{fmtPrice(rDetail.sell_price)} aUEC / SCU</div>
									<div class="rd-flags">
										{#if rDetail.sell_terminal_autoload}<span class="chip">Autoload</span>{/if}
										{#if rDetail.sell_terminal_cargo_deck}<span class="chip">Cargodeck</span>{/if}
										{#if rDetail.sell_terminal_ground}<span class="chip">Boden</span>{:else}<span class="chip">Space</span>{/if}
									</div>
								</div>
							</div>
							<div class="rd-math">
								<span><b>SCU</b> {rScu || 1}</span>
								<span><b>Einkauf</b> {fmtPrice(routeMath(rDetail).cost)} aUEC</span>
								<span><b>Brutto</b> {fmtPrice(routeMath(rDetail).gross)} aUEC</span>
								<span><b>Netto-Gewinn</b> {fmtPrice(routeMath(rDetail).profit)} aUEC</span>
								<span><b>Marge</b> {(rDetail.margin * 100).toFixed(1)} %</span>
								<span><b>Bestand dort</b> {fmtPrice(rDetail.sell_stock)} SCU</span>
							</div>
							<div class="md-note">Preisstand: {rSweptAt ? new Date(rSweptAt * 1000).toLocaleString('de-DE') : '—'} · Quelle: UEX</div>
							<div class="md-actions">
								<span class="btn-ghost" onclick={() => (rDetail = null)}>Zurück (Esc)</span>
							</div>
						</div>
					{:else}
						<div class="mbar2">
							<span class="m-lbl">Schiff</span>
							<select class="qnum rsel" aria-label="Schiff aus Hangar" onchange={(e) => applyShipScu(e.currentTarget.value)}>
								<option value="">— manuell —</option>
								{#each hangarShips as s (s.id)}
									<option value={s.name}>{s.custom_name || s.name}</option>
								{/each}
							</select>
							<input class="qnum" type="number" min="0" placeholder="SCU" aria-label="SCU-Kapazität" bind:value={rScu} />
							<input class="rtext" placeholder="Ware" aria-label="Ware filtern" bind:value={rCommodity} />
							<input class="rtext" placeholder="Startort" aria-label="Startort filtern" bind:value={rFrom} />
							<input class="rtext" placeholder="Zielort" aria-label="Zielort filtern" bind:value={rTo} />
						</div>
						<div class="mbar2">
							<button type="button" class="chip" class:is-on={rLegal === 'all'} onclick={() => (rLegal = 'all')}>Alle</button>
							<button type="button" class="chip" class:is-on={rLegal === 'legal'} onclick={() => (rLegal = 'legal')}>Legal</button>
							<button type="button" class="chip" class:is-on={rLegal === 'illegal'} onclick={() => (rLegal = 'illegal')}><span class="dot"></span>Illegal</button>
							<span class="m-lbl">Autoload</span>
							<button type="button" class="chip" class:is-on={rAutoload === 'any'} onclick={() => (rAutoload = 'any')}>Egal</button>
							<button type="button" class="chip" class:is-on={rAutoload === 'yes'} onclick={() => (rAutoload = 'yes')}>Ja</button>
							<button type="button" class="chip" class:is-on={rAutoload === 'no'} onclick={() => (rAutoload = 'no')}>Nein</button>
							<span class="m-lbl">Cargodeck</span>
							<button type="button" class="chip" class:is-on={rCargo === 'any'} onclick={() => (rCargo = 'any')}>Egal</button>
							<button type="button" class="chip" class:is-on={rCargo === 'yes'} onclick={() => (rCargo = 'yes')}>Ja</button>
							<button type="button" class="chip" class:is-on={rCargo === 'no'} onclick={() => (rCargo = 'no')}>Nein</button>
							<span class="m-lbl">Ort</span>
							<button type="button" class="chip" class:is-on={rPlace === 'any'} onclick={() => (rPlace = 'any')}>Alle</button>
							<button type="button" class="chip" class:is-on={rPlace === 'ground'} onclick={() => (rPlace = 'ground')}>Boden</button>
							<button type="button" class="chip" class:is-on={rPlace === 'space'} onclick={() => (rPlace = 'space')}>Space</button>
						</div>
						<div class="mbar2">
							<span class="m-lbl">Sort</span>
							<button type="button" class="chip" class:is-on={rSort === 'profit_desc'} onclick={() => (rSort = 'profit_desc')}>Gewinn/SCU ↓</button>
							<button type="button" class="chip" class:is-on={rSort === 'profit_asc'} onclick={() => (rSort = 'profit_asc')}>Gewinn/SCU ↑</button>
							<button type="button" class="chip" class:is-on={rSort === 'total_profit_desc'} onclick={() => (rSort = 'total_profit_desc')}>Profit absolut ↓</button>
							<button type="button" class="chip" class:is-on={rSort === 'margin_desc'} onclick={() => (rSort = 'margin_desc')}>Marge ↓</button>
							<button type="button" class="chip" class:is-on={rSort === 'distance_asc'} onclick={() => (rSort = 'distance_asc')}>Distanz ↑</button>
							<span class="m-spacer"></span>
							{#if rSweptAt}<span class="chip"><span class="dot"></span>Stand {new Date(rSweptAt * 1000).toLocaleTimeString('de-DE')}</span>{/if}
							{#if rRefreshing}<span class="chip">↻ Updating…</span>{/if}
						</div>
						{#if rLoading && !routes_.length}
							<div class="center" style="padding: 28px 16px">
								<span class="spin"></span>
								<div class="big" style="margin-top: 12px">Sweepe UEX-Terminals …</div>
								<div class="sm">Erster Lauf ~1 min, danach aus dem Cache.</div>
							</div>
						{:else if rNote && !routes_.length}
							<div class="row-empty" style="padding: 28px 16px">{rNote}</div>
						{:else if routes_.length}
							<div class="mlist">
								{#each routes_ as r, i (r.commodity_id + '-' + r.buy_terminal_id + '-' + r.sell_terminal_id)}
									<div
										class="mrow rrow"
										role="option"
										aria-selected={i === rSel}
										class:is-sel={i === rSel}
										data-route-idx={i}
										onclick={() => { rSel = i; openRouteDetail(); }}
									>
										<div class="m-main">
											<div class="m-title">{r.commodity} {r.illegal ? '⚠' : ''}</div>
											<div class="m-sub">{r.buy_terminal} → {r.sell_terminal} · {DIST_LABELS[r.distance] ?? r.distance}</div>
										</div>
										<div class="m-right">
											<div class="m-price sell">+{fmtPrice(r.profit_per_scu)}</div>
											<div class="m-meta">{fmtPrice(r.profit_total)} aUEC · Marge {(r.margin * 100).toFixed(0)} %</div>
										</div>
									</div>
								{/each}
							</div>
							<div class="m-foot">
								<span>{routes_.length} Routen · Gewinn/SCU = Verkauf − Kauf</span>
								<span class="src">UEX Corp</span>
							</div>
						{:else}
							<div class="row-empty" style="padding: 28px 16px">Keine Routen für diese Filter.</div>
						{/if}
					{/if}
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
				{:else if flat.length && ((input as string).trim() || searchTotal > 0)}
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
											<span class="ic">{ICONS[g.kind] ?? '?'}</span>
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
									<div class="sm">{friendlyErr(xpan.error)}</div>
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
										<span class="buy"><i>Best Kauf</i>{dBest.buy === Infinity ? '—' : fmt(dBest.buy)}</span>
										<span class="sell"><i>Best Verkauf</i>{dBest.sell > 0 ? fmt(dBest.sell) : '—'}</span>
									</div>
									<div class="d-variant" role="group" aria-label="Preis-Detail-Darstellung">
										<button class:active={detailVariant === 'a'} onclick={() => { localVariant = 'a'; void chooseVariant('a'); }} title="Variante A — Zeilenboxen">A</button>
											<button class:active={detailVariant === 'c'} onclick={() => { localVariant = 'c'; void chooseVariant('c'); }} title="Variante C — Trader-Hero (Default)">C</button>
									</div>
								</div>

								{#if dFilter}
									<div class="lock-hint"><span class="dot"></span>Filter aktiv: <b>{dFilter}</b> · {dRows.length} Zeilen · Feld leeren = alles zeigen</div>
								{/if}

								{#if dToast}
									<div class="d-toast" role="status">{dToast} <b>↵ Auswahl</b></div>
								{/if}

								<div class="d-body">
									{#if d.location}
										<!-- Location-Aggregat: je Terminal eine Gruppe (N2: über dLoc gefiltert) -->
										{#each dLoc as grp}
											<div class="ptab locgrp">
												<div class="cap">{grp.terminal_name}</div>
												{#if grp.buys.length || grp.sells.length}
													<div class="ph"><span>Item</span><span>Preis</span><span>SCU</span></div>
												<!-- P8: leere Richtungs-Sektionen entfallen — nur vorhandene Richtungen bekommen Sektionen. -->
												{#if grp.buys.length}
													<div class="psub buy">Kauf</div>
													{#each grp.buys as r}
														<div class="pr" class:best={r === grp.buys[0]} class:is-sel={r.fi === dSel} class:is-chosen={r.fi === dChosen}>
															<span class="loc">{r.name}<small>{stockTxt(r.p, 'buy')}</small></span>
															<span class="val buy">{fmt(r.p.buy_price)}</span><span class="unit">aUEC</span>
														</div>
													{/each}
												{/if}
												{#if grp.sells.length}
													<div class="psub sell">Verkauf</div>
													{#each grp.sells as r}
														<div class="pr" class:best={r === grp.sells[0]} class:is-sel={r.fi === dSel} class:is-chosen={r.fi === dChosen}>
															<span class="loc">{r.name}<small>{stockTxt(r.p, 'sell')}</small></span>
															<span class="val sell">{fmt(r.p.sell_price)}</span><span class="unit">aUEC</span>
														</div>
													{/each}
												{/if}
												{:else}
													<div class="pr"><span class="loc muted">keine Preisdaten</span></div>
												{/if}
											</div>
										{/each}
										{#if !dLoc.length}
											<div class="np-state">Kein Terminal/Keine Ware passt zum Filter</div>
										{/if}
									{:else if d.prices.length === 0}
										<!-- QA-Auflage B: Item ohne Preise → "nicht kaufbar" -->
										<div class="np-state">nicht kaufbar · keine Preisdaten</div>
									{:else}
										<!-- P8: Commodity / Terminal mit Preisen (N2: über dRows gefiltert). -->
										<!-- Variante C (Default): Bestpreis-Hero + kompakte Terminal-Liste; -->
										<!-- Variante A: gruppierte Kauf-/Verkauf-Boxen. Leere Richtungen werden -->
										<!-- in BEIDEN Varianten durch EINE Statuszeile ersetzt (kein Spaltenkopf). -->
										{@const dIsTerminal = detailHit?.kind === 'terminal'}
										{@const buys = dRows.filter((r) => r.kind === 'buy')}
										{@const sells = dRows.filter((r) => r.kind === 'sell')}
										{@const bb = buys[0]}
										{@const bs = sells[0]}
										{@const margin = bb && bs ? bs.p.sell_price - bb.p.buy_price : 0}
										{#if detailVariant !== 'a'}
											<div class="vC-hero">
												<div class="vC-kpi buy">
													<span class="k">Günstigster Kauf</span>
													<span class="v">{bb ? fmt(bb.p.buy_price) : '—'}</span>
													{#if bb}<span class="u">aUEC / SCU</span>{/if}
													<span class="w">{bb ? `${bb.name} · ${stockTxt(bb.p, 'buy')}` : 'kein Kauf-Angebot'}</span>
												</div>
												<div class="vC-kpi sell">
													<span class="k">Höchster Verkauf</span>
													<span class="v">{bs ? fmt(bs.p.sell_price) : '—'}</span>
													{#if bs}<span class="u">aUEC / SCU</span>{/if}
													<span class="w">{bs ? `${bs.name} · ${stockTxt(bs.p, 'sell')}` : 'kein Verkauf-Angebot'}</span>
												</div>
												<div class="vC-kpi margin">
													<span class="k">Marge / SCU</span>
													<span class="v">{bb && bs ? fmt(margin) : '—'}</span>
													{#if bb && bs}<span class="u">aUEC / SCU · {fmt(margin * 100)} je 100 SCU</span>{/if}
													<span class="w">{bb && bs ? `${bb.name} → ${bs.name}` : 'nur eine Seite vorhanden — kein Rundlauf möglich'}</span>
												</div>
											</div>
											<div class="vC-listhead"><span>{dIsTerminal ? 'Item' : 'Terminal'}</span><span>Kauf</span><span>Verkauf</span></div>
											{#each dRows as r}
												<div class="pr vC-r" class:is-sel={r.fi === dSel} class:is-chosen={r.fi === dChosen}>
													<span class="vC-t">{r.name}<small>{stockTxt(r.p, r.kind)}</small></span>
													{#if r.kind === 'buy'}
														<span class="vC-c buy">{fmt(r.p.buy_price)}</span>
														<span class="vC-c nil">–</span>
													{:else}
														<span class="vC-c nil">–</span>
														<span class="vC-c sell">{fmt(r.p.sell_price)}</span>
													{/if}
												</div>
											{/each}
											{#if !dRows.length}
												<div class="np-state">Keine Preise passen zum Filter</div>
											{/if}
										{:else}
											{#if buys.length}
												<div class="ptab">
													<div class="cap">Alle <b class="buy">Kauf</b>-Preise · {buys.length} {dIsTerminal ? 'Artikel' : 'Terminal'}</div>
													<div class="ph"><span>{dIsTerminal ? 'Item' : 'Terminal'}</span><span>Preis</span><span>SCU</span></div>
													{#each buys as r, bi}
														<div class="pr" class:best={bi === 0} class:is-sel={r.fi === dSel} class:is-chosen={r.fi === dChosen}>
															<span class="loc">{r.name}<small>{stockTxt(r.p, 'buy')}</small></span>
															<span class="val buy">{fmt(r.p.buy_price)}</span><span class="unit">aUEC</span>
														</div>
													{/each}
												</div>
											{:else}
												<div class="np-state">{dIsTerminal ? 'Kein Artikel zum Kauf' : 'Kein Terminal kauft diese Ware'}</div>
											{/if}
											{#if sells.length}
												<div class="ptab">
													<div class="cap">Alle <b class="sell">Verkauf</b>-Preise · {sells.length} {dIsTerminal ? 'Artikel' : 'Terminal'}</div>
													<div class="ph"><span>{dIsTerminal ? 'Item' : 'Terminal'}</span><span>Preis</span><span>SCU</span></div>
													{#each sells as r, si}
														<div class="pr" class:best={si === 0} class:is-sel={r.fi === dSel} class:is-chosen={r.fi === dChosen}>
															<span class="loc">{r.name}<small>{stockTxt(r.p, 'sell')}</small></span>
															<span class="val sell">{fmt(r.p.sell_price)}</span><span class="unit">aUEC</span>
														</div>
													{/each}
												</div>
											{:else}
												<div class="np-state">{dIsTerminal ? 'Kein Artikel zum Verkauf' : 'Kein Terminal verkauft diese Ware'}</div>
											{/if}
										{/if}
								{/if}
								</div>

								<!-- P6: keine Extra-Hinweiszeile über der Fußzeile — die
								     dynamische Fußzeile unten ist die einzige Hint-Zeile. -->
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
								<span class="r"><b>F2</b> Marktplatz · <b>M</b></span>
								<span class="r"><b>F12</b> Einstellungen</span>
								<span class="r"><b>Rechner</b> · =2*250+15%</span>
								<span class="r"><b>Kategorie</b> · :c gold</span>
							</div>
					</div>
				{/if}
			</div>
		{/if}

		<!-- Fußzeile (bleibt in allen Zuständen stehen → kein Layoutsprung) —
		     EINZIGE dynamische Hint-Zeile (P6): Inhalt wechselt je Ansicht
		     (Search / Detail / Market) und trägt das F5-Sync-Feedback (P4). -->
		<div class="foot">
			<div class="hints">
				{#if phase === 'market'}
					{#if offerDetail}
						<span><kbd>Esc</kbd> zurück zur Liste</span>
						<span><kbd>↵</kbd> im Browser öffnen</span>
					{:else}
						<span><kbd>↑↓</kbd> Angebote</span>
						<span><kbd>Alt+1..0</kbd> Offer</span>
						<span><kbd>↵</kbd> Detail</span>
						<span><kbd>Tab</kbd> Weitere laden</span>
						<span><kbd>F5</kbd> Market syncen</span>
						<span><kbd>Esc</kbd> zurück zur Suche</span>
					{/if}
				{:else if inDetail}
					{#if dRows.length}
						<span><kbd>↑↓</kbd> Zeile ({dSel + 1}/{dRows.length})</span>
						<span><kbd>↵</kbd> Auswahl</span>
					{/if}
					<span>Suchfeld = Filter</span>
					<span><kbd>F5</kbd> Preise syncen</span>
					<span><kbd>Esc</kbd> zurück</span>
				{:else}
					<span><kbd>↑↓</kbd> Nav</span>
					<span><kbd>Alt+1..0</kbd> Treffer</span>
					<span><kbd>↵</kbd> Detail</span>
					<span><kbd>Esc</kbd> zurück</span>
					<span><kbd>M</kbd> Market</span>
					<span><kbd>F5</kbd> Preise syncen</span>
				{/if}
			</div>
			{#if f5Msg}
				<span class="f5-msg" role="status">{f5Msg}</span>
			{/if}
			<span class="sync {syncLabel().cls}"><span class="dot"></span>{syncLabel().text}</span>
			<!-- Zahnrad entfernt (t_a39f6811 P2): Rail + F12 decken die Konfiguration ab. -->
			{#if isSpotlight}
				<div class="spot-status">
					<span class="dot" class:live={overlay?.game_running}></span>
					<button onclick={toggleClickThrough}>
						{overlay?.click_through ? 'KLICK' : 'FOKUS'}
					</button>
				</div>
			{/if}
		</div>
		</div><!-- /shell-main -->
	</div>
</main>
