<script lang="ts">
	import {  t, tr } from '../lib/i18n';
// A11y-Helfer: Enter/Space aktiviert klickbare Nicht-Button-Elemente (nur bei Eigenfokus).
function keyActivate(e: KeyboardEvent) {
	if (e.target !== e.currentTarget) return;
	if (e.key !== 'Enter' && e.key !== ' ') return;
	e.preventDefault();
	(e.currentTarget as HTMLElement).click();
}
	// Inventar-Modus (Karte t_1bab1fea, v0.3.0 K2) — lokal, privat,
	// Qualitätsbewusst. Eine Position = (Item-/Commodity-Referenz ODER
	// Freitext, Qualität 0–1000 (Pflicht), Standort (Location-Entity ODER
	// Freitext-Depot), Menge, optional Kaufpreis aUEC). Keine Sync-/API-
	// Anbindung — die Daten leben ausschließlich in der Cache-DB.
	//
	// Views: Gesamtübersicht (nach Standort gruppiert, Items darunter),
	// Item-Detail (alle Qualitäten/Standorte eines Materials + Summen).
	// Filter/Sortierung über die Chips; das HAUPT-Suchfeld der Palette
	// filtert live (gleiche In-Context-Metapher wie die Detail-Suche).
	// Tastatur: ↑↓ Zeile · ↵ Bearbeiten/Detail · Entf Löschen · Esc zurück.
	import { invoke } from '@tauri-apps/api/core';

	type InvEntry = {
		// Ids kommen als STRINGS über die IPC (Wiki-Ids > 2^53 runden als JS-
		// Number still — t_925a577a-Konvention, t_1bab1fea Blocker 2).
		id: string;
		entity_kind: string | null;
		entity_id: string | null;
		name: string;
		quality: number;
		location_id: string | null;
		location_name: string;
		quantity: number;
		price: number | null;
		created_at: number;
		updated_at: number;
	};

	// Suchtreffer (Mirror der SearchHit-DTO in +page.svelte) für den
	// Entity-Picker im Hinzufügen-Formular.
	type SearchHit = { id: string; name: string; kind: string; subtitle: string; code: string | null };

	type SortMode = 'loc' | 'qty' | 'quality';

	let { onexit, filter = $bindable('') }: { onexit: () => void; filter?: string } = $props();

	// ---- State ---------------------------------------------------------------
	let rows = $state<InvEntry[]>([]);
	let loaded = $state(false);
	let sortMode = $state<SortMode>('loc');
	let view = $state<'overview' | 'item' | 'location'>('overview');
	let itemDetail = $state<string | null>(null); // Materialname des Item-Detail
	let locationDetail = $state<string | null>(null); // Standortname des Standort-Detail
	let sel = $state(0);
	let toast = $state('');
	let toastTimer: ReturnType<typeof setTimeout> | null = null;

	// P13 (K5): Switch List-View ⇄ Cards (grid of cards per Stand-ort,
	// Item-Groups inside the cards). Persistiert via localStorage
	// (same pattern as Palette-fetchedAt → survives app restart).
	const INV_CARDS_KEY = 'architects-overlay.inventory.cards';
	let cardView = $state(localStorage.getItem(INV_CARDS_KEY) === '1');
	function toggleCardView() {
		cardView = !cardView;
		try {
			localStorage.setItem(INV_CARDS_KEY, cardView ? '1' : '0');
		} catch {
			/* localStorage incomplete — Switch stays active in-Memory. */
		}
		sel = 0;
	}

	// P14 (K5): 4 Quality-Steps (previously 2 with 750-border): <500 Bad
	// (reddish) · 500–749 ok · 750–899 good · 900–1000 Very good. q-chip-
	// class everywhere (also in the card view — Consistent).
	function qTier(q: number): string {
		if (q < 500) return 'bad';
		if (q < 750) return 'ok';
		if (q < 900) return 'good';
		return 'top';
	}

	// Formular (Hinzufügen/Bearbeiten). `editingId = null` → Hinzufügen.
	let inForm = $state(false);
	let editingId = $state<string | null>(null);
	let fEntityKind = $state<'commodity' | 'item' | null>(null);
	let fEntityId = $state<string | null>(null);
	let fName = $state('');
	let fQuality = $state('');
	let fLocationId = $state<string | null>(null);
	let fLocation = $state('');
	let fQty = $state('');
	let fPrice = $state('');
	let formError = $state('');

	// Entity-Picker: Suche im Formular (bestehende entities) + Freitext-Fall.
	let pickQuery = $state('');
	let pickHits = $state<SearchHit[]>([]);
	let pickBusy = $state(false);
	let pickIdx = $state(0);
	let locIdx = $state(0);
	let pickInputEl = $state<HTMLInputElement | null>(null);
	let locInputEl = $state<HTMLInputElement | null>(null);
	let qualityEl = $state<HTMLInputElement | null>(null);
	let pickTimer: ReturnType<typeof setTimeout> | null = null;
	// Standort-Vorschläge: bestehende Inventar-Standorte + Location-Entities.
	let locSuggest = $state<string[]>([]);
	let locEntities = $state<SearchHit[]>([]);

	// ---- Laden ---------------------------------------------------------------
	async function reload() {
		try {
			rows = await invoke<InvEntry[]>('inventory_list');
			loaded = true;
		} catch (e) {
			toastMsg(String(e));
			loaded = true;
		}
	}
	$effect(() => {
		void reload();
	});

	function toastMsg(t: string) {
		toast = t;
		if (toastTimer) clearTimeout(toastTimer);
		toastTimer = setTimeout(() => (toast = ''), 4000);
	}

	function fmtQ(n: number): string {
		return new Intl.NumberFormat('de-DE', { maximumFractionDigits: 0 }).format(n);
	}
	function fmtQty(n: number): string {
		const r = Math.abs(n % 1) < 1e-9 ? fmtQ(n) : n.toLocaleString('de-DE', { maximumFractionDigits: 2 });
		return r;
	}
	function fmtPrice(p: number | null): string {
		return p ? new Intl.NumberFormat('de-DE', { maximumFractionDigits: 0 }).format(p) : '—';
	}

	// ---- Views (derived) -----------------------------------------------------
	// Gefilterte Zeilen: das Haupt-Suchfeld der Palette filtert über
	// Standort- UND Materialnamen (Leerstring = alles).
	let visRows = $derived(
		rows.filter((r) => {
			const f = filter.trim().toLowerCase();
			if (!f) return true;
			return r.name.toLowerCase().includes(f) || r.location_name.toLowerCase().includes(f);
		})
	);

	// Sortierung: Standort (default, gruppierend) / Menge absteigend /
	// Qualität absteigend. Qualität nebeneinander → zweite Ordnung Menge.
	let sortedRows = $derived.by(() => {
		const out = [...visRows];
		if (sortMode === 'qty') out.sort((a, b) => b.quantity - a.quantity);
		else if (sortMode === 'quality') out.sort((a, b) => b.quality - a.quality || b.quantity - a.quantity);
		return out;
	});

	// Gruppen nur im Standort-Sortiment: nach Standort gruppiert, zweite
	// Ebene = Item-Subgroups (Material → Positionen, Qualität absteigend).
	// Übrige Sortierungen = flach.
	function matGroups(entries: InvEntry[]) {
		const map = new Map<string, InvEntry[]>();
		for (const e of entries) {
			if (!map.has(e.name)) map.set(e.name, []);
			map.get(e.name)!.push(e);
		}
		return [...map.entries()].map(([name, list]) => ({
			name,
			list,
			total: list.reduce((s, e) => s + e.quantity, 0)
		}));
	}
	let groups = $derived.by(() => {
		if (sortMode !== 'loc') return [];
		const map = new Map<string, InvEntry[]>();
		for (const r of sortedRows) {
			if (!map.has(r.location_name)) map.set(r.location_name, []);
			map.get(r.location_name)!.push(r);
		}
		return [...map.entries()].map(([loc, entries]) => ({
			loc,
			entries,
			total: entries.reduce((s, e) => s + e.quantity, 0),
			mats: matGroups(entries)
		}));
	});

	// Cards-view-Groups: Grouping per MATERIAL (QX) independent of the
	// sortmode; the cards react to the Sort-Chips (qty → Quantity,
	// quality → Ø Quality, loc → Name). Subgroups inside each card = Stand-ort.
	let cardGroups = $derived.by(() => {
		const map = new Map<string, InvEntry[]>();
		for (const r of sortedRows) {
			if (!map.has(r.name)) map.set(r.name, []);
			map.get(r.name)!.push(r);
		}
		const arr = [...map.entries()].map(([mat, entries]) => {
			const total = entries.reduce((s, e) => s + e.quantity, 0);
			const avgQ = Math.round(entries.reduce((s, e) => s + e.quality * e.quantity, 0) / Math.max(total, 1e-9));
			const locMap = new Map<string, InvEntry[]>();
			for (const e of entries) {
				if (!locMap.has(e.location_name)) locMap.set(e.location_name, []);
				locMap.get(e.location_name)!.push(e);
			}
			const locs = [...locMap.entries()].map(([loc, list]) => ({
				loc,
				list,
				total: list.reduce((s, e) => s + e.quantity, 0)
			}));
			return { mat, entries, total, avgQ, locs };
		});
		if (sortMode === 'qty') arr.sort((a, b) => b.total - a.total);
		else if (sortMode === 'quality') arr.sort((a, b) => b.avgQ - a.avgQ);
		else arr.sort((a, b) => (a.mat < b.mat ? -1 : 1));
		return arr;
	});

	// Flaches Navigationsmodell je Ansicht. `sub`-Zeilen sind die Item-
	// Subgroup-Header (zweite Gruppierungsebene), reine Anzeige.
	type NavRow = { entry: InvEntry | null; kind: 'header' | 'sub' | 'row'; label?: string };
	let navRows = $derived.by((): NavRow[] => {
		if (view === 'item' && itemDetail) {
			const mine = sortedRows.filter((r) => r.name === itemDetail);
			return mine.map((entry) => ({ entry, kind: 'row' as const }));
		}
		if (view === 'location' && locationDetail) {
			const mine = sortedRows.filter((r) => r.location_name === locationDetail);
			const out: NavRow[] = [];
			for (const m of matGroups(mine)) {
				out.push({ entry: null, kind: 'sub', label: m.name });
				for (const e of m.list) out.push({ entry: e, kind: 'row' });
			}
			return out;
		}
		if (cardView) {
			const out: NavRow[] = [];
			for (const g of cardGroups) {
				out.push({ entry: null, kind: 'header', label: g.mat });
				for (const l of g.locs) {
					out.push({ entry: null, kind: 'sub', label: l.loc });
					for (const e of l.list) out.push({ entry: e, kind: 'row' });
				}
			}
			return out;
		}
		if (sortMode === 'loc') {
			const out: NavRow[] = [];
			for (const g of groups) {
				out.push({ entry: null, kind: 'header', label: g.loc });
				for (const m of g.mats) {
					out.push({ entry: null, kind: 'sub', label: m.name });
					for (const e of m.list) out.push({ entry: e, kind: 'row' });
				}
			}
			return out;
		}
		return sortedRows.map((entry) => ({ entry, kind: 'row' as const }));
	});

	// Selektion klemmen (Filter/Formularwechsel schrumpfen die Liste).
	$effect(() => {
		const top = Math.max(navRows.filter((n) => n.kind === 'row').length - 1, 0);
		if (sel > top) sel = top;
	});

	// Item-Detail: alle Positionen eines Materials + Summen.
	let itemRows = $derived(
		itemDetail ? visRows.filter((r) => r.name === itemDetail) : []
	);
	let itemTotal = $derived(itemRows.reduce((s, r) => s + r.quantity, 0));
	let itemAvgQ = $derived.by(() => {
		if (!itemRows.length) return 0;
		return Math.round(
			itemRows.reduce((s, r) => s + r.quality * r.quantity, 0) /
				Math.max(itemRows.reduce((s, r) => s + r.quantity, 0), 1e-9)
		);
	});

	// Standort-Detail: alle Positionen eines Standorts + Summen. Die Item-
	// Subgroups rendert die Ansicht selbst; navRows nutzt dieselbe Aufteilung.
	let locRows = $derived(
		locationDetail ? visRows.filter((r) => r.location_name === locationDetail) : []
	);
	let locTotal = $derived(locRows.reduce((s, r) => s + r.quantity, 0));
	let locMatCount = $derived(new Set(locRows.map((r) => r.name)).size);
	let locAvgQ = $derived.by(() => {
		if (!locRows.length) return 0;
		return Math.round(
			locRows.reduce((s, r) => s + r.quality * r.quantity, 0) /
				Math.max(locRows.reduce((s, r) => s + r.quantity, 0), 1e-9)
		);
	});

	// Distinct-Materialien (für die Übersichtszahl).
	let materialCount = $derived(new Set(visRows.map((r) => r.name)).size);

	// ---- Aktionen ------------------------------------------------------------
	function openItem(name: string) {
		itemDetail = name;
		view = 'item';
		sel = 0;
	}

	/// Standort-Detail: alle Positionen eines Standorts (mit Item-Subgroups).
	function openLocation(name: string) {
		locationDetail = name;
		view = 'location';
		sel = 0;
	}

	function back() {
		if (inForm) {
			closeForm();
			return;
		}
		if (view === 'item') {
			view = 'overview';
			itemDetail = null;
			blur();
			return;
		}
		if (view === 'location') {
			view = 'overview';
			locationDetail = null;
			blur();
			return;
		}
		onexit();
	}

	/// Fokus auf body zurückgeben (Entf-Löschen und die Root-Esc-Kette der
	/// Palette arbeiten auf document.body bzw. dem Suchfeld).
	function blur() {
		const a = document.activeElement;
		if (a instanceof HTMLElement && a !== document.body) a.blur();
	}

	async function removeRow(id: string) {
		try {
			await invoke('inventory_delete', { id });
			rows = rows.filter((r) => r.id !== id);
			toastMsg(tr('inventory.deleted_note'));
		} catch (e) {
			toastMsg(String(e));
		}
	}

	function openAdd() {
		editingId = null;
		fEntityKind = null;
		fEntityId = null;
		fName = '';
		fQuality = '';
		fLocationId = null;
		fLocation = '';
		fQty = '';
		fPrice = '';
		formError = '';
		pickQuery = '';
		pickHits = [];
		pickIdx = 0;
		locIdx = 0;
		inForm = true;
		void loadLocSuggestions();
		// Keyboard-first: der Cursor startet direkt im Material-Suchfeld.
		setTimeout(() => pickInputEl?.focus(), 0);
	}

	function openEdit(e: InvEntry) {
		editingId = e.id;
		fEntityKind = (e.entity_kind as 'commodity' | 'item' | null) ?? null;
		fEntityId = e.entity_id;
		fName = e.name;
		fQuality = String(e.quality);
		fLocationId = e.location_id;
		fLocation = e.location_name;
		fQty = String(e.quantity);
		fPrice = e.price === null ? '' : String(e.price);
		formError = '';
		pickQuery = '';
		pickHits = [];
		inForm = true;
		void loadLocSuggestions();
		// Bearbeiten: Material steht fest — Cursor startet im Qualität-Feld.
		setTimeout(() => qualityEl?.focus(), 0);
	}

	function closeForm() {
		inForm = false;
		editingId = null;
		formError = '';
		pickHits = [];
		locEntities = [];
		blur();
	}

	async function loadLocSuggestions() {
		try {
			locSuggest = await invoke<string[]>('inventory_locations');
		} catch {
			locSuggest = [];
		}
	}

	function onPickInput() {
		pickIdx = 0;
		if (pickTimer) clearTimeout(pickTimer);
		const q = pickQuery.trim();
		if (!q) {
			pickHits = [];
			return;
		}
		pickTimer = setTimeout(() => void runPick(q), 180);
	}

	async function runPick(q: string) {
		pickBusy = true;
		try {
			const resp = await invoke<{ groups: { kind: string; hits: SearchHit[] }[]; total: number }>(
				'search',
				{ query: q, filters: null }
			);
			pickHits = resp.groups
				.filter((g) => g.kind === 'commodity' || g.kind === 'item')
				.flatMap((g) => g.hits);
		} catch (e) {
			pickHits = [];
			formError = String(e);
		}
		pickBusy = false;
	}

	function chooseEntity(h: SearchHit) {
		fEntityKind = h.kind === 'item' ? 'item' : 'commodity';
		// String-Id unangetastet durchreichen (Wiki-Ids > 2^53 runden als
		// JS-Number still — t_1bab1fea Blocker 2, Muster t_925a577a).
		fEntityId = h.id;
		fName = h.name;
		pickIdx = 0;
		// Keyboard-Flow: nach der Materialwahl landet der Cursor im
		// Pflichtfeld Qualität.
		if (qualityEl) qualityEl.focus();
	}

	function chooseFreeText() {
		fEntityKind = null;
		fEntityId = null;
		fName = pickQuery.trim();
	}

	function onLocInput() {
		fLocationId = null; // Freitext, bis ein Entity-Vorschlag gewählt wird
		locIdx = 0;
		const q = fLocation.trim();
		if (!q) {
			locEntities = [];
			return;
		}
		if (pickTimer) clearTimeout(pickTimer);
		pickTimer = setTimeout(() => void runLocSearch(q), 180);
	}

	async function runLocSearch(q: string) {
		try {
			const resp = await invoke<{ groups: { kind: string; hits: SearchHit[] }[]; total: number }>(
				'search',
				{ query: q, filters: ['location'] }
			);
			locEntities = resp.groups.flatMap((g) => g.hits).filter((h) => h.kind === 'location');
		} catch {
			locEntities = [];
		}
	}

	function chooseLocation(h: SearchHit) {
		fLocation = h.name;
		// String-Id unangetastet durchreichen (Wiki-Ids > 2^53 runden als
		// JS-Number still — t_1bab1fea Blocker 2, Muster t_925a577a).
		fLocationId = h.id;
		locEntities = [];
		locIdx = 0;
	}

	async function saveForm() {
		const quality = parseInt(String(fQuality), 10);
		const qty = parseFloat(String(fQty).replace(',', '.'));
		if (!fName.trim()) {
			formError = tr('inventory.err_material');
			return;
		}
		if (!Number.isFinite(quality) || quality < 0 || quality > 1000) {
			formError = tr('inventory.err_quality');
			return;
		}
		if (!fLocation.trim()) {
			formError = tr('inventory.err_location');
			return;
		}
		if (!Number.isFinite(qty) || qty < 0) {
			formError = tr('inventory.err_qty');
			return;
		}
		const priceRaw = String(fPrice).trim().replace(',', '.');
		const price = priceRaw ? parseFloat(priceRaw) : null;
		if (priceRaw && !Number.isFinite(price)) {
			formError = tr('inventory.err_price');
			return;
		}
		const payload = {
			entity_kind: fEntityKind,
			entity_id: fEntityId,
			name: fName.trim(),
			quality,
			location_id: fLocationId,
			location_name: fLocation.trim(),
			quantity: qty,
			price
		};
		try {
			if (editingId === null) {
				const added = await invoke<InvEntry>('inventory_add', { entry: payload });
				rows = [...rows, added];
				toastMsg(`Position erfasst: ${added.name} Q${added.quality} · ${added.location_name}`);
			} else {
				await invoke('inventory_update', { id: editingId, entry: payload });
				rows = rows.map((r) =>
					r.id === editingId ? { ...r, ...payload, updated_at: r.updated_at } : r
				);
				toastMsg(tr('inventory.changed_note'));
			}
			closeForm();
			void loadLocSuggestions();
		} catch (e) {
			formError = String(e);
		}
	}

	// ---- Tastatur (nur Inventar-Modus; F-Tasten/Esc-Root bleiben bei der
	// Palette — hier greift ausschließlich dieser Handler) --------------------
	function onKey(e: KeyboardEvent) {
		if (inForm) {
			// Esc im Formular = Abbrechen. Eingaben tippen bleibt normal.
			if (e.key === 'Escape') {
				closeForm();
				e.preventDefault();
				return;
			}
			// Keyboard-first Erfassen (analog Detailsuche): Enter wählt den
			// markierten Entity-/Location-Vorschlag, ↓/↑ wandern in der
			// Vorschlagsliste, Strg+Enter speichert von jedem Feld aus.
			const t = e.target;
			const inPick = t === pickInputEl || t === locInputEl || (t instanceof HTMLElement && (t.classList.contains('if-pick') || t.closest('.if-picks') !== null));
			if (e.ctrlKey && e.key === 'Enter') {
				void saveForm();
				e.preventDefault();
				return;
			}
			if (t === pickInputEl || (t instanceof HTMLElement && t.classList.contains('if-pick'))) {
				if (e.key === 'ArrowDown') {
					pickIdx = Math.min(pickIdx + 1, Math.max(pickHits.length - 1, 0));
					e.preventDefault();
					return;
				}
				if (e.key === 'ArrowUp') {
					pickIdx = Math.max(pickIdx - 1, 0);
					e.preventDefault();
					return;
				}
				if (e.key === 'Enter' && pickHits[pickIdx]) {
					chooseEntity(pickHits[pickIdx]);
					e.preventDefault();
					return;
				}
			}
			if (t === locInputEl || (t instanceof HTMLElement && t.classList.contains('if-pick') && inPick)) {
				if (e.key === 'ArrowDown' && t === locInputEl) {
					locIdx = Math.min(locIdx + 1, Math.max(locEntities.length - 1, 0));
					e.preventDefault();
					return;
				}
				if (e.key === 'Enter' && t === locInputEl && locEntities[locIdx]) {
					chooseLocation(locEntities[locIdx]);
					e.preventDefault();
					return;
				}
			}
			return;
		}
		const k = e.key;
		if (k === 'Escape') {
			back();
			e.preventDefault();
			return;
		}
		if (k === 'ArrowDown') {
			move(1);
			e.preventDefault();
			return;
		}
		if (k === 'ArrowUp') {
			move(-1);
			e.preventDefault();
			return;
		}
		if (k === 'Enter') {
			const row = navRows.filter((n) => n.kind === 'row')[sel];
			if (row?.entry) {
				if (view === 'overview') openItem(row.entry.name);
				else openEdit(row.entry);
			}
			e.preventDefault();
			return;
		}
		if (k === 'Delete' || k === 'Backspace') {
			const row = navRows.filter((n) => n.kind === 'row')[sel];
			if (row?.entry && e.target === document.body) {
				void removeRow(row.entry.id);
				e.preventDefault();
			}
			return;
		}
		if (k === 'n' || k === 'N') {
			// Nur außerhalb von Eingabefeldern (Zeile → openEdit öffnet Inputs).
			const t = e.target;
			if (t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement) return;
			openAdd();
			e.preventDefault();
			return;
		}
	}

	function move(dir: number) {
		const rowsIdx = navRows.filter((n) => n.kind === 'row').length;
		if (!rowsIdx) return;
		sel = Math.min(Math.max(sel + dir, 0), rowsIdx - 1);
		document.querySelectorAll<HTMLElement>('.inv-row')[sel]?.scrollIntoView({ block: 'nearest' });
	}

	function setSort(s: SortMode) {
		sortMode = s;
		sel = 0;
	}
