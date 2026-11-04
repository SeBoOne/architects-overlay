<script lang="ts">
	// FlowRenderer — generischer Screen-Flow-Renderer (SF1, t_c35bc233).
	//
	// Rendert deklarative Screen-Ketten aus dem Manifest (`flows`) mit FESTER
	// Overlay-Optik: Listen/Buttons wie Trade-Route-/Suchergebnis-Listen
	// (Theme-Variablen, keine neuen Farben), keine Fremdstyling-Welt.
	// KEIN connector-spezifisches Wissen: alles kommt aus dem Flow-Schema
	// (Screens: code_entry | list | detail | form | wait | results,
	// conditions, transitions, poll, autocomplete).
	//
	// D-B (t_159ebc5c, Sebo-Feedback 3–11):
	// - Punkt 5: genau EINE Überschrift pro Ansicht = Screen-Titel
	//   (Flow-Titel und Zurück/Esc-Buttons entfallen).
	// - Punkt 6+9: keine Zurück-/Esc-Buttons; Navigation voran nur über
	//   Manifest-Actions, zurück nur Esc (Kaskade in ConnectorViews).
	// - Punkt 4: Pin-/Plumbing-Mapping-Keys (pins ∪ Action-params/-templates)
	//   werden in Flow-Listen nicht als Spalten gerendert.
	// - Punkt 8: Warteseite neu: zentrierte Ladeanimation + „Warten auf die
	//   Verlosung…“ + Wunschliste aus state-Key `wished` (fehlt → ausblenden);
	//   alle Kopier-Buttons im Flow ersatzlos entfernt.
	// - Punkt 11: results-Screen gruppiert gleichnamige Items (Name ×Menge),
	//   rid/waived/collected bleiben nur als Zustand für enabled_if sichtbarlos.
	// - Punkt 1 (Client): Frisch-Fetch beim Betreten (force), Auto-Refresh aus
	//   poll_seconds (list-Screens Default 60), dezenter ↻-Button auf Listen.
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
	// E-SCLM-UX v4 (t_102e220d, Sebo-Feedback-Runde 2):
	// - Punkt 1: rid aus normalisierten Ergebnis-Skalaren (nie aus
	//   Selection-Pins) — Rust zipt results-Screens zu Zeilenarrays, der
	//   Index-Spread ließ rid/waived/collected nie in fstate landen.
	// - Punkt 2: One-Way-Status — ist ein enabled_if-Flag gesetzt, ersetzt
	//   ein Statusfeld BEIDE Write-Buttons (kein collected↔waived-Weg).
	// - Punkt 3+4: results-Zweig greift wieder (der List-Zweig fängt
	//   results nicht mehr ab) und zeigt pro Gruppe nur Name (+ ×N).
	// - Punkt 5: list-Hauptspalte = name/title (Rust-BTreeMap sortiert
	//   Mapping-Keys alphabetisch — sonst wäre z. B. grade prominent).
	// - Punkt 6: nach einer erfolgreichen Write-Aktion wird der
	//   Flow-Zustand per connector_flow_open aufgefrischt (state.wished
	//   steht sofort auf der Warteseite).
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
		actionEnabled,
		dotPath,
		normalizeResults,
		groupNames,
		loadFlowPins,
		saveFlowPin,
		type ConnectorEntry,
		type FetchResultDTO,
		type FlowDTO,
		type FlowOpenOutcomeDTO,
		type ScreenDTO,
		type AutocompleteRuleDTO,
		type ScreenActionDTO
	} from './connector';

	type Result = { data: FetchResultDTO | null; error: string; loading: boolean };

	let {
		entry,
		flow,
		initial = null,
		/** Vorgemergter Flow-Zustand (SF2-R1 D1: gemappte code_entry-Daten des
		 *  Suchleisten-Prefetch — max_wishes/raffle_done landen im fstate,
		 *  ohne sie persistieren zu müssen). */
		initialState = null
	}: {
		entry: ConnectorEntry;
		flow: FlowDTO;
		/** Start-Screen-ID (z. B. Wiedereinstieg direkt beim Ergebnis-Screen). */
		initial?: string | null;
		initialState?: Record<string, unknown> | null;
	} = $props();

	const lang = $derived(currentLang());

	function findScreen(id: string): ScreenDTO | undefined {
		return flow.screens.find((s) => s.id === id);
	}

	// ---- Flow-Zustand -----------------------------------------------------
	// Bewusst nur der Initialwert: der Einstiegs-Screen wird einmal beim
	// Mount bestimmt (untrack — spätere Prop-Änderungen ändern den Start nicht).
	// D-A-Vertrag: der Start-Screen kommt serverseitig entschieden an
	// (Resume auf progress_screen), der Renderer rät nichts dazu.
	const startId: string = untrack(() => (initial && findScreen(initial) ? initial : flow.entry));
	let screenId: string = $state(startId);
	let current: ScreenDTO | null = $derived(findScreen(screenId) ?? null);
	let fstate: Record<string, unknown> = $state({
		...(untrack(() => initialState) ?? {})
	}); // gemappte Daten + Pins (initialState bewusst nur beim Mount)
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
	// SF2-R1 D3: Action-Eingabefelder (z. B. pcode-Claim beim ersten
	// Wishes-Save) — Werte fließen als {key}-Binding in die Action.
	let actionInputs: Record<string, string> = $state({});
	// SCLM v2 Karte C (Requirement 9): enabled_if — one-way-Flags
	// (collected/waived) deaktivieren die zugehörige Aktion.
	function actionIsEnabled(a: ScreenActionDTO): boolean {
		return actionEnabled(a, fstate);
	}

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
	// SCLM v2 Karte C (Spec Abschnitt 3): result_for liefert 200 mit
	// {result: null} → „Ergebnis noch offen“ — siehe resultsPending unten
	// (liefert als leerer Zeilenkern, E-SCLM-UX v4 Punkt 3).
	// D-B Punkt 4 (generisch): Mapping-Keys, die nur Pin-/Plumbing-Zwecke
	// sind — Union aus Screen-pins, keystore_fields-Quellen, Action-params
	// und pins_map-Keys. Bewusst KEINE Template-Platzhalter (body/path/
	// header): die referenzieren oft Mensch-Spalten (z. B. rename-Template
	// {name} in c_participants, {scu} in commodities) — deren Aufnahme in
	// die Union würde menschliche Spalten verstecken (Review R1, Punkt 4).
	// Whitelist-Logik: Keys, die NICHT in dieser Union liegen, bleiben sichtbar.
	const hiddenMappingKeys = $derived.by(() => {
		const s = shown;
		const hidden = new Set<string>();
		if (!s) return hidden;
		for (const k of s.pins ?? []) hidden.add(k);
		for (const src of Object.values(s.keystore_fields ?? {})) hidden.add(src);
		for (const a of s.actions ?? []) {
			for (const p of a.params ?? []) hidden.add(p.key);
			for (const k of Object.keys(a.pins_map ?? {})) hidden.add(k);
		}
		return hidden;
	});
	/** Sichtbare Spalten einer Flow-Liste = Mapping-Reihenfolge minus Plumbing. */
	// E-SCLM-UX v4 Punkt 5: Rust serialisiert Mapping-Keys aus einem
	// BTreeMap (alphabetisch) — die Manifest-Reihenfolge geht auf dem Weg
	// verloren. Menschliche Hauptspalte ist name/title; die Reste bleiben
	// dezente Meta-Paare daneben/unterhalb.
	const listColumns = $derived.by(() => {
		const cols = Object.keys(shown?.mapping ?? {}).filter((k) => !hiddenMappingKeys.has(k));
		const main = cols.findIndex((k) => /^(name|title)$/i.test(k));
		return main > 0 ? [cols[main], ...cols.filter((_, i) => i !== main)] : cols;
	});
	// E-SCLM-UX v4 Punkt 1/3/4: normalisierte Ergebnisse (scalars für
	// rid/waived/collected im Zustand, names für die Gewinn-Gruppen).
	const resultsNorm = $derived.by(() => {
		if (shown?.type !== 'results' || !res.data) return null;
		return normalizeResults(res.data.mapped, shown.mapping ?? {});
	});
	// Spec Abschnitt 3: 200 {result: null} → map_view liefert ein LEERES
	// Zeilenarray (results-Screens sind Table-Views) — nicht nur das
	// Object-Szenario abfangen; leerer Zeilenkern = „Ergebnis noch offen“.
	const resultsPending = $derived(
		shown?.type === 'results' && res.data !== null && (resultsNorm?.rowCount ?? 0) === 0
	);
	// E-SCLM-UX v4 Punkt 4 (war D-B Punkt 11): Gewinnübersicht gruppiert
	// gleichnamige Items (Name ×Menge, Reihenfolge des ersten Auftretens).
	// Quelle ist resultsNorm.names — Namen, nie Flags/Skalare; rid/waived/
	// collected bleiben zustandslos sichtbarlos für enabled_if/Action-rid.
	const resultGroups = $derived(resultGroupsSource());
	function resultGroupsSource(): { name: string; count: number }[] {
		return resultsNorm ? groupNames(resultsNorm.names) : [];
	}
	// E-SCLM-UX v4 Punkt 2: One-Way-Status — ist ein enabled_if-Flag
	// (z. B. collected/waived) wahrhaftig gesetzt (Bool-Toleranz wie
	// evaluateCondition: true/1/'1'/'true'), ersetzen wir beide
	// Write-Buttons durch EINEN Status; der jeweils andere Button ist dann
	// nicht mehr darstellbar, geschweige denn clickable. Ersteller-Reset
	// → frischer Fetch → Flag wieder falsch → Buttons erscheinen von selbst.
	const oneWayFlag = $derived.by(() => {
		const s = shown;
		if (!s) return null;
		for (const a of s.actions ?? []) {
			if (isNavigationAction(a) || !a.enabled_if) continue;
			let v: unknown;
			try {
				v = dotPath(fstate, a.enabled_if.on);
			} catch {
				continue;
			}
			if (v === true || v === 1 || v === '1' || v === 'true') return a.enabled_if.on;
		}
		return null;
	});
	const oneWayLabel = $derived.by(() => {
		if (!oneWayFlag) return '';
		// Text-Key pro Flag (connector.flow.status_collected / _waived …);
		// fehlt er (Manifest mit unbekanntem Flag), auf Action-Titel fallen.
		const key = `connector.flow.status_${oneWayFlag}`;
		const txt = tr(key);
		if (txt && txt !== key) return txt;
		const a = (shown?.actions ?? []).find((x) => !isNavigationAction(x) && x.enabled_if?.on === oneWayFlag);
		return a ? labelOf(a.title, lang) : txt;
	});
	// D-B Punkt 8: Wunschliste der Warteseite — D-A injiziert state-Key
	// `wished` (string[] loot-Namen). Fehlt der Key (oder ist leer) → die
	// Sektion wird ausgeblendet (D-A läuft parallel, Kontrakt sauber behandelt).
	const wishedList = $derived.by(() => {
		const w = fstate.wished;
		if (!Array.isArray(w)) return [] as string[];
		return w.filter((x) => typeof x === 'string' || typeof x === 'number').map(String);
	});
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

	// Action-Felder aus dem Flow-Zustand vorseeden (gepinnter pcode etc.) —
	// User-Input gewinnt (nur leere Felder werden gefüllt).
	$effect(() => {
		const s = shown;
		if (!s) return;
		const seed: Record<string, string> = {};
		for (const a of s.actions ?? []) {
			for (const f of a.fields ?? []) {
				const k = `${a.id}.${f.key}`;
				if (actionInputs[k] !== undefined) continue;
				const v = fstate[f.key];
				if (typeof v === 'string' && v.trim()) seed[k] = v;
				else if (typeof v === 'number') seed[k] = String(v);
			}
		}
		if (Object.keys(seed).length > 0) actionInputs = { ...actionInputs, ...seed };
	});

	// ---- Daten laden (generisch: Fetch + Mapping via Backend) --------------
	// D-B Punkt 1: force=true beim Betreten (Frische bei Screen-Eintritt;
	// Poll-Intervalle laden mit force=false, der Backend-Cache greift nur dort).
	async function load(force = false) {
		const s = shown;
		if (!isTauri || !s || s.type === 'form' || !s.endpoint) return;
		const missing = (s.params ?? []).find((p) => !(String(fstate[p.key] ?? '')).trim());
		if (missing) {
			// SCLM v2 Karte C (Requirement 1): Flows mit open-Block haben
			// keinen manuellen Code-Eingabe-Screen mehr — ohne vorgemergten
			// Zustand (flow_open) gehört der Code in die Suchzeile.
			res = {
				data: null,
				error: flow.open
					? tr('connector.flow.open_via_search')
					: tr('error.connector.param_required', { key: labelOf(missing.label, lang) }),
				loading: false
			};
			return;
		}
		res = { data: null, error: '', loading: true };
		try {
			const data = await invoke<FetchResultDTO>('connector_fetch', {
				id: entry.id,
				// SF3: Flow-qualifizierte Screen-ID (`<flow>:<screen>`) —
				// Screen-IDs dürfen sich zwischen Flows wiederholen, der
				// Cache-Key/Resolve braucht die Flow-Qualifikation.
				viewId: `${flow.id}:${s.id}`,
				force,
				params: bindings(s)
			});
			res = { data, error: '', loading: false };
			// Mapped-Daten in den Flow-Zustand (Conditions sehen den letzten Fetch).
			// E-SCLM-UX v4 Punkt 1: results-Screens mapped der Rust-Kern als
			// ZEILENARRAY (Table-Zip) — ein Object-Spread über ein Array
			// verteilt nur Indizes 0..n und rid/waived/collected würden den
			// Zustand nie erreichen (Ursache der rid-Fehlermeldung). Für
			// results-Screens daher normalisiert: nur die Skalaren.
			if (data.mapped && typeof data.mapped === 'object') {
				if (s.type === 'results') {
					fstate = { ...fstate, ...normalizeResults(data.mapped, s.mapping ?? {}).scalars };
				} else {
					fstate = { ...fstate, ...(data.mapped as Record<string, unknown>) };
				}
			}
			// Exit-/Branch-Bedingung (SF2-R1 D2, generisch): Jeder Screen mit
			// `exit` verzweigt nach dem Load — wait pollt bis raffle_done,
			// code_entry leitet bei bereits durchgezogener Verlosung direkt
			// zu results (Wiedereinstieg → direkt Gewinne).
			if (s.exit) {
				const target = branchTarget(s.exit, fstate);
				if (target && target !== s.id) {
					go(target);
					return;
				}
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

	// ---- Navigation (D-B Punkt 6+9: voran nur über Manifest-Actions) ------
	// Eine Zurück-Navigation im Renderer gibt es nicht mehr: zurück geht es
	// ausschließlich per Esc (Kaskade in ConnectorViews → Landliste).
	function go(id: string) {
		if (id === screenId) return;
		if (!findScreen(id)) return; // unbekanntes Ziel: stillschweigend ignorieren? Nein — bewusst loggen.
		screenId = id;
		formValues = {};
		formNote = '';
		marked = [];
		actionNote = '';
		actionInputs = {};
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
		// Punkt 1: Frisch-Fetch beim Screen-Wechsel erledigt der $effect
		// (trackt `shown`, ruft load(true) bei jeder Screen-Änderung) —
		// ein zweiter load hier wäre ein Doppel-Request pro Transition
		// (Review R1, Finding 1).
	}

	// ---- Poll-Regeln -------------------------------------------------------
	// wait: bestehender poll_seconds + exit-Bedingung (unverändert).
	// results mit poll_seconds: pollen weiter (null-Pfad „Ergebnis noch offen“
	// → wait-Verhalten, SCLM v2 Karte C Requirement 7).
	// D-B Punkt 1: list-Screens pollen zusätzlich nach poll_seconds aus dem
	// Manifest (Default 60, wenn null); floor 15 s (API-Empfehlung: kein Burst).
	function schedulePolling() {
		if (timer) clearInterval(timer);
		timer = null;
		const s = shown;
		if (!s || !isTauri) return;
		let secs: number | null = null;
		let force = false;
		if (s.type === 'wait') secs = Math.max(s.poll_seconds ?? 15, 5);
		else if (s.type === 'results' && s.poll_seconds) secs = Math.max(s.poll_seconds, 10);
		else if (s.type === 'list' && s.endpoint) {
			secs = Math.max(s.poll_seconds ?? 60, 15);
			// Punkt 1: Listen-Auffrischung ist Intention des Intervalls —
			// force=true macht sie unabhängig vom Backend-TTL (D-A läuft
			// parallel und cache-bypassed Flow-Screens ohnehin).
			force = true;
		}
		if (secs === null) return;
		timer = setInterval(() => void load(force), secs * 1000);
	}
	$effect(() => {
		// Punkt 1: Frisch-Fetch beim Mount; Timer-Cleanup bei Screen-Wechsel
		// (kein Intervall-Leak — pro Screen wird neu schedulePolling't).
		untrack(() => void load(true));
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
			const before = screenId;
			await load(true);
			if (res.error) {
				// SF3: Fehlgeschlagener Code-Load (404/403/fehlender Param) als
				// Note unterm Formular — der Fehler-Branch rendert bei code_entry
				// nicht (Formular bleibt bedienbar).
				formNote = res.error;
			} else {
				// Erfolgreicher Code-Load: deklarierte Pins sichern (code/sid…).
				for (const k of s.pins ?? []) {
					const v = fstate[k];
					if (typeof v === 'string') pin(k, v);
					else if (typeof v === 'number') pin(k, String(v));
				}
				// SF2-R1 D2: load() hat bei erfüllter exit-Bedingung (z. B.
				// raffle_done → results) bereits transitioniert — dann nicht
				// mit on_submit überschreiben.
				if (screenId === before && s.on_submit) go(s.on_submit);
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
				formId: `${flow.id}:${s.id}`,
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
		// SF2-R1 D3: Action-Eingabefelder (User-Input gewinnt über fstate).
		for (const [k, v] of Object.entries(actionInputs)) {
			const key = k.slice(k.indexOf('.') + 1);
			if (v.trim()) out[key] = v;
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
		// enabled_if (one-way-Flags): deaktivierte Aktionen laufen nicht.
		if (!actionIsEnabled(a)) return;
		// SF2-R1 D3: Pflicht-Action-Felder (z. B. pcode-Claim) vor dem Write
		// prüfen — fehlende Werte laufen sonst als 403/400 beim Server an.
		for (const f of a.fields ?? []) {
			if (f.required === false) continue;
			const v = String(actionValues()[f.key] ?? '').trim();
			if (!v) {
				actionNote = tr('error.connector.param_required', { key: labelOf(f.label, lang) });
				return;
			}
		}
		actionBusy = true;
		actionNote = '';
		try {
			const out = await invoke<{ status: number; body: unknown }>('connector_screen_action', {
				id: entry.id,
				screenId: `${flow.id}:${s.id}`,
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
				// SF2-R1 D3: eingegebene Action-Felder sind Session-Pins (z. B.
				// pcode-Claim — der erste Save bindet den Code, spätere Saves/
				// results brauchen ihn nicht mehr einzutippen).
				for (const f of a.fields ?? []) {
					const v = String(actionValues()[f.key] ?? '').trim();
					if (v) pin(f.key, v);
				}
				// E-SCLM-UX v4 Punkt 6: Flow-Zustand nach der Write-Kette
				// auffrischen — connector_flow_open mit dem gespeicherten
				// Code (flow.open.param, z. B. 'code') erneut ausführen.
				// Rust ist idempotent (bereits beigetreten → Claim-Weg ohne
				// Doppel-POST) und liefert das frische FlowOpenOutcome inkl.
				// state.wished — die Warteseite zeigt die Wünsche SOFORT,
				// ohne dass der User den Flow erneut öffnen muss.
				if (flow.open) {
					const openKey = flow.open.param || 'code';
					const openCode = String(fstate[openKey] ?? pins[openKey] ?? '').trim();
					if (openCode) {
						try {
							const ref = await invoke<FlowOpenOutcomeDTO>('connector_flow_open', {
								id: entry.id,
								flowId: flow.id, // camelCase wie beim Erst-Open (Rust-Parameter flow_id)
								code: openCode
							});
							fstate = { ...fstate, ...(ref.state ?? {}) };
						} catch {
							/* Best-effort: die Write-Aktion selbst ist bereits
							   erfolgreich — ein fehlendes State-Refresh darf
							   den Nutzer nicht mit einer Fehlernote quittieren. */
						}
					}
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

	// SF2-R1 D3: Actions mit Eingabefeldern (z. B. pcode-Claim) — D-B Punkt 7:
	// als eigene, ruhig formatierte Zeile UNTERHALB der Actions rendern
	// (moderate Breite, dezent), nicht neben/klemmend in den Buttons.
	const fieldActions = $derived.by(() => {
		const s = shown;
		if (!s) return [];
		return (s.actions ?? []).filter((a) => (a.fields?.length ?? 0) > 0 && actionIsEnabled(a));
	});

	onDestroy(() => {
		if (timer) clearInterval(timer);
	});
</script>

<section class="flow">
	<!-- D-B Punkt 5: genau EINE Überschrift = Screen-Titel. Flow-Titel und
	     Zurück-/Esc-Buttons sind entfallen (Punkt 6+9); der own-Mode-Rail-Name
	     lebt nur im Rail-Menü des Overlays. -->
	{#if shown}
		<header class="fhead">
			<span class="fscreentitle">{labelOf(shown.title, lang)}</span>
			<span class="fstep">
				{tr('connector.flow.step', {
					n: String(flow.screens.findIndex((x) => x.id === shown.id) + 1),
					total: String(flow.screens.length)
				})}
			</span>
		</header>
	{/if}

	{#if shown === null}
		<p class="fempty">{tr('error.connector.flow_screen_missing')}</p>
	{:else}
		<!-- SCLM v2 Karte C (Requirement 9): Flows mit open-Block sind
		     strikt linear — Flow-Schritte sind KEINE Tabs. Legacy-Flows ohne
		     open-Block (z. B. creator) behalten ihre Screen-Navigation, jetzt
		     in Chip-Reiter-Optik (Requirement 10: keine Pill-Tabs). -->
		{#if !flow.open}
			<div class="ftabs" role="tablist">
				{#each flow.screens as s (s.id)}
					<button
						type="button"
						role="tab"
						aria-selected={s.id === shown.id}
						class="chip"
						class:is-on={s.id === shown.id}
						onclick={() => go(s.id)}
					>{labelOf(s.title, lang)}</button>
				{/each}
			</div>
		{/if}

		<div class="fbody">
			{#if res.loading && shown.type !== 'code_entry' && shown.type !== 'form'}
				<p class="fempty">…</p>
			{:else if shown.type === 'code_entry' || shown.type === 'form'}
				<!-- SF3: Form/Code-Entry zeigen ihr Formular IMMER — auch wenn ein
				     vorheriger Fetch fehlschlug (fehlender Param). Sonst wäre der
				     Code-Screen über den Tab-Einstieg unbenutzbar (nur Fehltext,
				     kein Eingabefeld). -->
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
					<button class="btn fsubmit" type="submit" disabled={formBusy}>
						{shown.type === 'code_entry' ? tr('connector.flow.code_enter') : $t('connector.form_submit')}
					</button>
					{#if formNote}<p class="fnote">{formNote}</p>{/if}
				</form>
			{:else if res.error}
				<p class="ferror">{res.error}</p>
			{:else if shown.type === 'wait'}
				<!-- D-B Punkt 8: Warteseite neu — keine Infozeilen, keine
				     Kopier-Buttons. Zentrierte CSS-Ladeanimation (Theme-Farben,
				     keyframes im scoped <style>) + „Warten auf die Verlosung…“.
				     Darunter die Wunschliste aus state-Key `wished` (D-A-Vertrag:
				     string[] loot-Namen; fehlt der Key → Sektion ausblenden). -->
				<div class="fwaitwrap">
					<span class="fspinner" aria-hidden="true"></span>
					<p class="fwaittext">{tr('connector.flow.waiting')}</p>
				</div>
				{#if wishedList.length > 0}
					<ul class="flist" aria-label={tr('connector.flow.wished_list')}>
						{#each wishedList as item, i (i)}
							<li>
								<div class="frow frow-static">
									<span class="fmain"><span class="ftitle">{item}</span></span>
								</div>
							</li>
						{:else}
							<li class="fempty">{$t('connector.empty')}</li>
						{/each}
					</ul>
				{/if}
			{:else if shown.type === 'list'}
				<!-- D-B Punkt 1+3+4: Listen im Overlay-Zeilenstil (wie Trade-
				     Route-/Suchergebnis-Listen), Pin-/Plumbing-Spalten gefiltert,
				     dezenter ↻-Refresh oben rechts.
				     E-SCLM-UX v4 Punkt 3: der results-Abfang (`Array.isArray
				     (mapped)`) ist entfallen — results-Screens sind im Kern
				     immer Zeilenarrays und liefen daher IMMER in diesen List-
			     Zweig (Tabelle mit Zahlen/Strichen statt results-Ansicht). -->
				<div class="fbar">
					{#if shown.type === 'list' && shown.multi}
						<span class="fmarkinfo">
							{tr('connector.flow.marked', {
								n: String(marked.length),
								max: String(multiMax ?? '∞')
							})}
						</span>
					{/if}
					<span class="fspacer"></span>
					<button
						type="button"
						class="frefresh"
						title={tr('connector.flow.refresh')}
						aria-label={tr('connector.flow.refresh')}
						onclick={() => void load(true)}
					>↻</button>
				</div>
				<ul class="flist">
					{#each rows as row, i (i)}
						<li>
							{#if shown.type === 'list' && shown.multi}
								<button
									class="frow frow-mark"
									class:is-marked={isMarked(i)}
									type="button"
									aria-pressed={isMarked(i)}
									onclick={() => toggleMark(i)}
								>
									<span class="fcheck">{isMarked(i) ? '☑' : '☐'}</span>
									<span class="fmain">
										<span class="ftitle">{listColumns.length > 0 ? formatValue(row?.[listColumns[0]]) : ''}</span>
										{#if listColumns.length > 1}
											<span class="fmeta">
												{#each listColumns.slice(1) as field (field)}
													<span class="fmetapair"><span class="fk">{field}</span>{formatValue(row?.[field])}</span>
												{/each}
											</span>
										{/if}
									</span>
								</button>
							{:else}
								<button
									class="frow"
									type="button"
									onclick={() => select(String(row.id ?? i), row)}
								>
									<span class="fmain">
										<span class="ftitle">{listColumns.length > 0 ? formatValue(row?.[listColumns[0]]) : ''}</span>
										{#if listColumns.length > 1}
											<span class="fmeta">
												{#each listColumns.slice(1) as field (field)}
													<span class="fmetapair"><span class="fk">{field}</span>{formatValue(row?.[field])}</span>
												{/each}
											</span>
										{/if}
									</span>
								</button>
							{/if}
						</li>
					{:else}
						<li class="fempty">{$t('connector.empty')}</li>
					{/each}
				</ul>
			{:else if resultsPending}
				<!-- Spec Abschnitt 3: 200 {result: null} abfangen (nicht nur
				     Status) — Hinweis + Poll-Verhalten wie wait; D-B Punkt 8/11:
				     gleiche zentrierte Spinner-Optik wie die Warteseite. -->
				<div class="fwaitwrap">
					<span class="fspinner" aria-hidden="true"></span>
					<p class="fwaittext">{tr('connector.flow.result_open')}</p>
				</div>
			{:else if shown.type === 'results'}
				<!-- D-B Punkt 11 / E-SCLM-UX v4 Punkt 3+4: Gewinnübersicht —
				     gleichnamige Items zu einer Zeile mit Menge gruppiert
				     (Overlay-Listenstil). Spalten: NUR Item-Name (+ ×Menge),
				     keine WAIVED/COLLECTED-/Zahlen-Spalten; rid/waived/collected
				     bleiben unsichtbar im Zustand für enabled_if und die rid-
				     Auflösung der Buttons. Dezenter ↻ wie auf Listen (nach
				     Ersteller-Reset zeigen die Buttons sich beim Refreshen). -->
				<div class="fbar">
					<span class="fspacer"></span>
					<button
						type="button"
						class="frefresh"
						title={tr('connector.flow.refresh')}
						aria-label={tr('connector.flow.refresh')}
						onclick={() => void load(true)}
					>↻</button>
				</div>
				<ul class="flist">
					{#each resultGroups as g (g.name)}
						<li>
							<div class="frow frow-static">
								<span class="fmain"><span class="ftitle">{g.name}</span></span>
								{#if g.count > 1}
									<span class="fqty">×{g.count}</span>
								{/if}
							</div>
						</li>
					{:else}
						<li class="fempty">{$t('connector.empty')}</li>
					{/each}
				</ul>
			{:else if shown.type === 'detail'}
				<!-- Detailzeilen ohne Kopier-Buttons (D-B Punkt 8: Kopier-Buttons
				     im Flow-Bereich ersatzlos entfernt). -->
				<dl class="fdetail">
					{#each Object.entries(shown.mapping ?? {}) as [field], j (field)}
						<div class="fdetail-line">
							<dt class="fk">{field}</dt>
							<dd class="fv">{formatValue(detailRow?.[field])}</dd>
						</div>
					{/each}
				</dl>
			{:else}
				<p class="fempty">{$t('connector.empty')}</p>
			{/if}
			{#if !resultsPending && (confirmAction || screenActions.length > 0)}
				<!-- Buttons nach vorhandener .btn/.btn-ghost-Norm (D-B Punkt 3):
				     Write-Aktionen = .btn (Akzent), goto-Navigation = .btn-ghost. -->
				<div class="factions">
					{#if confirmAction}
						<button
							class="btn fsubmit"
							type="button"
							disabled={actionBusy || marked.length === 0 || !actionIsEnabled(confirmAction)}
							onclick={() => void confirmMulti()}
						>
							{labelOf(confirmAction.title, lang)}
						</button>
					{/if}
					{#each screenActions as a (a.id)}
						{#if isNavigationAction(a)}
							<button
								class="btn-ghost"
								type="button"
								disabled={!actionIsEnabled(a)}
								onclick={() => void runAction(a)}
							>
								{labelOf(a.title, lang)}
							</button>
						{:else if !oneWayFlag}
							<!-- enabled_if (Requirement 9): bereits gesetzte
							     one-way-Flags (collected/waived) rendern die
							     Aktion deaktiviert (Rust-Defense).
							     E-SCLM-UX v4 Punkt 2: ist ein Flag gesetzt,
							     werden BEIDE Write-Buttons gar nicht mehr
							     gerendert — unten steht stattdessen der
							     Status (kein collected↔waived-Weg). -->
							<button
								class="btn fsubmit"
								type="button"
								disabled={actionBusy || !actionIsEnabled(a)}
								title={actionIsEnabled(a) ? '' : $t('connector.flow.action_done')}
								onclick={() => void runAction(a)}
							>
								{labelOf(a.title, lang)}
							</button>
						{/if}
					{/each}
					{#if oneWayFlag}
						<!-- E-SCLM-UX v4 Punkt 2: One-Way-Status pro ERGEBNIS —
						     „Abgeholt ✓“ ODER „Verzichtet“, nicht beides und
						     nicht änderbar; nur Ersteller/Admin setzt zurück
						     (dann liefert der frische Fetch die Buttons zurück).
						     Chip-Optik wie vorhanden (keine neuen Farben). -->
						<span class="fstatus chip" role="status">{oneWayLabel}</span>
					{/if}
				</div>
				<!-- D-B Punkt 7: Action-Eingabefelder (pcode-Claim) als eigene,
				     ruhig formatierte Zeile unterhalb der Actions — moderate
				     Breite, normale Schriftgröße, dezent. -->
				{#if fieldActions.length > 0}
					<div class="factionfields">
						{#each fieldActions as a (a.id)}
							{#each a.fields ?? [] as f (f.key)}
								<label class="ffield">
									<span class="flabel">{labelOf(f.label, lang)}{f.required !== false ? ' *' : ''}</span>
									<input
										class="finput"
										type="text"
										bind:value={actionInputs[`${a.id}.${f.key}`]}
									/>
								</label>
							{/each}
						{/each}
					</div>
				{/if}
				{#if actionNote}<p class="fnote">{actionNote}</p>{/if}
			{/if}
		</div>
	{/if}
</section>

<style>
	/* D-B Punkt 3: keine neuen Farben — ausschließlich bestehende
	   Theme-Variablen (--line/--lineb/--panel2/--fg/--fgdim/--fgfaint/
	   --accent/--font-mono), Typo-Größen wie .m-title/.m-sub (.mlist-Muster). */
	.flow { display: flex; flex-direction: column; gap: 10px; min-height: 0; }
	.fhead {
		display: flex; align-items: baseline; justify-content: space-between; gap: 10px;
		padding-bottom: 6px; border-bottom: 1px solid var(--line);
	}
	/* Einzige Überschrift der Ansicht (Punkt 5) — wie .m-title, einen Tick lauter. */
	.fscreentitle { font-weight: 700; font-size: 14px; color: var(--fg); }
	.fstep { font: 700 10px/1 var(--font-mono); letter-spacing: 0.1em; text-transform: uppercase; color: var(--fgfaint); }
	.fempty { font-size: 12px; color: var(--fgdim); margin: 0; padding: 8px 0; }
	.ferror { font-size: 12px; margin: 0; color: var(--alert); }
	.fnote { font-size: 11px; color: var(--fgdim); margin: 0; }
	.fbody { display: flex; flex-direction: column; gap: 10px; min-height: 0; }

	/* Listenleiste: Markier-Zähler + dezenter ↻-Refresh (Punkt 1) */
	.fbar { display: flex; align-items: center; gap: 8px; }
	.fspacer { flex: 1; }
	.fmarkinfo { font-size: 11px; color: var(--fgdim); }
	.frefresh {
		background: transparent; color: var(--fgdim); border: 1px solid var(--lineb);
		border-radius: 3px; padding: 3px 8px; font: 700 11px/1 var(--font-mono);
		cursor: pointer;
	}
	.frefresh:hover { border-color: var(--fgdim); color: var(--fg); }

	/* Flow-Listen wie .mlist-Zeilen: volle Breite, --line-Trenner, Hover-Panel */
	.flist {
		list-style: none; margin: 0; padding: 0;
		border-top: 1px solid var(--line);
		overflow: auto; max-height: min(46vh, 340px); scrollbar-width: thin;
	}
	.frow {
		display: grid; grid-template-columns: 1fr auto; align-items: center; gap: 10px;
		width: 100%; text-align: left; background: transparent; color: inherit;
		border: 0; border-bottom: 1px solid var(--line);
		padding: 7px 10px; min-height: 42px; cursor: pointer; font: inherit;
	}
	.frow:hover { background: var(--panel2); }
	.frow-mark { grid-template-columns: auto 1fr; }
	.frow.is-marked { background: var(--panel2); box-shadow: inset 4px 0 0 var(--accent); }
	.frow-static { cursor: default; }
	.fcheck { font-size: 14px; color: var(--fgdim); }
	.fmain { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
	.ftitle { font-size: 13.5px; font-weight: 500; color: var(--fg); overflow-wrap: anywhere; }
	.fmeta { display: flex; flex-wrap: wrap; gap: 10px; font-size: 11px; color: var(--fgdim); }
	.fmetapair { display: inline-flex; gap: 4px; align-items: baseline; }
	.fk { font: 700 9.5px/1 var(--font-mono); letter-spacing: 0.08em; text-transform: uppercase; color: var(--fgfaint); }
	.fqty { font: 600 12px/1 var(--font-mono); color: var(--fgdim); }

	/* D-B Punkt 8: zentrierte Ladeanimation (Theme-Farben, scoped keyframes) */
	.fwaitwrap { display: flex; flex-direction: column; align-items: center; gap: 12px; padding: 28px 10px 20px; }
	@keyframes flow-spin { to { transform: rotate(360deg); } }
	.fspinner {
		width: 34px; height: 34px; border-radius: 50%;
		border: 3px solid var(--lineb); border-top-color: var(--accent);
		animation: flow-spin 0.9s linear infinite;
	}
	.fwaittext {
		font: 700 11px/1.7 var(--font-mono); letter-spacing: 0.12em; text-transform: uppercase;
		color: var(--fgdim); margin: 0;
	}

	/* Formulare (bestehende dezent-Linie, Akzent-Fokus wie .qnum) */
	.fform { display: flex; flex-direction: column; gap: 10px; max-width: 460px; }
	.ffield { display: flex; flex-direction: column; gap: 3px; }
	.flabel { font-size: 11px; color: var(--fgdim); }
	.finput {
		background: transparent; color: inherit; border: 1px solid var(--lineb);
		border-radius: 3px; padding: 6px 9px; font: inherit; font-size: 12.5px;
	}
	.finput:focus { outline: none; border-color: var(--accent); }
	.fauto { font-size: 10px; color: var(--fgfaint); }
	.fsubmit { align-self: flex-start; }
	.facs { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 2px; }
	.fac {
		background: var(--panel2); color: var(--fg); border: 1px solid var(--lineb);
		border-radius: 3px; padding: 5px 8px; font-size: 12px; text-align: left;
		cursor: pointer; width: 100%;
	}
	.fac:hover { border-color: var(--accent); }

	/* Legacy-Flows ohne open-Block: Reiter in globaler Chip-Optik (Req. 10) */
	.ftabs { display: flex; gap: 4px; flex-wrap: wrap; }

	/* Detailzeilen (ohne Kopier-Buttons) */
	.fdetail { margin: 0; display: flex; flex-direction: column; }
	.fdetail-line {
		display: grid; grid-template-columns: 150px 1fr; gap: 10px; align-items: baseline;
		padding: 6px 0; border-bottom: 1px solid var(--line);
	}
	.fv { font-size: 12.5px; color: var(--fg); overflow-wrap: anywhere; }

	/* Actions nach globaler .btn/.btn-ghost-Norm (Punkt 3) — nur Layout hier */
	.factions { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; }

	/* E-SCLM-UX v4 Punkt 2: One-Way-Status anstelle der Write-Buttons —
	   globaler .chip-Look (Theme-Variablen), nur Ausrichtung hier. */
	.fstatus { align-self: center; }

	/* D-B Punkt 7: Action-Feld als eigene Zeile unterhalb der Buttons */
	.factionfields {
		display: flex; flex-direction: column; gap: 8px; max-width: 460px;
		margin-top: 2px; padding-top: 10px; border-top: 1px solid var(--line);
	}
</style>
