<script lang="ts">
	// FlowRenderer — generischer Screen-Flow-Renderer (SF1, t_c35bc233).
	//
	// Rendert deklarative Screen-Ketten aus dem Manifest (`flows`) mit FESTER
	// Overlay-Optik: Tabs statt Buttons (Muster wie Routen/Trade-Logs),
	// ke Fremdstyling. KEIN connector-spezifisches Wissen: alles kommt aus
	// dem Flow-Schema (Screens: code_entry | list | detail | form | wait |
	// results, conditions, transitions, poll, autocomplete).
	//
	// - Display-Regel (`when`): Screen nur zeigen, wenn Bedingung wahr; sonst
	//   der `otherwise`-Screen.
	// - Transitions: `on_submit` / `on_select` → nächster Screen.
	// - Wait: poll_seconds-Intervall, exit-Bedingung (z. B. raffle_done) →
	//   Ziel-Screen.
	// - Zustands-Pinning pro Session: gepinnte Werte (admin_code/join_code/
	//   pcode) leben im Connector-Zustand, NICHT global (localStorage pro
	//   Connector+Flow+Session).
	//
	// ESC-Kaskade: erst Flow beenden → onexit (wie im Rest des Overlays).
	import { onDestroy, untrack } from 'svelte';
	import { invoke, isTauri } from '@tauri-apps/api/core';
	import { t, tr, trError, currentLang } from './i18n';
	import {
		labelOf,
		formatValue,
		evaluateCondition,
		branchTarget,
		applyAutofill,
		resolvePlaceholders,
		loadFlowPins,
		saveFlowPin,
		type ConnectorEntry,
		type FetchResultDTO,
		type FlowDTO,
		type ScreenDTO,
		type ConditionDTO,
		type AutocompleteRuleDTO
	} from './connector';

	type Result = { data: FetchResultDTO | null; error: string; loading: boolean };

	let {
		entry,
		flow,
		initial = null,
		onexit
	}: {
		entry: ConnectorEntry;
		flow: FlowDTO;
		/** Start-Screen-ID (z. B. Wiedereinstieg direkt beim Ergebnis-Screen). */
		initial?: string | null;
		onexit?: () => void;
	} = $props();

	const lang = $derived(currentLang());
	const flowTitle = $derived(labelOf(flow.title, lang));

	function findScreen(id: string): ScreenDTO | undefined {
		return flow.screens.find((s) => s.id === id);
	}

	// ---- Flow-Zustand -----------------------------------------------------
	// Bewusst nur der Initialwert: der Einstiegs-Screen wird einmal beim
	// Mount bestimmt (untrack — spätere Prop-Änderungen ändern den Start nicht).
	const startId: string = untrack(() =>
		initial && findScreen(initial) ? initial : flow.entry
	);
	let screenId: string = $state(startId);
	let current: ScreenDTO | null = $derived(findScreen(screenId) ?? null);
	let path: string[] = $state([]); // Historie für Zurück-Navigation
	let fstate: Record<string, unknown> = $state({}); // gemappte Daten + Pins
	let pins: Record<string, string> = $state({});
	let pinsLoaded = $state(false);
	let res: Result = $state({ data: null, error: '', loading: false });
	let formValues: Record<string, string> = $state({});
	let formNote = $state('');
	let formBusy = $state(false);
	let autofilled: Record<string, boolean> = $state({});
	let acItems: Record<string, { label: string; value: Record<string, unknown> }[]> = $state({});
	let acOpen: Record<string, boolean> = $state({});
	let timer: ReturnType<typeof setInterval> | null = null;

	// Display-Regel: wenn `when` falsch → otherwise-Screen (falls deklariert).
	function resolvedScreen(): ScreenDTO | null {
		let s = current;
		let guard = 0;
		while (s?.when && guard++ < 8) {
			const alt = s.when.otherwise ? findScreen(s.when.otherwise) : null;
			if (evaluateCondition(s.when, fstate)) break;
			s = alt ?? null;
		}
		return s;
	}
	const shown = $derived(resolvedScreen());
	// Zeilen robust aus dem gemappten Ergebnis (Listen/Ergebnisse erwarten
	// ein Array) — als $derived, nicht im Template (Svelte-Parsing-Grenze).
	const rows = $derived<Record<string, unknown>[]>(
		Array.isArray(res.data?.mapped) ? (res.data!.mapped as Record<string, unknown>[]) : []
	);
	// Detail-Objekt (nicht-Array-Mapping-Ergebnis) ebenfalls vorkompiliert.
	const detailRow = $derived<Record<string, unknown> | null>(
		res.data?.mapped && !Array.isArray(res.data.mapped)
			? (res.data.mapped as Record<string, unknown>)
			: null
	);

	// ---- Session-Pins (Zustand pro Session, nicht global) ------------------
	function ensurePins() {
		if (pinsLoaded) return;
		pins = loadFlowPins(entry.id, flow.id);
		pinsLoaded = true;
		// Pins sind Teil des Zustands (Condition-Auswertung + Templates).
		for (const [k, v] of Object.entries(pins)) if (!(k in fstate)) fstate[k] = v;
	}
	$effect(() => {
		ensurePins();
	});

	function pin(key: string, value: string | null) {
		pins = saveFlowPin(entry.id, flow.id, key, value);
		if (value === null) delete fstate[key];
		else fstate[key] = value;
	}

	// ---- Daten laden (generisch: Fetch + Mapping via Backend) --------------
	async function load(force = false) {
		const s = shown;
		if (!isTauri || !s || s.type === 'form' || !s.endpoint) return;
		const missing = (s.params ?? []).find((p) => !(String(fstate[p.key] ?? '')).trim());
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
				viewId: s.id,
				force,
				params: bindings(s)
			});
			res = { data, error: '', loading: false };
			// Mapped-Daten in den Flow-Zustand (Conditions sehen den letzten Fetch).
			if (data.mapped && typeof data.mapped === 'object') {
				fstate = { ...fstate, ...(data.mapped as Record<string, unknown>) };
			}
			// Wait-Screens: Exit-Bedingung prüfen → Transition.
			if (s.type === 'wait') {
				const target = s.exit ? branchTarget(s.exit, fstate) : null;
				if (target) go(target);
			}
		} catch (err) {
			res = { data: null, error: trError(err), loading: false };
		}
	}

	/** Template-Bindings: gepinnte Werte + Formular-Werte für {param}. */
	function bindings(s: ScreenDTO): Record<string, string> {
		const out: Record<string, string> = {};
		for (const p of s.params ?? []) {
			const v = fstate[p.key] ?? pins[p.key];
			if (typeof v === 'string' || typeof v === 'number') out[p.key] = String(v);
		}
		return out;
	}

	// ---- Navigation (ESC-Kaskade wie im Overlay) ---------------------------
	function go(id: string) {
		if (id === screenId) return;
		if (!findScreen(id)) return; // unbekanntes Ziel: stillschweigend ignorieren? Nein — bewusst loggen.
		path = [...path, screenId];
		screenId = id;
		formValues = {};
		formNote = '';
		autofilled = {};
		acItems = {};
		acOpen = {};
		res = { data: null, error: '', loading: false };
		untrack(() => void load());
	}

	function back() {
		const prev = path[path.length - 1];
		if (!prev) return;
		path = path.slice(0, -1);
		screenId = prev;
		formValues = {};
		res = { data: null, error: '', loading: false };
		untrack(() => void load());
	}

	function exit() {
		onexit?.();
	}

	// ---- Poll-Regeln (wait: poll_seconds + exit-condition) -----------------
	function schedulePolling() {
		if (timer) clearInterval(timer);
		timer = null;
		const s = shown;
		if (!s || s.type !== 'wait') return;
		const secs = Math.max(s.poll_seconds ?? 15, 5);
		timer = setInterval(() => void load(false), secs * 1000);
	}
	$effect(() => {
		untrack(() => void load());
		schedulePolling();
		return () => {
			if (timer) clearInterval(timer);
		};
	});

	// ---- Formular: Submit → Transition -------------------------------------
	async function submitForm() {
		const s = shown;
		if (!s?.form) return;
		formBusy = true;
		try {
			const out = await invoke<{ status: number; body: unknown }>('connector_form_submit', {
				id: entry.id,
				formId: s.id,
				values: formValues
			});
			formNote =
				out.status < 400
					? tr('connector.form_ok', { status: String(out.status) })
					: tr('connector.form_http_error', { status: String(out.status) });
			// Response-Werte (z. B. neue Codes) in den Zustand + Pins.
			if (out.body && typeof out.body === 'object') {
				const body = out.body as Record<string, unknown>;
				for (const [k, v] of Object.entries(body)) {
					fstate[k] = v;
					if ((s.pins ?? []).includes(k) && typeof v === 'string') pin(k, v);
				}
			}
			if (out.status < 400 && s.on_submit) go(s.on_submit);
		} catch (err) {
			formNote = trError(err);
		} finally {
			formBusy = false;
		}
	}

	// ---- Listenauswahl → Transition ---------------------------------------
	function select(id: string, row: Record<string, unknown>) {
		const s = shown;
		if (!s) return;
		// Ausgewählte Zeile in den Zustand (Condition + Templates sehen sie).
		fstate = { ...fstate, ...row };
		// Gepinnte Keys aus der Zeile (z. B. participant-ID pinnen).
		for (const k of s.pins ?? []) {
			const v = row[k];
			if (typeof v === 'string' || typeof v === 'number') pin(k, String(v));
		}
		if (s.on_select) go(s.on_select);
	}

	// ---- Autocomplete + Auto-Fill ------------------------------------------
	async function searchAC(rule: AutocompleteRuleDTO, text: string) {
		if (!isTauri || text.trim().length < 2) {
			acItems = { ...acItems, [rule.field]: [] };
			acOpen = { ...acOpen, [rule.field]: false };
			return;
		}
		try {
			const ep = resolvePlaceholders(rule.endpoint, {
				...Object.fromEntries(Object.entries(fstate).map(([k, v]) => [k, String(v ?? '')])),
				q: text
			});
			const data = await invoke<FetchResultDTO>('connector_fetch', {
				id: entry.id,
				viewId: `${flow.id}:${rule.field}`,
				force: true,
				params: { q: text, __flow_ep: ep }
			});
			const rows = Array.isArray(data.body) ? (data.body as unknown[]) : [];
			acItems = {
				...acItems,
				[rule.field]: rows
					.slice(0, 8)
					.map((r) => ({
						label: typeof r === 'object' && r !== null ? String((r as Record<string, unknown>).name ?? JSON.stringify(r)) : String(r),
						value: (r ?? {}) as Record<string, unknown>
					}))
			};
			acOpen = { ...acOpen, [rule.field]: true };
		} catch {
			acItems = { ...acItems, [rule.field]: [] };
		}
	}

	function pickAC(rule: AutocompleteRuleDTO, item: Record<string, unknown>) {
		const filled = applyAutofill(item, rule);
		formValues = { ...formValues, ...filled };
		for (const k of Object.keys(filled)) autofilled[k] = true;
		acOpen = { ...acOpen, [rule.field]: false };
	}

	onDestroy(() => {
		if (timer) clearInterval(timer);
	});
