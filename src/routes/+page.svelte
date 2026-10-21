<script lang="ts">
	// SC Architects Overlay — Suchpalette (Phase 1).
	//
	// Beide Tauri-Fenster rendern dieselbe Palette ("main" = Entwicklungs-Shell,
	// "spotlight" = Overlay über Star Citizen). Der Spotlight ist click-through-
	// fähig und zeigt den Spiel-/Fokus-Status; Esc blendet ihn aus.
	//
	// Zustände (Design t_06285098 · Variante C COCKPIT / HUD Amber):
	//   idle · typing · detail · loading · error · offline · market
	// Suche: 120 ms Debounce gegen den lokalen SQLite-Cache (`search`-IPC),
	// gruppiert Commodities → Items → Locations, Tastatur ↑↓ ↵ Esc (M = Market).
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';

	// ---- IPC-Typen (Mirror der Serde-DTOs in src-tauri/src/trade.rs) ---------
	type OverlayState = { visible: boolean; game_running: boolean; click_through: boolean };
	type SearchHit = { id: number; name: string; kind: string; subtitle: string; code: string | null };
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
	type PriceDetail = { commodity: string; source: string; fetched_at: number; prices: PricePoint[] };
	type DatasetStatus = { kind: string; label: string; fresh: boolean; age_secs: number | null; count: number };
	type SyncStatus = {
		provider: string;
		syncing: boolean;
		online: boolean;
		last_error: string | null;
		datasets: DatasetStatus[];
	};

	const OVERLAY_STATE_EVENT = 'overlay-state-changed';
	const SYNC_EVENT = 'trade-sync';

	type Phase = 'idle' | 'typing' | 'detail' | 'loading' | 'error' | 'offline' | 'market';

	// ---- Zustand -------------------------------------------------------------
	let isSpotlight = $state(false);
	let overlay = $state<OverlayState | null>(null);
	let phase = $state<Phase>('loading');
	let input = $state('');
	let filter = $state('all'); // all | commodity | item | location
	let groups = $state<GroupHits[]>([]);
	let sel = $state(0);
	let detail = $state<PriceDetail | null>(null);
	let detailBusy = $state(false);
	let status = $state<SyncStatus | null>(null);
	let errorMsg = $state<string | null>(null);
	let bootTrace = $state('start');

	// Flaches Navigationsmodell: alle Zeilen in Reihenfolge + Offsets je Gruppe.
	let flat: SearchHit[] = [];
	let groupOffsets: number[] = [];
	let debounce: ReturnType<typeof setTimeout> | null = null;

	function hasData(s?: SyncStatus | null): boolean {
		return (s ?? status)?.datasets.some((d) => d.count > 0) ?? false;
	}

	// ---- Initialisierung -----------------------------------------------------
	function setWinTitle(t: string) {
		if (typeof document !== 'undefined') document.title = t;
	}

	async function boot() {
		// A/B-Test GANZ VORN (diagnose 2026-10-21): reiner bool-ping vs.
		// bool-command mit AppHandle vs. Objekt-command vs. sync-command.
		try {
			const p = await invoke<boolean>('ping');
			bootTrace = 'ping-ok:' + p;
			setWinTitle('boot:ping-ok:' + p);
		} catch (e) {
			bootTrace = 'ping-err:' + String(e);
			setWinTitle('boot:ping-err');
		}
		try {
			const b = await invoke<boolean>('overlay_set_click_through', { ignore: false });
			bootTrace += '|ct-ok:' + b;
			setWinTitle('boot:ct-ok:' + b);
		} catch (e) {
			bootTrace += '|ct-err:' + String(e);
			setWinTitle('boot:ct-err');
		}
		try {
			const o = await invoke<Record<string, unknown>>('overlay_get_state');
			bootTrace += '|obj-ok';
			setWinTitle('boot:obj-ok');
		} catch (e) {
			bootTrace += '|obj-err:' + String(e);
			setWinTitle('boot:obj-err');
		}

		try {
			const label = getCurrentWindow().label;
			document.documentElement.dataset.window = label;
			if (label === 'spotlight') {
				isSpotlight = true;
				overlay = await invoke<OverlayState>('overlay_get_state');
				await listen<OverlayState>(OVERLAY_STATE_EVENT, (e) => {
					overlay = e.payload;
				});
			}
		} catch {
			// Ohne Tauri-Webview (plain vite dev) → unkritisch, Palette läuft.
		}

		try {
			status = await invoke<SyncStatus>('sync_status');
			bootTrace += '|sync-ok';
			setWinTitle('boot:sync-ok');
		} catch (e) {
			bootTrace += '|sync-err:' + String(e);
			setWinTitle('boot:sync-err');
		}

		try {
			await listen<SyncStatus>(SYNC_EVENT, (e) => onSyncEvent(e.payload));
			bootTrace += '/listen-ok';
			if (!hasData()) {
				// Erster Start: lokalen Cache füllen (einmaliger Sync-Spinner).
				phase = status.online ? 'loading' : 'error';
				await invoke('sync_now');
				bootTrace += '/invoke-ok';
			} else {
				phase = status.online ? 'idle' : 'offline';
			}
		} catch (e) {
			status = { provider: 'uex', syncing: false, online: false, last_error: String(e), datasets: [] };
			phase = 'error';
			bootTrace += '/listen-err:' + String(e);
		}
	}
	void boot();

	function onSyncEvent(s: SyncStatus) {
		status = s;
		if (phase === 'typing' || phase === 'detail') return; // laufende Aktion nicht stören
		if (!hasData(s)) {
			phase = s.last_error ? 'error' : s.syncing ? 'loading' : phase;
		} else {
			phase = s.online ? (input ? 'typing' : 'idle') : 'offline';
		}
	}

	// ---- Suche (debounced, cache-only, < 50 ms) ------------------------------
	function onInput() {
		if (debounce) clearTimeout(debounce);
		debounce = setTimeout(() => runSearch(), 120);
	}

	async function runSearch() {
		const q = input.trim();
		if (!q) {
			groups = [];
			flat = [];
			groupOffsets = [];
			sel = 0;
			phase = 'idle';
			return;
		}
		try {
			const resp = await invoke<SearchResponse>('search', {
				query: q,
				filter: filter === 'all' ? null : filter
			});
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
	}

	function cycleFilter() {
		const order = ['all', 'commodity', 'item', 'location'];
		const i = order.indexOf(filter);
		filter = order[(i + 1) % order.length];
		if (input.trim()) void runSearch();
	}

	async function openSelected() {
		if (!flat.length) return;
		const row = flat[sel];
		errorMsg = null;
		detail = null;
		detailBusy = true;
		phase = 'detail';
		try {
			const d = await invoke<PriceDetail>('get_prices', { commodity: row.name });
			detail = d;
		} catch (e) {
			errorMsg = String(e);
			phase = 'error';
		}
		detailBusy = false;
	}

	function esc() {
		if (phase === 'detail' || phase === 'market') {
			phase = status?.online === false && status ? 'offline' : 'typing';
		} else if (isSpotlight) {
			void invoke<boolean>('overlay_set_visible', { visible: false });
		} else {
			input = '';
			groups = [];
			flat = [];
			groupOffsets = [];
			phase = 'idle';
		}
	}

	function toggleMarket() {
		phase = phase === 'market' ? 'typing' : 'market';
	}

	async function toggleClickThrough() {
		if (!overlay) return;
		const ignore = !overlay.click_through;
		await invoke<boolean>('overlay_set_click_through', { ignore });
		overlay = await invoke<OverlayState>('overlay_get_state');
	}

	async function retrySync() {
		errorMsg = null;
		phase = 'loading';
		await invoke('sync_now');
	}

	function onKey(e: KeyboardEvent) {
		const k = e.key;
		if (k === 'ArrowDown') {
			move(1);
			e.preventDefault();
		} else if (k === 'ArrowUp') {
			move(-1);
			e.preventDefault();
		} else if (k === 'Enter') {
			void openSelected();
			e.preventDefault();
		} else if (k === 'Escape') {
			esc();
			e.preventDefault();
		} else if (k === 'm' || k === 'M') {
			toggleMarket();
			e.preventDefault();
		} else if (k === 'Tab') {
			cycleFilter();
			e.preventDefault();
		} else if ((k === 'o' || k === 'O') && phase === 'error') {
			phase = 'offline';
		}
	}

	// ---- Formatierung (Deutsch, Mono — Spalten bleiben stehen) ---------------
	function fmt(n: number, d = 2): string {
		return n.toLocaleString('de-DE', { maximumFractionDigits: d, minimumFractionDigits: 0 });
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
		const ages = ds.map((d) => d.age_secs).filter((a) => a != null);
		if (!ages.length) return '—';
		return rel(nowSec() - Math.max(...(ages as number[])));
	}

	function syncLabel(): { text: string; cls: string } {
		const s = status;
		if (!s) return { text: 'SYNC …', cls: 'load' };
		if (s.syncing) return { text: 'SYNC LÄUFT', cls: 'load' };
		if (!hasData(s)) return s.last_error ? { text: 'SYNC FEHLER', cls: 'alert' } : { text: 'SYNC LÄUFT', cls: 'load' };
		if (!s.online) return { text: `OFFLINE · CACHE ${cacheAge(s)}`, cls: 'off' };
		return { text: `SYNC ${cacheAge(s)} · ${s.provider.toUpperCase()}`, cls: '' };
	}

	// ---- Detail-Zusammenfassung ----------------------------------------------
	function stats() {
		const ps = detail?.prices ?? [];
		const sell = ps.map((p) => p.sell_price).filter((v) => v > 0);
		const buy = ps.map((p) => p.buy_price).filter((v) => v > 0);
		const bestSell = sell.length ? Math.max(...sell) : null;
		const bestBuy = buy.length ? Math.max(...buy) : null;
		const avg = sell.length ? sell.reduce((a, b) => a + b, 0) / sell.length : null;
		const span = sell.length ? { lo: Math.min(...sell), hi: Math.max(...sell) } : null;
		return { bestSell, bestBuy, avg, span };
	}

	function barPct(price: number, lo: number, hi: number): number {
		if (hi <= lo) return 100;
		return Math.max(6, ((price - lo) / (hi - lo)) * 100);
	}

	// Alterspanne der Preise in Minuten: `{ lo, hi }` in Minuten, `null` wenn keine.
	function priceAgeRange(): { lo: number; hi: number } | null {
		const mins = (detail?.prices ?? [])
			.map((p) => Math.max(0, nowSec() - p.updated) / 60)
			.filter((m) => Number.isFinite(m));
		if (!mins.length) return null;
		return { lo: Math.min(...mins), hi: Math.max(...mins) };
	}

	const ICONS = { commodity: 'C', item: 'I', location: 'L' };

	function setFilter(f: string) {
		filter = f;
		if (input.trim()) void runSearch();
	}
</script>

<svelte:window onkeydown={onKey} />

<main class="pal-root">
	<div class="shell">
		<i class="deco t1"></i><i class="deco t2"></i>

		<!-- Suchzeile -->
		<div class="srow">
			<span class="ico">
				<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round">
					<circle cx="10.5" cy="10.5" r="6.5" />
					<path d="M15.4 15.4 21 21" />
				</svg>
			</span>
			<input
				class="q"
				type="text"
				bind:value={input}
				oninput={onInput}
				placeholder="Commodity, Item oder Terminal suchen …"
				spellcheck="false"
				autocomplete="off"
			/>
			<span class="smode">{phase === 'market' ? 'MARKETPLACE' : 'SUCHE'}</span>
			<span class="esc">ESC</span>
		</div>

		<!-- Filterleiste (verschwindet in Detail/Loading/Error/Market) -->
		{#if phase === 'typing' || phase === 'offline' || phase === 'idle'}
			<div class="fbar">
				<span class="chip" class:is-on={filter === 'all'} onclick={() => setFilter('all')}>Alle</span>
				<span class="chip" class:is-on={filter === 'commodity'} onclick={() => setFilter('commodity')}>
					<span class="dot"></span>Commodities
				</span>
				<span class="chip" class:is-on={filter === 'item'} onclick={() => setFilter('item')}>
					<span class="dot"></span>Items
				</span>
				<span class="chip" class:is-on={filter === 'location'} onclick={() => setFilter('location')}>
					<span class="dot"></span>Locations
				</span>
				{#if status?.online === false}
					<span class="spacer"></span>
					<span class="chip warn"><span class="dot"></span>Cache · veraltet</span>
				{/if}
			</div>
		{/if}

		<!-- Inhalt je Zustand -->
		{#if phase === 'loading'}
			<div class="pane-loading" role="status">
				<div class="center">
					<span class="spin"></span>
					<div class="big" style="margin-top: 12px">Erster Sync läuft</div>
					<div class="sm">Commodities · Terminals · Preise — danach wird sofort lokal gesucht.</div>
					<div class="sm" style="margin-top: 8px; font-size: 11px; opacity: 0.65">
						boot: {bootTrace} · phase: {phase} · status: {status ? 'ok' : '—'}
					</div>
				</div>
			</div>

		{:else if phase === 'error'}
			<div class="pane-error">
				<div class="center">
					<div class="big" style="color: var(--alert)">UEX nicht erreichbar</div>
					<div class="sm">
						{errorMsg ?? status?.last_error ?? 'Keine Verbindung'}<br />
						Cache-Stand: {cacheAge()}
					</div>
					<span class="btn" onclick={retrySync}>Erneut versuchen</span>
					<div class="sm" style="margin-top: 10px">
						<b style="color: var(--fgdim)">O</b> Offline mit Cache fortsetzen
					</div>
				</div>
			</div>

		{:else if phase === 'detail'}
			<div class="pane-detail">
				{#if detailBusy || !detail}
					<div class="center">
						<span class="spin"></span>
						<div class="big" style="margin-top: 12px">Lade Preise …</div>
					</div>
				{:else}
					{@const s = stats()}
					{@const ageRange = priceAgeRange()}
					<div class="dhead">
						<div class="t">
							<h2>{detail.commodity}</h2>
							<span class="tag" style="color: var(--fgdim)">UEX #{detail.prices[0]?.commodity_id ?? '—'}</span>
						</div>
						<div class="kpis">
							<div class="kpi">
								<div class="k">Bester Verkauf</div>
								<div class="v sell">{s.bestSell != null ? fmt(s.bestSell) : '—'}</div>
								<div class="u">aUEC/SCU</div>
							</div>
							<div class="kpi">
								<div class="k">Bester Kauf</div>
								<div class="v buy">{s.bestBuy != null ? fmt(s.bestBuy) : '—'}</div>
								<div class="u">{s.bestBuy == null ? 'kein Terminal kauft' : 'aUEC/SCU'}</div>
							</div>
							<div class="kpi">
								<div class="k">Ø UEX</div>
								<div class="v">{s.avg != null ? fmt(s.avg) : '—'}</div>
								<div class="u">{s.span ? `Spanne ${fmt(s.span.lo)}–${fmt(s.span.hi)}` : 'keine Kurse'}</div>
							</div>
						</div>
					</div>
					<table class="dtable">
						<thead>
							<tr>
								<th>Terminal</th>
								<th>Kauf</th>
								<th>Verkauf</th>
								<th>Bestand</th>
								<th>Alter</th>
							</tr>
						</thead>
						<tbody>
							{#each [...(detail.prices ?? [])].sort((a, b) => b.sell_price - a.sell_price) as p}
								<tr class:best={p.sell_price === s.bestSell}>
									<td>
										{p.location}
										{#if s.span}
											<span class="bar"><i style="width: {barPct(p.sell_price, s.span.lo, s.span.hi)}%"></i></span>
										{/if}
									</td>
									<td>{p.buy_price > 0 ? fmt(p.buy_price) : '—'}</td>
									<td>{p.sell_price > 0 ? fmt(p.sell_price) : '—'}</td>
									<td>{fmt(Math.max(p.sell_stock, p.buy_stock), 0)}</td>
									<td>{rel(p.updated)}</td>
								</tr>
							{/each}
						</tbody>
					</table>
					<div class="note">
						Quelle: <span class="src">UEX Corp</span> · Preise
						{ageRange ? `${fmt(ageRange.lo, 0)}–${fmt(ageRange.hi, 0)} min alt` : 'Alter unbekannt'}
					</div>
				{/if}
			</div>

		{:else if phase === 'market'}
			<div class="pane-market">
				<div class="center">
					<div class="big">Marketplace</div>
					<div class="sm">
						Anzeigen anderer Spieler erscheinen hier in Phase 2.<br />
						Zum Einstellen eigener Anzeigen fehlt der <b style="color: var(--fgdim)">UEX User-Token</b>.
					</div>
				</div>
			</div>

		{:else if phase === 'typing' || phase === 'offline' || phase === 'idle'}
			<div class="pane-list">
				{#if status?.online === false}
					<div class="offlineband">
						<span class="dot"></span>Offline · Cache von {cacheAge()} · Preise ggf. veraltet
					</div>
				{/if}
				{#if (input as string).trim() && phase !== 'idle'}
					<div class="list">
						{#each groups as g, gi}
							{#if filter === 'all' || g.kind === filter}
								<div class="vgroup">{g.label} <span class="n">{g.hits.length}</span></div>
								{#if g.hits.length}
									{#each g.hits as h, hi}
										{@const idx = groupOffset(gi) + hi}
										<div
											class="row"
											class:is-sel={idx === sel}
											onclick={() => {
												sel = idx;
												void openSelected();
											}}
										>
											<div class="ic">{ICONS[g.kind as keyof typeof ICONS] ?? '?'}</div>
											<div>
												<div class="name">{h.name}</div>
												<div class="sub">{h.subtitle}</div>
											</div>
											<div class="right">
												{#if phase === 'offline'}<span class="stale">cache</span>{:else}<b>{h.code ?? ''}</b>{/if}
											</div>
										</div>
									{/each}
								{:else}
									<div class="row-empty">Keine Treffer</div>
								{/if}
							{/if}
						{/each}
					</div>
				{:else}
					<!-- idle -->
					<div class="pane-idle">
						<div class="sect"><div class="h">Zuletzt gesucht</div></div>
						<div class="recent"><span class="r">Keine — tippe, um zu suchen</span></div>
						<div class="sect"><div class="h">Schnellzugriff</div></div>
						<div class="recent">
							<span class="r"><b>Marketplace</b> · M</span>
							<span class="r"><b>Vollständiger Sync</b></span>
						</div>
					</div>
				{/if}
			</div>
		{/if}

		<!-- Fußzeile (bleibt in allen Zuständen stehen → kein Layoutsprung) -->
		<div class="foot">
			<div class="hints">
				<span><kbd>↑↓</kbd> Nav</span>
				<span><kbd>↵</kbd> öffnen</span>
				<span><kbd>Esc</kbd> {isSpotlight ? 'ausblenden' : 'leeren'}</span>
				<span><kbd>M</kbd> Market</span>
			</div>
			<span class="sync {syncLabel().cls}"><span class="dot"></span>{syncLabel().text}</span>
			{#if isSpotlight}
				<div class="spot-status">
					<span class="dot" class:live={overlay?.game_running}></span>
					<button onclick={toggleClickThrough}>
						{overlay?.click_through ? 'KLICK' : 'FOKUS'}
					</button>
				</div>
			{/if}
		</div>
	</div>
</main>