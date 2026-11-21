<script lang="ts">
	// MiningLists (M9-C, t_dc01f34b) — Mining-Listen-Tab im Mining-Modus.
	//
	// Gleiche generische Listen-Engine wie CraftLists (M9-B, t_36778bc3) —
	// kind='mining' (Modi sind Views darauf, Plan „Generisch"). Einträge sind
	// DIREKT Materialien (Material + Menge + optional Qualität), keine
	// Blueprints: der Eintrag trägt copies = Menge (Aggregation 1 × copies),
	// der Stepper ändert die Menge über `list_set_copies` — keine
	// Engine-Kopie nötig.
	//
	// Plus: Beste-Orte-Optimierung — Greedy-Set-Cover (mining-places.ts) über
	// die Strata-`ore-locations`-Daten; Qualitäts-Bänder (SCMDB, M8-R2-Snap-
	// Daten) als Filter „Ort liefert die geforderte Qualität?".
	//
	// Formular-Konvention (Sebo): Chips/Buttons statt natives <select>,
	// Typeahead startet LEER mit gefilterter Gesamtliste.
	import { t, tr } from './i18n';
	import { invoke } from '@tauri-apps/api/core';
	import { planBestPlaces, type PlacesPlan, type PlaceLocRow } from './mining-places.ts';

	type Ore = {
		id: string;
		name: string;
		slug: string;
		category: string | null;
		tier: string | null;
	};
	type ListSummary = {
		id: number;
		kind: string;
		name: string;
		created_at: number;
		item_count: number;
		// M9-D (t_36f3966c): erzeugende Crafting-Liste (Brücke), null = standalone.
		source_list_id: number | null;
		source_list_name: string | null;
	};
	type ListItemMat = {
		material: string;
		ore_id: string | null;
		unit: string;
		quantity: number;
		quality: number;
	};
	type ListItem = {
		id: number;
		item_id: string;
		item_name: string;
		craft_time: number;
		copies: number;
		created_at: number;
		mats: ListItemMat[];
	};
	type AggMat = {
		material: string;
		unit: string;
		quantity: number;
		min_quality: number;
		done_qty: number;
	};
	type ListDetail = {
		id: number;
		kind: string;
		name: string;
		created_at: number;
		items: ListItem[];
		aggregated: AggMat[];
	};
	type LocationRow = {
		locationId: string;
		location: string;
		system: string | null;
		tier: number;
	};

	let {
		ores = [],
		tierDefault = 4,
		// M9-D (t_36f3966c): Liste aus der Craft→Mining-Brücke — beim Eintreten
		// direkt öffnen (einmal pro Sprung-Ziel, M8-K3-Fix-Muster).
		openListId = null
	}: {
		ores?: Ore[];
		tierDefault?: number;
		openListId?: number | null;
	} = $props();

	// M9-D: Sprung-Ziel genau einmal öffnen — sonst reißt der Effect jede
	// manuelle Listenauswahl wieder auf das Sprung-Ziel zurück (Klebe-Effekt).
	let listJumpHandled: number | null = null;
	$effect(() => {
		const id = openListId;
		if (id === null || listJumpHandled === id) return;
		listJumpHandled = id;
		void openList(id);
	});

	// ---- State ---------------------------------------------------------------
	let lists = $state<ListSummary[]>([]);
	let activeId = $state<number | null>(null);
	let detail = $state<ListDetail | null>(null);
	let error = $state('');
	let newName = $state('');
	let renameMode = $state(false);
	let renameVal = $state('');
	let partial = $state<Record<string, string>>({});

	// M9-H (t_7709c7f2, Punkt 5): „Als erledigt" öffnet die Inventar-Maske
	// mit dem fehlenden Restbetrag voreingetragen (nur Qualität + Lagerort
	// werden abgefragt); Teil-Eintrag → Teilmenge voreingetragen. Reserviert
	// wird NUR für verknüpfte Listen (source_list_id, Craft→Mining-Brücke
	// M9-D) — standalone Mining-Listen landen ohne Reservierungs-Verweis.
	let addInvFor = $state<AggMat | null>(null);
	let invLoc = $state('');
	let invQ = $state('');
	let invQty = $state('');
	let invNote = $state('');
	let invError = $state('');
	let invLocs = $state<string[]>([]);
	let invLocEl = $state<HTMLInputElement | null>(null);
	// Verknüpfte Liste? (Brücken-Kind — nur diese reservieren, Spec Punkt 5)
	const activeLinked = $derived(lists.find((l) => l.id === activeId)?.source_list_id != null);
	const remaining = (m: AggMat) => Math.max(0, Math.round((m.quantity - m.done_qty) * 100) / 100);
	function openAddInv(m: AggMat, qty: number) {
		addInvFor = m;
		invNote = '';
		invError = '';
		// Vorschläge: Anforderung als Qualität, offene Menge als Menge.
		invQ = String(m.min_quality);
		invQty = String(qty);
		void loadInvLocs();
		setTimeout(() => invLocEl?.focus(), 0);
	}
	/// „Als erledigt": Restbetrag voreingetragen.
	function doneAllInv(m: AggMat) {
		openAddInv(m, remaining(m));
	}
	/// Teil-Eintrag: eingegebene Teilmenge voreingetragen (ungültige Eingabe
	/// → Restbetrag, damit die Maske nie mit 0/leer startet).
	function donePartialInv(m: AggMat) {
		const raw = (partial[rowKey(m)] ?? '').replace(',', '.').trim();
		const v = Number(raw);
		openAddInv(m, raw && Number.isFinite(v) && v > 0 ? v : remaining(m));
	}
	async function loadInvLocs() {
		try {
			invLocs = await invoke<string[]>('inventory_locations');
		} catch {
			invLocs = [];
		}
	}
	async function submitAddInv() {
		if (addInvFor === null || activeId === null) return;
		const m = addInvFor;
		const loc = invLoc.trim();
		const q = Number(invQ);
		const qty = Number(String(invQty).replace(',', '.'));
		if (!loc) {
			invError = $t('inventory.err_location');
			return;
		}
		if (!Number.isFinite(q) || q < 0 || q > 1000 || !Number.isInteger(q)) {
			invError = $t('inventory.err_quality');
			return;
		}
		if (!Number.isFinite(qty) || qty <= 0) {
			invError = $t('inventory.err_qty');
			return;
		}
		invError = '';
		const entry = {
			entity_kind: null,
			entity_id: null,
			name: m.material,
			quality: q,
			location_id: null,
			location_name: loc,
			quantity: qty,
			price: null
		};
		try {
			// Fortschritt der Liste mitführen: Menge addieren, auf Soll klemmen.
			const newDone = Math.min(m.quantity, m.done_qty + qty);
			// NUR verknüpfte Listen reservieren (Spec Punkt 5) — standalone
			// Listen bekommen eine normale Inventar-Position ohne Verweis.
			const addedName = activeLinked
				? (
						await invoke<{ entry: { name: string; quality: number; location_name: string } }>(
							'inventory_add_for_list',
							{ entry, listId: activeId, material: m.material, unit: m.unit }
						)
					).entry.name
				: (await invoke<{ name: string }>('inventory_add', { entry })).name;
			await invoke('list_set_done', {
				listId: activeId,
				material: m.material,
				unit: m.unit,
				doneQty: newDone
			});
			// Erst reloaden (openList räumt Eingaben/Formular auf), dann den
			// Erfolgshinweis setzen, damit er stehen bleibt.
			await openList(activeId);
			invNote = activeLinked
				? $t('lists.reserved_note', {
						name: addedName,
						q: q,
						loc: loc,
						list: detail?.name ?? ''
					})
				: $t('inventory.added_note', { name: addedName, q: q, loc: loc });
		} catch (e) {
			invError = String(e);
		}
	}

	// Eintrag hinzufügen: Material-Typeahead + Menge + Einheit + Qualität.
	let addQuery = $state('');
	let addQty = $state('100');
	let addUnit = $state<'SCU' | 'gems' | 'items'>('SCU');
	let addQuality = $state('');
	// Beste Orte
	let tierThreshold = $state(0); // 0 = ungesetzt → aus tierDefault initialisieren
	let plan = $state<PlacesPlan | null>(null);
	let planLoading = $state(false);

	const UNIT_COLORS: Record<string, string> = {
		SCU: '#e0a83e',
		gems: '#7ec8e3',
		items: '#b8bfc9'
	};
	const unitName = (u: string) => $t(`craft.unit_${u.toLowerCase()}`);
	const fmtQty = (q: number) =>
		Number.isInteger(q) ? String(q) : String(Math.round(q * 100) / 100);
	const rowKey = (m: { material: string; unit: string }) => `${m.material}:${m.unit}`;
	const pct = (m: AggMat) =>
		m.quantity > 0 ? Math.min(100, Math.round((m.done_qty / m.quantity) * 100)) : 0;

	// Typeahead: Start LEER → Gesamtliste, Filter case-insensitiv Name/Slug.
	const addMatches = $derived.by(() => {
		const q = addQuery.trim().toLowerCase();
		const pool = q
			? ores.filter(
					(o) => o.name.toLowerCase().includes(q) || o.slug.toLowerCase().includes(q)
				)
			: ores;
		return pool.slice(0, 8);
	});
	const addSelected = $derived(
		ores.find((o) => o.name.toLowerCase() === addQuery.trim().toLowerCase()) ?? null
	);
	// M9-D: erzeugende Crafting-Liste der aktiven Liste (Brücke) — Badge im Kopf.
	const activeSourceName = $derived(
		lists.find((l) => l.id === activeId)?.source_list_name ?? null
	);

	// ---- Laden / CRUD (identisch zur Engine in CraftLists) --------------------
	async function loadLists() {
		try {
			lists = await invoke<ListSummary[]>('lists_list', { kind: 'mining' });
			if (activeId !== null && !lists.some((l) => l.id === activeId)) {
				activeId = null;
				detail = null;
			}
			if (activeId === null && lists.length > 0) void openList(lists[0].id);
		} catch (e) {
			error = String(e);
		}
	}
	async function openList(id: number) {
		error = '';
		// M9-H: Hinweis/Formular der vorherigen Liste verfallen lassen.
		invNote = '';
		invError = '';
		addInvFor = null;
		try {
			detail = await invoke<ListDetail>('list_detail', { listId: id });
			activeId = id;
		} catch (e) {
			error = String(e);
		}
	}
	async function createList() {
		const name = newName.trim();
		if (!name) return;
		error = '';
		try {
			const id = await invoke<number>('list_create', { kind: 'mining', name });
			newName = '';
			await loadLists();
			await openList(id);
		} catch (e) {
			error = String(e);
		}
	}
	async function renameList() {
		if (activeId === null) return;
		const name = renameVal.trim();
		if (!name) return;
		error = '';
		try {
			await invoke('list_rename', { listId: activeId, name });
			renameMode = false;
			await loadLists();
			await openList(activeId);
		} catch (e) {
			error = String(e);
		}
	}
	async function deleteList() {
		if (activeId === null) return;
		error = '';
		try {
			await invoke('list_delete', { listId: activeId });
			activeId = null;
			detail = null;
			plan = null;
			await loadLists();
		} catch (e) {
			error = String(e);
		}
	}
	async function removeItem(row: number) {
		error = '';
		try {
			await invoke('list_remove_item', { itemRow: row });
			if (activeId !== null) await openList(activeId);
			await loadLists();
		} catch (e) {
			error = String(e);
		}
	}
	async function stepQty(row: number, cur: number, delta: number) {
		const next = Math.max(1, Math.round((cur + delta) * 100) / 100);
		error = '';
		try {
			await invoke('list_set_copies', { itemRow: row, copies: next });
			if (activeId !== null) await openList(activeId);
		} catch (e) {
			error = String(e);
		}
	}

	// Eintrag anlegen: Material + Menge (copies) + Einheit + optionale Qualität.
	async function addMaterial() {
		const ore = addSelected;
		const name = (ore?.name ?? addQuery.trim());
		if (!name) return;
		const qty = Number(addQty.replace(',', '.'));
		if (!Number.isFinite(qty) || qty <= 0) {
			error = $t('lists.err_copies');
			return;
		}
		const quality = addQuality.trim() === '' ? 500 : Math.round(Number(addQuality));
		error = '';
		try {
			await invoke('list_add_item', {
				listId: activeId,
				item: {
					item_id: ore?.id ?? name,
					item_name: name,
					craft_time: 0,
					copies: qty, // Menge = copies (Aggregation 1 × copies)
					mats: [
						{
							material: name,
							ore_id: ore?.id ?? null,
							unit: addUnit,
							quantity: 1,
							quality: Number.isFinite(quality) ? Math.min(1000, Math.max(0, quality)) : 500
						}
					]
				}
			});
			addQuery = '';
			addQuality = '';
			await loadLists();
			if (activeId !== null) await openList(activeId);
		} catch (e) {
			error = String(e);
		}
	}

	// ---- Abhaken (identisch CraftLists) --------------------------------------
	async function setDone(m: AggMat, done: number | null) {
		if (activeId === null) return;
		error = '';
		try {
			await invoke('list_set_done', {
				listId: activeId,
				material: m.material,
				unit: m.unit,
				doneQty: done
			});
			delete partial[rowKey(m)];
			await openList(activeId);
		} catch (e) {
			error = String(e);
		}
	}
	// (doneAll/donePartial ersetzt durch doneAllInv/donePartialInv — M9-H
	// Punkt 5: Abhaken läuft jetzt über die Inventar-Maske mit Vorbefüllung.)

	// ---- Beste Orte (Set-Cover + Qualitäts-Bänder) ----------------------------
	async function computePlaces() {
		if (!detail) return;
		plan = null;
		planLoading = true;
		error = '';
		try {
			// Aggregation → ortbare Materialien (ore_id aus dem Eintrags-Snapshot).
			const agg = detail.aggregated.filter((m) => pct(m) < 100);
			const oreIdOf = new Map<string, string | null>();
			for (const it of detail.items)
				for (const mm of it.mats) oreIdOf.set(`${mm.material}:${mm.unit}`, mm.ore_id);
			const mats = agg.map((m) => ({
				material: m.material,
				unit: m.unit,
				quantity: m.quantity,
				min_quality: m.min_quality,
				oreId: oreIdOf.get(rowKey(m)) ?? null
			}));
			const withOre = mats.filter((m) => m.oreId);
			if (!withOre.length) {
				plan = { places: [], uncovered: mats };
				planLoading = false;
				return;
			}
			// SCMDB-Bänder je Material (M8-R2-Command, Cache 7d) + ore-locations
			// je Erz (Strata, cache-first) parallel.
			const [snaps, locLists] = await Promise.all([
				invoke<{ uuid: string; values: number[] }[]>('scmdb_quality_snaps', {
					uuids: withOre.map((m) => m.oreId!),
					names: withOre.map((m) => m.material)
				}),
				Promise.all(
					withOre.map((m) =>
						invoke<{
							results: Record<string, LocationRow[]>;
							attribution: string;
						}>('strata_ore_locations', { ore: m.oreId, sort: 'rank' })
					)
				)
			]);
			const bandsByMaterial: Record<string, number[]> = {};
			for (let i = 0; i < withOre.length; i++) {
				// Key-Matching mit mining-places.ts: raw `${material}:${unit}`
				// (quality-calc materialKey = lowercase/Nur-Name konvergiert nie).
				const key = `${withOre[i].material}:${withOre[i].unit}`;
				if (snaps[i]?.values?.length) bandsByMaterial[key] = [...snaps[i].values];
			}
			const rowsByOre: Record<string, PlaceLocRow[]> = {};
			for (let i = 0; i < withOre.length; i++) {
				const res = locLists[i];
				const groups = res?.results ?? {};
				rowsByOre[withOre[i].oreId!] = ['space', 'surface', 'cave', 'exploration'].flatMap(
					(g) =>
						(groups[g] ?? []).map((r) => ({
							locationId: r.locationId,
							location: r.location,
							system: r.system,
							group: g,
							tier: r.tier,
							concentration: 0
						}))
				);
			}
			plan = planBestPlaces(mats, rowsByOre, bandsByMaterial, tierThreshold, (m) => {
				const row = agg.find((a) => rowKey(a) === rowKey(m));
				return row ? pct(row) >= 100 : false;
			});
		} catch (e) {
			error = String(e);
		}
		planLoading = false;
	}

	const groupName = (g: string) => tr(`mining.group_${g}`);
	const tierLabel = (tier: number) => tr(`mining.tier_${Math.min(6, Math.max(1, tier))}`);

	// Tier-Schwelle einmalig aus dem Status-Default vorbelegen (Settings:
	// strata_default_tier_threshold, M7-Welle-2).
	$effect(() => {
		if (tierThreshold === 0 && tierDefault > 0) tierThreshold = tierDefault;
	});

	void loadLists();
