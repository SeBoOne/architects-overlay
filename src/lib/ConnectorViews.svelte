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
	import { onDestroy } from 'svelte';
	import { invoke, isTauri } from '@tauri-apps/api/core';
	import { t, tr, trError, currentLang } from './i18n';
	import {
		labelOf,
		formatValue,
		tableColumns,
		calendarDay,
		formValue,
		type ConnectorEntry,
		type FetchResultDTO,
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
	let viewId = $state<string>('');
	// Erster View als Start-Auswahl (einmalig beim Mount).
	let res = $state<Result>({ data: null, error: '', loading: false });
	let formValues = $state<Record<string, string>>({});
	let formNote = $state('');
	let formBusy = $state(false);
	let timer: ReturnType<typeof setInterval> | null = null;

	const view = $derived<ViewDTO | undefined>(views.find((v) => v.id === viewId));
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
		res = { data: null, error: '', loading: true };
		try {
			const data = await invoke<FetchResultDTO>('connector_fetch', {
				id: entry.id,
				viewId: view.id,
				force
			});
			res = { data, error: '', loading: false };
		} catch (err) {
			res = { data: null, error: trError(err), loading: false };
		}
	}

	function selectView(id: string) {
		if (id === viewId) return;
		viewId = id;
		formValues = {};
		formNote = '';
		res = { data: null, error: '', loading: false };
		schedulePolling();
		// Der Fetch läuft im $effect (reagiert auf viewId) — hier kein zweiter
		// Aufruf, sonst Double-Fetch pro Tab-Wechsel.
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
				const raw = formValues[f.key] ?? '';
				if (f.required && raw.trim() === '') {
					formNote = tr('error.connector.field_required', { key: labelOf(f.label, lang) });
					formBusy = false;
					return;
				}
				values[f.key] = formValue(raw, f.type);
			}
			const out = await invoke<{ status: number; body: unknown }>('connector_form_submit', {
				id: entry.id,
				formId: v.id,
				values
			});
			formNote =
				out.status < 400
					? tr('connector.form_ok', { status: String(out.status) })
					: tr('connector.form_http_error', { status: String(out.status) });
		} catch (err) {
			formNote = trError(err);
		}
		formBusy = false;
	}

	$effect(() => {
		// Bei View-/Entry-Wechsel: Polling neu aufsetzen + initial laden.
		if (!viewId && views.length) viewId = views[0].id;
		void viewId;
		schedulePolling();
		void load(false);
	});
	onDestroy(() => {
		if (timer) clearInterval(timer);
	});
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

	{#if views.length > 1}
		<div class="connector-tabs" role="tablist" aria-label={title}>
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

	{#if res.error}
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
</style>
