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
	// Modi (Karte t_315a26cc): Mode-Hotkeys konfigurierbar (PB t_1c9bc240) —
	// Defaults: F1 Suche · F2 Marktplatz · F3 Routen · F4 Inventar · F11 Hangar ·
	// F12 Einstellungen — In-App-Keyhandler, die nur greifen, wenn
	// das Overlay sichtbar+fokussiert ist (compositor_focus beim Show); die Rail
	// links zeigt die Modi mit F-Key-Label, aktiver Modus in Akzentfarbe.
	// Market-Modus (F2 / Rail): DAS Haupt-Suchfeld durchsucht Offers
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
	// PB (t_1c9bc240, B1): Mode-Hotkeys — Defaults, Validação e Lookup em UM módulo.
	import {
		DEFAULT_MODE_HOTKEYS,
		MODE_HOTKEY_RESERVED,
		hotkeyFor,
		modeKeyMap,
		normalizeFkey,
		validateModeHotkeys,
		type ModeHotkeys
	} from '../lib/hotkeys';
	import Inventory from '../lib/Inventory.svelte';
	import RoutesTypeahead from '../lib/RoutesTypeahead.svelte';
	// MD (t_10be9255, D2): dunkles, mausbedienbares Dropdown mit Typeahead —
	// ersetzt die nativen <select> im Anlegen-Formular.
	import SelectCombo from '../lib/SelectCombo.svelte';
	// Hangar-Autocomplete (P10, Karte t_aa6cc070): static ship/vehicle catalogue
	// + Snub fixed list (no live ship-list endpoint: UEX 2.0 + Wiki API checked).
	import { suggestShips, isSnubShip } from '../lib/hangar_ships';

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
		// M3a: CDN-Bild-URLs (Thumbnail-Variante -t.jpg) + Freigabe-Status
		// (date_approved > 0; pending erscheint nur im eigenen Secret-Key-Pull).
		photos: string[];
		approved: boolean;
	};
	type MarketResponse = {
		source: 'cache' | 'live' | 'offline';
		fetched_at: number;
		offers: Offer[];
		note: string | null;
	};

	// M2a (Read-Phase): „Mein Shop" — eigene Offers + Marktplatz-Deals.
	// t_350c4300 B2: die Trade-Quelle sind die MARKETPLACE-Negotiations
	// (`/marketplace_negotiations` — Listing-Verhandlungen/-Abschlüsse des
	// Accounts als Anbieter ODER Kunde). Der frühere /user_trades-Commodity-
	// Verlauf (Agricium etc.) ist Cargo-Handel und gehört hier NICHT hin.
	// Mirrors provider::Negotiation (seam-DTO) + IPC-Response.
	type Negotiation = {
		id: number;
		listing_id: number;
		hash: string;
		price: number;
		unit: string;
		currency: string;
		deal_value: number | null;
		deal_value_currency: string;
		listing_title: string;
		listing_slug: string;
		advertiser_name: string;
		advertiser_username: string;
		client_name: string;
		client_username: string;
		is_listing_advertiser: boolean;
		date_added: number;
		date_modified: number;
		date_closed: number | null;
	};
	type OwnOffersResponse = {
		source: 'cache' | 'live' | 'offline';
		fetched_at: number;
		offers: Offer[];
		note: string | null;
		key_set: boolean;
	};
	type NegotiationsResponse = {
		source: 'cache' | 'live' | 'offline';
		fetched_at: number;
		negotiations: Negotiation[];
		note: string | null;
		key_set: boolean;
	};
	// M3b (t_893761f1): Deal-Chat — Mirrors provider::NegotiationMessage +
	// IPC-Responses. `event`/`message` sind null-bar (System-Event vs. Text).
	type NegotiationMessage = {
		id: number;
		id_listing: number;
		id_negotiation: number;
		event: string | null;
		message: string | null;
		listing_title: string;
		listing_slug: string;
		negotiation_hash: string;
		user_name: string;
		user_username: string;
		api_name: string;
		date_added: number;
		date_read: number | null;
	};
	type MessagesResponse = {
		source: 'cache' | 'live' | 'offline';
		fetched_at: number;
		messages: NegotiationMessage[];
		note: string | null;
		key_set: boolean;
	};
	type MessageSendResponse = { id_message: number };
	const SHOP_USER_KEY = 'architects-overlay.shop.username';
	function loadShopUser(): string {
		try {
			return (localStorage.getItem(SHOP_USER_KEY) || '').trim();
		} catch {
			return '';
		}
	}

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
		// M2a-Fix (t_2f197d1c): bool-only (roher Key geht nie ans Frontend)
		// + UEX-Benutzername für den marketplace_listings?username=-Pull.
		uex_secret_key_set: boolean;
		uex_username: string;
		commodity_source: string;
		// K2 (t_5fe8eb4c, P12): font/icon scale in % (Chips 80–125, backend 50–200).
		scale: number;
		// C2a (t_6565fe8b): kanonische Kürzel (Soll — persistiert) + Ist
		// (registriert das OS die Hotkeys wirklich — Wayland: nein).
		shortcut_toggle: string;
		shortcut_click_through: string;
		global_shortcuts_active: boolean;
		// C2b (t_1adbc017): Consent (persistiert) + Compositor-Ist + Sync-Note.
		shortcuts_sync_omarchy: boolean;
		// PB (t_1c9bc240, B1): Mode-Hotkeys persistidos (null = SSR/Probe/Config
		// antiga → Defaults aus lib/hotkeys.ts = Migration zur neuen Belegung).
		mode_hotkeys: ModeHotkeys | null;
		// M2b: 2-Schritt-Bestätigung für Marktplatz-Writes (every_action|delete_only).
		marketplace_confirm: string;
		hyprland_session: boolean;
		omasettings_available: boolean;
		sync_note: string;
	};
	let inConfig = $state(false);
	let cfg = $state<SettingsSnapshot | null>(null);
	let apiKeyInput = $state('');
	let apiKeyNote = $state('');
	// M2a-Fix (t_2f197d1c): zweites Credential + Username (F12-View).
	let secretKeyInput = $state('');
	let secretKeyNote = $state('');
	let usernameInput = $state('');
	let usernameNote = $state('');
	// K6 (t_9b385831, P2): kurze Confirm-Zeile im Config-View nach Aktionen
	// (Preisquelle umstellen) — analog K3 Notif-Timeout: verschwindet nach 3 s
	// von selbst, damit die Wirkung sichtbar ist, ohne das View zuzumüllen.
	let cfgToast = $state('');
	let cfgToastTimer: ReturnType<typeof setTimeout> | undefined;
	function cfgConfirm(msg: string) {
		cfgToast = msg;
		if (cfgToastTimer) clearTimeout(cfgToastTimer);
		cfgToastTimer = setTimeout(() => (cfgToast = ''), 3000);
	}

	// ---- C2a (t_6565fe8b): konfigurierbare globale Tastenkürzel ------------
	// Key-Capture: Feld über „Ändern“ scharf stellen → Kombination drücken,
	// erste vollständige Mod+Taste-Kombination wird sofort ans Backend gegeben
	// (dort final validiert: ≥1 Mod + 1 Taste, Escape-Verbot, interne
	// Konflikte, Doppelvergabe). Esc bricht die Capture ab. Fehler bleiben im
	// Feld sichtbar, der Capture-Modus bleibt aktiv — alter Kürzel bleibt aktiv.
	type ShortcutField = 'toggle' | 'click';
	// PB (t_1c9bc240, B1): konfigurierbare Mode-Slots — Suche (F1), Marktplatz
	// (F2) und Einstellungen (F12) bleiben fest belegt.
	type ModeHotkeyField = 'hangar' | 'routes' | 'inventory';
	let scCapture = $state<ShortcutField | null>(null);
	let scPreview = $state('');
	let scNote = $state('');
	let scNoteFor = $state<ShortcutField | null>(null);

	const MOD_LABELS: Record<string, string> = { Ctrl: 'STRG', Alt: 'ALT', Shift: 'UMSCHALT', Super: 'WIN' };
	const KEY_LABELS: Record<string, string> = {
		Space: 'LEERTASTE', Enter: 'ENTER', Tab: 'TAB', ArrowUp: '↑', ArrowDown: '↓',
		ArrowLeft: '←', ArrowRight: '→', PageUp: 'BILD HOC', PageDown: 'BILD RUN',
		Backspace: 'BACKSPACE', Delete: 'ENTF', Home: 'POS1', End: 'ENDE', Insert: 'EINFG',
		PrintScreen: 'DRUCK', ScrollLock: 'ROLLEN', CapsLock: 'FESTSTELL', Pause: 'PAUSE',
		Backquote: '`', Backslash: '\\', BracketLeft: '[', BracketRight: ']', Comma: ',',
		Equal: '=', Minus: '-', Period: '.', Quote: "'", Semicolon: ';', Slash: '/',
	};
	function canonicalToLabel(canonical: string): string {
		if (!canonical) return '—';
		return canonical
			.split('+')
			.map((tok) => MOD_LABELS[tok] ?? KEY_LABELS[tok] ?? tok.toUpperCase())
			.join('+');
	}
	// Browser-Event-Code → kanonischer Taste-Token (Backend-Parser akzeptiert
	// genau diese Tokens; Ziffernblock/Browser-Sonderkeys bewusst null).
	function codeToToken(code: string): string | null {
		if (/^Key[A-Z]$/.test(code)) return code.slice(3);
		if (/^Digit\d$/.test(code)) return code.slice(5);
		if (/^F\d{1,2}$/.test(code)) return code;
		const fixed: Record<string, string> = {
			Space: 'Space', Enter: 'Enter', Tab: 'Tab', Backspace: 'Backspace', Delete: 'Delete',
			Home: 'Home', End: 'End', Insert: 'Insert', PageUp: 'PageUp', PageDown: 'PageDown',
			PrintScreen: 'PrintScreen', ScrollLock: 'ScrollLock', CapsLock: 'CapsLock', Pause: 'Pause',
			ArrowUp: 'ArrowUp', ArrowDown: 'ArrowDown', ArrowLeft: 'ArrowLeft', ArrowRight: 'ArrowRight',
			Backquote: 'Backquote', Backslash: 'Backslash', BracketLeft: 'BracketLeft',
			BracketRight: 'BracketRight', Comma: 'Comma', Equal: 'Equal', Minus: 'Minus',
			Period: 'Period', Quote: 'Quote', Semicolon: 'Semicolon', Slash: 'Slash',
		};
		return fixed[code] ?? null;
	}
	function startShortcutCapture(field: ShortcutField) {
		scCapture = field;
		scPreview = '';
		scNote = '';
		scNoteFor = null;
	}
	function cancelShortcutCapture() {
		scCapture = null;
		scPreview = '';
		scNote = '';
		scNoteFor = null;
	}
	// Capture-Key BEFORE every other handler (see onKey): nothing pressed
	// during capture may switch modes. Modifiers alone keep waiting; first
	// complete combo submits immediately.
	function captureShortcut(e: KeyboardEvent) {
		const field = scCapture;
		if (!field) return; // $state-Variable: lokal fixieren, TS kann sie nicht narrowing
		e.preventDefault();
		if (e.key === 'Escape') {
			cancelShortcutCapture();
			return;
		}
		if (['Control', 'Shift', 'Alt', 'AltGraph', 'Meta'].includes(e.key)) return;
		const token = codeToToken(e.code);
		if (!token) {
			scNoteFor = field;
			scNote = `Taste ${e.key} nicht unterstützt — Buchstabe, Ziffer, F1–F24 oder Sonderzeichen der Haupttastatur nutzen.`;
			return;
		}
		const mods: string[] = [];
		if (e.ctrlKey) mods.push('Ctrl');
		if (e.altKey) mods.push('Alt');
		if (e.shiftKey) mods.push('Shift');
		if (e.metaKey) mods.push('Super');
		if (!mods.length) {
			scNoteFor = field;
			scNote = 'Zuerst eine Mod-Taste (Strg/Alt/Shift/Win) halten, dann die normale Taste.';
			return;
		}
		// kanonische Mod-Reihenfolge Ctrl→Alt→Shift→Super = Backend-Kanonik
		const canonical = `${mods.join('+')}+${token}`;
		scPreview = canonicalToLabel(canonical);
		void submitShortcut(field, canonical);
	}
	async function submitShortcut(field: ShortcutField, canonical: string) {
		const cmd = field === 'toggle' ? 'settings_set_shortcut_toggle' : 'settings_set_shortcut_click_through';
		try {
			cfg = await invoke<SettingsSnapshot>(cmd, { combo: canonical });
			applySettings(cfg);
			const applied = field === 'toggle' ? cfg?.shortcut_toggle : cfg?.shortcut_click_through;
			scCapture = null;
			scPreview = '';
			scNote = '';
			scNoteFor = null;
			cfgConfirm(`Tastenkürzel übernommen: ${canonicalToLabel(applied ?? '')} — gilt ab sofort.`);
			scheduleSyncNoteRefresh();
		} catch (err) {
			// Nichts persistiert, OS-Registration unverändert (Backend hat den
			// alten Wert re-registriert). Capture bleibt scharf: sofort neu
			// tryierbar, Fehler sichtbar direkt am betroffenen Feld.
			scNoteFor = field;
			scNote = String(err);
			scPreview = '';
		}
	}
	// C2b: Der Compositor-Sync laeuft im Backend-Hintergrundthread (ANR-
	// Schutz, t_1adbc017) — die Sync-Note ist darum nicht sofort im
	// Snapshot. Einmal nachziehen, damit Erfolg/Fehler am Feld sichtbar wird.
	function scheduleSyncNoteRefresh() {
		window.setTimeout(async () => {
			try {
				const fresh = await invoke<SettingsSnapshot>('settings_get');
				if (fresh && fresh.sync_note) cfg = fresh;
			} catch { /* Overlay gerade zu — naechster Settings-Open holt die Note */ }
		}, 3000);
	}
	// PB (t_1c9bc240, B1): Capture-Verwaltung der Mode-Hotkeys. Identisches
	// Muster wie bei den globalen Kürzeln (C2a): nur EIN Capture scharf,
	// Validierung Backend-seitig (nur F1–F12, F5 reserviert Resync,
	// Doppelbelegung abgelehnt), Snapshot kommt inkl. sync_note zurück.
	const modeHotkeyFields: { id: ModeHotkeyField; label: string }[] = [
		{ id: 'hangar', label: 'Hangar' },
		{ id: 'routes', label: 'Handelsrouten' },
		{ id: 'inventory', label: 'Inventar' }
	];
	let hkCapture: ModeHotkeyField | null = $state(null);
	let hkNote = $state('');
	function startModeHotkeyCapture(id: ModeHotkeyField) {
		scCapture = null;
		hkCapture = id;
		hkNote = '';
	}
	function cancelModeHotkeyCapture() {
		hkCapture = null;
		hkNote = '';
	}
	async function submitModeHotkey(field: ModeHotkeyField, key: string) {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_mode_hotkey', { field, key });
			applySettings(cfg);
			const applied = cfg?.mode_hotkeys?.[field] ?? '';
			hkCapture = null;
			hkNote = '';
			cfgConfirm(`Mode-Hotkey übernommen: ${applied} — gilt ab sofort.`);
		} catch (err) {
			hkNote = String(err);
			hkCapture = null;
		}
	}
	async function resetModeHotkeys() {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_reset_mode_hotkeys');
			applySettings(cfg);
			hkCapture = null;
			hkNote = '';
			cfgConfirm('Mode-Hotkeys auf Standard-Belegung zurückgesetzt.');
		} catch (err) {
			hkNote = String(err);
		}
	}
	// C2b: Consent-Chip "Tastenkürzel mit System (Omarchy) synchronisieren".
	// Erstanlass persistiert die Zustimmung; erst ab true führt die App die
	// zwei arch-overlay-Binds im OmaSettings-Store (Backup vor erstem
	// Schreibvorgang, Verifikation per hyprctl binds + Lua-Check im Backend).
	async function toggleOmarchySync() {
		const on = !(cfg?.shortcuts_sync_omarchy ?? false);
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_shortcuts_sync', { on });
			applySettings(cfg);
			cfgConfirm(cfg.sync_note || (on
				? 'Compositor-Sync aktiv — Kürzel ändern jetzt auch die Hyprland-Binds.'
				: 'Compositor-Sync deaktiviert.'));
		scheduleSyncNoteRefresh();
		} catch (err) {
			cfgConfirm(`Compositor-Sync: ${String(err)}`);
		}
	}
	let shortcutFields = $derived([
		{ id: 'toggle' as ShortcutField, label: 'Overlay-Toggle', fallback: 'Ctrl+Shift+U', value: cfg?.shortcut_toggle },
		{ id: 'click' as ShortcutField, label: 'Click-through', fallback: 'Ctrl+Shift+O', value: cfg?.shortcut_click_through },
		// Ohne Backend (SSR/Probe): die kanonischen Defaults anzeigen.
	].map((f) => ({ ...f, display: canonicalToLabel(f.value || f.fallback) })));

	// P8-Detail-Variante: Backend-Wert ist Quelle der Wahrheit, aber die Umschal-
	// tung wirkt SOFORT auch ohne Backend (statischer Build/Probe) über einen
	// lokalen Fallback, bis settings_set_detail_variant antwortet.
	let localVariant = $state<'a' | 'c' | null>(null);

	// ---- Modi (Karte t_315a26cc) --------------------------------------------
	// Zentraler Mode-State: Defaults F1 Suche · F2 Marktplatz · F3 Routen ·
	// F4 Inventar · F11 Hangar · F12 Einstellungen (konfigurierbar, PB t_1c9bc240). Der bestehende Market-Modus (F2/Rail) ist derselbe
	// State — `phase === 'market'` bleibt die Render-Quelle, Config bleibt
	// `inConfig`; `railMode` trägt nur die Platzhalter-Modi (eigene Karten).
	// P1 t_a39f6811: Handelsrouten — eigener Mode für die Rail/Keyhandler;
	// intern bleibt es der Market-Phase mit mTab='routes'.
	type Mode = 'search' | 'market' | 'routes' | 'inventory' | 'hangar' | 'settings';
	// M2a: dritter Tab „Mein Shop" (eigene Offers + letzte Trades, Read-Phase).
	let mTab = $state<'offers' | 'routes' | 'shop' | 'trades'>('offers');
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
	// P9 (t_aa6cc070): DAS HAUPT-Suchfeld (input) filtert the Hangar-List.
	// P11 (t_aa6cc070): Action-Message auto-fade (hangarNoteSet, ~3.5 s).
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
	// Autocomplete-Candidate (P10): Name + Manufacturer + kind — from the
	// static package and/or already-registered Hangar entries.
	type HangarSuggestion = { name: string; manufacturer: string; kind: string };

	// P11 (Karte t_aa6cc070): Action-Message under the Filter/Import/Add-Buttons
	// auto-fades after ~3.5 s (Timeout, NO Blocker-Refactor). All hangar_* note
	// writes go through hangarNoteSet — old note replaced, removal rescheduled.
	let hangarItems = $state<HangarItem[]>([]);
	let hangarLoaded = $state(false);
	let hangarBusy = $state(false);
	let hangarNote = $state('');
	let hangarNoteTimer: ReturnType<typeof setTimeout> | null = null;
	const HANGAR_NOTE_MS = 3500;
	function hangarNoteSet(msg = '') {
		if (hangarNoteTimer) clearTimeout(hangarNoteTimer);
		hangarNoteTimer = null;
		hangarNote = msg;
		if (msg) hangarNoteTimer = setTimeout(() => {
			hangarNote = '';
			hangarNoteTimer = null;
		}, HANGAR_NOTE_MS);
	}
	let hangarFilter = $state<'all' | 'pledge' | 'ingame' | 'lti' | 'module'>('all');
	let hangarSel = $state(0);
	let hangarView = $state<'list' | 'detail'>('list');
	let hangarDetail = $state<HangarItem | null>(null);

	// P9 (Karte t_aa6cc070): DAS HAUPT-Suchfeld (input) serves as In-Context-
	// Filter for the Hangar-List (like Inventory/Detail) — Name/Custom-Name/
	// Manufacturer/Typ. German Typ-words map to entity_type (schiff→ship …);
	// base_ship + ship_code go into the haystack so "Cutlass" finds the Modules.
	// Leerstring = everything (like the other In-Context-Filters).
	const hangarFiltered = $derived.by(() => {
		let items = hangarItems;
		if (hangarFilter === 'pledge') items = items.filter((i) => i.origin === 'pledge');
		else if (hangarFilter === 'ingame') items = items.filter((i) => i.origin === 'ingame');
		else if (hangarFilter === 'lti') items = items.filter((i) => i.insurance === 'LTI');
		else if (hangarFilter === 'module') items = items.filter((i) => i.entity_type === 'module');
		const q = input.trim().toLowerCase();
		if (q) {
			// Typ-Queries: both the raw word AND the mapped word match entity_type.
			const TYPE_DE: Record<string, string> = { schiff: 'ship', fahrzeug: 'vehicle', fahrzeuge: 'vehicle', modul: 'module', module: 'module' };
			items = items.filter((i) => {
				const hay = `${i.name} ${i.custom_name} ${i.manufacturer} ${i.base_ship} ${i.ship_code}`.toLowerCase();
				if (hay.includes(q)) return true;
				const mapped = TYPE_DE[q];
				return mapped != null && i.entity_type.toLowerCase() === mapped;
			});
		}
		return items;
	});

	async function loadHangar(force = false) {
		if (hangarLoaded && !force) return;
		hangarBusy = true;
		hangarNoteSet('');
		try {
			hangarItems = await invoke<HangarItem[]>('hangar_list');
			hangarLoaded = true;
		} catch (e) {
			hangarNoteSet(String(e));
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
		hangarNoteSet('');
		try {
			const text = await file.text();
			const res = await invoke<HangarImportResult>('hangar_import', { format, text });
			hangarNoteSet(`Import (${res.format}): ${res.imported} neu · ${res.updated} aktualisiert · ${res.skipped} übersprungen`);
			await loadHangar(true);
		} catch (e) {
			hangarNoteSet(String(e));
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
			hangarNoteSet('Export: architects-overlay-hangar.json');
		} catch (e) {
			hangarNoteSet(String(e));
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
			hangarNoteSet(`Gelöscht: ${hangarDisplayName(item)}`);
		} catch (e) {
			hangarNoteSet(String(e));
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
	// P10: current suggestions for the add-form (reactive to Field-Content).
	let hangarSugs = $derived(hangarNameSuggest());

	// P10 (Karte t_aa6cc070): Autocomplete for the Name-Field on Anlegen —
	// already-registered Hangar Ships/Fahrvehicles first (In-Hangar-Wissu),
	// static Ship-Package (src/lib/hangar_ships.ts) fills the rest. A click
	// fills Name + Manufacturer + Typ in one go (fields, no Preset-Form).
	function hangarNameSuggest() {
		const q = hangarForm.name.trim().toLowerCase();
		const out: HangarSuggestion[] = [];
		const seen = new Set<string>();
		const add = (name: string, manufacturer: string, kind: string) => {
			const key = name.toLowerCase();
			if (name && !seen.has(key)) {
				seen.add(key);
				out.push({ name, manufacturer, kind });
			}
		};
		for (const it of hangarItems) {
			if (it.entity_type === 'module') continue;
			const hay = `${it.name} ${it.custom_name}`.toLowerCase();
			if (!q || hay.includes(q) || it.manufacturer.toLowerCase().includes(q)) add(it.name, it.manufacturer, it.entity_type);
		}
		for (const s of suggestShips(q, 12)) add(s.name, s.manufacturer, s.kind);
		return out.slice(0, 10);
	}
	function hangarPickSuggestion(s: HangarSuggestion) {
		// Selection applies Name + Manufacturer + Typ — empty Manufacturer in
		// the static package never overwrites a user-filled value.
		hangarForm = {
			...hangarForm,
			name: s.name,
			manufacturer: s.manufacturer || hangarForm.manufacturer,
			entity_type: s.kind
		};
	}
	// P10: Basisschiff-Field visible ONLY for entity_type=module OR Snub-Ships
	// (Snub = fixed Wiki-Category list "Snub ships" — hangar schema has no
	// ship_category column; documented in src/lib/hangar_ships.ts).
	const hangarBaseOpen = $derived(hangarForm.entity_type === 'module' || isSnubShip(hangarForm.name));
	function hangarBaseSuggest() {
		// Base-Ship-Autocomplete from the ALREADY REGISTERED Hangar entries
		// (Card: "Autocomplete from those already entered in the Hangar").
		const q = hangarForm.base_ship.trim().toLowerCase();
		const out: string[] = [];
		const seen = new Set<string>();
		for (const s of hangarShips) {
			const dn = hangarDisplayName(s);
			const hay = `${s.name} ${s.custom_name}`.toLowerCase();
			if ((q === '' || hay.includes(q) || s.manufacturer.toLowerCase().includes(q)) && !seen.has(dn)) {
				seen.add(dn);
				out.push(dn);
				if (out.length >= 10) break;
			}
		}
		return out;
	}
	// P10: Reactive Base-Ship-Chips — registered Hangar entries only.
	const hangarBaseSugs = $derived(hangarBaseOpen ? hangarBaseSuggest() : [] as string[]);
	function hangarSetBase(name: string) {
		hangarForm = { ...hangarForm, base_ship: name };
	}

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
		hangarNoteSet('');
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
			hangarNoteSet(msg);
		} catch (e) {
			hangarNoteSet(String(e));
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
		// K2 (t_5fe8eb4c, P12): scale live like the palette — snapshot.scale (%)
		// becomes the CSS custom property behind the Shell's zoom.
		document.documentElement.style.setProperty(
			'--ui-scale', String(snapshot.scale / 100));
	}

	async function loadSettings() {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_get');
			applySettings(cfg);
			// M2a-Fix (t_2f197d1c): Settings-Username ist die kanonische Quelle
			// für den „Mein Shop"-Pull — localStorage nur als SSR-Fallback.
			if (cfg?.uex_username && !shopUser.trim()) shopUser = cfg.uex_username.trim();
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
		secretKeyInput = '';
		secretKeyNote = '';
		usernameInput = '';
		usernameNote = '';
		cancelShortcutCapture();
		// PB (t_1c9bc240, B1): Config-Open startet immer sauber.
		cancelModeHotkeyCapture();
		inConfig = true;
		if (searchEl) searchEl.blur();
	}

	function closeConfig() {
		inConfig = false;
		apiKeyInput = '';
		apiKeyNote = '';
		secretKeyInput = '';
		secretKeyNote = '';
		usernameInput = '';
		usernameNote = '';
		// Capture darf ein Schließen nicht überleben (kein Geister-Fenster-Modus).
		cancelShortcutCapture();
		// PB (t_1c9bc240, B1): dasselbe für das Mode-Hotkey-Capture.
		cancelModeHotkeyCapture();
		cfgToast = '';
		if (cfgToastTimer) clearTimeout(cfgToastTimer);
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
			// K6 P2-Feedback: Confirm-Zeile (3 s) + aktive Quelle-Zeile im View;
			// der Flip ist live (SourceSwitch) — die nächste Preisabfrage nutzt
			// sofort die neue Quelle, sichtbar am Quelle-Label der Preisdetail.
			cfgConfirm(
				cfg?.commodity_source === 'uex'
					? 'Preisquelle umgestellt: UEX primär (Wiki-Fallback) — gilt ab sofort.'
					: 'Preisquelle umgestellt: Wiki primär (UEX-Fallback) — gilt ab sofort.'
			);
			// K6 P2d (t_9b385831): Statusbar-Chip ('syncLabel' liest status.provider)
			// sofort konsistent machen. Ohne diesen Refresh aktualisiert der Chip
			// erst beim nächsten Preis-Sync (im Live-Smoke Run433 als 'minimal
			// verzögert' beobachtet). sync_status wertet den live geflippten
			// SourceSwitch aus (providers.rs name()) — kein Restart/Reload nötig.
			try {
				status = await invoke<SyncStatus>('sync_status');
			} catch {
				// Chip-Refresh ist best-effort: Der Flip selbst ist bereits
				// persistiert und wirkt auf die nächste Preisabfrage ohnehin.
			}
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

	// K2 (t_5fe8eb4c, P12): live font/icon scale (F12-View chips 80/90/100/110/125).
	// settings_set_scale persists and returns the snapshot; without a backend (SSR
	// or static probe) the CSS property is set directly — same fallback pattern as
	// localVariant for the Detail-Variante.
	async function chooseScale(pct: number) {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_scale', { pct });
			applySettings(cfg);
		} catch (e) {
			apiKeyNote = String(e);
			if (typeof document !== 'undefined')
				document.documentElement.style.setProperty(
					'--ui-scale', String(pct / 100));
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

	// M2a-Fix (t_2f197d1c): zweites Credential + Username — dieselben Muster
	// wie der App-Key (invoke, Suppression leerer Eingaben, Note beim Ergebnis).
	async function saveSecretKey() {
		const key = secretKeyInput.trim();
		if (!key) return;
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_uex_secret_key', { key });
			secretKeyInput = '';
			secretKeyNote = 'Secret-Key gespeichert — wird NIE im Klartext angezeigt.';
			if (!shopUser.trim() && cfg?.uex_username) shopUser = cfg.uex_username.trim();
		} catch (e) {
			secretKeyNote = String(e);
		}
	}

	async function removeSecretKey() {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_uex_secret_key', { key: null });
			secretKeyInput = '';
			secretKeyNote = 'Secret-Key entfernt.';
		} catch (e) {
			secretKeyNote = String(e);
		}
	}

	async function saveUsername() {
		const name = usernameInput.trim();
		if (!name) return;
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_uex_username', { name });
			usernameInput = '';
			usernameNote = 'UEX-Username gespeichert — wird für den „Mein Shop“-Pull verwendet.';
			// Kanonische Quelle (Settings) → Shop-Input synchron halten.
			shopUser = name;
			try {
				localStorage.setItem(SHOP_USER_KEY, name);
			} catch {
				/* localStorage incomplete — In-Memory bleibt aktiv. */
			}
		} catch (e) {
			usernameNote = String(e);
		}
	}

	// M2b (Write-Layer, Sicherheitsmodell): 2-Schritt-Bestätigung für
	// schreibende Marktplatz-Aktionen umschalten (every_action → jede Aktion,
	// delete_only → nur Abmelden). Keine Batch-Aktionen.
	let mktConfirmNote = $state('');
	async function setMarketplaceConfirm(mode: 'every_action' | 'delete_only') {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_marketplace_confirm', { mode });
			mktConfirmNote = mode === 'every_action'
				? 'Jede schreibende Aktion wird bestätigt (Default).'
				: 'Nur beim ABMELDEN wird bestätigt — Anlegen läuft direkt.';
		} catch (e) {
			mktConfirmNote = String(e);
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
	// M3a-R1 (t_f2e70b16): CDN-Thumbs kommen als `application/octet-stream` +
	// `X-Content-Type-Options: nosniff` — der Webview (Chromium ORB) blockt
	// direkte <img>-Ladungen (live belegt: net::ERR_BLOCKED_BY_ORB). Das
	// Backend proxied (Base64-Data-URI via `marketplace_photo`); Fehler
	// (403/Timeout/kein JPG/PNG) → null → Kachel weggelassen bzw. Leerzustand.
	let offerPhotoUris = $state<(string | null)[]>([]);
	$effect(() => {
		const detail = offerDetail;
		offerPhotoUris = [];
		if (!detail?.photos?.length) return;
		let cancelled = false;
		offerPhotoUris = detail.photos.map(() => null);
		Promise.all(
			detail.photos.map(async (url) => {
				try {
					return await invoke<string>('marketplace_photo', { url });
				} catch {
					return null;
				}
			}),
		).then((uris) => {
			if (!cancelled) offerPhotoUris = uris;
		});
		return () => {
			cancelled = true;
		};
	});
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

	// P8 (K5): Search-History — Querystrings of the last searches persistiert
	// (same localStorage-pattern as fetchedAt above). Root-Cause of the empty
	// idle-pane "Zuletzt gesucht": searches were never persisted and the
	// pane showed a static "Keine" — now filled from `recent`.
	const RECENT_KEY = 'architects-overlay.recentSearches';
	function loadRecent(): string[] {
		try {
			const v = JSON.parse(String(localStorage.getItem(RECENT_KEY) ?? '[]'));
			return Array.isArray(v) ? v.filter((s) => typeof s === 'string' && s.trim()).slice(0, 8) : [];
		} catch {
			return [];
		}
	}
	let recent = $state(loadRecent());
	function pushRecent(qRaw: string) {
		const q = qRaw.trim();
		if (!q) return;
		// P8-Fix (Review t_50c64411 B1): no history entry may be the Prefix of
		// a newer one → typing-Fragments of a partial search get dropped.
		// (old: Dedupe exact-Equals only → prefix of a newer entry kept)
		recent = [q, ...recent.filter((s) => s !== q && !q.startsWith(s))].slice(0, 8);
		try {
			localStorage.setItem(RECENT_KEY, JSON.stringify(recent));
		} catch {
			/* localStorage incomplete — In-Memory-Tracking stays active. */
		}
	}
	// P8-Fix (Review t_50c64411 B1): pushRecent must NOT run in runSearch —
	// runSearch fires on every 180ms debounce-Interval while typing, one
	// History-entry per Prefix. Only a completed search persistiert:
	// persistRecent fires after 750ms Typing-Stille (see onInput) or gets
	// flushed when the search is explicitly left (Detail open/close).
	let recentTimer: ReturnType<typeof setTimeout> | null = null;
	function persistRecent() {
		recentTimer = null;
		if (phase === 'market' || railMode || inDetail) return;
		const q = parseQuery();
		if (!q.isCalc && q.text) pushRecent(q.text);
	}
	function flushRecent() {
		if (recentTimer === null) return;
		clearTimeout(recentTimer);
		recentTimer = null;
		persistRecent();
	}
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

	// ---- Mein Shop (M2a, Read-Phase): eigene Offers + Marktplatz-Deals -----
	let shopUser = $state(loadShopUser());
	let myOffers = $state<Offer[]>([]);
	let myNegotiations = $state<Negotiation[]>([]);
	let shopLoading = $state(false);
	let shopSource = $state<'cache' | 'live' | 'offline'>('live');
	let shopNote = $state<string | null>(null);
	let shopKeySet = $state(true);
	let shopErr = $state<string | null>(null);
	function persistShopUser(v: string) {
		shopUser = v;
		try {
			if (v.trim()) localStorage.setItem(SHOP_USER_KEY, v.trim());
			else localStorage.removeItem(SHOP_USER_KEY);
		} catch {
			/* localStorage incomplete — In-Memory bleibt aktiv. */
		}
		// M2a-Fix (t_2f197d1c): der Username lebt kanonisch in den Settings
		// (F12) — der Shop-Input schreibt best-effort zurück, damit beide
		// Sichten synchron bleiben. Kein await: UI darf hier nicht blockieren.
		if (typeof invoke === 'function') {
			invoke('settings_set_uex_username', { name: v.trim() || null }).catch(() => {});
		}
	}
	async function loadShop(bg = false) {
		// M2a-Fix (t_2f197d1c): ohne persönlichen Secret-Key KEINE Requests —
		// UEX antwortet sonst nur 400 user_not_found/403 invalid_secret_key.
		// Der Gate zeigt stattdessen die deutsche Meldung + Settings-Link
		// (cfgs null = SSR/Probe ohne Backend → Verhalten wie bisher).
		if (cfg && !cfg.uex_secret_key_set) {
			shopLoading = false;
			shopErr = null;
			shopNote = null;
			myOffers = [];
			myNegotiations = [];
			return;
		}
		shopLoading = !bg;
		shopErr = null;
		shopNote = null;
		// M2b-Review-Fix: Erfolgs-Deep-Link nur für die letzte Create-Aktion —
		// beim Neu-Laden des Shops verfallen (Listings stecken sowieso im
		// UEX-Approval, ein fehlgeschlagener Write darf keinen alten Link zeigen).
		if (!bg) shopCreatedUrl = null;
		// Eigene Offers brauchen den username-Filter; ohne key kein Abschnitt.
		// Beide Reads parallel — unabhängig voneinander, ein Fehler reißt den
		// anderen nicht mit. t_350c4300 B2: der zweite Read ist die
		// MARKTPLATZ-Quelle get_my_negotiations (nicht mehr get_my_trades —
		// der /user_trades-Commodity-Verlauf zeigt hier die falschen Daten).
		const results = await Promise.allSettled([
			invoke<OwnOffersResponse>('get_my_offers', { username: shopUser.trim() }),
			invoke<NegotiationsResponse>('get_my_negotiations')
		]);
		shopLoading = false;

		// Offers
		if (results[0].status === 'fulfilled') {
			const r = results[0].value;
			myOffers = r.offers;
			shopSource = r.source;
			shopKeySet = r.key_set;
			if (r.note) shopNote = r.note;
		} else {
			myOffers = [];
			shopErr = String(results[0].reason);
		}
		// Marktplatz-Deals (unabhängig vom Offers-Ergebnis)
		if (results[1].status === 'fulfilled') {
			const r = results[1].value;
			myNegotiations = r.negotiations;
			shopKeySet = shopKeySet || r.key_set;
			if (r.note && !shopNote) shopNote = r.note;
		} else if (!shopErr) {
			shopErr = String(results[1].reason);
		}
	}
	// Beim Betreten des Mein-Shop-Tabs laden (stale-first im Backend).
	$effect(() => {
		if (phase === 'market' && mTab === 'shop') void loadShop(true);
	});

	// ---- Trades (M3b, t_893761f1): Deal-Liste + Chat im Overlay -------------
	// Deals = dieselbe Quelle wie „Letzte 5 Trades" (get_my_negotiations,
	// stale-first im Backend). Chat: GET+POST /marketplace_negotiations_messages
	// — konversationell, KEINE 2-Schritt-Bestätigung; Fehlerpfad (429/403)
	// Meldung + Deep-Link statt Retry. Lifecycle (Start/Akzeptieren/Beenden/
	// Bewertung) ist API-SEITIG NICHT vorhanden (Research §3) → Deep-Link-
	// Buttons an die UEX-Webseite (Listing-Seite, dort sitzen die Buttons).
	let tradesLoading = $state(false);
	let tradesErr = $state<string | null>(null);
	let tradesNote = $state<string | null>(null);
	let tradesDeal = $state<Negotiation | null>(null);
	let chatMsgs = $state<NegotiationMessage[]>([]);
	let chatLoading = $state(false);
	let chatErr = $state<string | null>(null);
	let chatDraft = $state('');
	let chatBusy = $state(false);
	let chatScroller: HTMLElement | null = null;
	// neuer-Nachricht-Indikator: pro Deal-Hash der zuletzt GESEHENE
	// date_modified-Stand; Row mit aktuellerer Interaktion zeigt einen Punkt.
	let tradesSeen: Record<string, number> = {};
	let tradesPoller: ReturnType<typeof setInterval> | null = null;

	const dealsSorted = $derived(
		[...myNegotiations].sort((a, b) => (b.date_modified || b.date_added) - (a.date_modified || a.date_added))
	);
	function dealUnread(n: Negotiation): boolean {
		const seen = tradesSeen[n.hash] ?? 0;
		return (n.date_modified || 0) > seen;
	}

	async function loadDeals(bg: boolean) {
		// Ohne Secret-Key ist der Pull nutzlos — sauberer Hinweis statt
		// roher UEX-Auth-Meldung (Muster loadShop).
		if (cfg && !cfg.uex_secret_key_set) {
			tradesLoading = false;
			tradesErr = null;
			tradesNote = 'Für den Trades-Reiter braucht UEX deinen persönlichen Secret-Key (Einstellungen → „UEX persönlicher Secret-Key“).';
			myNegotiations = [];
			return;
		}
		tradesLoading = !bg;
		tradesErr = null;
		tradesNote = null;
		try {
			const r = await invoke<NegotiationsResponse>('get_my_negotiations');
			myNegotiations = r.negotiations;
			if (r.note) tradesNote = r.note;
		} catch (e) {
			tradesErr = String(e);
		}
		tradesLoading = false;
	}

	async function selectDeal(n: Negotiation, bg = false) {
		tradesDeal = n;
		tradesSeen = { ...tradesSeen, [n.hash]: n.date_modified || 0 };
		chatErr = null;
		chatLoading = !bg;
		try {
			const r = await invoke<MessagesResponse>('get_negotiation_messages', {
				hash: n.hash,
				id_negotiation: n.id
			});
			chatMsgs = r.messages;
			if (r.note) chatErr = r.note;
		} catch (e) {
			chatErr = String(e);
		}
		chatLoading = false;
		// Verlauf ans Ende scrollen (neueste unten).
		queueMicrotask(() => {
			if (chatScroller) chatScroller.scrollTop = chatScroller.scrollHeight;
		});
	}

	async function sendChat() {
		if (chatBusy || !tradesDeal) return;
		const text = chatDraft.trim();
		if (!text) return;
		chatBusy = true;
		chatErr = null;
		const deal = tradesDeal;
		try {
			await invoke<MessageSendResponse>('send_negotiation_message', {
				id_negotiation: deal.id,
				hash: deal.hash,
				message: text
			});
			chatDraft = '';
			// Verlauf nach dem Write nachziehen (Pull statt Optimismus).
			await selectDeal(deal, true);
		} catch (e) {
			// KEIN Retry — Meldung + Deep-Link (429/403-Pfad wie bei den
			// anderen Writes). chatErr rendert den Button im Chat-Pane.
			chatErr = String(e);
		}
		chatBusy = false;
	}

	function chatKey(e: KeyboardEvent) {
		if (e.key === 'Enter' && !e.shiftKey) {
			e.preventDefault();
			void sendChat();
		}
	}

	function dealTime(ts: number): string {
		if (!ts) return '—';
		const d = new Date(ts * 1000);
		const today = new Date();
		return d.toDateString() === today.toDateString()
			? d.toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' })
			: d.toLocaleDateString('de-DE') + ' ' + d.toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' });
	}

	// Lifecycle-Deep-Links (M3b-4): Start/Akzeptieren/Beenden/Bewertung sind
	// NICHT per API vorhanden (Research §3) — die Buttons öffnen die
	// UEX-Webseite an der Listing-/Deal-Stelle (verifiziertes Muster
	// /marketplace/item/info/{slug}/; Einzeldeal-URL nicht dokumentiert).
	async function openTradesDeepLink(n: Negotiation, what: string) {
		try {
			await invoke<string>('open_marketplace_listing', { slug: n.listing_slug });
		} catch (e) {
			tradesErr = what + ' · ' + String(e);
		}
	}

	// Beim Betreten des Trades-Tabs laden + dezent pollen (max 1/min im
	// Idle, Research §5). Beim Verlassen Interval abbauen.
	$effect(() => {
		if (phase === 'market' && mTab === 'trades') {
			void loadDeals(true);
			if (tradesPoller) clearInterval(tradesPoller);
			tradesPoller = setInterval(() => {
				void loadDeals(true);
				if (tradesDeal && !chatBusy) void selectDeal(tradesDeal, true);
			}, 60_000);
		} else if (tradesPoller) {
			clearInterval(tradesPoller);
			tradesPoller = null;
		}
		return () => {
			if (tradesPoller) {
				clearInterval(tradesPoller);
				tradesPoller = null;
			}
		};
	});
	// Status einer eigenen Listung aus der Expiry ableiten (UEX liefert auf
	// marketplace_listings kein eigenes Status-Feld): ohne Ende → „aktiv",
	// Zukunft → „läuft ab", Vergangenheit → „abgelaufen".
	function shopStatus(o: Offer): string {
		if (!o.approved) return 'wartet auf Freigabe';
		if (!o.expires) return 'aktiv';
		if (o.expires <= Math.floor(Date.now() / 1000)) return 'abgelaufen';
		return 'läuft ab ' + new Date(o.expires * 1000).toLocaleDateString('de-DE');
	}
	// M3a: Mein-Shop-Filter (Alle/Aktiv/Wartend) — clientseitig, denn UEX
	// /marketplace_listings bietet keinen Status-Query-Parameter (live geprüft:
	// nur username=; Freigabe steckt im Feld date_approved, 0 = wartend).
	let shopFilter = $state<'all' | 'active' | 'pending'>('all');
	const shopOffersFiltered = $derived(
		shopFilter === 'all'
			? myOffers
			: myOffers.filter((o) => (shopFilter === 'pending' ? !o.approved : o.approved))
	);
	// Letzte 5 Marktplatz-Trades (neueste zuerst, t_350c4300 B2) — Aktivität
	// nach date_modified (letzte Interaktion, inkl. Abschluss), Fallback auf
	// date_added. Die letzten-5-Logik liegt laut Entscheidung in der UI; der
	// Cache liefert den vollen Verlauf.
	const recentNegotiations = $derived(
		[...myNegotiations]
			.sort((a, b) => (b.date_modified || b.date_added) - (a.date_modified || a.date_added))
			.slice(0, 5)
	);
	// Status-Zeile eines Deals (Marktplatz-Bezug statt Commodity-Trace):
	// abgeschlossen = deal_value + date_closed, sonst laufende Verhandlung.
	function dealStatus(n: Negotiation): string {
		if (n.date_closed) {
			const dv = n.deal_value ? ' · ' + fmtPrice(n.deal_value) + ' ' + (n.deal_value_currency || n.currency || '') : '';
			return 'abgeschlossen' + dv;
		}
		return 'in Verhandlung · seit ' + new Date((n.date_added || 0) * 1000).toLocaleDateString('de-DE');
	}
	// Rolle des eigenen Accounts in diesem Deal (m-op-Anzeige).
	function dealRole(n: Negotiation): 'sell' | 'buy' {
		return n.is_listing_advertiser ? 'sell' : 'buy';
	}
	async function openListingDeepLink(slug: string | null) {
		try {
			// Leerer Slug → Marketplace-Startseite (Backend-Doku). Kein iframe —
			// Systembrowser via tauri-plugin-opener, nichts Schreibendes.
			await invoke<string>('open_marketplace_listing', { slug: slug ?? '' });
		} catch (e) {
			shopErr = String(e);
		}
	}
	async function openMyShopDeepLink() {
		try {
			// t_350c4300 B3: Ziel ist der Manager-Listings-Tab
			// (https://uexcorp.space/marketplace/manage/tab/listings/) — dort
			// verwaltet der Nutzer seine Listings (und sieht die Deals dazu).
			// Login macht er selbst im Browser.
			await invoke<string>('open_marketplace_trades');
		} catch (e) {
			shopErr = String(e);
		}
	}
	// M2b-Review-Fix (t_6e26ec82 R2): öffnet den Permalink eines frisch
	// angelegten Listings (Create-Antwort liefert eine volle URL). Falls leer,
	// fällt der Button auf den Marketplace-Home zurück.
	async function openCreatedListing(url: string) {
		try {
			if (url && url.startsWith('https://uexcorp.space/')) {
				await invoke<string>('open_created_listing', { url });
			} else {
				await invoke<string>('open_marketplace_listing', { slug: '' });
			}
		} catch (e) {
			shopErr = String(e);
		}
	}

	// ---- M2b (Write-Layer): Anlegen + Abmelden — 2-Schritt-Bestätigung,
	// kein Edit (Bearbeiten = Abmelden + Neu anlegen), Pull nach Write. ----
	type AdvertiseForm = {
		operation: 'sell' | 'buy';
		// MC (t_5faaf946): Listing Type — UEX `type`: item|service|contract.
		// NN (t_060d134e): '' möglich — geleertes Dropdown leert den Wert;
		// die Validierung verlangt vor dem Write erneut eine Auswahl.
		listing_type: 'item' | 'service' | 'contract' | '';
		// Item-Auswahl (Typeahead) — UEX `id_item`. Pflicht wenn listing_type=item.
		item_id: string;
		item_label: string;
		title: string;
		description: string;
		price: string;
		quantity: string;
		unit: string;
		location: string;
		source: string;
		// MC: Currency (UEC|WIF|MGS), Default UEC.
		currency: string;
		// MC: Quality 0–1000 (optional). NN (t_060d134e): 1000 statt 100.
		quality: string;
		// MC: Hide Listing (optional, Default No) → is_hidden 1/0.
		hidden: boolean;
		// MC: Availability (optional) — Semantik lt. Doku [7]: Verfügbarkeits-
		// Art (sofort/abholbereit/auf Bestellung/…), NICHT Lagerbestand (das
		// ist `in_stock`) und kein Zeitraum.
		availability: string;
		// MC: Bilder bis 5 (Base64-data-URIs für Vorschau; das erste Bild geht
		// doku-konform als `image_data` an UEX).
		images: string[];
		hours: string;
		// M2b-Review-Fix (t_6e26ec82): id_category ist für type=item Pflicht.
		// Auswahl = value des <select> ('' = noch nichts gewählt / „bitte wählen").
		category: string;
	};
	// UEX /categories — Kategorie-Auswahl für die Anlegen-Form (id + Name).
	type MarketCategory = {
		id: number;
		name: string;
		section: string | null;
	};
	// M2b-Review-Fix (t_6e26ec82 R2): die Create-Antwort von
	// marketplace_advertise ist KEIN Offer — UEX liefert `{ id, url }`: die
	// neue Listing-Id + den Permalink (Deep-Link-Target). Daran hängt die
	// Erfolgsmeldung, NICHT an einem Pull (Listings erscheinen erst nach
	// UEX-Approval in /marketplace_listings).
	type AdvertiseResult = {
		id: number;
		url: string;
	};
	let advertCategories = $state<MarketCategory[]>([]);
	let advertCatOpen = $state(false);
	// Permalink eines frisch angelegten Listings (für den Erfolgs-Deep-Link).
	let shopCreatedUrl = $state<string | null>(null);
	$effect(() => {
		// Beim Öffnen der Form die Kategorien laden (einmalig pro Session;
		// UEX /categories ist klein + öffentlich). Ein Fehler hier blockiert
		// das Anlegen NICHT — die Form-Validierung meldet dann, dass eine
		// Kategorie fehlt, statt ohne Daten zu funktionieren.
		if (advertOpen && !advertCatOpen) {
			advertCatOpen = true;
			invoke<MarketCategory[]>('get_categories')
				.then((c) => (advertCategories = c))
				.catch(() => {
					advertCategories = [];
				});
		}
		if (!advertOpen) {
			advertCatOpen = false;
		}
	});
	const emptyAdvertise = (): AdvertiseForm => ({
		operation: 'sell',
		listing_type: 'item',
		item_id: '',
		item_label: '',
		title: '',
		description: '',
		price: '',
		quantity: '1',
		unit: 'unit',
		location: '',
		source: '',
		currency: 'UEC',
		quality: '',
		hidden: false,
		availability: '',
		images: [],
		hours: '168',
		category: ''
	});
	// 2-Schritt: Zustand, welche Aktion gerade auf Bestätigung wartet.
	let advertForm = $state(emptyAdvertise());
	let advertOpen = $state(false);
	// M3a (Popup): Verwerfen-Frage bei ungespeicherten Eingaben. `advertBase`
	// friert den Formularzustand beim Öffnen ein; `advertDirty` vergleicht.
	let advertDiscard = $state(false);
	let advertBase = $state('');

	function advertDirty(): boolean {
		try {
			return JSON.stringify(advertForm) !== advertBase;
		} catch {
			return false;
		}
	}

	/** Popup schließen: ungespeicherte Eingaben → kurze Verwerfen-Frage. */
	function requestCloseAdvert() {
		if (advertDirty()) {
			advertDiscard = true;
			return;
		}
		advertOpen = false;
		advertDiscard = false;
		editingOffer = null;
	}
	let advertNote = $state('');
	// MC (t_5faaf946): Ort- + Item-Typeahead-Optionen (async geladen).
	// Ort: `location_suggest` (EntityKind::Locations, NUR Locations — Shops/
	// Terminals erscheinen nie, gleiche Quelle wie der Routen-Filter).
	type AdvertTypeaheadOpt = { value: string; label: string };
	let advertLocSug = $state<string[]>([]);
	let advertItemSug = $state<{ id: number; name: string; subtitle: string }[]>([]);
	let advertLocDeb: ReturnType<typeof setTimeout> | null = null;
	let advertItemDeb: ReturnType<typeof setTimeout> | null = null;
	$effect(() => {
		const q = advertForm.location.trim();
		if (advertLocDeb) clearTimeout(advertLocDeb);
		advertLocDeb = setTimeout(() => {
			void (async () => {
				try {
					advertLocSug = q ? await invoke<string[]>('location_suggest', { query: q }) : [];
				} catch {
					advertLocSug = [];
				}
			})();
		}, 120);
	});
	$effect(() => {
		const q = advertForm.item_label.trim();
		if (advertItemDeb) clearTimeout(advertItemDeb);
		advertItemDeb = setTimeout(() => {
			void (async () => {
				try {
					advertItemSug = q ? await invoke<{ id: number; name: string; subtitle: string }[]>('item_suggest', { query: q }) : [];
				} catch {
					advertItemSug = [];
				}
			})();
		}, 120);
	});
	let advertLocOpts = $derived(
		advertLocSug.map((n) => ({ value: n, label: n })) as AdvertTypeaheadOpt[]
	);
	let advertItemOpts = $derived(
		advertItemSug.map((i) => ({ value: String(i.id), label: i.name })) as AdvertTypeaheadOpt[]
	);
	// MC: Dateiauswahl, Base64-Vorschau, X-Entfernen. UEX überträgt doku-
	// konform EIN Bild als image_data (Doku [7]). NN (t_060d134e): genau
	// 1 Bild — eine neue Wahl ERSETZT das alte (keine 5er-Sammlung mehr).
	let advertFileInput = $state<HTMLInputElement | null>(null);
	function addAdvertImages(files: FileList | null) {
		if (!files || !files.length) return;
		const file = files[0];
		if (!/^image\/(jpeg|png)$/.test(file.type)) {
			advertNote = 'Nur JPG/PNG-Bilder sind erlaubt (UEX-Limit).';
			return;
		}
		if (file.size > 10 * 1024 * 1024) {
			advertNote = 'Bild „' + file.name + '“ ist größer als 10 MB (UEX-Limit).';
			return;
		}
		const reader = new FileReader();
		reader.onload = () => {
			if (typeof reader.result === 'string') {
				// Replace-Semantik: das neue Bild ersetzt ein ggf. vorhandenes.
				advertForm.images = [reader.result as string];
			}
		};
		reader.readAsDataURL(file);
	}
	let confirmAction = $state<null | { kind: 'advertise' } | { kind: 'delete'; offer: Offer }>(null);
	let writeBusy = $state(false);
	// „Bearbeiten" gibt es NICHT als eigenen Zweig — das Formular wird beim
	// Editieren nur VORBEFÜLLT; der Nutzer meldet die alte Listung separat ab
	// und legt neu an (klare UX-Kennzeichnung, M2b-1).
	let editingOffer = $state<Offer | null>(null);
	function openAdvertise(offer: Offer | null) {
		if (offer) {
			editingOffer = offer;
			advertForm = {
				operation: offer.operation === 'buy' ? 'buy' : 'sell',
				listing_type: offer.listing_type === 'service' || offer.listing_type === 'contract'
					? offer.listing_type
					: 'item',
				item_id: offer.item_id ? String(offer.item_id) : '',
				item_label: '',
				title: offer.title,
				description: offer.description,
				price: String(Math.round(offer.price)),
				quantity: String(offer.quantity),
				unit: offer.unit || 'unit',
				location: offer.location,
				// Offer (Wire-Projektion) trägt kein source/availability — Edit
				// = Vorbefüllung nur der vorhandenen Felder; Rest bleibt leer.
				source: '',
				currency: offer.currency || 'UEC',
				quality: '',
				hidden: false,
				availability: '',
				images: [],
				hours: '168',
				// Bearbeiten = vorbelegtes Formular: die Kategorie der Bestands-
				// listung vorwählen (falls im /categories-Pool vorhanden).
				category: String(offer.category_id || '')
			};
		} else {
			editingOffer = null;
			advertForm = emptyAdvertise();
		}
		advertOpen = true;
		advertDiscard = false;
		try {
			advertBase = JSON.stringify(advertForm);
		} catch {
			advertBase = '';
		}
		advertNote = '';
	}
	// 2-Schritt: prüft das Settings-Modell (M2b-2). ConfirmEveryAction = immer
	// bestätigen, ConfirmDeleteOnly = nur das Abmelden; Anlegen läuft direkt.
	// Die Formular-Validierung (inkl. Kategorie-Pflicht, M2b-Review-Fix
	// t_6e26ec82 + MC-Pflichtregel) passiert VOR dem Confirm — der Nutzer
	// bestätigt nur ein vollständiges Formular, und ohne Auswahl erscheint
	// nie ein Write-Dialog.
	//
	// MC (t_5faaf946): Pflicht-Regel — alle als (optional) markierten Felder
	// sind optional (Item Source, Quality, Hide Listing, Availability, Bilder),
	// ALLE ANDEREN Pflicht. Fehlermeldungen pro Feld (deutsch), gesammelt in
	// `advertErrors`, kein stiller API-Fehler (UEX-400 wird nie verschluckt).
	type AdvertErrors = Partial<
		Record<
			| 'title'
			| 'description'
			| 'category'
			| 'item'
			| 'location'
			| 'price'
			| 'quantity'
			| 'unit'
			| 'currency'
			| 'quality'
			| 'listing_type'
			| 'hours'
			| 'images',
			string
		>
	>;
	let advertErrors = $state<AdvertErrors>({});
	// UEX-Pflichtfeld-Referenzen lt. Doku [7] + UEX-Maske (Screenshot).
	const SOURCE_CHOICES = ['looted', 'pledged', 'purchased_in_game', 'pirated', 'gifted', 'crafted'];
	const CURRENCY_CHOICES = ['UEC', 'WIF', 'MGS'];
	const AVAILABILITY_CHOICES = [
		'immediate', 'ready_pickup', 'on_demand', 'pre_order', 'work_order',
		'reserve_only', 'scheduled', 'in_progress', 'negotiable'
	];
	// Unit-Referenzen je Listing-Type (Doku [7]): type=item / service / contract.
	const UNIT_MAP: Record<'item' | 'service' | 'contract', string[]> = {
		item: ['box', 'crate', 'cscu', 'dozen', 'hundred', 'pack', 'pair', 'scu', 'set', 'stack', 'thousand', 'unit'],
		service: ['contract', 'cycle', 'day', 'event', 'expedition', 'gm', 'hour', 'minute', 'mission', 'month', 'operation', 'route', 'run', 'service', 'session', 'shift', 'trip', 'week'],
		contract: ['contract', 'mission']
	};
	function validateAdvertise(f: AdvertiseForm): AdvertErrors {
		const errs: AdvertErrors = {};
		// NN (t_060d134e): geleertes Listing-Type-Dropdown → Pflicht-Hinweis.
		if (!f.listing_type) errs.listing_type = 'Listing Type ist Pflicht.';
		if (!f.title.trim()) errs.title = 'Titel ist Pflicht.';
		else if (f.title.trim().length > 140) errs.title = 'Titel max. 140 Zeichen (Doku).';
		if (!f.description.trim()) errs.description = 'Beschreibung ist Pflicht.';
		else if (f.description.trim().length > 65535) errs.description = 'Beschreibung max. 65.535 Zeichen (Doku).';
		const catId = Number(f.category);
		if (!f.category || !Number.isFinite(catId) || catId <= 0) {
			errs.category = 'Bitte eine Kategorie wählen (Pflicht für Item-/Contract-Listings).';
		}
		// Item ist PFLICHT nur bei type=item (Task-Regel „Pflicht wenn Type=Item").
		if (f.listing_type === 'item' && !f.item_id) {
			errs.item = 'Bitte ein Item wählen (Pflicht für Item-Listings).';
		}
		if (!f.location.trim()) errs.location = 'Ort ist Pflicht.';
		const price = Math.round(Number(f.price));
		if (!Number.isFinite(price) || price <= 0) errs.price = 'Preis muss eine positive Ganzzahl sein.';
		const qty = Math.round(Number(f.quantity));
		if (!Number.isFinite(qty) || qty <= 0) errs.quantity = 'Menge muss eine positive Ganzzahl sein.';
		if (!f.unit.trim()) errs.unit = 'Einheit ist Pflicht.';
		if (!f.currency.trim()) errs.currency = 'Währung ist Pflicht.';
		// NN (t_060d134e): Quality-Bereich 0–1000 (UEX-Maske; 100 war zu eng).
		if (f.quality !== '' && (!Number.isFinite(Number(f.quality)) || Number(f.quality) < 0 || Number(f.quality) > 1000)) {
			errs.quality = 'Quality muss zwischen 0 und 1000 liegen.';
		}
		if (!f.hours) errs.hours = 'Laufzeit ist Pflicht.';
		// NN (t_060d134e): UEX überträgt nur 1 Bild (image_data) — UI erlaubt
		// genau 1 (neue Wahl ersetzt die alte), keine 5er-Sammlung mehr.
		if (f.images.length > 1) errs.images = 'Maximal 1 Bild (UEX-Limit).';
		return errs;
	}
	function applyAdvertErrors(errs: AdvertErrors): boolean {
		advertErrors = errs;
		if (Object.keys(errs).length > 0) {
			advertNote = 'Bitte die markierten Felder korrigieren.';
			advertOpen = true;
			return false;
		}
		advertNote = '';
		return true;
	}
	function requestAdvertise() {
		if (writeBusy) return;
		const f = advertForm;
		if (!applyAdvertErrors(validateAdvertise(f))) return;
		const confirmMode = cfg?.marketplace_confirm ?? 'every_action';
		if (confirmMode === 'every_action') {
			confirmAction = { kind: 'advertise' };
		} else {
			void doAdvertise();
		}
	}
	async function doAdvertise() {
		confirmAction = null;
		advertNote = '';
		const f = advertForm;
		if (!applyAdvertErrors(validateAdvertise(f))) return;
		const catId = Number(f.category);
		const price = Math.round(Number(f.price));
		const qty = Math.round(Number(f.quantity));
		// Nach jedem Write PULL (M2b-4) — kein Optimismus. Fehlerpfad: kein
		// Retry, Meldung + Deep-Link-Button (M2b-3).
		writeBusy = true;
		// MC (t_5faaf946): neue Felder mit UEX-Feldnamen; `quality` als
		// eigenes Feld (UEX-Output + Maske führen es), Availability nur wenn
		// gewählt, Bilder doku-konform als EIN image_data (erstes Bild, Base64).
		const payload = {
			id_item: f.listing_type === 'item' && f.item_id ? Number(f.item_id) : null,
			// id_category: gewählte Kategorie (Pflichtfeld, s. o.)
			id_category: catId,
			id_star_system: null,
			id_terminal: null,
			id_organization: null,
			operation: f.operation,
			language: 'de_DE',
			type: f.listing_type,
			unit: f.unit.trim() || 'unit',
			price,
			currency: f.currency.trim() || 'UEC',
			title: f.title.trim(),
			description: f.description.trim(),
			source: f.source.trim() || null,
			availability: f.availability.trim() || null,
			durability: null,
			quality: f.quality !== '' ? Math.round(Number(f.quality)) : null,
			location: f.location.trim() || null,
			in_stock: qty,
			hours_expiration: Number(f.hours) || null,
			is_hidden: f.hidden ? 1 : null,
			image_data: f.images.length ? f.images[0] : null,
			is_production: 1
		};
		try {
			// M2b-Review-Fix (t_6e26ec82 R2): die Create-Antwort ist KEIN Offer —
			// UEX liefert `{ id, url }` (der Deep-Link). Die Erfolgsmeldung hängt
			// an id/url aus der Antwort, NICHT an einem Pull (Listings erscheinen
			// erst nach UEX-Approval in /marketplace_listings).
			const res = await invoke<AdvertiseResult>('marketplace_advertise', { payload });
			editingOffer = null;
			advertOpen = false;
			advertDiscard = false;
			advertForm = emptyAdvertise();
			// M2b-4: Pull nach jedem Write (Bestätigung durch den Response/GET).
			// Ein frisch angelegtes Listing fehlt bis zum UEX-Approval — Erfolg
			// wird trotzdem sauber gemeldet (Button öffnet die Listung via id/url).
			await loadShop(true);
			shopCreatedUrl = res.url || null;
			shopNote = (f.operation === 'sell'
				? 'Listing „' + f.title.trim() + '“ wurde angelegt.'
				: 'Kauf-Gesuch „' + f.title.trim() + '“ wurde angelegt.') +
				' Es erscheint nach UEX-Approval in deiner Offers-Liste.';
		} catch (e) {
			// KEIN Retry — Fehlermeldung + Deep-Link-Button (UI rendert shopErr).
			shopErr = String(e);
			advertOpen = true;
		}
		writeBusy = false;
	}
	// Abmelden bestätigt IMMER (any Modus — Löschen ist die riskantere Aktion,
	// M2b-2 kein Retry/Spam, M2b-3 Fehlerpfad).
	function requestDelete(offer: Offer) {
		if (writeBusy) return;
		confirmAction = { kind: 'delete', offer };
	}
	async function doDelete(offer: Offer) {
		confirmAction = null;
		writeBusy = true;
		try {
			await invoke('marketplace_delete', { id: offer.id });
			// Pull statt Optimismus.
			await loadShop(true);
			shopNote = 'Listing „' + offer.title + '“ wurde abgemeldet.';
		} catch (e) {
			shopErr = String(e);
		}
		writeBusy = false;
	}
	// MC (t_5faaf946): Unit-Auswahl hängt vom Listing-Type ab (Doku [7] —
	// separate Referenzliste für item/service/contract); UNIT_MAP oben.
	function unitsFor(type: AdvertiseForm['listing_type']): string[] {
		if (type !== 'item' && type !== 'service' && type !== 'contract') return UNIT_MAP.item;
		return UNIT_MAP[type];
	}
	// Laufzeit: >24h als Tage anzeigen (48→2d … 720→30d), ≤24h in h bleiben.
	// Der gespeicherte Wert bleibt unverändert die Stundenzahl (API erwartet h).
	function hourLabel(h: string): string {
		const n = Number(h);
		if (!Number.isFinite(n)) return h + ' h';
		return n > 24 ? (n / 24) + 'd' : n + ' h';
	}
	const HOUR_CHOICES = ['1', '2', '3', '5', '12', '24', '48', '72', '120', '168', '336', '720'];

	// NN (t_060d134e): Preis-Eingabe mit Tausender-Punkten — angezeigt wird
	// gruppiert (1.500.000.000), gespeichert bleibt die reine Ziffernfolge
	// (advertForm.price), sodass Validierung/Parse unverändert Number() nutzt.
	function fmtGroup(v: string): string {
		const digits = String(v).replace(/\D/g, '');
		return digits.replace(/\B(?=(\d{3})+(?!\d))/g, '.');
	}
	function onPriceInput(e: Event) {
		const el = e.currentTarget as HTMLInputElement;
		const digits = el.value.replace(/\D/g, '').slice(0, 12);
		advertForm.price = digits;
		el.value = digits ? fmtGroup(digits) : '';
	}

	// ---- Handelsrouten (Karte t_128b7547) ------------------------------------
	// Separater Tab im Marktmodus (Design-Empfehlung der Karte). Datenquelle:
	// UEX-Sweep über alle Commodity-Terminals (Backend `get_routes`), Filte-
	// rung/Sortierung serverseitig im Backend. BEWUSST ohne Mindest-/Maximal-
	// bestand-Filter (Unterschied zu Arkanis).
	type RouteSort = 'profit_desc' | 'profit_asc' | 'distance_asc' | 'margin_desc' | 'natural';
	type RouteEnrichment = {
		uex_score: number;
		scu_origin: number;
		scu_destination: number;
		scu_reachable: number;
		status_origin: number;
		status_destination: number;
		container_sizes_origin: string;
		container_sizes_destination: string;
		user_price_origin_rows: number;
		user_price_dest_rows: number;
		game_version_origin: string | null;
		game_version_destination: string | null;
		faction_origin: string | null;
		faction_destination: string | null;
		has_quantum_marker_origin: boolean;
		has_quantum_marker_destination: boolean;
		date_added: number;
	};
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
		quantity: number;
		profit_total: number;
		cost_total: number;
		margin: number;
		distance: number;
		enrichment?: RouteEnrichment;
	};
	type RoutesResponse = { source: string; swept_at: number; refreshing: boolean; routes: TradeRoute[] };

	let routes_ = $state<TradeRoute[]>([]);
	let rLoading = $state(false);
	let rNote = $state<string | null>(null);
	let rSweptAt = $state(0);
	let rRefreshing = $state(false);
	let rSel = $state(0);
	let rDetail = $state<TradeRoute | null>(null);
	// F2 (Karte t_96120e79): Alternativ-Tabelle — alle Kauf-/Verkaufsorte der
	// gewählten Ware. Datenbasis = lokaler Routen-Datensatz (kein neuer UEX-Call).
	type RouteLocationRow = {
		terminal_id: number;
		terminal: string;
		chain: string;
		price: number;
		stock: number;
		status: number;
		autoload: boolean;
		cargo_deck: boolean;
		ground: boolean;
	};
	type RouteCommodityLocations = {
		commodity_id: number;
		commodity: string;
		buys: RouteLocationRow[];
		sells: RouteLocationRow[];
	};
	let rLocations = $state<RouteCommodityLocations | null>(null);
	// Nach dem Öffnen eines Details die Alternativ-Tabelle der Ware laden.
	let rLocsLoading = $state(false);
	$effect(() => {
		const cur = rDetail;
		if (!cur) {
			rLocations = null;
			return;
		}
		let cancelled = false;
		rLocsLoading = true;
		invoke<RouteCommodityLocations | null>('get_route_locations', {
			commodity_id: cur.commodity_id
		})
			.then((locs) => {
				if (!cancelled) rLocations = locs;
			})
			.catch(() => {
				if (!cancelled) rLocations = null;
			})
			.finally(() => {
				if (!cancelled) rLocsLoading = false;
			});
		return () => {
			cancelled = true;
		};
	});
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
	// RB2 (Karte t_9b42249a): Budget (aUEC) — kappt die kaufbare Menge je
	// Route auf min(Schiff-SCU, floor(Budget / Kaufpreis pro SCU)). Leer
	// (kein Budget) = Vollkapazität wie heute. Persistiert in localStorage
	// (überlebt Neustart).
	let rBudget = $state<string | number>('');
	const RBUDGET_KEY = 'architects-overlay.routes.budget';
	// RB2: Zahl > 0 = Budget aktiv → Menge je Route gekappt. Svelte-Number-
	// Input liefert je nach Zustand '' (leer), string (aus localStorage) oder
	// number (getippt) — normalisieren, statt roh zu trimmen.
	function rBudgetStr(): string {
		if (typeof rBudget === 'number') return Number.isFinite(rBudget) ? String(rBudget) : '';
		return rBudget ?? '';
	}
	function rBudgetNum(): number | null {
		const s = rBudgetStr().trim();
		if (s === '') return null;
		const n = Number(s);
		return Number.isFinite(n) ? n : null;
	}
	const rBudgetActive = $derived((rBudgetNum() ?? 0) > 0);
	try {
		const v = localStorage.getItem(RBUDGET_KEY);
		if (v != null) rBudget = v;
	} catch {
		/* localStorage unvollständig — leer starten. */
	}
	// Persistenz: Jede Änderung am Budget-Input sofort sichern.
	$effect(() => {
		try {
			localStorage.setItem(RBUDGET_KEY, rBudgetStr());
		} catch {
			/* localStorage unvollständig — nur In-Memory. */
		}
	});
	// Schiff aus Hangar → SCU automatisch (UEX /vehicles.scu), oder manuell.
	let rShipName = $state('');
	// P15b (Karte t_5fbd43b6): Type-ahead — Tippen kurzwidd die Ship-Selection;
	// geneueneinerer → auto-Selected (SCU applies automatically).
	let rShipQ = $state('');
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
					budget: rBudgetNum(),
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

	// RB1 (Karte t_723939ff): Start-/Zielort-Typeahead — Ort = EIN Eingabefeld
	// mit Dropdown. Backend `location_suggest` liest EntityKind::Locations
	// (Wiki+UEX merged); Shops/Terminals (EntityKind::Terminals) erscheinen
	// NICHT in der Liste. Auswahl übernimmt der Typeahead (Enter/Klick).
	let rFromSug = $state<string[]>([]);
	let rToSug = $state<string[]>([]);
	$effect(() => {
		const qf = rFrom.trim();
		const qt = rTo.trim();
		void (async () => {
			try {
				const sug = qf ? await invoke<string[]>('location_suggest', { query: qf }) : [];
				rFromSug = sug;
			} catch {
				rFromSug = [];
			}
		})();
		void (async () => {
			try {
				const sug = qt ? await invoke<string[]>('location_suggest', { query: qt }) : [];
				rToSug = sug;
			} catch {
				rToSug = [];
			}
		})();
	});
	let fromOpts = $derived(rFromSug.map((n) => ({ value: n, label: n })));
	let toOpts = $derived(rToSug.map((n) => ({ value: n, label: n })));

	// Filter/Sort-Änderung → neu laden (debounced, Text-Filter tippt sich).
	let rDebounce: ReturnType<typeof setTimeout> | null = null;
	function queueRoutes() {
		if (rDebounce) clearTimeout(rDebounce);
		rDebounce = setTimeout(() => void loadRoutes(), 250);
	}
	$effect(() => {
		// Reaktive Abhängigkeiten: alle Filter + Sort; mTab steuert den Start.
		rCommodity; rFrom; rTo; rLegal; rAutoload; rCargo; rPlace; rSort; rScu; rBudget;
		if (phase === 'market' && mTab === 'routes') {
			void loadHangar(); // Schiff-Auswahl aus dem Hangar (SCU auto)
			queueRoutes();
		}
	});

	/** Hangar-Schiffe (keine Module) für die Schiff-Auswahl. */
	let hangarShips = $derived(hangarItems.filter((i) => i.entity_type !== 'module'));

	// P15b (Karte t_5fbd43b6): Type-ahead — Tippen in dem Schiff-Fild narower die
	// <select> (Substring über Name/Custom-Name, case-insensitive). Genaw eine
	// Mate → auto-Selected, SCU auto via applyShipScu.
	let shipMatches = $derived(
		rShipQ.trim()
			? hangarShips.filter((s) => {
				const q = rShipQ.trim().toLowerCase();
				return s.name.toLowerCase().includes(q) ||
					(s.custom_name || '').toLowerCase().includes(q);
			})
			: hangarShips
	);
	// RB1: Typeahead-Kandidaten für das Schiff-Eingabefeld (Wert = echter
	// Ship-Name für applyShipScu, Label = Custom-Name falls gesetzt).
	let shipTypeaheadOpts = $derived(
		shipMatches.map((s) => ({ value: s.name, label: s.custom_name || s.name }))
	);
	$effect(() => {
		// P15b: geneueneinerer (exactly one Mate) → auto-Selected + SCU auto.
		if (phase === 'market' && mTab === 'routes' && shipMatches.length === 1) {
			const only = shipMatches[0];
			if (rShipName !== only.name) void applyShipScu(only.name);
		}
	});
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
	/** Gewinn = Einheit * Preisdifferenz * Menge (RB2: budget-gekappt). */
	function routeMath(r: TradeRoute) {
		// RB2: Budget aktiv → Menge = Backend `quantity` (budget-gekappt, kann 0
		// sein = nicht bezahlbar). Ohne Budget → SCU-Kapazität wie heute.
		const scu = rBudgetActive ? Math.max(Number(r.quantity) || 0, 0) : (rScu || 1);
		const cost = r.buy_price * scu;
		const gross = r.sell_price * scu;
		return { qty: scu, cost, gross, profit: gross - cost };
	}

	// F2 (Karte t_96120e79): die Alternativ-Tabelle ersetzt das RB3-Panel.
	// Sortierung: Kauforte nach Preis aufsteigend, Verkaufsorte nach Preis
	// absteigend — so ist der beste Handel oben. Gefiltert auf die gewählte
	// Ware per `commodity_id` (genau wie der Detail-Kopf).
	let rBuySorted = $derived.by(() => {
		const locs = rLocations;
		if (!locs) return [] as RouteLocationRow[];
		return [...locs.buys].sort((a, b) => a.price - b.price);
	});
	let rSellSorted = $derived.by(() => {
		const locs = rLocations;
		if (!locs) return [] as RouteLocationRow[];
		return [...locs.sells].sort((a, b) => b.price - a.price);
	});
	/** Klick auf einen Kauf-/Verkaufsort setzt Start/Ziel im Filter (trivial,
	 *  bestehende Logik: Typeahead-Felder binden direkt an rFrom/rTo). */
	function setRouteLocField(kind: 'from' | 'to', terminal: string) {
		if (kind === 'from') rFrom = terminal;
		else rTo = terminal;
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
		// P8-Fix: second, much longer debounce = Typing-Stille → only a
		// completed search reacheth the History (750ms after last keystroke).
		if (phase !== 'market') {
			if (recentTimer) clearTimeout(recentTimer);
			recentTimer = setTimeout(persistRecent, 750);
		}
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
		// P8-Fix: persistiert nicht hier — see persistRecent (750ms Stille).
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
		flushRecent(); // P8-Fix
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
		flushRecent(); // P8-Fix
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

	// Market-Modus betreten (F2 / Rail): P3 — wenn die Market-Daten
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

	// ---- Modi-Wechsel (Mode-Hotkeys + Rail-Klicks) ------------------------------
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
			mTab = 'offers'; // Marktplatz-Hotkey = Angebots-Tab (Routen tragen den Routen-Hotkey)
		} else if (m === 'routes') {
			// Routen-Hotkey/Rail: Handelsrouten-Tab im Marktmodus (t_a39f6811 P1).
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
	// PB (t_1c9bc240, B2): `force = true` (F5-Resync) umgeht die Cache-Frische
	// im Backend → ECHTER Live-Fetch über denselben Snapshot-Pfad wie der
	// Hintergrund-Sweep, statt nur den Cache neu zu rendern.
	async function fetchMarket(limit: number, showAll = false, bg = false, force = false) {
		if (showAll) mLoadingMore = true; else if (!bg) mLoading = true;
		mNote = null;
		const filterObj = {
			location: '',
			q: input.trim(),
			item_id: null,
			operation: mOp === 'all' ? null : mOp,
			region: '',
			sort: mSort,
			limit,
			force_refresh: force
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

	// P4/PB (t_1c9bc240, B2): F5 (app-lokal, nie Browser-Reload) — RESYNC NUR
	// AN RELEVANTEN ORTEN. Wirksamkeits-Fix: der alte Market-Pfad war cache-
	// first (UI-Reload des Snapshots, die „zuletzt gesynced“-Anzeigen blieben
	// stehen). Jetzt führt F5 denselben Pfad aus wie der Hintergrund-Sweep:
	//   Angebote-Tab   → get_marketplace mit force_refresh (echter Live-Fetch,
	//                    Snapshot-Store identisch zum Background-Sweep).
	//   Mein-Shop-Tab  → loadShop (User-scoped Pull mit eigener Quelle).
	//   Routen-Tab     → Backend-Sweep `sync_now` (SYNC-Event aktualisiert die
	//                    Statuszeile) + Routen-Reload.
	// Überall sonst: KEIN Call, kein Feedback, kein Hinweis in der Fußzeile.
	async function onF5() {
		if (phase !== 'market' || offerDetail) return;
		if (mTab === 'shop') {
			flashStatus('Mein Shop sync läuft …');
			await loadShop();
			flashStatus(
				shopErr
					? `Shop-Sync fehlgeschlagen · ${shopErr}`
					: `Mein Shop aktualisiert · Quelle ${shopSource === 'live' ? 'UEX live' : shopSource === 'cache' ? 'Cache' : 'Offline-Snapshot'}`
			);
			return;
		}
		if (mTab === 'routes') {
			flashStatus('Routen-Sweep läuft …');
			try {
				status = await invoke<SyncStatus>('sync_now');
				flashStatus(`Daten gesweept · Cache ${cacheAge(status)}`);
			} catch (e) {
				flashStatus(`Sweep fehlgeschlagen · ${String(e)}`);
				return;
			}
			await loadRoutes();
			return;
		}
		// Angebote-Tab: forcierte Market-Sync (echter Live-Fetch).
		flashStatus('Market-Sync läuft …');
		await fetchMarket(100, false, false, true);
		flashStatus(
			mNote
				? `Market-Sync fehlgeschlagen · ${mNote}`
				: `Market aktualisiert · ${offers.length} Angebote · vor ${rel(mFetchedAt || nowSec())}`
		);
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

	// P2 (t_5b6d908f): Ein einziger Gewinn/SCU-Schalter (3-Zustand) statt zwei
	// Sortier-Chips — neutral → absteigend → aufsteigend → neutral. 'natural'
	// (Backend RouteSort::Natural) lässt die Liste in der Originalliste stehen.
	function cycleProfitSort() {
		if (rSort === 'profit_desc') rSort = 'profit_asc';
		else if (rSort === 'profit_asc') rSort = 'natural';
		else rSort = 'profit_desc';
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
		// C2a (t_6565fe8b): scharfe Shortcut-Capture überschreibt alles — keys
		// während der Capture dürfen keine Modi wechseln (F12 mitten in der
		// Capture dürfte das Settings-View sonst schließen) und erreichen auch
		// kein Suchfeld. Capture konsumiert das Event vollständig.
		if (scCapture) {
			captureShortcut(e);
			return;
		}
		// M3a: Advert-Popup offen → Navigation/Escape des Marktplatzes
		// laufen NICHT mehr (Enter im Titel-Feld würde sonst das Detail
		// öffnen, Esc das Overlay schließen). Esc: Discard-Frage → Popup.
		if (advertOpen) {
			if (k === 'Escape') {
				e.preventDefault();
				if (confirmAction) confirmAction = null;
				else if (advertDiscard) advertDiscard = false;
				else requestCloseAdvert();
			}
			return;
		}
		// PB (t_1c9bc240, B1): Mode-Hotkey-Capture — VOR allen anderen Shortcuts,
		// damit die gedrückte F-Taste hier landet und keinen Modus auslöst.
		if (hkCapture) {
			if (k === 'Escape') {
				cancelModeHotkeyCapture();
				e.preventDefault();
				return;
			}
			if (/^F([1-9]|1[0-2])$/.test(k)) {
				const field = hkCapture;
				void submitModeHotkey(field, k);
				e.preventDefault();
				return;
			}
			// Keine F-Taste: Capture bleibt scharf (Hinweis bleibt stehen).
			return;
		}
		// ---- F-Tasten: In-App-Moduswechsel (Karte t_315a26cc) ----------------
		// PB (t_1c9bc240, B1): Belegung KONFIGURIERBAR — Lookup über das
		// persistierte Map (Settings/F12), Defaults in lib/hotkeys.ts:
		// F1 Suche · F2 Marktplatz · F3 Routen · F4 Inventar · F11 Hangar ·
		// F12 Einstellungen. F5 ist NICHT im Map — bleibt der kontextsensitive
		// Resync (B2, weiter unten im Handler). KEINE Hyprland-Binds — die
		// Tasten greifen nur, solange das Overlay sichtbar+fokussiert ist
		// (compositor_focus beim Show): SC verliert nie Tasten. F-Tasten
		// erzeugen keine Zeichen, sie können also auch bei Fokus im
		// Suchfeld/Stepper nicht in Eingaben geraten; einzig gegen
		// contentEditable-Editoren (die F-Tasten selbst belegen können) wird
		// mit demselben Check geschützt wie die bestehenden m/o-Shortcuts.
		if (/^F([1-9]|1[0-2])$/.test(k) && k !== 'F5') {
			if (
				typeof (e.target as HTMLElement | null)?.isContentEditable === 'boolean' &&
				(e.target as HTMLElement).isContentEditable
			) {
				return; // Rich-Text-Editor behält seine F-Tasten
			}
			const modeKey = modeKeyMap(cfg).get(k);
			if (modeKey) {
				setMode(modeKey);
				e.preventDefault();
				return;
			}
			// Nicht belegte F-Taste (z. B. F6/F7–F10): fällt durch — kein Modus.
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
		// Shortcuts (Nav/Alt-Zahlen/F2/F5) sind in dieser Ansicht gesperrt —
		// Eingabe im API-Key-Feld bleibt natürlich erlaubt.
		if (inConfig) {
			if (k === 'Escape') {
				closeConfig();
				e.preventDefault();
			}
			return;
		}
		// P4/PB (t_1c9bc240, B2): F5 ist app-lokal — nie Browser-Reload — und
		// wirkt NUR, wo Resync relevant ist (Market-Modus: Angebote, Mein Shop,
		// Routen). Suche/Detail/Rail-Modi (Inventar/Hangar)/Config: KEIN Call,
		// KEIN Feedback (der Handler ist hier nur noch erreichbar, weil die
		// F5-Behandlung NACH den railMode/inConfig-Rückgaben liegt).
		if (k === 'F5') {
			if (phase === 'market' && !offerDetail) {
				if (mTab === 'shop' && cfg && !cfg.uex_secret_key_set) {
					e.preventDefault();
					return; // ohne Secret-Key gibt es nichts zu syncen — kein Fake-Feedback
				}
				void onF5();
				e.preventDefault();
			}
			// sonst: F5 fällt durch — kein Modus, kein Sync, kein Reload.
			return;
		}
		// Marktplatz-Modus: Angebot-Navigation + Detail.
		if (phase === 'market') {
			const t = e.target;
			const inStepper = t instanceof HTMLInputElement && t.type === 'number';
			const onChip = t instanceof HTMLButtonElement;
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

		<!-- Rail: linke Modus-Leiste (Mode-Hotkeys) — gehört nur zum Overlay.
		     K2 (t_5fe8eb4c): palW ist die VIEWPORT-wide pal-root nuun (width moved
		     onto .shell); Shell-Box = max(70vw,640px) clamp 94vw ⇒ Palette <520px
		     only below ~553px viewport → threshold re-based accordingly. -->
		<!-- PB (t_1c9bc240, B1): Rail zeigt die persistierten Mode-Hotkeys. -->
		<Rail mode={mode} compact={palW > 0 && palW < 553} onselect={setMode} hotkeys={cfg?.mode_hotkeys ?? undefined} />

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
				: railMode === 'hangar'
					? 'Hangar search — Name, Custom-Name, Manufacturer or Base-Ship …'
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
							{#if hangarBaseOpen}<label>Basisschiff<input bind:value={hangarForm.base_ship} placeholder="Pick from Hangar …" /></label>{/if}
							<label>Hersteller<input bind:value={hangarForm.manufacturer} /></label>
							<label>Preis (USD)<input bind:value={hangarFormPrice} inputmode="decimal" placeholder="z. B. 180" /></label>
							<label>Datum<input bind:value={hangarForm.purchase_date} placeholder="2025-01-31" /></label>
						</div>
						<!-- P10: Autocomplete-Chips for Name + Base-Ship (the latter
						     only for module/Snub, see hangarBaseOpen). Click fills
						     Manufacturer + Typ automatically. -->
						{#if hangarSugs.length}
						<div class="hf-suggest">
							{#each hangarSugs as s (s.name)}
							<button type="button" class="hsug" title="Select — Manufacturer/Typ gets filled" onclick={() => hangarPickSuggestion(s)}>{s.name}{#if s.manufacturer} · {s.manufacturer}{/if}</button>
							{/each}
						</div>
						{/if}
						{#if hangarBaseOpen}
						<div class="hf-basesug">
							<span class="hsug-h">Basisschiff:</span>
							{#each hangarBaseSugs as n (n)}
							<button type="button" class="hsug" title="Set Basisschiff" onclick={() => hangarSetBase(n)}>{n}</button>
							{/each}
						</div>
						{/if}
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
							<div class="hangar-empty">Keine Einträge{hangarFilter !== 'all' ? ` (Filter: ${hangarFilter.toUpperCase()})` : ''} {input.trim() ? ` · Search: ${input.trim()}` : ''} — Import oben.</div>
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
									{#if hangarBaseOpen}<label>Basisschiff<input bind:value={hangarForm.base_ship} placeholder="Pick from Hangar …" /></label>{/if}
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
						<!-- K6 P2 (a): aktive Quelle deutlich — identischer Label wie
						     die Statusbar ("WIKI+UEX (UEX)"), plus Klartext, was der
						     Schalter bedeutet (die Chips wählen die PRIMÄRquelle, die
						     andere Quelle bleibt Fallback für Fehlstellen). -->
						<div class="cfg-src-active">
							Aktive Quelle:
							<b>{cfg?.commodity_source === 'uex' ? 'WIKI+UEX (UEX)' : 'WIKI+UEX (WIKI)'}</b>
							— {cfg?.commodity_source === 'uex'
								? 'Commodity-Preise primär aus UEX, Wiki ergänzt Fehlstellen.'
								: 'Commodity-Preise primär aus dem Wiki, UEX ergänzt Fehlstellen.'}
						</div>
						<!-- K6 P1-Akzeptanz: Quelle auf UEX ohne Key → klarer Hinweis. -->
						{#if cfg?.commodity_source === 'uex' && !cfg?.uex_api_key_set}
							<div class="cfg-src-warn" role="alert">
								UEX API-Key nicht gesetzt — UEX-Lesezugriff funktioniert derzeit
								ohne Key; falls UEX ablehnt (HTTP 403), greift automatisch der
								Wiki-Fallback. Key unten setzen.
							</div>
						{/if}
						<!-- K6 P2 (b): Confirm nach Umschalten (Timeout 3 s). -->
						{#if cfgToast}
							<div class="cfg-note" role="status">{cfgToast}</div>
						{/if}
					</section>
					<section class="cfg-sect">
						<div class="h">Schriftscale</div>
						<div class="cfg-chips">
							{#each [80, 90, 100, 110, 125] as pct}
								<button
									type="button"
									class="chip"
									class:is-on={cfg?.scale === pct}
									onclick={() => void chooseScale(pct)}
								>{pct} %</button>
							{/each}
						</div>
					</section>
					<section class="cfg-sect">
						<div class="h">Tastenkürzel</div>
						{#each shortcutFields as f (f.id)}
							<div class="cfg-sc-row" class:is-cap={scCapture === f.id}>
								<div class="cfg-sc-meta">
									<div class="cfg-sc-lbl">{f.label}</div>
									<!-- Soll-Ist: aktiv = persistierter Backend-Wert
									     (überlebt Neustart), nicht der Entwurf im Feld. -->
									<div class="cfg-sc-active">
										Aktiv: <b>{f.display}</b>
									</div>
								</div>
								<button
									type="button"
									class="chip cfg-sc-capture"
									class:is-on={scCapture === f.id}
									onclick={() => (scCapture === f.id ? cancelShortcutCapture() : startShortcutCapture(f.id))}
								>{scCapture === f.id ? 'Abbrechen (Esc)' : 'Ändern'}</button>
								{#if scCapture === f.id}
									<div class="cfg-sc-help" role="status">
										{scPreview ? `Gewählt: ${scPreview} — prüfen…` : 'Mod-Taste (Strg/Alt/Shift/Win) + normale Taste drücken'}
									</div>
								{/if}
								{#if scNoteFor === f.id}
									<div class="cfg-sc-err" role="alert">{scNote}</div>
								{/if}
							</div>
						{/each}
						<!-- Konfliktliste + Ehrlichkeit zum Registrierungszustand
					     (Preisquellen-Feedback-Muster, K6): die In-App-Handler
					     prüfen keine Mod-Tasten, darum sind diese Tasten für
					     globale Kürzel gesperrt. -->
						<div class="cfg-sc-note">
							Für globale Hotkeys gesperrt: F-Tasten sind die Mode-Hotkeys
							(konfigurierbar, Abschnitt „Mode-Hotkeys“), F5 führt den Resync aus.
							Weitere interne Belegungen: Esc, Enter, Tab, Pfeiltasten, m, Alt+Zahlen.
							{#if cfg && cfg.hyprland_session}
								<!-- C2b (t_1adbc017): Hyprland führt die Kürzel als
								     Compositor-Binds. OmaSettings-Pfad nur mit
								     einmaliger Zustimmung; ohne Plugin bleibt der
								     dokumentierte manuelle Weg (nichts wird geschrieben). -->
								{#if cfg.omasettings_available}
									<div class="cfg-sc-row">
										<button
											type="button"
											class="chip"
											class:is-on={cfg.shortcuts_sync_omarchy}
											onclick={() => void toggleOmarchySync()}
										>{cfg.shortcuts_sync_omarchy ? '✓' : '○'} Tastenkürzel mit System (Omarchy) synchronisieren</button>
									</div>
									{#if cfg.shortcuts_sync_omarchy}
										<div class="cfg-sc-ok" role="status">
											Hyprland-Binds werden aus den gespeicherten Kürzeln geführt
											(<span class="src">omasettings keys</span> ·
											<span class="src">arch-overlay toggle</span>) — Änderungen wirken sofort.
										</div>
									{:else}
										<div class="cfg-sc-warn" role="status">
											Gespeicherte Kürzel sind die kanonische Quelle, aber die
											Hyprland-Binds werden noch nicht daraus geführt.
										</div>
									{/if}
								{:else}
									<div class="cfg-sc-warn" role="status">
										OmaSettings-Plugin nicht gefunden — Binds manuell anlegen
										(<span class="src">arch-overlay toggle</span>, Anleitung: docs/WAYLAND.md).
										Die App verändert keine fremde Hyprland-Konfiguration.
									</div>
								{/if}
								{#if cfg.sync_note}
									<div class="cfg-note" role="status">{cfg.sync_note}</div>
								{/if}
							{:else if cfg && !cfg.global_shortcuts_active}
								<div class="cfg-sc-warn" role="status">
									Globale Kürzel <b>nicht OS-seitig registriert</b> (reines Wayland ohne Hyprland):
									die gespeicherten Kombinationen bleiben die kanonische Quelle; Compositor-Binds
									manuell nach docs/WAYLAND.md anlegen.
								</div>
							{:else if cfg}
								<div class="cfg-sc-ok" role="status">Globale Kürzel OS-seitig registriert — Änderungen wirken sofort ohne Neustart.</div>
							{/if}
						</div>
					</section>
					<!-- PB (t_1c9bc240, B1): Mode-Hotkeys — konfigurierbare F-Tasten der
					     In-App-Modi. Persistiert im Settings-Crate (TOML), Validierung:
					     nur F1–F12, F5 reserviert für den Resync, keine Doppelbelegung.
					     Capture-Muster identisch zu den globalen Kürzeln (C2a). -->
					<section class="cfg-sect">
						<div class="h">Mode-Hotkeys</div>
						{#each modeHotkeyFields as f (f.id)}
							<div class="cfg-sc-row" class:is-cap={hkCapture === f.id}>
								<div class="cfg-sc-meta">
									<div class="cfg-sc-lbl">{f.label}</div>
									<div class="cfg-sc-active">
										Aktiv: <b>{hotkeyFor(cfg, f.id)}</b>
									</div>
								</div>
								<button
									type="button"
									class="chip cfg-sc-capture"
									class:is-on={hkCapture === f.id}
									onclick={() => (hkCapture === f.id ? (hkCapture = null) : startModeHotkeyCapture(f.id))}
								>{hkCapture === f.id ? 'Abbrechen (Esc)' : 'Ändern'}</button>
							</div>
						{/each}
						{#if hkCapture}
							<div class="cfg-sc-help" role="status">
								F-Taste drücken (F1–F12) für „{modeHotkeyFields.find((f) => f.id === hkCapture)?.label}“ — Esc bricht ab.
							</div>
						{/if}
						{#if hkNote}
							<div class="cfg-sc-err" role="alert">{hkNote}</div>
						{/if}
						<div class="cfg-sc-note">
							F5 ist fest für den Resync reserviert (Marktplatz, Mein Shop, Routen-Sweep)
							und kann keinem Modus zugewiesen werden. Änderungen gelten sofort.
							<button type="button" class="chip" onclick={() => void resetModeHotkeys()}>Standard-Belegung</button>
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
							<button type="button" class="btn-ghost btn-danger" onclick={removeApiKey}>Entfernen</button>
							{/if}
						</div>
						{#if apiKeyNote}
							<div class="cfg-note">{apiKeyNote}</div>
						{/if}
					</section>
					<section class="cfg-sect">
						<div class="h">UEX persönlicher Secret-Key · UEX-Username</div>
						<div class="sm" style="margin-bottom: 8px; color: var(--fgdim)">
							Für den eigenen Shop („Mein Shop“, user-scoped Reads) braucht UEX
							zusätzlich deinen persönlichen Secret-Key (40 Zeichen, noch maskierter
							als der App-Key) und den Usernamen, exakt wie auf uexcorp.space registriert.
						</div>
						<div class="cfg-key-status">
							Secret-Key:
							<b class:ok={cfg?.uex_secret_key_set} class:off={!cfg?.uex_secret_key_set}>
								{cfg?.uex_secret_key_set ? 'gesetzt' : 'nicht gesetzt'}
							</b>
						</div>
						<div class="cfg-key-row">
							<input
								type="password"
								autocomplete="off"
								placeholder="Secret-Key einfügen … (maskiert)"
								bind:value={secretKeyInput}
								onkeydown={(e) => e.stopPropagation()}
							/>
							<button type="button" class="btn" onclick={saveSecretKey} disabled={!secretKeyInput.trim()}>
								Setzen
							</button>
							{#if cfg?.uex_secret_key_set}
							<button type="button" class="btn-ghost btn-danger" onclick={removeSecretKey}>Entfernen</button>
							{/if}
						</div>
						{#if secretKeyNote}
							<div class="cfg-note">{secretKeyNote}</div>
						{/if}
						<div class="cfg-key-status" style="margin-top: 10px">
							UEX-Username:
							<b class:ok={cfg?.uex_username?.trim()} class:off={!cfg?.uex_username?.trim()}>
								{cfg?.uex_username?.trim() ? cfg.uex_username.trim() : 'nicht gesetzt'}
							</b>
						</div>
						<div class="cfg-key-row">
							<input
								type="text"
								autocomplete="off"
								placeholder="z. B. sebo_one"
								bind:value={usernameInput}
								onkeydown={(e) => e.stopPropagation()}
							/>
							<button type="button" class="btn" onclick={saveUsername} disabled={!usernameInput.trim()}>
								Setzen
							</button>
						</div>
						{#if usernameNote}
							<div class="cfg-note">{usernameNote}</div>
						{/if}
					
												<div class="cfg-key-status" style="margin-top: 12px">
							Schreibbestätigung (2-Schritt):
							<button type="button" class="chip" class:is-on={cfg?.marketplace_confirm !== 'delete_only'} onclick={() => void setMarketplaceConfirm('every_action')}>Nach jeder Aktion (Default)</button>
							<button type="button" class="chip" class:is-on={cfg?.marketplace_confirm === 'delete_only'} onclick={() => void setMarketplaceConfirm('delete_only')}>Nur beim Abmelden</button>
						</div>
						{#if mktConfirmNote}
							<div class="cfg-note">{mktConfirmNote}</div>
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
					<span class="btn-ghost" onclick={retrySync}>Erneut versuchen</span>
					<div class="sm" style="margin-top: 10px">
						<b style="color: var(--fgdim)">O</b> Offline mit Cache fortsetzen
					</div>
				</div>
			</div>

		{:else if phase === 'market'}
			<div class="pane-market">
			<!-- PP t_e2cdb571 (2): Tablist nur im MARKTPLATZ-Mode (mode === 'market').
			     Root-Cause des Reiter-Leaks: der bisherige Guard hing an der Phase
			     (phase === 'market') — der Routen-View ist intern dieselbe Phase mit
			     mTab='routes', also blieben „Angebote"/„Mein Shop" dort stehen.
			     Der Tab-State selbst leakt nicht; die Tablist hing am falschen
			     Zustand. Jetzt am abgeleiteten Mode (routes = kein Marktplatz-Reiter). -->
			{#if !offerDetail && mode === 'market'}
				<div class="mtabs" role="tablist">
					<button type="button" role="tab" aria-selected={mTab === 'offers'} class="chip" class:is-on={mTab === 'offers'} onclick={() => (mTab = 'offers')}>Angebote</button>
					<!-- t_350c4300 B1: kein „Handelsrouten"-Tab im Marktplatz — Routen
					     sind nur noch über das Navigationsband (Rail) bzw. den
					     Mode-Hotkey für Routen (Default F3, konfigurierbar)
					     erreichbar (setMode('routes') → mTab='routes' bleibt der
					     interne Mechanismus). -->
					<button type="button" role="tab" aria-selected={mTab === 'shop'} class="chip" class:is-on={mTab === 'shop'} onclick={() => (mTab = 'shop')}>Mein Shop</button>
					<!-- M3b (t_893761f1): Trades-Reiter — Deals + Chat im Overlay. -->
					<button type="button" role="tab" aria-selected={mTab === 'trades'} class="chip" class:is-on={mTab === 'trades'} onclick={() => (mTab = 'trades')}>Trades</button>
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
						<!-- M3a-R1: Bilder per Backend-Proxy (Base64-Data-URI) — direkte
						     CDN-URLs werden im Webview ORB-blockiert (octet-stream+nosniff).
						     Fehlgeschlagene Fotos (403/Timeout/kein JPG/PNG) → Kachel weg;
						     wenn KEIN Foto lädt → Leerzustand "Kein Bild hinterlegt." -->
						{#if offerDetail.photos?.length}
							<div class="md-photos">
								{#each offerDetail.photos as ph, pi (ph + '#' + pi)}
									{#if offerPhotoUris[pi]}
										<button
											type="button"
											class="md-photo"
											title="Bild groß öffnen (uexcorp.space)"
											onclick={() => void openListingDeepLink(offerDetail!.slug)}
										>
											<img src={offerPhotoUris[pi]} alt={'Bild ' + (pi + 1) + ' von „' + offerDetail!.title + '“'} loading="lazy" />
										</button>
									{/if}
								{/each}
								{#if offerPhotoUris.length > 0 && offerPhotoUris.every((u) => u === null)}
									<div class="md-desc empty">Kein Bild hinterlegt.</div>
								{/if}
							</div>
						{:else}
							<div class="md-desc empty">Kein Bild hinterlegt.</div>
						{/if}
						{#if offerDetail.description}
							<div class="md-desc">{offerDetail.description}</div>
						{:else}
							<div class="md-desc empty">Keine Beschreibung.</div>
						{/if}
						<div class="md-actions">
							<span class="btn-ghost" onclick={openOfferDeepLink}>AUF UEXCORP.SPACE ÖFFNEN ↗</span>
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
									<div class="u">aUEC Gewinn bei {routeMath(rDetail).qty} SCU</div>
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
								<span><b>Menge</b> {fmtQty(routeMath(rDetail).qty)} SCU</span>
								<span><b>Einkauf</b> {fmtPrice(routeMath(rDetail).cost)} aUEC</span>
								<span><b>Brutto</b> {fmtPrice(routeMath(rDetail).gross)} aUEC</span>
								<span title="Gain = (Sell−Buy) × Menge · absolute"><b>Netto-Gewinn</b> {fmtPrice(routeMath(rDetail).profit)} aUEC</span>
								<span title="Marge = (Sell−Buy) ÷ Buy · relative gain in % (independent of SCU)"><b>Marge</b> {(rDetail.margin * 100).toFixed(1)} %</span>
								<span><b>Bestand dort</b> {fmtPrice(rDetail.sell_stock)} SCU</span>
						</div>
						{#if rLocations && (rBuySorted.length > 0 || rSellSorted.length > 0)}
												<div class="rd-pair">
													<div class="rd-pair-col">
														<div class="rd-pair-head">Kauforte <span class="rd-pair-sub">Preis ↑</span></div>
														{#if rBuySorted.length === 0}
															<div class="rd-pair-empty">keine Kauforte</div>
														{:else}
															{#each rBuySorted as loc (loc.terminal_id)}
																<button type="button" class="rd-pair-row" data-side="buy" data-term-id={loc.terminal_id}
																	class:is-selected={loc.terminal_id === rDetail?.buy_terminal_id}
																	title={loc.terminal_id === rDetail?.buy_terminal_id ? 'Aktueller Kaufort dieser Route' : 'Kaufort im Filter setzen'}
																	onclick={() => setRouteLocField('from', loc.terminal)}>
																	<span class="rd-pair-main">
																		<span class="rd-pair-term">{loc.terminal}</span>
																		<span class="rd-pair-chain">{loc.chain}</span>
																	</span>
																	<span class="rd-pair-right">
																		<span class="rd-pair-price">{fmtPrice(loc.price)}</span>
																		<span class="rd-pair-meta">
																			<span class="r-chip" title="Bestand am Terminal">≋ {fmt(Math.round(loc.stock))} SCU</span>
																			{#if loc.status > 0}<span class="r-chip" title="Nachfrage (Lager-Level)">{loc.status}/7</span>{/if}
																			{#if loc.autoload}<span class="r-chip">Autoload</span>{/if}
																		</span>
																	</span>
																</button>
															{/each}
														{/if}
													</div>
													<div class="rd-pair-col">
														<div class="rd-pair-head">Verkaufsorte <span class="rd-pair-sub">Preis ↓</span></div>
														{#if rSellSorted.length === 0}
															<div class="rd-pair-empty">keine Verkaufsorte</div>
														{:else}
															{#each rSellSorted as loc (loc.terminal_id)}
																<button type="button" class="rd-pair-row" data-side="sell" data-term-id={loc.terminal_id}
																	class:is-selected={loc.terminal_id === rDetail?.sell_terminal_id}
																	title={loc.terminal_id === rDetail?.sell_terminal_id ? 'Aktueller Verkaufsort dieser Route' : 'Verkaufsort im Filter setzen'}
																	onclick={() => setRouteLocField('to', loc.terminal)}>
																	<span class="rd-pair-main">
																		<span class="rd-pair-term">{loc.terminal}</span>
																		<span class="rd-pair-chain">{loc.chain}</span>
																	</span>
																	<span class="rd-pair-right">
																		<span class="rd-pair-price sell">+{fmtPrice(loc.price)}</span>
																		<span class="rd-pair-meta">
																			<span class="r-chip" title="Bestand am Terminal">≋ {fmt(Math.round(loc.stock))} SCU</span>
																			{#if loc.status > 0}<span class="r-chip" title="Nachfrage (Lager-Level)">{loc.status}/7</span>{/if}
																			{#if loc.autoload}<span class="r-chip">Autoload</span>{/if}
																		</span>
																	</span>
																</button>
															{/each}
														{/if}
													</div>
												</div>
											{:else if rDetail && !rLocsLoading}
												<div class="rd-alt-none">keine Alternativorte für {rDetail.commodity}</div>
											{/if}
						<div class="md-note">Preisstand: {rSweptAt ? new Date(rSweptAt * 1000).toLocaleString('de-DE') : '—'} · Quelle: UEX</div>
							<div class="md-actions">
								<span class="btn-ghost" onclick={() => (rDetail = null)}>Zurück (Esc)</span>
							</div>
						</div>
					{:else}
						<div class="mbar2">
							<span class="m-lbl">Schiff</span>
							<RoutesTypeahead bind:value={rShipQ} options={shipTypeaheadOpts} placeholder="z. B. Ironclad" ariaLabel="Schiff Type-ahead — Tippen filtert, ↵ übernimmt" onselect={(v: string) => applyShipScu(v)} />
							<input class="qnum" type="number" min="0" placeholder="SCU" aria-label="SCU-Kapazität" bind:value={rScu} />
							<span class="m-lbl">Budget</span>
							<div class="rb-wrap">
								<input class="qnum rb-qnum" type="number" min="0" step="any" placeholder="aUEC" aria-label="Budget (aUEC) — kappt kaufbare Menge" bind:value={rBudget} />
								{#if rBudgetNum() !== null && rBudgetNum()! > 0}
									<span class="rb-fmt" title="Budget in aUEC (formatiert)">{fmtPrice(rBudgetNum()!)}</span>
								{/if}
							</div>
							<span class="m-lbl">Ware</span>
							<input class="rtext" placeholder="Ware" aria-label="Ware filtern" bind:value={rCommodity} />
							<span class="m-lbl">Start</span>
							<RoutesTypeahead bind:value={rFrom} options={fromOpts} placeholder="Startort" ariaLabel="Startort Type-ahead — Tippen filtert, ↵ übernimmt" />
							<span class="m-lbl">Ziel</span>
							<RoutesTypeahead bind:value={rTo} options={toOpts} placeholder="Zielort" ariaLabel="Zielort Type-ahead — Tippen filtert, ↵ übernimmt" />
							<a class="rta-clear" role="button" tabindex="0" title="Filter zurücksetzen" onclick={() => { rShipQ = ''; rScu = 0; rCommodity = ''; rFrom = ''; rTo = ''; rBudget = ''; }}>Filter ×</a>
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
							<button
								type="button"
								class="chip"
								class:is-on={rSort === 'profit_desc' || rSort === 'profit_asc'}
								onclick={cycleProfitSort}
								title="Gewinn/SCU umschalten: neutral → absteigend → aufsteigend → neutral"
							>Gewinn/SCU{rSort === 'profit_desc' ? ' ↓' : rSort === 'profit_asc' ? ' ↑' : ''}</button>
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
										class:is-rbzero={rBudgetActive && r.quantity <= 0}
										data-route-idx={i}
										onclick={() => { rSel = i; openRouteDetail(); }}
										>
									<div class="m-main">
										<div class="m-title">{r.commodity} {r.illegal ? '⚠' : ''}</div>
										<div class="m-sub">{r.buy_terminal} → {r.sell_terminal} · {DIST_LABELS[r.distance] ?? r.distance}</div>
										{#if r.enrichment && (r.enrichment.scu_reachable > 0 || r.enrichment.scu_destination > 0)}
											<div class="r-chip" title="UEX-Enrichment (Option b): Volumen/Nachfrage/Scarcity — nur Zusatzinfo, nicht sortierrelevant">
												<span class="r-chip-vol">≋ {r.enrichment.scu_reachable > 0 ? fmt(Math.round(r.enrichment.scu_reachable)) : fmt(Math.round(r.enrichment.scu_destination))} SCU</span>
												{#if r.enrichment.status_destination > 0}
												<span class="r-chip-stock" title="Nachfrage-Level Verkauf (1–7)">{r.enrichment.status_destination}</span>
												{/if}
											</div>
										{/if}
										{#if rBudgetActive}
										<div class="r-chip r-chip-menge" title="RB2: mit diesem Budget kaufbare Menge (min SCHIFF-SCU, floor Budget ÷ Kaufpreis)">Menge {fmtQty(r.quantity)} SCU</div>
										{/if}
									</div>
									<div class="m-right">
										<div class="m-price sell" title="Gain/SCU = Sell − Buy · absolute aUEC per SCU">+{fmtPrice(r.profit_per_scu)}</div>
										<div class="m-meta">{fmtPrice(r.profit_total)} aUEC · <span title="Marge = relative: Gain/SCU ÷ Kauf-Preisdifference in % (independent of SCU; not just Gain/SCU rescaled)">Marge {(r.margin * 100).toFixed(0)} %</span></div>
										{#if rBudgetActive && r.quantity <= 0}<div class="r-hint">Budget zu niedrig für 1 SCU</div>{/if}
									</div>
									</div>
								{/each}
							</div>
							<div class="m-foot">
								<span>{routes_.length} Routen · Gain/SCU = Sell − Buy (absolute aUEC/SCU) · Marge = relative % (Gain/SCU ÷ Buy)</span>
								<span class="src">UEX Corp</span>
							</div>
						{:else}
							<div class="row-empty" style="padding: 28px 16px">Keine Routen für diese Filter.</div>
						{/if}
					{/if}
				{:else if mTab === 'trades'}
					<!-- M3b (t_893761f1): Trades-Reiter — Deal-Liste (get_my_negotiations,
					     stale-first) + Chat im Overlay (GET/POST
					     /marketplace_negotiations_messages). Lifecycle-Aktionen
					     (Starten/Beenden/Bewerten) sind API-SEITIG nicht vorhanden
					     (Research §3) → Deep-Link-Buttons, keine Fake-Buttons. -->
					{#if tradesDeal}
						<div class="mbar">
							<span class="chip" onclick={() => (tradesDeal = null)}>← Deals</span>
							<span class="m-lbl" style="flex:1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap">
								{tradesDeal.listing_title}
							</span>
							<span class="chip" title="Deal auf uexcorp.space öffnen (Listing-Seite)" onclick={() => void openTradesDeepLink(tradesDeal!, 'Deal öffnen')}>Auf UEX öffnen ↗</span>
						</div>
						<div class="chat" bind:this={chatScroller}>
							{#if chatLoading}
								<div class="center" style="padding: 20px 0"><span class="spin"></span><div class="big" style="margin-top: 10px">Lade Verlauf …</div></div>
							{:else if chatMsgs.length === 0}
								<div class="row-empty" style="padding: 14px 16px">Noch keine Nachrichten in dieser Verhandlung.</div>
							{:else}
								{#each chatMsgs as m (m.id)}
									{#if m.event && !m.message}
										<div class="chat-ev">{m.event}</div>
									{:else}
										<div class="chat-msg {m.user_username === shopUser.trim() ? 'own' : ''}">
											<div class="chat-head">
												<span class="chat-who">{m.user_name || m.user_username || '—'}</span>
												<span class="chat-time">{dealTime(m.date_added)}</span>
											</div>
											<div class="chat-text">{m.message}</div>
										</div>
									{/if}
								{/each}
							{/if}
						</div>
						{#if chatErr}
							<div class="row-empty" style="padding: 10px 16px; color: var(--alert); display: flex; align-items: center; gap: 12px" role="alert">
								<span style="flex: 1">{chatErr}</span>
								<!-- Send-Fehlerpfad (429/403): Meldung + Deep-Link statt Retry. -->
								<button type="button" class="btn-ghost" style="color: var(--alert)" onclick={() => void openTradesDeepLink(tradesDeal!, 'Fehler-Pfad')}>AUF UEXCORP.SPACE ÖFFNEN ↗</button>
							</div>
						{/if}
						<div class="chat-input">
							<input
								class="rtext"
								placeholder="Nachricht an {tradesDeal.is_listing_advertiser ? (tradesDeal.client_username || 'Käufer') : (tradesDeal.advertiser_username || 'Anbieter')} …"
								bind:value={chatDraft}
								onkeydown={chatKey}
								maxlength={65535}
								aria-label="Chat-Nachricht senden"
							/>
							<button type="button" class="btn" disabled={chatBusy || !chatDraft.trim()} onclick={() => void sendChat()}>
								{chatBusy ? 'Senden …' : 'Senden'}
							</button>
						</div>
					{:else}
						<div class="mbar">
							<span class="m-lbl">Deals</span>
							<span class="chip" onclick={() => void loadDeals(true)}>Aktualisieren</span>
							<span class="chip" title="Eigene Listings im UEX-Manager öffnen (Login im Browser)" onclick={openMyShopDeepLink}>Listings verwalten ↗</span>
						</div>
						{#if tradesLoading}
							<div class="center">
								<span class="spin"></span>
								<div class="big" style="margin-top: 12px">Lade Trades …</div>
							</div>
						{:else if tradesErr && !myNegotiations.length}
							<div class="pane-error">
								<div class="center">
									<div class="big" style="color: var(--alert)">Trades nicht erreichbar</div>
									<div class="sm">{tradesErr}</div>
									<span class="btn-ghost" onclick={() => void loadDeals(false)}>Erneut versuchen</span>
									<span class="btn-ghost" onclick={openMyShopDeepLink}>AUF UEXCORP.SPACE ÖFFNEN ↗</span>
								</div>
							</div>
						{:else if dealsSorted.length === 0}
							<div class="row-empty" style="padding: 28px 16px">
								{tradesNote ?? 'Keine Verhandlungen/Abschlüsse zu deinen Listings — starte einen Deal auf uexcorp.space.'}
							</div>
						{:else}
							{#if tradesNote}
								<div class="row-empty" style="padding: 10px 16px">{tradesNote}</div>
							{/if}
							<div class="mlist">
								{#each dealsSorted as n (n.id)}
									<div
										class="mrow trow"
										role="option"
										aria-selected={false}
										title="Deal-Chat öffnen"
										onclick={() => void selectDeal(n)}
									>
										<div class="m-op {dealRole(n)}">{dealRole(n) === 'sell' ? 'Verkauf' : 'Kauf'}</div>
										<div class="m-main">
											<div class="m-title">{n.listing_title}</div>
											<div class="m-sub">
												{n.is_listing_advertiser
													? `Gegenüber ${n.client_username || n.client_name || '—'} (Käufer)`
													: `Gegenüber ${n.advertiser_username || n.advertiser_name || '—'} (Anbieter)`}
											</div>
										</div>
										<div class="m-right">
											<div class="m-meta">{dealTime(n.date_modified || n.date_added)}</div>
											<div class="m-meta">{dealStatus(n)}{#if dealUnread(n)}<span class="chat-dot" title="Neue Interaktion"></span>{/if}</div>
										</div>
									</div>
								{/each}
							</div>
							<div class="m-foot">
								<span>{dealsSorted.length} Deals · Chat im Overlay · Starten/Beenden/Bewerten nur auf UEX (Deep-Link) · Polling 1/min</span>
								<span class="src">UEX Corp</span>
							</div>
						{/if}
					{/if}
				{:else if mTab === 'shop'}
					<div class="mbar">
						<span class="m-lbl">UEX-Username</span>
						<input
							class="rtext"
							placeholder="z. B. sebo_one"
							aria-label="UEX-Benutzername — filtert deine eigenen Offers"
							title="UEX-Benutzername aus den Settings/GitHub-Account"
							bind:value={shopUser}
							onkeydown={(e) => {
								if (e.key === 'Enter') {
									persistShopUser(shopUser);
									void loadShop();
								}
							}}
						/>
						<span
							class="btn-ghost"
							onclick={() => {
								persistShopUser(shopUser);
								void loadShop();
							}}
						>Laden</span>
						<span class="m-spacer"></span>
						{#if shopLoading}<span class="chip">↻ …</span>{/if}
						{#if shopSource === 'offline'}
							<span class="chip warn"><span class="dot"></span>Offline-Snapshot</span>
						{:else if shopSource === 'cache'}
							<span class="chip"><span class="dot"></span>Cache</span>
						{/if}
					</div>
					{#if !cfg?.uex_api_key_set}
						<div class="pane-error" style="margin: 10px 0">
							<div class="center">
								<div class="big" style="color: var(--alert)">UEX API-Key nicht gesetzt</div>
								<div class="sm">Eigene Offers + Marktplatz-Trades brauchen deinen UEX-API-Key — ohne Key bleibt der Abschnitt leer.</div>
								<span class="btn-ghost" onclick={openConfig}>In den Einstellungen hinterlegen</span>
							</div>
						</div>
					{:else if !cfg?.uex_secret_key_set}
						<!-- M2a-Fix (t_2f197d1c): ohne persönlichen Secret-Key KEIN
						     Request (UEX antwortet sonst nur 400 user_not_found /
						     403 invalid_secret_key) — stattdessen klare deutsche
						     Meldung + Settings-Verweis. -->
						<div class="pane-error" style="margin: 10px 0">
							<div class="center">
								<div class="big" style="color: var(--alert)">Secret-Key fehlt oder ist falsch — siehe Einstellungen</div>
								<div class="sm">User-scoped Reads (eigene Offers, Marktplatz-Trades) brauchen zusätzlich zum App-Key den „UEX persönlichen Secret-Key“ (40 Zeichen) — exakt wie auf uexcorp.space registriert.</div>
								<button type="button" class="btn-ghost" onclick={openConfig}>In den Einstellungen hinterlegen</button>
							</div>
						</div>
					{:else if shopLoading}
						<div class="center">
							<span class="spin"></span>
							<div class="big" style="margin-top: 12px">Lade Mein Shop …</div>
							<div class="sm">Eigene Offers + Marktplatz-Trades vom UEX-Client.</div>
						</div>
					{:else if shopErr && !myOffers.length && !myNegotiations.length}
						<div class="pane-error">
							<div class="center">
								<div class="big" style="color: var(--alert)">Mein Shop nicht erreichbar</div>
								<div class="sm">{shopErr}</div>
								<span class="btn-ghost" onclick={() => void loadShop()}>Erneut versuchen</span>
								<span class="btn-ghost" onclick={openMyShopDeepLink}>AUF UEXCORP.SPACE ÖFFNEN ↗</span>
							</div>
						</div>
					{:else}
						{#if shopErr}
							<div class="row-empty" style="padding: 10px 16px; color: var(--alert); display: flex; align-items: center; gap: 12px" role="alert">
								<span style="flex: 1">{shopErr}</span>
								<!-- M2b-3: Fehlerpfad — Meldung + Deep-Link-Button (passende
								     UEX-Seite: Marketplace-Home, dort weitermachen). Kein Retry. -->
								<button type="button" class="btn-ghost" style="color: var(--alert)" onclick={() => void openListingDeepLink(null)}>AUF UEXCORP.SPACE ÖFFNEN ↗</button>
							</div>
						{/if}
						{#if shopNote}
												<div class="row-empty" style="padding: 10px 16px; display: flex; align-items: center; gap: 12px">
													<span style="flex: 1">{shopNote}</span>
													{#if shopCreatedUrl}
														<!-- M2b-Review-Fix (t_6e26ec82 R2): Erfolgs-Deep-Link aus der
														     Create-Antwort (Listings erscheinen erst nach UEX-Approval —
														     der Pull kann sie nicht zeigen, der Direct-Link schon). -->
														<button type="button" class="btn-ghost" onclick={() => void openCreatedListing(shopCreatedUrl!)}>ANGELEGTES LISTING ÖFFNEN ↗</button>
													{/if}
												</div>
											{/if}
											<div class="mbar2" style="margin-top: 10px">
													<span class="m-lbl">Eigene Offers ({myOffers.length})</span>
													<!-- M3a: Filter Alle/Aktiv/Wartend (kleine Chips) — pending
													     Listings (date_approved = 0) bleiben sichtbar statt
													     unsichtbar zu sein, bis UEX sie freigibt. -->
													<button type="button" class="chip" class:is-on={shopFilter === 'all'} onclick={() => (shopFilter = 'all')}>Alle</button>
													<button type="button" class="chip" class:is-on={shopFilter === 'active'} onclick={() => (shopFilter = 'active')}>Aktiv</button>
													<button type="button" class="chip" class:is-on={shopFilter === 'pending'} onclick={() => (shopFilter = 'pending')}>Wartend</button>
													<span class="m-spacer"></span>
													<span class="chip" title="Anlegen = NEUES Listing posten (UEX marketplace_advertise). Bearbeiten gibt es nicht — Abmelden + Neu anlegen." onclick={() => void openAdvertise(null)}>Anlegen</span>
													<span class="chip" title="Marketplace auf uexcorp.space öffnen" onclick={() => void openListingDeepLink(null)}>Im Browser öffnen ↗</span>
												</div>
												{#if advertOpen}
												<!-- M3a: das „Neues Listing“-Formular als modales Popup
												     (gleiches Muster wie die 2-Schritt-Bestätigung: Scrim +
												     feste Karte). Passt in die Overlay-Größe, mlist-Body
												     scrollt intern, Button-Leiste bleibt erreichbar. -->
													<div class="advert-scrim" role="presentation">
														<div class="advert-modal" role="dialog" aria-modal="true" aria-label="Neues Listing anlegen">
													<div class="pane-error advert-pane" style="background: rgba(255,255,255,.03)">
														<div class="mbar2"><span class="m-lbl">{editingOffer ? 'NEU ANLEGEN (Bearbeiten = erst Abmelden, dann Neu)' : 'NEUES LISTING ANLEGEN'}</span></div>
														{#if editingOffer}
															<div class="row-empty" style="padding: 8px 12px">Dieses Formular ist VORBEFÜLLT mit „{editingOffer.title}“. Für eine Änderung musst du das alte Listing zuerst ABmelden (unten) und dann mit den neuen Werten neu anlegen — UEX unterstützt kein Bearbeiten.</div>
														{/if}
														<div class="mlist" style="padding: 6px 14px">
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px">
																<span class="m-lbl">Art</span>
																<button type="button" class="chip" class:is-on={advertForm.operation === 'sell'} onclick={() => (advertForm.operation = 'sell')}>Verkauf (sell)</button>
																<button type="button" class="chip" class:is-on={advertForm.operation === 'buy'} onclick={() => (advertForm.operation = 'buy')}>Kauf (buy)</button>
																<span class="m-lbl">Listing Type</span>
																<SelectCombo
																	bind:value={advertForm.listing_type}
																	ariaLabel="Listing Type"
																	options={[
																		{ value: 'item', label: 'Item' },
																		{ value: 'service', label: 'Service' },
																		{ value: 'contract', label: 'Contract' }
																	]}
																	onselect={() => {
																		// Unit-Referenzliste wechselt mit dem Type (Doku [7]);
																		// fallback auf die erste Unit der neuen Liste.
																		const u = unitsFor(advertForm.listing_type);
																		if (!u.includes(advertForm.unit)) advertForm.unit = u[0];
																	}}
																	/>
																	{#if advertErrors.listing_type}<span class="chip warn" style="margin-left: 4px">{advertErrors.listing_type}</span>{/if}
																	</div>
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px">
																<span class="m-lbl">Titel</span>
																				<!-- NN (t_060d134e): breit genug für lange Titel (z. B.
																				     „Atzkav Sniper Rifle 970 Q Crafted …“) ohne Abschneiden. -->
																				<input class="rtext" style="width: 560px" placeholder="Artikelname (alphanumerisch + -)" maxlength="140" bind:value={advertForm.title} />
																				{#if advertErrors.title}<span class="chip warn" style="margin-left: 4px">{advertErrors.title}</span>{/if}
																				<span class="m-lbl">Preis</span>
																				<!-- NN (t_060d134e): breit für einstellige Milliarden +
																				     Tausender-Punkte bei der Eingabe (gespeichert wird die
																				     reine Ziffernfolge — Validierung/Parse unverändert). -->
																				<input
																					class="qnum"
																					style="width: 170px"
																					type="text"
																					inputmode="numeric"
																					placeholder="z. B. 1.500.000.000"
																					aria-label="Preis (Ganzzahl, Tausender-Punkte werden bei der Eingabe angezeigt)"
																					value={fmtGroup(advertForm.price)}
																					oninput={onPriceInput}
																				/>
																{#if advertErrors.price}<span class="chip warn" style="margin-left: 4px">{advertErrors.price}</span>{/if}
																<span class="m-lbl">Währung</span>
																<SelectCombo bind:value={advertForm.currency} ariaLabel="Currency" placeholder="UEC" options={CURRENCY_CHOICES.map((c) => ({ value: c, label: c }))} />
																{#if advertErrors.currency}<span class="chip warn" style="margin-left: 4px">{advertErrors.currency}</span>{/if}
															</div>
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px">
																<span class="m-lbl">Menge</span>
																<input class="qnum" type="number" min="1" step="1" bind:value={advertForm.quantity} />
																{#if advertErrors.quantity}<span class="chip warn" style="margin-left: 4px">{advertErrors.quantity}</span>{/if}
																<span class="m-lbl">Einheit</span>
																<SelectCombo bind:value={advertForm.unit} ariaLabel="Einheit" options={unitsFor(advertForm.listing_type).map((u) => ({ value: u, label: u }))} />
																{#if advertErrors.unit}<span class="chip warn" style="margin-left: 4px">{advertErrors.unit}</span>{/if}
																<span class="m-lbl">Ort</span>
																<div style="position: relative; display: inline-flex">
																	<RoutesTypeahead
																		bind:value={advertForm.location}
																		options={advertLocOpts}
																		placeholder="z. B. Port Tressler"
																		ariaLabel="Ort Type-ahead — Locations, keine Shops"
																		onselect={(v: string) => {
																			advertForm.location = v;
																			advertErrors.location = undefined;
																		}}
																	/>
																</div>
																{#if advertErrors.location}<span class="chip warn" style="margin-left: 4px">{advertErrors.location}</span>{/if}
															</div>
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px">
																<!-- M2b-Review-Fix (t_6e26ec82): id_category ist für type=item Pflicht —
																     UEX sendet sonst 400 missing_id_category. Die Auswahl kommt aus
																     /categories (get_categories-IPC), angezeigt mit Section-Präfix. -->
																<span class="m-lbl">Kategorie</span>
																<SelectCombo
																	bind:value={advertForm.category}
																	ariaLabel="Kategorie"
																	placeholder="— bitte wählen —"
																	options={[
																		{ value: '', label: '— bitte wählen —' },
																		...advertCategories.map((c) => ({
																			value: String(c.id),
																			label: (c.section ? c.section + ' › ' : '') + c.name
																		}))
																	]}
																/>
																{#if advertErrors.category}<span class="chip warn" style="margin-left: 4px">{advertErrors.category}</span>{/if}
																{#if advertForm.listing_type === 'item'}
																	<span class="m-lbl">Item</span>
																	<div style="position: relative; display: inline-flex">
																		<RoutesTypeahead
																			bind:value={advertForm.item_label}
																			options={advertItemOpts}
																			placeholder="Item suchen (z. B. Custodian SMG)"
																			ariaLabel="Item Type-ahead — Pflicht für Item-Listings"
																			onselect={(v: string) => {
																				advertForm.item_id = v;
																				advertErrors.item = undefined;
																			}}
																		/>
																	</div>
																	{#if advertForm.item_id}
																		<span class="chip" title="Item gewählt">✓ {advertForm.item_label}</span>
																	{/if}
																	{#if advertErrors.item}<span class="chip warn" style="margin-left: 4px">{advertErrors.item}</span>{/if}
																{/if}
															</div>
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px">
																<span class="m-lbl">Laufzeit</span>
																<SelectCombo
										bind:value={advertForm.hours}
										ariaLabel="Laufzeit"
										options={HOUR_CHOICES.map((h) => ({ value: h, label: hourLabel(h) }))}
									/>
																{#if advertErrors.hours}<span class="chip warn" style="margin-left: 4px">{advertErrors.hours}</span>{/if}
																<span class="m-lbl">Item Source</span>
																<SelectCombo
																	bind:value={advertForm.source}
																	ariaLabel="Item Source"
																	placeholder="— (optional) —"
																	options={[
																		{ value: '', label: '— (optional) —' },
																		...SOURCE_CHOICES.map((s) => ({ value: s, label: s.replace(/_/g, ' ') }))
																	]}
																/>
																<span class="m-lbl">Quality</span>
																<!-- NN (t_060d134e): Quality 0–1000 (war 0–100) — UEX-Maske
												     erlaubt bis 1000, z. B. „970 Q Crafted“. -->
								<input class="qnum" type="number" min="0" max="1000" step="1" placeholder="0–1000" title="Quality 0–1000" bind:value={advertForm.quality} />
																{#if advertErrors.quality}<span class="chip warn" style="margin-left: 4px">{advertErrors.quality}</span>{/if}
																<span class="m-lbl">Availability</span>
																<SelectCombo
																	bind:value={advertForm.availability}
																	ariaLabel="Availability"
																	placeholder="— (optional) —"
																	options={[
																		{ value: '', label: '— (optional) —' },
																		...AVAILABILITY_CHOICES.map((a) => ({ value: a, label: a.replace(/_/g, ' ') }))
																	]}
																/>
															</div>
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px">
																<span class="m-lbl">Hide Listing</span>
																<button
																	type="button"
																	class="chip"
																	class:is-on={advertForm.hidden}
																	onclick={() => (advertForm.hidden = !advertForm.hidden)}
																	title="Verborgenes Listing erscheint erst nach manuellem UEX-Approval"
																>{advertForm.hidden ? 'Ja' : 'No'}</button>
															</div>
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px">
																<span class="m-lbl">Beschreibung</span>
																<input class="rtext" style="flex: 1" placeholder="Freitext (Markdown ok, max. 65.535 Zeichen)" maxlength="65535" bind:value={advertForm.description} />
																{#if advertErrors.description}<span class="chip warn" style="margin-left: 4px">{advertErrors.description}</span>{/if}
															</div>
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px; align-items: center">
																<span class="m-lbl">Bild (JPG/PNG, 1 erlaubt)</span>
																				<!-- NN (t_060d134e): genau 1 Bild — UEX überträgt nur 1
																				     (image_data). Neue Wahl ersetzt die alte; Vorschau +
																				     Entfernen bleiben. -->
																				<input
																					class="rtext"
																					type="file"
																					accept="image/jpeg,image/png"
																					style="max-width: 260px"
																	onchange={(e) => {
																		const el = e.currentTarget as HTMLInputElement;
																		addAdvertImages(el.files);
																		el.value = '';
																	}}
																/>
																{#each advertForm.images as img, i (img + '#' + i)}
																	<span style="position: relative; display: inline-block; margin: 2px" title="Bild {i + 1}">
																		<img src={img} alt="Vorschau {i + 1}" style="width: 56px; height: 56px; object-fit: cover; border-radius: 6px; border: 1px solid rgba(255,255,255,.2)" />
																		<button
																			type="button"
																			class="chip warn"
																			style="position: absolute; top: -8px; right: -8px; padding: 0 6px; line-height: 16px; min-height: 16px"
																			title="Bild entfernen"
																			onclick={() => (advertForm.images = advertForm.images.filter((_, k) => k !== i))}
																		>✕</button>
																	</span>
																{/each}
																{#if advertForm.images.length}
																	<span class="sm" style="opacity: .6">UEX überträgt 1 Bild pro Listing (image_data) — ein neu gewähltes Bild ersetzt das alte.</span>
																{/if}
																{#if advertErrors.images}<span class="chip warn" style="margin-left: 4px">{advertErrors.images}</span>{/if}
															</div>
															<div class="mbar2 advert-foot" style="margin-top: 8px">
																<button type="button" class="btn" disabled={writeBusy} onclick={() => void requestAdvertise()}>{writeBusy ? 'Wird gesendet …' : 'Anlegen (2-Schritt)'}</button>
																<button type="button" class="btn-ghost" disabled={writeBusy} onclick={() => requestCloseAdvert()}>Abbrechen</button>
																{#if advertNote}<span class="chip warn" style="margin-left: 8px">{advertNote}</span>{/if}
															</div>
																												</div>
																											</div>
																											<!-- M3a: Ungespeicherte Eingaben → kurze Verwerfen-Frage
														     statt verlustfreiem Wegwerfen. -->
														{#if advertDiscard}
															<div class="advert-discard">
																<div class="big" style="color: var(--alert)">Eingaben verwerfen?</div>
																<div class="sm" style="color: var(--fgdim)">Es wurden noch keine Daten an UEX gesendet.</div>
																<div class="mbar2" style="margin-top: 10px; justify-content: center; border: 0; background: transparent; padding: 0">
																	<button type="button" class="btn btn-danger" onclick={() => { advertOpen = false; advertDiscard = false; editingOffer = null; advertForm = emptyAdvertise(); }}>Verwerfen</button>
																	<button type="button" class="btn-ghost" onclick={() => (advertDiscard = false)}>Weiter bearbeiten</button>
																</div>
															</div>
														{/if}
																	</div>
																</div>
												{/if}
												{#if shopOffersFiltered.length}
							<div class="mlist">
								{#each shopOffersFiltered as o, i (o.id)}
									<div
										class="mrow"
										role="option"
										aria-selected={false}
										class:is-sel={false}
										data-shop-offer-idx={i}
										title="Listung auf uexcorp.space öffnen (nur Lesen)"
										onclick={() => void openListingDeepLink(o.slug)}
									>
										<div class="m-op {o.operation}">{o.operation === 'sell' ? 'WTS' : 'WTB'}</div>
										<div class="m-main">
											<div class="m-title">{o.title}</div>
											<div class="m-sub">{o.location || '—'}{o.region ? ' · ' + o.region : ''} · {shopStatus(o)}</div>
											{#if !o.approved}
												<span class="chip warn" title="UEX hat dieses Listing noch nicht freigegeben — es erscheint öffentlich erst nach der Moderation.">wartet auf Freigabe</span>
											{/if}
										</div>
																				<div class="m-right">
																					<div class="m-price {o.operation === 'sell' ? 'sell' : 'buy'}">{fmtPrice(o.price)}</div>
																					<div class="m-meta">{fmtQty(o.quantity)} {o.unit || 'unit'} · {rel(o.updated)}</div>
																				</div>
																				<div class="m-right" style="gap: 4px">
																					<button type="button" class="btn-ghost" title="Bearbeiten gibt es nicht: Liste AB und lege neu an (2-Schritt bestätigt beides)" onclick={(ev) => { ev.stopPropagation(); void openAdvertise(o); }}>✎ Neu</button>
																					<button type="button" class="btn-ghost" title="Listing auf uexcorp.space ABMELDEN (2-Schritt-Bestätigung)" onclick={(ev) => { ev.stopPropagation(); requestDelete(o); }}>Abmelden</button>
																				</div>
																			</div>
																		{/each}
							</div>
							<div class="m-foot">
								<span>Klick öffnet die Listung auf uexcorp.space — nur Lesen, keine Änderungen.</span>
								<span class="src">UEX Corp</span>
							</div>
						{:else}
							<div class="row-empty" style="padding: 14px 16px">
								{#if shopFilter !== 'all' && myOffers.length}
									Keine Offers im Filter „{shopFilter === 'pending' ? 'Wartend' : 'Aktiv'}“.
								{:else}
									{shopUser.trim() ? `Keine eigenen Offers für „${shopUser.trim()}“ bei UEX.` : 'Benutzername oben eintragen und Laden drücken.'}
								{/if}
							</div>
						{/if}
						<div class="mbar2" style="margin-top: 14px">
							<span class="m-lbl">Letzte 5 Marktplatz-Trades</span>
							<span class="m-spacer"></span>
							<!-- t_350c4300 B3: alle „Listings verwalten"-Buttons öffnen exakt
							     den Manager-Listings-Tab (Backend open_marketplace_trades);
							     Login macht der Nutzer selbst im Browser. -->
							<span class="chip" title="Eigene Listings im UEX-Manager öffnen (Login im Browser)" onclick={openMyShopDeepLink}>Listings verwalten ↗</span>
						</div>
						{#if recentNegotiations.length}
							<div class="mlist">
								{#each recentNegotiations as n (n.id)}
									<!-- PP t_e2cdb571 (1): eigene trow-Klasse — die Offer-Grid-Spalten
									     (26px Badge + 44px Op) passten nicht zur 3-Kinder-Struktur,
									     was Op-Chip in den Badge-Slot und Text in 44px quetschte. -->
									<div
										class="mrow trow"
										role="option"
										title="Marktplatz-Deal auf uexcorp.space öffnen (Listing-Seite)"
										onclick={() => void openListingDeepLink(n.listing_slug)}
									>
										<div class="m-op {dealRole(n)}">{dealRole(n) === 'sell' ? 'Verkauf' : 'Kauf'}</div>
										<div class="m-main">
											<div class="m-title">{n.listing_title}</div>
											<div class="m-sub">
												{n.is_listing_advertiser
													? `Gegenüber ${n.client_username || n.client_name || '—'} (Käufer)`
													: `Gegenüber ${n.advertiser_username || n.advertiser_name || '—'} (Anbieter)`}
											</div>
										</div>
										<div class="m-right">
											<div class="m-price {dealRole(n)}">{fmtPrice(n.deal_value ?? n.price)} <span class="m-meta">/{n.unit || 'unit'}</span></div>
											<div class="m-meta">{dealStatus(n)}</div>
										</div>
									</div>
								{/each}
							</div>
							<div class="m-foot">
								<span>Zeige {recentNegotiations.length} von {myNegotiations.length} Marktplatz-Trades · Einzeldeal-Deep-Link bei UEX nicht dokumentiert → Listing-Seite im Browser.</span>
								<span class="src">UEX Corp</span>
							</div>
												{:else}
													<div class="row-empty" style="padding: 14px 16px">Noch keine Marktplatz-Trades (Verhandlungen/Abschlüsse) im Verlauf.</div>
												{/if}
											{/if}
											<!-- D3 (t_10be9255): 2-Schritt-Bestätigung als MODALES Popup über der
													     Eingabemaske (dunkler Scrim). Vorher inline im Pane → auf dem
													     Hauptbildschirm aus dem Fenster gelaufen (Buttons nicht erreich-
													     bar). Jetzt: Karte mit Zusammenfassung + Bestätigen/Abbrechen,
													     immer innerhalb der Overlay-Größe (interne Scroll-Region). Während
													     der Bestätigungsphase sind die Eingaben gesperrt (Scrim + Lock). -->
														{#if confirmAction}
															<div class="confirm-scrim" role="presentation">
																<div class="confirm-modal" role="dialog" aria-modal="true" aria-label="Bestätigung">
																	<div class="center" style="padding: 0 0 10px">
																		<div class="big" style="color: var(--alert)">
																			{confirmAction.kind === 'delete' ? 'Listing ABMELDEN?' : 'Listing ANLEGEN?'}
																		</div>
																	</div>
																	{#if confirmAction.kind === 'delete'}
																		<div class="c-row"><b>Titel</b><span>{confirmAction.offer.title}</span></div>
																		<div class="c-row"><b>Preis</b><span>{fmtPrice(confirmAction.offer.price)} {confirmAction.offer.currency}</span></div>
																		<div class="c-row"><b>Menge</b><span>{fmtQty(confirmAction.offer.quantity)} {confirmAction.offer.unit || 'unit'}</span></div>
																		<div class="c-row"><b>Ort</b><span>{confirmAction.offer.location || '—'}</span></div>
																		<div class="sm" style="margin-top: 8px; color: var(--fgdim)">Das Listing wird auf uexcorp.space ABGEMELDET.</div>
																	{:else}
																		<div class="c-row"><b>Titel</b><span>{advertForm.title}</span></div>
																		<div class="c-row"><b>Art</b><span>{advertForm.operation === 'sell' ? 'Verkaufs-Angebot' : 'Kauf-Gesuch'} ({advertForm.listing_type})</span></div>
																		<div class="c-row"><b>Preis</b><span>{fmtGroup(advertForm.price)} {advertForm.currency}</span></div>
																		<div class="c-row"><b>Menge</b><span>{advertForm.quantity} {advertForm.unit}</span></div>
																		<div class="c-row"><b>Ort</b><span>{advertForm.location || '—'}</span></div>
																		<div class="sm" style="margin-top: 8px; color: var(--fgdim)">Das Angebot wird auf uexcorp.space ANGELEGT. Eingaben sind bis zur Entscheidung gesperrt.</div>
																	{/if}
																	<div class="mbar2" style="margin-top: 12px; justify-content: center; border: 0; padding: 0; background: transparent">
																		<button type="button" class="btn" class:btn-danger={confirmAction.kind === 'delete'} disabled={writeBusy} onclick={() => confirmAction?.kind === 'delete' ? void doDelete(confirmAction.offer) : void doAdvertise()}>
																			{confirmAction.kind === 'delete' ? 'JA, ABMELDEN' : 'JA, ANLEGEN'}
																		</button>
																		<button type="button" class="btn-ghost" disabled={writeBusy} onclick={() => (confirmAction = null)}>Abbrechen</button>
																	</div>
																</div>
															</div>
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
							<span class="btn-ghost" onclick={() => loadMarket()}>Erneut versuchen</span>
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

								<!-- K6 P2 (c): live sichtbare Wirkung — die Preisdetail zeigt
								     die Quelle, aus DERen Cache diese Preise kommen (Backend-
								     Provider-Label, identisch zur Statusbar). Nach Quelle-
								     Umschalten + erneutem Öffnen steht hier die neue Quelle.
								     K6 P1: ohne Key und UEX primär → Hinweis (Preise kommen
								     nur aus dem Wiki-Fallback). -->
								<div class="d-src" role="note">
									Quelle: {d.source.toUpperCase()}
									{#if d.source.includes('uex') && cfg && !cfg.uex_api_key_set}
										· <span class="warn">kein UEX-API-Key — UEX anonym geladen; bei Ablehnung Wiki-Fallback</span>
									{/if}
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
						<div class="recent">
						{#each recent.slice(0, 6) as q (q)}
							<span class="r" title="This search" onclick={() => { input = q; void runSearch(); }}>{q}</span>
						{:else}
							<span class="r">Keine — tippe, um zu suchen</span>
						{/each}
					</div>
						<div class="sect"><div class="h">Schnellzugriff</div></div>
							<div class="recent">
								<span class="r"><b>{hotkeyFor(cfg, 'market')}</b> Marktplatz · <b>{hotkeyFor(cfg, 'routes')}</b> Routen</span>
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
					<!-- PB (t_1c9bc240, B2): der F5-Hinweis erscheint nur an Orten mit
					     Resync-Bezug (Angebote, Mein Shop, Routen) — nie global. -->
					{#if offerDetail}
						<span><kbd>Esc</kbd> zurück zur Liste</span>
						<span><kbd>↵</kbd> im Browser öffnen</span>
					{:else if mTab === 'shop'}
						{#if cfg?.uex_secret_key_set}
							<span><kbd>F5</kbd> Mein Shop syncen</span>
						{/if}
						<span><kbd>Esc</kbd> zurück zur Suche</span>
					{:else if mTab === 'routes'}
						<span><kbd>↑↓</kbd> Routen</span>
						<span><kbd>↵</kbd> Detail</span>
						<span><kbd>F5</kbd> Routen-Sweep</span>
						<span><kbd>Esc</kbd> zurück zur Suche</span>
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
					<span><kbd>Esc</kbd> zurück</span>
				{:else}
					<span><kbd>↑↓</kbd> Nav</span>
					<span><kbd>Alt+1..0</kbd> Treffer</span>
					<span><kbd>↵</kbd> Detail</span>
					<span><kbd>Esc</kbd> zurück</span>
					<span><kbd>{hotkeyFor(cfg, 'market')}</kbd> Marktplatz</span>
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
