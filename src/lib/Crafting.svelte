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
	//                      als Legenden-Anmerkung), minQuality-Badge („—“ bei
	//                      null), Craft-Zeit.
	//   4. Qualitäts-PLANUNGSRECHNER — Ziel-Slider 0–1000 → geforderte
	//                      MINDEST-Input-Qualität = max(minQuality) über alle
	//                      Kosten; je Material grün/rot. M8-K6: dazu der echte
	//                      Stat-Rechner (QualityStats) — Modifier-Interpolation
	//                      als %-Abweichung, sobald ein SC-Wiki-Blueprint mit
	//                      Modifier-Daten gematcht hat (sonst still weg).
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
	import {
		aspectQuality,
		materialKey,
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
		hidden = false // M8-K3: keep-alive während des Mining-Sprungs (Tastatur stumm)
	}: {
		onexit: () => void;
		filter?: string;
		onOpenOre?: ((oreId: string) => void) | null;
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

	const ATTRIBUTION_FALLBACK = 'Data from celd.space / Strata Mining Tools';

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
		if (status?.enabled && status?.key_set) void loadItems();
	}

	async function loadItems() {
		loading = true;
		error = '';
		try {
			const r = await invoke<ItemsResponse>('strata_items');
			items = [...r.items].sort((a, b) => a.name.localeCompare(b.name));
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
		shown = STEP;
	});

	// ---- Detail --------------------------------------------------------------
	async function openDetail(item: Item) {
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
					// Endpoint, UNION je Material). Fehler degradieren still
					// (Toggle zeigt dann keine Einrastpunkte).
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
		scwiki = null;
		scwikiError = false;
		detailError = '';
	}

	// ---- Qualitäts-PLANUNGSRECHNER (Machbarkeits-Check, KEINE Stat-Vorhersage)
	// Geforderte MINDEST-Input-Qualität = max(minQuality) über alle Kosten;
	// fehlende Angaben (null) tragen nicht bei. 0 ist gültige Daten.
	const requiredQuality = $derived(
		detail ? Math.max(0, ...detail.costs.map((c) => c.minQuality ?? 0)) : 0
	);

	// ---- M8-R1-A: individuelle Material-Regler -------------------------------
	type QualitySnapResponse = { uuid: string; values: number[] };

	// Snap-Punkte je Material nachladen (Commodity-Endpoint, Union je
	// Material) und per normiertem Materialnamen indizieren.
	async function loadSnaps(aspects: Aspect[]) {
		const byUuid = new Map<string, string>(); // uuid → norm. Materialname
		for (const a of aspects ?? []) {
			const uuid = a.input?.uuid?.trim();
			const key = materialKey(a.input?.name);
			if (uuid && key) byUuid.set(uuid, key);
		}
		if (byUuid.size === 0) return;
		try {
			const snaps = await invoke<QualitySnapResponse[]>('scwiki_quality_snaps', {
				uuids: [...byUuid.keys()]
			});
			const map: Record<string, number[]> = {};
			for (const s of snaps ?? []) {
				const key = byUuid.get(s.uuid);
				if (key && s.values?.length) map[key] = [...s.values];
			}
			snapPoints = map;
		} catch {
			snapPoints = {}; // still weg — Regler bleiben frei regelbar
		}
	}

	const rowKey = (c: CostEntry, idx: number) => `${c.material}:${idx}`;
	const rowQuality = (c: CostEntry, idx: number): number => matQual[rowKey(c, idx)] ?? 0;
	// Zeile grün/rot am EIGENEN Reglerwert (User-P2: Badge klebt am Regler).
	const rowMeets = (c: CostEntry, idx: number) =>
		c.minQuality === null || rowQuality(c, idx) >= c.minQuality;
	// Aspekt einer Kosten-Zeile (per normiertem Materialnamen gematcht).
	const rowAspect = (c: CostEntry): Aspect | null =>
		(detail?.blueprintAspects ?? []).find(
			(a) => materialKey(a.input?.name) === materialKey(c.material)
		) ?? null;
	// Snap-Punkte + Toggle-Status einer Zeile.
	const rowSnaps = (c: CostEntry): number[] => snapPoints[materialKey(c.material)] ?? [];
	const rowSnapOn = (c: CostEntry, idx: number): boolean =>
		rowSnaps(c).length > 0 && (snapOn[rowKey(c, idx)] ?? snapDefault());
	function toggleSnap(c: CostEntry, idx: number) {
		const key = rowKey(c, idx);
		const next = !rowSnapOn(c, idx);
		snapOn = { ...snapOn, [key]: next };
		setSnapDefault(next); // letzter Toggle = neuer Default (nice-to-have)
		if (next) {
			const snapped = nearestSnap(rowSnaps(c), rowQuality(c, idx));
			if (snapped !== null) matQual = { ...matQual, [key]: snapped };
		}
	}
	function setRowQuality(c: CostEntry, idx: number, v: number) {
		const key = rowKey(c, idx);
		const snapped = rowSnapOn(c, idx) ? nearestSnap(rowSnaps(c), v) : v;
		matQual = { ...matQual, [key]: snapped ?? v };
	}

	// Gematchte Aspekte + deren Aspekt-Qualitäten (aus den Einzel-Reglern).
	const aspectQualities = $derived.by(() => {
		const map = new Map<string, number>();
		if (!detail?.blueprintAspects?.length) return map;
		const mq = new Map<string, number>();
		for (const [k, v] of Object.entries(matQual)) {
			const name = k.slice(0, k.lastIndexOf(':'));
			if (!name || typeof v !== 'number' || !Number.isFinite(v)) continue;
			mq.set(materialKey(name), v);
		}
		for (const a of detail.blueprintAspects) {
			const q = aspectQuality(a, mq);
			if (q !== null) map.set(a.key, q);
		}
		return map;
	});
	// Gesamt-Qualität = min über die Aspekt-Qualitäten (schwächster Aspekt
	// bestimmt — Rechenmodell im quality-calc-Header; null = nichts berechenbar).
	const combined = $derived.by(() => {
		const qs = [...aspectQualities.values()];
		return qs.length ? Math.min(...qs) : null;
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
			<div class="cdetail">
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
							{#each detail.costs as c, idx (c.material + ':' + idx)}
								<div
									class="cmat"
									class:is-ok={rowMeets(c, idx)}
									class:is-fail={!rowMeets(c, idx)}
									role="listitem"
								>
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
									<span
										class="cmq"
										class:is-req={c.minQuality !== null && rowQuality(c, idx) < c.minQuality}
										title={c.minQuality === null
											? $t('craft.min_quality_none')
											: $t('craft.min_quality') + ' ' + c.minQuality}
									>
										{c.minQuality === null ? '—' : String(c.minQuality)}
									</span>
								</div>
								{#if rowAspect(c)}
									<!-- M8-R1-A: individueller Material-Qualitätsregler (0–1000)
									     mit minQuality-Badge (grün/rot am eigenen Wert), Snap-
									     Toggle (quality_quantized_values) und %-Abweichungen. -->
									<div class="cmq-slider" class:is-snap={rowSnapOn(c, idx)}>
										<input
											class="cmq-range"
											type="range"
											min="0"
											max="1000"
											step="1"
											aria-label={`${$t('craft.mat_quality')}: ${c.material}`}
											value={rowQuality(c, idx)}
											oninput={(e) => setRowQuality(c, idx, Number(e.currentTarget.value))}
										/>
										<span class="cmq-val" class:is-ok={rowMeets(c, idx)} class:is-fail={!rowMeets(c, idx)}
											>{rowQuality(c, idx)}</span
										>
										{#if rowSnaps(c).length > 0}
											<button
												type="button"
												class="cmq-snap"
												class:is-on={rowSnapOn(c, idx)}
												aria-pressed={rowSnapOn(c, idx)}
												title={rowSnapOn(c, idx)
													? $t('craft.snap_on') + ' [' + rowSnaps(c).join(', ') + ']'
													: $t('craft.snap_off')}
												onclick={() => toggleSnap(c, idx)}
											>{$t('craft.snap')}</button>
										{/if}
										<span class="cmq-changes">
											{#each rowChanges(c, idx) as ch (ch.property_key)}
												<span
													class="cmq-change"
													class:is-better={ch.better === true}
													class:is-worse={ch.better === false}>{ch.label} {ch.pct === null ? '—' : (ch.pct > 0 ? '+' : '') + ch.pct + '%'}</span
												>
											{/each}
										</span>
									</div>
								{/if}
							{/each}
							</div>

							<!-- M8-K5: dismantle_returns klein unter der Materialliste. -->
							{#if bp?.dismantle_returns?.length}
								<div class="cdism">
									<span class="cdl">{$t('craft.dismantle_returns')}</span>
									{#each bp.dismantle_returns as d, di (d.name + ':' + di)}
										<span class="cdi"
											>{d.name}{#if d.quantity_scu !== null && d.quantity_scu !== undefined}
												{fmtQty(d.quantity_scu)} SCU{/if}</span
										>
									{/each}
								</div>
							{/if}

							<!-- M8-K5: Stat-Block — vehicle_weapon/ammunition-Details nur
							     wenn vorhanden (Spec: damages-Liste, rpm, range, capacity). -->
							{#if vw || ammo}
								<div class="cstats">
									<div class="cp-head">{$t('craft.weapon_stats')}</div>
									{#if vw}
										<div class="cs-grid">
											{#if vw.damage_per_shot !== null && vw.damage_per_shot !== undefined}
												<span class="cs-item"
													><span class="cs-l">{$t('craft.dmg_per_shot')}</span>
													{fmtQty(vw.damage_per_shot)}</span
												>
											{/if}
											{#if vw.rpm !== null && vw.rpm !== undefined}
												<span class="cs-item"
													><span class="cs-l">{$t('craft.rpm')}</span>
													{fmtQty(vw.rpm)}</span
												>
											{/if}
											{#if vw.range !== null && vw.range !== undefined}
												<span class="cs-item"
													><span class="cs-l">{$t('craft.range')}</span>
													{fmtQty(vw.range)} m</span
												>
											{/if}
											{#if vw.capacity !== null && vw.capacity !== undefined}
												<span class="cs-item"
													><span class="cs-l">{$t('craft.capacity')}</span>
													{fmtQty(vw.capacity)}</span
												>
											{/if}
										</div>
										{#if vw.damages.length}
											<div class="cs-dmg">
												{#each vw.damages as d, idx (d.damage_type + ':' + idx)}
													<span class="cs-dmgrow"
														>{d.damage_type}: {d.damage === null || d.damage === undefined
															? '—'
															: fmtQty(d.damage)}</span
													>
												{/each}
											</div>
										{/if}
									{/if}
									{#if ammo}
										<div class="cs-grid">
											{#if ammo.speed !== null && ammo.speed !== undefined}
												<span class="cs-item"
													><span class="cs-l">{$t('craft.ammo_speed')}</span>
													{fmtQty(ammo.speed)} m/s</span
												>
											{/if}
											{#if ammo.range !== null && ammo.range !== undefined}
												<span class="cs-item"
													><span class="cs-l">{$t('craft.range')}</span>
													{fmtQty(ammo.range)} m</span
												>
											{/if}
											{#if ammo.capacity !== null && ammo.capacity !== undefined}
												<span class="cs-item"
													><span class="cs-l">{$t('craft.capacity')}</span>
													{fmtQty(ammo.capacity)}</span
												>
											{/if}
											{#if ammo.lifetime !== null && ammo.lifetime !== undefined}
												<span class="cs-item"
													><span class="cs-l">{$t('craft.ammo_lifetime')}</span>
													{fmtQty(ammo.lifetime)} s</span
												>
											{/if}
										</div>
									{/if}
								</div>
							{/if}

							<!-- Qualitäts-PLANUNGSRECHNER — Machbarkeits-Check + (M8-K6)
							     echter Stat-Rechner via QualityStats (s. o.). -->
						<div class="cplanner">
							<div class="cp-head">{$t('craft.planner_title')}</div>
							<!-- M8-R1-A: Gesamt-Qualität = Anzeige-/Mischwert aus den
							     individuellen Reglern (min über Aspekte, s. quality-calc) —
							     der alte Gesamt-Slider bleibt sichtbar, ist aber read-only. -->
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
							<div class="cp-result">
								{$t('craft.required_quality')}: <strong>{requiredQuality}</strong>
							</div>
							<!-- M8-K6/R1-A: echter Stat-Rechner über die individuellen
							     Aspekt-Qualitäten — nur wenn ein SC-Wiki-Blueprint mit
							     Modifier-Daten gematcht hat (sonst still weg, kein Hard-Fail). -->
							{#if detail.blueprintAspects?.length}
								<QualityStats
									aspects={detail.blueprintAspects}
									summary={detail.blueprintSummary ?? null}
									aspectQualities={aspectQualities}
								/>
							{/if}
							<p class="cp-note">{$t('craft.planner_note')}</p>
								<div class="cp-legend">
									<span class="cpl-ok">✓ {$t('craft.q_ok')}</span>
									<span class="cpl-fail">✗ {$t('craft.q_fail')}</span>
									<span class="cpl-units">
										{$t('craft.units_legend', { gems: String(1000) })}
									</span>
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
				{:else}
					<div class="cnote cempty">{$t('craft.detail_hint')}</div>
				{/if}
			</div>
		</div>
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
	/* Planungsrechner-Färbung: grün = Ziel-Qualität ≥ minQuality, rot = darunter */
	.cmat.is-ok {
		border-left: 3px solid #4be08a;
	}
	.cmat.is-fail {
		border-left: 3px solid #ff6b6b;
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
	.cmq {
		flex: none;
		width: 44px;
		text-align: center;
		font-size: 11px;
		padding: 1px 0;
		border-radius: 6px;
		border: 1px solid var(--line, #2a2f3a);
		opacity: 0.8;
		font-variant-numeric: tabular-nums;
	}
	.cmq.is-req {
		border-color: var(--acc, #e0a83e);
		opacity: 1;
	}
	/* M8-R1-A: individueller Material-Qualitätsregler unter der Zeile */
	.cmq-slider {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 3px 10px 6px 14px;
		border-bottom: 1px solid var(--line-soft, rgba(255, 255, 255, 0.05));
	}
	.cmq-slider:last-child {
		border-bottom: 0;
	}
	.cmq-range {
		flex: 1;
		min-width: 0;
		accent-color: var(--acc, #e0a83e);
	}
	.cmq-val {
		flex: none;
		width: 38px;
		text-align: right;
		font-size: 11px;
		font-variant-numeric: tabular-nums;
	}
	.cmq-val.is-ok {
		color: #4be08a;
	}
	.cmq-val.is-fail {
		color: #ff6b6b;
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
	.cmq-changes {
		flex: none;
		display: flex;
		gap: 6px;
		font-size: 10.5px;
		font-variant-numeric: tabular-nums;
	}
	.cmq-change {
		opacity: 0.75;
	}
	.cmq-change.is-better {
		color: #4be08a;
		opacity: 1;
	}
	.cmq-change.is-worse {
		color: #ff6b6b;
		opacity: 1;
	}

	/* Qualitäts-PLANUNGSRECHNER */
	.cplanner {
		margin-top: 12px;
		border: 1px solid var(--line, #2a2f3a);
		border-radius: 10px;
		padding: 10px 12px;
	}
	.cp-head {
		font-weight: 700;
		font-size: 12.5px;
		margin-bottom: 8px;
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
	.cp-result {
		margin-top: 8px;
		font-size: 12.5px;
	}
	.cp-result strong {
		color: var(--acc, #e0a83e);
	}
	.cp-note {
		margin: 8px 0 0;
		font-size: 11px;
		line-height: 1.45;
		opacity: 0.7;
	}
	.cp-legend {
		display: flex;
		flex-wrap: wrap;
		gap: 6px 14px;
		margin-top: 8px;
		font-size: 11px;
	}
	.cpl-ok {
		color: #4be08a;
	}
	.cpl-fail {
		color: #ff6b6b;
	}
	.cpl-units {
		opacity: 0.65;
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
	/* Stat-Block: vehicle_weapon/ammunition */
	.cstats {
		margin-top: 10px;
		border: 1px solid var(--line, #2a2f3a);
		border-radius: 10px;
		padding: 10px 12px;
	}
	.cs-grid {
		display: flex;
		flex-wrap: wrap;
		gap: 4px 16px;
	}
	.cs-item {
		display: inline-flex;
		align-items: baseline;
		gap: 5px;
		font-size: 12px;
		font-variant-numeric: tabular-nums;
	}
	.cs-l {
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		opacity: 0.6;
	}
	.cs-dmg {
		display: flex;
		flex-wrap: wrap;
		gap: 3px 12px;
		margin-top: 6px;
		font-size: 11px;
		opacity: 0.85;
		font-variant-numeric: tabular-nums;
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