</script>

<svelte:window onkeydown={onKey} />

<div class="pane-inventory" role="region" aria-label={$t('nav.inventory')}>
	{#if inForm}
		<!-- Erfassen/Bearbeiten -->
		<div class="inv-form" role="form" aria-label={$t('inventory.record_position')}>
			<div class="if-head">
				<button type="button" class="inv-back" onclick={closeForm} aria-label={$t('common.cancel')}>←</button>
				<div class="if-title">{editingId === null ? $t('inventory.create_caps') : $t('inventory.edit_caps')}</div>
			</div>
			<div class="if-body">
				<section class="if-sect">
					<div class="h">{$t('inventory.material')}</div>
					<input
						class="if-input"
						type="text"
						placeholder={$t('inventory.ph_search_item')}
						bind:value={pickQuery}
						oninput={onPickInput}
						bind:this={pickInputEl}
					/>
					{#if pickBusy}
						<div class="if-hint">{$t('inventory.search_ph')}</div>
					{/if}
					{#if pickHits.length}
						<div class="if-picks" role="listbox" aria-label={$t('inventory.entity_matches')}>
							{#each pickHits.slice(0, 8) as h, pi}
								<button
									type="button"
									class="if-pick"
									class:is-on={fEntityId !== null && h.id === fEntityId || pi === pickIdx}
									onclick={() => chooseEntity(h)}
								>
									<span class="k">{h.kind === 'item' ? 'i' : 'c'}</span>
									{h.name}
									{#if h.subtitle}<small>{h.subtitle}</small>{/if}
								</button>
							{/each}
						</div>
					{/if}
					{#if pickQuery.trim()}
						<button type="button" class="if-pick free" onclick={chooseFreeText}>
							<span class="k">f</span>Als Freitext übernehmen: „{pickQuery.trim()}“
						</button>
					{/if}
					<div class="if-chosen">{$t('inventory.material_label')}<b>{fName || '—'}</b>
						{#if fEntityKind}
							<span class="src">({fEntityKind === 'item' ? $t('inventory.item_entity') : $t('inventory.commodity_entity')} #{fEntityId})</span>
						{:else if fName}
							<span class="src">{$t('inventory.free_text')}</span>
						{/if}
					</div>
				</section>
				<div class="if-grid">
					<section class="if-sect">
						<div class="h">{$t('common.quality')}<i>{$t('inventory.required_range')}</i></div>
						<input
							class="if-input"
							type="number"
							min="0"
							max="1000"
							step="1"
							placeholder={$t('inventory.ph_eg_quality')}
							bind:value={fQuality}
						bind:this={qualityEl}
						/>
					</section>
					<section class="if-sect">
						<div class="h">{$t('common.quantity')}<i>{$t('inventory.scu_units')}</i></div>
						<input class="if-input" type="number" min="0" step="1" placeholder={$t('inventory.ph_eg_qty')} bind:value={fQty} />
					</section>
				</div>
				<section class="if-sect">
					<div class="h">{$t('inventory.location')}</div>
					<input
						class="if-input"
						type="text"
						placeholder={$t('inventory.ph_eg_location')}
						bind:value={fLocation}
						oninput={onLocInput}
						list="inv-locs"
						bind:this={locInputEl}
					/>
					<datalist id="inv-locs">
						{#each locSuggest as loc}
							<option value={loc}></option>
						{/each}
					</datalist>
					{#if locEntities.length}
						<div class="if-picks" role="listbox" aria-label={$t('inventory.location_entities')}>
							{#each locEntities.slice(0, 6) as h, li}
								<button type="button" class="if-pick" class:is-on={(fLocationId !== null && h.id === fLocationId) || li === locIdx} onclick={() => chooseLocation(h)}>
									<span class="k">l</span>{h.name}{#if h.subtitle}<small>{h.subtitle}</small>{/if}
								</button>
							{/each}
						</div>
					{/if}
					<div class="if-hint">{$t('inventory.freetext_note')}</div>
				</section>
				<section class="if-sect">
					<div class="h">{$t('inventory.buy_price')}<i>{$t('inventory.optional_auec')}</i></div>
					<input class="if-input" type="number" min="0" step="1" placeholder={$t('inventory.ph_eg_price')} bind:value={fPrice} />
				</section>
				{#if formError}
					<div class="if-error" role="alert">{formError}</div>
				{/if}
				<div class="if-actions">
					<button type="button" class="btn" onclick={saveForm} disabled={!String(fName).trim() || !String(fQuality).trim() || !String(fLocation).trim()}>
						{editingId === null ? $t('inventory.record_btn') : $t('common.save')}
					</button>
					<button type="button" class="btn-ghost" onclick={closeForm}>{$t('common.cancel')}</button>
					{#if editingId !== null}
						<button
							type="button"
							class="btn-ghost danger"
							onclick={() => editingId !== null && (void removeRow(editingId), closeForm())}
						>{$t('common.delete')}</button>
					{/if}
				</div>
			</div>
		</div>
	{:else if view === 'item' && itemDetail}
		<!-- Item-Detail: alle Qualitäten/Standorte des Materials + Summen -->
		<div class="inv-detail">
			<div class="if-head">
				<button type="button" class="inv-back" onclick={back} aria-label={$t('inventory.back_to_overview')}>←</button>
				<div class="if-title">{itemDetail.toUpperCase()}</div>
				<div class="if-sums">
					<span><i>{$t('inventory.total')}</i> {fmtQty(itemTotal)}</span>
					<span><i>{$t('inventory.positions')}</i> {itemRows.length}</span>
					<span><i>Ø Qualität</i> {itemTotal ? itemAvgQ : '—'}</span>
				</div>
			</div>
			<div class="inv-list">
				{#each itemRows as r (r.id)}
					<div role="button" tabindex="0" onkeydown={keyActivate} class="inv-row" class:is-sel={sel === itemRows.indexOf(r)} onclick={() => (sel = itemRows.indexOf(r))} ondblclick={() => openEdit(r)}>
						<span class="q q-{qTier(r.quality)}">Q{r.quality}</span>
						<div class="main">
							<div class="loc">{r.location_name}{#if r.location_id !== null}<span class="src">{$t('inventory.dot_location')}</span>{/if}</div>
							<div class="sub">{r.entity_kind === null ? $t('inventory.free_text') : r.entity_kind === 'item' ? $t('shop.item') : $t('common.commodity')}{#if r.price} {$t('inventory.purchase_price', { price: fmtPrice(r.price) })}{/if}</div>
						</div>
						<div class="qty">{fmtQty(r.quantity)} <small>{r.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}</small></div>
						<button type="button" class="inv-edit" aria-label={$t('common.edit')} onclick={(ev) => (ev.stopPropagation(), openEdit(r))}>✎</button>
					</div>
				{:else}
					<div class="inv-empty">{$t('inventory.no_positions_material')}</div>
				{/each}
			</div>
		</div>
	{:else if view === 'location' && locationDetail}
		<!-- Standort-Detail: alle Positionen dieses Standorts, gruppiert per
			Item-Subgroup, mit Summen (Gesamt/Positionen/Materialien/Ø Qualität). -->
		<div class="inv-detail">
			<div class="if-head">
				<button type="button" class="inv-back" onclick={back} aria-label={$t('inventory.back_to_overview')}>←</button>
				<div class="if-title">{locationDetail.toUpperCase()}</div>
				<div class="if-sums">
					<span><i>{$t('inventory.total')}</i> {fmtQty(locTotal)}</span>
					<span><i>{$t('inventory.positions')}</i> {locRows.length}</span>
					<span><i>{$t('inventory.materials')}</i> {locMatCount}</span>
					<span><i>Ø Qualität</i> {locRows.length ? locAvgQ : '—'}</span>
				</div>
			</div>
			<div class="inv-list">
				{#each matGroups(locRows) as m}
					<div class="isub">
						<button
							type="button"
							class="isub-btn"
							onclick={() => openItem(m.name)}
							title={$t('inventory.open_material_detail')}
							aria-label="Material {m.name} öffnen"
						>{m.name}</button>
						<span class="n">{m.list.length} {$t('inventory.pos_short')} · {fmtQty(m.total)} {m.list[0]?.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}</span>
					</div>
					{#each m.list as r (r.id)}
						<div role="button" tabindex="0" onkeydown={keyActivate} class="inv-row" class:is-sel={sel === locRows.indexOf(r)} onclick={() => (sel = locRows.indexOf(r))} ondblclick={() => openEdit(r)}>
							<span class="q q-{qTier(r.quality)}">Q{r.quality}</span>
							<div class="main">
								<div class="loc">{r.name}{#if r.entity_kind !== null}<span class="src"> · {r.entity_kind === 'item' ? 'Item' : 'Commodity'}</span>{/if}</div>
								<div class="sub">{#if r.price}Kauf {fmtPrice(r.price)} aUEC{:else}kein Kaufpreis{/if}</div>
							</div>
							<div class="qty">{fmtQty(r.quantity)} <small>{r.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}</small></div>
							<button type="button" class="inv-edit" aria-label={$t('common.edit')} onclick={(ev) => (ev.stopPropagation(), openEdit(r))}>✎</button>
						</div>
					{:else}
						<div class="inv-empty">{$t('inventory.no_positions_location')}</div>
					{/each}
				{:else}
					<div class="inv-empty">{$t('inventory.no_positions_location')}</div>
				{/each}
			</div>
		</div>
	{:else}
		<div class="inv-bar">
			<button type="button" class="chip" class:is-on={sortMode === 'loc'} onclick={() => setSort('loc')}>{$t('inventory.location')}</button>
			<button type="button" class="chip" class:is-on={sortMode === 'qty'} onclick={() => setSort('qty')}>{$t('inventory.qty_down')}</button>
			<button type="button" class="chip" class:is-on={sortMode === 'quality'} onclick={() => setSort('quality')}>{$t('inventory.quality_down')}</button>
			<span class="sp"></span>
			<button type="button" class="chip view" class:is-on={cardView} onclick={toggleCardView} title={$t('inventory.toggle_cards')}>{$t('inventory.cards')}</button>
			<button type="button" class="chip add" onclick={openAdd}>{$t('inventory.plus_position')}<kbd>N</kbd></button>
		</div>
		<div class="inv-list">
			{#if !loaded}
				<div class="inv-empty">{$t('inventory.loading')}</div>
			{:else if !visRows.length}
				<div class="inv-empty">
					{filter.trim() ? $t('inventory.no_match_filter', { filter }) : ''}
					{@html filter.trim() ? '' : $t('inventory.empty_note')}
				</div>
			{:else if cardView}
				<!-- QX: Cards-View — grid of Material-cards, Stand-ort-
				     Subgroups inside the cards (card header: Ø Quality, P14-Colors). -->
				<div class="inv-cards">
					{#each cardGroups as g (g.mat)}
							<div class="inv-card">
								<div class="ic-head">
									<button
										type="button"
										class="vgroup"
										onclick={() => openItem(g.mat)}
										title={$t('inventory.open_material_detail')}
										aria-label="Open Material {g.mat}"
									>{g.mat} <span class="n">{g.entries.length} {$t('inventory.pos_short')} · {fmtQty(g.total)} {g.entries[0]?.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}</span></button>
									<span class="q q-{qTier(g.avgQ)}">Ø {g.avgQ}</span>
								</div>
								{#each g.locs as l}
									<div class="isub">
										<button
											type="button"
											class="isub-btn"
											onclick={() => openLocation(l.loc)}
											title={$t('inventory.open_location_detail')}
											aria-label="Open Stand-ort {l.loc}"
										>{l.loc}</button>
										<span class="n">{l.list.length} {$t('inventory.pos_short')} · {fmtQty(l.total)}</span>
									</div>
									{#each l.list as r (r.id)}
									<div role="button" tabindex="0" onkeydown={keyActivate} class="inv-row" class:is-sel={sel === navRows.filter((n) => n.kind === 'row').findIndex((n) => n.entry?.id === r.id)} onclick={() => (sel = navRows.filter((n) => n.kind === 'row').findIndex((n) => n.entry?.id === r.id))} ondblclick={() => openEdit(r)}>
										<span class="q q-{qTier(r.quality)}">Q{r.quality}</span>
										<div role="button" tabindex="0" onkeydown={keyActivate} class="main" onclick={() => openLocation(r.location_name)}>
											<div class="loc">{r.location_name}</div>
											<div class="sub">{r.entity_kind === null ? $t('inventory.free_text') : r.entity_kind === 'item' ? $t('shop.item') : $t('common.commodity')}{#if r.price} {$t('inventory.purchase_price', { price: fmtPrice(r.price) })}{/if}</div>
										</div>
										<div class="qty">{fmtQty(r.quantity)} <small>{r.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}</small></div>
										<button type="button" class="inv-edit" aria-label={$t('common.edit')} onclick={(ev) => (ev.stopPropagation(), openEdit(r))}>✎</button>
									</div>
								{/each}
							{/each}
						</div>
					{/each}
				</div>
			{:else if sortMode === 'loc'}
				{#each groups as g (g.loc)}
					<button
						type="button"
						class="vgroup"
						onclick={() => openLocation(g.loc)}
						title={$t('inventory.open_location_detail')}
						aria-label="Standort {g.loc} öffnen"
					>
						{g.loc} <span class="n">{g.entries.length} {$t('inventory.pos_short')} · {fmtQty(g.total)} {g.entries[0]?.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}</span>
					</button>
					{#each g.mats as m}
						<div class="isub">
							<button
								type="button"
								class="isub-btn"
								onclick={() => openItem(m.name)}
								title={$t('inventory.open_material_detail')}
								aria-label="Material {m.name} öffnen"
							>{m.name}</button>
							<span class="n">{m.list.length} · {fmtQty(m.total)}</span>
						</div>
						{#each m.list as r (r.id)}
							<div role="button" tabindex="0" onkeydown={keyActivate} class="inv-row" class:is-sel={sel === navRows.filter((n) => n.kind === 'row').findIndex((n) => n.entry?.id === r.id)} onclick={() => (sel = navRows.filter((n) => n.kind === 'row').findIndex((n) => n.entry?.id === r.id))} ondblclick={() => openEdit(r)}>
							<span class="q q-{qTier(r.quality)}">Q{r.quality}</span>
							<div role="button" tabindex="0" onkeydown={keyActivate} class="main" onclick={() => openItem(r.name)}>
								<div class="loc">{r.name}</div>
								<div class="sub">{r.entity_kind === null ? $t('inventory.free_text') : r.entity_kind === 'item' ? $t('shop.item') : $t('common.commodity')}{#if r.price} {$t('inventory.purchase_price', { price: fmtPrice(r.price) })}{/if}</div>
							</div>
							<div class="qty">{fmtQty(r.quantity)} <small>{r.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}</small></div>
							<button type="button" class="inv-edit" aria-label={$t('common.edit')} onclick={(ev) => (ev.stopPropagation(), openEdit(r))}>✎</button>
							</div>
							{/each}
							{/each}
							{/each}
							{:else}
				{#each sortedRows as r, i (r.id)}
					<div role="button" tabindex="0" onkeydown={keyActivate} class="inv-row" class:is-sel={sel === i} onclick={() => (sel = i)} ondblclick={() => openEdit(r)}>
						<span class="q q-{qTier(r.quality)}">Q{r.quality}</span>
						<div role="button" tabindex="0" onkeydown={keyActivate} class="main" onclick={() => openItem(r.name)}>
							<div class="loc">{r.name} <small>{r.location_name}</small></div>
							<div class="sub">{r.entity_kind === null ? $t('inventory.free_text') : r.entity_kind === 'item' ? $t('shop.item') : $t('common.commodity')}{#if r.price} {$t('inventory.purchase_price', { price: fmtPrice(r.price) })}{/if}</div>
						</div>
						<div class="qty">{fmtQty(r.quantity)} <small>{r.entity_kind === 'item' ? $t('inventory.units_pc') : 'SCU'}</small></div>
						<button type="button" class="inv-edit" aria-label={$t('common.edit')} onclick={(ev) => (ev.stopPropagation(), openEdit(r))}>✎</button>
					</div>
				{/each}
			{/if}
		</div>
		<div class="inv-foot">
			<span>{visRows.length} Positionen · {materialCount} Materialien</span>
			<span><kbd>↑↓</kbd>{$t('inventory.row')}<kbd>↵</kbd> {view === 'overview' ? 'Material' : 'Bearbeiten'} · <kbd>Entf</kbd>{$t('inventory.delete_row')}<kbd>Esc</kbd>{$t('search.back')}</span>
		</div>
	{/if}
	{#if toast}
		<div class="inv-toast" role="status">{toast}</div>
	{/if}
</div>

<style>
	.pane-inventory {
		position: relative;
		display: flex;
		flex-direction: column;
		min-height: 0;
		flex: 1;
	}
	.inv-bar,
	.inv-foot,
	.if-head {
		display: flex;
		gap: 8px;
		align-items: center;
		padding: 8px 12px;
		border-bottom: 1px solid color-mix(in srgb, var(--line) 60%, transparent);
	}
	.inv-bar {
		border-top: 1px solid color-mix(in srgb, var(--line) 60%, transparent);
	}
	.inv-foot {
		justify-content: space-between;
		font-size: 10px;
		color: var(--fgdim);
		border-top: 1px solid color-mix(in srgb, var(--line) 60%, transparent);
		border-bottom: none;
	}
	.sp,
	.m-spacer {
		flex: 1;
	}
	.chip {
		border: none;
		background: transparent;
		color: var(--fgdim);
		font: inherit;
		font-size: 10.5px;
		letter-spacing: 0.05em;
		text-transform: uppercase;
		padding: 4px 9px;
		border-radius: 3px;
		cursor: pointer;
	}
	.chip:hover {
		color: var(--fg);
	}
	.chip.is-on {
		background: color-mix(in srgb, var(--accent) 18%, transparent);
		color: var(--accent);
	}
	.chip.add {
		color: var(--accent);
		margin-right: 6px;
	}
	.chip.add kbd {
		margin-left: 2px;
	}
	.inv-list {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding: 4px 8px 8px;
	}
	/* QX: Cards-View — Material-card-grid (Overlay-Style). */
	.inv-cards {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
		gap: 10px;
		align-content: start;
	}
	.inv-card {
		display: flex;
		flex-direction: column;
		border: 1px solid color-mix(in srgb, var(--line) 60%, transparent);
		border-radius: 4px;
		padding: 2px 10px 8px;
	}
	.inv-card .ic-head {
		display: flex;
		align-items: baseline;
		gap: 10px;
		margin-bottom: 2px;
	}
	.vgroup {
		/* Standort-Gruppenkopf = klickbarer Button (öffnet das Standort-Detail). */
		/* Button-Reset + bisherige Optik unverändert. */
		border: none;
		background: transparent;
		font: inherit;
		cursor: pointer;
		display: block;
		width: 100%;
		text-align: left;
		font-size: 10px;
		letter-spacing: 0.12em;
		text-transform: uppercase;
		color: var(--fgdim);
		padding: 10px 6px 4px;
		border-bottom: 1px solid color-mix(in srgb, var(--line) 40%, transparent);
	}
	.vgroup:hover,
	.vgroup:focus-visible {
		color: var(--accent);
	}
	.vgroup .n {
		color: var(--fgdim);
		margin-left: 6px;
	}
	/* Zweite Gruppierungsebene: Item-Subgroup-Header innerhalb einer */
	/* Standortgruppe (bzw. im Standort-Detail). Klein + gedimmt, klickbar */
	/* ins Material-Detail. */
	.isub {
		display: flex;
		align-items: baseline;
		gap: 6px;
		padding: 5px 6px 3px 16px;
		border-bottom: 1px solid color-mix(in srgb, var(--line) 25%, transparent);
	}
	.isub-btn {
		border: none;
		background: transparent;
		font: inherit;
		cursor: pointer;
		padding: 0;
		color: var(--fg);
		font-size: 11.5px;
		letter-spacing: 0.04em;
	}
	.isub-btn:hover,
	.isub-btn:focus-visible {
		color: var(--accent);
	}
	.isub .n {
		color: var(--fgdim);
		font-size: 10px;
	}
	.inv-row {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 7px 6px;
		border-bottom: 1px solid color-mix(in srgb, var(--line) 25%, transparent);
		cursor: pointer;
	}
	.inv-row.is-sel {
		background: color-mix(in srgb, var(--accent) 10%, transparent);
		box-shadow: inset 2px 0 0 var(--accent);
	}
	.inv-row .q,
	.inv-card .q {
		font: 700 11px/1 var(--mono, monospace);
		padding: 3px 6px;
		border-radius: 3px;
		min-width: 34px;
		text-align: center;
		background: color-mix(in srgb, var(--fgdim) 15%, transparent);
		color: var(--fg);
	}
	.inv-row .q-bad,
	.inv-card .q-bad {
		color: var(--alert, #ff6b6b);
		background: color-mix(in srgb, var(--alert, #ff6b6b) 16%, transparent);
	}
	/* q-ok = the Default neutral chip look (see .inv-row .q). */
	.inv-row .q-good,
	.inv-card .q-good {
		color: var(--ok);
		background: color-mix(in srgb, var(--ok) 16%, transparent);
	}
	.inv-row .q-top,
	.inv-card .q-top {
		color: color-mix(in srgb, var(--ok) 55%, white);
		background: color-mix(in srgb, var(--ok) 28%, transparent);
	}
	.inv-row .main {
		flex: 1;
		min-width: 0;
	}
	.inv-row .loc {
		font-size: 12.5px;
		color: var(--fg);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.inv-row .loc small {
		color: var(--fgdim);
		margin-left: 6px;
	}
	.inv-row .sub {
		font-size: 10px;
		color: var(--fgdim);
	}
	.inv-row .qty {
		font: 600 12px/1 var(--mono, monospace);
		color: var(--fg);
		text-align: right;
		white-space: nowrap;
	}
	.inv-row .qty small {
		color: var(--fgdim);
		font-weight: 400;
	}
	.inv-row .src {
		color: var(--fgdim);
		font-size: 9px;
		letter-spacing: 0.06em;
		text-transform: uppercase;
	}
	.inv-edit {
		border: none;
		background: transparent;
		color: var(--fgdim);
		cursor: pointer;
		font-size: 12px;
		padding: 2px 6px;
		border-radius: 3px;
	}
	.inv-edit:hover {
		color: var(--accent);
		background: color-mix(in srgb, var(--accent) 12%, transparent);
	}
	.inv-empty {
		padding: 28px 16px;
		color: var(--fgdim);
		text-align: center;
		font-size: 12px;
	}
	/* Formular */
	.inv-form,
	.inv-detail {
		display: flex;
		flex-direction: column;
		flex: 1;
		min-height: 0;
	}
	.inv-back {
		border: none;
		background: transparent;
		color: var(--fg);
		font-size: 14px;
		cursor: pointer;
		padding: 2px 8px;
		border-radius: 3px;
	}
	.inv-back:hover {
		color: var(--accent);
	}
	.if-title {
		font-size: 11px;
		letter-spacing: 0.14em;
		color: var(--accent);
	}
	.if-sums {
		margin-left: auto;
		display: flex;
		gap: 12px;
		font-size: 11px;
		color: var(--fg);
		font-family: var(--mono, monospace);
	}
	.if-sums i {
		font-style: normal;
		color: var(--fgdim);
		margin-right: 4px;
	}
	.if-body {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding: 10px 14px 14px;
		display: flex;
		flex-direction: column;
		gap: 10px;
	}
	.if-sect .h {
		font-size: 10px;
		letter-spacing: 0.12em;
		text-transform: uppercase;
		color: var(--fgdim);
		margin-bottom: 4px;
	}
	.if-sect .h i {
		font-style: normal;
		text-transform: none;
		letter-spacing: 0;
		margin-left: 6px;
		color: var(--fgdim);
		opacity: 0.8;
	}
	.if-input {
		width: 100%;
		background: color-mix(in srgb, var(--fg) 6%, transparent);
		border: 1px solid color-mix(in srgb, var(--line) 70%, transparent);
		border-radius: 4px;
		color: var(--fg);
		font: inherit;
		font-size: 12.5px;
		padding: 6px 8px;
		box-sizing: border-box;
	}
	.if-input:focus {
		outline: none;
		border-color: var(--accent);
	}
	.if-grid {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 10px;
	}
	.if-picks {
		display: flex;
		flex-direction: column;
		gap: 2px;
		margin-top: 6px;
		max-height: 160px;
		overflow-y: auto;
	}
	.if-pick {
		display: flex;
		align-items: center;
		gap: 8px;
		text-align: left;
		border: none;
		background: transparent;
		color: var(--fg);
		font: inherit;
		font-size: 12px;
		padding: 5px 6px;
		border-radius: 3px;
		cursor: pointer;
	}
	.if-pick:hover {
		background: color-mix(in srgb, var(--accent) 10%, transparent);
	}
	.if-pick.is-on {
		background: color-mix(in srgb, var(--accent) 16%, transparent);
		color: var(--accent);
	}
	.if-pick .k {
		font-size: 9px;
		color: var(--fgdim);
		min-width: 12px;
	}
	.if-pick small {
		color: var(--fgdim);
		margin-left: 4px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.if-pick.free {
		color: var(--accent);
	}
	.if-chosen {
		font-size: 11px;
		color: var(--fgdim);
		margin-top: 6px;
	}
	.if-chosen b {
		color: var(--fg);
	}
	.if-chosen .src {
		color: var(--fgdim);
		font-size: 9px;
		text-transform: uppercase;
		letter-spacing: 0.06em;
	}
	.if-hint {
		font-size: 10px;
		color: var(--fgdim);
		margin-top: 4px;
	}
	.if-error {
		color: var(--alert, #ff6b6b);
		font-size: 11.5px;
	}
	.if-actions {
		display: flex;
		gap: 8px;
		margin-top: 4px;
	}
	.btn {
		border: none;
		background: color-mix(in srgb, var(--accent) 20%, transparent);
		color: var(--accent);
		font: inherit;
		font-size: 11px;
		letter-spacing: 0.08em;
		text-transform: uppercase;
		padding: 7px 14px;
		border-radius: 3px;
		cursor: pointer;
	}
	.btn:disabled {
		opacity: 0.45;
		cursor: default;
	}
	.btn-ghost {
		border: none;
		background: transparent;
		color: var(--fgdim);
		font: inherit;
		font-size: 11px;
		padding: 7px 10px;
		border-radius: 3px;
		cursor: pointer;
	}
	.btn-ghost:hover {
		color: var(--fg);
	}
	.btn-ghost.danger:hover {
		color: var(--alert, #ff6b6b);
	}
	.inv-toast {
		position: absolute;
		left: 50%;
		bottom: 46px;
		transform: translateX(-50%);
		background: color-mix(in srgb, var(--accent) 22%, var(--bg, #101010));
		color: var(--fg);
		border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
		font-size: 11px;
		padding: 6px 12px;
		border-radius: 4px;
		white-space: nowrap;
	}
	kbd {
		font-family: var(--mono, monospace);
	}
</style>
