<script lang="ts">
// A11y-Helfer: Enter/Space aktiviert klickbare Nicht-Button-Elemente (nur bei Eigenfokus,
// keine Kaskade in fokussierte Kind-Buttons). Reine Tastatur-Ergaenzung, kein Verhaltenswechsel.
function keyActivate(e: KeyboardEvent) {
	if (e.target !== e.currentTarget) return;
	if (e.key !== 'Enter' && e.key !== ' ') return;
	e.preventDefault();
	(e.currentTarget as HTMLElement).click();
}
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
	//   - Detail-Tastatur: ↑↓ Zeile, ↵ nur im Commodity-Detail =
//     Trade-Erfassen-Popup vorausgefüllt (Q2), Esc zurück.
	//   - N2 Enter-Detail-Filter: das Suchfeld ist im Detail der In-Context-Filter
	//     (Location → Waren-/Terminalnamen, Commodity/Item → Orts-/Terminalnamen).
	//   - Marketplace: ≥15 Offers pro Seite + "Weitere laden" (Pagination).
	//
	// Zustände: idle · typing · loading · error · offline · market
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { invoke, isTauri } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	// TW (t_f178b084): Info-Popup — Laufzeit-Version aus tauri packageInfo
	// (getVersion()); Fallback __APP_VERSION__ = tauri.conf.json (Build-Zeit).
	import { getVersion } from '@tauri-apps/api/app';
	// Relatives Import (kein $lib-Alias, siehe oben): Sebos Vector-Logo
	// (Kopie von brand/logo/logo.svg) für den Info-Popup-Kopf (UG t_a1f0d83f).
	import logoUrl from '../lib/assets/logo.svg';
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
	import { suggestShips, isSnubShip, SHIP_CATALOG } from '../lib/hangar_ships';
	// T1 (t_530e5f59): i18n — reaktiver t()-Store, Built-ins statisch,
	// lokale (Community-)Dateien via locale_import/locale_list.
	import {  t, importLocaleFromText, initI18n, localeList, setLanguage, trError, tr } from '../lib/i18n';

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
		date_closed_client: number | null;
	};
	type OwnOffersResponse = {
		source: 'cache' | 'live' | 'offline';
		fetched_at: number;
		offers: Offer[];
		note: string | null;
		key_set: boolean;
		// QZ (t_ab10109f): lokale pending-Listings nach dem 1:1-Abgleich.
		pending: PendingOfferView[];
	};
	// QZ: lokale pending-Listings (cache-Tabelle pending_offers) — Mirror von
	// cache::PendingOffer. „Freigabe unbestätigt" leitet die UI aus created_at
	// (> 24 h) ab.
	type PendingOfferView = {
		id: number;
		title: string;
		operation: string;
		price: number;
		currency: string;
		unit: string;
		quantity: number;
		location: string;
		status: string;
		created_at: number;
	};
	type NegotiationsResponse = {
		source: 'cache' | 'live' | 'offline';
		fetched_at: number;
		negotiations: Negotiation[];
		note: string | null;
		key_set: boolean;
	};
	// PT (t_7fdc1317): Tracking-Reiter — Mirror von trade::TradesResponse.
	type TradesResponse = {
		source: 'cache' | 'live' | 'offline';
		fetched_at: number;
		trades: UTrade[];
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

	const OVERLAY_STATE_EVENT = 'overlay-state-changed';
	// Tray tr('common.open_config') → Backend-Event (siehe overlay::TRAY_OPEN_CONFIG_EVENT).
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
	// `dSel` = fokussierte Detail-Zeile (↑↓-Navigation). Q2 (t_56d5264a):
	// keine Enter-Auswahl mehr — dChosen/dToast entfallen.
	let dSel = $state(0);

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
		// UC t_d323be7e: Update-Check-Regler (an/aus + Intervall in Stunden).
		updater_enabled: boolean;
		updater_interval_hours: number;
		// T1 (t_530e5f59): UI-Sprach-Code (normalisiert im Backend).
		language: string;
	};
	let inConfig = $state(false);
	let cfg = $state<SettingsSnapshot | null>(null);
	let apiKeyInput = $state('');
	let apiKeyNote = $state('');
	// M2a-Fix (t_2f197d1c): zweites Credential + Username (F12-View).
	let secretKeyInput = $state('');
	let secretKeyNote = $state('');
	// PW-1 (t_fbe40115): Username-Feld entfallen — Name wird automatisch aus
	// dem Secret-Key ermittelt; usernameNote zeigt nur noch den Status.
	let usernameNote = $state('');
	// S3 (t_988e132f): Modul-Kompatibilität (SC-Wiki) — Status + manueller Sync.
	type ModulStatus = {
		version: string;
		synced_at: number;
		age_seconds: number | null;
		module_count: number;
		ship_count: number;
	};
	let modulStatus = $state<ModulStatus | null>(null);
	let modulBusy = $state(false);
	let modulNote = $state('');
	async function modulLoadStatus() {
		try {
			modulStatus = await invoke<ModulStatus>('modul_compat_status');
		} catch {
			modulStatus = null;
		}
	}
	async function modulRefreshNow() {
		modulBusy = true;
		modulNote = '';
		try {
			const r = await invoke<string>('modul_compat_refresh');
			modulNote =
				r === 'synced'
					? tr('settings.modules_updated_note')
					: tr('settings.update_already_running');
			await modulLoadStatus();
		} catch (e) {
			modulNote = String(e);
		} finally {
			modulBusy = false;
		}
	}
	function modulAgeText(): string {
		const s = modulStatus?.age_seconds;
		if (s == null) return 'Alter unbekannt';
		if (s < 120) return `vor ${s} s`;
		const m = Math.round(s / 60);
		if (m < 120) return `vor ${m} min`;
		const h = Math.round(m / 60);
		if (h < 48) return `vor ${h} h`;
		return `vor ${Math.round(h / 24)} Tagen`;
	}
	// K6 (t_9b385831, P2): kurze Confirm-Zeile im Config-View nach Aktionen
	// (Preisquelle umstellen) — analog K3 Notif-Timeout: verschwindet nach 3 s
	// von selbst, damit die Wirkung sichtbar ist, ohne das View zuzumüllen.
	let cfgToast = $state('');
	let cfgToastTimer: ReturnType<typeof setTimeout> | undefined;
	// PS (t_c84e545d): "Datenquellen"-Sektion — Alter der beiden
	// Preis-Datentypen (getrennt!) + manuelle Sofort-Aktualisierung.
	let priceSyncAge = $state<{ commodity_age_secs: number | null; item_age_secs: number | null; commodity_count: number; item_count: number } | null>(null);
	let priceSyncing = $state(false);

	async function refreshPriceAges() {
		try {
			status = await invoke<SyncStatus>('sync_status');
			priceSyncAge = await invoke<{ commodity_age_secs: number | null; item_age_secs: number | null; commodity_count: number; item_count: number }>('prices_sync_ages');
		} catch {
			/* Status-Refresh ist best-effort */
		}
	}

	async function refreshCommodityPrices() {
		priceSyncing = true;
		try {
			priceSyncAge = await invoke<NonNullable<typeof priceSyncAge>>('refresh_commodity_prices_now');
			cfgConfirm('Commodity-Preise aktualisiert.');
		} catch (e) {
			cfgConfirm(`Commodity-Preise: ${String(e)}`);
		}
		priceSyncing = false;
	}

	async function refreshItemPrices() {
		priceSyncing = true;
		try {
			priceSyncAge = await invoke<NonNullable<typeof priceSyncAge>>('refresh_item_prices_now');
			cfgConfirm('Item-Preise aktualisiert.');
		} catch (e) {
			cfgConfirm(`Item-Preise: ${String(e)}`);
		}
		priceSyncing = false;
	}

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
			scNote = tr('settings.hotkey_needs_mod');
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
		{ id: 'hangar', label: 'nav.hangar' },
		{ id: 'routes', label: 'nav.routes_full' },
		{ id: 'inventory', label: 'nav.inventory' }
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
			cfgConfirm(tr('settings.hotkeys_reset'));
		} catch (err) {
			hkNote = String(err);
		}
	}
	// C2b: Consent-Chip tr('settings.sync_shortcuts_system').
	// Erstanlass persistiert die Zustimmung; erst ab true führt die App die
	// zwei arch-overlay-Binds im OmaSettings-Store (Backup vor erstem
	// Schreibvorgang, Verifikation per hyprctl binds + Lua-Check im Backend).
	async function toggleOmarchySync() {
		const on = !(cfg?.shortcuts_sync_omarchy ?? false);
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_shortcuts_sync', { on });
			applySettings(cfg);
			cfgConfirm(cfg.sync_note || (on
				? tr('settings.compositor_sync_on')
				: 'Compositor-Sync deaktiviert.'));
		scheduleSyncNoteRefresh();
		} catch (err) {
			cfgConfirm(`Compositor-Sync: ${String(err)}`);
		}
	}
	let shortcutFields = $derived([
		{ id: 'toggle' as ShortcutField, label: 'settings.shortcut_toggle', fallback: 'Ctrl+Shift+U', value: cfg?.shortcut_toggle },
		{ id: 'click' as ShortcutField, label: 'settings.shortcut_click', fallback: 'Ctrl+Shift+O', value: cfg?.shortcut_click_through },
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
	// UH (t_424660aa): concept = Shipmatrix 'in-concept' → 'Konzept'-Badge im
	// Name-Typeahead.
	type HangarSuggestion = { name: string; manufacturer: string; kind: string; concept?: boolean };

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
	// TC (t_f269f5b7): Detail ist ein modales Popup (wie die Formular-Popups) —
	// hangarDetail != null = Popup offen; die Liste bleibt dahinter stehen.
	// Das alte hangarView 'list'/'detail' (eigene Detail-Seite) ist entfernt.
	let hangarDetail = $state<HangarItem | null>(null);

	// P9 (Karte t_aa6cc070): DAS HAUPT-Suchfeld (input) serves as In-Context-
	// Filter for the Hangar-List (like Inventory/Detail) — Name/Custom-Name/
	// Manufacturer/Typ. German Typ-words map to entity_type (schiff→ship …);
	// base_ship + ship_code go into the haystack so "Cutlass" finds the Modules.
	// Leerstring = everything (like the other In-Context-Filters).
	const hangarFiltered = $derived.by(() => {
		// TB (t_78a4a5e5, Punkt 2): verlinkte Module sind KEINE Listen-Einträge
		// mehr — sie erscheinen ausschließlich in den Ship-Details ihres
		// Basisschiffs. Bezugslose bleiben sichtbar („Modul (unverbunden)“,
		// R2-Vereinbarung: manuelle Zuordnung bis sie verlinkt sind).
		let items = hangarItems.filter((i) => i.entity_type !== 'module' || !i.base_ship.trim());
		if (hangarFilter === 'pledge') items = items.filter((i) => i.origin === 'pledge');
		else if (hangarFilter === 'ingame') items = items.filter((i) => i.origin === 'ingame');
		else if (hangarFilter === 'lti') items = items.filter((i) => i.insurance === 'LTI');
			// R2 (Punkt 8): Module-Filter zeigt die Schiffe/Fahrzeuge, an denen
			// Module registriert sind — nicht mehr die Modul-Zeilen selbst.
			else if (hangarFilter === 'module') items = items.filter((i) => i.entity_type !== 'module' && hangarModulesOf(i.name).length > 0);
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
		// Review-Fix (R2-D2): Legacy-Zeilen, deren base_ship als Display-Name
		// (custom_name) gespeichert wurde, weiterhin matchen — der Zuordnungs-
		// Typeahead schreibt jetzt den Modellnamen.
		const legacyDn = hangarShips
			.filter((s) => s.name === shipName && s.custom_name)
			.map((s) => s.custom_name as string);
		return hangarItems.filter((i) => i.entity_type === 'module' && (i.base_ship === shipName || legacyDn.includes(i.base_ship ?? '')));
	}

	// R2 (t_2158474c, Punkt 6): Datumsformat dd.mm.jjjj — Anzeige UND Eingabe.
	// Gespeichert bleibt ISO (YYYY-MM-DD, Schema/API-seitig); die Konvertierung
	// passiert ausschließlich in der UI-Schicht (Detail-Anzeige + Formular).
	function isoToDe(v: string): string {
		const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec((v || '').trim());
		return m ? `${m[3]}.${m[2]}.${m[1]}` : v || '';
	}
	function deToIso(v: string): string {
		const s = (v || '').trim();
		const m = /^(\d{1,2})\.(\d{1,2})\.(\d{4})$/.exec(s);
		if (m) {
			const d = Number(m[1]);
			const mo = Number(m[2]);
			const y = Number(m[3]);
			if (mo >= 1 && mo <= 12 && d >= 1 && d <= 31) {
				return `${String(y).padStart(4, '0')}-${String(mo).padStart(2, '0')}-${String(d).padStart(2, '0')}`;
			}
		}
		// Bereits ISO oder Freitext → unverändert speichern (Rückwärtskompatiblität).
		return s;
	}

	// R2 (Punkt 5): Versicherung als Select/Autocomplete statt Freitext.
	const INSURANCE_CHOICES = [
		{ value: '', label: 'common.none' },
		{ value: '3 Monate', label: 'hangar.ins_3m' },
		{ value: '6 Monate', label: 'hangar.ins_6m' },
		{ value: '12 Monate', label: 'hangar.ins_12m' },
		{ value: '24 Monate', label: 'hangar.ins_24m' },
		{ value: '120 Monate', label: 'hangar.ins_120m' },
		{ value: 'LTI', label: 'hangar.ins_lti' }
	];

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

	// R2 (Punkt 1): Löschen läuft über die 2-Schritt-Bestätigung (hangarConfirm).
	function hangarDelete(item: HangarItem) {
		hangarDetail = item;
		hangarConfirm = { kind: 'delete' };
	}
	// Legacy-Direktlöschung entfernt — Write-Pfad jetzt in hangarDoDelete.

	function hangarBlank(): HangarItem {
		return {
			id: null,
			name: '',
			// TB (t_78a4a5e5, Punkt 1): Auswahl-Felder starten LEER (Platzhalter
			// statt Vorauswahl) — die Validierung verlangt vor dem Write eine Wahl.
			origin: '',
			insurance: null,
			custom_name: '',
			entity_type: '',
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
	// TB (t_78a4a5e5, Punkt 3): Hangar-Preise ENTFERNT — kein Preis-Feld mehr
	// im Formular, keine Anzeige; die DB-Spalte bleibt, wird nicht mehr befüllt.
	// R2 (Punkt 6): Datum im Formular als dd.mm.jjjj (Anzeige/Eingabe) —
	// gespeichert wird ISO (Konvertierung in hangarSaveForm).
	let hangarFormDate = $state('');
	// R2 (Punkt 5): Versicherung als Select-String ('' = Ohne → null).
	let hangarFormIns = $state('');
	// R2 (Punkt 1): 2-Schritt-Bestätigung wie elsewhere (save/delete) —
	// modales Popup über dem Formular (confirm-modal im advert-scrim).
	let hangarConfirm = $state<null | { kind: 'save' | 'delete' }>(null);
	// R2 (Punkt 8): Modul-Verwaltung in den Ship-Details — Autocomplete aus
	// den registrierten Modul-Einträgen (Import liefert z. B. „600i
	// Exploration Module“), Freitext bleibt erlaubt.
	let hangarModPick = $state('');
	// TB (t_78a4a5e5, Punkt 2): Modul-Add erfasst zusätzlich Versicherung
	// und Herkunft (gleiche Optionen wie der Hangar-Eintrag). Herkunft ist
	// Pflichtwahl (startet leer), Versicherung '' = Ohne.
	let hangarModIns = $state('');
	let hangarModOrigin = $state('');
	// S3 (t_988e132f): SC-Wiki-Modul-Kompatibilität. Das Autocomplete in den
	// Ship-Details schlägt NUR Module vor, die laut Zuordnung zum Schiff
	// S4 (t_66980b3c): Nur-Datei-Regel — Modul-Feld NUR bei Schiffen mit
	// Eintrag in module-overrides.json; Autocomplete = ausschließlich deren
	// Datei-Module. Kein API-Fallschirm mehr („kein Eintrag → alle Module“
	// entfallen: kein Eintrag → gar kein Feld).
	type ModulHit = { name: string; category: string };
	type ModulForShip = { known: boolean; has_ports: boolean; modules: ModulHit[] };
	let hangarWikiMods = $state<ModulForShip | null>(null);
	async function hangarLoadWikiMods(name: string) {
		hangarWikiMods = null;
		if (!name.trim()) return;
		try {
			hangarWikiMods = await invoke<ModulForShip>('modul_for_ship', { ship: name });
		} catch {
			hangarWikiMods = null; // kein Feld, bis die Zuordnung antwortet
		}
	}
	const hangarModFieldVisible = $derived(
		hangarWikiMods !== null && hangarWikiMods.known && hangarWikiMods.modules.length > 0
	);
	// R2: Zuordnung eines „unverbundenen“ Moduls (Basisschiff-Autocomplete).
	let hangarAssignBase = $state('');
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
		const add = (name: string, manufacturer: string, kind: string, concept?: boolean) => {
			const key = name.toLowerCase();
			if (name && !seen.has(key)) {
				seen.add(key);
				out.push({ name, manufacturer, kind, concept });
			}
		};
		for (const it of hangarItems) {
			if (it.entity_type === 'module') continue;
			const hay = `${it.name} ${it.custom_name}`.toLowerCase();
			if (!q || hay.includes(q) || it.manufacturer.toLowerCase().includes(q)) add(it.name, it.manufacturer, it.entity_type);
		}
		for (const s of suggestShips(q, 12)) add(s.name, s.manufacturer, s.kind, s.concept);
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
		// (Card: tr('hangar.autocomplete_note')).
		// Review-Fix (R2-D2): Wert = MODELLNAME (s.name) — hangarModulesOf
		// matcht base_ship gegen i.name; Display-Namen nur als Suchtext.
		const q = hangarForm.base_ship.trim().toLowerCase();
		const out: string[] = [];
		const seen = new Set<string>();
		for (const s of hangarShips) {
			const dn = hangarDisplayName(s);
			const hay = `${s.name} ${s.custom_name}`.toLowerCase();
			if ((q === '' || hay.includes(q) || s.manufacturer.toLowerCase().includes(q)) && !seen.has(s.name)) {
				seen.add(s.name);
				out.push(s.name);
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

	// R2 (Punkt 2/9): Einheitlicher Typeahead-Mechanismus (RoutesTypeahead,
	// Maus + Tastatur + Overlay-Design) für Name UND Basisschiff — gleiche
	// Kandidatenquelle wie vorher (registrierte Einträge zuerst, dann das
	// statische Ship-Paket mit ALLEN Schiffen UND Fahrzeugen).
	const hangarNameOpts = $derived(
		hangarNameSuggest().map((s) => ({
			value: s.name,
			label: s.manufacturer ? `${s.name} · ${s.manufacturer}` : s.name,
			badge: s.concept ? 'Konzept' : undefined
		}))
	);
	const hangarBaseOpts = $derived(
		hangarBaseOpen
			? hangarBaseSuggest().map((n) => ({ value: n, label: n }))
			: ([] as { value: string; label: string }[])
	);
	// R2 (Punkt 8): Modul-Autocomplete — S3 (t_988e132f): NUR Module, die
	// laut SC-Wiki-Zuordnung zum Schiff passen; ohne Zuordnungseintrag
	// (Fallschirm) der ganze Katalog. Registrierte Modul-Einträge (Bestand)
	// bleiben zusätzlich wählbar. Freitext bleibt erlaubt.
	function hangarModuleSuggest() {
		const q = hangarModPick.trim().toLowerCase();
		const out: string[] = [];
		const seen = new Set<string>();
		const push = (n: string) => {
			if (n && !seen.has(n)) {
				seen.add(n);
				out.push(n);
			}
		};
		const wiki = hangarWikiMods;
		if (wiki && wiki.known) {
			// S4: ausschließlich die Datei-Module des Schiffs.
			for (const m of wiki.modules) {
				if (q === '' || m.name.toLowerCase().includes(q)) {
					push(m.name);
					if (out.length >= 10) break;
				}
			}
		}
		for (const it of hangarItems) {
			if (it.entity_type !== 'module') continue;
			const dn = hangarDisplayName(it);
			if ((q === '' || dn.toLowerCase().includes(q) || it.name.toLowerCase().includes(q)) && !seen.has(dn)) {
				push(dn);
				if (out.length >= 10) break;
			}
		}
		return out;
	}
	// Review-Fix (R2-D2): Der Typeahead-Pick setzt das bind:value auf den
	// LABEL (Display-Name) — die Zuordnung braucht den MODELLNAMEN
	// (hangarModulesOf matcht base_ship gegen i.name).
	function hangarPickAssign(_v: string, label: string) {
		const s = hangarShips.find((x) => x.name === label || hangarDisplayName(x) === label);
		if (s) hangarAssignBase = s.name;
	}
	const hangarModOpts = $derived(hangarModuleSuggest().map((n) => ({ value: n, label: n })));
	function hangarPickName(v: string) {
		// R2: Direkt-Lookup nach exaktem Namen — NICHT über hangarSugs (das
		// Binding hat den Wert beim onselect bereits auf den Label gesetzt,
		// die alte Suggest-Liste passt dann nicht mehr zum Filter).
		const q = v.trim().toLowerCase();
		const reg = hangarItems.find((i) => i.entity_type !== 'module' && i.name.toLowerCase() === q);
		const cat = SHIP_CATALOG.find((s) => s.name.toLowerCase() === q);
		hangarForm = {
			...hangarForm,
			name: v,
			manufacturer: reg?.manufacturer || cat?.manufacturer || hangarForm.manufacturer,
			entity_type: reg ? reg.entity_type : cat ? cat.kind : hangarForm.entity_type
		};
	}

	function hangarOpenAdd() {
		hangarForm = hangarBlank();
		hangarFormDate = '';
		hangarFormIns = '';
		hangarFormOpen = true;
		hangarEdit = false;
		hangarDetail = null;
		hangarWikiMods = null;
	}

	function hangarStartEdit(it: HangarItem) {
		hangarForm = { ...it };
		hangarFormDate = isoToDe(it.purchase_date);
		hangarFormIns = it.insurance ?? '';
		hangarEdit = true;
		// R2 (Punkt 1): Bearbeiten öffnet dasselbe modale Popup (über dem Detail).
		hangarFormOpen = true;
	}

	function hangarCancelEdit() {
		hangarFormOpen = false;
		hangarEdit = false;
		hangarConfirm = null;
	}

	// R2 (Punkt 1): Speichern über die 2-Schritt-Bestätigung (wie
	// Listing-/Trade-Formulare) — erst Zusammenfassung, dann Write.
	function requestHangarSave() {
		if (!hangarForm.name.trim()) {
			hangarNoteSet('Name fehlt');
			return;
		}
		// TB (t_78a4a5e5, Punkt 1): Typ/Herkunft starten leer — Pflichtwahl
		// erst beim Speichern (Meldung statt Vorauswahl im Formular).
		if (!hangarForm.entity_type) {
			hangarNoteSet(tr('hangar.err_choose_type'));
			return;
		}
		if (!hangarForm.origin) {
			hangarNoteSet(tr('hangar.err_choose_origin'));
			return;
		}
		hangarConfirm = { kind: 'save' };
	}

	async function hangarSaveForm() {
		hangarBusy = true;
		hangarNoteSet('');
		try {
			const name = hangarForm.name.trim();
			if (!name) throw new Error('Name fehlt');
			const item: HangarItem = {
				...hangarForm,
				name,
				insurance: hangarFormIns === '' ? null : hangarFormIns,
				purchase_date: deToIso(hangarFormDate),
				// TB (t_78a4a5e5, Punkt 3): Preise werden nicht mehr erfasst —
				// Spalte bleibt in der DB, wird aber nie mehr befüllt.
				purchase_price: null
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
			hangarConfirm = null;
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

	async function hangarDoDelete(item: HangarItem) {
		hangarBusy = true;
		hangarNoteSet('');
		try {
			await invoke('hangar_delete', { id: item.id });
			hangarConfirm = null;
			await loadHangar(true);
			hangarNoteSet(`Gelöscht: ${hangarDisplayName(item)}`);
			// TC (t_f269f5b7): Löschung erfolgte aus dem Detail-Popup — Popup
			// schließt sich, Liste dahinter aktualisiert sich via loadHangar.
			hangarDetail = null;
			hangarWikiMods = null;
		} catch (e) {
			hangarNoteSet(String(e));
		} finally {
			hangarBusy = false;
		}
	}

	// R2 (Punkt 8): Modul in den Ship-Details hinzufügen (entity_type=module,
	// base_ship=Schiff) — der einzige Weg, Module anzulegen. TB
	// (t_78a4a5e5, Punkt 2): mit Versicherung + Herkunft aus den Selects.
	async function hangarModuleAdd() {
		const ship = hangarDetail;
		const modName = hangarModPick.trim();
		if (!ship || !modName) return;
		if (!hangarModOrigin) {
			hangarNoteSet(tr('hangar.err_choose_module_origin'));
			return;
		}
		hangarBusy = true;
		hangarNoteSet('');
		try {
			await invoke<HangarItem>('hangar_add', {
				item: {
					...hangarBlank(),
					name: modName,
					entity_type: 'module',
					base_ship: ship.name,
					origin: hangarModOrigin,
					insurance: hangarModIns === '' ? null : hangarModIns
				}
			});
			hangarModPick = '';
			hangarModIns = '';
			hangarModOrigin = '';
			await loadHangar(true);
			const fresh = hangarItems.find((i) => i.id === ship.id);
			if (fresh) hangarDetail = fresh;
			hangarNoteSet(`Modul hinzugefügt: ${modName}`);
		} catch (e) {
			hangarNoteSet(String(e));
		} finally {
			hangarBusy = false;
		}
	}

	async function hangarModuleRemove(m: HangarItem) {
		if (m.id == null) return;
		hangarBusy = true;
		hangarNoteSet('');
		try {
			await invoke('hangar_delete', { id: m.id });
			await loadHangar(true);
			if (hangarDetail?.id != null) {
				const fresh = hangarItems.find((i) => i.id === hangarDetail!.id);
				if (fresh) hangarDetail = fresh;
			}
			hangarNoteSet(`Modul entfernt: ${hangarDisplayName(m)}`);
		} catch (e) {
			hangarNoteSet(String(e));
		} finally {
			hangarBusy = false;
		}
	}

	// R2: „Modul (unverbunden)“ einem Schiff manuell zuordnen (Basisschiff setzen).
	async function hangarModuleAssign(m: HangarItem) {
		const base = hangarAssignBase.trim();
		if (m.id == null || !base) return;
		hangarBusy = true;
		hangarNoteSet('');
		try {
			await invoke<HangarItem>('hangar_update', { item: { ...m, base_ship: base } });
			hangarAssignBase = '';
			await loadHangar(true);
			// TC (t_f269f5b7): Zuordnung im Detail-Popup — Popup schließt,
			// das zugeordnete Modul verschwindet aus der Liste (verlinkt).
			hangarDetail = null;
			hangarWikiMods = null;
			hangarNoteSet(`Zugeordnet: ${hangarDisplayName(m)} → ${base}`);
		} catch (e) {
			hangarNoteSet(String(e));
		} finally {
			hangarBusy = false;
		}
	}

	function hangarOpenDetail(item: HangarItem) {
		// TC (t_f269f5b7): öffnet das Detail-POPUP (kein View-Wechsel mehr) —
		// die Liste bleibt dahinter unverändert stehen.
		hangarDetail = item;
		// S4: Zuweisung für dieses Schiff laden (nur Datei — ohne Eintrag
		// bleibt hangarWikiMods null → kein Modul-Feld).
		if (item.entity_type !== 'module') {
			void hangarLoadWikiMods(item.name);
		} else {
			hangarWikiMods = null;
		}
	}

	// TC (t_f269f5b7): Popup schließen (Esc/Close) — zurück zur Liste ohne
	// View-Wechsel; Modul-Zuweisung der letzten Ansicht verwerfen.
	function hangarCloseDetail() {
		hangarDetail = null;
		hangarWikiMods = null;
	}

	function hangarBack() {
		if (hangarConfirm) {
			// Esc in der Bestätigung schließt nur den Bestätigungs-Dialog.
			hangarConfirm = null;
			return;
		}
		if (hangarFormOpen || hangarEdit) {
			// Formular zuerst schließen, dann View-Ebene.
			hangarFormOpen = false;
			hangarEdit = false;
			return;
		}
		if (hangarDetail) {
			// TC (t_f269f5b7): Esc im Detail-Popup schließt das Popup —
			// eine Ebene zurück zur Liste (Escape-Back-Kette konsistent).
			hangarCloseDetail();
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
			// PW-1: Username-Feld entfallen — auto-ermittelt, gecacht. Ohne
			// gecachten Namen einmalig nachziehen (best-effort, Hintergrund).
			if (cfg?.uex_secret_key_set && !cfg?.uex_username?.trim()) void resolveUsername(true);
		} catch {
			// Ohne Tauri (SSR/vite-dev) → Palette bleibt am app.html-Default.
		}
		// T1 (t_530e5f59): i18n initialisieren — persistierte Sprache + lokale
		// Sprachdateien laden. Reihenfolge bewusst NACH applySettings: der
		// erste Paint nutzt schon die richtige Sprache (kein Flacker-Switch).
		await initI18n(cfg?.language);
	}

	// UC t_d323be7e: Auto-Updater — Status-Chip + on-demand Update aus der
	// Settings. Der periodische Check läuft im Backend (Start + alle N
	// Stunden); die UI zeigt nur an und installiert NICHTS ohne Klick auf
	// „Jetzt aktualisieren“ (Umsetzung 2: on-demand only).
	type UpdaterStatus = {
		enabled: boolean;
		interval_hours: number;
		app_version: string;
		note: string;
		checking: boolean;
		available: { version: string; kind: string } | null;
	};
	let upStatus = $state<UpdaterStatus | null>(null);
	let upBusy = $state(false);

	async function loadUpdaterStatus() {
		try {
			upStatus = await invoke<UpdaterStatus>('updater_status');
		} catch { /* Ohne Backend (SSR/dev) — kein Chip. */ }
	}

	async function updaterCheckNow() {
		upBusy = true;
		try {
			upStatus = await invoke<UpdaterStatus>('updater_check_now');
		} catch (e) {
			upStatus = upStatus ? { ...upStatus, note: String(e) } : upStatus;
		}
		upBusy = false;
	}

	async function updaterInstallNow() {
		if (upStatus?.available?.kind === 'deb') return; // kein Self-Update — nur Hint
		upBusy = true;
		try {
			// Bei nsis/appimage beendet sich das Overlay selbst nach dem
			// verifizierten Download; linux-bin ersetzt nur die exe und zeigt
			// den Neustart-Hinweis — die Antwort ist Best-Effort-Status.
			upStatus = await invoke<UpdaterStatus>('updater_install');
		} catch (e) {
			upStatus = upStatus ? { ...upStatus, note: String(e) } : upStatus;
		}
		upBusy = false;
	}

	async function setUpdaterEnabled(on: boolean) {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_updater_enabled', { on });
			applySettings(cfg);
			void loadUpdaterStatus();
		} catch { /* ohne Backend nichts tun */ }
	}

	async function setUpdaterInterval(h: number) {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_updater_interval', { hours: h });
			applySettings(cfg);
			void loadUpdaterStatus();
		} catch { /* ohne Backend nichts tun */ }
	}

	function openConfig() {
		// Kein Enter-Filter-Zustand darf bestehen, während der Config-View offen
		// ist: Detail + In-Context-Filter + Auswahl werden vorab geräumt.
		inDetail = false;
		detailHit = null;
		expanded = null;
		focusIdx = null;
		// S3 (t_988e132f): Modul-Kompatibilitäts-Status beim Öffnen frisch lesen.
		void modulLoadStatus();
		xpan = null;
		xpanToken++;
		dSel = 0;
		apiKeyInput = '';
		apiKeyNote = '';
		secretKeyInput = '';
		secretKeyNote = '';
		usernameNote = '';
		// UC t_d323be7e: Update-Status beim Öffnen frisch lesen.
		void loadUpdaterStatus();
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

	async function chooseVariant(name: string) {
		try {
			cfg = await invoke<SettingsSnapshot>('settings_set_detail_variant', { name });
			applySettings(cfg);
		} catch (e) {
			apiKeyNote = String(e);
		}
	}

	// ---- T1 (t_530e5f59): Sprache — Auswahl, Live-Switch, Import ------------
	// Live-Switch: setLanguagePersisted() ruft settings_set_language (persistiert
	// sofort) und schaltet dann den Store — alle $t()-Auftretende ziehen OHNE
	// Neustart nach (Settings-UI inklusive: die Section rendert selbst über $t).
	// Chip-Spiegel cfg.language: invoke zuerst, Snapshot-Spiegel danach — wie
	// chooseScale (nie einen Zustand zeigen, den das Backend nicht bestätigt).
	type LocaleNote = { key: string; params?: Record<string, string | number>; raw?: string };
	let localeNote = $state<LocaleNote | null>(null);
	let localeFileInput: HTMLInputElement | null = $state(null);

	async function chooseLanguage(code: string) {
		localeNote = null;
		try {
			// invoke zuerst (Backend persistiert), Store danach — genau wie
			// chooseScale/choosePalette: nie einen Zustand zeigen, den das
			// Backend nicht bestätigt hat.
			cfg = await invoke<SettingsSnapshot>('settings_set_language', { code });
			setLanguage(code);
		} catch (e) {
			localeNote = { key: '', raw: trError(e) };
		}
	}

	async function onLocaleFilePick(e: Event) {
		const input = e.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		// Gleiche Datei darf erneut gewählt werden → value immer zurücksetzen.
		input.value = '';
		if (!file) return;
		localeNote = null;
		try {
			const text = await file.text();
			const meta = await importLocaleFromText(text);
			localeNote = { key: 'settings.language.imported', params: { name: meta.name } };
		} catch (e) {
			localeNote = { key: '', raw: trError(e) };
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
			apiKeyNote = tr('settings.apikey_saved');
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
			secretKeyNote = tr('settings.secret_saved');
			// PW-1: Username wird jetzt automatisch aus dem Secret-Key ermittelt.
			usernameNote = '';
			void resolveUsername();
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

	// PW-1 (t_fbe40115): Username auto aus dem Secret-Key (GET /user, Backend
	// cachet den Namen). Kein Frontend-Input mehr; bei 403/ungültigem Key
	// kommt die deutsche Meldung aus `friendly()` durch.
	// PW-1: UEX-Verwalten-Links (nur Deep-Link in den Systembrowser).
	async function openUex(cmd: 'open_uex_api_apps' | 'open_uex_account') {
		try {
			await invoke(cmd);
		} catch (e) {
			secretKeyNote = String(e);
		}
	}

	async function resolveUsername(bg = false) {
		if (typeof invoke !== 'function') return;
		try {
			cfg = await invoke<SettingsSnapshot>('settings_resolve_uex_username');
			if (!bg) {
				usernameNote = cfg?.uex_username?.trim()
					? `UEX-Username automatisch ermittelt: ${cfg.uex_username.trim()}`
					: '';
			}
		} catch (e) {
			usernameNote = `Username konnte nicht ermittelt werden: ${String(e)}`;
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
				? tr('settings.confirm_mode_every')
				: tr('settings.confirm_mode_unlist');
		} catch (e) {
			mktConfirmNote = String(e);
		}
	}

	// Scroll-Position der Ergebnisliste für den Zurück-Blick.
	let listScroll = $state(0);
	// Status-Toast nach Auswahl einer Detail-Zeile.

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
	// MQ (t_d6ff5212): Bild-Lightbox — Inline-Zoom im Overlay. Klick auf ein
	// Offer-Foto öffnet das Modal statt den Browser-Deep-Link; Scrim-Klick
	// oder Esc schließt. Die externen Links bleiben den Buttons vorbehalten.
	let lightbox = $state<{ uri: string; title: string } | null>(null);
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
	// PW-1 (t_fbe40115): der Username lebt KANONISCH in den Settings (auto
	// aus dem Secret-Key) — das Mein-Shop-Feld ist entfallen, shopUser ist
	// nur noch eine Projektion des gecachten Namens.
	let shopUser = $derived(cfg?.uex_username?.trim() ?? '');
	let myOffers = $state<Offer[]>([]);
	let myNegotiations = $state<Negotiation[]>([]);
	// QZ (t_ab10109f): lokale pending-Listings (cache pending_offers) — im
	// Mein Shop zusätzlich zu den API-Listings sichtbar.
	let shopPending = $state<PendingOfferView[]>([]);
	let shopLoading = $state(false);
	let shopSource = $state<'cache' | 'live' | 'offline'>('live');
	let shopNote = $state<string | null>(null);
	let shopKeySet = $state(true);
	let shopErr = $state<string | null>(null);
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
			shopPending = [];
			return;
		}
		// PW-1: ohne ermittelten Username (Secret-Key frisch gesetzt, /user
		// noch nicht gelaufen) kein Offers-Pull — einmalig auflösen.
		if (cfg && !shopUser.trim()) {
			shopLoading = true;
			await resolveUsername(true);
			shopLoading = false;
			if (!shopUser.trim()) {
				shopNote = tr('settings.username_missing');
				myOffers = [];
				return;
			}
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
			// QZ (t_ab10109f): pending = lokale Einträge NACH dem 1:1-Abgleich
			// (freigegebene sind im Backend bereits entfernt).
			shopPending = r.pending ?? [];
			shopSource = r.source;
			shopKeySet = r.key_set;
			if (r.note) shopNote = r.note;
			// QW-3 (t_28bfd027): UEX 2.0 liefert NUR freigegebene Listings —
			// Doku „List all active marketplace advertisements"; live belegt
			// (2026-10-29): GET /marketplace_listings?username=AAWA_SeBoOne →
			// 0 Rows, obwohl ein Listing auf Freigabe wartet; auch der globale
			// Feed (1000 Rows) enthält es nicht. Wartende Listings sind via
			// API 2.0 nicht abrufbar — statt eines stummen leeren Abschnitts
			// erklärt der Hinweis das und bietet den Direkt-Link an (der
			// shopCreatedUrl-Deep-Link aus der Create-Aktion zeigt es sofort).
			// API 2.0 nicht abrufbar — QZ (t_ab10109f): das Overlay speichert
			// frisch angelegte Listings lokal (pending_offers) und zeigt sie
			// bis zur Freigabe als „wartet auf Freigabe"; der 1:1-Abgleich
			// entfernt den Eintrag, sobald UEX das Listing listet.
			if (!myOffers.length && !shopPending.length && !shopNote) {
				shopNote =
					tr('shop.no_active_listings');
			}
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
	// QY (t_07edb03d): Deal beendet = EINE Seite hat abgeschlossen (beidseitig
	// date_closed+date_closed_client, einseitig genau eins der beiden ≠ 0).
	const tradesDealEnded = $derived(
		!!tradesDeal && (!!tradesDeal.date_closed || !!tradesDeal.date_closed_client)
	);
	let chatMsgs = $state<NegotiationMessage[]>([]);
	let chatLoading = $state(false);
	let chatErr = $state<string | null>(null);
	let chatDraft = $state('');
	let chatBusy = $state(false);
	let chatScroller = $state<HTMLElement | null>(null);
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
			tradesNote = tr('trades.need_secret_note');
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

	// PT (t_7fdc1317): Tracking-Reiter in den Handelsrouten — eigene
	// Commodity-Trades (/user_trades) + Tages-Aggregat (Ausgaben/Einnahmen/
	// Nettogewinn in aUEC) + Erfassen/Bearbeiten/Löschen via user_trades_add/
	// edit/remove (2-Schritt-Bestätigung wie die Marketplace-Writes).
	// WICHTIG: das ist der MARKTPLATZ-agnostische Cargo-Verlauf — bewusst
	// NICHT die Negotiations-Quelle des Trades-Reiters (M3b).
	type UTrade = {
		id: number;
		commodity: string;
		commodity_id: number;
		terminal: string;
		terminal_id: number;
		operation: string;
		scu: number;
		price: number;
		date_added: number;
	};
	type TrkDay = { key: string; iso: string; expense: number; income: number; net: number; count: number };
	type TrkSug = { id: number; name: string; subtitle: string | null };

	let rTab = $state<'matrix' | 'tracking'>('matrix');
	let myTrades = $state<UTrade[]>([]);
	let trkLoading = $state(false);
	let trkErr = $state<string | null>(null);
	let trkNote = $state<string | null>(null);
	let trkLoaded = $state(false);
	let trkOpen = $state(false); // Formular „Trade erfassen“
	let trkEditId = $state<number | null>(null); // null = neu
	let trkBusy = $state(false);
	let trkMsg = $state<string | null>(null);
	// 2-Schritt-Bestätigung (Muster confirmAction, aber lokal im Reiter — der
	// bestehende Dialog hängt im Angebots-Markup und rendert hier nicht).
	let trkConfirm = $state<null | { kind: 'add' | 'edit' | 'remove'; id: number | null }>(null);
	let trkForm = $state({
		commodity: '',
		commodity_id: null as number | null,
		terminal: '',
		terminal_id: null as number | null,
		operation: 'buy' as 'buy' | 'sell',
		scu: '',
		price: ''
	});
	let trkComOpts = $state<{ label: string; value: string }[]>([]);
	let trkTermOpts = $state<{ label: string; value: string }[]>([]);

	// Typeahead-Wiring (Muster rFromSug): Tippen im Formularfeld → Backend-
	// Suggest (commodity_suggest/terminal_suggest) → Optionen für den
	// RoutesTypeahead. Fehler still wegdrücken (Typeahead ist optional).
	$effect(() => {
		// QY-3: Abhängigkeiten Richtung + gewählter Ort einbeziehen —
		// Richtungswechsel oder Ort-Auswahl aktualisiert die Waren-Liste.
		const q = trkForm.commodity.trim();
		const op = trkForm.operation;
		const tid = trkForm.terminal_id;
		void (async () => {
			await trkSuggest('com', q, tid, op);
		})();
	});
	$effect(() => {
		const q = trkForm.terminal.trim();
		const op = trkForm.operation;
		const cid = trkForm.commodity_id;
		void (async () => {
			await trkSuggest('term', q, cid, op);
		})();
	});

	async function loadTrk(bg = false, force = false) {
		if (cfg && !cfg.uex_secret_key_set) {
			trkLoading = false;
			trkErr = null;
			trkNote =
				tr('trades.trade_log_need_secret');
			myTrades = [];
			trkLoaded = true;
			return;
		}
		trkLoading = !bg;
		trkErr = null;
		trkNote = null;
		try {
			// RA (t_1bd5328b): force reicht bis zum uex-client durch —
			// „Aktualisieren" umgeht die Snapshot-TTL und liest live bei UEX.
			const r = await invoke<TradesResponse>('get_my_trades', { force });
			myTrades = r.trades as unknown as UTrade[];
			if (r.note) trkNote = r.note;
		} catch (e) {
			trkErr = String(e);
		}
		trkLoading = false;
		trkLoaded = true;
	}

	function openTrkForm(t?: UTrade) {
		if (t) {
			trkEditId = t.id;
			trkForm = {
				commodity: t.commodity,
				commodity_id: t.commodity_id,
				terminal: t.terminal,
				terminal_id: t.terminal_id,
				operation: t.operation === 'sell' ? 'sell' : 'buy',
				scu: String(t.scu),
				price: String(t.price)
			};
		} else {
			trkEditId = null;
			trkForm = {
				commodity: '',
				commodity_id: null,
				terminal: '',
				terminal_id: null,
				operation: 'buy',
				scu: '',
				price: ''
			};
		}
		trkComOpts = [];
		trkTermOpts = [];
		trkMsg = null;
		trkOpen = true;
	}

	// Q2 (t_56d5264a): Enter auf einer Commodity-Preiszeile öffnet das
	// Trade-Erfassen-Popup VORAUSGEFÜLLT — Richtung nach der Zeile (Kauf-Zeile
	// = Ort verkauft = Spieler kauft dort → 'buy'; Verkauf-Zeile = Ort kauft →
	// 'sell'), Ware/Preis/Ort(Terminal) aus der Zeile. Der Nutzer tippt nur
	// noch die Menge (SCU); Enter bestätigt, Escape bricht ab.
	function openTrkPrefill(op: 'buy' | 'sell', price: number, p: PricePoint, commodity: string) {
		trkEditId = null;
		trkForm = {
			commodity,
			commodity_id: p.commodity_id,
			terminal: p.location,
			terminal_id: p.terminal_id,
			operation: op,
			scu: '',
			price: String(price)
		};
		trkComOpts = [];
		trkTermOpts = [];
		trkMsg = null;
		trkOpen = true;
	}

	// Doku §5a/§5c: die API kennt KEIN Datumsfeld — der Trade bekommt das
	// Aufnahme-Datum von UEX (date_added). Kein Fake-Datumsfeld im Formular.
	async function trkSuggest(
		kind: 'com' | 'term',
		query: string,
		crossId: number | null,
		op: 'buy' | 'sell'
	) {
		// QY-3 (t_07edb03d): Cross-Autocomplete — Richtungskorrekt über die
		// terminal_prices-Stammdaten (can_buy/can_sell aus Spielersicht):
		// Richtung=Kauf → Ort-Autocomplete bietet nur Orte, die die gewählte
		// Ware VERKAUFEN (Spieler kann dort kaufen); Richtung=Verkauf → Orte,
		// die sie KAUFEN. Umgekehrt: Ort gewählt → nur dort handelbare Waren
		// für die Richtung. Ohne gecachte Stammdaten fällt das Backend auf
		// das ungefilterte Suggest zurück (Auswahl bleibt bedienbar).
		try {
			const r =
				kind === 'com'
					? await invoke<TrkSug[]>('trade_commodity_suggest', {
							query,
							id_terminal: crossId ?? 0,
							operation: op
						})
					: await invoke<TrkSug[]>('trade_location_suggest', {
							query,
							id_commodity: crossId ?? 0,
							operation: op
						});
			const opts = r.map((s) => ({ label: s.name, value: String(s.id) }));
			if (kind === 'com') trkComOpts = opts;
			else trkTermOpts = opts;
		} catch {
			// Typeahead ist optional — leere Liste statt Fehler-Wall.
			if (kind === 'com') trkComOpts = [];
			else trkTermOpts = [];
		}
	}

	function trkValidate(): string | null {
		if (!trkForm.commodity_id) return tr('trades.choose_commodity');
		if (!trkForm.terminal_id) return tr('trades.choose_location');
		const scu = Math.round(Number(trkForm.scu));
		if (!Number.isFinite(scu) || scu <= 0) return tr('trades.err_qty_positive');
		const price = Number(trkForm.price);
		if (!Number.isFinite(price) || price <= 0) return tr('trades.err_price_positive');
		return null;
	}

	function requestTrkWrite() {
		if (trkBusy) return;
		const err = trkValidate();
		if (err) {
			trkMsg = err;
			return;
		}
		trkMsg = null;
		trkConfirm = { kind: trkEditId ? 'edit' : 'add', id: trkEditId };
	}

	function requestTrkRemove(t: UTrade) {
		trkConfirm = { kind: 'remove', id: t.id };
	}

	async function doTrkWrite() {
		if (trkBusy || !trkConfirm) return;
		trkConfirm = null;
		trkBusy = true;
		trkMsg = null;
		const wasEdit = !!trkEditId;
		const payload = {
			id_terminal: trkForm.terminal_id!,
			id_commodity: trkForm.commodity_id!,
			operation: trkForm.operation,
			scu: Math.round(Number(trkForm.scu)),
			price: Number(trkForm.price)
		};
		try {
			if (trkEditId) await invoke('trade_edit', { id: trkEditId, payload });
			else await invoke('trade_add', { payload });
			trkOpen = false;
			trkEditId = null;
			trkMsg = wasEdit ? 'Trade aktualisiert.' : 'Trade erfasst.';
			await loadTrk(true, true);
		} catch (e) {
			// Kein Retry (M2b-Philosophie): Meldung + Deep-Link-Button.
			trkMsg = String(e);
		}
		trkBusy = false;
	}

	async function doTrkRemove() {
		if (trkBusy || !trkConfirm) return;
		const id = trkConfirm.id;
		trkConfirm = null;
		if (!id) return;
		trkBusy = true;
		trkMsg = null;
		try {
			await invoke('trade_remove', { id });
			trkMsg = tr('trades.deleted_note');
			await loadTrk(true, true);
		} catch (e) {
			trkMsg = String(e);
		}
		trkBusy = false;
	}

	async function openTrkUex() {
		// QY-4 (t_07edb03d): Haupt-Button zeigt auf den UEX Trade-Log
		// (/trade/home), nicht auf den Account.
		try {
			await invoke('open_uex_trade_log');
		} catch (e) {
			trkMsg = String(e);
		}
	}

	// Tages-Aggregat: Kauf = Ausgabe (scu × price), Verkauf = Einnahme,
	// Nettogewinn = Einnahme − Ausgabe. Clientseitig gruppiert nach Tag.
	const trkDays = $derived.by(() => {
		const map = new Map<string, TrkDay>();
		for (const t of myTrades) {
			const d = new Date((t.date_added || 0) * 1000);
			const y = d.getFullYear(), m = String(d.getMonth() + 1).padStart(2, '0'), dd = String(d.getDate()).padStart(2, '0');
			const iso = `${y}-${m}-${dd}`; // YYYY-MM-DD (lokal) — chronologisch sortierbar, Deckung mit de-DE-Anzeige
			const key = d.toLocaleDateString('de-DE');
			let day = map.get(iso);
			if (!day) {
				day = { key, iso, expense: 0, income: 0, net: 0, count: 0 };
				map.set(iso, day);
			}
			const vol = (t.scu || 0) * (t.price || 0);
			if (t.operation === 'sell') day.income += vol;
			else day.expense += vol;
			day.net = day.income - day.expense;
			day.count += 1;
		}
		return [...map.values()].sort((a, b) => (b.iso > a.iso ? 1 : -1));
	});
	const trkSorted = $derived([...myTrades].sort((a, b) => (b.date_added || 0) - (a.date_added || 0)));

	async function sendChat() {
		if (chatBusy || !tradesDeal) return;
		// QW-4 (t_28bfd027) / QY (t_07edb03d): beendeter Deal (beidseitig
		// ODER einseitig — date_closed XOR date_closed_client) → kein Senden
		// (UI-Gate; das Backend lehnt den Write zusätzlich ab).
		if (tradesDeal.date_closed || tradesDeal.date_closed_client) {
			chatErr = tr('trades.readonly_note');
			return;
		}
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

	// Q1 (t_535a98fb): „Deal auf UEX öffnen" — exakte Deal-URL
	// /marketplace/negotiate/hash/<hash> (hash aus dem Negotiation-Datensatz),
	// NICHT mehr das Listing.
	async function openTradesDeal(n: Negotiation) {
		try {
			await invoke<string>('open_negotiation_deal', { hash: n.hash });
		} catch (e) {
			tradesErr = String(e);
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
		if (!o.approved) return tr('shop.status_pending');
		if (!o.expires) return tr('shop.status_active');
		if (o.expires <= Math.floor(Date.now() / 1000)) return tr('shop.status_expired');
		return tr('shop.status_expires_on', { date: new Date(o.expires * 1000).toLocaleDateString('de-DE') });
	}
	// QZ (t_ab10109f): Filter „Aktiv/Wartend" entfernt — UEX kann wartende
	// Listings nicht per API liefern; Wartende kommen jetzt lokal (shopPending)
	// und sind immer sichtbar, ein Filter darüber ist sinnlos.
	// QZ (t_ab10109f): „Letzte 5 Trades“-Liste aus Mein Shop entfernt — der
	// Trade-Log-Reiter (PT) ist die einzige Stelle dafür. dealStatus/dealRole
	// bleiben dort (Deals-Liste) in Gebrauch.
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
	// QZ (t_ab10109f): Trades-Reiter → „Trades verwalten“ öffnet den Manager-
	// HOME-Tab (dort sitzen die Trade-Verwaltungsaktionen) — NICHT listings.
	async function openMyTradesDeepLink() {
		try {
			await invoke<string>('open_marketplace_trades_home');
		} catch (e) {
			shopErr = String(e);
		}
	}
	// QZ (t_ab10109f): lokalen pending-Eintrag entfernen (Nutzer-Aktion).
	async function removePendingOffer(id: number) {
		try {
			await invoke('pending_offers_remove', { id });
			shopPending = shopPending.filter((p) => p.id !== id);
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
	// TB (t_78a4a5e5, Punkt 1): ALLE Auswahl-Felder starten LEER (Platzhalter
	// statt Vorauswahl) — die komplette Optionsliste ist sofort sichtbar;
	// nur die Validierung verlangt die Wahl (Pflichtfelder). Nur `operation`
	// (Chips, beide Optionen immer sichtbar) und `quantity` (Zahl-Eingabe,
	// sinnvoller Startwert) behalten ihren Default.
	const emptyAdvertise = (): AdvertiseForm => ({
		operation: 'sell',
		listing_type: '',
		item_id: '',
		item_label: '',
		title: '',
		description: '',
		price: '',
		quantity: '1',
		unit: '',
		location: '',
		source: '',
		currency: '',
		quality: '',
		hidden: false,
		availability: '',
		images: [],
		hours: '',
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
			advertNote = tr('shop.err_image_type');
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
		if (!f.listing_type) errs.listing_type = tr('shop.err_listing_type');
		if (!f.title.trim()) errs.title = tr('shop.err_title_required');
		else if (f.title.trim().length > 140) errs.title = tr('shop.err_title_max');
		if (!f.description.trim()) errs.description = tr('shop.err_description_required');
		else if (f.description.trim().length > 65535) errs.description = tr('shop.err_description_max');
		const catId = Number(f.category);
		if (!f.category || !Number.isFinite(catId) || catId <= 0) {
			errs.category = tr('shop.err_category');
		}
		// Item ist PFLICHT nur bei type=item (Task-Regel „Pflicht wenn Type=Item").
		if (f.listing_type === 'item' && !f.item_id) {
			errs.item = tr('shop.err_item');
		}
		if (!f.location.trim()) errs.location = tr('shop.err_location');
		const price = Math.round(Number(f.price));
		if (!Number.isFinite(price) || price <= 0) errs.price = tr('shop.err_price');
		const qty = Math.round(Number(f.quantity));
		if (!Number.isFinite(qty) || qty <= 0) errs.quantity = tr('shop.err_qty');
		if (!f.unit.trim()) errs.unit = tr('shop.err_unit');
		if (!f.currency.trim()) errs.currency = tr('shop.err_currency');
		// NN (t_060d134e): Quality-Bereich 0–1000 (UEX-Maske; 100 war zu eng).
		if (f.quality !== '' && (!Number.isFinite(Number(f.quality)) || Number(f.quality) < 0 || Number(f.quality) > 1000)) {
			errs.quality = tr('shop.err_quality');
		}
		if (!f.hours) errs.hours = tr('shop.err_duration');
		// NN (t_060d134e): UEX überträgt nur 1 Bild (image_data) — UI erlaubt
		// genau 1 (neue Wahl ersetzt die alte), keine 5er-Sammlung mehr.
		if (f.images.length > 1) errs.images = tr('shop.err_image_max');
		return errs;
	}
	function applyAdvertErrors(errs: AdvertErrors): boolean {
		advertErrors = errs;
		if (Object.keys(errs).length > 0) {
			advertNote = tr('shop.err_fix_fields');
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
			shopNote =
				tr(f.operation === 'sell' ? 'shop.created_note' : 'shop.request_created_note', {
					title: f.title.trim()
				}) + ' ' + tr('shop.appears_after_approval');
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
	// TB (t_78a4a5e5, Punkt 1): ohne gewählten Type KEINE Unit-Liste —
	// das Feld startet leer und verlangt erst die Type-Wahl.
	function unitsFor(type: AdvertiseForm['listing_type']): string[] {
		if (type !== 'item' && type !== 'service' && type !== 'contract') return [];
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
		/** Preisstand (UEX date_modified, Unix-Sek). 0 = unbekannt. */
		updated: number;
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
	// R2 (t_2158474c): Basisschiff-Optionen für die Zuordnung eines
	// unverbundenen Moduls (Detail-„Zuordnen“-Typeahead).
	const hangarAssignOpts = $derived(
		hangarShips
			.filter((s) => {
				const q = hangarAssignBase.trim().toLowerCase();
				const hay = `${s.name} ${s.custom_name}`.toLowerCase();
				return q === '' || hay.includes(q) || s.manufacturer.toLowerCase().includes(q);
			})
			.slice(0, 10)
			// Review-Fix (R2-D2): value = MODELLNAME (s.name), nicht Display-Name —
			// hangarModulesOf matcht base_ship gegen i.name; custom_name-Schiffe
			// (z. B. „Riftwalker“) würden das Modul sonst verlieren. Label bleibt
			// der Display-Name.
			.map((s) => ({ value: s.name, label: hangarDisplayName(s) }))
			);

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
	// t_15e598e7: Alters-Schwelle für den Preisstand-Chip — Zeilen, deren
	// UEX-Preisstand (date_modified) älter als 7 Tage ist, gelten als
	// veraltete Preisdaten (Warnfarbe) statt unauffällig frisch zu wirken.
	const ROUTE_LOC_STALE_SECS = 7 * 86400;
	function rLocStale(loc: RouteLocationRow): boolean {
		return loc.updated > 0 && nowSec() - loc.updated > ROUTE_LOC_STALE_SECS;
	}
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
				// Tray tr('common.open_config'): Fenster zeigen macht das Backend — hier
				// nur der Config-View (gleicher Pfad wie der Zahnrad-Button).
				await listen(overlayTrayOpenConfig, () => openConfig());
				// UC t_d323be7e: Backend-Signal nach einem Update-Check —
				// die Note/im Config-View zieht live nach (auch im Tray-Pfad).
				await listen('updater://status', () => { void loadUpdaterStatus(); });
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
		// PW (t_fbe40115, Punkt 7): F5 im Marktmodus (Angebote, Mein Shop,
		// Routen-Sweep) triggert sync_now → SYNC-Event. Ohne Guard setzte
		// dieser Handler phase = idle/typing und warf die Ansicht mitten
		// im Routen-Tab zurück zur Suche. Der Marktmodus besitzt seine
		// eigenen Sync-Quellen (onF5) — die Statuszeile trägt das Event.
		if (phase === 'market') return;
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
	}

	// PS (t_c84e545d): Detail-Refresh (Button oder F5) — force = true lässt
	// das Backend die Frische-Prüfung für GENAU diesen Datensatz umgehen.
	async function refreshDetailLive() {
		const h = detailHit;
		if (!h || xpan?.busy) return;
		const kindArg =
			h.kind === 'terminal' ? 'terminal' : h.kind === 'location' ? 'location' : h.kind === 'item' ? 'item' : null;
		xpan = { busy: true, detail: xpan?.detail ?? null, error: null };
		const myToken = ++xpanToken;
		try {
			const d = await invoke<PriceDetail>('get_prices', {
				commodity: h.name,
				...(kindArg && h.id ? { id: h.id } : {}),
				...(kindArg ? { kind: kindArg } : {}),
				force: true
			});
			if (myToken === xpanToken) xpan = { busy: false, detail: d, error: null };
			flashStatus(`Detail aktualisiert · Stand ${rel(d.fetched_at)}`);
		} catch (e) {
			if (myToken === xpanToken) xpan = { busy: false, detail: null, error: String(e) };
		}
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
			flashStatus(tr('shop.sync_running'));
			await loadShop();
			flashStatus(
				shopErr
					? `Shop-Sync fehlgeschlagen · ${shopErr}`
					: `Mein Shop aktualisiert · Quelle ${shopSource === 'live' ? 'UEX live' : shopSource === 'cache' ? 'Cache' : 'Offline-Snapshot'}`
			);
			return;
		}
		if (mTab === 'routes') {
			flashStatus(tr('routes.sweep_running'));
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
		flashStatus(tr('market.sync_running'));
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
		// PW-5 (t_fbe40115): Navigation nur über die SICHTBAREN Angebote (mVew)
		// — der alte Clamp auf visOffers.length erlaubte Selektion unsichtbarer
		// Zeilen (Bug „separate Listen"). Am letzten sichtbaren Element wird
		// automatisch nachgeladen (Pfeiltaste über letztes Element).
		const last = mVew.length - 1;
		if (dir > 0 && mSel >= last && mMshown < visOffers.length) {
			loadMore();
			return;
		}
		mSel = Math.min(Math.max(mSel + dir, 0), last);
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

	// PW-5 (t_fbe40115): Auto-Pagination — Scroll-Ende in der Angebotsliste
	// löst das Nachladen aus (kein Button mehr). Schwellwert nahe am Boden,
	// damit das Nachladen einsetzt BEVOR der User am absoluten Rand steht.
	function onMarketListScroll(e: Event) {
		const el = e.currentTarget as HTMLElement | null;
		if (!el || mLoadingMore || !mAllLoaded) return;
		if (mMshown >= visOffers.length) return;
		if (el.scrollTop + el.clientHeight >= el.scrollHeight - 40) loadMore();
	}
	let mListEl: HTMLElement | null = $state(null);

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
	// Q2 (t_56d5264a): dSelect/dChosen/dToast entfallen — die alte
	// Enter-Auswahl war eine Sackgasse (Highlight ohne Funktion). Neu:
	// Commodity-Detail Enter öffnet das Trade-Erfassen-Popup (siehe
	// openTrkPrefill), überall sonst macht Enter NICHTS. ↑↓-Fokus (dSel/is-sel)
	// bleibt für die Zeilen-Navigation. Die Klassen sind deklarativ per class:
	// im Template gebunden (siehe .pr in der Detail-Tabelle), NICHT per
	// DOM-Query — manuell gesetzte Klassen überleben den Svelte-Flush nicht.

	// QA Runde 2 (N3): rohen serde-/Rust-Fehlertext nicht als deutsche UI zeigen.
	// Bekannte Muster auf freundliche Meldungen mappen; Unbekanntes bleibt lesbar,
	// aber mit deutschem Rahmen statt technischem Fehler-String.
	function friendlyErr(e: unknown): string {
		const s = String(e ?? '');
		const lo = s.toLowerCase();
		if (!s) return tr('market.price_err_unknown');
		if (lo.includes('invalid type: null') || lo.includes('expected f64') || lo.includes('expected u'))
			return tr('market.price_err_empty');
		if (lo.includes('requests limit reached') || lo.includes('rate limit'))
			return tr('market.price_err_ratelimit');
		if (lo.includes('not found'))
			return tr('market.price_err_notfound');
		if (lo.includes('timeout') || lo.includes('timed out'))
			return tr('market.price_err_timeout');
		if (lo.includes('connection failed') || lo.includes('network') || lo.includes('offline'))
			return tr('market.price_err_network');
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
		if (s.syncing) return { text: tr('market.sync_running_caps'), cls: 'load' };
		if (!hasData(s)) return s.last_error ? { text: 'SYNC FEHLER', cls: 'alert' } : { text: tr('market.sync_running_caps'), cls: 'load' };
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

	// TW (t_f178b084): Info-Popup („ⓘ“-Chip RECHTS neben dem Fokus/Klick-Button
	// im spot-status-Bereich) — Lockup + Version + Build-Datum, Community-
	// Disclaimer (Pflicht), UEX-Badge (offizielles PNG-Asset) + SC-Wiki-
	// Text-Badge, Repo-/Release-Links. Kein View-Wechsel: shell-anchored
	// advert-scrim wie die übrigen Popups; andere offene Popups bleiben
	// unangetastet (eigener State, Esc schließt nur dieses Popup).
	let infoOpen = $state(false);
	let infoVersion = $state<string | null>(null);
	let infoMsg = $state<string | null>(null);

	async function openInfo() {
		infoMsg = null;
		infoOpen = true;
		if (infoVersion === null) {
			// Sofort lesbar (Build-Zeit-Konstante aus tauri.conf.json), dann
			// die echte Laufzeit-Version aus packageInfo nachziehen.
			infoVersion = __APP_VERSION__;
			try {
				const v = await getVersion();
				if (v) infoVersion = v;
			} catch {
				// Ohne Tauri (SSR/dev) bleibt die Build-Zeit-Konstante.
			}
		}
	}

	function closeInfo() {
		infoOpen = false;
		infoMsg = null;
	}

	async function openInfoLink(kind: string) {
		// Whitelist im Backend (info::open_info_link) — die UI nennt nur den
		// kind, niemals eine freie URL. Fehler als Meldung IM Popup (nicht
		// modal), kein Retry, kein View-Wechsel.
		infoMsg = null;
		try {
			await invoke<string>('open_info_link', { kind });
		} catch (e) {
			infoMsg = String(e);
		}
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
		// TW (t_f178b084): Info-Popup offen → es schluckt die Tastatur komplett
		// (Esc/✕ schließen). Der Guard steht VOR den Guards der anderen Popups:
		// Esc schließt nur das Info-Popup und lässt die dahinter liegenden
		// Popups unangetastet (kein gegenseitiges Zerstören).
		if (infoOpen) {
			if (k === 'Escape') {
				e.preventDefault();
				closeInfo();
			}
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
		// Q2 (t_56d5264a): Trade-Erfassen-Popup offen → es schluckt die
		// Tastatur, egal aus welchem Modus es geöffnet wurde (auch aus dem
		// Commodity-Detail). Escape: erst die 2-Schritt-Bestätigung schließen,
		// dann das Formular (Abbruch). Enter bestätigt via <form>-Submit.
		if (trkOpen) {
			if (k === 'Escape') {
				e.preventDefault();
				if (trkConfirm) trkConfirm = null;
				else {
					trkOpen = false;
					trkMsg = null;
					trkEditId = null;
				}
			} else if (k === 'Enter') {
				// Q2: Enter im Popup bestätigt — in den Zahl-Feldern (Menge/
				// Preis) geht es in die 2-Schritt-Bestätigung, im Bestätigungs-
				// Dialog führt es die Aktion aus. Typeahead-Felder sind aus-
				// genommen (deren Enter übernimmt die Vorschlagsauswahl).
				e.preventDefault();
				const t = e.target;
				const inNum = t instanceof HTMLInputElement && t.type === 'number';
				if (trkConfirm) {
					if (trkConfirm.kind !== 'remove') void doTrkWrite();
				} else if (inNum) {
					requestTrkWrite();
				}
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
			// BUG t_3a345e4f: Das alte else-if hat Escape für ALLE anderen
			// railModes (inkl. 'inventory') selbst consumed → Esc aus dem
			// Inventar-Detail warf direkt nach SUCHE, statt eine Ebene
			// zurück. Inventory.svelte behandelt Esc selbst (back()-Kette
			// bis onexit), also hier NICHT anfassen.
			if (railMode === 'hangar') {
				if (k === 'Escape') {
					hangarBack();
					e.preventDefault();
					return;
				}
				// R2 (Punkt 1): Formular-/Bestätigungs-Popup offen → es schluckt
				// die Tastatur (keine Listen-Navigation darunter). Enter im
				// Formular bestätigt über den <form>-Submit-Pfad.
				if (hangarFormOpen) {
					if (k === 'Enter' && hangarConfirm?.kind === 'save') {
						e.preventDefault();
						void hangarSaveForm();
					}
					return;
				}
				// TC (t_f269f5b7): Detail-Popup offen → schluckt ebenfalls die
				// Tastatur (Esc oben schließt es); die Liste darunter navigiert
				// nicht. Typeahead/Selects im Popup tippen über die Feld-Handler.
				if (hangarDetail) {
					return;
				}
				// Liste (immer sichtbar — das Detail ist ein Popup): ↑/↓ Auswahl,
				// Enter öffnet das Detail-Popup.
				{
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
			} else if (railMode !== 'inventory' && k === 'Escape') {
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
			// PS (t_c84e545d): F5 auf einer offenen Detail-Seite (Commodity
			// ODER Item) aktualisiert GENAU DIESEN Datensatz live — einzelner
			// API-Call (rate-limit-bewusst), Frische-Datum in der Anzeige.
			if (inDetail && detailHit) {
				e.preventDefault();
				void refreshDetailLive();
				return;
			}
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
				// PT (t_7fdc1317): Tracking-Sub-Reiter — Esc schließt Formular/
				// Bestätigung zuerst, Pfeile/Enter navigieren NICHT die Matrix.
				if (rTab === 'tracking') {
					if (k === 'Escape') {
						if (trkConfirm) trkConfirm = null;
						else if (trkOpen) {
							trkOpen = false;
							trkMsg = null;
							trkEditId = null;
						} else toggleMarket();
						e.preventDefault();
						return;
					}
					return; // Tracking: keine Listen-Navigation
				}
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
			// MQ (t_d6ff5212): Lightbox offen → sie schluckt die Tastatur. Esc
			// schließt, alle anderen Offer-/Market-Shortcuts laufen nicht.
			if (lightbox) {
				if (k === 'Escape') {
					lightbox = null;
					e.preventDefault();
				}
				return;
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
			// QW-2 (t_28bfd027): ←/→ open/close das Offer-Detail NUR im
			// Angebote-Reiter — davor leckte die Open/Close-Logik über die
			// anderen Market-Reiter (Mein Shop/Trades), wo die Auswahl eines
			// Offers semantisch keinen Sinn ergibt. Enter/Escape decken den
			// Detail-Zugang überall ab; in shop/trades haben ←/→ keinerlei
			// Wirkung mehr.
			if (mTab === 'offers' && k === 'ArrowRight' && !offerDetail) {
				openOfferDetail();
				e.preventDefault();
				return;
			}
			if (mTab === 'offers' && k === 'ArrowLeft' && offerDetail) {
				offerDetail = null;
				lightbox = null;
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
					lightbox = null;
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
			// Q2 (t_56d5264a): Enter-Auswahl (Highlight/Sackgasse) entfällt.
			// Nur im Commodity-Detail öffnet Enter das vorausgefüllte
			// Trade-Erfassen-Popup (openTrkPrefill); Terminal/Location/Item-
			// Detail: Enter macht NICHTS (dSelect ist weg). ↑↓-Fokus bleibt.
				if (detailHit?.kind === 'commodity') {
					const r = dRows[dSel];
					if (r) {
						openTrkPrefill(
						r.kind,
						r.kind === 'buy' ? r.p.buy_price : r.p.sell_price,
						r.p,
						xpan?.detail?.commodity ?? detailHit?.name ?? ''
					);
					}
				}
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
	<!-- UG (t_a1f0d83f): shell-info — solange das Info-Popup offen ist,
	     bekommt die content-driven Shell ein min-height-Floor, damit das
	     (jetzt größere, Abschnitts-) Popup ohne internen Scroll vollständig
	     in die Shell passt (max-height 84vh bleibt die Obergrenze). -->
	<div
		class="shell"
		class:in-detail={inDetail}
		class:shell-advert={advertOpen || trkOpen || hangarFormOpen}
		class:shell-info={infoOpen}
	>

												<!-- PT: Formular „Trade erfassen" als modales Popup.
											     QW-5 (t_28bfd027): der frühere confirm-scrim war position:fixed
											     am VIEWPORT zentriert — im Overlay-Fenster saß das Popup dadurch
											     unterhalb der Shell-Kante (Probe: Modal bis y=660 bei Shell-Ende
											     y=372), Felder/Buttons unten unerreichbar. Jetzt wie das
											     Listing-Popup (advert-scrim): absolute in die Shell, volle
											     nutzbare Höhe, Body scrollt intern, Fuß (Buttons) fix. -->
												{#if trkOpen}
													<div class="advert-scrim" role="presentation">
														<div class="trk-modal" role="dialog" aria-modal="true" aria-label={$t('trades.form.title')}>
														<!-- Q2 (t_56d5264a): echtes <form> — Enter in den Zahl-Feldern bestätigt
														     (impliziter Submit) wie der „Trade prüfen”-Button. -->
															<form class="trk-body" onsubmit={(e) => { e.preventDefault(); requestTrkWrite(); }}>
									<div class="center" style="padding: 0 0 10px">
										<div class="big">{trkEditId ? $t('trades.form.edit_caps') : $t('trades.form.create_caps')}</div>
									</div>
																	<!-- QY-2 (t_07edb03d): Formular-Rhythmus wie das Listing-Popup —
								     Felder in VOLLER Popup-Breite, größere Feld-Überschriften,
								     vertikaler Abstand; Scrim zentriert, Body scrollt intern. -->
								<div class="trk-field">
									<span class="trk-lbl">{$t('trades.direction')}</span>
									<div class="trk-dir">
										<button type="button" class="chip" class:is-on={trkForm.operation === 'buy'} onclick={() => (trkForm.operation = 'buy')}>{$t('common.buy')}</button>
										<button type="button" class="chip" class:is-on={trkForm.operation === 'sell'} onclick={() => (trkForm.operation = 'sell')}>{$t('common.sell')}</button>
									</div>
								</div>
								<div class="trk-field">
									<span class="trk-lbl">{$t('common.commodity')}</span>
									<RoutesTypeahead
										fullWidth
										bind:value={trkForm.commodity}
										options={trkComOpts}
										placeholder={$t('common.ph_eg_gold')}
										ariaLabel={$t('trades.ph_commodity_typeahead')}
										onselect={(v: string) => {
											trkForm.commodity_id = Number(v) || null;
											// QY-3: Ort-Auswahl invalidieren — der Ort muss die
											// Ware in der gewählten Richtung handeln.
											trkForm.terminal = '';
											trkForm.terminal_id = null;
											trkTermOpts = [];
										}}
									/>
								</div>
								<div class="trk-field">
									<span class="trk-lbl">{$t('trades.location')}</span>
									<RoutesTypeahead
										fullWidth
										bind:value={trkForm.terminal}
										options={trkTermOpts}
										placeholder={$t('common.ph_eg_location')}
										ariaLabel={$t('trades.ph_location_typeahead')}
										onselect={(v: string) => {
											trkForm.terminal_id = Number(v) || null;
											trkComOpts = [];
										}}
									/>
								</div>
								<div class="trk-field">
									<span class="trk-lbl">{$t('trades.qty_scu')}</span>
									<input class="qnum" style="width:100%" type="number" min="1" placeholder="SCU" bind:value={trkForm.scu} />
								</div>
								<div class="trk-field">
									<span class="trk-lbl">{$t('trades.price_scu')}</span>
									<input class="qnum" style="width:100%" type="number" min="0" step="any" placeholder="aUEC" bind:value={trkForm.price} />
								</div>
<div class="sm" style="color: var(--fgdim); margin-top: 6px">{$t('trades.date_uex_note_title')}</div>
									{#if trkMsg}
																	<div class="sm" style="color: var(--alert); margin-top: 6px" role="alert">{trkMsg}</div>
																{/if}
																</form>
									<div class="advert-foot">
										<div class="md-actions">
											<button type="button" class="btn" disabled={trkBusy} onclick={requestTrkWrite}>
												{trkBusy ? $t('common.saving') : trkEditId ? $t('trades.review_change') : $t('trades.review_trade')}
											</button>
											<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={() => { trkOpen = false; trkMsg = null; trkEditId = null; }}>{$t('trades.cancel_esc')}</span>
										</div>
									</div>
									</div>
									</div>
									{/if}
						<!-- PT: 2-Schritt-Bestätigung (add/edit/remove) — QW-5: jetzt
						     ebenfalls shell-anchored (advert-scrim) statt fixed am Viewport. -->
						{#if trkConfirm}
							<div class="advert-scrim" role="presentation">
								<div class="confirm-modal" role="dialog" aria-modal="true" aria-label={$t('common.confirm')}>
									<div class="center" style="padding: 0 0 10px">
										<div class="big" style="color: var(--alert)">
											{trkConfirm.kind === 'remove' ? $t('trades.confirm_delete_title') : trkConfirm.kind === 'edit' ? $t('trades.confirm_edit_title') : $t('trades.confirm_create_title')}
										</div>
									</div>
									{#if trkConfirm.kind === 'remove'}
										<div class="sm" style="color: var(--fgdim)">{$t('trades.delete_warning')}</div>
									{:else}
										<div class="c-row"><b>{$t('trades.direction')}</b><span>{trkForm.operation === 'sell' ? 'Verkauf' : 'Kauf'}</span></div>
										<div class="c-row"><b>{$t('common.commodity')}</b><span>{trkForm.commodity || '—'}</span></div>
										<div class="c-row"><b>{$t('trades.location')}</b><span>{trkForm.terminal || '—'}</span></div>
										<div class="c-row"><b>{$t('common.quantity')}</b><span>{Math.round(Number(trkForm.scu))} SCU</span></div>
										<div class="c-row"><b>{$t('common.price')}</b><span>{fmtPrice(Number(trkForm.price))} aUEC/SCU</span></div>
										<div class="sm" style="margin-top: 8px; color: var(--fgdim)">Der Trade wird auf uexcorp.space {(trkConfirm.kind === 'edit' ? $t('trades.changed_caps') : 'ANGELEGT')}.</div>
									{/if}
									<div class="md-actions">
										<button type="button" class="btn" disabled={trkBusy} onclick={() => { if (trkConfirm) (trkConfirm.kind === 'remove' ? void doTrkRemove() : void doTrkWrite()); }}>
											{trkBusy ? $t('common.sending') : $t('common.confirm_btn')}
										</button>
										<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={() => (trkConfirm = null)}>{$t('trades.cancel_esc')}</span>
									</div>
								</div>
							</div>
						{/if}

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
			<!-- svelte-ignore a11y_autofocus --><!-- Autofocus: Suche ist der primäre Einstiegspunkt des Overlays -->
			<input
				class="q"
				type="text"
				autofocus
				bind:this={searchEl}
				bind:value={input}
				oninput={onInput}
				placeholder={railMode === 'inventory'
				? $t('search.ph_inventory')
				: railMode === 'hangar'
					? $t('search.ph_hangar')
					: phase === 'market'
					? mTab === 'routes'
						? $t('search.ph_routes')
						: $t('search.ph_market')
					: inDetail
					? detailHit?.kind === 'location' || detailHit?.kind === 'terminal'
						? `Filter: Waren in ${detailHit?.name} …`
						: `Filter: Orte mit Preisen für ${detailHit?.name} …`
					: $t('search.ph_default')}
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
			<div class="hangar" role="region" aria-label={$t('nav.hangar')}>
				<header class="hangar-head">
					<button class="hangar-back" onclick={hangarBack} title={$t('common.back_esc')} aria-label={$t('common.back')}>←</button>
					<span class="hangar-title">{$t('nav.hangar_caps')}</span>
					<div class="hangar-filters">
						{#each [['all', 'ALLE'], ['pledge', 'PLEDGE'], ['ingame', 'INGAME'], ['lti', 'LTI'], ['module', 'MODULE']] as [f, lbl] (f)}
							<button
								type="button"
								class="hchip"
								class:is-on={hangarFilter === f}
								onclick={() => { hangarFilter = f as typeof hangarFilter; hangarSel = 0; hangarDetail = null; }}
							>{lbl}</button>
						{/each}
					</div>
					<div class="hangar-actions">
						<button class="hbtn" disabled={hangarBusy} onclick={() => hangarTriggerImport('xplorer')}>{$t('hangar.import_xplorer')}</button>
						<button class="hbtn" disabled={hangarBusy} onclick={() => hangarTriggerImport('ccugame')}>{$t('hangar.import_ccugame')}</button>
						<button class="hbtn" disabled={hangarBusy} onclick={hangarExport}>{$t('hangar.export')}</button>
						<button class="hbtn" disabled={hangarBusy} onclick={hangarOpenAdd} title={$t('hangar.add_manual')}>{$t('hangar.plus_create')}</button>
					</div>
				</header>
				<!-- S1 (t_38f79fff): KEINE Inline-Meldung über der Liste mehr —
				     Meldungen erscheinen als Modal-Toast im Vordergrund (siehe
				     hangar-toast unten, z über Formular-/Confirm-Scrim). -->
				<!-- S1 (t_38f79fff): Liste bleibt beim offenen Anlegen-/Edit-Popup
				     UNVERÄNDERT hinter dem Scrim stehen (kein DOM-Entfernen, kein
				     Shell-Schrumpfen — die Shell wächst stattdessen via
				     class:shell-advert, gleicher Mechanismus wie Listing-/Trade-
				     Popup). -->
				<!-- TC (t_f269f5b7): Liste ist IMMER sichtbar — das Detail ist ein
					     Popup (unten) und ersetzt die alte eigene Detail-Seite
					     (hangarView 'list'/'detail' entfernt, kein doppelter Pfad). -->
					<div class="hangar-list" role="listbox" aria-label={$t('hangar.entries_aria')}>
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
									{#if it.entity_type === 'module' && !it.base_ship}<span class="hmodel">{$t('hangar.unlinked_module_prefix')}</span>{/if}
								</span>
								<span class="hchip o-{it.origin}">{it.origin === 'pledge' ? 'PLEDGE' : 'INGAME'}</span>
								<span class="hchip ins">{it.insurance ?? '—'}</span>
								<!-- TB (t_78a4a5e5, Punkt 3): Preis-Anzeige entfernt. -->
							</button>
						{:else}
							<div class="hangar-empty">{$t('hangar.no_entries')}{hangarFilter !== 'all' ? ` (${$t('common.filter')}: ${hangarFilter.toUpperCase()})` : ''} {input.trim() ? ` · ${$t('common.search')}: ${input.trim()}` : ''} — {$t('hangar.import_above')}</div>
						{/each}
						</div>
						{#if hangarDetail}
					<!-- TC (t_f269f5b7): Schiffs-/Fahrzeug-Details als modales Popup —
					     GLEICHES Muster wie die Anlege-/Edit-Formulare (shell-anchored
					     advert-scrim, Breite ~90 %, volle Höhe minus Scrim-Rand, Body
					     scrollt intern). Die Liste bleibt unverändert hinter dem Scrim
					     stehen; Schließen per Esc (hangarBack) oder Close-Button, ohne
					     View-Wechsel. -->
					<div class="advert-scrim" role="presentation">
						<div class="hdetail-modal" role="dialog" aria-modal="true" aria-label={`Details ${hangarDisplayName(hangarDetail)}`}>
							<div class="hd-head">
								<span class="hd-name">{hangarDisplayName(hangarDetail)}</span>
								<span class="hchip o-{hangarDetail.origin}">{hangarDetail.origin === 'pledge' ? 'PLEDGE' : 'INGAME'}</span>
								<span class="hchip ins">{hangarDetail.insurance ?? '—'}</span>
								{#if !hangarEdit}
									<button class="hbtn" disabled={hangarBusy} onclick={() => hangarStartEdit(hangarDetail!)}>{$t('common.edit')}</button>
								{/if}
								<button class="hbtn danger" disabled={hangarBusy} onclick={() => hangarDelete(hangarDetail!)}>{$t('common.delete')}</button>
								<button type="button" class="hd-close" title={$t('common.close_esc')} aria-label={$t('hangar.close_details')} onclick={hangarCloseDetail}>✕</button>
							</div>
							<div class="hangar-detail">
							{#if !hangarEdit}
							<!-- R2 (Punkt 7): Ship-Code/Pledge/Pledge-ID/Warbond/CCUD ENTFERNT —
							     Import-Artefakte („nicht wichtig“), keine Anzeige mehr. -->
							<dl class="hd-grid">
								<div><dt>{$t('hangar.model')}</dt><dd>{hangarDetail.name}</dd></div>
								<div><dt>{$t('hangar.manufacturer')}</dt><dd>{hangarDetail.manufacturer || '—'}</dd></div>
								<div><dt>{$t('hangar.type')}</dt><dd>{hangarDetail.entity_type}</dd></div>
								<div><dt>{$t('hangar.custom_name')}</dt><dd>{hangarDetail.custom_name || '—'}</dd></div>
								<div><dt>{$t('hangar.date')}</dt><dd>{isoToDe(hangarDetail.purchase_date) || '—'}</dd></div>
								<!-- TB (t_78a4a5e5, Punkt 3): Preis-Zeile entfernt. -->
							</dl>
							{#if hangarDetail.entity_type !== 'module' && hangarWikiMods?.known}
							<!-- SA (t_d0076295): Nur-Datei-Regel — der GESAMTE Module-
							     Bereich erscheint NUR bei Schiffen mit Eintrag in
							     module-overrides.json; Schiffe ohne Eintrag zeigen
							     GAR NICHTS über Module (kein Hinweistext mehr). -->
							<div class="hd-mods">
								<div class="h">Module ({hangarModulesOf(hangarDetail.name).length})</div>
								{#each hangarModulesOf(hangarDetail.name) as m (m.id ?? m.name)}
									<!-- TB (t_78a4a5e5, Punkt 2): Modulzeile zeigt Versicherung
									     UND Herkunft; Klick auf den Namen öffnet das Edit-Popup. -->
									<div class="hmod">
										<button type="button" class="hmod-name" title={$t('hangar.module_edit')} aria-label={`Modul ${hangarDisplayName(m)} bearbeiten`} onclick={() => hangarStartEdit(m)}>{hangarDisplayName(m)}</button>
										<span class="hchip o-{m.origin}">{m.origin === 'pledge' ? 'PLEDGE' : 'INGAME'}</span>
										<span class="hchip ins">{m.insurance ?? '—'}</span>
										<button type="button" class="hmod-x" title={$t('hangar.module_remove')} aria-label={`Modul ${hangarDisplayName(m)} entfernen`} disabled={hangarBusy} onclick={() => hangarModuleRemove(m)}>✕</button>
									</div>
								{:else}
									<div class="hmodempty">{$t('hangar.no_modules')}</div>
								{/each}
								<!-- S4 (t_66980b3c): Nur-Datei-Regel — Feld NUR bei
							     Schiffen mit Eintrag in module-overrides.json;
							     kein API-Fallschirm mehr. -->
								{#if hangarModFieldVisible}
								<div class="hmod-add">
									<RoutesTypeahead bind:value={hangarModPick} options={hangarModOpts} fullWidth ariaLabel={$t('hangar.search_module_btn')} placeholder={$t('hangar.ph_search_module')} onselect={() => {}} />
								</div>
								<!-- TB (t_78a4a5e5, Punkt 2): Versicherung + Herkunft je Modul —
								     gleiche Optionen wie der Hangar-Eintrag; Herkunft ist
								     Pflichtwahl (startet leer), Versicherung '' = Ohne. -->
								<div class="hmod-add hmod-add-meta">
									<SelectCombo bind:value={hangarModOrigin} ariaLabel={$t('hangar.module_origin_aria')} placeholder={$t('hangar.ph_choose_origin')} options={[{ value: 'pledge', label: $t('hangar.origin_pledged') }, { value: 'ingame', label: $t('hangar.origin_ingame') }]} />
									<SelectCombo bind:value={hangarModIns} ariaLabel={$t('hangar.module_insurance_aria')} options={INSURANCE_CHOICES.map((c) => ({ ...c, label: $t(c.label) }))} placeholder={$t('common.none')} />
									<button type="button" class="hbtn" disabled={hangarBusy || !hangarModPick.trim()} onclick={hangarModuleAdd}>{$t('hangar.add')}</button>
								</div>
								{/if}
							</div>
							{:else if hangarDetail.entity_type === 'module' && !hangarDetail.base_ship}
							<!-- R2: unverbundenes Modul (Migration/Bestand ohne base_ship) —
							     bleibt erhalten, bis es hier manuell zugeordnet wird. -->
							<div class="hd-mods">
								<div class="h">{$t('hangar.module_unlinked')}</div>
								<div class="hmodempty">{$t('hangar.module_unassigned_note')}</div>
								<div class="hmod-add">
									<RoutesTypeahead bind:value={hangarAssignBase} options={hangarAssignOpts} fullWidth ariaLabel={$t('hangar.assign_base_title')} placeholder={$t('hangar.ph_search_base_ship')} onselect={hangarPickAssign} />
									<button type="button" class="hbtn" disabled={hangarBusy || !hangarAssignBase.trim()} onclick={() => hangarModuleAssign(hangarDetail!)}>{$t('hangar.assign')}</button>
								</div>
							</div>
							{/if}
							{/if}
							</div>
						</div>
					</div>
				{/if}
			</div>
			{#if hangarFormOpen}
				<!-- R2 (Punkt 1): Hangar-Formular als modales Popup — shell-anchored
				     advert-scrim (wie Listing-/Trade-Formular), volle nutzbare Höhe,
				     Body scrollt intern, Buttons fix unten. Bearbeiten öffnet DASSELBE
				     Popup über dem Detail. Bestätigung (2-Schritt) wie elsewhere. -->
				<div class="advert-scrim" role="presentation">
					<div class="hform-modal" role="dialog" aria-modal="true" aria-label={hangarEdit ? $t('hangar.edit_entry_title') : $t('hangar.create_entry_title')}>
						<form onsubmit={(e) => { e.preventDefault(); requestHangarSave(); }}>
						<div class="hform-body">
							<div class="mbar2" style="margin-bottom: 14px"><span class="m-lbl">{hangarEdit ? $t('hangar.edit_entry_caps') : $t('hangar.create_entry_caps')}</span></div>
							<!-- R2 (Punkt 4): Name in voller Popup-Breite (Schiffsnamen sind lang). -->
							<div class="trk-field">
								<span class="trk-lbl">{$t('common.name')}</span>
								<!-- R2 (Punkt 2/9): einheitlicher Typeahead (Maus + Tastatur),
								     ALLE Schiffe UND Fahrzeuge (registrierte zuerst, dann Katalog).
								     TB (t_78a4a5e5, Punkt 2): bei Modul-Edit Freitext — das
								     Name-Typeahead schlägt Schiffe vor und würde via
								     hangarPickName den entity_type des Moduls überschreiben. -->
								{#if hangarForm.entity_type === 'module'}
									<input class="rtext" style="width: 100%" aria-label={$t('hangar.module_name')} placeholder={$t('hangar.ph_eg_module')} bind:value={hangarForm.name} />
								{:else}
									<RoutesTypeahead bind:value={hangarForm.name} options={hangarNameOpts} fullWidth ariaLabel={$t('hangar.name_aria')} placeholder={$t('hangar.ph_eg_ship')} onselect={hangarPickName} />
								{/if}
							</div>
							<div class="hform-row">
								<!-- R2 (Punkt 3): Overlay-Dropdowns (QY-2-Stil) statt nativer
								     Selects. Typ=Modul ENTFERNT (Punkt 8: Module nur noch in
								     den Ship-Details). -->
								<div class="trk-field">
									<span class="trk-lbl">{$t('hangar.type')}</span>
									<!-- TB (t_78a4a5e5, Punkt 1): startet leer (Platzhalter) —
									     Pflichtwahl bei Speichern. TB Punkt 2: bei Modul-Edit
									     statisch „Modul“ (Module werden nicht als Schiff/Fahrzeug
									     umtypisiert; die Auswahl kennt keinen Modul-Eintrag). -->
									{#if hangarForm.entity_type === 'module'}
										<input class="rtext" style="width: 100%" value="Modul" disabled aria-label={$t('hangar.module_type')} />
									{:else}
										<SelectCombo bind:value={hangarForm.entity_type} ariaLabel={$t('hangar.type')} placeholder={$t('common.choose_placeholder')} options={[{ value: 'ship', label: $t('common.ship') }, { value: 'vehicle', label: $t('common.vehicle') }]} />
									{/if}
								</div>
								<div class="trk-field">
									<span class="trk-lbl">{$t('hangar.origin')}</span>
									<!-- TB (t_78a4a5e5, Punkt 1): startet leer (Platzhalter) —
									     Pflichtwahl bei Speichern. Labels lt. Task „Pledged /
									     In-Game“ (gleiche Werte wie die Modul-Add-Zeile). -->
									<SelectCombo bind:value={hangarForm.origin} ariaLabel={$t('hangar.origin')} placeholder={$t('common.choose_placeholder')} options={[{ value: 'ingame', label: $t('hangar.origin_ingame') }, { value: 'pledge', label: $t('hangar.origin_pledged') }]} />
								</div>
							</div>
							<div class="hform-row">
								<!-- R2 (Punkt 5): Versicherung als Select. -->
								<div class="trk-field">
									<span class="trk-lbl">{$t('hangar.insurance')}</span>
									<SelectCombo bind:value={hangarFormIns} ariaLabel={$t('hangar.insurance')} options={INSURANCE_CHOICES.map((c) => ({ ...c, label: $t(c.label) }))} placeholder={$t('common.none')} />
								</div>
								<!-- TB (t_78a4a5e5, Punkt 3): Preis-Feld entfernt. -->
								</div>
							<!-- R2 (Punkt 6): Datumseingabe dd.mm.jjjj (gespeichert ISO). -->
							<div class="hform-row">
								<div class="trk-field">
									<span class="trk-lbl">{$t('hangar.date')}</span>
									<input class="rtext" style="width: 100%" placeholder="31.01.2025" aria-label={$t('hangar.date_hint')} bind:value={hangarFormDate} />
								</div>
								<div class="trk-field">
									<span class="trk-lbl">{$t('hangar.custom_name')}</span>
									<input class="rtext" style="width: 100%" bind:value={hangarForm.custom_name} />
								</div>
							</div>
							<div class="trk-field">
								<span class="trk-lbl">{$t('hangar.manufacturer')}</span>
								<input class="rtext" style="width: 100%" bind:value={hangarForm.manufacturer} />
							</div>
							{#if hangarBaseOpen}
							<!-- Snub-Ships brauchen ein Basisschiff (Bestand: Modul-Zeilen
							     im Edit-Pfad); einheitlicher Typeahead. -->
							<div class="trk-field">
								<span class="trk-lbl">{$t('hangar.base_ship')}</span>
								<RoutesTypeahead bind:value={hangarForm.base_ship} options={hangarBaseOpts} fullWidth ariaLabel={$t('hangar.base_ship')} placeholder={$t('hangar.ph_pick_from_hangar')} onselect={() => {}} />
							</div>
							{/if}
						</div>
						<div class="hform-foot">
							<button type="submit" class="hbtn" disabled={hangarBusy}>{$t('common.save')}</button>
							<button type="button" class="hbtn" onclick={hangarCancelEdit}>{$t('common.cancel')}</button>
						</div>
					</form>
					</div>
			</div>
		{/if}
		{#if hangarConfirm}
		<!-- R2 (Punkt 1): 2-Schritt-Bestätigung (confirm-modal im
		shell-anchored Scrim — NICHT fixed am Viewport, QW-5-Lektion).
		Review-Fix (R2-D1): aus {#if hangarFormOpen} herausgezogen — der
		Confirm muss auch im DETAIL-View erscheinen (Löschen ohne
		geöffnetes Formular; vorher war der Lösch-Flow tot). -->
		<div class="advert-scrim" style="z-index: 210" role="presentation">
		<div class="confirm-modal" role="dialog" aria-modal="true" aria-label={$t('common.confirm')}>
			<div class="center" style="padding: 0 0 10px">
				<div class="big" style="color: var(--alert)">
					{hangarConfirm.kind === 'delete' ? $t('hangar.confirm_delete_title') : hangarEdit ? $t('hangar.confirm_save_title') : $t('hangar.confirm_create_title')}
				</div>
			</div>
			{#if hangarConfirm.kind === 'delete'}
				<div class="c-row"><b>{$t('common.name')}</b><span>{hangarDisplayName(hangarDetail ?? hangarForm)}</span></div>
				<div class="sm" style="margin-top: 8px; color: var(--fgdim)">{$t('hangar.delete_warning')}</div>
			{:else}
				<div class="c-row"><b>{$t('common.name')}</b><span>{hangarForm.name}</span></div>
				<div class="c-row"><b>{$t('hangar.type')}</b><span>{hangarForm.entity_type === 'vehicle' ? 'Fahrzeug' : hangarForm.entity_type === 'module' ? 'Modul' : hangarForm.entity_type ? 'Schiff' : '—'}</span></div>
				<div class="c-row"><b>{$t('hangar.origin')}</b><span>{hangarForm.origin === 'pledge' ? 'Pledged' : hangarForm.origin === 'ingame' ? 'In-Game' : '—'}</span></div>
				<div class="c-row"><b>{$t('hangar.insurance')}</b><span>{hangarFormIns || $t('common.none')}</span></div>
				<div class="c-row"><b>{$t('hangar.date')}</b><span>{hangarFormDate || '—'}</span></div>
				<!-- TB (t_78a4a5e5, Punkt 3): Preis-Zeile im Confirm entfernt. -->
				<div class="sm" style="margin-top: 8px; color: var(--fgdim)">{$t('hangar.inputs_locked_note')}</div>
			{/if}
			<div class="mbar2" style="margin-top: 12px; justify-content: center; border: 0; padding: 0; background: transparent">
				{#if hangarConfirm.kind === 'delete'}
					<button type="button" class="btn btn-danger" disabled={hangarBusy} onclick={() => hangarDetail && hangarDoDelete(hangarDetail)}>{$t('common.confirm_delete')}</button>
				{:else}
					<button type="button" class="btn" disabled={hangarBusy} onclick={() => void hangarSaveForm()}>{$t('common.confirm_save')}</button>
				{/if}
				<button type="button" class="btn-ghost" disabled={hangarBusy} onclick={() => (hangarConfirm = null)}>{$t('common.cancel')}</button>
			</div>
		</div>
		</div>
		{/if}
		{#if hangarNote}
		<!-- S1 (t_38f79fff): Hangar-Meldungen als Modal-Toast im VORDERGRUND —
		     z-index über dem Formular-Popup (150) UND der 2-Schritt-Bestätigung
		     (210); Erfolg/Fehler landen nie mehr hinter einem Popup. Eigene
		     Karte (QZ-Vorbild: eigenes Popup), kein Inline-Text über der Liste;
		     Auto-Fade weiterhin über hangarNoteSet (~3.5 s). -->
		<div class="hangar-toast" role="alert" aria-live="assertive">
			<span class="ht-msg">{hangarNote}</span>
			<button type="button" class="ht-x" title={$t('common.dismiss_note')} aria-label={$t('common.dismiss_note')} onclick={() => hangarNoteSet('')}>✕</button>
		</div>
		{/if}
		{:else if inConfig}
			<!-- Config-View: eigenes View in der Palette (wie die Detail-"Seite").
			     Esc/zurück-Pfeil → zurück zur Suche; kein neues Fenster. -->
			<div class="pane-config" role="dialog" aria-label={$t('settings.config')}>
				<div class="cfg-head">
					<button
						type="button"
						class="cfg-back"
						onclick={closeConfig}
						title={$t('common.back_to_search_esc')}
						aria-label={$t('common.back_to_search')}
					>←</button>
					<div class="cfg-title">{$t('nav.configuration_caps')}</div>
					<span class="cfg-live">{$t('settings.immediate_note')}</span>
				</div>
				<div class="cfg-body">
										<!-- UD (t_a36daafb): thematische Gruppe „Darstellung“ — reine UI-Umordnung -->
					<div class="cfg-group">
						<div class="h cfg-g-head">{$t('settings.appearance')}</div>
<section class="cfg-sect">
						<div class="h">{$t('settings.color_palette')}</div>
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
							>{$t('settings.mfcd')}</button>
							<button
								type="button"
								class="chip"
								class:is-on={cfg?.palette === 'bloom'}
								onclick={() => choosePalette('bloom')}
							>Bloom</button>
						</div>
					</section>
<section class="cfg-sect">
						<div class="h">{$t('settings.font_scale')}</div>
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
						<div class="h">{$t('settings.price_detail_variant')}</div>
						<div class="cfg-chips">
							<button
								type="button"
								class="chip"
								class:is-on={cfg?.detail_variant === 'c'}
								onclick={() => chooseVariant('c')}
							>{$t('settings.variant_c_standard')}</button>
							<button
								type="button"
								class="chip"
								class:is-on={cfg?.detail_variant === 'a'}
								onclick={() => chooseVariant('a')}
							>A</button>
						</div>
					</section>
					<!-- T1 (t_530e5f59): Sprache — LIVE-Switch (Store, kein Neustart),
					     Persistenz via settings_set_language, lokale (Community-)Datei
					     per File-Button (locale_import — analog Connector-Plugin-Import).
					     Chips listet localeList: Built-ins (en/de) + lokale Dateien mit
					     Anzeigename aus der Datei (meta.name, native Schreibweise). -->
<section class="cfg-sect cfg-sect-language">
						<div class="h">{$t('settings.language.title')}</div>
						<div class="cfg-chips">
							{#each $localeList.locales as loc (loc.meta.code)}
								<button
									type="button"
									class="chip"
									class:is-on={cfg?.language === loc.meta.code}
									onclick={() => void chooseLanguage(loc.meta.code)}
								>{loc.meta.name}</button>
							{/each}
						</div>
						<div class="cfg-key-row">
							<button type="button" class="chip" onclick={() => localeFileInput?.click()}>
								{$t('settings.language.add')}
							</button>
							<!-- Versteckter File-Input: die Webview zeigt den nativen
							     Dialog, onchange liest file.text() und locale_import
							     validiert+persistiert. Probe steuert denselben Pfad
							     über DataTransfer-Files. -->
							<input
								bind:this={localeFileInput}
								type="file"
								accept=".json,application/json"
								class="cfg-locale-file"
								style="display: none"
								onchange={onLocaleFilePick}
							/>
						</div>
						<div class="cfg-note">{$t('settings.language.add_hint')}</div>
						{#if localeNote}
							<div class="cfg-note" role="status">
								{localeNote.raw ?? $t(localeNote.key, localeNote.params)}
							</div>
						{/if}
					</section>
					</div>
					<!-- UD (t_a36daafb): thematische Gruppe „Bedienung“ — reine UI-Umordnung -->
					<div class="cfg-group">
						<div class="h cfg-g-head">{$t('settings.controls')}</div>
<section class="cfg-sect">
						<div class="h">{$t('settings.shortcuts')}</div>
						{#each shortcutFields as f (f.id)}
							<div class="cfg-sc-row" class:is-cap={scCapture === f.id}>
								<div class="cfg-sc-meta">
									<div class="cfg-sc-lbl">{$t(f.label)}</div>
									<!-- Soll-Ist: aktiv = persistierter Backend-Wert
									     (überlebt Neustart), nicht der Entwurf im Feld. -->
									<div class="cfg-sc-active">{$t('settings.active_label')}<b>{f.display}</b>
									</div>
								</div>
								<button
									type="button"
									class="chip cfg-sc-capture"
									class:is-on={scCapture === f.id}
									onclick={() => (scCapture === f.id ? cancelShortcutCapture() : startShortcutCapture(f.id))}
								>{scCapture === f.id ? 'Abbrechen (Esc)' : $t('common.change')}</button>
								{#if scCapture === f.id}
									<div class="cfg-sc-help" role="status">
										{scPreview ? `Gewählt: ${scPreview} — prüfen…` : $t('settings.capture_hint')}
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
											<span class="src">{$t('settings.arch_toggle_label')}</span>{$t('settings.changes_immediate_suffix')}</div>
									{:else}
										<div class="cfg-sc-warn" role="status">
											Gespeicherte Kürzel sind die kanonische Quelle, aber die
											Hyprland-Binds werden noch nicht daraus geführt.
										</div>
									{/if}
								{:else}
									<div class="cfg-sc-warn" role="status">
										OmaSettings-Plugin nicht gefunden — Binds manuell anlegen
										(<span class="src">{$t('settings.arch_toggle_label')}</span>, Anleitung: docs/WAYLAND.md).
										Die App verändert keine fremde Hyprland-Konfiguration.
									</div>
								{/if}
								{#if cfg.sync_note}
									<div class="cfg-note" role="status">{cfg.sync_note}</div>
								{/if}
							{:else if cfg && !cfg.global_shortcuts_active}
								<div class="cfg-sc-warn" role="status">{$t('settings.global_shortcuts')}<b>{$t('settings.not_os_registered')}</b> (reines Wayland ohne Hyprland):
									die gespeicherten Kombinationen bleiben die kanonische Quelle; Compositor-Binds
									manuell nach docs/WAYLAND.md anlegen.
								</div>
							{:else if cfg}
								<div class="cfg-sc-ok" role="status">{$t('settings.global_registered_note')}</div>
							{/if}
						</div>
					</section>
					<!-- PB (t_1c9bc240, B1): Mode-Hotkeys — konfigurierbare F-Tasten der
					     In-App-Modi. Persistiert im Settings-Crate (TOML), Validierung:
					     nur F1–F12, F5 reserviert für den Resync, keine Doppelbelegung.
					     Capture-Muster identisch zu den globalen Kürzeln (C2a). -->
<section class="cfg-sect">
						<div class="h">{$t('settings.mode_hotkeys')}</div>
						{#each modeHotkeyFields as f (f.id)}
							<div class="cfg-sc-row" class:is-cap={hkCapture === f.id}>
								<div class="cfg-sc-meta">
									<div class="cfg-sc-lbl">{$t(f.label)}</div>
									<div class="cfg-sc-active">{$t('settings.active_label')}<b>{hotkeyFor(cfg, f.id)}</b>
									</div>
								</div>
								<button
									type="button"
									class="chip cfg-sc-capture"
									class:is-on={hkCapture === f.id}
									onclick={() => (hkCapture === f.id ? (hkCapture = null) : startModeHotkeyCapture(f.id))}
								>{hkCapture === f.id ? 'Abbrechen (Esc)' : $t('common.change')}</button>
							</div>
						{/each}
						{#if hkCapture}
							<div class="cfg-sc-help" role="status">
								F-Taste drücken (F1–F12) für „{modeHotkeyFields.find((f) => f.id === hkCapture)?.label ? $t(modeHotkeyFields.find((f) => f.id === hkCapture)!.label) : ''}“ — Esc bricht ab.
							</div>
						{/if}
						{#if hkNote}
							<div class="cfg-sc-err" role="alert">{hkNote}</div>
						{/if}
						<div class="cfg-sc-note">
							F5 ist fest für den Resync reserviert (Marktplatz, Mein Shop, Routen-Sweep)
							und kann keinem Modus zugewiesen werden. Änderungen gelten sofort.
							<button type="button" class="chip" onclick={() => void resetModeHotkeys()}>{$t('settings.default_mapping')}</button>
						</div>
					</section>
					</div>
					<!-- UD (t_a36daafb): thematische Gruppe „Daten & Dienste“ — reine UI-Umordnung -->
					<div class="cfg-group">
						<div class="h cfg-g-head">{$t('settings.data_services')}</div>
<section class="cfg-sect">
						<div class="h">{$t('settings.uex_api_key')}</div>
						<div class="cfg-key-status">{$t('settings.status')}<b class:ok={cfg?.uex_api_key_set} class:off={!cfg?.uex_api_key_set}>
								{cfg?.uex_api_key_set ? 'gesetzt' : 'nicht gesetzt'}
							</b>
						</div>
						<div class="cfg-key-row">
							<input
								type="password"
								autocomplete="off"
								placeholder={$t('settings.ph_paste_key')}
								bind:value={apiKeyInput}
								onkeydown={(e) => e.stopPropagation()}
							/>
							<button type="button" class="btn" onclick={saveApiKey} disabled={!apiKeyInput.trim()}>{$t('settings.set')}</button>
							{#if cfg?.uex_api_key_set}
							<button type="button" class="btn-ghost btn-danger" onclick={removeApiKey}>{$t('settings.remove')}</button>
							{/if}
						</div>
						<button
							type="button"
							class="chip"
							style="margin-top: 8px"
							title={$t('settings.manage_api_keys_uex')}
							onclick={() => void openUex('open_uex_api_apps')}
						>{$t('settings.manage_api_keys')}</button>
						{#if apiKeyNote}
							<div class="cfg-note">{apiKeyNote}</div>
						{/if}
					</section>
<section class="cfg-sect">
						<div class="h">{$t('settings.uex_secret_key')}</div>
						<div class="sm" style="margin-bottom: 8px; color: var(--fgdim)">
							Für den eigenen Shop („Mein Shop“, user-scoped Reads) braucht UEX
							zusätzlich deinen persönlichen Secret-Key (40 Zeichen, noch maskierter
							als der App-Key). Den Key auf uexcorp.space generieren/kopieren —
							den Benutzernamen ermittelt das Overlay automatisch daraus.
						</div>
						<div class="cfg-key-status">{$t('settings.secret_key')}<b class:ok={cfg?.uex_secret_key_set} class:off={!cfg?.uex_secret_key_set}>
								{cfg?.uex_secret_key_set ? 'gesetzt' : 'nicht gesetzt'}
							</b>
						</div>
						<div class="cfg-key-row">
							<input
								type="password"
								autocomplete="off"
								placeholder={$t('settings.ph_paste_secret')}
								bind:value={secretKeyInput}
								onkeydown={(e) => e.stopPropagation()}
							/>
							<button type="button" class="btn" onclick={saveSecretKey} disabled={!secretKeyInput.trim()}>{$t('settings.set')}</button>
							{#if cfg?.uex_secret_key_set}
							<button type="button" class="btn-ghost btn-danger" onclick={removeSecretKey}>{$t('settings.remove')}</button>
							{/if}
						</div>
						<button
							type="button"
							class="chip"
							style="margin-top: 8px"
							title={$t('settings.manage_secret_uex')}
							onclick={() => void openUex('open_uex_account')}
						>{$t('settings.manage_secret_key')}</button>
						{#if secretKeyNote}
							<div class="cfg-note">{secretKeyNote}</div>
						{/if}
						<!-- PW-1 (t_fbe40115): Username-Feld ENTFALLEN — der Name wird
						     automatisch aus dem Secret-Key ermittelt (GET /user) und
						     gecacht; hier nur noch Status statt Eingabefeld. -->
						<div class="cfg-key-status" style="margin-top: 10px">{$t('settings.uex_username')}<b class:ok={cfg?.uex_username?.trim()} class:off={!cfg?.uex_username?.trim()}>
								{cfg?.uex_username?.trim() ? cfg.uex_username.trim() : $t('settings.username_pending')}
							</b>
						</div>
						{#if usernameNote}
							<div class="cfg-note">{usernameNote}</div>
						{/if}
						<button type="button" class="chip" style="margin-top: 6px" onclick={() => void resolveUsername()}>{$t('settings.redetect_username')}</button>
					
												<div class="cfg-key-status" style="margin-top: 12px">{$t('settings.write_confirmation')}<button type="button" class="chip" class:is-on={cfg?.marketplace_confirm !== 'delete_only'} onclick={() => void setMarketplaceConfirm('every_action')}>{$t('settings.confirm_every_action')}</button>
							<button type="button" class="chip" class:is-on={cfg?.marketplace_confirm === 'delete_only'} onclick={() => void setMarketplaceConfirm('delete_only')}>{$t('settings.confirm_unlist_only')}</button>
						</div>
						<!-- MQ (t_d6ff5212): „Nur beim Abmelden“ war unklar → eindeutige
						     Labels + 1-Zeilen-Erklärung, was jeweils bestätigt wird. -->
						<div class="cfg-note">{$t('settings.confirm_note')}</div>
						{#if mktConfirmNote}
							<div class="cfg-note">{mktConfirmNote}</div>
						{/if}
				</section>
<section class="cfg-sect">
						<div class="h">{$t('settings.data_sources')}</div>
						<!-- PS (t_c84e545d): Item-Preise (patch-getrieben, kein
						     Auto-Sync) und Commodity-Preise (stündlich) zeigen
						     ihre Frische GETRENNT und lassen sich per Button
						     sofort erneuern. -->
						{#if priceSyncAge}
							<div class="cfg-note">
								Commodity-Preise: {rel(priceSyncAge.commodity_age_secs ?? 0)} sync ·
								{priceSyncAge.commodity_count} Waren · stündlich automatisch
							</div>
							<button
								type="button"
								class="chip"
								disabled={priceSyncing}
								onclick={refreshCommodityPrices}
							>{priceSyncing ? $t('common.syncing') : $t('settings.refresh_commodity_prices')}</button>
							<div class="cfg-note">
								Item-Preise: {rel(priceSyncAge.item_age_secs ?? 0)} sync ·
								{priceSyncAge.item_count} Items · pro Patch konstant (kein Auto-Sync)
							</div>
							<button
								type="button"
								class="chip"
								disabled={priceSyncing}
								onclick={refreshItemPrices}
							>{priceSyncing ? $t('common.syncing') : $t('settings.refresh_item_prices')}</button>
						{:else}
							<div class="cfg-note">{$t('settings.no_price_data_note')}</div>
						{/if}
					</section>
					<!-- S4 (t_66980b3c): Modul-Zuweisungen ausschließlich aus
				     module-overrides.json — kein game-versions-Trigger mehr;
				     der Button zieht NUR die Remote-Datei. -->
<section class="cfg-sect">
					<div class="h">{$t('settings.module_assignments')}</div>
					<div class="cfg-key-status">
						Datenstand:
						{#if modulStatus && modulStatus.version}
							<b class="ok">{modulStatus.version}</b>
							· {modulAgeText()} · {modulStatus.module_count} Module / {modulStatus.ship_count} Schiffe
						{:else}
							<b class="off">{$t('settings.no_sync_yet')}</b>
						{/if}
					</div>
					<div class="cfg-key-row">
						<button type="button" class="btn" disabled={modulBusy} onclick={() => void modulRefreshNow()}>
							{modulBusy ? $t('common.updating') : $t('settings.update_module_assignments')}
						</button>
					</div>
					{#if modulNote}
						<div class="cfg-note">{modulNote}</div>
					{/if}
					</section>
					</div>
					<!-- UD (t_a36daafb): thematische Gruppe „App“ — reine UI-Umordnung -->
					<div class="cfg-group">
						<div class="h cfg-g-head">{$t('settings.app')}</div>
					<!-- UC t_d323be7e: Auto-Updater — dezenter Update-Chip + on-demand
				     „Jetzt aktualisieren“ (nie auto-install ohne Wunsch). Bei
				     .deb-Installationen nur der apt-Hinweis (keine root-Aktionen
				     aus der App). -->
<section class="cfg-sect">
					<div class="h">{$t('settings.updater')}</div>
					<div class="cfg-chips">
						<button
							type="button"
							class="chip"
							class:is-on={cfg?.updater_enabled !== false}
							onclick={() => void setUpdaterEnabled(true)}
						>{$t('settings.auto_check_on')}</button>
						<button
							type="button"
							class="chip"
							class:is-on={cfg?.updater_enabled === false}
							onclick={() => void setUpdaterEnabled(false)}
						>{$t('settings.auto_check_off')}</button>
					</div>
					<div class="cfg-chips">
						{#each [2, 6, 12, 24] as h}
							<button
								type="button"
								class="chip"
								class:is-on={(cfg?.updater_interval_hours ?? 6) === h}
								onclick={() => void setUpdaterInterval(h)}
							>{h} h</button>
						{/each}
					</div>
					<div class="cfg-note">
						Overlay {upStatus?.app_version ?? '…'} · geprüft gegen die Gitea-Release (alle {cfg?.updater_interval_hours ?? 6} h, nur ein GET — kein Telemetrie).
					</div>
					{#if upStatus}
						{#if upStatus.available}
							<div class="cfg-key-status">
								<b class="ok">Update v{upStatus.available.version} verfügbar</b>
							</div>
						{:else if upStatus.note}
							<div class="cfg-note">{upStatus.note}</div>
						{/if}
						<div class="cfg-key-row">
							<button type="button" class="btn" disabled={upBusy} onclick={() => void updaterCheckNow()}>
								{upBusy ? $t('settings.checking') : $t('settings.check_now')}
							</button>
							{#if upStatus.available && upStatus.available.kind !== 'deb'}
								<button type="button" class="btn" disabled={upBusy} onclick={() => void updaterInstallNow()}>
									{upBusy ? $t('common.loading') : `Jetzt aktualisieren (v${upStatus.available.version})`}
								</button>
							{/if}
						</div>
						{#if upStatus.note && upStatus.available}
							<div class="cfg-note">{upStatus.note}</div>
						{/if}
					{/if}
					</section>
					</div>
				</div>
			</div>
		{:else if phase === 'loading'}
			<div class="pane-loading" role="status">
				<div class="center">
					<span class="spin"></span>
					<div class="big" style="margin-top: 12px">{$t('settings.initial_sync')}</div>
					<div class="sm">{$t('settings.initial_sync_note')}</div>
				</div>
			</div>

		{:else if phase === 'error'}
			<div class="pane-error">
				<div class="center">
					<div class="big" style="color: var(--alert)">{$t('settings.uex_unreachable')}</div>
					<div class="sm">
						{errorMsg ?? status?.last_error ?? $t('settings.no_connection')}<br />
						Cache-Stand: {cacheAge()}
					</div>
					<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={retrySync}>{$t('common.try_again')}</span>
					<div class="sm" style="margin-top: 10px">
						<b style="color: var(--fgdim)">O</b>{$t('settings.continue_offline')}</div>
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
					<button type="button" role="tab" aria-selected={mTab === 'offers'} class="chip" class:is-on={mTab === 'offers'} onclick={() => (mTab = 'offers')}>{$t('market.tab_offers')}</button>
					<!-- t_350c4300 B1: kein „Handelsrouten"-Tab im Marktplatz — Routen
					     sind nur noch über das Navigationsband (Rail) bzw. den
					     Mode-Hotkey für Routen (Default F3, konfigurierbar)
					     erreichbar (setMode('routes') → mTab='routes' bleibt der
					     interne Mechanismus). -->
					<button type="button" role="tab" aria-selected={mTab === 'shop'} class="chip" class:is-on={mTab === 'shop'} onclick={() => (mTab = 'shop')}>{$t('market.tab_shop')}</button>
					<!-- M3b (t_893761f1): Trades-Reiter — Deals + Chat im Overlay. -->
					<button type="button" role="tab" aria-selected={mTab === 'trades'} class="chip" class:is-on={mTab === 'trades'} onclick={() => (mTab = 'trades')}>{$t('market.tab_trades')}</button>
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
							<span><b>{$t('trades.seller')}</b> {offerDetail.seller || '—'}</span>
							<span><b>{$t('common.quantity')}</b> {fmtQty(offerDetail.quantity)}</span>
							<span><b>{$t('trades.location')}</b> {offerDetail.location || '—'}</span>
							<span><b>{$t('trades.age')}</b> {rel(offerDetail.updated)}</span>
							{#if offerDetail.expires}
								<span><b>{$t('trades.end')}</b> {rel(offerDetail.expires)}</span>
							{/if}
							{#if offerDetail.quality}
								<span><b>{$t('common.quality')}</b> {offerDetail.quality}</span>
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
											title={$t('market.image_zoom_title')}
											onclick={() => {
												const uri = offerPhotoUris[pi];
												if (uri && offerDetail) lightbox = { uri, title: offerDetail.title };
											}}
										>
											<img src={offerPhotoUris[pi]} alt={$t('market.image_n_of', { n: pi + 1, title: offerDetail!.title })} loading="lazy" />
										</button>
									{/if}
								{/each}
								{#if offerPhotoUris.length > 0 && offerPhotoUris.every((u) => u === null)}
									<div class="md-desc empty">{$t('shop.no_image')}</div>
								{/if}
							</div>
						{:else}
							<div class="md-desc empty">{$t('shop.no_image')}</div>
						{/if}
						{#if offerDetail.description}
							<div class="md-desc">{offerDetail.description}</div>
						{:else}
							<div class="md-desc empty">{$t('shop.no_description')}</div>
						{/if}
						<div class="md-actions">
							<!-- MQ (t_d6ff5212): Verhandlungs-Einstieg sichtbar machen. Die
							     API bietet KEIN POST /marketplace_negotiations (Research) —
							     Deals entstehen auf der UEX-Webseite → Deep-Link-Button. -->
							<span role="button" tabindex="0" onkeydown={keyActivate} class="btn" title={$t('trades.deal_start_title')} onclick={() => void openListingDeepLink(offerDetail!.slug)}>{$t('trades.negotiate_uex_caps')}</span>
							<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={openOfferDeepLink}>{$t('trades.open_uex_caps')}</span>
							<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={() => { offerDetail = null; lightbox = null; }}>{$t('common.back')}</span>
						</div>
						<div class="md-note">{$t('trades.negotiate_starts_on')}<span class="src">uexcorp.space/marketplace/item/info/{offerDetail.slug}</span>{$t('trades.negotiate_browser_note')}</div>
						{#if lightbox}
							<!-- MQ (t_d6ff5212): Bild-Lightbox — Inline-Zoom statt Browser.
							     Klick auf den Scrim (oder Esc) schließt; Klick aufs Bild nicht. -->
							<div class="lightbox-scrim" role="presentation" onclick={() => (lightbox = null)}>
								<div class="lightbox-modal" role="dialog" aria-modal="true" aria-label={$t('market.image_zoomed_aria')}>
									<!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_click_events_have_key_events --><!-- Lightbox-Bild: Klick stoppt nur Propagation (Schließen via Scrim/Esc), keine Interaktion -->
									<img src={lightbox.uri} alt={$t('market.zoomed_alt', { title: lightbox.title })} onclick={(ev) => ev.stopPropagation()} />
									<div class="lightbox-cap">{lightbox.title} — {$t('market.lightbox_close_hint')}</div>
								</div>
							</div>
						{/if}
					</div>
				{:else if mTab === 'routes'}
					<!-- PT (t_7fdc1317): Sub-Reiter innerhalb der Handelsrouten —
					     „Routen" (bestehende Matrix) und „Trade Log" (eigene
					     Commodity-Trades + Tagesgewinn). -->
					<div class="mbar2">
						<button type="button" role="tab" aria-selected={rTab === 'matrix'} class="chip" class:is-on={rTab === 'matrix'} onclick={() => (rTab = 'matrix')}>{$t('market.tab_routes')}</button>
						<button type="button" role="tab" aria-selected={rTab === 'tracking'} class="chip" class:is-on={rTab === 'tracking'} onclick={() => { rTab = 'tracking'; rDetail = null; if (!trkLoaded) void loadTrk(false); }}>{$t('market.tab_trade_log')}</button>
					</div>
					{#if rTab === 'tracking'}
						<div class="mbar">
							<span class="m-lbl">{$t('market.tab_trade_log')}</span>
							<span role="button" tabindex="0" onkeydown={keyActivate} class="chip" onclick={() => void loadTrk(true, true)}>{$t('common.refresh')}</span>
							<span role="button" tabindex="0" onkeydown={keyActivate} class="chip" title={$t('trades.form.title_long')} onclick={() => openTrkForm()}>{$t('trades.plus_record')}</span>
							<span role="button" tabindex="0" onkeydown={keyActivate} class="chip" title={$t('trades.open_trade_log_uex_title')} onclick={() => void openTrkUex()}>{$t('trades.trade_log_uex_link')}</span>
						</div>
						{#if trkLoading && !myTrades.length}
							<div class="center" style="padding: 28px 16px">
								<span class="spin"></span>
								<div class="big" style="margin-top: 12px">{$t('trades.loading')}</div>
							</div>
						{:else if trkErr && !myTrades.length}
							<div class="row-empty" style="padding: 28px 16px; color: var(--alert)">
								{trkErr}
								<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={() => void loadTrk(false)}>{$t('common.try_again')}</span>
							</div>
						{:else if myTrades.length === 0}
							{#if trkMsg}
								<div class="sm" style="color: {trkMsg === $t('trades.deleted_note') ? 'var(--fg)' : 'var(--alert)'}; margin: 6px 16px" role="alert">{trkMsg}</div>
							{/if}
							<div class="row-empty" style="padding: 28px 16px">
								{trkNote ?? tr('trades.no_commodity_trades')}
							</div>
						{:else}
							{#if trkNote}
								<div class="row-empty" style="padding: 10px 16px">{trkNote}</div>
							{/if}
							<!-- Tages-Aggregat: Ausgaben/Einnahmen/Nettogewinn (aUEC).
							     Q1 (t_535a98fb): Buchhaltungs-Konvention — positive Beträge
							     GRÜN (amt-pos), negative ROT (amt-neg). -->
							<div class="mlist">
								{#each trkDays as day (day.key)}
									<div class="mrow trow" style="cursor: default">
										<div class="m-main">
											<div class="m-title">{day.key}</div>
											<div class="m-sub">{day.count} Trade{day.count === 1 ? '' : 's'}</div>
										</div>
										<div class="m-right">
											<div class="m-meta">
												<span class="amt-neg" title={$t('trades.expenses_today_title')}>−{fmtPrice(day.expense)}</span>
												<span class="amt-pos" title={$t('trades.income_today_title')}>+{fmtPrice(day.income)}</span>
											</div>
											<div class="m-price {day.net >= 0 ? 'amt-pos' : 'amt-neg'}" title={$t('trades.net_profit_today_title')}>{day.net >= 0 ? '+' : '−'}{fmtPrice(Math.abs(day.net))}</div>
										</div>
									</div>
								{/each}
							</div>
							<!-- R1-Fix (t_a91c184f): trkMsg im Listen-Kontext rendern — Delete-Fehler
							     (deutsche UEX-Codes via friendly()) sind so im Trade-Log sichtbar,
							     nicht nur im geschlossenen Add/Edit-Formular. -->
							{#if trkMsg}
								<div class="sm" style="color: {trkMsg === $t('trades.deleted_note') ? 'var(--fg)' : 'var(--alert)'}; margin: 6px 16px" role="alert">{trkMsg}</div>
							{/if}
							<div class="mlist">
								{#each trkSorted as trk (trk.id)}
									<div class="mrow trow" style="cursor: default">
										<div class="m-op {trk.operation === 'sell' ? 'sell' : 'buy'}">{trk.operation === 'sell' ? 'Verkauf' : 'Kauf'}</div>
										<div class="m-main">
											<div class="m-title">{trk.commodity}</div>
											<div class="m-sub">{trk.terminal} · {fmt(trk.scu)} SCU @ {fmtPrice(trk.price)} aUEC</div>
										</div>
										<div class="m-right">
											<!-- Q1 (t_535a98fb): Gesamtwert = Menge × Preis — Verkauf
											     = Einnahme (GRÜN, +), Kauf = Ausgabe (ROT, −). -->
											<div
												class="m-price {trk.operation === 'sell' ? 'amt-pos' : 'amt-neg'}"
												title={$t('trades.total_value_title', { scu: fmt(trk.scu), price: fmtPrice(trk.price) })}
											>{trk.operation === 'sell' ? '+' : '−'}{fmtPrice((trk.scu || 0) * (trk.price || 0))}</div>
											<div class="m-meta">{new Date((trk.date_added || 0) * 1000).toLocaleString('de-DE')}</div>
											<div class="m-meta">
												<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" title={$t('trades.form.edit_title')} onclick={() => openTrkForm(trk)}>{$t('common.edit')}</span>
												<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" style="color: var(--alert)" title={$t('trades.form.delete_title')} onclick={() => requestTrkRemove(trk)}>{$t('common.delete')}</span>
											</div>
										</div>
									</div>
								{/each}
							</div>
							<div class="m-foot">
								<span>{myTrades.length} Trades · Tages-Aggregat clientseitig (Kauf = Ausgabe, Verkauf = Einnahme) · Quelle UEX /user_trades</span>
								<span class="src">UEX Corp</span>
							</div>
						{/if}
												{:else if rDetail}
						<div class="rdetail" role="dialog" aria-modal="true" aria-label={$t('routes.detail_title')}>
							<div class="md-head">
								<div class="md-t">
									<h2>{rDetail.commodity} {rDetail.illegal ? $t('market.illegal_chip') : ''}</h2>
									<span class="tag" style="color: var(--fgdim)">Distanz: {DIST_LABELS[rDetail.distance] ?? rDetail.distance}</span>
								</div>
								<div class="md-price">
									<div class="v sell">+{fmtPrice(rDetail.profit_total)}</div>
									<div class="u">aUEC Gewinn bei {routeMath(rDetail).qty} SCU</div>
								</div>
							</div>
							<div class="rd-leg">
								<div class="rd-stop">
									<div class="rd-cap">{$t('trades.buy_caps')}</div>
									<div class="rd-term">{rDetail.buy_terminal}</div>
									<div class="rd-chain">{rDetail.buy_chain}</div>
									<div class="rd-price">{fmtPrice(rDetail.buy_price)} aUEC / SCU</div>
									<div class="rd-flags">
										{#if rDetail.buy_terminal_autoload}<span class="chip">{$t('market.filter_autoload')}</span>{/if}
										{#if rDetail.buy_terminal_cargo_deck}<span class="chip">{$t('market.filter_cargodeck')}</span>{/if}
										{#if rDetail.buy_terminal_ground}<span class="chip">{$t('market.filter_ground')}</span>{:else}<span class="chip">{$t('market.filter_space')}</span>{/if}
									</div>
								</div>
								<div class="rd-arrow">→</div>
								<div class="rd-stop">
									<div class="rd-cap">{$t('trades.sell_caps')}</div>
									<div class="rd-term">{rDetail.sell_terminal}</div>
									<div class="rd-chain">{rDetail.sell_chain}</div>
									<div class="rd-price">{fmtPrice(rDetail.sell_price)} aUEC / SCU</div>
									<div class="rd-flags">
										{#if rDetail.sell_terminal_autoload}<span class="chip">{$t('market.filter_autoload')}</span>{/if}
										{#if rDetail.sell_terminal_cargo_deck}<span class="chip">{$t('market.filter_cargodeck')}</span>{/if}
										{#if rDetail.sell_terminal_ground}<span class="chip">{$t('market.filter_ground')}</span>{:else}<span class="chip">{$t('market.filter_space')}</span>{/if}
									</div>
								</div>
							</div>
							<div class="rd-math">
								<span><b>{$t('common.quantity')}</b> {fmtQty(routeMath(rDetail).qty)} SCU</span>
								<span><b>{$t('routes.purchase')}</b> {fmtPrice(routeMath(rDetail).cost)} aUEC</span>
								<span><b>{$t('routes.gross')}</b> {fmtPrice(routeMath(rDetail).gross)} aUEC</span>
								<span title={$t('routes.gain_formula_title')}><b>{$t('routes.net_profit')}</b> {fmtPrice(routeMath(rDetail).profit)} aUEC</span>
								<span title={$t('routes.margin_formula_title')}><b>{$t('routes.margin')}</b> {(rDetail.margin * 100).toFixed(1)} %</span>
								<span><b>{$t('routes.stock_there')}</b> {fmtPrice(rDetail.sell_stock)} SCU</span>
						</div>
						{#if rLocations && (rBuySorted.length > 0 || rSellSorted.length > 0)}
												<div class="rd-pair">
													<div class="rd-pair-col">
														<div class="rd-pair-head">{$t('routes.buy_locations')}<span class="rd-pair-sub">{$t('routes.price_up')}</span></div>
														{#if rBuySorted.length === 0}
															<div class="rd-pair-empty">{$t('routes.no_buy_locations')}</div>
														{:else}
															{#each rBuySorted as loc (loc.terminal_id)}
																<button type="button" class="rd-pair-row" data-side="buy" data-term-id={loc.terminal_id}
																	class:is-selected={loc.terminal_id === rDetail?.buy_terminal_id}
																	title={loc.terminal_id === rDetail?.buy_terminal_id ? $t('routes.current_buy_terminal_title') : $t('routes.set_buy_terminal_filter')}
																	onclick={() => setRouteLocField('from', loc.terminal)}>
																	<span class="rd-pair-main">
																		<span class="rd-pair-term">{loc.terminal}</span>
																		<span class="rd-pair-chain">{loc.chain}</span>
																	</span>
																	<span class="rd-pair-right">
																		<span class="rd-pair-price">{fmtPrice(loc.price)}</span>
																		<span class="rd-pair-meta">
																			<span class="r-chip" title={$t('market.stock_at_terminal_title')}>≋ {fmt(Math.round(loc.stock))} SCU</span>
																			{#if loc.status > 0}<span class="r-chip" title={$t('market.demand_title')}>{loc.status}/7</span>{/if}
																			{#if loc.autoload}<span class="r-chip">{$t('market.filter_autoload')}</span>{/if}
																			{#if loc.updated > 0}<span class="r-chip r-chip-age" class:stale={rLocStale(loc)} title="Preisstand (UEX date_modified): {new Date(loc.updated * 1000).toLocaleString('de-DE')}">⏱ {rel(loc.updated)}</span>{/if}
																			</span>
																			</span>
																			</button>
																			{/each}
																			{/if}
																			</div>
																			<div class="rd-pair-col">
														<div class="rd-pair-head">{$t('routes.sell_locations')}<span class="rd-pair-sub">{$t('routes.price_down')}</span></div>
														{#if rSellSorted.length === 0}
															<div class="rd-pair-empty">{$t('routes.no_sell_locations')}</div>
														{:else}
															{#each rSellSorted as loc (loc.terminal_id)}
																<button type="button" class="rd-pair-row" data-side="sell" data-term-id={loc.terminal_id}
																	class:is-selected={loc.terminal_id === rDetail?.sell_terminal_id}
																	title={loc.terminal_id === rDetail?.sell_terminal_id ? $t('routes.current_sell_terminal_title') : $t('routes.set_sell_terminal_filter')}
																	onclick={() => setRouteLocField('to', loc.terminal)}>
																	<span class="rd-pair-main">
																		<span class="rd-pair-term">{loc.terminal}</span>
																		<span class="rd-pair-chain">{loc.chain}</span>
																	</span>
																	<span class="rd-pair-right">
																		<span class="rd-pair-price sell">+{fmtPrice(loc.price)}</span>
																		<span class="rd-pair-meta">
																			<span class="r-chip" title={$t('market.stock_at_terminal_title')}>≋ {fmt(Math.round(loc.stock))} SCU</span>
																			{#if loc.status > 0}<span class="r-chip" title={$t('market.demand_title')}>{loc.status}/7</span>{/if}
																			{#if loc.autoload}<span class="r-chip">{$t('market.filter_autoload')}</span>{/if}
																			{#if loc.updated > 0}<span class="r-chip r-chip-age" class:stale={rLocStale(loc)} title="Preisstand (UEX date_modified): {new Date(loc.updated * 1000).toLocaleString('de-DE')}">⏱ {rel(loc.updated)}</span>{/if}
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
								<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={() => (rDetail = null)}>{$t('common.back_esc')}</span>
							</div>
						</div>
					{:else}
						<div class="mbar2">
							<span class="m-lbl">{$t('routes.ship')}</span>
							<RoutesTypeahead bind:value={rShipQ} options={shipTypeaheadOpts} placeholder={$t('market.ph_eg_ironclad')} ariaLabel={$t('routes.ph_ship_typeahead')} onselect={(v: string) => applyShipScu(v)} />
							<input class="qnum" type="number" min="0" placeholder="SCU" aria-label={$t('routes.scu_capacity_aria')} bind:value={rScu} />
							<span class="m-lbl">{$t('routes.budget')}</span>
							<div class="rb-wrap">
								<input class="qnum rb-qnum" type="number" min="0" step="any" placeholder="aUEC" aria-label={$t('routes.budget_title')} bind:value={rBudget} />
								{#if rBudgetNum() !== null && rBudgetNum()! > 0}
									<span class="rb-fmt" title={$t('routes.budget_formatted_title')}>{fmtPrice(rBudgetNum()!)}</span>
								{/if}
							</div>
							<span class="m-lbl">{$t('common.commodity')}</span>
							<input class="rtext" placeholder={$t('common.commodity')} aria-label={$t('routes.filter_commodity_title')} bind:value={rCommodity} />
							<span class="m-lbl">{$t('routes.start')}</span>
							<RoutesTypeahead bind:value={rFrom} options={fromOpts} placeholder={$t('routes.origin')} ariaLabel={$t('routes.ph_origin_typeahead')} />
							<span class="m-lbl">{$t('routes.destination_short')}</span>
							<RoutesTypeahead bind:value={rTo} options={toOpts} placeholder={$t('routes.destination')} ariaLabel={$t('routes.ph_destination_typeahead')} />
							<a class="rta-clear" href="#0" role="button" tabindex="0" title={$t('routes.reset_filter_title')} onclick={(ev) => { ev.preventDefault(); rShipQ = ''; rScu = 0; rCommodity = ''; rFrom = ''; rTo = ''; rBudget = ''; }}>{$t('routes.filter_chip')}</a>
						</div>
						<div class="mbar2">
							<button type="button" class="chip" class:is-on={rLegal === 'all'} onclick={() => (rLegal = 'all')}>{$t('common.all')}</button>
							<button type="button" class="chip" class:is-on={rLegal === 'legal'} onclick={() => (rLegal = 'legal')}>{$t('routes.legal')}</button>
							<button type="button" class="chip" class:is-on={rLegal === 'illegal'} onclick={() => (rLegal = 'illegal')}><span class="dot"></span>{$t('routes.illegal')}</button>
							<span class="m-lbl">{$t('market.filter_autoload')}</span>
							<button type="button" class="chip" class:is-on={rAutoload === 'any'} onclick={() => (rAutoload = 'any')}>{$t('routes.any')}</button>
							<button type="button" class="chip" class:is-on={rAutoload === 'yes'} onclick={() => (rAutoload = 'yes')}>{$t('common.yes')}</button>
							<button type="button" class="chip" class:is-on={rAutoload === 'no'} onclick={() => (rAutoload = 'no')}>{$t('common.no')}</button>
							<span class="m-lbl">{$t('market.filter_cargodeck')}</span>
							<button type="button" class="chip" class:is-on={rCargo === 'any'} onclick={() => (rCargo = 'any')}>{$t('routes.any')}</button>
							<button type="button" class="chip" class:is-on={rCargo === 'yes'} onclick={() => (rCargo = 'yes')}>{$t('common.yes')}</button>
							<button type="button" class="chip" class:is-on={rCargo === 'no'} onclick={() => (rCargo = 'no')}>{$t('common.no')}</button>
							<span class="m-lbl">{$t('trades.location')}</span>
							<button type="button" class="chip" class:is-on={rPlace === 'any'} onclick={() => (rPlace = 'any')}>{$t('common.all')}</button>
							<button type="button" class="chip" class:is-on={rPlace === 'ground'} onclick={() => (rPlace = 'ground')}>{$t('market.filter_ground')}</button>
							<button type="button" class="chip" class:is-on={rPlace === 'space'} onclick={() => (rPlace = 'space')}>{$t('market.filter_space')}</button>
						</div>
						<div class="mbar2">
							<span class="m-lbl">{$t('routes.sort')}</span>
							<button
								type="button"
								class="chip"
								class:is-on={rSort === 'profit_desc' || rSort === 'profit_asc'}
								onclick={cycleProfitSort}
								title={$t('routes.profit_sort_toggle_title')}
							>Gewinn/SCU{rSort === 'profit_desc' ? ' ↓' : rSort === 'profit_asc' ? ' ↑' : ''}</button>
							<button type="button" class="chip" class:is-on={rSort === 'margin_desc'} onclick={() => (rSort = 'margin_desc')}>{$t('routes.margin_down')}</button>
							<button type="button" class="chip" class:is-on={rSort === 'distance_asc'} onclick={() => (rSort = 'distance_asc')}>{$t('routes.distance_up')}</button>
							<span class="m-spacer"></span>
							{#if rSweptAt}<span class="chip"><span class="dot"></span>Stand {new Date(rSweptAt * 1000).toLocaleTimeString('de-DE')}</span>{/if}
							{#if rRefreshing}<span class="chip">↻ Updating…</span>{/if}
						</div>
						{#if rLoading && !routes_.length}
							<div class="center" style="padding: 28px 16px">
								<span class="spin"></span>
								<div class="big" style="margin-top: 12px">{$t('routes.sweeping')}</div>
								<div class="sm">{$t('routes.first_run_note')}</div>
							</div>
						{:else if rNote && !routes_.length}
							<div class="row-empty" style="padding: 28px 16px">{rNote}</div>
						{:else if routes_.length}
							<div class="mlist">
								{#each routes_ as r, i (r.commodity_id + '-' + r.buy_terminal_id + '-' + r.sell_terminal_id)}
									<div tabindex="-1" onkeydown={keyActivate}
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
											<div class="r-chip" title={$t('market.uex_enrichment_title')}>
												<span class="r-chip-vol">≋ {r.enrichment.scu_reachable > 0 ? fmt(Math.round(r.enrichment.scu_reachable)) : fmt(Math.round(r.enrichment.scu_destination))} SCU</span>
												{#if r.enrichment.status_destination > 0}
												<span class="r-chip-stock" title={$t('market.demand_sell_title')}>{r.enrichment.status_destination}</span>
												{/if}
											</div>
										{/if}
										{#if rBudgetActive}
										<div class="r-chip r-chip-menge" title={$t('routes.rb2_budget_title')}>Menge {fmtQty(r.quantity)} SCU</div>
										{/if}
									</div>
									<div class="m-right">
										<div class="m-price sell" title={$t('routes.gain_scu_formula_title')}>+{fmtPrice(r.profit_per_scu)}</div>
										<div class="m-meta">{fmtPrice(r.profit_total)} aUEC · <span title={$t('routes.margin_relative_title')}>Marge {(r.margin * 100).toFixed(0)} %</span></div>
										{#if rBudgetActive && r.quantity <= 0}<div class="r-hint">{$t('routes.budget_too_low')}</div>{/if}
									</div>
									</div>
								{/each}
							</div>
							<div class="m-foot">
								<span>{routes_.length} Routen · Gain/SCU = Sell − Buy (absolute aUEC/SCU) · Marge = relative % (Gain/SCU ÷ Buy)</span>
								<span class="src">UEX Corp</span>
							</div>
						{:else}
							<div class="row-empty" style="padding: 28px 16px">{$t('routes.no_routes')}</div>
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
							<span role="button" tabindex="0" onkeydown={keyActivate} class="chip" onclick={() => (tradesDeal = null)}>{$t('trades.back_to_deals')}</span>
							<span class="m-lbl" style="flex:1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap">
								{tradesDeal.listing_title}
							</span>
							<span role="button" tabindex="0" onkeydown={keyActivate} class="chip" title={$t('trades.open_deal_uex_title')} onclick={() => void openTradesDeal(tradesDeal!)}>{$t('trades.deal_open_uex_link')}</span>
							<!-- QW-4 (t_28bfd027): geschlossene Deals sind READ-ONLY. -->
							{#if tradesDealEnded}<span class="chip warn"><span class="dot"></span>{tradesDeal.date_closed && tradesDeal.date_closed_client ? $t('trades.status_closed') : $t('trades.status_ended')}</span>{/if}
						</div>
						<div class="chat" bind:this={chatScroller}>
							{#if chatLoading}
								<div class="center" style="padding: 20px 0"><span class="spin"></span><div class="big" style="margin-top: 10px">{$t('trades.loading_history')}</div></div>
							{:else if chatMsgs.length === 0}
								<div class="row-empty" style="padding: 14px 16px">{$t('trades.no_messages')}</div>
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
								<button type="button" class="btn-ghost" style="color: var(--alert)" onclick={() => void openTradesDeal(tradesDeal!)}>{$t('trades.open_deal_caps')}</button>
							</div>
						{/if}
						<!-- QW-4 (t_28bfd027): abgeschlossener Deal (date_closed von
					     einer oder beiden Seiten gesetzt) → KEIN Composer/Senden,
					     nur lesbarer Verlauf. Backend-Gate (trade.rs) lehnt den
					     Write defensiv ebenfalls ab. -->
					{#if !tradesDealEnded}
					<div class="chat-input">
							<input
								class="rtext"
								placeholder="Nachricht an {tradesDeal.is_listing_advertiser ? (tradesDeal.client_username || $t('trades.buyer')) : (tradesDeal.advertiser_username || 'Anbieter')} …"
								bind:value={chatDraft}
								onkeydown={chatKey}
								maxlength={65535}
								aria-label={$t('trades.send_chat_title')}
							/>
							<button type="button" class="btn" disabled={chatBusy || !chatDraft.trim()} onclick={() => void sendChat()}>
								{chatBusy ? $t('common.sending') : 'Senden'}
							</button>
							</div>
							{:else}
							<!-- Read-only-Fuß: Verlauf bleibt sichtbar, Senden entfällt.
							     QY: auch einseitig beendet (nur eine Seite geschlossen). -->
							<div class="mbar">
								<span class="chip warn"><span class="dot"></span>{tradesDeal.date_closed && tradesDeal.date_closed_client ? $t('trades.completed') : $t('trades.ended')} — Verlauf nur lesbar</span>
								<!-- Q1 (t_535a98fb): unterer zweiter Button ENTFIELT — der obere
								     „Deal auf UEX öffnen" bleibt immer sichtbar und zielt auf
								     den Deal (negotiate/hash), nicht auf das Listing. -->
							</div>
							{/if}
					{:else}
						<div class="mbar">
							<span class="m-lbl">{$t('trades.deals')}</span>
							<span role="button" tabindex="0" onkeydown={keyActivate} class="chip" onclick={() => void loadDeals(true)}>{$t('common.refresh')}</span>
							<span role="button" tabindex="0" onkeydown={keyActivate} class="chip" title={$t('trades.open_trades_manager_title')} onclick={openMyTradesDeepLink}>{$t('shop.manage_listings_link')}</span>
						</div>
						<!-- MQ (t_d6ff5212): Deal-Einstieg sichtbar machen — Deals KANN das
						     Overlay nicht starten (API hat kein POST /marketplace_negotiations).
						     Sie entstehen auf uexcorp.space; hier nur verhandeln/chatten/beenden. -->
						<div class="row-empty" style="padding: 8px 16px; text-align: left">{$t('shop.deals_created_on')}<span class="src">uexcorp.space</span>{$t('shop.deal_start_note')}</div>
						{#if tradesLoading}
							<div class="center">
								<span class="spin"></span>
								<div class="big" style="margin-top: 12px">{$t('trades.loading')}</div>
							</div>
						{:else if tradesErr && !myNegotiations.length}
							<div class="pane-error">
								<div class="center">
									<div class="big" style="color: var(--alert)">{$t('trades.unreachable')}</div>
									<div class="sm">{tradesErr}</div>
									<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={() => void loadDeals(false)}>{$t('common.try_again')}</span>
									<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={openMyShopDeepLink}>{$t('trades.open_uex_caps')}</span>
								</div>
							</div>
						{:else if dealsSorted.length === 0}
							<div class="row-empty" style="padding: 28px 16px">
								{tradesNote ?? $t('trades.no_deals_note')}
							</div>
						{:else}
							{#if tradesNote}
								<div class="row-empty" style="padding: 10px 16px">{tradesNote}</div>
							{/if}
							<div class="mlist">
								{#each dealsSorted as n (n.id)}
									<div tabindex="-1" onkeydown={keyActivate}
										class="mrow trow"
										role="option"
										aria-selected={false}
										title={$t('trades.open_deal_chat_title')}
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
											<div class="m-meta">{dealStatus(n)}{#if dealUnread(n)}<span class="chat-dot" title={$t('trades.new_interaction_title')}></span>{/if}</div>
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
						<!-- PW-1 (t_fbe40115): Username-Feld entfallen — der Name
						     kommt automatisch aus dem Secret-Key (Settings-Cache). -->
						<span class="m-lbl">{$t('market.tab_shop')}</span>
						<span class="chip">{shopUser.trim() || $t('settings.username_detecting')}</span>
						<span role="button" tabindex="0" onkeydown={keyActivate}
							class="btn-ghost"
							title={$t('shop.reload_offers_title')}
							onclick={() => void loadShop()}
						>{$t('common.load')}</span>
						<span class="m-spacer"></span>
						{#if shopLoading}<span class="chip">↻ …</span>{/if}
						{#if shopSource === 'offline'}
							<span class="chip warn"><span class="dot"></span>{$t('shop.offline_snapshot')}</span>
						{:else if shopSource === 'cache'}
							<span class="chip"><span class="dot"></span>{$t('shop.cache')}</span>
						{/if}
					</div>
					{#if !cfg?.uex_api_key_set}
						<div class="pane-error" style="margin: 10px 0">
							<div class="center">
								<div class="big" style="color: var(--alert)">{$t('shop.no_api_key')}</div>
								<div class="sm">{$t('shop.need_api_key_note')}</div>
								<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={openConfig}>{$t('shop.set_in_settings')}</span>
							</div>
						</div>
					{:else if !cfg?.uex_secret_key_set}
						<!-- M2a-Fix (t_2f197d1c): ohne persönlichen Secret-Key KEIN
						     Request (UEX antwortet sonst nur 400 user_not_found /
						     403 invalid_secret_key) — stattdessen klare deutsche
						     Meldung + Settings-Verweis. -->
						<div class="pane-error" style="margin: 10px 0">
							<div class="center">
								<div class="big" style="color: var(--alert)">{$t('shop.secret_missing')}</div>
								<div class="sm">{$t('shop.user_scoped_note')}</div>
								<button type="button" class="btn-ghost" onclick={openConfig}>{$t('shop.set_in_settings')}</button>
							</div>
						</div>
					{:else if shopLoading}
						<div class="center">
							<span class="spin"></span>
							<div class="big" style="margin-top: 12px">{$t('shop.loading')}</div>
							<div class="sm">{$t('shop.from_uex_client')}</div>
						</div>
					{:else if shopErr && !myOffers.length && !myNegotiations.length}
						<div class="pane-error">
							<div class="center">
								<div class="big" style="color: var(--alert)">{$t('shop.unreachable')}</div>
								<div class="sm">{shopErr}</div>
								<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={() => void loadShop()}>{$t('common.try_again')}</span>
								<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={openMyShopDeepLink}>{$t('trades.open_uex_caps')}</span>
							</div>
						</div>
					{:else}
						{#if shopErr}
							<div class="row-empty" style="padding: 10px 16px; color: var(--alert); display: flex; align-items: center; gap: 12px" role="alert">
								<span style="flex: 1">{shopErr}</span>
								<!-- M2b-3: Fehlerpfad — Meldung + Deep-Link-Button (passende
								     UEX-Seite: Marketplace-Home, dort weitermachen). Kein Retry. -->
								<button type="button" class="btn-ghost" style="color: var(--alert)" onclick={() => void openListingDeepLink(null)}>{$t('trades.open_uex_caps')}</button>
							</div>
						{/if}
						{#if shopNote}
												<div class="row-empty" style="padding: 10px 16px; display: flex; align-items: center; gap: 12px">
													<span style="flex: 1">{shopNote}</span>
													{#if shopCreatedUrl}
														<!-- M2b-Review-Fix (t_6e26ec82 R2): Erfolgs-Deep-Link aus der
														     Create-Antwort (Listings erscheinen erst nach UEX-Approval —
														     der Pull kann sie nicht zeigen, der Direct-Link schon). -->
														<button type="button" class="btn-ghost" onclick={() => void openCreatedListing(shopCreatedUrl!)}>{$t('shop.open_created_caps')}</button>
													{/if}
												</div>
											{/if}
											<div class="mbar2" style="margin-top: 10px">
													<span class="m-lbl">Eigene Offers ({myOffers.length}{shopPending.length ? ` + ${shopPending.length} wartend` : ''})</span>
															<!-- QZ (t_ab10109f): Filter Alle/Aktiv/Wartend ENTFERNT — UEX
															     kann wartende Listings nicht per API liefern; Wartende
															     kommen lokal (pending_offers) und sind immer sichtbar. -->
															<span class="m-spacer"></span>
															<span role="button" tabindex="0" onkeydown={keyActivate} class="chip" title={$t('shop.create_new_note_title')} onclick={() => void openAdvertise(null)}>{$t('shop.create')}</span>
															<span role="button" tabindex="0" onkeydown={keyActivate} class="chip" title={$t('shop.open_listings_manager_title')} onclick={openMyShopDeepLink}>{$t('shop.manage_listings')}</span>
												</div>
												{#if advertOpen}
												<!-- M3a: das „Neues Listing“-Formular als modales Popup
												     (gleiches Muster wie die 2-Schritt-Bestätigung: Scrim +
												     feste Karte). Passt in die Overlay-Größe, mlist-Body
												     scrollt intern, Button-Leiste bleibt erreichbar. -->
													<div class="advert-scrim" role="presentation">
														<div class="advert-modal" role="dialog" aria-modal="true" aria-label={$t('shop.new_listing_title')}>
													<div class="pane-error advert-pane" style="background: rgba(255,255,255,.03)">
														<div class="mbar2"><span class="m-lbl">{editingOffer ? $t('shop.recreate_title') : $t('shop.new_listing_caps')}</span></div>
														{#if editingOffer}
															<div class="row-empty" style="padding: 8px 12px">Dieses Formular ist VORBEFÜLLT mit „{editingOffer.title}“. Für eine Änderung musst du das alte Listing zuerst ABmelden (unten) und dann mit den neuen Werten neu anlegen — UEX unterstützt kein Bearbeiten.</div>
														{/if}
														<div class="mlist" style="padding: 6px 14px">
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px">
																<span class="m-lbl">{$t('shop.kind')}</span>
																<button type="button" class="chip" class:is-on={advertForm.operation === 'sell'} onclick={() => (advertForm.operation = 'sell')}>{$t('shop.sell_type')}</button>
																<button type="button" class="chip" class:is-on={advertForm.operation === 'buy'} onclick={() => (advertForm.operation = 'buy')}>{$t('shop.buy_type')}</button>
																<span class="m-lbl">{$t('shop.listing_type')}</span>
																<SelectCombo
																	bind:value={advertForm.listing_type}
																	ariaLabel={$t('shop.listing_type')}
																	placeholder={$t('common.choose_placeholder')}
																	options={[
																		{ value: 'item', label: $t('shop.item') },
																		{ value: 'service', label: $t('shop.opt_service') },
																		{ value: 'contract', label: $t('shop.opt_contract') }
																	]}
																	onselect={() => {
																		// Unit-Referenzliste wechselt mit dem Type (Doku [7]);
																		// TB (t_78a4a5e5, Punkt 1): NIE automatisch vorwählen —
																		// Unit wird geleert, wenn sie zur neuen Liste nicht passt
																		// (der Nutzer wählt selbst; Validierung verlangt die Wahl).
																		const u = unitsFor(advertForm.listing_type);
																		if (!u.includes(advertForm.unit)) advertForm.unit = '';
																	}}
																	/>
																	{#if advertErrors.listing_type}<span class="chip warn" style="margin-left: 4px">{advertErrors.listing_type}</span>{/if}
																	</div>
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px">
																<span class="m-lbl">{$t('common.title')}</span>
																				<!-- NN (t_060d134e): breit genug für lange Titel (z. B.
																				     „Atzkav Sniper Rifle 970 Q Crafted …“) ohne Abschneiden. -->
																				<input class="rtext" style="width: 560px" placeholder={$t('shop.item_name_title')} maxlength="140" bind:value={advertForm.title} />
																				{#if advertErrors.title}<span class="chip warn" style="margin-left: 4px">{advertErrors.title}</span>{/if}
																				<span class="m-lbl">{$t('common.price')}</span>
																				<!-- NN (t_060d134e): breit für einstellige Milliarden +
																				     Tausender-Punkte bei der Eingabe (gespeichert wird die
																				     reine Ziffernfolge — Validierung/Parse unverändert). -->
																				<input
																					class="qnum"
																					style="width: 170px"
																					type="text"
																					inputmode="numeric"
																					placeholder={$t('shop.ph_eg_price')}
																					aria-label={$t('shop.price_hint_title')}
																					value={fmtGroup(advertForm.price)}
																					oninput={onPriceInput}
																				/>
																{#if advertErrors.price}<span class="chip warn" style="margin-left: 4px">{advertErrors.price}</span>{/if}
																<span class="m-lbl">{$t('shop.currency')}</span>
																<SelectCombo bind:value={advertForm.currency} ariaLabel={$t('shop.currency')} placeholder={$t('shop.ph_choose_currency')} options={CURRENCY_CHOICES.map((c) => ({ value: c, label: c }))} />
																{#if advertErrors.currency}<span class="chip warn" style="margin-left: 4px">{advertErrors.currency}</span>{/if}
															</div>
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px">
																<span class="m-lbl">{$t('common.quantity')}</span>
																<input class="qnum" type="number" min="1" step="1" bind:value={advertForm.quantity} />
																{#if advertErrors.quantity}<span class="chip warn" style="margin-left: 4px">{advertErrors.quantity}</span>{/if}
																<span class="m-lbl">{$t('shop.unit')}</span>
																<SelectCombo bind:value={advertForm.unit} ariaLabel={$t('shop.unit')} placeholder={$t('shop.ph_choose_unit')} options={unitsFor(advertForm.listing_type).map((u) => ({ value: u, label: u }))} />
																{#if advertErrors.unit}<span class="chip warn" style="margin-left: 4px">{advertErrors.unit}</span>{/if}
																<span class="m-lbl">{$t('trades.location')}</span>
																<div style="position: relative; display: inline-flex">
																	<RoutesTypeahead
																		bind:value={advertForm.location}
																		options={advertLocOpts}
																		placeholder={$t('shop.ph_eg_location')}
																		ariaLabel={$t('shop.ph_location_typeahead')}
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
																<span class="m-lbl">{$t('shop.category')}</span>
																<SelectCombo
																	bind:value={advertForm.category}
																	ariaLabel={$t('shop.category')}
																	placeholder={$t('common.choose_placeholder')}
																	options={[
																		{ value: '', label: $t('common.choose_placeholder') },
																		...advertCategories.map((c) => ({
																			value: String(c.id),
																			label: (c.section ? c.section + ' › ' : '') + c.name
																		}))
																	]}
																/>
																{#if advertErrors.category}<span class="chip warn" style="margin-left: 4px">{advertErrors.category}</span>{/if}
																{#if advertForm.listing_type === 'item'}
																	<span class="m-lbl">{$t('shop.item')}</span>
																	<div style="position: relative; display: inline-flex">
																		<RoutesTypeahead
																			bind:value={advertForm.item_label}
																			options={advertItemOpts}
																			placeholder={$t('shop.ph_search_item')}
																			ariaLabel={$t('shop.ph_item_typeahead')}
																			onselect={(v: string) => {
																				advertForm.item_id = v;
																				advertErrors.item = undefined;
																			}}
																		/>
																	</div>
																	{#if advertForm.item_id}
																		<span class="chip" title={$t('shop.item_selected_title')}>✓ {advertForm.item_label}</span>
																	{/if}
																	{#if advertErrors.item}<span class="chip warn" style="margin-left: 4px">{advertErrors.item}</span>{/if}
																{/if}
															</div>
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px">
																<span class="m-lbl">{$t('shop.duration')}</span>
																<SelectCombo
																	bind:value={advertForm.hours}
																	ariaLabel={$t('shop.duration')}
																	placeholder={$t('shop.ph_choose_duration')}
																	options={HOUR_CHOICES.map((h) => ({ value: h, label: hourLabel(h) }))}
																/>
																{#if advertErrors.hours}<span class="chip warn" style="margin-left: 4px">{advertErrors.hours}</span>{/if}
																<span class="m-lbl">{$t('shop.item_source')}</span>
																<SelectCombo
																	bind:value={advertForm.source}
																	ariaLabel={$t('shop.item_source')}
																	placeholder={$t('common.ph_optional')}
																	options={[
																		{ value: '', label: $t('common.ph_optional') },
																		...SOURCE_CHOICES.map((s) => ({ value: s, label: s.replace(/_/g, ' ') }))
																	]}
																/>
																<span class="m-lbl">{$t('shop.quality')}</span>
																<!-- NN (t_060d134e): Quality 0–1000 (war 0–100) — UEX-Maske
												     erlaubt bis 1000, z. B. „970 Q Crafted“. -->
								<input class="qnum" type="number" min="0" max="1000" step="1" placeholder={$t('common.range_0_1000')} title={$t('shop.quality_title')} bind:value={advertForm.quality} />
																{#if advertErrors.quality}<span class="chip warn" style="margin-left: 4px">{advertErrors.quality}</span>{/if}
																<span class="m-lbl">{$t('shop.availability')}</span>
																<SelectCombo
																	bind:value={advertForm.availability}
																	ariaLabel={$t('shop.availability')}
																	placeholder={$t('common.ph_optional')}
																	options={[
																		{ value: '', label: $t('common.ph_optional') },
																		...AVAILABILITY_CHOICES.map((a) => ({ value: a, label: a.replace(/_/g, ' ') }))
																	]}
																/>
															</div>
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px">
																<span class="m-lbl">{$t('shop.hide_listing')}</span>
																<button
																	type="button"
																	class="chip"
																	class:is-on={advertForm.hidden}
																	onclick={() => (advertForm.hidden = !advertForm.hidden)}
																	title={$t('shop.hidden_listing_title')}
																>{advertForm.hidden ? $t('common.yes') : $t('common.no')}</button>
															</div>
															<div class="mbar2 advert-desc-row" style="flex-wrap: wrap; gap: 6px">
																<span class="m-lbl" style="align-self: flex-start; padding-top: 6px">{$t('shop.description')}</span>
																<!-- MQ (t_d6ff5212): mehrzeiliges Textfeld. API-Beleg
																     [POST /marketplace_advertise Doku]: description string(65535),
																     Newlines werden nicht eingeschränkt → \n wird 1:1 durchgereicht
																     (Rust String, JSON-Encoder escaped sauber).
																     QZ (t_ab10109f): flex-growt in den freien Popup-Raum
																     (Höhe aus dem Flex-Layout, kein Autogrow-oninput mehr). -->
																<textarea
																	class="rtext advert-desc"
																	rows="3"
																	placeholder={$t('shop.description_hint_title')}
																	maxlength="65535"
																	bind:value={advertForm.description}
																></textarea>
																{#if advertErrors.description}<span class="chip warn" style="margin-left: 4px">{advertErrors.description}</span>{/if}
															</div>
															<div class="mbar2" style="flex-wrap: wrap; gap: 6px; align-items: center">
																<span class="m-lbl">{$t('shop.image_label')}</span>
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
																			title={$t('shop.remove_image_title')}
																			onclick={() => (advertForm.images = advertForm.images.filter((_, k) => k !== i))}
																		>✕</button>
																	</span>
																{/each}
																{#if advertForm.images.length}
																	<span class="sm" style="opacity: .6">{$t('shop.image_note')}</span>
																{/if}
																{#if advertErrors.images}<span class="chip warn" style="margin-left: 4px">{advertErrors.images}</span>{/if}
															</div>
															<div class="mbar2 advert-foot" style="margin-top: 8px">
																<button type="button" class="btn" disabled={writeBusy} onclick={() => void requestAdvertise()}>{writeBusy ? $t('shop.sending') : $t('shop.create_2step')}</button>
																<button type="button" class="btn-ghost" disabled={writeBusy} onclick={() => requestCloseAdvert()}>{$t('common.cancel')}</button>
																{#if advertNote}<span class="chip warn" style="margin-left: 8px">{advertNote}</span>{/if}
															</div>
																												</div>
																											</div>
																											<!-- M3a: Ungespeicherte Eingaben → kurze Verwerfen-Frage
														     statt verlustfreiem Wegwerfen. -->
														{#if advertDiscard}
															<div class="advert-discard">
																<div class="big" style="color: var(--alert)">{$t('shop.discard_confirm')}</div>
																<div class="sm" style="color: var(--fgdim)">{$t('shop.nothing_sent')}</div>
																<div class="mbar2" style="margin-top: 10px; justify-content: center; border: 0; background: transparent; padding: 0">
																	<button type="button" class="btn btn-danger" onclick={() => { advertOpen = false; advertDiscard = false; editingOffer = null; advertForm = emptyAdvertise(); }}>{$t('shop.discard')}</button>
																	<button type="button" class="btn-ghost" onclick={() => (advertDiscard = false)}>{$t('shop.keep_editing')}</button>
																</div>
															</div>
														{/if}
																	</div>
																</div>
												{/if}
												{#if myOffers.length || shopPending.length}
												<div class="mlist">
												<!-- QZ (t_ab10109f): lokale pending-Listings (Status
												„wartet auf Freigabe", nach 24 h „Freigabe unbestätigt")
												über den API-Listings — kein Auto-Delete, Entfernen nur
												explizit durch den Nutzer. -->
												{#each shopPending as p (p.id)}
												<div
												class="mrow"
												role="option"
												aria-selected={false}
												title={$t('shop.saved_locally_title')}
												>
												<div class="m-op {p.operation}">{p.operation === 'sell' ? 'WTS' : 'WTB'}</div>
												<div class="m-main">
													<div class="m-title">{p.title}</div>
													<div class="m-sub">{p.location || '—'} · seit {new Date(p.created_at * 1000).toLocaleString('de-DE')}</div>
													<span class="chip warn" title={p.created_at < Date.now() / 1000 - 24 * 3600 ? tr('shop.approval_stale_note') : tr('shop.pending_moderation_title')}>{p.created_at < Date.now() / 1000 - 24 * 3600 ? $t('shop.approval_unconfirmed') : $t('shop.status_pending')}</span>
												</div>
												<div class="m-right">
													<div class="m-price {p.operation === 'sell' ? 'sell' : 'buy'}">{fmtPrice(p.price)}</div>
													<div class="m-meta">{fmtQty(p.quantity)} {p.unit || 'unit'}</div>
												</div>
												<div class="m-right" style="gap: 4px">
													<button type="button" class="btn-ghost" title={$t('shop.remove_local_title')} onclick={() => void removePendingOffer(p.id)}>{$t('settings.remove')}</button>
												</div>
												</div>
												{/each}
												{#each myOffers as o, i (o.id)}
									<div tabindex="-1" onkeydown={keyActivate}
										class="mrow"
										role="option"
										aria-selected={false}
										class:is-sel={false}
										data-shop-offer-idx={i}
										title={$t('shop.open_listing_readonly_title')}
										onclick={() => void openListingDeepLink(o.slug)}
									>
										<div class="m-op {o.operation}">{o.operation === 'sell' ? 'WTS' : 'WTB'}</div>
										<div class="m-main">
											<div class="m-title">{o.title}</div>
											<div class="m-sub">{o.location || '—'}{o.region ? ' · ' + o.region : ''} · {shopStatus(o)}</div>
											{#if !o.approved}
												<span class="chip warn" title={$t('shop.pending_moderation_title')}>{$t('shop.pending')}</span>
											{/if}
										</div>
																				<div class="m-right">
																					<div class="m-price {o.operation === 'sell' ? 'sell' : 'buy'}">{fmtPrice(o.price)}</div>
																					<div class="m-meta">{fmtQty(o.quantity)} {o.unit || 'unit'} · {rel(o.updated)}</div>
																				</div>
																				<div class="m-right" style="gap: 4px">
																					<button type="button" class="btn-ghost" title={$t('shop.negotiate_browser_title')} onclick={(ev) => { ev.stopPropagation(); void openListingDeepLink(o.slug); }}>💬 Verhandeln</button>
									<button type="button" class="btn-ghost" title={$t('shop.no_edit_note_title')} onclick={(ev) => { ev.stopPropagation(); void openAdvertise(o); }}>{$t('shop.new_button')}</button>
																					<button type="button" class="btn-ghost" title={$t('shop.unlist_2step_title')} onclick={(ev) => { ev.stopPropagation(); requestDelete(o); }}>{$t('shop.unlist')}</button>
																				</div>
																			</div>
																		{/each}
							</div>
							<div class="m-foot">
								<span>{$t('shop.click_readonly_note')}</span>
								<span class="src">UEX Corp</span>
							</div>
						{:else}
							<div class="row-empty" style="padding: 14px 16px">
									{shopUser.trim() ? `Keine eigenen Offers für „${shopUser.trim()}“ bei UEX.` : $t('shop.no_username_note')}
								</div>
						{/if}
						<!-- QZ (t_ab10109f): „Letzte 5 Marktplatz-Trades" ENTFERNT — die
						     Liste gehört in den Trade-Log-Reiter (PT), sie doppelt sich
						     hier nicht mehr. Mein Shop zeigt nur Offers (+ lokale pending). -->
						{/if}
						<!-- D3 (t_10be9255): 2-Schritt-Bestätigung als MODALES Popup über der
													     Eingabemaske (dunkler Scrim). Vorher inline im Pane → auf dem
													     Hauptbildschirm aus dem Fenster gelaufen (Buttons nicht erreich-
													     bar). Jetzt: Karte mit Zusammenfassung + Bestätigen/Abbrechen,
													     immer innerhalb der Overlay-Größe (interne Scroll-Region). Während
													     der Bestätigungsphase sind die Eingaben gesperrt (Scrim + Lock). -->
														{#if confirmAction}
															<div class="confirm-scrim" role="presentation">
																<div class="confirm-modal" role="dialog" aria-modal="true" aria-label={$t('common.confirm')}>
																	<div class="center" style="padding: 0 0 10px">
																		<div class="big" style="color: var(--alert)">
																			{confirmAction.kind === 'delete' ? $t('shop.confirm_unlist_title') : $t('shop.confirm_create_title')}
																		</div>
																	</div>
																	{#if confirmAction.kind === 'delete'}
																		<div class="c-row"><b>{$t('common.title')}</b><span>{confirmAction.offer.title}</span></div>
																		<div class="c-row"><b>{$t('common.price')}</b><span>{fmtPrice(confirmAction.offer.price)} {confirmAction.offer.currency}</span></div>
																		<div class="c-row"><b>{$t('common.quantity')}</b><span>{fmtQty(confirmAction.offer.quantity)} {confirmAction.offer.unit || 'unit'}</span></div>
																		<div class="c-row"><b>{$t('trades.location')}</b><span>{confirmAction.offer.location || '—'}</span></div>
																		<div class="sm" style="margin-top: 8px; color: var(--fgdim)">{$t('shop.unlist_note')}</div>
																	{:else}
																		<div class="c-row"><b>{$t('common.title')}</b><span>{advertForm.title}</span></div>
																		<div class="c-row"><b>{$t('shop.kind')}</b><span>{advertForm.operation === 'sell' ? $t('shop.sell_offer_label') : $t('shop.buy_request_label')} ({advertForm.listing_type})</span></div>
																		<div class="c-row"><b>{$t('common.price')}</b><span>{fmtGroup(advertForm.price)} {advertForm.currency}</span></div>
																		<div class="c-row"><b>{$t('common.quantity')}</b><span>{advertForm.quantity} {advertForm.unit}</span></div>
																		<div class="c-row"><b>{$t('trades.location')}</b><span>{advertForm.location || '—'}</span></div>
																		<div class="sm" style="margin-top: 8px; color: var(--fgdim)">{$t('shop.create_note')}</div>
																	{/if}
																	<div class="mbar2" style="margin-top: 12px; justify-content: center; border: 0; padding: 0; background: transparent">
																		<button type="button" class="btn" class:btn-danger={confirmAction.kind === 'delete'} disabled={writeBusy} onclick={() => confirmAction?.kind === 'delete' ? void doDelete(confirmAction.offer) : void doAdvertise()}>
																			{confirmAction.kind === 'delete' ? $t('shop.confirm_unlist') : $t('shop.confirm_create')}
																		</button>
																		<button type="button" class="btn-ghost" disabled={writeBusy} onclick={() => (confirmAction = null)}>{$t('common.cancel')}</button>
																	</div>
																</div>
															</div>
														{/if}
				{:else if mLoading && !offers.length}
					<div class="center">
						<span class="spin"></span>
						<div class="big" style="margin-top: 12px">{$t('market.loading')}</div>
					</div>
				{:else if mNote && !offers.length}
					<div class="pane-error">
						<div class="center">
							<div class="big" style="color: var(--alert)">{$t('market.unreachable')}</div>
							<div class="sm">{mNote}</div>
							<span role="button" tabindex="0" onkeydown={keyActivate} class="btn-ghost" onclick={() => loadMarket()}>{$t('common.try_again')}</span>
						</div>
					</div>
				{:else}
					<div class="mbar">
						<button type="button" class="chip" class:is-on={mOp === 'all'} onclick={() => setOp('all')}>{$t('common.all')}</button>
						<button type="button" class="chip" class:is-on={mOp === 'sell'} onclick={() => setOp('sell')}><span class="dot"></span>{$t('common.sell')}</button>
						<button type="button" class="chip" class:is-on={mOp === 'buy'} onclick={() => setOp('buy')}><span class="dot"></span>{$t('common.buy')}</button>
						<span class="m-spacer"></span>
					</div>
					<div class="mbar2">
						<span class="m-lbl">{$t('common.quality')}</span>
						<input class="qnum" type="number" min="0" max="1000" step="10" placeholder={$t('common.min')} aria-label={$t('shop.min_quality_title')} title={$t('shop.min_quality_title')} bind:value={mqMin} />
						<span class="qnum-dash">–</span>
						<input class="qnum" type="number" min="0" max="1000" step="10" placeholder={$t('common.max')} aria-label={$t('shop.max_quality_title')} title={$t('shop.max_quality_title')} bind:value={mqMax} />
						<button type="button" class="chip" onclick={() => { mqMin = ''; mqMax = ''; }}>Q ×</button>
						<span class="m-lbl">{$t('routes.sort')}</span>
						<button type="button" class="chip" class:is-on={mSort === 'price_asc'} onclick={() => setSort('price_asc')}>{$t('routes.price_up')}</button>
						<button type="button" class="chip" class:is-on={mSort === 'price_desc'} onclick={() => setSort('price_desc')}>{$t('routes.price_down')}</button>
						<button type="button" class="chip" class:is-on={mSort === 'newest'} onclick={() => setSort('newest')}>{$t('market.newest')}</button>
						<span class="m-spacer"></span>
						{#if mSource === 'offline'}
							<span class="chip warn"><span class="dot"></span>{$t('shop.offline_snapshot')}</span>
						{:else if mSource === 'cache'}
							<span class="chip"><span class="dot"></span>{$t('shop.cache')}</span>
						{/if}
					</div>

					{#if visOffers.length}
						<div class="mlist" bind:this={mListEl} onscroll={onMarketListScroll}>
							{#each mVew as o, i}
								<div tabindex="-1" onkeydown={keyActivate}
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
								{#if mLoadingMore || mMshown < visOffers.length || !mAllLoaded}
									<span class="spin"></span><span class="sm">{mLoadingMore ? $t('market.loading_more') : $t('market.autoload_more')}</span>
								{:else}
									<span class="sm">{$t('market.all_loaded')}</span>
								{/if}
								<span class="src">UEX Corp</span>
							</div>
						</div>
					{:else}
						<div class="row-empty" style="padding: 28px 16px">{$t('market.no_offers')}</div>
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
						<span class="c">{$t('market.found')}<b>{searchTotal}</b>{$t('market.results_in')}<b>{searchMs}</b> ms</span>
					</div>

					<div class="list">
						{#each groups as g, gi}
							{@const show = filters.length ? filters.includes(g.kind) : true}
							{#if show && g.hits.length}
								<div class="vgroup">{$t(g.label)} <span class="n">{g.hits.length}</span></div>
								{#each g.hits as h, hi}
									{@const idx = groupOffset(gi) + hi}
									<div class="rwrap" class:in-detail={inDetail && expanded === idx}>
										<div onkeydown={keyActivate}
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
								<div class="vgroup">{$t(g.label)} <span class="n">0</span></div>
								<div class="row-empty">{$t('market.no_matches')}</div>
							{/if}
						{/each}
					</div>

					<!-- Detail-Seite (Overlay über der Liste) -->
					{#if inDetail}
						<div class="pane-detail" role="dialog" aria-modal="true" aria-label={$t('common.detail')}>
							{#if xpan?.busy && !xpan?.detail}
								<div class="d-ghost">
									<div class="cap">{$t('market.loading_prices')}</div>
								</div>
							{:else if xpan?.error}
								<div class="d-ghost">
									<div class="cap">{$t('common.error')}</div>
									<div class="sm">{friendlyErr(xpan.error)}</div>
								</div>
							{:else if xpan?.detail}
								{@const d = xpan.detail}
								<div class="d-head">
									<button class="d-back" aria-label={$t('common.back_to_list')} onclick={closeDetail}>←</button>
									<div class="d-title">
										<h2>{detailHit?.name ?? d.commodity}</h2>
										<span class="c">{detailHit?.subtitle ?? ''}</span>
									</div>
									<div class="d-price">
										<span class="buy"><i>{$t('market.best_buy')}</i>{dBest.buy === Infinity ? '—' : fmt(dBest.buy)}</span>
										<span class="sell"><i>{$t('market.best_sell')}</i>{dBest.sell > 0 ? fmt(dBest.sell) : '—'}</span>
									</div>
									<div class="d-variant" role="group" aria-label={$t('market.price_detail_title')}>
										<button class:active={detailVariant === 'a'} onclick={() => { localVariant = 'a'; void chooseVariant('a'); }} title={$t('settings.price_variant_a_title')}>A</button>
											<button class:active={detailVariant === 'c'} onclick={() => { localVariant = 'c'; void chooseVariant('c'); }} title={$t('settings.price_variant_c_title')}>C</button>
									</div>
								</div>

								<!-- K6 P2 (c): live sichtbare Wirkung — die Preisdetail zeigt
								     die Quelle, aus DERen Cache diese Preise kommen (Backend-
								     Provider-Label, identisch zur Statusbar). Nach Quelle-
								     Umschalten + erneutem Öffnen steht hier die neue Quelle.
								     K6 P1: ohne Key und UEX primär → Hinweis (Preise kommen
								     nur aus dem Wiki-Fallback). -->
								<div class="d-src" role="note">
									Quelle: {d.source.toUpperCase()} · Stand: {rel(d.fetched_at)}
									<button
										type="button"
										class="chip"
										aria-label={$t('routes.update_record_title')}
										onclick={() => void refreshDetailLive()}
									>↻ Aktualisieren</button>
									{#if d.source.includes('uex') && cfg && !cfg.uex_api_key_set}
										· <span class="warn">{$t('market.no_key_anon')}</span>
									{/if}
								</div>

								{#if dFilter}
									<div class="lock-hint"><span class="dot"></span>{$t('market.filter_active')}<b>{dFilter}</b> · {dRows.length} Zeilen · Feld leeren = alles zeigen</div>
								{/if}

								

								<div class="d-body">
									{#if d.location}
										<!-- Location-Aggregat: je Terminal eine Gruppe (N2: über dLoc gefiltert) -->
										{#each dLoc as grp}
											<div class="ptab locgrp">
												<div class="cap">{grp.terminal_name}</div>
												{#if grp.buys.length || grp.sells.length}
													<div class="ph"><span>{$t('shop.item')}</span><span>{$t('common.price')}</span><span>SCU</span></div>
												<!-- P8: leere Richtungs-Sektionen entfallen — nur vorhandene Richtungen bekommen Sektionen. -->
												{#if grp.buys.length}
													<div class="psub buy">{$t('common.buy')}</div>
													{#each grp.buys as r}
														<div class="pr" class:best={r === grp.buys[0]} class:is-sel={r.fi === dSel}>
															<span class="loc">{r.name}<small>{stockTxt(r.p, 'buy')}</small></span>
															<span class="val buy">{fmt(r.p.buy_price)}</span><span class="unit">aUEC</span>
														</div>
													{/each}
												{/if}
												{#if grp.sells.length}
													<div class="psub sell">{$t('common.sell')}</div>
													{#each grp.sells as r}
														<div class="pr" class:best={r === grp.sells[0]} class:is-sel={r.fi === dSel}>
															<span class="loc">{r.name}<small>{stockTxt(r.p, 'sell')}</small></span>
															<span class="val sell">{fmt(r.p.sell_price)}</span><span class="unit">aUEC</span>
														</div>
													{/each}
												{/if}
												{:else}
													<div class="pr"><span class="loc muted">{$t('market.no_price_data')}</span></div>
												{/if}
											</div>
										{/each}
										{#if !dLoc.length}
											<div class="np-state">{$t('market.no_terminal_match')}</div>
										{/if}
									{:else if d.prices.length === 0}
										<!-- QA-Auflage B: Item ohne Preise → "nicht kaufbar" -->
										<div class="np-state">{$t('market.not_buyable')}</div>
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
													<span class="k">{$t('market.cheapest_buy')}</span>
													<span class="v">{bb ? fmt(bb.p.buy_price) : '—'}</span>
													{#if bb}<span class="u">{$t('common.auec_scu')}</span>{/if}
													<span class="w">{bb ? `${bb.name} · ${stockTxt(bb.p, 'buy')}` : $t('market.no_buy_offer')}</span>
												</div>
												<div class="vC-kpi sell">
													<span class="k">{$t('market.highest_sell')}</span>
													<span class="v">{bs ? fmt(bs.p.sell_price) : '—'}</span>
													{#if bs}<span class="u">{$t('common.auec_scu')}</span>{/if}
													<span class="w">{bs ? `${bs.name} · ${stockTxt(bs.p, 'sell')}` : $t('market.no_sell_offer')}</span>
												</div>
												<div class="vC-kpi margin">
													<span class="k">{$t('market.margin_scu')}</span>
													<span class="v">{bb && bs ? fmt(margin) : '—'}</span>
													{#if bb && bs}<span class="u">aUEC / SCU · {fmt(margin * 100)} je 100 SCU</span>{/if}
													<span class="w">{bb && bs ? `${bb.name} → ${bs.name}` : $t('market.one_side_only')}</span>
												</div>
											</div>
											<div class="vC-listhead"><span>{dIsTerminal ? $t('shop.item') : $t('market.n_terminals')}</span><span>{$t('common.buy')}</span><span>{$t('common.sell')}</span></div>
											{#each dRows as r}
												<div class="pr vC-r" class:is-sel={r.fi === dSel}>
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
												<div class="np-state">{$t('market.no_price_match')}</div>
											{/if}
										{:else}
											{#if buys.length}
												<div class="ptab">
													<div class="cap">{$t('common.all')}<b class="buy">{$t('common.buy')}</b>-Preise · {buys.length} {dIsTerminal ? $t('market.n_items') : $t('market.n_terminals')}</div>
													<div class="ph"><span>{dIsTerminal ? $t('shop.item') : $t('market.n_terminals')}</span><span>{$t('common.price')}</span><span>SCU</span></div>
													{#each buys as r, bi}
														<div class="pr" class:best={bi === 0} class:is-sel={r.fi === dSel}>
															<span class="loc">{r.name}<small>{stockTxt(r.p, 'buy')}</small></span>
															<span class="val buy">{fmt(r.p.buy_price)}</span><span class="unit">aUEC</span>
														</div>
													{/each}
												</div>
											{:else}
												<div class="np-state">{dIsTerminal ? $t('market.no_item_to_buy') : $t('market.no_terminal_buys')}</div>
											{/if}
											{#if sells.length}
												<div class="ptab">
													<div class="cap">{$t('common.all')}<b class="sell">{$t('common.sell')}</b>-Preise · {sells.length} {dIsTerminal ? $t('market.n_items') : $t('market.n_terminals')}</div>
													<div class="ph"><span>{dIsTerminal ? $t('shop.item') : $t('market.n_terminals')}</span><span>{$t('common.price')}</span><span>SCU</span></div>
													{#each sells as r, si}
														<div class="pr" class:best={si === 0} class:is-sel={r.fi === dSel}>
															<span class="loc">{r.name}<small>{stockTxt(r.p, 'sell')}</small></span>
															<span class="val sell">{fmt(r.p.sell_price)}</span><span class="unit">aUEC</span>
														</div>
													{/each}
												</div>
											{:else}
												<div class="np-state">{dIsTerminal ? $t('market.no_item_to_sell') : $t('market.no_terminal_sells')}</div>
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
						<div class="sect"><div class="h">{$t('search.last_searched')}</div></div>
						<div class="recent">
						{#each recent.slice(0, 6) as q (q)}
							<span role="button" tabindex="0" onkeydown={keyActivate} class="r" title={$t('search.this_search_title')} onclick={() => { input = q; void runSearch(); }}>{q}</span>
						{:else}
							<span class="r">{$t('search.none_type_to_search')}</span>
						{/each}
					</div>
						<div class="sect"><div class="h">{$t('search.quick_access')}</div></div>
							<div class="recent">
								<span class="r"><b>{hotkeyFor(cfg, 'market')}</b>{$t('search.marketplace_dot')}<b>{hotkeyFor(cfg, 'routes')}</b>{$t('market.tab_routes')}</span>
								<span class="r"><b>F12</b>{$t('nav.settings')}</span>
								<span class="r"><b>{$t('search.calculator')}</b>{$t('search.math_eg')}</span>
								<span class="r"><b>{$t('shop.category')}</b>{$t('search.c_eg')}</span>
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
						<span><kbd>Esc</kbd>{$t('common.back_to_list')}</span>
						<span><kbd>↵</kbd>{$t('search.open_in_browser')}</span>
					{:else if mTab === 'shop'}
						{#if cfg?.uex_secret_key_set}
							<span><kbd>F5</kbd>{$t('search.shop_sync')}</span>
						{/if}
						<span><kbd>Esc</kbd>{$t('search.back_to_search_lower')}</span>
					{:else if mTab === 'routes'}
						<span><kbd>↑↓</kbd>{$t('market.tab_routes')}</span>
						<span><kbd>↵</kbd>{$t('common.detail')}</span>
						<span><kbd>F5</kbd>{$t('search.route_sweep')}</span>
						<span><kbd>Esc</kbd>{$t('search.back_to_search_lower')}</span>
					{:else}
						<span><kbd>↑↓</kbd>{$t('market.tab_offers')}</span>
						<span><kbd>{$t('search.alt_hotkeys')}</kbd>{$t('search.offer')}</span>
						<span><kbd>↵</kbd>{$t('common.detail')}</span>
						<span><kbd>F5</kbd>{$t('search.market_sync')}</span>
						<span><kbd>Esc</kbd>{$t('search.back_to_search_lower')}</span>
					{/if}
				{:else if inDetail}
					{#if dRows.length}
						<span><kbd>↑↓</kbd> Zeile ({dSel + 1}/{dRows.length})</span>
						<span><kbd>↵</kbd>{$t('search.trade_commodities')}</span>
					{/if}
					<span>{$t('search.field_is_filter')}</span>
					<span><kbd>Esc</kbd>{$t('search.back')}</span>
				{:else if railMode || inConfig}
					<!-- PW-6 (t_fbe40115): Hangar/Inventar/Settings — KEINE Alt-1..0-
					     Ergebnis-Hinweise (keine Ergebnisliste) und kein F2-Marktplatz-
					     Hinweis; nur die kontextuellen Tasten dieser Views. -->
					<span><kbd>↑↓</kbd>{$t('search.nav_label')}</span>
					<span><kbd>↵</kbd>{$t('common.detail')}</span>
					<span><kbd>Esc</kbd>{$t('search.back')}</span>
				{:else}
					<span><kbd>↑↓</kbd>{$t('search.nav_label')}</span>
					<span><kbd>{$t('search.alt_hotkeys')}</kbd>{$t('search.matches')}</span>
					<span><kbd>↵</kbd>{$t('common.detail')}</span>
					<span><kbd>Esc</kbd>{$t('search.back')}</span>
					<span><kbd>{hotkeyFor(cfg, 'market')}</kbd>{$t('nav.marketplace')}</span>
				{/if}
			</div>
			{#if f5Msg}
				<span class="f5-msg" role="status">{f5Msg}</span>
			{/if}
			{#if !railMode && !inConfig}
				<!-- PW-6 (t_fbe40115): Sync-Hinweis nur in Sync-relevanten Views
				     (Suche/Detail/Marktplatz) — nicht in Hangar/Inventar/Settings
				     (vgl. PB-Logik für den F5-Hinweis). -->
				<!-- PS-R1-Fix (t_6ba53273): In Terminal/Location-Details ist der
				     Stempel irreführend — F5/↻-Refresh greift nur bei Commodity/
				     Item-Detail. Dort bleibt der Stempel, hier nicht. -->
				{#if !(inDetail && (detailHit?.kind === 'terminal' || detailHit?.kind === 'location'))}
					<span class="sync {syncLabel().cls}"><span class="dot"></span>{syncLabel().text}</span>
				{/if}
			{/if}
			<!-- Zahnrad entfernt (t_a39f6811 P2): Rail + F12 decken die Konfiguration ab. -->
			{#if isSpotlight}
				<div class="spot-status">
					<span class="dot" class:live={overlay?.game_running}></span>
					<button onclick={toggleClickThrough}>
						{overlay?.click_through ? 'KLICK' : 'FOKUS'}
					</button>
					<!-- TW (t_f178b084): Info-Chip RECHTS neben dem Fokus/Klick-Button
					     — öffnet das Info-Popup (modales Muster, kein View-Wechsel). -->
					<button
						type="button"
						class="info-btn"
						title={$t('info.title')}
						aria-label={$t('info.open_title')}
						onclick={openInfo}>ⓘ</button
					>
				</div>
			{/if}
		</div>
		{#if infoOpen}
			<!-- TW (t_f178b084): Info-Popup — GLEICHES Muster wie die Formular-/
			     Detail-Popups (shell-anchored advert-scrim, inline z-index über
			     den übrigen Scrims): Scrim über der Shell, Karte zentriert,
			     Esc/✕ schließt, KEIN View-Wechsel. Andere offene Popups bleiben
			     unangetastet (eigener State). -->
			<!-- UG (t_a1f0d83f): Info-Popup-Politur — Sebos Vector-Logo (brand/logo/
			     logo.svg) prominent MITTIG im Kopf, Abschnitts-Struktur (Über →
			     Datenquellen → Rechtliches → Footer-Links). Shell wächst mit
			     (class:shell-info → min-height), damit das Popup ohne internen
			     Scroll vollständig in die content-driven Shell passt. -->
			<div class="advert-scrim" style="z-index: 240" role="presentation">
				<div class="info-modal" role="dialog" aria-modal="true" aria-label={$t('info.title')}>
					<div class="info-head">
						<div class="info-hero">
							<img class="info-logo" src={logoUrl} alt="Architects Overlay — Sebos Vector-Logo" draggable="false" />
							<div class="info-title">Architects Overlay</div>
							<div class="info-meta">
								<span>Version v{infoVersion}</span>
								<span>Build {__BUILD_DATE__}</span>
							</div>
						</div>
						<button
							type="button"
							class="hd-close"
							title={$t('common.close_esc')}
							aria-label={$t('info.close_title')}
							onclick={closeInfo}>✕</button
						>
					</div>
					<div class="info-body">
						<section class="info-sec">
							<h3 class="info-sec-title">{$t('info.about')}</h3>
							<p class="info-text">
								Architects Overlay ist ein inoffizielles Community-Projekt für Star Citizen:
								Handelsrouten, Marktplatz-Preise und Hangar-Verwaltung direkt im Spiel —
								als Overlay von der Community, für die Community. Non-Commercial, ohne
								RSI-/CIG-Assets als eigenes Branding.
							</p>
						</section>
						<section class="info-sec">
							<h3 class="info-sec-title">{$t('settings.data_sources')}</h3>
							<div class="info-badges">
								<!-- UEX: offizielles API-Badge-Asset (uexcorp.space/about/resources) —
								     Link zur API-Doku; das Overlay ruft die API 2.0
								     (api.uexcorp.space) für Daten & Marktplatz auf. -->
								<span
									class="info-badge"
									role="button"
									tabindex="0"
									onkeydown={keyActivate}
									title={$t('info.open_uex_docs_title')}
									onclick={() => void openInfoLink('uex_api')}
								>
									<img src="/uex-api-badge-powered.png" alt="Powered by UEX" draggable="false" />
									<span class="info-badge-text">
										<b>{$t('info.data_marketplace')}</b>
										<span>{$t('info.uex_api_source')}</span>
									</span>
								</span>
								<!-- SC-Wiki: keine offiziellen Badge-Assets gefunden (Swagger-
								     Doku, kein Logo-Download) → Text-Badge im Overlay-Stil.
								     Die Wiki-Doku enthält keine gesonderte Credit-Pflicht;
								     Attribution hiermit erledigt (API + Namen + Link). -->
								<span
									class="info-badge"
									role="button"
									tabindex="0"
									onkeydown={keyActivate}
									title={$t('info.open_scwiki_title')}
									onclick={() => void openInfoLink('scwiki_api')}
								>
									<span class="info-badge-mark">{$t('info.scwiki_api')}</span>
									<span class="info-badge-text">
										<b>{$t('info.ship_data')}</b>
										<span>{$t('info.scwiki_source')}</span>
									</span>
								</span>
							</div>
						</section>
						<section class="info-sec">
							<h3 class="info-sec-title">{$t('info.legal')}</h3>
							<p class="info-disclaimer">
								Architects Overlay steht in keiner Verbindung zu Cloud Imperium Games (CIG),
								Roberts Space Industries oder deren Partnern und wird nicht offiziell
								unterstützt oder gesponsert. Star Citizen, Roberts Space Industries und das
								RSI-Logo sind Marken von Cloud Imperium Games Corp.
							</p>
							<div class="info-cig">
								<!-- CIG Fan-Content-Policy: „Made by the Community“-Asset —
								     OFFIZIELL, UNVERÄNDERT (kein Umfärben); Quelle: CIG Fankit/
								     Fandom-FAQ (siehe brand/third-party/README.md). -->
								<img
									src="/cig-made-by-the-community.png"
									alt="Made by the Community"
									draggable="false"
								/>
								<span class="info-cig-src">{$t('info.cig_asset_note')}</span>
							</div>
							<p class="info-license">{$t('info.license_note')}</p>
						</section>
						<footer class="info-foot">
							<div class="info-links">
								<span class="chip" role="button" tabindex="0" onkeydown={keyActivate} title={$t('info.github_repo_title')} onclick={() => void openInfoLink('repo')}>{$t('info.github_repo')}</span>
								<span class="chip" role="button" tabindex="0" onkeydown={keyActivate} title={$t('info.release_page_title')} onclick={() => void openInfoLink('releases')}>{$t('info.releases')}</span>
							</div>
							{#if infoMsg}<span class="info-msg" role="status">{infoMsg}</span>{/if}
						</footer>
					</div>
				</div>
			</div>
		{/if}
		</div><!-- /shell-main -->
	</div>
</main>
