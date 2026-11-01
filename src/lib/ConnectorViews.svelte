<script lang="ts">
	// ConnectorViews — generische Views aus Manifest-Typen (C2, t_91124322).
	//
	// Rendert ALLE View-Typen (table/cards/detail/calendar/form) eines
	// aktivierten Connectors — KEIN connector-spezifisches Wissen. Daten
	// kommen über `connector_fetch` (Rust: Header-Auth + TTL-Cache +
	// Dot-Path-Mapping) bzw. Writes über `connector_form_submit`
	// (nur user-initiiert, kein Polling).
	//
	// Polling: `poll_seconds` aus dem Manifest (Default 300 — sparsam,
	// sclm.site ist LIVE), Manual-Button = force:true (umgeht den Cache),
	// Cached-Alter wird angezeigt.
	import { onDestroy, untrack } from 'svelte';
	import { invoke, isTauri } from '@tauri-apps/api/core';
	import { t, tr, trError, currentLang } from './i18n';
	import FlowRenderer from './FlowRenderer.svelte';
	import {
		labelOf,
		formatValue,
		tableColumns,
		calendarDay,
		formValue,
		loadPins,
		savePin,
		type ConnectorEntry,
		type FetchResultDTO,
		type FlowDTO,
		type ViewDTO
		} from './connector';

	type Result = { data: FetchResultDTO | null; error: string; loading: boolean };

	let {
		entry,
		embedded = false,
		onexit
	}: {
		entry: ConnectorEntry;
		embedded?: boolean;
		onexit?: () => void;
	} = $props();

	const views = $derived(entry.manifest.views ?? []);
	// SF1: Screen-Flows — eigene Tabs (Muster wie Routen/Trade-Logs), der
	// Flow-Renderer übernimmt die komplette Kette (generisch, Overlay-Optik).
	const flows = $derived(entry.manifest.flows ?? []);
	let viewId = $state<string>('');
	let flowId = $state<string | null>(null);
	// Erster View als Start-Auswahl (einmalig beim Mount).
	let res = $state<Result>({ data: null, error: '', loading: false });
	let formValues = $state<Record<string, string>>({});
	let formNote = $state('');
	let formBusy = $state(false);
	// C3: View-Parameter (Session-Codes) + Pin-Speicher pro Connector/View.
	let paramValues = $state<Record<string, string>>({});
	let paramLoaded = $state(false);
	let pinnedKeys = $state<Record<string, boolean>>({});
	let timer: ReturnType<typeof setInterval> | null = null;

	const view = $derived<ViewDTO | undefined>(views.find((v) => v.id === viewId));
	// SF1: aktivierter Flow (für den Content-Zweig unterhalb).
	const activeFlow = $derived<FlowDTO | null>(
		flowId ? (flows.find((f) => f.id === flowId) ?? null) : null
	);
	const lang = $derived(currentLang());
	const title = $derived(labelOf(entry.manifest.name, lang));
	// Zeilen/Detail robust aus dem gemappten Ergebnis (Rust mappt per
	// Dot-Path): Zeilen-Views erwarten ein Array, Detail ein Objekt.
	const rows = $derived<Record<string, unknown>[]>(
		Array.isArray(res.data?.mapped) ? (res.data!.mapped as Record<string, unknown>[]) : []
	);
	const detailRow = $derived<Record<string, unknown> | null>(
		res.data?.mapped && !Array.isArray(res.data.mapped)
			? (res.data.mapped as Record<string, unknown>)
			: null
	);

	function noteForView(v: ViewDTO | undefined): string {
		return v ? `${title} · ${labelOf(v.title, lang)}` : title;
	}

	async function load(force = false) {
		if (!isTauri || !view || view.type === 'form') return;
		// Pflicht-Parameter: fehlt einer → gar nicht erst fetchen (Backend
		// würde dieselbe Meldung liefern; hier sparen wir den Request).
		const missing = (view.params ?? []).find((p) => !(paramValues[p.key] ?? '').trim());
		if (missing) {
			res = {
				data: null,
				error: tr('error.connector.param_required', { key: labelOf(missing.label, lang) }),
				loading: false
			};
			return;
		}
		res = { data: null, error: '', loading: true };
		try {
			const data = await invoke<FetchResultDTO>('connector_fetch', {
				id: entry.id,
				viewId: view.id,
				force,
				params: { ...paramValues }
			});
			res = { data, error: '', loading: false };
		} catch (err) {
			res = { data: null, error: trError(err), loading: false };
		}
	}

	function selectView(id: string) {
		if (id === viewId) return;
		viewId = id;
		flowId = null; // SF1: Tab-Mutual-Exklusivität (Flow deaktivieren)
		formValues = {};
		formNote = '';
		paramValues = {};
		pinnedKeys = {};
		paramLoaded = false;
		res = { data: null, error: '', loading: false };
		schedulePolling();
		// Der Fetch läuft im $effect (reagiert auf viewId) — hier kein zweiter
		// Aufruf, sonst Double-Fetch pro Tab-Wechsel.
	}

	// SF1: Flow-Tab aktivieren — der Flow-Renderer übernimmt die Kette.
	function selectFlow(id: string) {
		if (id === flowId) return;
		flowId = id;
		formValues = {};
		formNote = '';
		paramValues = {};
		pinnedKeys = {};
		res = { data: null, error: '', loading: false };
		if (timer) {
			clearInterval(timer);
			timer = null;
		}
	}

	// Polling sparsam (Manifest `poll_seconds`, Default 300 s): der
	// Backend-TTL-Cache macht doppelte Requests ohnehin unmöglich — das
	// Intervall hier entscheidet, wie OFT die UI nachfragt.
	function schedulePolling() {
		if (timer) {
			clearInterval(timer);
			timer = null;
		}
		const secs = view?.poll_seconds ?? 300;
		if (!view || view.type === 'form' || !isTauri) return;
		timer = setInterval(() => void load(false), Math.max(secs, 30) * 1000);
	}

	function age(): string {
		const ms = res.data?.fetched_at_ms ?? 0;
		if (!ms) return '';
		const s = Math.max(0, Math.floor((Date.now() - ms) / 1000));
		return s < 90 ? `${s}s` : `${Math.floor(s / 60)}min`;
	}

	async function submitForm() {
		const v = view;
		if (!v?.form) return;
		formBusy = true;
		formNote = '';
		try {
			const values: Record<string, string | number> = {};
			for (const f of v.form.fields) {
				const raw = formValue(formValues[f.key] ?? '', f.type);
				if (f.required && String(raw).trim() === '') {
					formNote = tr('error.connector.field_required', { key: labelOf(f.label, lang) });
					formBusy = false;
					return;
				}
				values[f.key] = raw;
			}
			const out = await invoke<{ status: number; body: unknown }>('connector_form_submit', {
				id: entry.id,
				formId: v.id,
				values
			});
			let note =
				out.status < 400
					? tr('connector.form_ok', { status: String(out.status) })
					: tr('connector.form_http_error', { status: String(out.status) });
			// Antwort-Körper kompakt mit anzeigen (generisch): SCLM liefert z. B.
			// bei „Event erstellen“ die frischen admin/join-Codes zurück.
			let bodyText = '';
			try {
				bodyText = JSON.stringify(out.body);
			} catch {
				/* nicht serialisierbar → ohne Körper */
			}
			if (bodyText && bodyText !== 'null') {
				note += ` — ${bodyText.length > 220 ? `${bodyText.slice(0, 220)}…` : bodyText}`;
			}
			formNote = note;
		} catch (err) {
			formNote = trError(err);
		}
		formBusy = false;
	}

	$effect(() => {
		// Bei View-/Entry-Wechsel: Pins laden, Polling neu aufsetzen, initial
		// laden (fehlende Pflicht-Parameter zeigen den Eingabe-Prompt). Die
		// Pin-Loads laufen UNTRACKED — der Effect hängt nur an viewId; die
		// paramValues-Writes dürfen ihn nicht neu triggern (Loop-Schutz).
		if (!viewId && views.length) viewId = views[0].id;
		const id = viewId;
		if (id && !paramLoaded) {
			paramLoaded = true;
			untrack(() => {
				const pins = loadPins(entry.id, id);
				paramValues = { ...pins };
				pinnedKeys = Object.fromEntries(Object.keys(pins).map((k) => [k, true]));
			});
		}
		schedulePolling();
		void load(false);
	});
	onDestroy(() => {
		if (timer) clearInterval(timer);
	});

	function pinParam(key: string) {
		const v = (paramValues[key] ?? '').trim();
		if (!v || !viewId) return;
		savePin(entry.id, viewId, key, v);
		pinnedKeys = { ...pinnedKeys, [key]: true };
		void load(true);
	}

	function unpinParam(key: string) {
		if (!viewId) return;
		savePin(entry.id, viewId, key, null);
		pinnedKeys = { ...pinnedKeys, [key]: false };
	}
