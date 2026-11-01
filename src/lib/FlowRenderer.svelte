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
		isNavigationAction,
		loadFlowPins,
		saveFlowPin,
		type ConnectorEntry,
		type FetchResultDTO,
		type FlowDTO,
		type ScreenDTO,
		type ConditionDTO,
		type AutocompleteRuleDTO,
		type ScreenActionDTO
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
	// SF2: Multi-Select-Markierungen (Index-Keys der sichtbaren Zeilen) und
	// Action-Zustand (Busy/Note pro Screen).
	let marked: string[] = $state([]);
	let actionBusy = $state(false);
	let actionNote = $state('');

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
	// SF2: Write-Action eines Multi-Screens = Confirm-Button.
	const confirmAction = $derived(
		shown && shown.type === 'list' && shown.multi
			? (shown.actions ?? []).find((a) => !isNavigationAction(a)) ?? null
			: null
	);
	const screenActions = $derived.by(() => {
		const s = shown;
		if (!s) return [];
		const list = s.actions ?? [];
		return confirmAction ? list.filter((a) => a.id !== confirmAction.id) : list;
	});

	// SF2: code_entry-Screens rendern ihre `params` als Eingabefelder
	// (Session-Code), form-Screens ihre `form.fields`.
	const formFields = $derived(
		shown?.type === 'code_entry'
			? (shown.params ?? []).map((p) => ({
					key: p.key,
					label: p.label,
					type: 'text' as const,
					required: true
				}))
			: (shown?.form?.fields ?? [])
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
		marked = [];
		actionNote = '';
		autofilled = {};
		acItems = {};
		acOpen = {};
		// SF2: Form-Screens beim Navigieren aus dem Flow-Zustand vorseeden
		// (Pins + Selektionen) — nur für Template-/Param-Platzhalter und NUR
		// wenn der User nicht schon getippt hat. Notwendig, weil required-
		// Felder (code/sid) sonst die HTML5-Validation blocken, bevor das
		// Submit-Seeding greift.
		const t = findScreen(id);
		if (t?.type === 'form' && t.form) {
			seedFromState(t, formValues);
		}
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
	/** `{key}`-Platzhalter aus einem Template-Wert extrahieren (SF2). */
	function templateKeys(v: unknown, out: Set<string>) {
		if (typeof v === 'string') {
			for (const m of v.matchAll(/\{([A-Za-z_][A-Za-z0-9_]*)\}/g)) out.add(m[1]);
		} else if (Array.isArray(v)) {
			for (const x of v) templateKeys(x, out);
		} else if (v && typeof v === 'object') {
			for (const x of Object.values(v as Record<string, unknown>)) templateKeys(x, out);
		}
	}

	/**
	 * SF2: Seed-Keys eines Form-Screens — Template-/Pfad-Platzhalter +
	 * Param-Keys + Header-Träger-Felder (z. B. `code` → X-SCLM-Code). Der
	 * Header-Träger ist sonst in keinem Template sichtbar und bliebe leer —
	 * required-HTML5-Validation würde den Submit blocken.
	 */
	function seedKeys(s: ScreenDTO): Set<string> {
		const keys = new Set<string>();
		if (s.form) {
			templateKeys(s.form.body_template, keys);
			templateKeys(s.form.path, keys);
		}
		for (const p of s.params ?? []) keys.add(p.key);
		for (const f of s.form?.fields ?? []) {
			if (f.header && f.header.trim()) keys.add(f.key);
		}
		return keys;
	}

	/** Fehlende Seed-Keys aus dem Flow-Zustand vorseeden (User-Input gewinnt). */
	function seedFromState(s: ScreenDTO, target: Record<string, string>) {
		for (const k of seedKeys(s)) {
			if (k in target) continue;
			const v = fstate[k];
			if (typeof v === 'string' && v.trim()) target[k] = v;
			else if (typeof v === 'number') target[k] = String(v);
		}
	}

	// ---- Formular: Submit → Transition -------------------------------------
	// SF2: Formular-Werte werden vor dem Submit aus dem Flow-Zustand
	// vorgefüllt — NUR für Platzhalter-Keys des Templates/Pfads und NUR wenn
	// das Feld nicht vom User gefüllt wurde (user input gewinnt). So fließen
	// gepinnte/Selektions-Werte (code, sid, loot_id) in den Body, ohne dass
	// das Manifest sie als Formular-Felder deklarieren muss.
	async function submitForm() {
		const s = shown;
		if (!s) return;
		// SF2: code_entry — eingegebene Werte in den Flow-Zustand, laden,
		// bei Erfolg zur on_submit-Transition.
		if (s.type === 'code_entry') {
			for (const [k, v] of Object.entries(formValues)) {
				if (typeof v === 'string' && v.trim()) fstate[k] = v;
			}
			await load(true);
			if (!res.error) {
				// Erfolgreicher Code-Load: deklarierte Pins sichern (code/sid…).
				for (const k of s.pins ?? []) {
					const v = fstate[k];
					if (typeof v === 'string') pin(k, v);
					else if (typeof v === 'number') pin(k, String(v));
				}
				if (s.on_submit) go(s.on_submit);
			}
			return;
		}
		if (!s.form) return;
		const seeded: Record<string, string> = { ...formValues };
		seedFromState(s, seeded);
		formBusy = true;
		try {
			const out = await invoke<{ status: number; body: unknown }>('connector_form_submit', {
				id: entry.id,
				formId: s.id,
				values: seeded
			});
			formNote =
				out.status < 400
					? tr('connector.form_ok', { status: String(out.status) })
					: tr('connector.form_http_error', { status: String(out.status) });
			// Eingereichte Werte + Response-Werte in den Zustand (und Pins).
			for (const [k, v] of Object.entries(seeded)) {
				if (!(k in fstate)) fstate[k] = v;
			}
			if (out.body && typeof out.body === 'object') {
				const body = out.body as Record<string, unknown>;
				for (const [k, v] of Object.entries(body)) {
					fstate[k] = v;
					if ((s.pins ?? []).includes(k) && typeof v === 'string') pin(k, v);
				}
				// SF2: Form-Mapping (Zustands-Key → Dot-Path in die Antwort) —
				// z. B. add_participant: {"participant_id": "id"}.
				for (const [k, path] of Object.entries(s.mapping ?? {})) {
					const v = dotGet(body, path);
					if (typeof v === 'string' || typeof v === 'number') {
						fstate[k] = v;
						if ((s.pins ?? []).includes(k)) pin(k, String(v));
					}
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

	// ---- SF2: Multi-Select-Listen ------------------------------------------
	// Zeilen sind markierbar (Checkboxen); der Confirm pinnt die Auswahl als
	// JSON-Array unter den Pin-Key (z. B. loot_id) und führt die Write-Action
	// des Screens aus. max_path begrenzt die Markierungen (z. B. max_wishes).
	const multiMax = $derived.by(() => {
		const mp = shown?.multi?.max_path;
		if (!mp) return null;
		const v = fstate[mp];
		return typeof v === 'number' ? v : typeof v === 'string' ? Number(v) || null : null;
	});

	function rowKey(i: number): string {
		return String(i);
	}

	function isMarked(i: number): boolean {
		return marked.includes(rowKey(i));
	}

	function toggleMark(i: number) {
		const k = rowKey(i);
		if (marked.includes(k)) {
			marked = marked.filter((x) => x !== k);
		} else if (multiMax === null || marked.length < multiMax) {
			marked = [...marked, k];
		}
		actionNote = '';
	}

	/** Alle primitiven Flow-Zustandswerte als Action-Bindings (Arrays inklusive
	 *  — Multi-Select-Auswahl bleibt als JSON-Liste strukturiert). */
	function actionValues(): Record<string, unknown> {
		const out: Record<string, unknown> = {};
		for (const [k, v] of Object.entries(fstate)) {
			if (
				typeof v === 'string' ||
				typeof v === 'number' ||
				typeof v === 'boolean' ||
				Array.isArray(v)
			)
				out[k] = v;
		}
		for (const [k, v] of Object.entries(formValues)) {
			if (v.trim()) out[k] = v;
		}
		return out;
	}

	/** Dot-Path in ein Antwortobjekt (pins_map: "pcode" → body.pcode). */
	function dotGet(obj: unknown, path: string): unknown {
		let cur: unknown = obj;
		for (const seg of path.split('.')) {
			if (cur && typeof cur === 'object') cur = (cur as Record<string, unknown>)[seg];
			else return undefined;
		}
		return cur;
	}

	async function runAction(a: ScreenActionDTO) {
		const s = shown;
		if (!s || actionBusy) return;
		if (isNavigationAction(a)) {
			if (a.goto) go(a.goto);
			return;
		}
		actionBusy = true;
		actionNote = '';
		try {
			const out = await invoke<{ status: number; body: unknown }>('connector_screen_action', {
				id: entry.id,
				screenId: s.id,
				actionId: a.id,
				values: actionValues()
			});
			actionNote =
				out.status < 400
					? tr('connector.form_ok', { status: String(out.status) })
					: tr('connector.form_http_error', { status: String(out.status) });
			if (out.status < 400) {
				// pins_map: Antwortfelder in den Flow-Zustand pinnen (z. B. pcode).
				for (const [key, path] of Object.entries(a.pins_map ?? {})) {
					const v = dotGet(out.body, path);
					if (typeof v === 'string') pin(key, v);
					else if (typeof v === 'number') pin(key, String(v));
				}
				marked = [];
				if (a.goto) go(a.goto);
				else await load(true); // ohne goto: Screen refreshen (Status sichtbar)
			}
		} catch (err) {
			actionNote = trError(err);
		} finally {
			actionBusy = false;
		}
	}

	/** Multi-Confirm: Auswahl pinnen (JSON-Array) → Write-Action ausführen. */
	async function confirmMulti() {
		const s = shown;
		if (!s) return;
		const write = (s.actions ?? []).find((a) => !isNavigationAction(a));
		const selected = rows.filter((_, i) => isMarked(i));
		if (selected.length === 0) return;
		if (multiMax !== null && selected.length > multiMax) return;
		// Pin-Keys erhalten die Auswahl als JSON-Array (apply_fill ersetzt den
		// Whole-Platzhalter strukturiert — loot_ids bleibt eine Liste).
		for (const k of s.pins ?? []) {
			const vals = selected
				.map((r) => r[k])
				.filter((v) => typeof v === 'string' || typeof v === 'number')
				.map((v) => String(v));
			fstate = { ...fstate, [k]: vals };
		}
		if (write) await runAction(write);
		else if (s.on_select) go(s.on_select);
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
				<form class="fform" onsubmit={(e) => { e.preventDefault(); void submitForm(); }}>
					{#each formFields as f (f.key)}
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
			{:else if shown.type === 'list' || (shown.type === 'results' && Array.isArray(res.data?.mapped))}
				{#if shown.type === 'list' && shown.multi}
					<p class="fmarkinfo">
						{tr('connector.flow.marked', {
							n: String(marked.length),
							max: String(multiMax ?? '∞')
						})}
					</p>
				{/if}
				<ul class="flist">
					{#each rows as row, i (i)}
						<li>
							{#if shown.type === 'list' && shown.multi}
								<button
									class="frow fmark"
									class:is-marked={isMarked(i)}
									type="button"
									aria-pressed={isMarked(i)}
									onclick={() => toggleMark(i)}
								>
									<span class="fcheck">{isMarked(i) ? '☑' : '☐'}</span>
									{#each Object.entries(shown.mapping ?? {}) as [field], j (field)}
										<span class="fk">{field}</span><span class="fv">{formatValue(row?.[field])}</span>
									{/each}
								</button>
							{:else}
								<button
									class="frow"
									type="button"
									onclick={() => select(String(row.id ?? i), row)}
								>
									{#each Object.entries(shown.mapping ?? {}) as [field], j (field)}
										<span class="fk">{field}</span><span class="fv">{formatValue(row?.[field])}</span>
									{/each}
								</button>
							{/if}
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
			{#if (confirmAction || screenActions.length > 0)}
				<div class="factions">
					{#if confirmAction}
						<button
							class="fbtn fsubmit"
							type="button"
							disabled={actionBusy || marked.length === 0}
							onclick={() => void confirmMulti()}
						>
							{labelOf(confirmAction.title, lang)}
						</button>
					{/if}
					{#each screenActions as a (a.id)}
						{#if isNavigationAction(a)}
							<button class="fbtn" type="button" onclick={() => void runAction(a)}>
								{labelOf(a.title, lang)}
							</button>
						{:else}
							<button
								class="fbtn fsubmit"
								type="button"
								disabled={actionBusy}
								onclick={() => void runAction(a)}
							>
								{labelOf(a.title, lang)}
							</button>
						{/if}
					{/each}
				</div>
				{#if actionNote}<p class="fnote">{actionNote}</p>{/if}
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
	.fmarkinfo { font-size: 11px; opacity: 0.75; margin: 0; }
	.fmark.is-marked { border-color: currentColor; font-weight: 700; }
	.fcheck { font-size: 13px; }
	.factions { display: flex; gap: 6px; flex-wrap: wrap; align-items: center; }
</style>
