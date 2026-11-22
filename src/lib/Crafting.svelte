<script lang="ts">
	// Crafting-Modus (M8-K2, t_f95f9d97) — F6-Modus im Rail/F-Keys-System.
	//
	// Datenquelle: Strata Public API via `strata_client` (M8-K1) — die Tauri-
	// Commands `strata_items` / `strata_craft_cost` liefern cache-first
	// Responses inkl. Pflicht-Attribution. Diese Komponente ist der F6-View:
	//
	//   1. Item-Suche    — das HAUPT-Suchfeld der Palette filtert als
	//                      In-Context-Filter die Liste (M7-F3-Muster: das Feld
	//                      IST der Filter; LEER = Gesamtliste).
	//   2. Kategorie     — Chips DYNAMISCH aus den Item-Daten (All + jede
	//                      vorkommende category), nichts hartkodiert.
	//   3. Item-Detail   — Materialliste mit Menge + EINHEIT farblich getrennt
	//                      (SCU / gems / items — NIE mischen; 1000 gems = 1 SCU
	//                      als Legenden-Anmerkung über der Liste), Craft-Zeit.
	//   4. Individuelle QUALITÄTSREGLER (M8-R1-A/R3) + Gesamt-Qualität:
	//                      je Material Regler-Zeile (Balken + Wert + Snap-
	//                      Toggle) und darunter die Effektzeile voll
	//                      ausgeschrieben (M8-R4). Gesamt-Qualität = MITTELWERT
	//                      aller Kosten-Zeilen-Regler (read-only Balken). M8-K6:
	//                      dazu der echte Stat-Rechner (QualityStats) —
	//                      Modifier-Interpolation als %-Abweichung, sobald ein
	//                      SC-Wiki-Blueprint mit Modifier-Daten gematcht hat
	//                      (sonst still weg).
	//
	// NICHT gebaut (M8-K5): Item-Details (Size/Grade/Class), Blaupausen-
	// Missionen, dismantle_returns.
	//
	// Formular-Konvention (Sebo): Liste startet VOLL, Filter per Haupt-Suchfeld
	// + Chips; KEIN natives <select> (Fokusduell — 2026-11-19 gelernt).
	// Key-Missing-State wie Mining: Erklär-Card + Link
	// https://strata.celd.space/api-keys. Attribution-Footer IMMER sichtbar.
	import { t, tr, trError, currentLang } from './i18n';
	import { invoke } from '@tauri-apps/api/core';
	// M8-K6 (t_f7a29367): echter Qualitäts-Stat-Rechner
	import QualityStats from './QualityStats.svelte';
	// M8-R1-B: Custom-Dropdowns (DOM-Popover, kein natives <select> —
	// Fokusduell-Bug im Spotlight-Webview, siehe M7-R1-Feedback).
	import SelectCombo from './SelectCombo.svelte';
	// M8-R1-A: individuelle Material-Regler + Snap-Punkte
	// M8-R3 (t_399b10d8): Custom-Quality-Slider (nativ range → div-basiert)
	import CraftQualitySlider from './CraftQualitySlider.svelte';
	// M9-B (t_36778bc3): Listen-Tab + "Zur Liste hinzufügen" im Detail.
	import CraftLists from './CraftLists.svelte';
	// M9-R1: gemeinsame Reiter-Komponente (Segmented Control, überall gleich)
	import Tabs from './Tabs.svelte';
	import {
		aspectQuality,
		materialKey,
		meanQuality,
		nearestSnap,
		statChanges,
		type Aspect,
		type SummaryProperty
	} from './quality-calc';

	type Item = {
		id: string;
		name: string;
		slug: string;
		category: string | null; // freie Kategorie — dynamisch, null-fähig
	};
	type ItemsResponse = { items: Item[]; count: number; attribution: string };
	type CostEntry = {
		material: string;
		oreId: string | null; // M8-K3 (Mining-Sprung) — hier nur Anzeige
		quantity: number;
		unit: 'SCU' | 'gems' | 'items';
		minQuality: number | null; // Zahl-oder-null; 0 ist gültige Daten
	};
	type CraftCostResponse = {
		item: { id: string; name: string; category: string | null };
		craftTime: number; // Sekunden
		costs: CostEntry[];
		attribution: string;
		// M8-K6: SC-Wiki-Blueprint-Match (optional — ohne Match bleibt der
		// Stat-Rechner-Block weg, kein Hard-Fail; Befüllung durch M8-K5).
		blueprintAspects?: Aspect[] | null;
		blueprintSummary?: SummaryProperty[] | null;
	};
	// M8-K5 (t_03eafbfb): SC-Wiki-Craft-Daten (Item-Details + Blueprint).
	// Feldnamen folgen den Rust-Structs (snake_case) — bewusst OHNE
	// camelCase-Rename, damit Cache-Payload und Wire identisch sind.
	type WeaponDamage = { damage_type: string; damage: number | null };
	type VehicleWeapon = {
		capacity: number | null;
		range: number | null;
		damage_per_shot: number | null;
		rpm: number | null;
		damages: WeaponDamage[];
	};
	type Ammunition = {
		size: number | null;
		capacity: number | null;
		speed: number | null;
		range: number | null;
		lifetime: number | null;
	};
	type ScWikiItem = {
		name: string;
		item_type: string;
		size: number | null;
		grade: string | null;
		class: string | null;
		mass: number | null;
		description_de: string | null;
		description_en: string | null;
		blueprint_uuids: string[];
		vehicle_weapon: VehicleWeapon | null;
		ammunition: Ammunition | null;
	};
	type UnlockingMission = {
		title: string;
		debug_name: string | null;
		reward_scope: string | null;
		count: number | null;
		web_url: string | null;
	};
	type MissionGroup = {
		label: string;
		chance: number | null;
		missions: UnlockingMission[];
	};
	type ScWikiBlueprint = {
		uuid: string;
		output_name: string;
		is_available_by_default: boolean;
		game_version: string | null;
		dismantle_returns: { name: string; quantity_scu: number | null }[];
		summary_properties: SummaryProperty[];
		aspects: Aspect[];
		unlocking_missions_grouped: MissionGroup[];
		web_url: string | null;
	};
	type ScWikiCraftDetail = {
		item: ScWikiItem;
		blueprint: ScWikiBlueprint | null;
		attribution: string;
	};
	type StrataStatus = {
		enabled: boolean;
		key_set: boolean;
		key_ref: string;
		tier_default: number;
		scwiki_ready: boolean; // M8-K5: SC-Wiki-Craft-Daten im Cache
	};

	let {
		onexit,
		filter = $bindable(''),
		onOpenOre = null, // M8-K3 (t_a8088d06): Bedarfserz → Mining-Sprung
		// M9-D (t_36f3966c): Brücken-Anlegen → Mining-Modus mit neuer Liste.
		onOpenMiningList = null,
		// M9-F (t_bb97da62): Inventar-Eingabemaske aus der Crafting-Liste öffnen.
		onOpenInventory = null,
		hidden = false // M8-K3: keep-alive während des Mining-Sprungs (Tastatur stumm)
	}: {
		onexit: () => void;
		filter?: string;
		onOpenOre?: ((oreId: string) => void) | null;
		onOpenMiningList?: ((listId: number) => void) | null;
		onOpenInventory?: (() => void) | null;
		hidden?: boolean;
	} = $props();

	// ---- State ---------------------------------------------------------------
	let status = $state<StrataStatus | null>(null);
	let checked = $state(false); // Status-Load abgeschlossen
	let items = $state<Item[]>([]);
	let attribution = $state('');
	let loading = $state(false);
	let error = $state('');

	// M8-R1-B (t_13ba04ff): Filter-Kategorisierung — Oberkategorien FPS/Ship/
	// Mission Items als CUSTOM-Dropdown (SelectCombo, KEIN natives <select> —
	// Fokusduell-Bug im Webview), Unterkategorie-Dropdown zeigt nur Optionen
	// der gewählten Oberkategorie. „Alle“ bleibt PILL/BUTTON und ist der
	// Filter-Reset (User-P5). Mapping datengetrieben auf die vorhandenen
	// category-Strings (Substring/Mapping-Tabelle), keine Hardcode-Explosion.
	let oberFilter = $state<string>('all');
	let subFilter = $state<string>('all');
	// Mapping-Tabelle: vorhandener category-String → { ober, sub }.
	// sub = Anzeige-Label der Unterkategorie (FPS-Präfix „FPS “ wird abgestreift).
	type Ober = 'fps' | 'ship' | 'mission' | 'other';
	const CAT_MAP: Record<string, { ober: Ober; sub: string }> = {
		'FPS Armor': { ober: 'fps', sub: 'Armor' },
		'FPS Weapon': { ober: 'fps', sub: 'Weapons' },
		'FPS Magazine': { ober: 'fps', sub: 'Magazine' },
		Cooler: { ober: 'ship', sub: 'Cooler' },
		'Mining Laser': { ober: 'ship', sub: 'Mining Laser' },
		'Mining Module': { ober: 'ship', sub: 'Mining Module' },
		'Ore Pod': { ober: 'ship', sub: 'Ore Pod' },
		'Power Plant': { ober: 'ship', sub: 'Power Plant' },
		'Quantum Drive': { ober: 'ship', sub: 'Quantum Drive' },
		Radar: { ober: 'ship', sub: 'Radar' },
		'Refuel Nozzle': { ober: 'ship', sub: 'Refuel Nozzle' },
		'Salvage Module': { ober: 'ship', sub: 'Salvage Module' },
		Shield: { ober: 'ship', sub: 'Shield' },
		'Ship Weapon': { ober: 'ship', sub: 'Ship Weapon' },
		'Mission Item': { ober: 'mission', sub: '' }
	};
	const oberOf = (cat: string | null): Ober => CAT_MAP[cat ?? '']?.ober ?? 'other';
	// „Sonstige“-Ober nur anzeigen, wenn die Daten tatsächlich Unmapped/null
	// enthalten (datengetrieben — in der Live-Basis nicht der Fall).
	const hasOther = $derived(items.some((i) => oberOf(i.category) === 'other'));
	// Unterkategorie-Optionen NUR der gewählten Oberkategorie (Spec): aus der
	// Mapping-Tabelle, value = roher category-String (Filter-Ebene). Mission
	// Items haben keine Unterkategorie (beides in einem).
	const subOptions = $derived.by(() => {
		if (oberFilter !== 'fps' && oberFilter !== 'ship') return [];
		const subs = Object.entries(CAT_MAP)
			.filter(([, v]) => v.ober === oberFilter)
			.map(([cat, v]) => ({ value: cat, label: v.sub }))
			.sort((a, b) => a.label.localeCompare(b.label));
		return [{ value: 'all', label: $t('common.all') }, ...subs];
	});
	// SelectCombo leert das Feld bei Blur ohne Auswahl → value='' — im Filter-
	// Kontext ist das ein Reset auf „Alle“ (kein Dead-End-Filter).
	$effect(() => {
		if (oberFilter === '') oberFilter = 'all';
		if (subFilter === '') subFilter = 'all';
	});
	// Detail
	let detail = $state<CraftCostResponse | null>(null);
	// M9-A: ID des Klick-Items (Besitz-Haken im Detail bezieht sich auf den
	// Listeneintrag — die Craft-Response-ID kann abweichen).
	let detailItemId = $state('');
	let detailLoading = $state(false);
	let detailError = $state('');
	// M8-K5: SC-Wiki-Craft-Detail (Item-Kopf/Stats/Missionen) — null =
	// kein Match (Abschnitte still weg, kein Hard-Fail).
	let scwiki = $state<ScWikiCraftDetail | null>(null);
	// M8-K5-Fix: SC-Wiki-Fetch fehlgeschlagen (z. B. API-Ausfall) — degradiert
	// ohne Hard-Fail, aber MIT sichtbarem Hinweis + Retry statt stummem Schlucken.
	let scwikiError = $state(false);
	// Qualitäts-Planungsrechner
	// M8-R1-A: individuelle Material-Regler — Qualität je Kosten-Zeile
	// (Key `material:idx`), 0–1000; Snap-Punkte + Toggle je Regler.
	let matQual = $state<Record<string, number>>({});
	let snapOn = $state<Record<string, boolean>>({});
	let snapPoints = $state<Record<string, number[]>>({}); // normMaterialname → Union
	// Persistenz des Snap-Defaults (nice-to-have, localStorage — Craft-
	// Setting ohne Backend-Roundtrip).
	const SNAP_DEFAULT_KEY = 'craft.snap_default';
	function snapDefault(): boolean {
		try {
			return localStorage.getItem(SNAP_DEFAULT_KEY) !== '0';
		} catch {
			return true;
		}
	}
	function setSnapDefault(on: boolean) {
		try {
			localStorage.setItem(SNAP_DEFAULT_KEY, on ? '1' : '0');
		} catch {
			/* kein Persistenz-Zwang */
		}
	}
	// Load-More (516 Items — bestehendes Muster: stückweise rendern)
	let shown = $state(80);
	const STEP = 80;

	// ---- M9-B (t_36778bc3): Tabs + "Zur Liste hinzufügen" --------------------
	// Tab-Wechsel Craft ↔ Listen (Listen-Engine-View; M9-Plan Welle 1 B —
	// eigener F-Modus blieb Offen-Punkt, daher reversibel als Tab gelöst).
	let tab = $state<'craft' | 'lists'>('craft');
	// Popover "Zur Liste hinzufügen" im Detail: existierende Listen + Neu.
	type ListSummaryLite = { id: number; name: string; item_count: number };
	let listPickOpen = $state(false);
	let myLists = $state<ListSummaryLite[]>([]);
	let newListName = $state('');
	let listNote = $state(''); // Erfolg/Fehler-Hinweis am Popover
	async function loadMyLists() {
		try {
			myLists = await invoke<ListSummaryLite[]>('lists_list', { kind: 'craft' });
		} catch {
			myLists = [];
		}
	}
	function toggleListPick() {
		listPickOpen = !listPickOpen;
		listNote = '';
		if (listPickOpen) void loadMyLists();
	}
	// Blueprint-Snapshot anhängen: Materialliste + AKTUELLE Regler-Werte
	// (Default 500 — „Qualität wird im Calculator beim Hinzufügen übernommen").
	async function addToList(listId: number) {
		if (!detail) return;
		listNote = '';
		try {
			await invoke('list_add_item', {
				listId,
				item: {
					item_id: detail.item.id,
					item_name: detail.item.name,
					craft_time: detail.craftTime,
					copies: 1,
					mats: detail.costs.map((c, idx) => ({
						material: c.material,
						ore_id: c.oreId,
						unit: c.unit,
						quantity: c.quantity,
						quality: rowQuality(c, idx)
					}))
				}
			});
			listNote = $t('lists.added');
		} catch (e) {
			listNote = trError(e);
		}
	}
	async function createAndAdd() {
		const name = newListName.trim();
		if (!name || !detail) return;
		listNote = '';
		try {
			const id = await invoke<number>('list_create', { kind: 'craft', name });
			newListName = '';
			await addToList(id);
			await loadMyLists();
		} catch (e) {
			listNote = trError(e);
		}
	}

	const ATTRIBUTION_FALLBACK = 'Data from celd.space / Strata Mining Tools';

	// ---- M9-A (t_60af9ec1): Blueprint-Pool-Besitz (lokal persistiert) --------
	// Haken „im Besitz" in Liste + Detail; Persistenz via Settings (config.toml)
	// — restart-fest, client-lokal, kein Cloud-Zwang. Key = Strata-Item-ID.
	let ownedIds = $state<Set<string>>(new Set());
	async function loadOwned() {
		try {
			ownedIds = new Set(await invoke<string[]>('blueprint_owned_get'));
		} catch {
			// Persist-Fehler degradiert still — Filter/Haken arbeiten mit leerer Menge.
			ownedIds = new Set();
		}
	}
	async function toggleOwned(id: string) {
		try {
			const nowOwned = await invoke<boolean>('blueprint_owned_toggle', { id });
			const next = new Set(ownedIds);
			if (nowOwned) next.add(id);
			else next.delete(id);
			ownedIds = next; // Neues Set → $state-Invalidierung
		} catch (e) {
			console.warn('blueprint_owned_toggle failed', e);
		}
	}
	// Verfügbarkeits-Filter neben den Kategorie-Filtern (Spec M9): drei
	// Zustände im Kreis — alle → verfügbar (im Besitz) → nicht verfügbar.
	let ownFilter = $state<'all' | 'owned' | 'not_owned'>('all');
	const OWN_FILTER_ORDER = ['all', 'owned', 'not_owned'] as const;
	function cycleOwnFilter() {
		const idx = OWN_FILTER_ORDER.indexOf(ownFilter);
		ownFilter = OWN_FILTER_ORDER[(idx + 1) % OWN_FILTER_ORDER.length];
		shown = STEP; // Filterwechsel → Liste zurücksetzen (bestehendes Muster)
	}

	// ---- Load ----------------------------------------------------------------
	async function loadStatus() {
		checked = false;
		error = '';
		try {
			status = await invoke<StrataStatus>('strata_status');
		} catch (e) {
			error = trError(e);
		}
		checked = true;
		if (status?.enabled && status?.key_set) {
			void loadItems();
			void loadOwned();
		}
	}

	async function loadItems() {
		loading = true;
		error = '';
		try {
			const r = await invoke<ItemsResponse>('strata_items');
			// t_9107f006: SC-Wiki-Daten-Dupes clientseitig entfernen ("Antium Core
			// Jet" kam doppelt) — nach normalem Namen dedupen, erste Version gewinnt.
			const seen = new Set<string>();
			items = [...r.items]
				.filter((it) => {
					const k = (it.name ?? '').trim().toLowerCase();
					if (seen.has(k)) return false;
					seen.add(k);
					return true;
				})
				.sort((a, b) => a.name.localeCompare(b.name));
			attribution = r.attribution || ATTRIBUTION_FALLBACK;
		} catch (e) {
			error = trError(e);
		}
		loading = false;
	}

	$effect(() => {
		void loadStatus();
	});

	// ---- Filter (LEER = Gesamtliste, case-insensitiv Name/Slug) --------------
	const itemMatches = (i: Item, q: string) =>
		i.name.toLowerCase().includes(q) || i.slug.toLowerCase().includes(q);
	const filteredItems = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		return items.filter((i) => {
			if (q && !itemMatches(i, q)) return false;
			if (oberFilter !== 'all' && oberOf(i.category) !== oberFilter) return false;
			if (subFilter !== 'all' && i.category !== subFilter) return false;
			// M9-A: Verfügbarkeits-Filter (im Besitz / nicht im Besitz).
			if (ownFilter === 'owned' && !ownedIds.has(i.id)) return false;
			if (ownFilter === 'not_owned' && ownedIds.has(i.id)) return false;
			return true;
		});
	});
	// Oberkategorie-Optionen: Alle + FPS/Ship/Mission Items (+ „Sonstige“ nur
	// wenn die Daten Unmapped/null enthalten — datengetrieben).
	const oberOptions = $derived.by(() => {
		const opts = [
			{ value: 'all', label: $t('common.all') },
			{ value: 'fps', label: $t('craft.ober_fps') },
			{ value: 'ship', label: $t('craft.ober_ship') },
			{ value: 'mission', label: $t('craft.ober_mission') }
		];
		if (hasOther) opts.push({ value: 'other', label: $t('craft.cat_other') });
		return opts;
	});
	// Oberkategorie-Wechsel resettet die Unterkategorie (Spec).
	function setOber(v: string) {
		oberFilter = v;
		subFilter = 'all';
	}
	// „Alle“-Pill = Filter-Reset (User-P5).
	function resetFilters() {
		oberFilter = 'all';
		subFilter = 'all';
	}
	// Filter-/Kategorie-Wechsel: Liste wieder von vorn rendern.
	$effect(() => {
		void filter;
		void oberFilter;
		void subFilter;
		void ownFilter;
		shown = STEP;
	});

	// ---- Detail --------------------------------------------------------------
	async function openDetail(item: Item) {
		detailItemId = item.id;
		detailLoading = true;
		detailError = '';
		detail = null;
		scwiki = null; // M8-K5: SC-Wiki-Detail des Vorgängers verfällt
		scwikiError = false;
		matQual = {}; // M8-R1-A: individuelle Regler neutral
		snapOn = {};
		snapPoints = {};
		try {
			const r = await invoke<CraftCostResponse>('strata_craft_cost', { item: item.id });
			detail = r;
			attribution = r.attribution || ATTRIBUTION_FALLBACK;
			// M8-K5: SC-Wiki-Bereicherung parallel nachladen — Fehler/Nicht-
			// Match degradiert still (Kopf-/Stats-/Missions-Blöcke bleiben weg).
			void loadScwiki(item.name);
		} catch (e) {
			detailError = trError(e);
		}
		detailLoading = false;
	}
	async function loadScwiki(name: string) {
		scwikiError = false;
		try {
			const r = await invoke<ScWikiCraftDetail | null>('scwiki_craft_detail', {
				itemName: name
			});
			// Race-Schutz: ein schnellerer Detailwechsel gewinnt.
			if (detail && detail.item.name === name) {
				scwiki = r;
				if (r?.blueprint) {
					// K6-Contract: QualityStats liest die Aspekte/Summary.
					detail.blueprintAspects = r.blueprint.aspects ?? null;
					detail.blueprintSummary = r.blueprint.summary_properties ?? null;
					// M8-R1-A: Snap-Punkte pro Material nachladen (Commodity-
					// Endpoint, UNION je Material; t_1dd162e3: Namen + BP-
					// UUID für den Raffinat→(Ore)-Fallback). Fehler degra-
					// dieren still (Toggle zeigt dann keine Einrastpunkte).
					void loadSnaps(detail.blueprintAspects ?? []);
				}
			}
		} catch {
			// kein Hard-Fail — SC-Wiki ist Bereicherung. Aber NICHT stumm:
			// sichtbarer Hinweis + Retry im Detail (Review-Blocker M8-K5).
			scwikiError = true;
		}
	}
	function retryScwiki() {
		if (detail) void loadScwiki(detail.item.name);
	}
	function closeDetail() {
		detail = null;
		detailItemId = '';
		scwiki = null;
		scwikiError = false;
		detailError = '';
	}

	// ---- M8-R1-A: individuelle Material-Regler -------------------------------
	type QualitySnapResponse = { uuid: string; values: number[] };

	// Snap-Punkte je Material nachladen (M8-R2: SCMDB qualityBands,
	// Union je Material) und per normiertem Materialnamen indizieren.
	// Kein Match im Mining-Index → leere Liste → Regler rastet auf
	// 500-only (500 ist der Always-Punkt, Snap gilt für ALLE).
	async function loadSnaps(aspects: Aspect[]) {
		// t_24961acd: UUID ist OPTIONAL (Cache-Einträge aus alten Builds
		// tragen teils kein uuid-Feld — reales A03-Blueprint) — die Auflösung
		// ist NAME-basiert (M8-R2). Requests über ALLE Aspekte mit
		// nicht-leerem Input-Namen; das Backend antwortet POSITIONAL
		// (`snaps[i]` ↔ Anfrage i), das Zuordnen läuft über den Index, nicht
		// über die (leerbare) UUID. Doppelte Materialnamen innerhalb eines
		// Blueprints sind unkritisch: dieselbe Ressource → dieselben Werte.
		const reqs: { uuid: string; name: string }[] = [];
		for (const a of aspects ?? []) {
			const name = (a.input?.name ?? '').trim();
			if (!name || !materialKey(name)) continue;
			reqs.push({ uuid: a.input?.uuid?.trim() ?? '', name });
		}
		if (reqs.length === 0) return;
		try {
			const snaps = await invoke<QualitySnapResponse[]>('scmdb_quality_snaps', {
				uuids: reqs.map((r) => r.uuid),
				names: reqs.map((r) => r.name)
			});
			const map: Record<string, number[]> = {};
			for (let i = 0; i < (snaps?.length ?? 0); i++) {
				const key = materialKey(reqs[i]?.name ?? '');
				if (key && snaps[i].values?.length) map[key] = [...snaps[i].values];
			}
			snapPoints = map;
		} catch {
			snapPoints = {}; // 500-only-Raster für ALLE Materialien
		}
	}

	const rowKey = (c: CostEntry, idx: number) => `${c.material}:${idx}`;
	// User: Standard = 500 (Mitte) — auch als Snaps-Wert für ALLE Materialien
	// (unter rowSnaps). Qualitäts-Mitte ist der neutrale Startwert.
	const DEFAULT_ROW_QUALITY = 500;
	const rowQuality = (c: CostEntry, idx: number): number =>
		matQual[rowKey(c, idx)] ?? DEFAULT_ROW_QUALITY;
	// Aspekt einer Kosten-Zeile (per normiertem Materialnamen gematcht).
	const rowAspect = (c: CostEntry): Aspect | null =>
		(detail?.blueprintAspects ?? []).find(
			(a) => materialKey(a.input?.name) === materialKey(c.material)
		) ?? null;
	// Snap-Punkte + Toggle-Status einer Zeile (M8-R2): SCMDB-Bänder
	// sind das Raster; 500 (Mitte) ist IMMER mit drin (Always-Punkt) —
	// auch ohne Band-Match rastet der Regler dann auf 500-only (das
	// 0.5.88-Verhalten „frei bei leer“ ist zurückgenommen).
	const rowSnaps = (c: CostEntry): number[] => {
		const bands = snapPoints[materialKey(c.material)] ?? [];
		const set = new Set<number>([DEFAULT_ROW_QUALITY, ...bands]);
		return [...set].sort((a, b) => a - b);
	};
	const rowSnapOn = (c: CostEntry, idx: number): boolean =>
		snapOn[rowKey(c, idx)] ?? snapDefault();
	// Hartes Clamp (M8-R2): bei Snap-an physische Regler-Grenzen
	// min = bands[0], max = bands[-1] (ohne Bands: 500/500 — der Regler
	// sitzt fest auf dem Always-Punkt); Snap-aus: das alte 0–1000.
	const rowBounds = (c: CostEntry, idx: number): [number, number] => {
		if (!rowSnapOn(c, idx)) return [0, 1000];
		const bands = snapPoints[materialKey(c.material)] ?? [];
		if (bands.length === 0) return [DEFAULT_ROW_QUALITY, DEFAULT_ROW_QUALITY];
		return [bands[0], bands[bands.length - 1]];
	};
	function toggleSnap(c: CostEntry, idx: number) {
		const key = rowKey(c, idx);
		const next = !rowSnapOn(c, idx);
		snapOn = { ...snapOn, [key]: next };
		setSnapDefault(next); // letzter Toggle = neuer Default (nice-to-have)
		if (next) {
			// Hartes Clamp (M8-R2): Wert in die Band-Grenzen ziehen + auf den
			// nächsten Rasterpunkt rasten (min/max sitzen zusätzlich als
			// Slider-Attribute physisch).
			const [min, max] = rowBounds(c, idx);
			const v = Math.min(max, Math.max(min, rowQuality(c, idx)));
			const snapped = nearestSnap(rowSnaps(c), v);
			if (snapped !== null) matQual = { ...matQual, [key]: snapped };
		}
	}
	// t_e261c7f4 Scroll-Erhalt: der Detail-Panel-Scrollstand darf beim
	// Regeln NICHT springen — scrollTop vor dem $state-Update sichern und
	// nach dem DOM-Flush (rAF) zurücksetzen, falls der Browser durch
	// Layout-/Focus-Änderungen neu geankert hat.
	let detailEl = $state<HTMLElement | null>(null);
	// M8-R3: Der Custom-Slider (CraftQualitySlider) rastet selbst (Hysterese)
	// — hier kommt nur noch der gerastete Wert an; der el-Write-Zirkus
	// (0.5.85–0.5.87 WebKit-Drag-Bug) entfällt strukturell: der Thumb ist
	// ein div, dessen Position rein aus dem State folgt.
	function setRowQuality(c: CostEntry, idx: number, v: number) {
		const scroller = detailEl;
		const top = scroller === null ? null : scroller.scrollTop;
		const key = rowKey(c, idx);
		matQual = { ...matQual, [key]: v };
		if (scroller !== null && top !== null) {
			requestAnimationFrame(() => {
				scroller.scrollTop = top;
			});
		}
	}

	// Gematchte Aspekte + deren Aspekt-Qualitäten (aus den Einzel-Reglern).
	// t_e261c7f4 Layout-Stabilität: ALLE Kosten-Zeilen tragen eine Qualität —
	// unberührte Regler zählen 0 (wie rowQuality), nicht „nicht gesetzt“.
	// Vorher war die Map leer, bis der erste Regler bewegt wurde → der
	// Stat-Block (Modifier-Zeilen + combined) wuchs beim ersten Move und
	// schob die Materialliste aus dem sichtbaren Bereich.
	const aspectQualities = $derived.by(() => {
		const map = new Map<string, number>();
		if (!detail?.blueprintAspects?.length) return map;
		const mq = new Map<string, number>();
		for (const [idx, c] of detail.costs.entries()) {
			mq.set(materialKey(c.material), rowQuality(c, idx));
		}
		for (const a of detail.blueprintAspects) {
			const q = aspectQuality(a, mq);
			if (q !== null) map.set(a.key, q);
		}
		return map;
	});
	// M8-R4 (t_e8f4f57d): Gesamt-Qualität = MITTELWERT der eingestellten
	// Qualitäten über ALLE Kosten-Zeilen (unberührte Regler zählen mit ihrem
	// Default 500 — wie angezeigt). Nicht mehr min über die Aspekte (User-
	// Befund: falsches Modell); die Aspekt-Qualitäten oben bleiben UNBERÜHRT
	// und dienen weiter der Effekt-/Modifier-Berechnung (QualityStats).
	const combined = $derived.by(() => {
		if (!detail) return null;
		return meanQuality(detail.costs.map((c, idx) => rowQuality(c, idx)));
	});
	// Gemischte Effekte: eine Änderung am Regler → %-Abweichung je Eigenschaft
	// (nur der betroffene Aspekt, K6-Modell).
	const rowChanges = (c: CostEntry, idx: number) => {
		const a = rowAspect(c);
		if (!a) return [];
		return statChanges([a], detail?.blueprintSummary ?? null, rowQuality(c, idx));
	};

	// ---- Anzeige-Helfer ------------------------------------------------------
	// Einheiten FARBLICH GETRENNT — nie mischen (Einheiten-Regel M8-K1).
	const UNIT_COLORS: Record<string, string> = {
		SCU: '#e0a83e', // Amber — Volumen (Schiff-Erze)
		gems: '#7ec8e3', // Cyan — Stück (FPS/Ground-Vehicle-Erze)
		items: '#b8bfc9' // Neutral — Stück (Nicht-Erze)
	};
	const unitName = (u: string) => tr(`craft.unit_${u.toLowerCase()}`);
	// Craft-Zeit kompakt: 960s → „16 min“, 180s → „3 min“.
	function fmtTime(sec: number): string {
		const h = Math.floor(sec / 3600);
		const m = Math.floor((sec % 3600) / 60);
		const s = sec % 60;
		if (h > 0) return m > 0 ? `${h} h ${m} min` : `${h} h`;
		if (m > 0) return s > 0 ? `${m} min ${s} s` : `${m} min`;
		return `${sec} s`;
	}
	const fmtQty = (q: number) =>
		Number.isInteger(q) ? String(q) : String(Math.round(q * 100) / 100);
	const catName = (c: string | null) => c ?? '—';

	// ---- M8-K5 Anzeige-Helfer (SC-Wiki-Detail) -------------------------------
	// Beschreibung de_EN → en_EN-Fallback (Spec), sonstige Sprache → en.
	const wikiDesc = $derived.by(() => {
		const it = scwiki?.item;
		if (!it) return null;
		const de = it.description_de?.trim();
		const en = it.description_en?.trim();
		if (currentLang() === 'de') return de || en || null;
		return en || de || null;
	});
	// Kopf-Zeilen nur für EXISTIERENDE Felder (keine „—"-Zeilen, Spec).
	const wikiProps = $derived.by(() => {
		const it = scwiki?.item;
		if (!it) return [] as { label: string; value: string }[];
		const out: { label: string; value: string }[] = [];
		if (it.size !== null && it.size !== undefined)
			out.push({ label: $t('craft.item_size'), value: `S${it.size}` });
		if (it.grade) out.push({ label: $t('craft.grade'), value: it.grade });
		if (it.class) out.push({ label: $t('craft.item_class'), value: it.class });
		if (it.mass !== null && it.mass !== undefined)
			out.push({ label: $t('craft.mass'), value: `${fmtQty(it.mass)} kg` });
		if (it.ammunition && it.ammunition.size !== null && it.ammunition.size !== undefined)
			out.push({ label: $t('craft.ammo'), value: `S${it.ammunition.size}` });
		return out;
	});
	const bp = $derived(scwiki?.blueprint ?? null);
	const bpAvailableByDefault = $derived(bp?.is_available_by_default ?? false);
	const vw = $derived(scwiki?.item.vehicle_weapon ?? null);
	const ammo = $derived(scwiki?.item.ammunition ?? null);
	// Chance-Label: 1 = Guaranteed (Label kommt aus den Daten), sonst %.
	const chanceLabel = (chance: number | null) =>
		chance !== null && chance !== undefined && chance < 1
			? `${Math.round(chance * 100)} %`
			: null;
	const isGuaranteed = (chance: number | null) =>
		chance === null || chance === undefined || chance >= 1;

	// M8-K3: Bedarfserz-Klick → Mining-Sprung (Callback des Parents).
	function openOre(id: string | null) {
		if (id) onOpenOre?.(id);
	}

	// ---- Keyboard (Esc-Kaskade wie Mining: Komponente behandelt selbst) ------
	function onKey(e: KeyboardEvent) {
		// M8-K3: keep-alive (während des Mining-Sprungs versteckt gemountet)
		// darf die Esc-Kaskade des sichtbaren Mining-Modus nicht stehlen.
		if (hidden) return;
		if (e.key !== 'Escape') return;
		e.preventDefault();
		e.stopPropagation();
		if (detail) {
			closeDetail();
			return;
		}
		onexit();
	}