</script>

<section
	class="connector"
	class:is-embedded={embedded}
	aria-label={noteForView(view)}
>
	{#if !embedded && onexit}
		<div class="connector-head">
			<span class="connector-title">{title}</span>
			<button type="button" class="btn-ghost" onclick={onexit}>Esc · {$t('common.back')}</button>
		</div>
	{/if}

	{#if views.length > 1 || flows.length > 0}
		<div class="connector-tabs" role="tablist" aria-label={title}>
			{#each flows as fl (fl.id)}
				<button
					type="button"
					class="ctab"
					class:is-on={flowId === fl.id}
					aria-pressed={flowId === fl.id}
					onclick={() => selectFlow(fl.id)}
				>{labelOf(fl.title, lang)}</button>
			{/each}
			{#each views as v (v.id)}
				<button
					type="button"
					role="tab"
					class="ctab"
					class:is-on={v.id === viewId}
					aria-selected={v.id === viewId}
					onclick={() => selectView(v.id)}
				>
					{labelOf(v.title, lang)}
				</button>
			{/each}
		</div>
	{/if}

	{#if view && view.type !== 'form' && (view.params ?? []).length > 0}
		<!-- C3: View-Parameter (z. B. Session-Code) — Eingabe + Pin im
		     Connector-Zustand, NICHT global in den Settings. -->
		<div class="cparams" role="group">
			{#each view.params ?? [] as p (p.key)}
				<label class="cparam">
					<span class="cfield-label"
						>{labelOf(p.label, lang)}{pinnedKeys[p.key]
							? ` · ${$t('connector.pinned')}`
							: ''}</span
					>
					<span class="cparam-row">
						<input
							type="text"
							bind:value={paramValues[p.key]}
							onchange={() => void load(true)}
						/>
						{#if pinnedKeys[p.key]}
							<button
								type="button"
								class="btn-ghost"
								title={$t('connector.unpin')}
								onclick={() => unpinParam(p.key)}
							>✕</button
							>
						{:else}
							<button
								type="button"
								class="btn-ghost"
								title={$t('connector.pin')}
								disabled={!(paramValues[p.key] ?? '').trim()}
								onclick={() => pinParam(p.key)}
							>📌</button
							>
						{/if}
					</span>
				</label>
			{/each}
		</div>
	{/if}

	{#if view && view.type !== 'form'}
		<div class="connector-tools">
			{#if res.loading}
				<span class="cstate">{$t('common.loading')}</span>
			{:else if res.data}
				<span class="cstate" title={$t('connector.ttl_title')}>
					{res.data.cached ? $t('connector.cached') : $t('connector.fresh')} · {age()}
				</span>
			{/if}
			<button type="button" class="btn-ghost" onclick={() => void load(true)}>
				{$t('common.refresh')}
			</button>
		</div>
	{/if}

	{#if activeFlow}
		<!-- SF1: Flow-Renderer übernimmt die komplette Screen-Kette
		     (generisch — KEIN connector-spezifisches Wissen hier). -->
		<FlowRenderer {entry} flow={activeFlow} />
	{:else if res.error}
		<div class="cerror" role="alert">{res.error}</div>
	{:else if view?.type === 'form'}
		<form
			class="cform"
			onsubmit={(e) => {
				e.preventDefault();
				void submitForm();
			}}
		>
			{#each view.form?.fields ?? [] as f (f.key)}
				<label class="cfield">
					<span class="cfield-label"
						>{labelOf(f.label, lang)}{f.required ? ' *' : ''}</span
					>
					{#if f.type === 'select'}
						<select bind:value={formValues[f.key]}>
							<option value=""></option>
							{#each f.options ?? [] as o}
								<option value={labelOf(o, lang)}>{labelOf(o, lang)}</option>
							{/each}
						</select>
					{:else}
						<input type={f.type === 'number' ? 'number' : 'text'} bind:value={formValues[f.key]} />
					{/if}
				</label>
			{/each}
			<div class="cform-actions">
				<button type="submit" class="btn" disabled={formBusy}>
					{formBusy ? $t('common.sending') : $t('connector.form_submit')}
				</button>
				{#if formNote}
					<span class="cform-note" role="status">{formNote}</span>
				{/if}
			</div>
		</form>
	{:else if view?.type === 'table'}
		<div class="ctable-wrap">
			<table class="ctable">
				<thead>
					<tr>
						{#each tableColumns(view) as col (col)}
							<th>{col}</th>
						{/each}
					</tr>
				</thead>
				<tbody>
					{#each rows as row, i (i)}
						<tr>
							{#each tableColumns(view) as col (col)}
								<td>{formatValue(row?.[col])}</td>
							{/each}
						</tr>
					{:else}
						<tr><td class="cempty" colspan={Math.max(tableColumns(view).length, 1)}>
							{$t('connector.empty')}
						</td></tr>
					{/each}
				</tbody>
			</table>
		</div>
	{:else if view?.type === 'cards'}
		<div class="ccards">
			{#each rows as row, i (i)}
				<div class="ccard">
					{#each Object.entries(view.mapping ?? {}) as [field], j (field)}
						<div class="ccard-line" class:ccard-main={j === 0}>
							<span class="ck">{field}</span>
							<span class="cv">{formatValue(row?.[field])}</span>
						</div>
					{/each}
				</div>
			{:else}
				<span class="cempty">{$t('connector.empty')}</span>
			{/each}
		</div>
	{:else if view?.type === 'detail'}
		<div class="cdetail">
			{#each Object.entries(view.mapping ?? {}) as [field], j (field)}
				<div class="cdetail-line">
					<span class="ck">{field}</span>
					<span class="cv" class:cdetail-main={j === 0}
						>{formatValue((res.data?.mapped as Record<string, unknown> | undefined)?.[field])}</span
					>
				</div>
			{/each}
		</div>
	{:else if view?.type === 'calendar'}
		<div class="ccal">
			{#each rows as row, i (i)}
				<!-- Mapping-Keys sind die Feldnamen der gemappten Zeilen (map_view
					erzeugt `row.insert(key, ...)`) - Kalender-Schema: start/title. -->
				{@const day = calendarDay(row?.start)}
				<div class="ccal-row">
					<span class="ccal-day">{day}</span>
					<span class="ccal-title"
						>{formatValue(row?.title)}</span
					>
				</div>
			{:else}
				<span class="cempty">{$t('connector.empty')}</span>
			{/each}
		</div>
	{/if}
</section>

<style>
	.connector {
		display: flex;
		flex-direction: column;
		gap: 8px;
		min-height: 0;
	}
	.connector-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 8px;
	}
	.connector-title {
		font-weight: 700;
		letter-spacing: 0.08em;
		text-transform: uppercase;
		font-size: 12px;
		opacity: 0.9;
	}
	.connector-tabs {
		display: flex;
		gap: 4px;
		flex-wrap: wrap;
	}
	.ctab {
		background: transparent;
		border: 1px solid currentColor;
		border-color: color-mix(in srgb, currentColor 25%, transparent);
		color: inherit;
		border-radius: 6px;
		padding: 3px 10px;
		font-size: 12px;
		cursor: pointer;
	}
	.ctab.is-on {
		border-color: currentColor;
		background: color-mix(in srgb, currentColor 12%, transparent);
		font-weight: 600;
	}
	.connector-tools {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 11px;
		opacity: 0.85;
	}
	.cerror {
		color: #ff7a7a;
		font-size: 12px;
	}
	.cstate {
		font-size: 11px;
		opacity: 0.8;
	}
	.cempty {
		opacity: 0.6;
		font-size: 12px;
		padding: 6px;
	}
	.ctable-wrap {
		overflow: auto;
		min-height: 0;
	}
	.ctable {
		width: 100%;
		border-collapse: collapse;
		font-size: 12px;
	}
	.ctable th,
	.ctable td {
		text-align: left;
		padding: 4px 8px;
		border-bottom: 1px solid color-mix(in srgb, currentColor 15%, transparent);
		white-space: nowrap;
	}
	.ctable th {
		text-transform: uppercase;
		letter-spacing: 0.06em;
		font-size: 10px;
		opacity: 0.75;
	}
	.ccards {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
		gap: 8px;
		overflow: auto;
	}
	.ccard {
		border: 1px solid color-mix(in srgb, currentColor 20%, transparent);
		border-radius: 8px;
		padding: 8px 10px;
		display: flex;
		flex-direction: column;
		gap: 3px;
	}
	.ccard-line,
	.cdetail-line {
		display: flex;
		gap: 8px;
		font-size: 12px;
	}
	.ck {
		opacity: 0.6;
		min-width: 80px;
	}
	.cdetail-main,
	.ccard-main .cv {
		font-weight: 600;
		font-size: 13px;
	}
	.cdetail {
		display: flex;
		flex-direction: column;
		gap: 6px;
		overflow: auto;
	}
	.ccal {
		display: flex;
		flex-direction: column;
		gap: 4px;
		overflow: auto;
	}
	.ccal-row {
		display: flex;
		gap: 10px;
		font-size: 12px;
		padding: 3px 0;
		border-bottom: 1px solid color-mix(in srgb, currentColor 12%, transparent);
	}
	.ccal-day {
		opacity: 0.7;
		font-variant-numeric: tabular-nums;
	}
	.cform {
		display: flex;
		flex-direction: column;
		gap: 8px;
		max-width: 420px;
	}
	.cfield {
		display: flex;
		flex-direction: column;
		gap: 3px;
		font-size: 12px;
	}
	.cfield-label {
		opacity: 0.8;
	}
	.cfield input,
	.cfield select {
		background: transparent;
		border: 1px solid color-mix(in srgb, currentColor 30%, transparent);
		color: inherit;
		border-radius: 6px;
		padding: 5px 8px;
		font: inherit;
	}
	.cform-actions {
		display: flex;
		align-items: center;
		gap: 8px;
	}
	.cform-note {
		font-size: 12px;
		opacity: 0.9;
	}
	.cparams {
		display: flex;
		flex-direction: column;
		gap: 6px;
		max-width: 420px;
	}
	.cparam {
		display: flex;
		flex-direction: column;
		gap: 3px;
		font-size: 12px;
	}
	.cparam-row {
		display: flex;
		gap: 4px;
		align-items: center;
	}
	.cparam-row input {
		flex: 1;
		background: transparent;
		border: 1px solid color-mix(in srgb, currentColor 30%, transparent);
		color: inherit;
		border-radius: 6px;
		padding: 5px 8px;
		font: inherit;
	}
</style>
