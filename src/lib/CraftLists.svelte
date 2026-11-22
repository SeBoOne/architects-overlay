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
	import { t, trError } from './i18n';
	import { invoke } from '@tauri-apps/api/core';
	// M9-D (t_36f3966c): Craft→Mining-Brücke — Aggregation → Mining-Listen-Einträge.
	import { bridgeItems } from './list-bridge.ts';
	// M9-K (t_db9b234f, P11): Listen-Umschalter als Custom-Dropdown statt Chips.
	import ListPicker from './ListPicker.svelte';

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
		// M9-F (t_bb97da62): aus dem Inventar reserviert (mengengewichtete Ø-Q).
		reserved_qty: number;
		reserved_avg_quality: number | null;
		// M9-M (t_74833eb0): Fortschritt + „Ziel erreicht“ aus dem Backend
		// (list_detail) — EINE Quelle der Wahrheit für beide Listen + Spiegel.
		progress: number;
		reached: boolean;
	};
	// M9-F: Inventar-Zeile als Reservierungs-Quelle (Mirror inventory_list).
	type InvRow = {
		id: string;
		name: string;
		quality: number;
		location_name: string;
		quantity: number;
		reservations: { id: string; qty: number; purpose: string; list_id: string | null }[];
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
		onOpenMiningList = null,
		// M9-F (t_bb97da62): Inventar-Eingabemaske aus der Crafting-Liste öffnen.
		onOpenInventory = null
		}: {
			onOpenOre?: ((oreId: string) => void) | null;
			onOpenMiningList?: ((listId: number) => void) | null;
			onOpenInventory?: (() => void) | null;
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
			invError = trError(e);
		}
	}

	// M9-F (t_bb97da62) — Aus Inventar reservieren: Quellen-Zeilen des
	// Materials (alle Qualitäten/Standorte, freie Mengen), Auswahl per
	// Chips (KEIN natives <select>), partielle Mengen erlaubt.
	// M9-I (t_e7f0b3a6) — Bearbeiten aus der Liste: dieselbe Maske im
	// Edit-Modus, vorgefüllt mit dem Ist-Stand (je Quelle Menge), Löschen
	// je Quelle per ✕ (reservation_delete), Mengen-Änderung via
	// reservation_update. Der harte Listen-Cap wird serverseitig erzwungen.
	let reserveFor = $state<AggMat | null>(null);
	let reserveRows = $state<InvRow[]>([]);
	let reserveSel = $state<Record<string, string>>({}); // id → qty-Text
	let reserveEdit = $state(false);
	// M9-I: je Inventar-Zeile die Reservierungs-Ids DIESER Liste (edit).
	let reserveResIds = $state<Record<string, string[]>>({});
	let reserveBusy = $state(false);
	let reserveError = $state('');
	// M9-N: Hinweis nach „Liste abschließen“.
	let completeNote = $state('');
	async function openReserve(m: AggMat) {
		reserveFor = m;
		reserveSel = {};
		reserveEdit = false;
		reserveResIds = {};
		reserveError = '';
		reserveRows = [];
		try {
			const all = await invoke<InvRow[]>('inventory_list');
			reserveRows = all.filter((r) => r.name.toLowerCase() === m.material.toLowerCase());
		} catch (e) {
			reserveError = trError(e);
		}
	}
	/// M9-I: Bearbeiten — Maske mit dem Ist-Stand vorgefüllt (je Quelle die
	/// Menge, die für DIESE Liste reserviert ist).
	async function openEditReserve(m: AggMat) {
		reserveFor = m;
		reserveEdit = true;
		reserveSel = {};
		reserveResIds = {};
		reserveError = '';
		reserveRows = [];
		try {
			const all = await invoke<InvRow[]>('inventory_list');
			reserveRows = all.filter((r) => r.name.toLowerCase() === m.material.toLowerCase());
			for (const r of reserveRows) {
				const own = r.reservations.filter((x) => x.list_id === String(activeId));
				if (own.length) {
					reserveResIds[r.id] = own.map((x) => x.id);
					reserveSel[r.id] = String(Math.round(own.reduce((s, x) => s + x.qty, 0) * 100) / 100);
				}
			}
		} catch (e) {
			reserveError = trError(e);
		}
	}
	/// M9-I: Quelle aus der Liste löschen (alle Reservierungen der Quelle
	/// für diese Liste) — die Maske bleibt offen und zeigt den Stand; das
	/// Detail (Chip hinter der Maske) wird live nachgezogen (list_detail
	/// klemmt serverseitig und liefert die frische Aggregation).
	async function removeResRow(r: InvRow) {
		const ids = reserveResIds[r.id] ?? [];
		if (!ids.length) return;
		reserveBusy = true;
		reserveError = '';
		try {
			for (const id of ids) await invoke('reservation_delete', { id });
			if (activeId !== null) detail = await invoke<ListDetail>('list_detail', { listId: activeId });
			if (reserveFor) await openEditReserve(reserveFor);
		} catch (e) {
			reserveError = trError(e);
		} finally {
			reserveBusy = false;
		}
	}
	const freeQty = (r: InvRow) =>
		Math.max(0, r.quantity - r.reservations.reduce((s, x) => s + x.qty, 0));
	async function submitReserve() {
		if (reserveFor === null || activeId === null || reserveBusy) return;
		const m = reserveFor;
		// M9-I: Edit-Modus — je Quelle: leere Eingabe/0 = löschen, Wert =
		// reservation_update (nicht für DIESE Liste reservierte Quellen
		// bleiben der additive Pick-Flow).
		if (reserveEdit) {
			reserveBusy = true;
			reserveError = '';
			try {
				let total = 0;
				for (const r of reserveRows) {
					const raw = reserveSel[r.id];
					const ids = reserveResIds[r.id] ?? [];
					if (ids.length) {
						const q = raw === undefined ? NaN : Number(String(raw).replace(',', '.'));
						if (!(q > 0)) {
							for (const id of ids) await invoke('reservation_delete', { id });
						} else {
							await invoke('reservation_update', { id: ids[0], qty: q });
							for (const id of ids.slice(1)) await invoke('reservation_delete', { id });
							total += q;
						}
					} else if (raw !== undefined) {
						const q = Number(String(raw).replace(',', '.'));
						if (q > 0) {
							await invoke('reservation_add', {
								'new': {
									inventory_id: r.id,
									list_id: String(activeId),
									purpose: '',
									qty: q,
									note: m.material
								}
							});
							total += q;
						}
					}
				}
				await openList(activeId);
				reserveNote = $t('lists.reserved_note2', { q: total, name: m.material, list: detail?.name ?? '' });
				reserveFor = null;
			} catch (e) {
				reserveError = trError(e);
			} finally {
				reserveBusy = false;
			}
			return;
		}
		const picks = Object.entries(reserveSel)
			.map(([id, raw]) => ({ id, qty: Number(raw.replace(',', '.')) }))
			.filter((p) => p.qty > 0);
		if (!picks.length) {
			reserveError = $t('inventory.err_purpose');
			return;
		}
		reserveBusy = true;
		reserveError = '';
		try {
			let total = 0;
			for (const p of picks) {
				await invoke('reservation_add', {
					'new': {
						inventory_id: p.id,
						list_id: String(activeId),
						purpose: '',
						qty: p.qty,
						note: m.material
					}
				});
				total += p.qty;
			}
			await openList(activeId);
			reserveNote = $t('lists.reserved_note2', { q: total, name: m.material, list: detail?.name ?? '' });
			reserveFor = null;
		} catch (e) {
			reserveError = trError(e);
		} finally {
			reserveBusy = false;
		}
	}
	let reserveNote = $state('');
	let reserveAllBusy = $state(false);

	// M9-J (t_14cc5d4e): „Alles reservieren" — Backend-Greedy best→worst über
	// alle Materialien der Liste (Ø-Qualitätsregel je Material, Cap Summe ≤
	// Bedarf). Re-Click = komplette Neuberechnung: Alt-Reservierungen der
	// Liste werden ersetzt, besseres/neues Inventar wird aufgenommen.
	async function reserveAll() {
		if (activeId === null || reserveAllBusy) return;
		reserveAllBusy = true;
		reserveError = '';
		reserveNote = '';
		try {
			const r = await invoke<{ created: number }>('lists_reserve_all', {
				listId: activeId
			});
			await openList(activeId);
			reserveNote = $t('lists.reserve_all_note', { n: r.created });
		} catch (e) {
			reserveError = String(e);
		} finally {
			reserveAllBusy = false;
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
	// M9-M (t_74833eb0): Fortschritt kommt als Backend-Metrik (progress in
	// AggMat, done + reserved gegen den Bedarf) — nicht mehr done/quantity.
	const pct = (m: AggMat) => (m.quantity > 0 ? m.progress : 0);

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
		// M9-F: Reservierungs-Hinweis/Picker ebenso verfallen lassen.
		reserveNote = '';
		reserveError = '';
		reserveFor = null;
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
	// M9-N: Liste abschließen — reservierte Mengen aus dem Inventar entnehmen.
	async function completeList() {
		if (activeId === null) return;
		error = '';
		try {
			await invoke('list_complete', { listId: activeId });
			completeNote = $t('lists.completed');
			await loadLists();
			await openList(activeId);
		} catch (e) {
			error = trError(e);
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
	<!-- M9-K (P11): Listen-Umschalter als Custom-Dropdown statt Chips. -->
	<div class="cl-bar">
		<ListPicker
			items={lists.map((l) => ({
				id: l.id,
				name: l.name,
				item_count: l.item_count,
				source_name: null
			}))}
			{activeId}
			onopen={(id) => void openList(id)}
			emptyHint={$t('lists.empty')}
		/>
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
				<!-- M9-F: Inventar-Eingabemaske aus der Crafting-Liste öffnen. -->
				<button
					type="button"
					class="cl-mini"
					onclick={() => onOpenInventory?.()}
					title={$t('lists.open_inventory')}
				>{$t('lists.open_inventory')}</button
				>
				<!-- M9-J: Alles reservieren — Greedy über alle Materialien. -->
				<button
					type="button"
					class="cl-mini"
					onclick={() => void reserveAll()}
					disabled={reserveAllBusy || !detail.aggregated.some((m) => m.quantity > 0)}
					title={$t('lists.reserve_all_title')}
				>{$t('lists.reserve_all')}</button
				>
				<!-- M9-N: Liste abschließen — reservierte Mengen konsumieren. -->
				<button
					type="button"
					class="cl-mini"
					onclick={() => void completeList()}
					title={$t('lists.complete_title')}
				>{$t('lists.complete')}</button
				>
				<button type="button" class="cl-mini is-danger" onclick={() => void deleteList()}>
					{$t('lists.delete')}
				</button>
			{/if}
		</div>

		{#if bridgeNote}
			<div class="cl-inv-note">{bridgeNote}</div>
		{/if}
		{#if completeNote}
			<div class="cl-inv-note">{completeNote}</div>
		{/if}
		{#if reserveNote}
			<div class="cl-inv-note">{reserveNote}</div>
		{/if}
		{#if reserveError}
			<div class="cl-err">{reserveError}</div>
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
							{#if m.reserved_qty > 0}
								<span class="cl-resv" title={$t('lists.reserve_title')}>
									{$t('lists.reserved_chip', { q: fmtQty(m.reserved_qty), qq: m.reserved_avg_quality ?? '—' })}
								</span>
							{/if}
							</div>
							<div class="cl-bar-track" role="progressbar" aria-valuenow={p} aria-valuemin="0" aria-valuemax="100">
							<div class="cl-bar-fill" class:is-done={m.reached} style={`width:${p}%`}></div>
							</div>
							<div class="cl-agg-actions">
							<button type="button" class="cl-mini" onclick={() => openReserve(m)} disabled={reserveBusy}>
								{$t('lists.reserve_btn')}
							</button>
							{#if m.reserved_qty > 0}
								<button type="button" class="cl-mini" onclick={() => openEditReserve(m)} disabled={reserveBusy}>
									{$t('lists.edit_reserve_btn')}
								</button>
							{/if}
							{#if reserveFor && reserveFor.material === m.material && reserveFor.unit === m.unit}
								<div class="cl-inv-form">
									<div class="cl-inv-title">
										{#if reserveEdit}
											{$t('lists.edit_reserve_title', { req: fmtQty(m.quantity), res: fmtQty(m.reserved_qty) })}
										{:else}
											{$t('lists.reserve_title')}
										{/if}
									</div>
									{#if reserveError}<div class="cl-err">{reserveError}</div>{/if}
									{#if !reserveRows.length}
										<div class="cl-hint">{$t('inventory.no_positions_material')}</div>
									{:else}
										<div class="cl-reserve-rows">
											{#each reserveRows as r (r.id)}
												{@const free = freeQty(r)}
												{@const own = reserveEdit ? (reserveResIds[r.id]?.length ?? 0) : 0}
												<div class="cl-res-row">
													<button
														type="button"
														class="cl-res-pick"
														class:is-on={reserveSel[r.id] !== undefined}
														onclick={() => reserveSel[r.id] === undefined ? reserveSel[r.id] = String(free) : delete reserveSel[r.id]}
														title={r.location_name}
													>Q{r.quality} · {r.location_name} · {$t('inventory.qty_breakdown', { free: fmtQty(free), total: fmtQty(r.quantity) })}</button>
													{#if reserveSel[r.id] !== undefined}
														<input
															class="cl-input cl-res-qty"
															type="text"
															inputmode="decimal"
															bind:value={reserveSel[r.id]}
															placeholder={$t('lists.reserve_src_ph')}
															aria-label={$t('lists.reserve_src_ph')}
															onkeydown={(e) => e.key === 'Enter' && void submitReserve()}
														/>
														{#if own}
															<button
																type="button"
																class="cl-res-del"
																onclick={() => void removeResRow(r)}
																title={$t('common.delete')}
																aria-label={$t('common.delete')}
															>✕</button>
														{/if}
													{/if}
												</div>
											{/each}
										</div>
										<button type="button" class="cl-btn" onclick={() => void submitReserve()} disabled={reserveBusy}>
											{#if reserveEdit}{$t('common.save')}{:else}{$t('lists.inv_add')}{/if}
										</button>
										<button type="button" class="cl-mini" onclick={() => (reserveFor = null)}>
											{$t('common.cancel')}
										</button>
									{/if}
								</div>
							{/if}
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
		/* M9-H (t_7709c7f2, Punkt 6): eigener Scroll + Padding — wie .mlists. */
		overflow-y: auto;
		padding: 2px 10px 12px 2px;
	}
	.clists::-webkit-scrollbar {
		width: 8px;
	}
	.clists::-webkit-scrollbar-thumb {
		background: color-mix(in srgb, var(--accent, #4aa3ff) 35%, transparent);
		border-radius: 4px;
	}
	.cl-bar {
		display: flex;
		gap: 10px;
		align-items: flex-start;
		flex-wrap: wrap;
	}
	/* M9-K (P11): Chips-Zeile durch ListPicker-Dropdown ersetzt — Chips-CSS
	   entfernt. */
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
	/* M9-F: Aus Inventar reservieren — Chip + Quellen-Picker. */
	.cl-resv {
		color: #e0a83e;
		font-size: 11px;
		white-space: nowrap;
	}
	.cl-reserve-rows {
		display: flex;
		flex-direction: column;
		gap: 3px;
		max-height: 140px;
		overflow-y: auto;
	}
	.cl-res-row {
		display: flex;
		gap: 6px;
		align-items: center;
	}
	.cl-res-pick {
		border: 1px solid rgba(255, 255, 255, 0.14);
		background: rgba(255, 255, 255, 0.05);
		border-radius: 6px;
		color: inherit;
		font: inherit;
		font-size: 11px;
		padding: 3px 8px;
		cursor: pointer;
		text-align: left;
	}
	.cl-res-pick.is-on {
		background: var(--accent, #4aa3ff);
		border-color: transparent;
		color: #0b0f14;
		font-weight: 600;
	}
	.cl-res-qty {
		width: 70px;
		padding: 3px 8px;
	}
	/* M9-I: Quelle je Qualitätsstufe aus der Reservierung löschen. */
	.cl-res-del {
		border: 1px solid rgba(255, 255, 255, 0.14);
		background: rgba(255, 255, 255, 0.05);
		border-radius: 6px;
		color: inherit;
		font: inherit;
		font-size: 11px;
		padding: 3px 8px;
		cursor: pointer;
	}
	.cl-res-del:hover {
		color: #ff7a7a;
		border-color: rgba(255, 122, 122, 0.5);
	}
</style>