</script>

<div class="mlists" role="region" aria-label={$t('mining.tab_lists')}>
	<!-- Listen-Auswahl (Chips) + Neu-Anlage -->
	<div class="ml-bar">
		<div class="ml-chips">
			{#each lists as l (l.id)}
				<button
					type="button"
					class="ml-chip"
					class:is-on={activeId === l.id}
					onclick={() => void openList(l.id)}
					title={l.source_list_name
						? $t('lists.source_badge', { name: l.source_list_name })
						: l.name}>{l.name} <span class="ml-count">{l.item_count}</span></button
				>
			{:else}
				<span class="ml-hint">{$t('lists.empty')}</span>
			{/each}
		</div>
		<div class="ml-new">
			<input
				class="ml-input"
				type="text"
				bind:value={newName}
				placeholder={$t('lists.new_list_ph')}
				aria-label={$t('lists.new_list_ph')}
				onkeydown={(e) => e.key === 'Enter' && void createList()}
			/>
			<button type="button" class="ml-btn" onclick={() => void createList()}>
				{$t('lists.new_list')}
			</button>
		</div>
	</div>

	{#if error}<div class="ml-err">{error}</div>{/if}

	{#if detail}
		<div class="ml-head">
			{#if renameMode}
				<input
					class="ml-input"
					type="text"
					bind:value={renameVal}
					aria-label={$t('lists.rename')}
					onkeydown={(e) => e.key === 'Enter' && void renameList()}
				/>
				<button type="button" class="ml-btn" onclick={() => void renameList()}>
					{$t('common.save')}
				</button>
				<button type="button" class="ml-btn" onclick={() => (renameMode = false)}>
					{$t('common.cancel')}
				</button>
			{:else}
				<span class="ml-title">{detail.name}</span>
				{#if activeSourceName}
					<span class="ml-src" title={$t('lists.source_badge', { name: activeSourceName })}
						>{$t('lists.source_badge', { name: activeSourceName })}</span
					>
				{/if}
				<button
					type="button"
					class="ml-mini"
					onclick={() => {
						renameVal = detail?.name ?? '';
						renameMode = true;
					}}>{$t('lists.rename')}</button
				>
				<button type="button" class="ml-mini is-danger" onclick={() => void deleteList()}>
					{$t('lists.delete')}
				</button>
			{/if}
		</div>

		<!-- Eintrag hinzufügen: Material (Typeahead) + Menge + Einheit + Qualität -->
		<!-- M9-G: verknüpfte Listen sind abhängig — Material/Mengen kommen aus
		     der Crafting-Liste und werden dort gepflegt (read-only hier). -->
		{#if !activeLinked}
		<div class="ml-add">
			<input
				class="ml-input ml-add-mat"
				type="text"
				bind:value={addQuery}
				placeholder={$t('mining.lists_add_ph')}
				aria-label={$t('mining.lists_add_ph')}
			/>
			<input
				class="ml-input ml-add-qty"
				type="text"
				inputmode="decimal"
				bind:value={addQty}
				placeholder={$t('mining.lists_qty_ph')}
				aria-label={$t('mining.lists_qty_ph')}
			/>
			<div class="ml-units" role="group" aria-label={$t('craft.unit_scu')}>
				{#each ['SCU', 'gems', 'items'] as u (u)}
					<button
						type="button"
						class="ml-mini"
						class:is-on={addUnit === u}
						onclick={() => (addUnit = u as 'SCU' | 'gems' | 'items')}>{unitName(u)}</button
					>
				{/each}
			</div>
			<input
				class="ml-input ml-add-q"
				type="text"
				inputmode="numeric"
				bind:value={addQuality}
				placeholder={$t('mining.lists_quality_ph')}
				aria-label={$t('mining.lists_quality_ph')}
			/>
			<button
				type="button"
				class="ml-btn"
				disabled={!addQuery.trim()}
				onclick={() => void addMaterial()}>{$t('lists.add_to_list')}</button
			>
			{#if addQuery.trim() && !addSelected}
				<div class="ml-suggest">
					{#each addMatches as o (o.id)}
						<button
							type="button"
							class="ml-suggest-row"
							onclick={() => {
								addQuery = o.name;
								// Einheit aus der Erz-Kategorie vorab setzen (ship → SCU).
								if (o.category === 'ship') addUnit = 'SCU';
								else if (o.category === 'fps' || o.category === 'ground_vehicle')
									addUnit = 'gems';
							}}
						>
							<span class="ml-suggest-name">{o.name}</span>
							<span class="ml-suggest-sub">{o.slug}</span>
						</button>
					{:else}
						<div class="ml-hint">{$t('mining.empty')}</div>
					{/each}
				</div>
			{/if}
		</div>
		{/if}

		<!-- Einträge: Material + Mengen-Stepper + Entfernen -->
		{#if activeLinked}
			<div class="ml-hint">
				{$t('lists.linked_readonly', { name: activeSourceName ?? '' })}
			</div>
		{/if}
		{#if detail.items.length}
			<div class="ml-items">
				{#each detail.items as it (it.id)}
					<div class="ml-item">
						<span class="ml-item-name">{it.item_name}</span>
						{#if !activeLinked}
							<span class="ml-copies">
								<button
									type="button"
									class="ml-step"
									aria-label={$t('lists.copies_less')}
									onclick={() => void stepQty(it.id, it.copies, -1)}>−</button
								>
								<span class="ml-copies-n">{fmtQty(it.copies)}</span>
								<button
									type="button"
									class="ml-step"
									aria-label={$t('lists.copies_more')}
									onclick={() => void stepQty(it.id, it.copies, 1)}>＋</button
								>
							</span>
							<button
								type="button"
								class="ml-mini is-danger"
								onclick={() => void removeItem(it.id)}>{$t('lists.remove')}</button
							>
						{/if}
					</div>
				{/each}
			</div>
		{:else}
			<div class="ml-hint">{$t('mining.lists_no_items')}</div>
		{/if}

		<!-- Aggregierte Mengen + Abhaken (ganz/teilweise/Reset) -->
		<div class="ml-agg-head">{$t('lists.aggregated')}</div>
		{#if invNote}<div class="ml-inv-note">{invNote}</div>{/if}
		{#if detail.aggregated.length}
			<div class="ml-agg">
				{#each detail.aggregated as m (rowKey(m))}
					{@const p = pct(m)}
					<div class="ml-agg-row">
						<div class="ml-agg-line">
							<span class="ml-mat">{m.material}</span>
							<span class="ml-qty">
								{fmtQty(m.done_qty)}/{fmtQty(m.quantity)}
								<span
									class="ml-unit"
									style={`--uc:${UNIT_COLORS[m.unit] ?? '#b8bfc9'}`}>{unitName(m.unit)}</span
								>
							</span>
							<span class="ml-q" title={$t('lists.quality_req')}>Q ≥ {m.min_quality}</span>
						</div>
						<div
							class="ml-bar-track"
							role="progressbar"
							aria-valuenow={p}
							aria-valuemin="0"
							aria-valuemax="100"
						>
							<div class="ml-bar-fill" class:is-done={p >= 100} style={`width:${p}%`}></div>
						</div>
						<div class="ml-agg-actions">
							<button type="button" class="ml-mini" onclick={() => doneAllInv(m)} disabled={p >= 100}>
								{$t('lists.done_all')}
							</button>
							<input
								class="ml-input ml-partial"
								type="text"
								inputmode="decimal"
								bind:value={partial[rowKey(m)]}
								placeholder={$t('lists.done_partial_ph')}
								aria-label={$t('lists.done_partial')}
								onkeydown={(e) => e.key === 'Enter' && donePartialInv(m)}
							/>
							<button
								type="button"
								class="ml-mini"
								onclick={() => donePartialInv(m)}
								disabled={p >= 100}>{$t('lists.done_partial')}</button
							>
							{#if m.done_qty > 0}
								<button type="button" class="ml-mini" onclick={() => void setDone(m, null)}>
									{$t('lists.reset')}
								</button>
							{/if}
							<button type="button" class="ml-mini" onclick={() => openAddInv(m, remaining(m))}>
								{$t('lists.to_inventory')}
							</button>
							{#if addInvFor && addInvFor.material === m.material && addInvFor.unit === m.unit}
								<div class="ml-inv-form">
									<div class="ml-inv-title">{$t('lists.inv_title', { list: detail.name })}</div>
									{#if invError}<div class="ml-err">{invError}</div>{/if}
									<div class="ml-inv-fields">
										<input
											bind:this={invLocEl}
											class="ml-input"
											type="text"
											bind:value={invLoc}
											list="ml-inv-locs"
											placeholder={$t('inventory.location')}
											aria-label={$t('inventory.location')}
											onkeydown={(e) => e.key === 'Enter' && void submitAddInv()}
										/>
										<datalist id="ml-inv-locs">
											{#each invLocs as loc}<option value={loc}></option>{/each}
										</datalist>
										<input
											class="ml-input ml-inv-q"
											type="text"
											inputmode="numeric"
											bind:value={invQ}
											placeholder={$t('inventory.quality')}
											aria-label={$t('inventory.quality')}
											onkeydown={(e) => e.key === 'Enter' && void submitAddInv()}
										/>
										<input
											class="ml-input ml-inv-q"
											type="text"
											inputmode="decimal"
											bind:value={invQty}
											placeholder={$t('lists.done_partial_ph')}
											aria-label={$t('lists.inv_qty')}
											onkeydown={(e) => e.key === 'Enter' && void submitAddInv()}
										/>
										<button type="button" class="ml-btn" onclick={() => void submitAddInv()}>
											{$t('lists.inv_add')}
										</button>
										<button type="button" class="ml-mini" onclick={() => (addInvFor = null)}>
											{$t('common.cancel')}
										</button>
									</div>
								</div>
							{/if}
							</div>
							</div>
							{/each}
							</div>
							{:else}
							<div class="ml-hint">{$t('lists.no_mats')}</div>
							{/if}

		<!-- Beste Orte: möglichst wenige Orte, Qualitäts-Bänder als Filter -->
		<div class="ml-places-head">
			<span class="ml-agg-head">{$t('mining.lists_places_title')}</span>
			<span class="ml-tier" role="group" aria-label={$t('mining.tier')}>
				{#each [1, 2, 3, 4, 5, 6] as tv (tv)}
					<button
						type="button"
						class="ml-mini"
						class:is-on={tierThreshold === tv}
						onclick={() => (tierThreshold = tv)}
						title={tierLabel(tv)}>{tv}</button
					>
				{/each}
			</span>
			<button
				type="button"
				class="ml-btn"
				disabled={planLoading || !detail?.aggregated.length}
				onclick={() => void computePlaces()}
			>
				{planLoading ? $t('mining.loading') : $t('mining.lists_places_compute')}
			</button>
		</div>
		{#if plan}
			{#if plan.places.length}
				<div class="ml-places">
					{#each plan.places as p, i (p.locationId)}
						<div class="ml-place">
							<div class="ml-place-head">
								<span class="ml-place-n">{i + 1}</span>
								<span class="ml-place-name">{p.location}</span>
								<span class="ml-place-sub">{p.system ?? '—'} · {groupName(p.group)}</span>
							</div>
							<div class="ml-place-mats">
								{#each p.covered as c (c.material + c.unit)}
									<span class="ml-place-mat"
										>{c.material}
										<span class="ml-place-qty"
											>{fmtQty(c.quantity)} {unitName(c.unit)}</span
										>
										<span class="ml-place-tier" title={tierLabel(c.tier)}>T{c.tier}</span>
									</span>
								{/each}
							</div>
						</div>
					{/each}
				</div>
			{:else}
				<div class="ml-hint">{$t('mining.lists_places_none')}</div>
			{/if}
			{#if plan.uncovered.length}
				<div class="ml-uncovered">
					<span class="ml-agg-head">{$t('mining.lists_uncovered')}</span>
					<div class="ml-place-mats">
						{#each plan.uncovered as u (u.material + u.unit)}
							<span class="ml-place-mat is-open">{u.material}</span>
						{/each}
					</div>
				</div>
			{/if}
		{/if}
	{/if}
</div>

<style>
	.mlists {
		display: flex;
		flex-direction: column;
		gap: 10px;
		min-height: 0;
		flex: 1;
		color: var(--fg, #d7dde5);
		/* M9-H (t_7709c7f2, Punkt 6): eigener Scroll + Padding — Inhalt war
		   an der Kante abgeschnitten (kein Scroll-Container) und klebte am Rand. */
		overflow-y: auto;
		padding: 2px 10px 12px 2px;
	}
	.mlists::-webkit-scrollbar {
		width: 8px;
	}
	.mlists::-webkit-scrollbar-thumb {
		background: color-mix(in srgb, var(--accent, #4aa3ff) 35%, transparent);
		border-radius: 4px;
	}
	/* M9-H Punkt 5: Inventar-Maske (Port aus CraftLists .cl-inv-*). */
	.ml-inv-note {
		font-size: 12px;
		padding: 6px 10px;
		border-radius: 6px;
		background: rgba(111, 220, 140, 0.12);
		border: 1px solid rgba(111, 220, 140, 0.3);
		color: #6fdc8c;
	}
	.ml-inv-form {
		display: flex;
		flex-direction: column;
		gap: 6px;
		background: rgba(255, 255, 255, 0.03);
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 6px;
		padding: 8px;
	}
	.ml-inv-title {
		font-size: 12px;
		opacity: 0.75;
	}
	.ml-inv-fields {
		display: flex;
		gap: 6px;
		align-items: center;
		flex-wrap: wrap;
	}
	.ml-inv-q {
		width: 74px;
		padding: 3px 8px;
	}
	.ml-bar {
		display: flex;
		gap: 10px;
		align-items: flex-start;
		flex-wrap: wrap;
	}
	.ml-chips {
		display: flex;
		gap: 6px;
		flex-wrap: wrap;
		flex: 1;
		min-width: 0;
	}
	.ml-chip {
		background: rgba(255, 255, 255, 0.05);
		border: 1px solid rgba(255, 255, 255, 0.14);
		border-radius: 999px;
		color: inherit;
		padding: 4px 12px;
		font: inherit;
		font-size: 12px;
		cursor: pointer;
	}
	.ml-chip.is-on {
		background: var(--accent, #4aa3ff);
		border-color: transparent;
		color: #0b0f14;
		font-weight: 600;
	}
	.ml-count {
		opacity: 0.65;
		font-size: 11px;
	}
	.ml-new,
	.ml-add,
	.ml-agg-actions,
	.ml-head,
	.ml-places-head {
		display: flex;
		gap: 6px;
		align-items: center;
	}
	.ml-add,
	.ml-places-head {
		flex-wrap: wrap;
	}
	.ml-input {
		background: rgba(255, 255, 255, 0.06);
		border: 1px solid rgba(255, 255, 255, 0.14);
		border-radius: 6px;
		color: inherit;
		padding: 5px 8px;
		font: inherit;
		font-size: 12px;
		width: 150px;
	}
	.ml-input:focus {
		outline: 1px solid var(--accent, #4aa3ff);
	}
	.ml-add-mat {
		width: 190px;
	}
	.ml-add-qty {
		width: 74px;
	}
	.ml-add-q {
		width: 74px;
	}
	.ml-units {
		display: flex;
		gap: 4px;
	}
	.ml-btn {
		background: var(--accent, #4aa3ff);
		border: none;
		border-radius: 6px;
		color: #0b0f14;
		padding: 5px 12px;
		font: inherit;
		font-size: 12px;
		font-weight: 600;
		cursor: pointer;
		white-space: nowrap;
	}
	.ml-btn:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.ml-mini {
		background: rgba(255, 255, 255, 0.07);
		border: 1px solid rgba(255, 255, 255, 0.14);
		border-radius: 6px;
		color: inherit;
		padding: 3px 9px;
		font: inherit;
		font-size: 11px;
		cursor: pointer;
		white-space: nowrap;
	}
	.ml-mini.is-danger {
		color: #ff8a80;
		border-color: rgba(255, 138, 128, 0.35);
	}
	.ml-mini.is-on {
		background: var(--accent, #4aa3ff);
		border-color: transparent;
		color: #0b0f14;
		font-weight: 600;
	}
	.ml-mini:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.ml-hint {
		opacity: 0.6;
		font-size: 12px;
	}
	.ml-err {
		color: #ff8a80;
		font-size: 12px;
	}
	.ml-title {
		font-weight: 700;
		font-size: 15px;
	}
	/* M9-D: Badge „aus Crafting-Liste X“ bei Brücken-verlinkten Listen. */
	.ml-src {
		font-size: 11px;
		color: var(--txt-dim, #9aa4b2);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		max-width: 220px;
	}
	/* Typeahead-Vorschläge (Position unter dem Add-Formular). */
	.ml-add {
		position: relative;
		flex-wrap: wrap;
	}
	.ml-suggest {
		display: flex;
		flex-direction: column;
		border: 1px solid rgba(255, 255, 255, 0.14);
		border-radius: 8px;
		background: rgba(12, 16, 22, 0.97);
		overflow: hidden;
		flex-basis: 100%;
	}
	.ml-suggest-row {
		display: flex;
		gap: 8px;
		align-items: baseline;
		background: none;
		border: none;
		border-bottom: 1px solid rgba(255, 255, 255, 0.06);
		color: inherit;
		padding: 6px 10px;
		font: inherit;
		font-size: 12px;
		cursor: pointer;
		text-align: left;
	}
	.ml-suggest-row:last-child {
		border-bottom: none;
	}
	.ml-suggest-row:hover {
		background: rgba(255, 255, 255, 0.06);
	}
	.ml-suggest-sub {
		opacity: 0.5;
		font-size: 11px;
	}
	.ml-items {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.ml-item {
		display: flex;
		gap: 8px;
		align-items: center;
		background: rgba(255, 255, 255, 0.04);
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 8px;
		padding: 6px 10px;
	}
	.ml-item-name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 13px;
	}
	.ml-copies {
		display: flex;
		align-items: center;
		gap: 4px;
		white-space: nowrap;
	}
	.ml-step {
		background: rgba(255, 255, 255, 0.07);
		border: 1px solid rgba(255, 255, 255, 0.14);
		border-radius: 5px;
		color: inherit;
		width: 22px;
		height: 22px;
		font: inherit;
		font-size: 13px;
		cursor: pointer;
		line-height: 1;
	}
	.ml-copies-n {
		font-size: 12px;
		min-width: 40px;
		text-align: center;
	}
	.ml-agg-head {
		font-size: 11px;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		opacity: 0.6;
	}
	.ml-agg,
	.ml-places {
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.ml-agg-row {
		background: rgba(255, 255, 255, 0.04);
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 8px;
		padding: 8px 10px;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.ml-agg-line {
		display: flex;
		gap: 10px;
		align-items: baseline;
	}
	.ml-mat {
		font-size: 13px;
		font-weight: 600;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.ml-qty {
		margin-left: auto;
		white-space: nowrap;
		font-size: 12px;
	}
	.ml-unit {
		color: var(--uc, #b8bfc9);
		font-size: 11px;
	}
	.ml-q {
		opacity: 0.6;
		font-size: 11px;
		white-space: nowrap;
	}
	.ml-bar-track {
		height: 4px;
		border-radius: 2px;
		background: rgba(255, 255, 255, 0.08);
		overflow: hidden;
	}
	.ml-bar-fill {
		height: 100%;
		background: var(--accent, #4aa3ff);
		border-radius: 2px;
	}
	.ml-bar-fill.is-done {
		background: #6fdc8c;
	}
	.ml-partial {
		width: 74px;
		padding: 3px 8px;
	}
	/* Beste Orte */
	.ml-tier {
		display: flex;
		gap: 3px;
	}
	.ml-place {
		background: rgba(255, 255, 255, 0.04);
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 8px;
		padding: 8px 10px;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.ml-place-head {
		display: flex;
		gap: 8px;
		align-items: baseline;
	}
	.ml-place-n {
		width: 18px;
		height: 18px;
		border-radius: 50%;
		background: var(--accent, #4aa3ff);
		color: #0b0f14;
		font-size: 11px;
		font-weight: 700;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		flex-shrink: 0;
		align-self: center;
	}
	.ml-place-name {
		font-weight: 600;
		font-size: 13px;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.ml-place-sub {
		opacity: 0.55;
		font-size: 11px;
		white-space: nowrap;
	}
	.ml-place-mats {
		display: flex;
		gap: 6px;
		flex-wrap: wrap;
	}
	.ml-place-mat {
		display: inline-flex;
		gap: 6px;
		align-items: baseline;
		background: rgba(255, 255, 255, 0.06);
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 6px;
		padding: 2px 8px;
		font-size: 12px;
	}
	.ml-place-mat.is-open {
		border-color: rgba(255, 138, 128, 0.35);
	}
	.ml-place-qty {
		opacity: 0.7;
		font-size: 11px;
	}
	.ml-place-tier {
		color: var(--accent, #4aa3ff);
		font-size: 10px;
		font-weight: 700;
	}
	.ml-uncovered {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
</style>