</script>

<svelte:window onkeydown={onKey} />

<div class="crafting" role="region" aria-label={$t('nav.crafting')}>
	{#if checked && status?.enabled && status?.key_set}
		<!-- M9-B: Tab-Zeile Craft ↔ Listen (Listen-Engine, t_36778bc3) -->
		<!-- M9-R1: gemeinsame Reiter-Komponente (Segmented Control) statt eigener .ctab-Optik -->
		<Tabs
			items={[
				{ id: 'craft', label: $t('craft.tab_craft') },
				{ id: 'lists', label: $t('craft.tab_lists') }
			]}
			active={tab}
			onSelect={(id) => (tab = id as 'craft' | 'lists')}
		/>
	{/if}
	{#if !checked}
		<div class="cnote">{$t('mining.loading')}</div>
	{:else if error}
		<div class="cerr">{error}</div>
	{:else if status && !status.enabled}
		<!-- Opt-in: Modus ist in der Settings ausgeschaltet -->
		<div class="ccard">
			<div class="cc-h">{$t('craft.disabled_title')}</div>
			<p class="cc-body">{$t('craft.disabled_body')}</p>
		</div>
	{:else if status && !status.key_set}
		<!-- Key-Missing-State wie Mining: Self-Service-Erklärung (ToS: eigener celd_-Key) -->
		<div class="ccard">
			<div class="cc-h">{$t('mining.key_missing_title')}</div>
			<p class="cc-body">{$t('mining.key_missing_body')}</p>
			<a
				class="cc-link"
				href="https://strata.celd.space/api-keys"
				target="_blank"
				rel="noopener noreferrer">{$t('mining.key_missing_link')}</a
			>
			<div class="cc-foot">{$t('mining.key_missing_settings')}</div>
		</div>
	{:else}
		{#if tab === 'lists'}
			<!-- M9-B: Listen-Tab — CRUD + Aggregation + Abhaken (CraftLists) -->
			<CraftLists onOpenOre={openOre} onOpenMiningList={onOpenMiningList} onOpenInventory={onOpenInventory} />
		{:else}
		<div class="ccols">
			<!-- Linke Spalte: Kategorie-Chips + Item-Liste -->
			<div class="ccol">
				<div class="cfrow">
					<div class="cf">
						<span class="cfl">{$t('craft.filter_category')}</span>
						<div class="cfchips">
							<!-- „Alle" bleibt PILL/BUTTON und ist der Filter-Reset (User-P5) -->
							<button
								type="button"
								class="cchip"
								class:is-on={oberFilter === 'all' && subFilter === 'all'}
								onclick={resetFilters}>{$t('common.all')}</button
							>
							<!-- Oberkategorie: CUSTOM-Dropdown (SelectCombo) — NIE natives <select> -->
							<SelectCombo
								bind:value={oberFilter}
								onselect={setOber}
								showAllOnFocus
								options={oberOptions}
								ariaLabel={$t('craft.filter_category')}
								placeholder={$t('craft.filter_category')}
							/>
							<!-- Unterkategorie: nur Optionen der gewählten Oberkategorie; bei
								Alle/Mission Items (keine Unterkategorie) komplett ausgeblendet -->
							{#if subOptions.length}
								<SelectCombo
									bind:value={subFilter}
									onselect={(v: string) => (subFilter = v)}
									showAllOnFocus
									options={subOptions}
									ariaLabel={$t('craft.filter_sub')}
									placeholder={$t('craft.filter_sub')}
								/>
							{/if}
							<!-- M9-A: Verfügbarkeits-Filter (Besitz) — 3-Zustands-Chip im
							     Kreis alle → verfügbar → nicht verfügbar (Spec M9). -->
							<button
								type="button"
								class="cchip cchip-own"
								class:is-on={ownFilter !== 'all'}
								class:is-own-not={ownFilter === 'not_owned'}
								onclick={cycleOwnFilter}
								title={$t('craft.own_filter_title')}
							>
								{ownFilter === 'owned'
									? $t('craft.own_filter_owned')
									: ownFilter === 'not_owned'
										? $t('craft.own_filter_not_owned')
										: $t('craft.own_filter_all')}
							</button>
						</div>
					</div>
				</div>
				{#if loading}
					<div class="cnote">{$t('mining.loading')}</div>
				{:else}
					<div class="clist" role="listbox" aria-label={$t('nav.crafting')}>
						{#each filteredItems.slice(0, shown) as i (i.id)}
							<div class="crow" role="option" aria-selected="false" tabindex="0">
								<button
									type="button"
									class="crow-main"
									onclick={() => void openDetail(i)}
									title={i.name}
								>
									<span class="crow-name">{i.name}</span>
									{#if i.category}
										<span class="cbadge">{catName(i.category)}</span>
									{/if}
								</button>
								<!-- M9-A: Besitz-Haken (Blueprint-Pool) — eigener Button,
								     toggelt ohne Detail-Öffnung, lokal persistiert. -->
								<button
									type="button"
									class="crow-own"
									class:is-on={ownedIds.has(i.id)}
									aria-pressed={ownedIds.has(i.id)}
									title={ownedIds.has(i.id) ? $t('craft.own_on') : $t('craft.own_off')}
									onclick={(e) => {
										e.stopPropagation();
										void toggleOwned(i.id);
									}}>✓</button
								>
							</div>
						{:else}
							<div class="cnote">{$t('mining.empty')}</div>
						{/each}
					</div>
					{#if filteredItems.length > shown}
						<button type="button" class="cmore" onclick={() => (shown += STEP)}>
							{$t('craft.load_more', { n: String(Math.min(STEP, filteredItems.length - shown)) })}
						</button>
					{/if}
					<div class="ccount">{$t('craft.count', { n: String(filteredItems.length) })}</div>
				{/if}
			</div>

			<!-- Rechte Spalte: Item-Detail (Materialliste + Planungsrechner) -->
			<div class="cdetail" bind:this={detailEl}>
				{#if detailLoading}
					<div class="cnote">{$t('mining.loading')}</div>
				{:else if detailError}
					<div class="cerr">{detailError}</div>
				{:else if detail}
					<div class="cd-head">
						<button
							type="button"
							class="cback"
							onclick={closeDetail}
							title={$t('common.back_esc')}
							aria-label={$t('common.back')}>←</button
						>
						<span class="cd-title">{detail.item.name}</span>
				<!-- Union-Resolution (M9-B + M9-A): Blueprint-Picker mit aktuellen
				     Regler-Qualitäten an eine Liste hängen + Besitz-Haken ganz rechts. -->
					<div class="cl-pick-wrap">
						<button type="button" class="cl-add" onclick={toggleListPick}>
							＋ {$t('lists.add_to_list')}
						</button>
						{#if listPickOpen}
							<div class="cl-pick" role="menu">
								{#each myLists as l (l.id)}
									<button
										type="button"
										class="cl-pick-row"
										role="menuitem"
										onclick={() => void addToList(l.id)}
									>
										<span class="cl-pick-name">{l.name}</span>
										<span class="cl-pick-n">{l.item_count}</span>
									</button>
								{:else}
									<div class="cl-pick-hint">{$t('lists.empty')}</div>
								{/each}
								<div class="cl-pick-new">
									<input
										class="cl-pick-input"
										type="text"
										bind:value={newListName}
										placeholder={$t('lists.new_list_ph')}
										aria-label={$t('lists.new_list_ph')}
										onkeydown={(e) => e.key === 'Enter' && void createAndAdd()}
									/>
									<button type="button" class="cl-pick-go" onclick={() => void createAndAdd()}>
										{$t('lists.new_list')}
									</button>
								</div>
								{#if listNote}<div class="cl-pick-note">{listNote}</div>{/if}
							</div>
						{/if}
					</div>
					<!-- M9-A: Besitz-Haken oben rechts im Detail (Spec M9). -->
					<button
						type="button"
						class="cd-own"
						class:is-on={ownedIds.has(detailItemId)}
						aria-pressed={ownedIds.has(detailItemId)}
						title={ownedIds.has(detailItemId) ? $t('craft.own_on') : $t('craft.own_off')}
						onclick={() => void toggleOwned(detailItemId)}>✓</button
					>
					</div>
					<div class="cd-sub">
						{#if detail.item.category}<span class="cbadge">{catName(detail.item.category)}</span>{/if}
						<span class="cd-time">{$t('craft.craft_time')}: {fmtTime(detail.craftTime)}</span>
					</div>

					<!-- M8-K5: Item-Detail-Kopf — nur EXISTIERENDE Felder (keine
					     „—"-Zeilen); Standard-Blaupause-Badge bei default-Unlock. -->
					{#if wikiProps.length > 0 || bpAvailableByDefault}
						<div class="cd-props">
							{#each wikiProps as p (p.label)}
								<span class="cprop"><span class="cprop-l">{p.label}</span> {p.value}</span>
							{/each}
							{#if bpAvailableByDefault}
								<span class="cprop is-default">{$t('craft.default_blueprint')}</span>
							{/if}
						</div>
					{/if}
					<!-- M8-R1-C (t_b6ee9f14): Stats-Vollanzeige direkt nach Size/Mass/Ammo —
					     default | neu je Zeile (auch für Q-unbeeinflusste → „unaffected"). -->
					{#if vw || ammo || detail.blueprintAspects?.length}
						<QualityStats
							aspects={detail.blueprintAspects ?? []}
							summary={detail.blueprintSummary ?? null}
							aspectQualities={aspectQualities}
							weapon={vw}
							ammo={ammo}
						/>
					{/if}
					{#if wikiDesc}
						<!-- kompakt klappbar (Spec) — Beschreibung de_EN/en_EN -->
						<details class="cd-desc">
							<summary>{$t('craft.description')}</summary>
							<p>{wikiDesc}</p>
						</details>
					{/if}

					{#if detail.costs.length === 0}
						<div class="cnote">{$t('craft.no_materials')}</div>
					{:else}
						<div class="cmats" role="list">
							<!-- M8-R4: Einheiten-Legende am Materiallisten-Kopf (war im
							     entfernten Planungsrechner-Kopf zu Hause). -->
							<div class="cmat-legend">{$t('craft.units_legend', { gems: String(1000) })}</div>
								{#each detail.costs as c, idx (c.material + ':' + idx)}
									<div class="cmat" role="listitem">
									{#if c.oreId && onOpenOre}
									<!-- M8-K3 (t_a8088d06): Bedarfserz klickbar → Mining-Modus
									     mit dem Erz vorgewählt (Orte-pro-Erz-Detail). -->
									<button
										type="button"
										class="cm-name cm-ore"
										onclick={() => openOre(c.oreId)}
										title={$t('craft.open_in_mining')}
									>{c.material}</button>
									{:else}
										<span class="cm-name">{c.material}</span>
									{/if}
									<span class="cm-qty">
										{fmtQty(c.quantity)}
										<span class="cm-unit" style={`--uc:${UNIT_COLORS[c.unit] ?? '#b8bfc9'}`}
											>{unitName(c.unit)}</span
										>
									</span>
									</div>
									{#if rowAspect(c)}
									<!-- M8-R1-A: individueller Material-Qualitätsregler (0–1000) —
									     M8-R4: Balken + Wert + Snap-Toggle in EINER Zeile, der
									     Effekttext in einer EIGENEN Zeile darunter (voll aus-
									     geschrieben, mehrzeilig erlaubt). -->
									<div class="cmq-slider" class:is-snap={rowSnapOn(c, idx)}>
										<!-- M8-R3: Custom-Slider — Track fix 0–1000 (500 immer
										     optische Mitte), Magnet-Snapping mit Hysterese im
										     Pointer-Handler, harte Grenzen via bounds-Clamp. -->
										<CraftQualitySlider
											bind:value={() => rowQuality(c, idx), (v) => setRowQuality(c, idx, v)}
											snaps={rowSnaps(c)}
											snapOn={rowSnapOn(c, idx)}
											bounds={rowBounds(c, idx)}
											ariaLabel={`${$t('craft.mat_quality')}: ${c.material}`}
										/>
										<span class="cmq-val">{rowQuality(c, idx)}</span>
																				<!-- t_9107f006: Toggle IMMER an der Regler-Zeile sichtbar
										(M8-R2: nie disabled — ohne Band-Match rastet der Regler
										auf 500-only, der Title zeigt das Raster). -->
										<button
										type="button"
										class="cmq-snap"
										class:is-on={rowSnapOn(c, idx)}
										class:is-empty={snapPoints[materialKey(c.material)] === undefined}
										aria-pressed={rowSnapOn(c, idx)}
										title={rowSnapOn(c, idx)
											? $t('craft.snap_on') + ' [' + rowSnaps(c).join(', ') + ']'
											: $t('craft.snap_off')}
										onclick={() => toggleSnap(c, idx)}
										>{$t('craft.snap')}</button>
										</div>
										<!-- M8-R4: Effektzeile EIGENSTÄNDIG unter der Regler-Zeile —
										Labels voll ausgeschrieben, mehrzeilig erlaubt, kein
										Ellipsis-Cut mehr. -->
										<div class="cmq-changes">
										{#each rowChanges(c, idx) as ch (ch.property_key)}
											<span
												class="cmq-change"
												class:is-better={ch.better === true}
												class:is-worse={ch.better === false}>{ch.label} {ch.pct === null ? '—' : (ch.pct > 0 ? '+' : '') + ch.pct + '%'}</span
											>
										{/each}
										</div>
										{/if}
							{/each}
							</div>

							<!-- M8-R4: Gesamt-Qualität = Mittelwert der Kosten-Zeilen-Regler
							(s. Script-Teil) — nur noch der read-only Balken, kein Kopf,
							keine required-Zeile, keine Legende (Einheiten-Legende hängt
							am Materiallisten-Kopf). Die Stats-Vollanzeige (QualityStats)
							steht oben direkt nach Size/Mass/Ammo (M8-R1-C). -->
							<div class="cplanner">
								<div class="cp-row">
									<label class="cp-label" for="craft-target-q">{$t('craft.combined_quality')}</label>
									<input
										id="craft-target-q"
										class="cp-slider"
										type="range"
										min="0"
										max="1000"
										step="1"
										disabled
										value={combined ?? 0}
									/>
									<span class="cp-val">{combined === null ? '—' : String(Math.round(combined))}</span>
								</div>
							</div>
							{/if}

							<!-- M8-K5: Blaupausen-Missionen — „Guaranteed" hervorgehoben,
							     Chance-Missionen mit %-Angabe, reward_scope als Badge;
							     Titel + Debug-Name kompakt (Spec). -->
							{#if bp?.unlocking_missions_grouped?.length}
								<div class="cmissions">
									<div class="cp-head">{$t('craft.missions_title')}</div>
									{#each bp.unlocking_missions_grouped as g, gi (g.label + ':' + gi)}
										<div class="cmg" class:is-guaranteed={isGuaranteed(g.chance)}>
											<div class="cmg-head">
												<span class="cmg-label">{g.label}</span>
												{#if chanceLabel(g.chance)}
													<span class="cmg-chance">{chanceLabel(g.chance)}</span>
												{/if}
											</div>
											{#each g.missions as m, mi (m.title + ':' + mi)}
												<div class="cm-mission">
													<span class="cm-title"
														>{m.title}{#if (m.count ?? 1) > 1} ×{m.count}{/if}</span
													>
													{#if m.debug_name}
														<span class="cm-debug" title={m.debug_name}>{m.debug_name}</span>
													{/if}
													{#if m.reward_scope}
														<span class="cbadge">{m.reward_scope}</span>
													{/if}
												</div>
											{/each}
										</div>
									{/each}
							</div>
						{/if}

						<!-- M8-R1-C: dismantle_returns NACH den Missionen (Spec-Reihenfolge). -->
						{#if bp?.dismantle_returns?.length}
							<div class="cdism">
								<span class="cdl">{$t('craft.dismantle_returns')}</span>
								{#each bp.dismantle_returns as d, di (d.name + ':' + di)}
									<span class="cdi"
										>{d.name}{' '}{#if d.quantity_scu !== null && d.quantity_scu !== undefined}
											{fmtQty(d.quantity_scu)} SCU{/if}</span
									>
								{/each}
							</div>
						{/if}
					{:else}
						<div class="cnote cempty">{$t('craft.detail_hint')}</div>
					{/if}
					</div>
					</div>
					{/if}
					{/if}
					{#if detail && scwikiError}
		<div class="cscwiki-err">
			{$t('craft.scwiki_error')}
			<!-- Kein Unicode-Glyph (⟳) als Button-Label — Font-Fallback zeichnet
			     Tofu/„D“ in Headless-Webviews. Beschrifteter i18n-Text statt Icon. -->
			<button class="cscwiki-retry" onclick={retryScwiki}>
				{$t('craft.scwiki_retry')}
			</button>
		</div>
	{/if}
	<div class="cattr">
		{attribution || ATTRIBUTION_FALLBACK}{#if scwiki}
			<span class="cattr-scwiki"> · {scwiki.attribution}</span>{/if}
	</div>
</div>

<style>
	/* Muster: Mining.svelte (Overlay-Stil, Variablen des Shells) */
	/* M9-B: Listen-Popover im Detail. Tab-Zeile = gemeinsame Tabs.svelte (M9-R1). */
	.cl-pick-wrap {
		position: relative;
		margin-left: auto;
	}
	.cl-add {
		background: rgba(255, 255, 255, 0.07);
		border: 1px solid rgba(255, 255, 255, 0.18);
		border-radius: 999px;
		color: inherit;
		padding: 3px 12px;
		font: inherit;
		font-size: 11px;
		cursor: pointer;
		white-space: nowrap;
	}
	.cl-pick {
		position: absolute;
		top: calc(100% + 6px);
		right: 0;
		z-index: 20;
		min-width: 200px;
		max-height: 260px;
		overflow: auto;
		background: #141a21;
		border: 1px solid rgba(255, 255, 255, 0.18);
		border-radius: 10px;
		padding: 8px;
		display: flex;
		flex-direction: column;
		gap: 6px;
		box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5);
	}
	.cl-pick-row {
		display: flex;
		gap: 8px;
		align-items: center;
		background: rgba(255, 255, 255, 0.05);
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 6px;
		color: inherit;
		padding: 5px 9px;
		font: inherit;
		font-size: 12px;
		cursor: pointer;
		text-align: left;
	}
	.cl-pick-name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.cl-pick-n {
		opacity: 0.6;
		font-size: 11px;
	}
	.cl-pick-hint {
		opacity: 0.6;
		font-size: 11px;
		padding: 2px 4px;
	}
	.cl-pick-new {
		display: flex;
		gap: 6px;
		border-top: 1px solid rgba(255, 255, 255, 0.12);
		padding-top: 8px;
	}
	.cl-pick-input {
		flex: 1;
		min-width: 0;
		background: rgba(255, 255, 255, 0.06);
		border: 1px solid rgba(255, 255, 255, 0.14);
		border-radius: 6px;
		color: inherit;
		padding: 4px 8px;
		font: inherit;
		font-size: 12px;
	}
	.cl-pick-go {
		background: var(--accent, #4aa3ff);
		border: none;
		border-radius: 6px;
		color: #0b0f14;
		padding: 4px 10px;
		font: inherit;
		font-size: 11px;
		font-weight: 600;
		cursor: pointer;
		white-space: nowrap;
	}
	.cl-pick-note {
		font-size: 11px;
		opacity: 0.85;
	}
	.crafting {
		display: flex;
		flex-direction: column;
		min-height: 0;
		flex: 1;
		color: var(--fg, #d7dde5);
	}
	.ccols {
		display: grid;
		grid-template-columns: minmax(240px, 1fr) minmax(280px, 1.2fr);
		gap: 12px;
		flex: 1;
		min-height: 0;
		overflow: auto;
	}
	.ccol,
	.cdetail {
		min-width: 0;
		min-height: 0;
		display: flex;
		flex-direction: column;
	}
	/* M8-R1-C (t_b6ee9f14): Detail-Panel scrollt INNERHALB der Spalte
	   (overflow-y:auto) + untere Padding, damit der Zierramen und die
	   letzte Sektion (dismantle) nicht an der Panel-Kante abgeschnitten
	   werden. Root-Cause des Abschneide-Bugs ist die nicht-scrollende
	   Shell-Kette — gefixt in app.css (.crafting-keep, s. dort). */
	.cdetail {
		overflow-y: auto;
		/* M9-H (t_7709c7f2, Punkt 6): horizontales Padding — Blueprint-Details
		   klebten am Panel-Rand (gleiche Problematik wie die Listen-Tabs). */
		padding: 2px 10px 10px 2px;
	}
	.cnote {
		padding: 10px 2px;
		opacity: 0.75;
		font-size: 12.5px;
	}
	.cempty {
		padding-top: 24px;
		text-align: center;
	}
	.cerr {
		padding: 10px 2px;
		color: #ff8a80;
		font-size: 12.5px;
	}

	/* Key-Missing / Disabled-Card (Muster Mining .mcard) */
	.ccard {
		border: 1px solid var(--line, #2a2f3a);
		border-radius: 10px;
		padding: 14px 16px;
		background: var(--card, rgba(255, 255, 255, 0.03));
		max-width: 460px;
	}
	.cc-h {
		font-weight: 700;
		font-size: 14px;
		margin-bottom: 6px;
	}
	.cc-body {
		margin: 0 0 10px;
		font-size: 12.5px;
		line-height: 1.5;
		opacity: 0.9;
	}
	.cc-link {
		color: var(--acc, #e0a83e);
		font-size: 12.5px;
		text-decoration: underline;
	}
	.cc-foot {
		margin-top: 10px;
		font-size: 11.5px;
		opacity: 0.65;
	}

	/* Filter-Chips (dynamisch, kein <select> — Fokusduell) */
	.cfrow {
		margin-bottom: 8px;
	}
	.cf {
		margin-bottom: 6px;
	}
	.cfl {
		display: block;
		font-size: 10.5px;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		opacity: 0.6;
		margin-bottom: 4px;
	}
	.cfchips {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
	}
	.cchip {
		border: 1px solid var(--line, #2a2f3a);
		background: transparent;
		color: inherit;
		border-radius: 999px;
		padding: 3px 10px;
		font-size: 11.5px;
		cursor: pointer;
	}
	.cchip:hover {
		border-color: var(--acc, #e0a83e);
	}
	.cchip.is-on {
		background: var(--acc, #e0a83e);
		border-color: var(--acc, #e0a83e);
		color: #14161a;
		font-weight: 600;
	}

	/* Item-Liste: kompakte Zeilen */
	.clist {
		display: flex;
		flex-direction: column;
		min-height: 0;
		overflow-y: auto;
	}
	.crow {
		display: flex;
		border-bottom: 1px solid var(--line-soft, rgba(255, 255, 255, 0.05));
	}
	.crow-main {
		flex: 1;
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
		background: transparent;
		border: 0;
		color: inherit;
		text-align: left;
		padding: 7px 6px;
		font-size: 12.5px;
		cursor: pointer;
	}
	.crow-main:hover {
		background: var(--hover, rgba(255, 255, 255, 0.05));
	}
	.crow-name {
		flex: 1;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.cbadge {
		flex: none;
		font-size: 10px;
		padding: 1px 7px;
		border-radius: 999px;
		border: 1px solid var(--line, #2a2f3a);
		opacity: 0.8;
		white-space: nowrap;
	}
	.cmore {
		margin-top: 8px;
		border: 1px solid var(--line, #2a2f3a);
		background: transparent;
		color: inherit;
		border-radius: 8px;
		padding: 6px 12px;
		font-size: 12px;
		cursor: pointer;
	}
	.cmore:hover {
		border-color: var(--acc, #e0a83e);
	}
	.ccount {
		margin-top: 6px;
		font-size: 10.5px;
		opacity: 0.55;
	}

	/* Detail */
	.cd-head {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.cback {
		background: transparent;
		border: 1px solid var(--line, #2a2f3a);
		color: inherit;
		border-radius: 8px;
		width: 26px;
		height: 26px;
		cursor: pointer;
		font-size: 14px;
		line-height: 1;
	}
	.cback:hover {
		border-color: var(--acc, #e0a83e);
	}
	.cd-title {
		font-weight: 700;
		font-size: 14.5px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	/* M9-A: Besitz-Haken (Blueprint-Pool) — Liste + Detail oben rechts. */
	.crow-own,
	.cd-own {
		flex: none;
		background: transparent;
		border: 1px solid var(--line, #2a2f3a);
		color: inherit;
		border-radius: 8px;
		width: 26px;
		height: 26px;
		cursor: pointer;
		font-size: 13px;
		line-height: 1;
		opacity: 0.55;
	}
	.crow-own:hover,
	.cd-own:hover {
		border-color: var(--acc, #e0a83e);
		opacity: 1;
	}
	.crow-own.is-on,
	.cd-own.is-on {
		background: var(--acc, #e0a83e);
		border-color: var(--acc, #e0a83e);
		color: #14161a;
		opacity: 1;
		font-weight: 700;
	}
	/* Detail: Haken rechtsbündig — Titel bleibt flexibel, Button drückt nach rechts. */
	.cd-own {
		margin-left: auto;
	}
	/* Verfügbarkeits-Chip: negativ-Zustand (nicht verfügbar) etwas gedämpft. */
	.cchip-own.is-own-not {
		border-style: dashed;
	}
	.cchip-own.is-own-not.is-on {
		background: transparent;
		border: 1px dashed var(--acc, #e0a83e);
		color: var(--acc, #e0a83e);
		font-weight: 600;
	}
	.cd-sub {
		display: flex;
		align-items: center;
		gap: 10px;
		margin: 6px 0 10px;
		font-size: 12px;
		opacity: 0.85;
	}
	.cd-time {
		opacity: 0.85;
	}

	/* Materialliste: Menge + EINHEIT farblich getrennt (nie mischen) */
	.cmats {
		display: flex;
		flex-direction: column;
		border: 1px solid var(--line, #2a2f3a);
		border-radius: 10px;
		overflow: hidden;
		/* t_8c0e6abc: overflow:hidden setzt die Flex-Min-Größe auf 0 — ohne
		   flex-shrink:0 kollabiert .cmats als Flex-Item der .cdetail-Column
		   auf ~0 und clippt die Regler-Zeilen (Höhen-Matrix im Fix-Card-Body). */
		flex-shrink: 0;
	}
	.cmat {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 7px 10px;
		font-size: 12.5px;
		border-bottom: 1px solid var(--line-soft, rgba(255, 255, 255, 0.05));
	}
	.cmat:last-child {
		border-bottom: 0;
	}
	/* M8-R4: Einheiten-Legende am Materiallisten-Kopf (dezent, wie cpl-units) */
	.cmat-legend {
		padding: 5px 10px 4px;
		font-size: 10.5px;
		opacity: 0.6;
		border-bottom: 1px solid var(--line-soft, rgba(255, 255, 255, 0.05));
	}
	.cm-name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	/* M8-K3: klickbares Bedarfserz — sieht aus wie der Name, hover verrät den Sprung. */
	.cm-ore {
		background: none;
		border: 0;
		padding: 0;
		font: inherit;
		color: inherit;
		cursor: pointer;
		text-align: left;
	}
	.cm-ore:hover,
	.cm-ore:focus-visible {
		text-decoration: underline;
		color: var(--acc, #e0a83e);
	}
	.cm-qty {
		flex: none;
		display: flex;
		align-items: baseline;
		gap: 5px;
		font-variant-numeric: tabular-nums;
	}
	.cm-unit {
		font-size: 10.5px;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		color: var(--uc, #b8bfc9);
		font-weight: 600;
	}
	/* M8-R1-A: individueller Material-Qualitätsregler unter der Zeile —
	   M8-R4: Regler-Zeile = Balken (feste 19rem-Breite, 500-Mitte pixel-
	   gleich bei allen Materialien) + Wert + Snap-Toggle; der Balken hat
	   die Zeile für sich, nichts verdrängt ihn mehr. */
	.cmq-slider {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 3px 10px 6px 14px;
		border-bottom: 1px solid var(--line-soft, rgba(255, 255, 255, 0.05));
	}
	.cmq-val {
		flex: none;
		width: 38px;
		text-align: right;
		font-size: 11px;
		font-variant-numeric: tabular-nums;
	}
	.cmq-snap {
		flex: none;
		font-size: 10px;
		padding: 1px 7px;
		border-radius: 8px;
		border: 1px solid var(--line, #2a2f3a);
		background: none;
		color: inherit;
		cursor: pointer;
		opacity: 0.65;
	}
	.cmq-snap.is-on {
		border-color: var(--acc, #e0a83e);
		color: var(--acc, #e0a83e);
		opacity: 1;
	}
	/* t_9107f006: ohne Quant-Werte dezenter, aber trotzdem sichtbar */
	.cmq-snap.is-empty {
		opacity: 0.35;
		border-style: dashed;
		cursor: default;
	}
	/* M8-R4: Effektzeile EIGENE Zeile unter der Regler-Zeile — Labels voll
	   ausgeschrieben, mehrzeilig erlaubt (flex-wrap), KEIN Ellipsis-Cut.
	   Die Regler-Zeilen sind isoliert (Balken + Wert + Toggle), die Effekt-
	   Zeile kann wachsen, ohne den Balken zu verdrängen; Scroll-Erhalt
	   (setRowQuality) stabilisiert die Position beim Regeln. */
	.cmq-changes {
		display: flex;
		flex-wrap: wrap;
		gap: 2px 10px;
		padding: 2px 10px 6px 14px;
		font-size: 10.5px;
		font-variant-numeric: tabular-nums;
		border-bottom: 1px solid var(--line-soft, rgba(255, 255, 255, 0.05));
	}
	.cmq-change {
		opacity: 0.8;
	}
	.cmq-change.is-better {
		color: #4be08a;
		opacity: 1;
	}
	.cmq-change.is-worse {
		color: #ff6b6b;
		opacity: 1;
	}

	/* M8-R4: Gesamt-Qualität — nur noch der read-only Balken (Mittelwert) */
	.cplanner {
		margin-top: 12px;
		border: 1px solid var(--line, #2a2f3a);
		border-radius: 10px;
		padding: 10px 12px;
	}
	.cp-row {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.cp-label {
		font-size: 11.5px;
		opacity: 0.8;
		white-space: nowrap;
	}
	.cp-slider {
		flex: 1;
		accent-color: var(--acc, #e0a83e);
	}
	.cp-val {
		min-width: 4ch;
		text-align: right;
		font-variant-numeric: tabular-nums;
		font-size: 12.5px;
		font-weight: 600;
	}

	/* Attribution-Footer (ToS-Pflicht) — IMMER sichtbar */
	.cattr {
		margin-top: 8px;
		font-size: 10.5px;
		opacity: 0.55;
		text-align: right;
	}
	/* M8-K5: zweite Quelle (SC-Wiki) im Footer — gleiche Zeile, kein Block */
	/* M8-K5-Fix: SC-Wiki-Fehler-Hinweis + Retry (degradiert sichtbar) */
	.cscwiki-err {
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 4px 10px;
		font-size: 11px;
		color: var(--muted, #8b93a1);
	}
	.cscwiki-retry {
		background: none;
		border: 1px solid var(--border, #333a45);
		border-radius: 4px;
		color: inherit;
		font-size: 11px;
		padding: 2px 8px;
		cursor: pointer;
	}
	.cscwiki-retry:hover {
		border-color: var(--accent, #5aa9e6);
	}
	.cattr-scwiki {
		white-space: nowrap;
	}

	/* ========= M8-K5: Item-Detail-Kopf / Stats / Missionen ========= */
	/* Kopf-Props: kleine Label-Wert-Chips, nur vorhandene Felder */
	.cd-props {
		display: flex;
		flex-wrap: wrap;
		gap: 5px;
		margin: 0 0 8px;
	}
	.cprop {
		display: inline-flex;
		align-items: baseline;
		gap: 4px;
		font-size: 11px;
		padding: 2px 8px;
		border: 1px solid var(--line, #2a2f3a);
		border-radius: 6px;
		opacity: 0.85;
	}
	.cprop-l {
		font-size: 9.5px;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		opacity: 0.65;
	}
	.cprop.is-default {
		border-color: var(--acc, #e0a83e);
		color: var(--acc, #e0a83e);
		opacity: 1;
		font-weight: 600;
	}
	/* Beschreibung kompakt klappbar */
	.cd-desc {
		margin: 0 0 10px;
		font-size: 12px;
	}
	.cd-desc summary {
		cursor: pointer;
		font-size: 10.5px;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		opacity: 0.6;
		width: fit-content;
	}
	.cd-desc summary:hover {
		opacity: 0.9;
	}
	.cd-desc p {
		margin: 6px 0 0;
		line-height: 1.45;
		opacity: 0.85;
		white-space: pre-line;
	}
	/* dismantle_returns: kompakte Zeile unter der Materialliste */
	.cdism {
		display: flex;
		align-items: baseline;
		flex-wrap: wrap;
		gap: 4px 10px;
		margin: 6px 2px 0;
		font-size: 11px;
		opacity: 0.8;
	}
	.cdl {
		font-size: 9.5px;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		opacity: 0.65;
	}
	.cdi {
		font-variant-numeric: tabular-nums;
	}
	.cp-head {
		font-weight: 700;
		font-size: 12.5px;
		margin-bottom: 8px;
	}
	/* Blaupausen-Missionen */
	.cmissions {
		margin-top: 12px;
		border: 1px solid var(--line, #2a2f3a);
		border-radius: 10px;
		padding: 10px 12px;
	}
	.cmg {
		padding: 6px 0;
		border-top: 1px solid var(--line-soft, rgba(255, 255, 255, 0.05));
	}
	.cmg:first-of-type {
		border-top: 0;
		padding-top: 0;
	}
	/* „Guaranteed" hervorgehoben (Spec) */
	.cmg.is-guaranteed .cmg-label {
		color: var(--acc, #e0a83e);
		font-weight: 700;
	}
	.cmg-head {
		display: flex;
		align-items: baseline;
		gap: 8px;
		margin-bottom: 4px;
	}
	.cmg-label {
		font-size: 12px;
		font-weight: 600;
	}
	.cmg-chance {
		font-size: 11px;
		opacity: 0.75;
		font-variant-numeric: tabular-nums;
	}
	.cm-mission {
		display: flex;
		align-items: baseline;
		gap: 8px;
		padding: 2px 0 2px 8px;
		font-size: 11.5px;
	}
	.cm-title {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.cm-debug {
		font-size: 10px;
		opacity: 0.5;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--mono, ui-monospace, monospace);
	}
</style>
