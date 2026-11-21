<script lang="ts">
	// CraftLists (M9-B, t_36778bc3) — Crafting-Listen-Tab im Crafting-Modus.
	//
	// Benannte Listen von Blueprints: CRUD (anlegen/umbenennen/löschen),
	// Einträge mit copies-Stepper, aggregierte Materialmengen über ALLE
	// Listen-Items (Mengen summiert, Qualität = MAX der Eintrags-Werte —
	// „höhere Qualität ist immer besser"), Abhaken pro Material ganz
	// (done = Sollmenge) oder teilweise (freier Wert, z. B. 250/300) inkl.
	// Fortschrittsbalken + Reset. Die Aggregation rechnet die Cache-Engine
	// (`list_detail`) — diese Komponente zeigt nur an.
	//
	// Formular-Konvention (Sebo): Chips/Buttons statt natives <select>
	// (Fokusduell-Bug im Spotlight-Webview). Esc übernimmt der Eltern-Modus.
	import { t } from './i18n';
	import { invoke } from '@tauri-apps/api/core';
	// M9-D (t_36f3966c): Craft→Mining-Brücke — Aggregation → Mining-Listen-Einträge.
	import { bridgeItems } from './list-bridge.ts';

	type ListSummary = {
		id: number;
		kind: string;
		name: string;
		created_at: number;
		item_count: number;
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

	// M8-K3-Konvention: Bedarfserz klickbar → Mining-Sprung (Callback des Parents).
	let {
		onOpenOre = null,
		// M9-D (t_36f3966c): nach dem Brücken-Anlegen zum Mining-Modus springen
		// und die neue Liste öffnen (M8-K3-Muster: Callback des Parents, kein
		// Modi-Kopplungs-Hardcode in der Komponente).
		onOpenMiningList = null
	}: {
		onOpenOre?: ((oreId: string) => void) | null;
		onOpenMiningList?: ((listId: number) => void) | null;
	} = $props();

	// ---- State ---------------------------------------------------------------
	let lists = $state<ListSummary[]>([]);
	let activeId = $state<number | null>(null);
	let detail = $state<ListDetail | null>(null);
	let error = $state('');
	let newName = $state('');
	let renameMode = $state(false);
	let renameVal = $state('');
	// Teilweise-Abhak-Eingabe je aggregierter Zeile (material:unit → Text).
	let partial = $state<Record<string, string>>({});

	// M9-E (t_aae8d4c9) — Inventar-Anbindung: „Material hinzufügen“ pro
	// aggregierter Zeile. Ort + Qualität werden hier abgefragt, die Position
	// landet reserviert für DIESE Liste im Inventar; eine höhere Qualität
	// hebt die Anforderung an (Antwort aus inventory_add_for_list).
	let addInvFor = $state<AggMat | null>(null);
	let invLoc = $state('');
	let invQ = $state('');
	let invQty = $state('');
	let invNote = $state('');
	let invNoteRaised = $state(false);
	let invRaisedQ = $state(0);
	let invError = $state('');
	let invLocs = $state<string[]>([]);
	let invLocEl = $state<HTMLInputElement | null>(null);
	const remaining = (m: AggMat) => Math.max(0, Math.round((m.quantity - m.done_qty) * 100) / 100);
	function openAddInv(m: AggMat) {
		addInvFor = m;
		invNote = '';
		invError = '';
		// Vorschläge: Anforderung als Qualität, offene Menge als Menge.
		invQ = String(m.min_quality);
		invQty = String(remaining(m));
		void loadInvLocs();
		setTimeout(() => invLocEl?.focus(), 0);
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
		const qty = Number(invQty.replace(',', '.'));
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
		try {
			const res = await invoke<{
				entry: { name: string; quality: number; location_name: string };
				requirement_raised: boolean;
				requirement: number;
			}>('inventory_add_for_list', {
				entry: {
					entity_kind: null,
					entity_id: null,
					name: m.material,
					quality: q,
					location_id: null,
					location_name: loc,
					quantity: qty,
					price: null
				},
				listId: activeId,
				material: m.material,
				unit: m.unit
			});
			// Erst reloaden (openList räumt Hinweis/Formular auf), dann den
			// Erfolgshinweis setzen, damit er stehen bleibt.
			await openList(activeId);
			invNote = $t('lists.reserved_note', {
				name: res.entry.name,
				q: res.entry.quality,
				loc: res.entry.location_name,
				list: detail?.name ?? ''
			});
			invNoteRaised = res.requirement_raised;
			invRaisedQ = res.requirement;
		} catch (e) {
			invError = String(e);
		}
	}

	const UNIT_COLORS: Record<string, string> = {
		SCU: '#e0a83e',
		gems: '#7ec8e3',
		items: '#b8bfc9'
	};
	const unitName = (u: string) => tr_unit(u);
	const fmtQty = (q: number) =>
		Number.isInteger(q) ? String(q) : String(Math.round(q * 100) / 100);
	function tr_unit(u: string): string {
		// craft.unit_scu/gems/items existieren bereits (M8-K1).
		return $t(`craft.unit_${u.toLowerCase()}`);
	}

	const rowKey = (m: { material: string; unit: string }) => `${m.material}:${m.unit}`;
	const pct = (m: AggMat) =>
		m.quantity > 0 ? Math.min(100, Math.round((m.done_qty / m.quantity) * 100)) : 0;

	// ---- Laden / CRUD --------------------------------------------------------
	async function loadLists() {
		try {
			lists = await invoke<ListSummary[]>('lists_list', { kind: 'craft' });
			// Aktive Liste verfallen lassen, wenn sie nicht mehr existiert.
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
		// M9-E: Hinweis/Formular der vorherigen Liste verfallen lassen.
		invNote = '';
		invError = '';
		addInvFor = null;
		// M9-D: Brücken-Hinweis ebenso verfallen lassen.
		bridgeNote = '';
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
			const id = await invoke<number>('list_create', { kind: 'craft', name });
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
	async function stepCopies(row: number, cur: number, delta: number) {
		const next = Math.max(1, Math.round((cur + delta) * 100) / 100);
		error = '';
		try {
			await invoke('list_set_copies', { itemRow: row, copies: next });
			if (activeId !== null) await openList(activeId);
		} catch (e) {
			error = String(e);
		}
	}

	// ---- Abhaken -------------------------------------------------------------
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
	function doneAll(m: AggMat) {
		void setDone(m, m.quantity);
	}
	function donePartial(m: AggMat) {
		const raw = (partial[rowKey(m)] ?? '').replace(',', '.').trim();
		const v = Number(raw);
		if (!raw || !Number.isFinite(v) || v < 0) return;
		void setDone(m, v);
	}
	function openOre(id: string | null) {
		if (id) onOpenOre?.(id);
	}

	// M9-D (t_36f3966c) — Craft→Mining-Brücke: aus der Aggregation DIESER
	// Liste eine Mining-Liste anlegen (Material + Menge + MAX-Qualität,
	// ore_id aus den Eintrags-Snapshots). Die Engine ist generisch — kein
	// Backend-Change: list_create('mining') + list_add_item je Material.
	// Nach dem Anlegen → Mining-Modus-Sprung mit der neuen Liste (M8-K3).
	let bridgeNote = $state('');
	let bridgeBusy = $state(false);
	async function bridgeToMining() {
		if (detail === null || bridgeBusy) return;
		const items = bridgeItems(detail);
		if (!items.length) return;
		error = '';
		bridgeBusy = true;
		try {
			// M9-D R1 (Reviewer): persistente Verknüpfung — die Mining-Liste
			// trägt die erzeugende Crafting-Liste (source_list_id); Inventar-
			// Reservierungs-Hinweise zeigen dann auf die Crafting-Liste.
			const id = await invoke<number>('list_create', {
				kind: 'mining',
				name: detail.name,
				sourceListId: detail.id
			});
			for (const item of items) {
				await invoke('list_add_item', { listId: id, item });
			}
			await loadLists();
			bridgeNote = $t('lists.bridge_note', { name: detail.name });
			onOpenMiningList?.(id);
		} catch (e) {
			error = String(e);
		} finally {
			bridgeBusy = false;
		}
	}

	void loadLists();
</script>

<div class="clists" role="region" aria-label={$t('craft.tab_lists')}>
	<!-- Listen-Auswahl (Chips) + Neu-Anlage -->
	<div class="cl-bar">
		<div class="cl-chips">
			{#each lists as l (l.id)}
				<button
					type="button"
					class="cl-chip"
					class:is-on={activeId === l.id}
					onclick={() => void openList(l.id)}
					title={l.name}>{l.name} <span class="cl-count">{l.item_count}</span></button
				>
			{:else}
				<span class="cl-hint">{$t('lists.empty')}</span>
			{/each}
		</div>
		<div class="cl-new">
			<input
				class="cl-input"
				type="text"
				bind:value={newName}
				placeholder={$t('lists.new_list_ph')}
				aria-label={$t('lists.new_list_ph')}
				onkeydown={(e) => e.key === 'Enter' && void createList()}
			/>
			<button type="button" class="cl-btn" onclick={() => void createList()}>
				{$t('lists.new_list')}
			</button>
		</div>
	</div>

	{#if error}<div class="cl-err">{error}</div>{/if}

	{#if detail}
		<!-- Listen-Kopf: Name + Umbenennen/Löschen -->
		<div class="cl-head">
			{#if renameMode}
				<input
					class="cl-input"
					type="text"
					bind:value={renameVal}
					aria-label={$t('lists.rename')}
					onkeydown={(e) => e.key === 'Enter' && void renameList()}
				/>
				<button type="button" class="cl-btn" onclick={() => void renameList()}>
					{$t('common.save')}
				</button>
				<button type="button" class="cl-btn" onclick={() => (renameMode = false)}>
					{$t('common.cancel')}
				</button>
			{:else}
				<span class="cl-title">{detail.name}</span>
				<button
					type="button"
					class="cl-mini"
					onclick={() => {
						renameVal = detail?.name ?? '';
						renameMode = true;
					}}>{$t('lists.rename')}</button
				>
				<!-- M9-D: Craft→Mining-Brücke — benötigt Material als Mining-Liste. -->
				<button
					type="button"
					class="cl-mini"
					onclick={() => void bridgeToMining()}
					disabled={bridgeBusy || !detail.aggregated.some((m) => m.quantity > 0)}
					title={$t('lists.bridge_title')}
				>{$t('lists.bridge')}</button
				>
				<button type="button" class="cl-mini is-danger" onclick={() => void deleteList()}>
					{$t('lists.delete')}
				</button>
			{/if}
		</div>

		{#if bridgeNote}
			<div class="cl-inv-note">{bridgeNote}</div>
		{/if}

		<!-- Einträge: Blueprint + copies + Entfernen -->
		{#if detail.items.length}
			<div class="cl-items">
				{#each detail.items as it (it.id)}
					<div class="cl-item">
						<span class="cl-item-name">{it.item_name}</span>
						{#if it.craft_time > 0}
							<span class="cl-item-time"
								>{Math.round(it.craft_time / 60)}
								{$t('lists.min')}</span
							>
						{/if}
						<span class="cl-copies">
							<button
								type="button"
								class="cl-step"
								aria-label={$t('lists.copies_less')}
								onclick={() => void stepCopies(it.id, it.copies, -1)}>−</button
							>
							<span class="cl-copies-n">×{fmtQty(it.copies)}</span>
							<button
								type="button"
								class="cl-step"
								aria-label={$t('lists.copies_more')}
								onclick={() => void stepCopies(it.id, it.copies, 1)}>＋</button
							>
						</span>
						<button
							type="button"
							class="cl-mini is-danger"
							onclick={() => void removeItem(it.id)}>{$t('lists.remove')}</button
						>
					</div>
				{/each}
			</div>
		{:else}
			<div class="cl-hint">{$t('lists.no_items')}</div>
		{/if}

		<!-- Aggregierte Materialmengen + Abhaken (ganz/teilweise) -->
		<div class="cl-agg-head">{$t('lists.aggregated')}</div>
		{#if invNote}
			<div class="cl-inv-note" class:is-raised={invNoteRaised}>
				{invNote}
				{#if invNoteRaised}{$t('lists.req_raised', { q: invRaisedQ })}{/if}
			</div>
		{/if}
		{#if detail.aggregated.length}
			<div class="cl-agg">
				{#each detail.aggregated as m (rowKey(m))}
					{@const p = pct(m)}
					<div class="cl-agg-row">
						<div class="cl-agg-line">
							{#if m.material && detail.items.some((it) => it.mats.some((mm) => mm.material === m.material && mm.ore_id))}
								<button
									type="button"
									class="cl-mat is-ore"
									onclick={() =>
										openOre(
											detail?.items.flatMap((it) => it.mats).find((mm) => mm.material === m.material)
												?.ore_id ?? null
										)}
									title={$t('craft.open_in_mining')}>{m.material}</button
								>
							{:else}
								<span class="cl-mat">{m.material}</span>
							{/if}
							<span class="cl-qty">
								{fmtQty(m.done_qty)}/{fmtQty(m.quantity)}
								<span
									class="cl-unit"
									style={`--uc:${UNIT_COLORS[m.unit] ?? '#b8bfc9'}`}>{unitName(m.unit)}</span
								>
							</span>
							<span class="cl-q" title={$t('lists.quality_req')}>Q ≥ {m.min_quality}</span>
						</div>
						<div class="cl-bar-track" role="progressbar" aria-valuenow={p} aria-valuemin="0" aria-valuemax="100">
							<div class="cl-bar-fill" class:is-done={p >= 100} style={`width:${p}%`}></div>
						</div>
						<div class="cl-agg-actions">
							<button type="button" class="cl-mini" onclick={() => doneAll(m)} disabled={p >= 100}>
								{$t('lists.done_all')}
							</button>
							<input
								class="cl-input cl-partial"
								type="text"
								inputmode="decimal"
								bind:value={partial[rowKey(m)]}
								placeholder={$t('lists.done_partial_ph')}
								aria-label={$t('lists.done_partial')}
								onkeydown={(e) => e.key === 'Enter' && donePartial(m)}
							/>
							<button
								type="button"
								class="cl-mini"
								onclick={() => donePartial(m)}
								disabled={p >= 100}>{$t('lists.done_partial')}</button
							>
							{#if m.done_qty > 0}
								<button type="button" class="cl-mini" onclick={() => void setDone(m, null)}>
									{$t('lists.reset')}
								</button>
							{/if}
								<button type="button" class="cl-mini" onclick={() => openAddInv(m)}>
									{$t('lists.to_inventory')}
								</button>
							{#if addInvFor && addInvFor.material === m.material && addInvFor.unit === m.unit}
								<div class="cl-inv-form">
									<div class="cl-inv-title">{$t('lists.inv_title', { list: detail.name })}</div>
									{#if invError}<div class="cl-err">{invError}</div>{/if}
									<div class="cl-inv-fields">
										<input
											bind:this={invLocEl}
											class="cl-input"
											type="text"
											bind:value={invLoc}
											list="cl-inv-locs"
											placeholder={$t('inventory.location')}
											aria-label={$t('inventory.location')}
											onkeydown={(e) => e.key === 'Enter' && void submitAddInv()}
										/>
										<datalist id="cl-inv-locs">
											{#each invLocs as loc}<option value={loc}></option>{/each}
										</datalist>
										<input
											class="cl-input cl-inv-q"
											type="text"
											inputmode="numeric"
											bind:value={invQ}
											placeholder={$t('inventory.quality')}
											aria-label={$t('inventory.quality')}
											onkeydown={(e) => e.key === 'Enter' && void submitAddInv()}
										/>
										<input
											class="cl-input cl-inv-q"
											type="text"
											inputmode="decimal"
											bind:value={invQty}
											placeholder={$t('lists.done_partial_ph')}
											aria-label={$t('lists.inv_qty')}
											onkeydown={(e) => e.key === 'Enter' && void submitAddInv()}
										/>
										<button type="button" class="cl-btn" onclick={() => void submitAddInv()}>
											{$t('lists.inv_add')}
										</button>
										<button type="button" class="cl-mini" onclick={() => (addInvFor = null)}>
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
			<div class="cl-hint">{$t('lists.no_mats')}</div>
		{/if}
	{/if}
</div>

<style>
	.clists {
		display: flex;
		flex-direction: column;
		gap: 10px;
		min-height: 0;
		flex: 1;
		color: var(--fg, #d7dde5);
	}
	.cl-bar {
		display: flex;
		gap: 10px;
		align-items: flex-start;
		flex-wrap: wrap;
	}
	.cl-chips {
		display: flex;
		gap: 6px;
		flex-wrap: wrap;
		flex: 1;
		min-width: 0;
	}
	.cl-chip {
		background: rgba(255, 255, 255, 0.05);
		border: 1px solid rgba(255, 255, 255, 0.14);
		border-radius: 999px;
		color: inherit;
		padding: 4px 12px;
		font: inherit;
		font-size: 12px;
		cursor: pointer;
	}
	.cl-chip.is-on {
		background: var(--accent, #4aa3ff);
		border-color: transparent;
		color: #0b0f14;
		font-weight: 600;
	}
	.cl-count {
		opacity: 0.65;
		font-size: 11px;
	}
	.cl-new {
		display: flex;
		gap: 6px;
	}
	.cl-input {
		background: rgba(255, 255, 255, 0.06);
		border: 1px solid rgba(255, 255, 255, 0.14);
		border-radius: 6px;
		color: inherit;
		padding: 5px 8px;
		font: inherit;
		font-size: 12px;
		width: 150px;
	}
	.cl-input:focus {
		outline: 1px solid var(--accent, #4aa3ff);
	}
	.cl-btn {
		background: var(--accent, #4aa3ff);
		border: none;
		border-radius: 6px;
		color: #0b0f14;
		padding: 5px 12px;
		font: inherit;
		font-size: 12px;
		font-weight: 600;
		cursor: pointer;
	}
	.cl-mini {
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
	.cl-mini.is-danger {
		color: #ff8a80;
		border-color: rgba(255, 138, 128, 0.35);
	}
	.cl-mini:disabled {
		opacity: 0.4;
		cursor: default;
	}
	.cl-hint {
		opacity: 0.6;
		font-size: 12px;
	}
	.cl-err {
		color: #ff8a80;
		font-size: 12px;
	}
	.cl-head {
		display: flex;
		gap: 8px;
		align-items: center;
	}
	.cl-title {
		font-weight: 700;
		font-size: 15px;
	}
	.cl-items {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.cl-item {
		display: flex;
		gap: 8px;
		align-items: center;
		background: rgba(255, 255, 255, 0.04);
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 8px;
		padding: 6px 10px;
	}
	.cl-item-name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 13px;
	}
	.cl-item-time {
		opacity: 0.55;
		font-size: 11px;
		white-space: nowrap;
	}
	.cl-copies {
		display: flex;
		align-items: center;
		gap: 4px;
		white-space: nowrap;
	}
	.cl-step {
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
	.cl-copies-n {
		font-size: 12px;
		min-width: 40px;
		text-align: center;
	}
	/* M9-B: Abhak-Ebene — Fortschritt + ganz/teilweise. */
	.cl-agg-head {
		font-size: 11px;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		opacity: 0.6;
	}
	.cl-agg {
		display: flex;
		flex-direction: column;
		gap: 8px;
	}
	.cl-agg-row {
		background: rgba(255, 255, 255, 0.04);
		border: 1px solid rgba(255, 255, 255, 0.08);
		border-radius: 8px;
		padding: 8px 10px;
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.cl-agg-line {
		display: flex;
		gap: 10px;
		align-items: baseline;
	}
	.cl-mat {
		font-size: 13px;
		font-weight: 600;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.cl-mat.is-ore {
		background: none;
		border: none;
		color: inherit;
		font: inherit;
		font-size: 13px;
		font-weight: 600;
		cursor: pointer;
		padding: 0;
		text-decoration: underline dotted;
		text-decoration-color: rgba(255, 255, 255, 0.35);
	}
	.cl-qty {
		margin-left: auto;
		white-space: nowrap;
		font-size: 12px;
	}
	.cl-unit {
		color: var(--uc, #b8bfc9);
		font-size: 11px;
	}
	.cl-q {
		opacity: 0.6;
		font-size: 11px;
		white-space: nowrap;
	}
	.cl-bar-track {
		height: 4px;
		border-radius: 2px;
		background: rgba(255, 255, 255, 0.08);
		overflow: hidden;
	}
	.cl-bar-fill {
		height: 100%;
		background: var(--accent, #4aa3ff);
		border-radius: 2px;
	}
	.cl-bar-fill.is-done {
		background: #6fdc8c;
	}
	.cl-agg-actions {
		display: flex;
		gap: 6px;
		align-items: center;
		flex-wrap: wrap;
	}
	.cl-partial {
		width: 74px;
		padding: 3px 8px;
	}
	/* M9-E: Inventar-Anbindung — Hinzufügen mit Ort+Qualität. */
	.cl-inv-note {
		font-size: 12px;
		padding: 6px 10px;
		border-radius: 6px;
		background: rgba(111, 220, 140, 0.12);
		border: 1px solid rgba(111, 220, 140, 0.3);
		color: #6fdc8c;
	}
	.cl-inv-form {
		display: flex;
		flex-direction: column;
		gap: 6px;
		background: rgba(255, 255, 255, 0.03);
		border: 1px solid rgba(255, 255, 255, 0.1);
		border-radius: 6px;
		padding: 8px;
	}
	.cl-inv-title {
		font-size: 12px;
		opacity: 0.75;
	}
	.cl-inv-fields {
		display: flex;
		gap: 6px;
		align-items: center;
		flex-wrap: wrap;
	}
	.cl-inv-q {
		width: 74px;
		padding: 3px 8px;
	}
</style>
