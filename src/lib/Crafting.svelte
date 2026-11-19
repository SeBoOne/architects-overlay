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
	//                      Kosten; je Material grün/rot. KEINE Stat-Vorhersage —
	//                      Machbarkeits-Check (Stat-Lücke dokumentiert im Plan).
	//
	// NICHT gebaut (API-Lücke, Plan „Offen“): Blaupausen-Missionen,
	// Größe/Muni/Klasse/Grade-Details, echte Stat-Berechnung.
	//
	// Formular-Konvention (Sebo): Liste startet VOLL, Filter per Haupt-Suchfeld
	// + Chips; KEIN natives <select> (Fokusduell — 2026-11-19 gelernt).
	// Key-Missing-State wie Mining: Erklär-Card + Link
	// https://strata.celd.space/api-keys. Attribution-Footer IMMER sichtbar.
	import { t, tr, trError } from './i18n';
	import { invoke } from '@tauri-apps/api/core';

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
	};
	type StrataStatus = {
		enabled: boolean;
		key_set: boolean;
		key_ref: string;
		tier_default: number;
	};

	let { onexit, filter = $bindable('') }: { onexit: () => void; filter?: string } = $props();

	// ---- State ---------------------------------------------------------------
	let status = $state<StrataStatus | null>(null);
	let checked = $state(false); // Status-Load abgeschlossen
	let items = $state<Item[]>([]);
	let attribution = $state('');
	let loading = $state(false);
	let error = $state('');

	let catFilter = $state<string>('all'); // dynamisch aus den Daten
	// Detail
	let detail = $state<CraftCostResponse | null>(null);
	let detailLoading = $state(false);
	let detailError = $state('');
	// Qualitäts-Planungsrechner
	let targetQuality = $state(0);
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
			if (catFilter !== 'all' && (i.category ?? '—') !== catFilter) return false;
			return true;
		});
	});
	// Kategorie-Chips DYNAMISCH aus den Daten: 'all' + jede vorkommende
	// category (null läuft als „—“-Slot — kein hartkodiertes Vokabular).
	const categories = $derived([...new Set(items.map((i) => i.category ?? '—'))].sort());
	// Filter-/Kategorie-Wechsel: Liste wieder von vorn rendern.
	$effect(() => {
		void filter;
		void catFilter;
		shown = STEP;
	});

	// ---- Detail --------------------------------------------------------------
	async function openDetail(item: Item) {
		detailLoading = true;
		detailError = '';
		detail = null;
		targetQuality = 0; // neuer Artikel → Planer neutral starten
		try {
			const r = await invoke<CraftCostResponse>('strata_craft_cost', { item: item.id });
			detail = r;
			attribution = r.attribution || ATTRIBUTION_FALLBACK;
		} catch (e) {
			detailError = trError(e);
		}
		detailLoading = false;
	}
	function closeDetail() {
		detail = null;
		detailError = '';
	}

	// ---- Qualitäts-PLANUNGSRECHNER (Machbarkeits-Check, KEINE Stat-Vorhersage)
	// Geforderte MINDEST-Input-Qualität = max(minQuality) über alle Kosten;
	// fehlende Angaben (null) tragen nicht bei. 0 ist gültige Daten.
	const requiredQuality = $derived(
		detail ? Math.max(0, ...detail.costs.map((c) => c.minQuality ?? 0)) : 0
	);
	const meets = (mq: number | null) => mq === null || targetQuality >= mq;

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

	// ---- Keyboard (Esc-Kaskade wie Mining: Komponente behandelt selbst) ------
	function onKey(e: KeyboardEvent) {
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
							<button
								type="button"
								class="cchip"
								class:is-on={catFilter === 'all'}
								onclick={() => (catFilter = 'all')}>{$t('common.all')}</button
							>
							{#each categories as c (c)}
								<button
									type="button"
									class="cchip"
									class:is-on={catFilter === c}
									onclick={() => (catFilter = catFilter === c ? 'all' : c)}>{catName(c)}</button
								>
							{/each}
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

					{#if detail.costs.length === 0}
						<div class="cnote">{$t('craft.no_materials')}</div>
					{:else}
						<div class="cmats" role="list">
							{#each detail.costs as c, idx (c.material + ':' + idx)}
								<div
									class="cmat"
									class:is-ok={meets(c.minQuality)}
									class:is-fail={!meets(c.minQuality)}
									role="listitem"
								>
									<span class="cm-name">{c.material}</span>
									<span class="cm-qty">
										{fmtQty(c.quantity)}
										<span class="cm-unit" style={`--uc:${UNIT_COLORS[c.unit] ?? '#b8bfc9'}`}
											>{unitName(c.unit)}</span
										>
									</span>
									<span
										class="cmq"
										class:is-req={c.minQuality !== null && requiredQuality > 0}
										title={c.minQuality === null
											? $t('craft.min_quality_none')
											: $t('craft.min_quality') + ' ' + c.minQuality}
									>
										{c.minQuality === null ? '—' : String(c.minQuality)}
									</span>
								</div>
							{/each}
						</div>

						<!-- Qualitäts-PLANUNGSRECHNER — Machbarkeits-Check, KEINE
						     Stat-Vorhersage (Item-Stats/Qualitäts-Kurven: API-Lücke, Plan). -->
						<div class="cplanner">
							<div class="cp-head">{$t('craft.planner_title')}</div>
							<div class="cp-row">
								<label class="cp-label" for="craft-target-q">{$t('craft.target_quality')}</label>
								<input
									id="craft-target-q"
									class="cp-slider"
									type="range"
									min="0"
									max="1000"
									step="1"
									bind:value={targetQuality}
								/>
								<span class="cp-val">{targetQuality}</span>
							</div>
							<div class="cp-result">
								{$t('craft.required_quality')}: <strong>{requiredQuality}</strong>
							</div>
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
				{:else}
					<div class="cnote cempty">{$t('craft.detail_hint')}</div>
				{/if}
			</div>
		</div>
	{/if}
	<div class="cattr">{attribution || ATTRIBUTION_FALLBACK}</div>
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
</style>
