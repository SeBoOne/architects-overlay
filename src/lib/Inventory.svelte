<script lang="ts">
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
		id: number;
		entity_kind: string | null;
		entity_id: number | null;
		name: string;
		quality: number;
		location_id: number | null;
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
	let view = $state<'overview' | 'item'>('overview');
	let itemDetail = $state<string | null>(null); // Materialname des Item-Detail
	let sel = $state(0);
	let toast = $state('');
	let toastTimer: ReturnType<typeof setTimeout> | null = null;

	// Formular (Hinzufügen/Bearbeiten). `editingId = null` → Hinzufügen.
	let inForm = $state(false);
	let editingId = $state<number | null>(null);
	let fEntityKind = $state<'commodity' | 'item' | null>(null);
	let fEntityId = $state<number | null>(null);
	let fName = $state('');
	let fQuality = $state('');
	let fLocationId = $state<number | null>(null);
	let fLocation = $state('');
	let fQty = $state('');
	let fPrice = $state('');
	let formError = $state('');

	// Entity-Picker: Suche im Formular (bestehende entities) + Freitext-Fall.
	let pickQuery = $state('');
	let pickHits = $state<SearchHit[]>([]);
	let pickBusy = $state(false);
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
	// Ebene = Positionen (Qualität absteigend). Übrige Sortierungen = flach.
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
			total: entries.reduce((s, e) => s + e.quantity, 0)
		}));
	});

	// Flaches Navigationsmodell je Ansicht.
	type NavRow = { entry: InvEntry | null; kind: 'header' | 'row' };
	let navRows = $derived.by((): NavRow[] => {
		if (view === 'item' && itemDetail) {
			const mine = sortedRows.filter((r) => r.name === itemDetail);
			return mine.map((entry) => ({ entry, kind: 'row' as const }));
		}
		if (sortMode === 'loc') {
			const out: NavRow[] = [];
			for (const g of groups) {
				out.push({ entry: null, kind: 'header' });
				for (const e of g.entries) out.push({ entry: e, kind: 'row' });
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

	// Distinct-Materialien (für die Übersichtszahl).
	let materialCount = $derived(new Set(visRows.map((r) => r.name)).size);

	// ---- Aktionen ------------------------------------------------------------
	function openItem(name: string) {
		itemDetail = name;
		view = 'item';
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
			return;
		}
		onexit();
	}

	async function removeRow(id: number) {
		try {
			await invoke('inventory_delete', { id });
			rows = rows.filter((r) => r.id !== id);
			toastMsg('Position gelöscht.');
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
		inForm = true;
		void loadLocSuggestions();
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
	}

	function closeForm() {
		inForm = false;
		editingId = null;
		formError = '';
		pickHits = [];
		locEntities = [];
	}

	async function loadLocSuggestions() {
		try {
			locSuggest = await invoke<string[]>('inventory_locations');
		} catch {
			locSuggest = [];
		}
	}

	function onPickInput() {
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
		fEntityId = Number(h.id);
		fName = h.name;
	}

	function chooseFreeText() {
		fEntityKind = null;
		fEntityId = null;
		fName = pickQuery.trim();
	}

	function onLocInput() {
		fLocationId = null; // Freitext, bis ein Entity-Vorschlag gewählt wird
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
		fLocationId = Number(h.id);
		locEntities = [];
	}

	async function saveForm() {
		const quality = parseInt(String(fQuality), 10);
		const qty = parseFloat(String(fQty).replace(',', '.'));
		if (!fName.trim()) {
			formError = 'Bitte ein Material wählen (Entity-Suche) oder als Freitext übernehmen.';
			return;
		}
		if (!Number.isFinite(quality) || quality < 0 || quality > 1000) {
			formError = 'Qualität ist Pflicht (0–1000).';
			return;
		}
		if (!fLocation.trim()) {
			formError = 'Bitte einen Standort angeben.';
			return;
		}
		if (!Number.isFinite(qty) || qty < 0) {
			formError = 'Bitte eine gültige Menge angeben.';
			return;
		}
		const priceRaw = String(fPrice).trim().replace(',', '.');
		const price = priceRaw ? parseFloat(priceRaw) : null;
		if (priceRaw && !Number.isFinite(price)) {
			formError = 'Kaufpreis muss eine Zahl sein (aUEC).';
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
				toastMsg('Position geändert.');
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

<div class="pane-inventory" role="region" aria-label="Inventar">
	{#if inForm}
		<!-- Erfassen/Bearbeiten -->
		<div class="inv-form" role="form" aria-label="Position erfassen">
			<div class="if-head">
				<button type="button" class="inv-back" onclick={closeForm} aria-label="Abbrechen">←</button>
				<div class="if-title">{editingId === null ? 'POSITION ERFASSEN' : 'POSITION BEARBEITEN'}</div>
			</div>
			<div class="if-body">
				<section class="if-sect">
					<div class="h">Material</div>
					<input
						class="if-input"
						type="text"
						placeholder="Item / Commodity suchen … oder Freitext"
						bind:value={pickQuery}
						oninput={onPickInput}
					/>
					{#if pickBusy}
						<div class="if-hint">Suche …</div>
					{/if}
					{#if pickHits.length}
						<div class="if-picks" role="listbox" aria-label="Entity-Treffer">
							{#each pickHits.slice(0, 8) as h}
								<button
									type="button"
									class="if-pick"
									class:is-on={fEntityId !== null && Number(h.id) === fEntityId}
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
					<div class="if-chosen">
						Material: <b>{fName || '—'}</b>
						{#if fEntityKind}
							<span class="src">({fEntityKind === 'item' ? 'Item-Entity' : 'Commodity-Entity'} #{fEntityId})</span>
						{:else if fName}
							<span class="src">(Freitext)</span>
						{/if}
					</div>
				</section>
				<div class="if-grid">
					<section class="if-sect">
						<div class="h">Qualität <i>Pflicht · 0–1000</i></div>
						<input
							class="if-input"
							type="number"
							min="0"
							max="1000"
							step="1"
							placeholder="z. B. 900"
							bind:value={fQuality}
						/>
					</section>
					<section class="if-sect">
						<div class="h">Menge <i>SCU bzw. Stück</i></div>
						<input class="if-input" type="number" min="0" step="1" placeholder="z. B. 12" bind:value={fQty} />
					</section>
				</div>
				<section class="if-sect">
					<div class="h">Standort</div>
					<input
						class="if-input"
						type="text"
						placeholder="z. B. Heim-Hangar, meine Carrack …"
						bind:value={fLocation}
						oninput={onLocInput}
						list="inv-locs"
					/>
					<datalist id="inv-locs">
						{#each locSuggest as loc}
							<option value={loc}></option>
						{/each}
					</datalist>
					{#if locEntities.length}
						<div class="if-picks" role="listbox" aria-label="Standort-Entities">
							{#each locEntities.slice(0, 6) as h}
								<button type="button" class="if-pick" class:is-on={fLocationId !== null && Number(h.id) === fLocationId} onclick={() => chooseLocation(h)}>
									<span class="k">l</span>{h.name}{#if h.subtitle}<small>{h.subtitle}</small>{/if}
								</button>
							{/each}
						</div>
					{/if}
					<div class="if-hint">Freitext = eigenes Depot · Treffer wählen = Location-Entity</div>
				</section>
				<section class="if-sect">
					<div class="h">Kaufpreis <i>optional · aUEC</i></div>
					<input class="if-input" type="number" min="0" step="1" placeholder="z. B. 4500" bind:value={fPrice} />
				</section>
				{#if formError}
					<div class="if-error" role="alert">{formError}</div>
				{/if}
				<div class="if-actions">
					<button type="button" class="btn" onclick={saveForm} disabled={!String(fName).trim() || !String(fQuality).trim() || !String(fLocation).trim()}>
						{editingId === null ? 'Erfassen' : 'Speichern'}
					</button>
					<button type="button" class="btn-ghost" onclick={closeForm}>Abbrechen</button>
					{#if editingId !== null}
						<button
							type="button"
							class="btn-ghost danger"
							onclick={() => editingId !== null && (void removeRow(editingId), closeForm())}
						>Löschen</button>
					{/if}
				</div>
			</div>
		</div>
	{:else if view === 'item' && itemDetail}
		<!-- Item-Detail: alle Qualitäten/Standorte des Materials + Summen -->
		<div class="inv-detail">
			<div class="if-head">
				<button type="button" class="inv-back" onclick={back} aria-label="zurück zur Übersicht">←</button>
				<div class="if-title">{itemDetail.toUpperCase()}</div>
				<div class="if-sums">
					<span><i>Gesamt</i> {fmtQty(itemTotal)}</span>
					<span><i>Positionen</i> {itemRows.length}</span>
					<span><i>Ø Qualität</i> {itemTotal ? itemAvgQ : '—'}</span>
				</div>
			</div>
			<div class="inv-list">
				{#each itemRows as r (r.id)}
					<div class="inv-row" class:is-sel={sel === itemRows.indexOf(r)} onclick={() => (sel = itemRows.indexOf(r))} ondblclick={() => openEdit(r)}>
						<span class="q q-{Math.floor(r.quality / 250) * 250}">Q{r.quality}</span>
						<div class="main">
							<div class="loc">{r.location_name}{#if r.location_id !== null}<span class="src"> · Location</span>{/if}</div>
							<div class="sub">{r.entity_kind === null ? 'Freitext' : r.entity_kind === 'item' ? 'Item' : 'Commodity'}{#if r.price} · Kauf {fmtPrice(r.price)} aUEC{/if}</div>
						</div>
						<div class="qty">{fmtQty(r.quantity)} <small>{r.entity_kind === 'item' ? 'Stk' : 'SCU'}</small></div>
						<button type="button" class="inv-edit" aria-label="Bearbeiten" onclick={(ev) => (ev.stopPropagation(), openEdit(r))}>✎</button>
					</div>
				{:else}
					<div class="inv-empty">Keine Positionen für dieses Material.</div>
				{/each}
			</div>
		</div>
	{:else}
		<!-- Gesamtübersicht: nach Standort gruppiert (Standort-Sortierung) -->
		<div class="inv-bar">
			<button type="button" class="chip" class:is-on={sortMode === 'loc'} onclick={() => setSort('loc')}>Standort</button>
			<button type="button" class="chip" class:is-on={sortMode === 'qty'} onclick={() => setSort('qty')}>Menge ↓</button>
			<button type="button" class="chip" class:is-on={sortMode === 'quality'} onclick={() => setSort('quality')}>Qualität ↓</button>
			<span class="sp"></span>
			<button type="button" class="chip add" onclick={openAdd}>+ Position <kbd>N</kbd></button>
		</div>
		<div class="inv-list">
			{#if !loaded}
				<div class="inv-empty">Lade Inventar …</div>
			{:else if !visRows.length}
				<div class="inv-empty">
					{filter.trim() ? `Keine Positionen passen zu „${filter}“.` : 'Inventar leer — erfasse die erste Position mit <b>N</b>.'}
				</div>
			{:else if sortMode === 'loc'}
				{#each groups as g (g.loc)}
					<div class="vgroup">{g.loc} <span class="n">{g.entries.length} Pos · {fmtQty(g.total)} {g.entries[0]?.entity_kind === 'item' ? 'Stk' : 'SCU'}</span></div>
					{#each g.entries as r (r.id)}
						<div class="inv-row" class:is-sel={sel === navRows.filter((n) => n.kind === 'row').findIndex((n) => n.entry?.id === r.id)} onclick={() => (sel = navRows.filter((n) => n.kind === 'row').findIndex((n) => n.entry?.id === r.id))} ondblclick={() => openEdit(r)}>
							<span class="q q-{Math.floor(r.quality / 250) * 250}">Q{r.quality}</span>
							<div class="main" onclick={() => openItem(r.name)}>
								<div class="loc">{r.name}</div>
								<div class="sub">{r.entity_kind === null ? 'Freitext' : r.entity_kind === 'item' ? 'Item' : 'Commodity'}{#if r.price} · Kauf {fmtPrice(r.price)} aUEC{/if}</div>
							</div>
							<div class="qty">{fmtQty(r.quantity)} <small>{r.entity_kind === 'item' ? 'Stk' : 'SCU'}</small></div>
							<button type="button" class="inv-edit" aria-label="Bearbeiten" onclick={(ev) => (ev.stopPropagation(), openEdit(r))}>✎</button>
						</div>
					{/each}
				{/each}
			{:else}
				{#each sortedRows as r, i (r.id)}
					<div class="inv-row" class:is-sel={sel === i} onclick={() => (sel = i)} ondblclick={() => openEdit(r)}>
						<span class="q q-{Math.floor(r.quality / 250) * 250}">Q{r.quality}</span>
						<div class="main" onclick={() => openItem(r.name)}>
							<div class="loc">{r.name} <small>{r.location_name}</small></div>
							<div class="sub">{r.entity_kind === null ? 'Freitext' : r.entity_kind === 'item' ? 'Item' : 'Commodity'}{#if r.price} · Kauf {fmtPrice(r.price)} aUEC{/if}</div>
						</div>
						<div class="qty">{fmtQty(r.quantity)} <small>{r.entity_kind === 'item' ? 'Stk' : 'SCU'}</small></div>
						<button type="button" class="inv-edit" aria-label="Bearbeiten" onclick={(ev) => (ev.stopPropagation(), openEdit(r))}>✎</button>
					</div>
				{/each}
			{/if}
		</div>
		<div class="inv-foot">
			<span>{visRows.length} Positionen · {materialCount} Materialien</span>
			<span><kbd>↑↓</kbd> Zeile · <kbd>↵</kbd> {view === 'overview' ? 'Material' : 'Bearbeiten'} · <kbd>Entf</kbd> Löschen · <kbd>Esc</kbd> zurück</span>
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
	.vgroup {
		font-size: 10px;
		letter-spacing: 0.12em;
		text-transform: uppercase;
		color: var(--fgdim);
		padding: 10px 6px 4px;
		border-bottom: 1px solid color-mix(in srgb, var(--line) 40%, transparent);
	}
	.vgroup .n {
		color: var(--fgdim);
		margin-left: 6px;
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
	.inv-row .q {
		font: 700 11px/1 var(--mono, monospace);
		padding: 3px 6px;
		border-radius: 3px;
		min-width: 34px;
		text-align: center;
		background: color-mix(in srgb, var(--fgdim) 15%, transparent);
		color: var(--fg);
	}
	.inv-row .q-750,
	.inv-row .q-1000 {
		color: var(--accent);
		background: color-mix(in srgb, var(--accent) 14%, transparent);
	}
	.inv-row .q-0 {
		color: var(--fgdim);
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
