<script lang="ts">
	// Mining-Modus (M7-K2, t_bb3303d0) — F5-Modus im Rail/F-Keys-System.
	//
	// Datenquelle: Strata Public API via `strata_client` (M7-K1) — die Tauri-
	// Commands `strata_*` liefern cache-first Responses inkl. Pflicht-
	// Attribution. Diese Komponente ist der F5-View:
	//
	//   1. Erz-Suche     — Typeahead (Name/Slug, case-insensitiv), Filter
	//                      category (ship/fps) + tier; Liste alphabetisch;
	//                      Pin-Button „Signatur pinnen" pro Erz-Zeile
	//                      (Pin-Content `scan-signature`, Rendering in K3).
	//   2. Ort-Suche     — Typeahead, Filter system + type.
	//   3. Detail        — „Orte pro Erz" (ore-locations), gruppiert nach
	//                      Umgebung (space/surface/cave/exploration). Gruppen
	//                      sind NICHT untereinander vergleichbar (eigene
	//                      Skala je Gruppe — als Gruppentitel sichtbar).
	//                      Two-Ring-Dial je Ort: außen RANK (rankPct), innen
	//                      CONCENTRATION (segmentiert, Amber→Mint), Zentrum
	//                      Erz-Kürzel + Scan-Signatur. Exakte Werte im
	//                      Hover-Tooltip; 6-Stufen-Legende bleibt sichtbar.
	//
	// Formular-Konvention (Sebo): Typeaheads starten LEER mit Gesamtliste,
	// sind MAUS-klickbar und filtern auf Tasteneingabe.
	//
	// Key-Missing-State: kein `celd_`-Key im Keystore → Erklär-Card mit Link
	// https://strata.celd.space/api-keys. Attribution-Footer IMMER sichtbar,
	// wenn Strata-Daten angezeigt werden (ToS-Pflicht).
	import { t, tr, trError } from './i18n';
	import { invoke } from '@tauri-apps/api/core';

	type Ore = {
		id: string;
		name: string;
		slug: string;
		category: string | null; // 'ship' | 'fps' | 'ground_vehicle' — null-fähig
		tier: number | null; // null-check, nie truthiness
		scan_signature: number | null; // 0 ist gültige Daten
		instability: number | null;
		resistance: number | null;
	};
	type OresResponse = { ores: Ore[]; count: number; attribution: string };
	type Location = { id: string; name: string; slug: string; system: string; kind: string };
	type LocationsResponse = { locations: Location[]; count: number; attribution: string };
	type LocationRow = {
		location_id: string;
		location: string;
		system: string;
		rank_pct: number;
		rank: number;
		n: number;
		concentration: number;
		tier: number;
		tier_name: string;
	};
	type OreLocationsResponse = {
		ore: { id: string; name: string };
		sort: string;
		tier_legend: Record<string, string> | null;
		results: {
			space: LocationRow[];
			surface: LocationRow[];
			cave: LocationRow[];
			exploration: LocationRow[];
		};
		attribution: string;
	};
	type StrataStatus = {
		enabled: boolean;
		key_set: boolean;
		key_ref: string;
		tier_default: number;
	};

	let { onexit }: { onexit: () => void } = $props();

	// ---- State ---------------------------------------------------------------
	let status = $state<StrataStatus | null>(null);
	let checked = $state(false); // Status-Load abgeschlossen
	let ores = $state<Ore[]>([]);
	let locations = $state<Location[]>([]);
	let attribution = $state('');
	let loading = $state(false);
	let error = $state('');

	let view = $state<'ores' | 'locations'>('ores');
	// Erz-Suche
	let oreQuery = $state('');
	let catFilter = $state<'all' | 'ship' | 'fps'>('all');
	let tierFilter = $state(0); // 0 = alle, sonst 1..6
	// Ort-Suche
	let locQuery = $state('');
	let sysFilter = $state('all');
	let typeFilter = $state('all');
	// Detail
	let detail = $state<OreLocationsResponse | null>(null);
	let detailOre = $state<Ore | null>(null); // Scan-Signatur für das Zentrum-Badge
	let detailLoading = $state(false);
	let detailError = $state('');
	let sortParam = $state<'rank' | 'concentration'>('rank');
	// Pin-Feedback (Statuszeile unten, Muster f5Msg)
	let note = $state('');
	let noteTimer: ReturnType<typeof setTimeout> | null = null;

	const ATTRIBUTION_FALLBACK = 'Data from celd.space / Strata Mining Tools';

	// 6-Stufen-Farbrampe (Orange→Amber→Oliv→Gelbgrün→Hellgrün→Mint) —
	// absolute Bands (Tier 1–6 der API), überall gleich interpretierbar.
	const TIER_COLORS: Record<number, string> = {
		1: '#d97742',
		2: '#e0a83e',
		3: '#a8a23e',
		4: '#9dc451',
		5: '#6bd694',
		6: '#4be08a'
	};
	function tierColor(tier: number): string {
		return TIER_COLORS[Math.min(6, Math.max(1, tier))] ?? TIER_COLORS[1];
	}

	function flash(msg: string) {
		note = msg;
		if (noteTimer) clearTimeout(noteTimer);
		noteTimer = setTimeout(() => (note = ''), 3000);
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
		if (status?.enabled && status?.key_set) void loadReference();
	}

	async function loadReference() {
		loading = true;
		error = '';
		try {
			const [o, l] = await Promise.all([
				invoke<OresResponse>('strata_ores'),
				invoke<LocationsResponse>('strata_locations')
			]);
			ores = [...o.ores].sort((a, b) => a.name.localeCompare(b.name));
			locations = [...l.locations].sort((a, b) => a.name.localeCompare(b.name));
			attribution = o.attribution || ATTRIBUTION_FALLBACK;
		} catch (e) {
			error = trError(e);
		}
		loading = false;
	}

	$effect(() => {
		void loadStatus();
	});

	// ---- Filter/Typeahead (LEER = Gesamtliste, case-insensitiv Name/Slug) ----
	const oreMatches = (o: Ore, q: string) =>
		o.name.toLowerCase().includes(q) || o.slug.toLowerCase().includes(q);
	const filteredOres = $derived(
		ores.filter((o) => {
			const q = oreQuery.trim().toLowerCase();
			if (q && !oreMatches(o, q)) return false;
			if (catFilter !== 'all' && o.category !== catFilter) return false;
			if (tierFilter > 0 && o.tier !== tierFilter) return false;
			return true;
		})
	);
	const systems = $derived([...new Set(locations.map((l) => l.system))].sort());
	const locTypes = $derived([...new Set(locations.map((l) => l.kind))].sort());
	const filteredLocations = $derived(
		locations.filter((l) => {
			const q = locQuery.trim().toLowerCase();
			if (q && !(l.name.toLowerCase().includes(q) || l.slug.toLowerCase().includes(q)))
				return false;
			if (sysFilter !== 'all' && l.system !== sysFilter) return false;
			if (typeFilter !== 'all' && l.kind !== typeFilter) return false;
			return true;
		})
	);

	// ---- Detail --------------------------------------------------------------
	async function openDetail(ore: Ore) {
		detailOre = ore;
		detailLoading = true;
		detailError = '';
		detail = null;
		try {
			detail = await invoke<OreLocationsResponse>('strata_ore_locations', {
				ore: ore.id,
				sort: sortParam
			});
			attribution = detail.attribution || ATTRIBUTION_FALLBACK;
		} catch (e) {
			detailError = trError(e);
		}
		detailLoading = false;
	}
	function setSort(s: 'rank' | 'concentration') {
		if (sortParam === s || !detailOre) return;
		sortParam = s;
		void openDetail(detailOre);
	}
	function closeDetail() {
		detail = null;
		detailOre = null;
		detailError = '';
	}

	// Gruppen-Reihenfolge wie Strata: Haupt-Ziele → explorable Regions →
	// Caves → Surface. Leere Gruppen entfallen.
	const GROUPS = [
		['space', 'mining.group_space'],
		['exploration', 'mining.group_exploration'],
		['cave', 'mining.group_cave'],
		['surface', 'mining.group_surface']
	] as const;
	const detailGroups = $derived.by(() => {
		const d = detail;
		if (!d) return [];
		return GROUPS.map(([key, label]) => ({ key, label, rows: d.results[key] })).filter(
			(g) => g.rows.length > 0
		);
	});

	// ---- Pin „Signatur pinnen" (Pin-Content scan-signature, Rendering K3) ----
	async function pinSignature(ore: Ore, index: number) {
		const tierName = ore.tier ? String(ore.tier) : '—';
		try {
			const ok = await invoke<boolean>('pin_add', {
				view_id: 'mining',
				box_index: index,
				title: ore.name,
				kpi: ore.scan_signature === null ? null : String(ore.scan_signature),
				rows: [
					{ idx: 0, name: tr('mining.scan_signature'), value: ore.scan_signature === null ? '—' : String(ore.scan_signature) },
					{ idx: 1, name: tr('mining.tier'), value: tierName },
					{ idx: 2, name: tr('mining.category'), value: ore.category ?? '—' }
				],
				// Content-Deskriptor — K3 (t_cfea5bea) registriert den Renderer.
				content: {
					type: 'scan-signature',
					oreId: ore.id,
					oreName: ore.name,
					scanSignature: ore.scan_signature,
					tier: ore.tier,
					category: ore.category
				},
				key: `scan-signature:${ore.id}`
			});
			flash(tr(ok ? 'pins.added' : 'pins.already_pinned'));
		} catch (e) {
			flash(trError(e));
		}
	}

	// ---- Keyboard (Esc-Kaskade wie Inventory: Komponente behandelt selbst) ----
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

	function abbrev(name: string): string {
		return name
			.replace(/[^A-Za-z0-9 ]/g, ' ')
			.split(/\s+/)
			.filter(Boolean)
			.map((w) => w[0])
			.join('')
			.slice(0, 3)
			.toUpperCase() || '??';
	}
	function pct(v: number): number {
		return Math.max(0, Math.min(100, Math.round(v)));
	}
	function tooltipFor(row: LocationRow): string {
		const conc = Math.round(row.concentration * 1000) / 10;
		return `${tr('mining.rank')}: ${row.rank_pct}% · ${tr('mining.concentration')}: ${conc}% · ${row.tier_name}`;
	}
	const tierLegend = $derived([
		{ tier: 1, band: '< 1 %' },
		{ tier: 2, band: '1–4 %' },
		{ tier: 3, band: '4–8 %' },
		{ tier: 4, band: '8–15 %' },
		{ tier: 5, band: '15–25 %' },
		{ tier: 6, band: '≥ 25 %' }
	]);