</script>

<section class="flow">
	<header class="fhead">
		<div class="ftitle">
			<span class="fname">{flowTitle}</span>
			{#if shown}
				<span class="fstep">
					{tr('connector.flow.step', {
						n: String(flow.screens.findIndex((x) => x.id === shown.id) + 1),
						total: String(flow.screens.length)
					})}
				</span>
			{/if}
		</div>
		<div class="fnav">
			{#if path.length > 0}
				<button class="fbtn" type="button" onclick={back}>{$t('common.back')}</button>
			{/if}
			<button class="fbtn" type="button" onclick={exit}>{$t('common.back_esc')}</button>
		</div>
	</header>

	{#if shown === null}
		<p class="fempty">{tr('error.connector.flow_screen_missing')}</p>
	{:else}
		<nav class="ftabs">
			{#each flow.screens as s (s.id)}
				<button
					class="ftab"
					class:is-on={s.id === shown.id}
					type="button"
					onclick={() => go(s.id)}
				>{labelOf(s.title, lang)}</button>
			{/each}
		</nav>

		<div class="fbody">
			{#if res.loading}
				<p class="fempty">…</p>
			{:else if res.error}
				<p class="ferror">{res.error}</p>
			{:else if shown.type === 'code_entry' || shown.type === 'form'}
				<form class="fform" onsubmit={(e) => { e.preventDefault(); if (shown.type === 'form') void submitForm(); }}>
					{#each shown.form?.fields ?? [] as f (f.key)}
						<label class="ffield">
							<span class="flabel">{labelOf(f.label, lang)}{f.required ? ' *' : ''}</span>
							<input
								class="finput"
								type={f.type === 'number' ? 'number' : 'text'}
								required={f.required}
								bind:value={formValues[f.key]}
								oninput={() => {
									const rule = (shown.autocomplete ?? []).find((a) => a.field === f.key);
									if (rule) void searchAC(rule, String(formValues[f.key] ?? ''));
								}}
							/>
							{#if autofilled[f.key]}
								<span class="fauto">{tr('connector.flow.autofill_applied')}</span>
							{/if}
						</label>
						{#if acOpen[f.key] && (acItems[f.key] ?? []).length > 0}
							<ul class="facs">
								{#each acItems[f.key] as item, i (i)}
									<li>
										<button
											class="fac"
											type="button"
											onclick={() => {
												const rule = (shown.autocomplete ?? []).find((a) => a.field === f.key);
												if (rule) pickAC(rule, item.value);
											}}
										>{item.label}</button>
									</li>
								{/each}
							</ul>
						{/if}
					{/each}
					<button class="fbtn fsubmit" type="submit" disabled={formBusy}>
						{shown.type === 'code_entry' ? tr('connector.flow.code_enter') : $t('connector.form_submit')}
					</button>
					{#if formNote}<p class="fnote">{formNote}</p>{/if}
				</form>
			{:else if shown.type === 'list' || shown.type === 'results'}
				<ul class="flist">
					{#each rows as row, i (i)}
						<li>
							<button
								class="frow"
								type="button"
								onclick={() => select(String(row.id ?? i), row)}
							>
								{#each Object.entries(shown.mapping ?? {}) as [field], j (field)}
									<span class="fk">{field}</span><span class="fv">{formatValue(row?.[field])}</span>
								{/each}
							</button>
						</li>
					{:else}
						<li class="fempty">{$t('connector.empty')}</li>
					{/each}
				</ul>
			{:else}
				<dl class="fdetail">
					{#each Object.entries(shown.mapping ?? {}) as [field], j (field)}
						<div class="fdetail-line">
							<dt class="fk">{field}</dt>
							<dd class="fv">{formatValue(detailRow?.[field])}</dd>
						</div>
					{/each}
				</dl>
			{/if}
			{#if shown.type === 'wait'}
				<p class="fwait">{tr('connector.flow.wait_poll', { s: String(shown.poll_seconds ?? 15) })}</p>
			{/if}
		</div>
	{/if}
</section>

<style>
	.flow { display: flex; flex-direction: column; gap: 8px; min-height: 0; }
	.fhead { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
	.ftitle { display: flex; flex-direction: column; gap: 2px; }
	.fname { font-weight: 700; text-transform: uppercase; letter-spacing: 0.06em; font-size: 12px; }
	.fstep { font-size: 11px; opacity: 0.7; }
	.fnav { display: flex; gap: 6px; }
	.fbtn {
		background: transparent; color: inherit; border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
		border-radius: 6px; padding: 4px 10px; font-size: 12px; cursor: pointer;
	}
	.fbtn:hover { border-color: currentColor; }
	.fsubmit { padding: 6px 14px; }
	.fsubmit:disabled { opacity: 0.5; cursor: default; }
	.fempty { font-size: 12px; opacity: 0.7; margin: 0; }
	.ferror { font-size: 12px; margin: 0; opacity: 0.9; }
	.fnote { font-size: 11px; opacity: 0.8; margin: 0; }
	.fwait { font-size: 11px; opacity: 0.7; margin: 0; }
	.ftabs { display: flex; gap: 4px; flex-wrap: wrap; }
	.ftab {
		background: transparent; color: inherit; border: 1px solid color-mix(in srgb, currentColor 20%, transparent);
		border-radius: 6px; padding: 3px 9px; font-size: 11px; cursor: pointer;
	}
	.ftab.is-on { border-color: currentColor; font-weight: 700; }
	.fbody { min-height: 0; overflow: auto; }
	.fform { display: flex; flex-direction: column; gap: 10px; }
	.ffield { display: flex; flex-direction: column; gap: 3px; }
	.flabel { font-size: 11px; opacity: 0.85; }
	.finput {
		background: transparent; color: inherit; border: 1px solid color-mix(in srgb, currentColor 25%, transparent);
		border-radius: 6px; padding: 6px 8px; font-size: 13px;
	}
	.fauto { font-size: 10px; opacity: 0.6; }
	.facs { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
	.fac {
		background: transparent; color: inherit; border: 1px solid color-mix(in srgb, currentColor 15%, transparent);
		border-radius: 4px; padding: 4px 8px; font-size: 12px; text-align: left; cursor: pointer; width: 100%;
	}
	.flist { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }
	.frow {
		background: transparent; color: inherit; border: 1px solid color-mix(in srgb, currentColor 18%, transparent);
		border-radius: 6px; padding: 6px 10px; font-size: 12px; text-align: left; cursor: pointer; width: 100%;
		display: flex; flex-wrap: wrap; gap: 6px; align-items: baseline;
	}
	.frow:hover { border-color: currentColor; }
	.fk { font-size: 10px; opacity: 0.65; text-transform: uppercase; letter-spacing: 0.05em; }
	.fv { font-size: 12px; }
	.fdetail { margin: 0; display: flex; flex-direction: column; gap: 4px; }
	.fdetail-line { display: flex; gap: 8px; align-items: baseline; }
</style>