</script>

<svelte:window onkeydown={onKey} />

<div class="mining" role="region" aria-label={$t('nav.mining')}>
	{#if !checked}
		<div class="mnote">{$t('mining.loading')}</div>
	{:else if error}
		<div class="merr">{error}</div>
	{:else if status && !status.enabled}
		<!-- Opt-in: Modus ist in der Settings ausgeschaltet -->
		<div class="mcard">
			<div class="mc-h">{$t('mining.disabled_title')}</div>
			<p class="mc-body">{$t('mining.disabled_body')}</p>
		</div>
	{:else if status && !status.key_set}
		<!-- Key-Missing-State: Self-Service-Erklärung (ToS: eigener celd_-Key) -->
		<div class="mcard">
			<div class="mc-h">{$t('mining.key_missing_title')}</div>
			<p class="mc-body">{$t('mining.key_missing_body')}</p>
			<a
				class="mc-link"
				href="https://strata.celd.space/api-keys"
				target="_blank"
				rel="noopener noreferrer">{$t('mining.key_missing_link')}</a
			>
			<div class="mc-foot">{$t('mining.key_missing_settings')}</div>
		</div>
	{:else}
		<div class="mcols">
			<!-- Linke Spalte: Erz-/Ort-Suche -->
			<div class="mcol">
				<div class="mtabs" role="tablist">
					<button
						type="button"
						class="mtab"
						class:is-on={view === 'ores'}
						role="tab"
						aria-selected={view === 'ores'}
						onclick={() => (view = 'ores')}>{$t('mining.tab_ores')}</button
					>
					<button
						type="button"
						class="mtab"
						class:is-on={view === 'locations'}
						role="tab"
						aria-selected={view === 'locations'}
						onclick={() => (view = 'locations')}>{$t('mining.tab_locations')}</button
					>
				</div>

				{#if view === 'ores'}
					<input
						class="mq"
						type="text"
						placeholder={$t('mining.ph_search_ore')}
						bind:value={oreQuery}
						onkeydown={(e) => e.stopPropagation()}
					/>
					<div class="mfrow">
						<div class="mf">
							<span class="mfl">{$t('mining.filter_category')}</span>
							<div class="mfchips">
								{#each ['all', 'ship', 'fps'] as c (c)}
									<button
										type="button"
										class="mchip"
										class:is-on={catFilter === c}
										onclick={() => (catFilter = c as 'all' | 'ship' | 'fps')}
										>{$t(c === 'all' ? 'mining.cat_all' : c === 'ship' ? 'mining.cat_ship' : 'mining.cat_fps')}</button
									>
								{/each}
							</div>
						</div>
						<div class="mf">
							<span class="mfl">{$t('mining.filter_tier')}</span>
							<div class="mfchips">
								<button
									type="button"
									class="mchip"
									class:is-on={tierFilter === 0}
									onclick={() => (tierFilter = 0)}>{$t('mining.tier_all')}</button
								>
								{#each [1, 2, 3, 4, 5, 6] as tv (tv)}
									<button
										type="button"
										class="mchip mtier"
										class:is-on={tierFilter === tv}
										style={`--tc:${tierColor(tv)}`}
										onclick={() => (tierFilter = tierFilter === tv ? 0 : tv)}>{tv}</button
									>
								{/each}
							</div>
						</div>
					</div>
					{#if loading}
						<div class="mnote">{$t('mining.loading')}</div>
					{:else}
						<div class="mlist" role="listbox" aria-label={$t('mining.tab_ores')}>
							{#each filteredOres as o, i (o.id)}
								<div class="mrow" role="option" aria-selected="false" tabindex="0">
									<button
										type="button"
										class="mrow-main"
										onclick={() => void openDetail(o)}
										title={o.name}
									>
										<span class="mrow-name">{o.name}</span>
										<span class="mrow-sub">{o.slug}</span>
										{#if o.tier}
											<span class="mtier-badge" style={`--tc:${tierColor(o.tier)}`}
												>{o.tier}</span
											>
										{/if}
									</button>
									<button
										type="button"
										class="mpin"
										title={$t('mining.pin_sig_title')}
										aria-label={$t('mining.pin_sig_title')}
										onclick={() => void pinSignature(o, i)}
									>
										<svg
											viewBox="0 0 24 24"
											fill="none"
											stroke="currentColor"
											stroke-width="2"
											stroke-linecap="round"
											stroke-linejoin="round"
										>
											<path d="M12 17v5" />
											<path d="M9 10.8 7 3h10l-2 7.8a5 5 0 0 1-6 0Z" />
										</svg>
									</button>
								</div>
							{:else}
								<div class="mnote">{$t('mining.empty')}</div>
							{/each}
						</div>
					{/if}
				{:else}
					<input
						class="mq"
						type="text"
						placeholder={$t('mining.ph_search_location')}
						bind:value={locQuery}
						onkeydown={(e) => e.stopPropagation()}
					/>
					<div class="mfrow">
						<div class="mf">
							<span class="mfl">{$t('mining.filter_system')}</span>
							<div class="mfchips">
								<button
									type="button"
									class="mchip"
									class:is-on={sysFilter === 'all'}
									onclick={() => (sysFilter = 'all')}>{$t('mining.sys_all')}</button
								>
								{#each systems as sy (sy)}
									<button
										type="button"
										class="mchip"
										class:is-on={sysFilter === sy}
										onclick={() => (sysFilter = sysFilter === sy ? 'all' : sy)}>{sy}</button
									>
								{/each}
							</div>
						</div>
						<div class="mf">
							<span class="mfl">{$t('mining.filter_type')}</span>
							<div class="mfchips">
								<button
									type="button"
									class="mchip"
									class:is-on={typeFilter === 'all'}
									onclick={() => (typeFilter = 'all')}>{$t('mining.type_all')}</button
								>
								{#each locTypes as ty (ty)}
									<button
										type="button"
										class="mchip"
										class:is-on={typeFilter === ty}
										onclick={() => (typeFilter = typeFilter === ty ? 'all' : ty)}>{ty}</button
									>
								{/each}
							</div>
						</div>
					</div>
					<div class="mlist" role="listbox" aria-label={$t('mining.tab_locations')}>
						{#each filteredLocations as l (l.id)}
							<div class="lrow" role="option" aria-selected="false" tabindex="0">
								<span class="mrow-name">{l.name}</span>
								<span class="mrow-sub">{l.system} · {l.kind}</span>
							</div>
						{:else}
							<div class="mnote">{$t('mining.empty')}</div>
						{/each}
					</div>
				{/if}
			</div>

			<!-- Rechte Spalte: Detail „Orte pro Erz" -->
			<div class="mdetail">
				{#if detailLoading}
					<div class="mnote">{$t('mining.loading')}</div>
				{:else if detailError}
					<div class="merr">{detailError}</div>
				{:else if detail}
					<div class="md-head">
						<button
							type="button"
							class="mback"
							onclick={closeDetail}
							title={$t('common.back_esc')}
							aria-label={$t('common.back')}>←</button
						>
						<span class="md-title">{detail.ore.name}</span>
						<div class="msort" role="group" aria-label={$t('mining.sort')}>
							<button
								type="button"
								class="mchip"
								class:is-on={sortParam === 'rank'}
								onclick={() => setSort('rank')}>{$t('mining.sort_rank')}</button
							>
							<button
								type="button"
								class="mchip"
								class:is-on={sortParam === 'concentration'}
								onclick={() => setSort('concentration')}>{$t('mining.sort_concentration')}</button
							>
						</div>
					</div>
					{#each detailGroups as g (g.key)}
						<div class="mgroup">
							<div class="mg-head">
								<span class="mg-title">{$t(g.label)}</span>
								<span class="mg-note">{$t('mining.group_note')}</span>
							</div>
							{#each g.rows as row (row.location_id)}
								<div class="drow" data-tip={tooltipFor(row)}>
									<svg class="dial" viewBox="0 0 48 48" aria-hidden="true">
										<!-- Außenring = RANK (durchgehender Bogen, Mint/Akzent) -->
										<circle class="dial-bg" cx="24" cy="24" r="20" pathLength="100" />
										<circle
											class="dial-rank"
											cx="24"
											cy="24"
											r="20"
											pathLength="100"
											style={`--p:${pct(row.rank_pct)}`}
										/>
										<!-- Innenring = CONCENTRATION (dünn, segmentiert, Tier-Farbe) -->
										<circle
											class="dial-bg"
											cx="24"
											cy="24"
											r="13"
											pathLength="100"
											style={`--tc:${tierColor(row.tier)}`}
										/>
										<circle
											class="dial-conc"
											cx="24"
											cy="24"
											r="13"
											pathLength="100"
											style={`--p:${pct(row.concentration * 100)};--tc:${tierColor(row.tier)}`}
										/>
										<text class="dial-abbr" x="24" y="23" text-anchor="middle"
											>{abbrev(detail.ore.name)}</text
										>
										<text class="dial-sig" x="24" y="33" text-anchor="middle">{detailOre?.scan_signature ?? '—'}</text>
									</svg>
									<div class="drow-main">
										<span class="drow-name">{row.location}</span>
										<span class="drow-sub">{row.system} · {row.n} {$t('mining.spawns')}</span>
									</div>
									<span class="mtier-badge" style={`--tc:${tierColor(row.tier)}`}
										>{row.tier_name}</span
									>
									<!-- Hover-Tooltip: exakte Werte (statische Dial-Labels entfallen) -->
									<span class="dtip" role="tooltip">{tooltipFor(row)}</span>
								</div>
							{/each}
						</div>
					{:else}
						<div class="mnote">{$t('mining.no_locations')}</div>
					{/each}
					<!-- Legende: 6 absolute Bands (= Tier 1–6), bleibt sichtbar -->
					<div class="mlegend">
						<div class="ml-title">{$t('mining.legend_title')}</div>
						{#each tierLegend as li (li.tier)}
							<div class="ml-item">
								<i style={`background:${tierColor(li.tier)}`}></i>
								<span class="ml-name">{$t(`mining.tier_${li.tier}`)}</span>
								<span class="ml-band">{li.band}</span>
							</div>
						{/each}
					</div>
				{:else}
					<div class="mnote">{$t('mining.detail_hint')}</div>
				{/if}
			</div>
		</div>

		{#if note}
			<div class="mtoast">{note}</div>
		{/if}
	{/if}

	<!-- Attribution-Footer (ToS-Pflicht): überall, wo Strata-Daten sichtbar sind -->
	<div class="mattr">{attribution || ATTRIBUTION_FALLBACK}</div>
</div>

<style>
	.mining {
		display: flex;
		flex-direction: column;
		min-height: 0;
		height: 100%;
		padding: 8px 4px 0 2px;
	}
	.mcols {
		display: flex;
		gap: 12px;
		flex: 1;
		min-height: 0;
	}
	.mcol {
		width: 320px;
		flex: 0 0 auto;
		display: flex;
		flex-direction: column;
		min-height: 0;
		gap: 8px;
	}
	.mdetail {
		flex: 1;
		min-width: 0;
		overflow-y: auto;
		padding-right: 4px;
	}
	/* Scrollbar-Konvention (Overlay) */
	.mdetail::-webkit-scrollbar,
	.mlist::-webkit-scrollbar {
		width: 8px;
	}
	.mdetail::-webkit-scrollbar-thumb,
	.mlist::-webkit-scrollbar-thumb {
		background: color-mix(in srgb, var(--accent) 35%, transparent);
		border-radius: 4px;
	}
	.mtabs {
		display: flex;
		gap: 6px;
	}
	.mtab {
		flex: 1;
		padding: 6px 10px;
		border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
		background: transparent;
		color: var(--fgdim, #9aa3b2);
		text-transform: uppercase;
		font-size: 11px;
		letter-spacing: 0.08em;
		cursor: pointer;
	}
	.mtab.is-on {
		background: color-mix(in srgb, var(--accent) 22%, transparent);
		color: var(--fg, #e8ecf3);
		border-color: var(--accent);
	}
	.mq {
		width: 100%;
		padding: 7px 10px;
		background: var(--panel2, #14161b);
		border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
		color: var(--fg, #e8ecf3);
		font: inherit;
		font-size: 13px;
	}
	.mfrow {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.mfl {
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.1em;
		color: var(--fgdim, #9aa3b2);
	}
	.mfchips {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
	}
	.mchip {
		padding: 2px 8px;
		font-size: 11px;
		border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
		background: transparent;
		color: var(--fgdim, #9aa3b2);
		cursor: pointer;
	}
	.mchip.is-on {
		background: color-mix(in srgb, var(--accent) 25%, transparent);
		color: var(--fg, #e8ecf3);
		border-color: var(--accent);
	}
	.mchip.mtier.is-on {
		border-color: var(--tc, var(--accent));
		color: var(--tc, var(--accent));
	}
	.mlist {
		overflow-y: auto;
		min-height: 0;
		flex: 1;
		display: flex;
		flex-direction: column;
		gap: 2px;
		padding-right: 4px;
	}
	.mrow,
	.lrow {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.mrow-main {
		flex: 1;
		display: flex;
		align-items: center;
		gap: 8px;
		background: transparent;
		border: 0;
		color: inherit;
		text-align: left;
		padding: 7px 6px;
		cursor: pointer;
		font: inherit;
		min-width: 0;
	}
	.mrow:hover .mrow-main,
	.mrow-main:hover {
		background: color-mix(in srgb, var(--accent) 14%, transparent);
	}
	.mrow-name {
		font-weight: 600;
		font-size: 13px;
		color: var(--fg, #e8ecf3);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.mrow-sub {
		font-size: 11px;
		color: var(--fgdim, #9aa3b2);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.lrow {
		padding: 7px 6px;
	}
	.lrow:hover {
		background: color-mix(in srgb, var(--accent) 14%, transparent);
	}
	.mtier-badge {
		padding: 1px 7px;
		border: 1px solid var(--tc, var(--accent));
		color: var(--tc, var(--accent));
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		white-space: nowrap;
	}
	.mpin {
		border: 0;
		background: transparent;
		color: var(--fgdim, #9aa3b2);
		cursor: pointer;
		padding: 4px;
		width: 26px;
		height: 26px;
		flex: 0 0 auto;
	}
	.mpin:hover {
		color: var(--accent);
		background: color-mix(in srgb, var(--accent) 18%, transparent);
	}
	.mpin svg {
		width: 15px;
		height: 15px;
	}
	.md-head {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-bottom: 8px;
	}
	.mback {
		border: 0;
		background: transparent;
		color: var(--fgdim, #9aa3b2);
		cursor: pointer;
		font-size: 15px;
		padding: 2px 8px;
	}
	.mback:hover {
		color: var(--accent);
	}
	.md-title {
		font-weight: 700;
		font-size: 15px;
		color: var(--fg, #e8ecf3);
		flex: 1;
	}
	.msort {
		display: flex;
		gap: 4px;
	}
	.mgroup {
		margin-bottom: 12px;
	}
	.mg-head {
		display: flex;
		align-items: baseline;
		gap: 8px;
		border-bottom: 1px solid color-mix(in srgb, var(--accent) 20%, transparent);
		padding-bottom: 3px;
		margin-bottom: 4px;
	}
	.mg-title {
		font-size: 11px;
		text-transform: uppercase;
		letter-spacing: 0.1em;
		color: var(--accent);
		font-weight: 700;
	}
	.mg-note {
		font-size: 10px;
		color: var(--fgdim, #9aa3b2);
	}
	.drow {
		position: relative;
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 6px 8px;
		border-radius: 3px;
	}
	.drow:hover {
		background: color-mix(in srgb, var(--accent) 10%, transparent);
	}
	.drow-main {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}
	.drow-name {
		font-weight: 600;
		font-size: 13px;
		color: var(--fg, #e8ecf3);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.drow-sub {
		font-size: 11px;
		color: var(--fgdim, #9aa3b2);
	}
	/* Two-Ring-Dial */
	.dial {
		width: 44px;
		height: 44px;
		flex: 0 0 auto;
	}
	.dial circle {
		fill: none;
		transform: rotate(-90deg);
		transform-origin: 50% 50%;
	}
	.dial-bg {
		stroke: color-mix(in srgb, var(--fgdim, #9aa3b2) 25%, transparent);
	}
	.dial-rank {
		stroke: var(--accent);
		stroke-width: 3.4;
		stroke-linecap: round;
		/* pathLength=100 → Füllgrad = --p (0..100), ohne Umfangs-Mathe */
		stroke-dasharray: var(--p, 0) 100;
	}
	.dial-conc {
		stroke: var(--tc, var(--accent));
		stroke-width: 2.2;
		/* segmentiert: Kurzhuben mit Lücken (Strata-Muster) */
		stroke-dasharray: calc(var(--p, 0) / 6) 1.2;
	}
	.dial-abbr {
		fill: var(--fg, #e8ecf3);
		font-size: 9px;
		font-weight: 700;
	}
	.dial-sig {
		fill: var(--fgdim, #9aa3b2);
		font-size: 7px;
	}
	/* Hover-Tooltip (exakte Werte) */
	.dtip {
		position: absolute;
		top: 100%;
		left: 56px;
		display: none;
		background: var(--panel, #11100d);
		border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
		color: var(--fg, #e8ecf3);
		font-size: 11px;
		padding: 4px 8px;
		white-space: nowrap;
		z-index: 5;
		pointer-events: none;
	}
	.drow:hover .dtip {
		display: block;
	}
	/* Legende */
	.mlegend {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 3px 10px;
		padding: 8px;
		border: 1px solid color-mix(in srgb, var(--accent) 18%, transparent);
		margin-top: 4px;
	}
	.ml-title {
		grid-column: 1 / -1;
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.1em;
		color: var(--fgdim, #9aa3b2);
	}
	.ml-item {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 11px;
	}
	.ml-item i {
		width: 10px;
		height: 10px;
		flex: 0 0 auto;
	}
	.ml-name {
		color: var(--fg, #e8ecf3);
	}
	.ml-band {
		color: var(--fgdim, #9aa3b2);
		margin-left: auto;
	}
	/* Cards/States */
	.mcard {
		border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
		padding: 16px;
		max-width: 460px;
		margin: 24px auto;
		background: color-mix(in srgb, var(--panel, #11100d) 80%, transparent);
	}
	.mc-h {
		font-weight: 700;
		color: var(--fg, #e8ecf3);
		margin-bottom: 6px;
	}
	.mc-body {
		color: var(--fgdim, #9aa3b2);
		font-size: 12px;
		line-height: 1.5;
		margin: 0 0 10px 0;
	}
	.mc-link {
		color: var(--accent);
		font-size: 12px;
	}
	.mc-foot {
		margin-top: 10px;
		font-size: 11px;
		color: var(--fgdim, #9aa3b2);
	}
	.mnote {
		color: var(--fgdim, #9aa3b2);
		font-size: 12px;
		padding: 8px 4px;
	}
	.merr {
		color: var(--warn, #ffcb63);
		font-size: 12px;
		padding: 8px 4px;
	}
	.mtoast {
		position: absolute;
		bottom: 52px; /* über dem Attribution-Footer — kein Überlappen (Probe-Beleg 03) */
		left: 50%;
		transform: translateX(-50%);
		background: var(--panel2, #1b1813);
		border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
		color: var(--fg, #e8ecf3);
		font-size: 12px;
		padding: 5px 12px;
		z-index: 8;
	}
	.mattr {
		border-top: 1px solid color-mix(in srgb, var(--fgdim, #9aa3b2) 20%, transparent);
		color: var(--fgdim, #9aa3b2);
		font-size: 10px;
		padding: 5px 2px 6px;
		text-align: center;
	}
</style>
