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
		type ScreenActionDTO,
		type ParamDTO,
		type LocalizedText
		} from './connector';

	type Result = { data: FetchResultDTO | null; error: string; loading: boolean };

	let {
		entry,
		flow,
		initial = null,
		/** Vorgemergter Flow-Zustand (SF2-R1 D1: gemappte code_entry-Daten des
		 *  Suchleisten-Prefetch — max_wishes/raffle_done landen im fstate,
		 *  ohne sie persistieren zu müssen). */
		initialState = null,
		/** Karten-G (Spec 15): Cockpit-Modal offen? — der Vorgänger (ConnectorViews)
		 *  muss die Esc-Kaskade dann ANHALTEN (Esc schließt erst das Modal). */
		modalActive = $bindable(false)
	}: {
		entry: ConnectorEntry;
		flow: FlowDTO;
		/** Start-Screen-ID (z. B. Wiedereinstieg direkt beim Ergebnis-Screen). */
		initial?: string | null;
		initialState?: Record<string, unknown> | null;
		modalActive?: boolean;
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
	// ---- Karten-G (Spec-Abschnitt 15): Ersteller-Cockpit --------------------
	// Ein Flow mit `tabs` wird zum Cockpit: permanente Sub-Reiter in bestehender
	// Chip-Optik statt Wizard-Kette; Formulare, Mehrfachauswahlen und Confirms
	// sind Modals ÜBER der unbewegten Liste (Esc schließt zuerst das Modal).
	// Die Modal-Klassifikation ist rein Manifest-strukturell (Form-Felder,
	// multi-Listen, statische Boolean-Bodies, Pin-Params, select-Felder) —
	// KEIN connector-spezifisches Wissen. Flow ohne tabs = bisheriges Verhalten
	// unverändert: jeder Cockpit-Zweig ist an `cockpit` gegattert.
	const cockpitTabs = $derived.by(() => {
		const seen = new Set<string>();
		return (flow.tabs ?? []).filter((t) => {
			if (!t?.id || !t.screen || seen.has(t.id) || !findScreen(t.screen)) return false;
			seen.add(t.id);
			return true;
		});
	});
	const cockpit = $derived(cockpitTabs.length > 0);
	// Aktiver Tab = der Tab, dessen Screen gerade gezeigt wird (Modal-Ziele
	// sind keine Tab-Screens — sie ändern den aktiven Tab nicht).
	const activeTabId = $derived(
		cockpitTabs.find((t) => t.screen === shown?.id)?.id ?? cockpitTabs[0]?.id ?? ''
	);
	function tabGo(t: { id: string; screen: string }) {
		if (t.screen === screenId) return;
		go(t.screen); // go() resettet Auswahl/Inputs; der $effect fetcht FRISCH (Fresh-Fetch-Regel, kein Poll-Cache)
	}

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
		// Cockpit (Spec 15): ID-/Claim-Spalten zusätzlich ausblenden
		// ("keine IDs-Spalten") — Codes bleiben nur auf der Übersicht sichtbar
		// (dokumentierte Copy-Ausnahme). rechte/erkenntnisse bleiben sichtbar.
		if (cockpit && s.type === 'list') {
			for (const k of Object.keys(s.mapping ?? {})) {
				if (/(^|_)(id|code)$|^(sid|rid|cid|pcode)$/i.test(k)) hidden.add(k);
			}
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
	// doneFlag + resultsPending: siehe Cockpit-Block (Spec 15) — sie brauchen
	// das cockpit-Flag und sind dort deklariert.
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

	// Spec Abschnitt 3: 200 {result: null} → map_view liefert ein LEERES
	// Zeilenarray (results-Screens sind Table-Views) — nicht nur das
	// Object-Szenario abfangen; leerer Zeilenkern = „Ergebnis noch offen“.
	// Cockpit (Spec 15): leer ist NUR nach der Ziehung „offen“ (done_path
	// z. B. raffle_done) — vor der Ziehung zeigt der Verlosungs-Tab Hinweis
	// + Start-Button. Die resultsPending-Regel bleibt participant-seitig
	// unverändert intact.
	const doneFlag = $derived.by(() => {
		const dp = flow.open?.done_path;
		if (!dp) return false;
		const v = fstate[dp];
		return v === true || v === 1 || v === '1' || v === 'true';
	});
	const resultsPending = $derived(
		shown?.type === 'results' &&
			res.data !== null &&
			(resultsNorm?.rowCount ?? 0) === 0 &&
			(!cockpit || doneFlag)
	);

	// Cockpit-Modal — ein Zustandstyp für alle vier Modal-Formen.
	type CockpitModal = {
		kind: 'form' | 'multi' | 'confirm' | 'prompt';
		title: string;
		warnings: string[];
		target: ScreenDTO | null;
		/** confirm: auszuführende Action; form: Ziel-Formular-Screen. */
		action: ScreenActionDTO | null;
		/** Optional-Kette (Verkauf an → Erlös-POST nach optionalem Feld). */
		chain: ScreenActionDTO | null;
		row: Record<string, unknown> | null;
		/** Injekt-Werte in die Action-Bindings (Zeilen-IDs, participant_id …). */
		extra: Record<string, unknown>;
		/** form: true → connector_form_submit auf target, sonst action ausführen. */
		formSubmit: boolean;
		/** prompt: Eingabefelder (label localized, value vorbelegt).
		 *  H-SCLM-UX R1 (Sebo P3): select-Felder tragen ihre Manifest-options
		 *  und werden als Overlay-Button-Optionen gerendert, nicht als
		 *  native <select> (borderloses Wayland-Overlay: Maus unbedienbar). */
		inputs: { key: string; label: string; required: boolean; type: string; value: string; options?: LocalizedText[] | null }[];
		/** multi: 'wishes' (Kontext = Teilnehmerzeile) | 'assign' (Kontext = markierte Loot-IDs). */
		multi: 'wishes' | 'assign' | null;
		multiCtxRow: Record<string, unknown> | null;
		multiCaptured: string[];
		note: string;
		/** wishes: zweistufige Leeren-Bestätigung (leere Auswahl = Löschen). */
		confirmStage: 'normal' | 'empty';
	};
	let modal = $state<CockpitModal | null>(null);
	let modalRows = $state<Record<string, unknown>[]>([]);
	/** ROHE Entity-Zeilen des Modal-Ziels (Zuordnungs-Eignung braucht die
	 *  Original-Struktur — `participants[].wishes[].id` zipt den Kern zu
	 *  unzuverlässigen Paar-Zeilen, daher nie über das Mapping gehen). */
	let modalRaw = $state<Record<string, unknown>[]>([]);
	let modalMarked = $state<string[]>([]);
	let modalLoading = $state(false);
	let modalBusy = $state(false);
	let copiedField = $state('');
	let copiedTimer: ReturnType<typeof setTimeout> | null = null;

	// ---- Manifest-strukturelle Helfer (generisch, connector-frei) ----------
	function singleField(a: ScreenActionDTO): ParamDTO | null {
		const fs = a.fields ?? [];
		return fs.length === 1 ? fs[0] : null;
	}
	/** Body ohne jeglichen {Platzhalter} (z. B. {for_sale:true}) — statisch. */
	function staticBodyEntry(a: ScreenActionDTO): [string, boolean] | null {
		const b = a.body_template;
		if (!b || typeof b !== 'object' || Array.isArray(b)) return null;
		const e = Object.entries(b as Record<string, unknown>);
		if (e.length !== 1 || typeof e[0][1] !== 'boolean') return null;
		return [e[0][0], e[0][1]];
	}
	/** Body = genau {key: "{key}"} (Selbst-Platzhalter: delta, scu, name …). */
	function bodySelfPlaceholder(a: ScreenActionDTO, key: string): boolean {
		const b = a.body_template;
		if (!b || typeof b !== 'object' || Array.isArray(b)) return false;
		const e = Object.entries(b as Record<string, unknown>);
		return e.length === 1 && e[0][0] === key && e[0][1] === `{${key}}`;
	}
	/** Zeilen-Aktion: non-nav Action, deren Params einen Screen-pin-Key
	 *  (loot_id/cid/participant_id/rid) tragen — sie gehört an die Zeile. */
	function isRowAction(a: ScreenActionDTO, s: ScreenDTO): boolean {
		if (isNavigationAction(a)) return false;
		const pins = s.pins ?? [];
		return (a.params ?? []).some((p) => pins.includes(p.key));
	}
	/** Primitive Werte einer Zeile als Action-Bindings (pfad-{id}-Auflösung). */
	function rowCtx(row: Record<string, unknown>): Record<string, unknown> {
		const out: Record<string, unknown> = {};
		for (const [k, v] of Object.entries(row ?? {})) {
			if (typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean') out[k] = v;
		}
		return out;
	}
	/** Lokaler Warn-Text eines Confirm-Modals (Key fehlt → keine Zeile). */
	function warnText(key: string): string {
		const full = `connector.flow.creator.warn_${key}`;
		const txt = tr(full);
		return txt !== full ? txt : '';
	}
	/** Kurzer Button-Text pro Action (Key fehlt → Manifest-Titel). */
	function rowBtnLabel(a: ScreenActionDTO): string {
		const short = `connector.flow.creator.row_${a.id}`;
		const txt = tr(short);
		if (txt !== short) return txt;
		if (a.enabled_if) {
			const rz = tr('connector.flow.creator.row_reset');
			if (rz !== 'connector.flow.creator.row_reset') return rz;
		}
		return labelOf(a.title, lang);
	}
	/** Kurzer Top-Button-Text nach goto-Ziel (Key fehlt → Manifest-Titel). */
	function topBtnLabel(a: ScreenActionDTO): string {
		if (a.goto) {
			const k = `connector.flow.creator.btn_${a.goto}`;
			const t2 = tr(k);
			if (t2 !== k) return t2;
		}
		return labelOf(a.title, lang);
	}
	/** enabled_if PRO ZEILE (Cockpit: collect/waive/reset leben pro
	 *  Ergebnis, nicht pro Auswahl): equals/not_equals auf dem Zeilenwert —
	 *  gleiche Bool-Toleranz wie evaluateCondition im Kern. */
	function rowActionEnabledFor(a: ScreenActionDTO, row: Record<string, unknown>): boolean {
		const c = a.enabled_if;
		if (!c) return true;
		const v = row?.[c.on];
		if (c.value === true) {
			const t = truthyFlagVal(v);
			return c.op === 'not_equals' ? !t : t;
		}
		const eq = String(v ?? '') === String(c.value);
		return c.op === 'not_equals' ? !eq : eq;
	}
	/** Cockpit-Formularfelder NUR für Menschen: form fields, deren Key als
	 *  {Platzhalter} im body_template steht (sid/code sind Plumbing und
	 *  werden ohnehin aus dem Zustand vorseedet — kein Eingabefeld). */
	function formUserKeys(s: ScreenDTO): Set<string> {
		const keys = new Set<string>();
		templateKeys(s.form?.body_template, keys);
		return keys;
	}
	/** Entity-Name + Id-Feld aus einem Mapping-Pfad `X[].id` (ROH-Body-Zugriff). */
	function entityOf(s: ScreenDTO, key: string): { array: string; idField: string } | null {
		const p = (s.mapping ?? {})[key];
		const m = p && /^([A-Za-z_][A-Za-z0-9_]*)\[\]\.([A-Za-z_][A-Za-z0-9_]*)$/.exec(p);
		return m ? { array: m[1], idField: m[2] } : null;
	}
	function firstEntityName(s: ScreenDTO): string | null {
		for (const p of Object.values(s.mapping ?? {})) {
			const m = /^([A-Za-z_][A-Za-z0-9_]*)\[\]\./.exec(p ?? '');
			if (m) return m[1];
		}
		return null;
	}
	function rawArray(body: unknown, name: string): Record<string, unknown>[] {
		const v = (body as Record<string, unknown> | null | undefined)?.[name];
		return Array.isArray(v) ? (v as Record<string, unknown>[]) : [];
	}
	function wishIdsOf(e: Record<string, unknown>): string[] {
		const w = e?.wishes;
		if (!Array.isArray(w)) return [];
		const ids: string[] = [];
		for (const x of w) {
			if (typeof x === 'string' || typeof x === 'number') {
				const s2 = String(x).trim();
				if (s2) ids.push(s2);
			} else if (x && typeof x === 'object') {
				const o = x as Record<string, unknown>;
				const v = o.loot_id ?? o.id ?? o.item_id;
				if (typeof v === 'string' || typeof v === 'number') {
					const s2 = String(v).trim();
					if (s2) ids.push(s2);
				}
			}
		}
		return ids;
	}
	function truthyFlagVal(v: unknown): boolean {
		return v === true || v === 1 || v === '1' || v === 'true';
	}

	// Cap der Mehrfachauswahl: multi.max_path, sonst Session-Key max_wishes
	// (Spec 15: Wünsche-bearbeiten-Grenze; der Admin-Pfad des Servers prüft
	// die Grenze NICHT — die UI setzt sie hart durch, Decision 5.3).
	const modalCap = $derived.by(() => {
		const t = modal?.target;
		if (!modal || modal.kind !== 'multi' || !t) return null;
		const keys = t.multi?.max_path ? [t.multi.max_path] : ['max_wishes'];
		for (const k of keys) {
			const v = fstate[k];
			if (typeof v === 'number' && v > 0) return Math.max(1, Math.floor(v));
			if (typeof v === 'string') {
				const n = Number(v);
				if (Number.isFinite(n) && n > 0) return Math.max(1, Math.floor(n));
			}
		}
		return null;
	});

	// ---- Cockpit-Aktionstafeln ---------------------------------------------
	type RowBtn = {
		kind: 'button' | 'stepper' | 'prompt' | 'confirm';
		a: ScreenActionDTO;
		chain?: ScreenActionDTO;
		fieldKey?: string;
	};
	/** Zeilen-Buttons eines Cockpit-Screens (Manifest-klassifiziert, s. o.). */
	function cockpitRowBtns(s: ScreenDTO): RowBtn[] {
		const out: RowBtn[] = [];
		const acts = s.actions ?? [];
		const chainHidden = new Set<string>();
		for (const a of acts) {
			if (isNavigationAction(a) || !isRowAction(a, s)) continue;
			const stat = staticBodyEntry(a);
			if (stat) {
				const [, val] = stat;
				const partner = acts.find(
					(b) =>
						b.id !== a.id &&
						!isNavigationAction(b) &&
						b.method === a.method &&
						b.path === a.path &&
						!staticBodyEntry(b) &&
						(b.fields?.length ?? 0) > 0
				);
				if (partner) chainHidden.add(partner.id);
				if (val === true && partner) {
					const pf = (partner.fields ?? [])[0];
					out.push({ kind: 'prompt', a, chain: partner, fieldKey: pf?.key });
				} else {
					// statische One-Way-Schalter (collect/waive/reset) + "aus":
					// Direkt-Button, enabled_if wird PRO ZEILE ausgewertet.
					out.push({ kind: 'button', a });
				}
				continue;
			}
			if (a.method === 'delete') {
				out.push({ kind: 'confirm', a });
				continue;
			}
			const f = singleField(a);
			if (f && bodySelfPlaceholder(a, f.key)) {
				const isColumn = f.key in (s.mapping ?? {});
				if (!isColumn) {
					out.push({ kind: 'stepper', a }); // Delta-Stepper (±1)
				} else if (rows.some((r) => typeof r?.[f.key] === 'number')) {
					out.push({ kind: 'stepper', a, fieldKey: f.key }); // Absolut-Stepper (cur±1)
				} else {
					out.push({ kind: 'prompt', a, fieldKey: f.key }); // Textfeld (prenfilled)
				}
				continue;
			}
			if (f) {
				out.push({ kind: 'prompt', a, fieldKey: f.key });
				continue;
			}
			out.push({ kind: 'confirm', a });
		}
		return out.filter((p) => !chainHidden.has(p.a.id));
	}
	/** Nav-Actions, deren Ziel ein Modal ist (multi-list OHNE aktuellen
	 *  multi-Screen = je ZEILE (Wünsche eines Teilnehmers); multi-list MIT
	 *  aktuellem multi-Screen = Top-Button (Zuordnung markierter Loot-IDs);
	 *  form-Ziel = Top-Button). Kein Tab-Ziel = nie Wizard-Navigation. */
	const cockpitRowNavs = $derived.by(() => {
		const s = shown;
		if (!cockpit || !s || s.type === 'detail' || s.type === 'results') return [];
		return (s.actions ?? []).filter((a) => {
			if (!isNavigationAction(a) || !a.goto) return false;
			const t = findScreen(a.goto);
			return !!t && !!t.multi && !s.multi;
		});
	});
	type TopPlan = { a: ScreenActionDTO; mode: 'formmodal' | 'multimodal' | 'confirmmodal' | 'nav' };
	const cockpitTopPlans = $derived.by((): TopPlan[] => {
		const s = shown;
		if (!cockpit || !s) return [];
		const out: TopPlan[] = [];
		for (const a of s.actions ?? []) {
			if (isNavigationAction(a)) {
				const t = a.goto ? findScreen(a.goto) : null;
				if (!t) continue;
				if (t.multi && !s.multi) continue; // Zeilen-Modal (cockpitRowNavs)
				if (t.multi && s.multi) {
					out.push({ a, mode: 'multimodal' });
				} else if (t.type === 'form') {
					out.push({ a, mode: formUserKeys(t).size > 0 ? 'formmodal' : 'confirmmodal' });
				} else {
					// Ziel ist (Tab-)Screen des Cockpits: normale Reiter-Navigation.
					out.push({ a, mode: 'nav' });
				}
				continue;
			}
			if (isRowAction(a, s)) continue; // Zeilen-Button (cockpitRowBtns)
			const f = singleField(a);
			// H-SCLM-UX R1 (Sebo P2): select-Felder in Actions werden nicht
			// mehr inline gerendert (kein CockpitSelect-Duplikat). Die
			// max_wishes-Aktion bleibt im Manifest, ihr einziger UI-Weg ist
			// der Einstellungen-Button (goto settings → Settings-Modal mit
			// Button-Optionen 1–6, Spec-Abschnitt 16).
			if (f && f.type === 'select') continue;
			out.push({ a, mode: 'confirmmodal' });
		}
		return out;
	});
	// Übersicht (detail): _code-Keys → Code-Zeile mit Copy-Button (dokumentierte
	// Ausnahme der No-Copy-Regel: NUR Codes im Creator-Cockpit, Spec 15) und
	// _count-Keys → Statistikkarten im bestehenden Overlay-Kartenstil.
	const cockpitCodeRows = $derived.by(() => {
		const s = shown;
		if (!cockpit || s?.type !== 'detail') return [];
		return Object.keys(s.mapping ?? {})
			.filter((k) => /_code$/i.test(k))
			.map((k) => ({
				field: k,
				value: detailRow?.[k] ?? fstate[k] ?? null
			}));
	});
	const cockpitStatRows = $derived.by(() => {
		const s = shown;
		if (!cockpit || s?.type !== 'detail') return [];
		return Object.keys(s.mapping ?? {})
			.filter((k) => /_count$/i.test(k))
			.map((k) => ({
				field: k,
				value: detailRow?.[k] ?? fstate[k] ?? null
			}));
	});
	const cockpitName = $derived(
		cockpit && shown?.type === 'detail' && 'name' in (shown.mapping ?? {})
			? formatValue(detailRow?.name ?? fstate.name)
			: ''
	);
	function metaFieldLabel(field: string): string {
		const k = `connector.flow.creator.field_${field}`;
		const txt = tr(k);
		return txt !== k ? txt : field;
	}
	/** Meta-Wert-Text: Booleans als localized an/aus (Verkaufs-Status),
	 *  sonst bestehende formatValue-Norm. */
	function metaValueText(v: unknown): string {
		if (typeof v === 'boolean')
			return v ? tr('connector.flow.creator.flag_on') : tr('connector.flow.creator.flag_off');
		return formatValue(v);
	}
	// Kopfzeile des Waren-Tabs: Gesamt-Erlös + pro Kopf (0 Teilnehmer → „—“),
	// reine Anzeige aus dem ROHEN Session-Body (commodity_summary.total_revenue).
	const revenueHead = $derived.by(() => {
		const s = shown;
		const body = res.data?.body as Record<string, unknown> | null | undefined;
		if (!cockpit || !s || s.type !== 'list' || !body || typeof body !== 'object') return null;
		// Screen-scharfes Gate (Review R1-Punkt 1, Spec 15 Punkt 7): die
		// Pro-Kopf-Zeile gehoert NUR zur Waren-Liste. Manifeststrukturelle
		// Identifikation ueber deren Commodity-ID (mapping-Key 'cid' —
		// identisch deklariert wie ihr pins-Eintrag): Session-weite
		// Body-Felder wie total_revenue/commodity_summary stecken im rohen
		// Body JEDES Screens und duerfen Teilnehmer-/Loot-Tab nicht
		// dekorieren. Keine Screen-Ids, kein SCLM-Wissen im Renderer.
		if (!('cid' in (s.mapping ?? {}))) return null;
		const cs = body.commodity_summary as Record<string, unknown> | undefined;
		const total = cs?.total_revenue ?? body.total_revenue;
		if (typeof total !== 'number' || !Number.isFinite(total)) return null;
		const parts = Array.isArray(body.participants) ? body.participants.length : 0;
		return { total, per: parts > 0 ? total / parts : null };
	});
	// Teilnehmer-Zeilen: Wunsch-Anzahl/-Namen dezent unter dem Namen. Quelle =
	// ROHER Session-Body (Teilnehmer-Entity mit wishes-ID-Liste; Namen via
	 // sibling {id,name}-Array). Manifest-unabhängige Datenform-Heuristik:
	// Connectors ohne dieses Muster zeigen schlicht keine Wunsch-Meta.
	const cockpitWishMeta = $derived.by(() => {
		const out = new Map<string, string[]>();
		const s = shown;
		const body = res.data?.body as Record<string, unknown> | null | undefined;
		if (!cockpit || !s || s.type !== 'list' || !body || typeof body !== 'object') return out;
		const idKey = Object.keys(s.mapping ?? {}).find((k) => /(^|_)id$/i.test(k));
		const ent = idKey ? entityOf(s, idKey) : null;
		if (!ent) return out;
		const ents = rawArray(body, ent.array);
		if (!ents.length) return out;
		const pools = Object.values(body).filter(
			(v): v is Record<string, unknown>[] =>
				Array.isArray(v) &&
				v.length > 0 &&
				v.every((x) => !!x && typeof x === 'object' && 'id' in x && 'name' in x)
		);
		for (const e of ents) {
			const id = String(e[ent.idField] ?? '').trim();
			if (!id || !Array.isArray(e.wishes)) continue;
			const ids = wishIdsOf(e);
			out.set(
				id,
				ids.map((lid) => {
					for (const p of pools) {
						const hit = p.find((x) => String(x.id) === lid);
						if (hit) return String(hit.name);
					}
					return lid;
				})
			);
		}
		return out;
	});
	// results-Zweig des Cockpits: pro Teilnehmer Name + Gruppen ×N + Status
	// bzw. Buttons (enabled_if PRO ZEILE; Reset-Buttons erscheinen nur bei
	// gesetztem Flag — nach Reset liefert der frische Fetch die Buttons zurück).
	/** I-SCLM Cockpit UX R2 (Punkt 5): Gewinn-Items aus den Ergebnis-Arrays.
	 *  Objekte liefern .name/.title; Strings nur, wenn sie echte Namen sind —
	 *  rein Lowercase-alphanumerische Tokens mit Ziffer (8-Zeichen-Codes im
	 *  Stil 'lvm3ytne': rid/pcode/loot-id) sind ID-Müll, nie ein Gewinn;
	 *  namen-artige Alpha-Tokens bleiben unberührt. */
	function wonItemNames(items: unknown[]): string[] {
		const out: string[] = [];
		for (const item of items) {
			if (typeof item === 'string') {
				const s2 = item.trim();
				if (!s2) continue;
				if (/^[a-z0-9]+$/.test(s2) && s2.length >= 6 && /\d/.test(s2)) continue;
				out.push(s2);
			} else if (item && typeof item === 'object') {
				const o = item as Record<string, unknown>;
				const n = typeof o.name === 'string' ? o.name : typeof o.title === 'string' ? o.title : null;
				if (n && n.trim()) out.push(n.trim());
			}
		}
		return out;
	}
	const cockpitResultRows = $derived.by(() => {
		const s = shown;
		if (!cockpit || s?.type !== 'results') return [];
		const mapping = s.mapping ?? {};
		const nameKey =
			Object.keys(mapping).find((k) => !/(id|code)$/i.test(k) && /(participant|name|title)/i.test(k)) ??
			Object.keys(mapping)[0];
		// I-SCLM Cockpit UX R2 (Punkt 5): Aufbau auf STRUKTURIERTEN Daten.
		// Der bisherige flache Zip (normalizeResults auf gemappten Zeilen)
		// schob jede `[]`-Mapping-Stringspalte in die Gewinne: Teilnehmer-
		// namen landeten als x1-Zeile, 8-Zeichen-Codes (participant_id/rid —
		// 'lvm3ytne'-Muster) als Item. Strukturiert: eine Zeile pro
		// Ergebnis-OBJEKT (ROH-Body-Entity — dieselbe Array-Quelle, die der
		// Rust-Mapping-Engine-Zip hat); Gewinne werden NUR aus den ARRAY-
		// Feldern des Ergebnis-Objekts gruppiert (SCLM: results[].won →
		// {name,type,grade,size}). Skalare (rid/waived/collected/name) werden
		// nie Gewinne, bleiben aber als Zeilenwerte für enabled_if und die
		// {rid}-Pfadauflösung der Buttons erhalten.
		const body = res.data?.body as Record<string, unknown> | null | undefined;
		const ent = body && typeof body === 'object' ? firstEntityName(s) : null;
		const entries = ent ? rawArray(body, ent) : [];
		if (entries.length > 0) {
			// Mapping-Pfad `X[].field` → Feld des Ergebnis-Objekts (Keys
			// folgen dem Manifest, keiner Connector-Hardcode-Kette).
			const suffix = (p: string, k: string): string => {
				const i = (p ?? '').indexOf('[].');
				return i >= 0 ? p.slice(i + 3) : k;
			};
			return entries.map((e) => {
				const row: Record<string, unknown> = {};
				const itemsSrc: unknown[] = [];
				for (const [k, path] of Object.entries(mapping)) {
					const v = e[suffix(path, k)];
					if (Array.isArray(v)) itemsSrc.push(...v);
					else row[k] = v;
				}
				const groups = groupNames(wonItemNames(itemsSrc));
				const flags: string[] = [];
				for (const a of s.actions ?? []) {
					if (isNavigationAction(a) || !a.enabled_if || a.enabled_if.value !== true) continue;
					// I-SCLM UX R2-R1 (Review Finding 2): collect +
					// reset_collected teilen sich enabled_if.on='collected'
					// — ohne Dedup erschiene der Status-Chip doppelt
					// (keyed each (fl) wirft sonst duplizierte Keys).
					if (truthyFlagVal(row[a.enabled_if.on]) && !flags.includes(a.enabled_if.on))
						flags.push(a.enabled_if.on);
				}
				return {
					row,
					name: formatValue(row[nameKey] ?? e.name ?? ''),
					groups,
					flags
				};
			});
		}
		// Fallback (Connectors ohne ROH-Body-Entity oder ohne []-Pfade):
		// alter flacher Pfad unverändert — für SCLM greift er nie (Session-
		// Body enthält immer results[]), andere Flows bleiben byte-identisch.
		return rows.map((row) => {
			const groups = groupNames(normalizeResults([row], mapping).names);
			const flags: string[] = [];
			for (const a of s.actions ?? []) {
				if (isNavigationAction(a) || !a.enabled_if || a.enabled_if.value !== true) continue;
				// Dedup wie im strukturierten Zweig (Review Finding 2).
				if (truthyFlagVal(row?.[a.enabled_if.on]) && !flags.includes(a.enabled_if.on))
					flags.push(a.enabled_if.on);
			}
			return {
				row,
				name: nameKey ? formatValue(row?.[nameKey]) : '',
				groups,
				flags
			};
		});
	});
	// Zuordnungs-Modal (assign): Eignung aus ROHEN Teilnehmer-Zeilen — nur mit
	// freien Wunsch-Slots (cap − aktuelle Wünsche − captured), bereits
	// zugeordnete Teilnehmer deaktiviert mit Hinweis (Spec 15 Punkt 6).
	const assignRows = $derived.by(() => {
		const m = modal;
		if (!m || m.multi !== 'assign') return [];
		const cap = modalCap;
		const captured = m.multiCaptured;
		return modalRaw.map((e) => {
			const ids = wishIdsOf(e);
			const overlap = captured.some((l) => ids.includes(l));
			const full = cap !== null && ids.length + captured.length > cap;
			return {
				id: String(e.id ?? '').trim(),
				name: String(e.name ?? ''),
				count: ids.length,
				eligible: !overlap && !full,
				reason: overlap
					? tr('connector.flow.creator.assign_hint_assigned')
					: full
						? tr('connector.flow.creator.assign_hint_full')
						: ''
			};
		});
	});

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
		// I-SCLM Cockpit UX R2 (Punkt 1): Add-Formulare (Manifest-Feld
		// prefill: false) dürfen sichtbare Nutzerfelder NICHT aus dem
		// Flow-Zustand befüllen — sonst landet gern Event-/letzter
		// Teilnehmername im neuen Eingabefeld. Plumbing-Keys (sid/code/
		// Params/Header) werden weiter still geseedet; Bearbeiten-
		// Formulare (prefill fehlt/true) bleiben vorbefüllt wie bisher.
		const visible = s.form?.prefill === false ? formUserKeys(s) : null;
		for (const k of seedKeys(s)) {
			if (k in target) continue;
			if (visible?.has(k)) continue;
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
	/** I-SCLM Cockpit UX R2 (Punkt 2b): Modal-Fehlerzeile mit Server-Wortlaut.
	 *  Response-Body-Feld `error` (sonst `message`) plus HTTP-Status und
	 *  METHODE pfad — kurz gehalten (max. 160 Zeichen), Locale-Basistext. */
	function httpErrorNote(status: number, body: unknown, method?: string | null, path?: string | null): string {
		let note = tr('connector.flow.server_report', {
			status: String(status),
			method: (method ?? '').toUpperCase(),
			path: path ?? ''
		});
		if (body && typeof body === 'object' && !Array.isArray(body)) {
			const b = body as Record<string, unknown>;
			const msg =
				typeof b.error === 'string' ? b.error.trim() : typeof b.message === 'string' ? b.message.trim() : '';
			if (msg) note += `: ${msg}`;
		}
		return note.length > 160 ? `${note.slice(0, 159)}…` : note;
	}
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
					: httpErrorNote(out.status, out.body, s.form?.method, s.form?.path);
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

	/** Karten-G (Spec 15): `extra` injiziert Bindings (Zeilen-IDs,
	 *  participant_id, whole-array loot_ids …) — Cockpit-Buttons haben keine
	 *  Eingabefelder, ihre Pin-Werte kommen direkt aus Zeile/Modal. Extra
	 *  hat die letzte Schreibhand. */
	async function runAction(a: ScreenActionDTO, extra?: Record<string, unknown>, screenOverride?: string) {
		const s = shown;
		if (!s || actionBusy) return;
		if (isNavigationAction(a)) {
			if (a.goto) go(a.goto);
			return;
		}
		// enabled_if (one-way-Flag): deaktivierte Aktionen laufen nicht.
		// Zeilen-Aktionen (extra = rowCtx(row)): enabled_if referenziert
		// Screen-Mapping-Keys (for_sale, collected...), die NUR in der Zeile
		// leben — nie im Flow-Zustand. Vorher nur gegen fstate evaluiert:
		// 'sale_off' (equals true, for_sale in fstate fehlend) fiel bei
		// jedem Klick durch — Button sichtbar, Klick stumm (Sebo-Befund
		// 2026-11-05, docs SCLM-FLOW-V2 §24). Merge: Zeilenwert gewinnt.
		if (!actionEnabled(a, extra ? { ...fstate, ...extra } : fstate)) return;
		const mergedValues = { ...actionValues(), ...(extra ?? {}) };
		// SF2-R1 D3: Pflicht-Action-Felder (z. B. pcode-Claim) vor dem Write
		// prüfen — fehlende Werte laufen sonst als 403/400 beim Server an.
		for (const f of a.fields ?? []) {
			if (f.required === false) continue;
			const v = String(mergedValues[f.key] ?? '').trim();
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
				// Modal-Acts (Wünsche full-replace u. a.) sind an der MODAL-
				// Ziel-Screen des Manifests deklariert, nicht am sichtbaren
				// Tab-Screen — Rust sucht die Action in der übergebenen
				// Screen (view_unknown sonst, Sebo-Befund 2026-11-05 §23).
				screenId: `${flow.id}:${screenOverride ?? s.id}`,
				actionId: a.id,
				values: mergedValues
			});
			actionNote =
				out.status < 400
					? tr('connector.form_ok', { status: String(out.status) })
					: httpErrorNote(out.status, out.body, a.method, a.path);
			if (out.status < 400) {
				// pins_map: Antwortfelder in den Flow-Zustand pinnen (z. B. pcode).
				for (const [key, path] of Object.entries(a.pins_map ?? {})) {
					const v = dotGet(out.body, path);
					if (typeof v === 'string') pin(key, v);
					else if (typeof v === 'number') pin(key, String(v));
				}
				// SF2-R1 D3: eingegebene Action-Felder sind Session-Pins (z. B.
				// pcode-Claim — der erste Save bindet den Code, spätere Saves/
				// results brauchen ihn nicht mehr einzutippen.
				// Cockpit-Ausnahme (Spec 15): Feld-Keys wie name/delta/scu sind
				// KEINE Pins und dürfen den Flow-Zustand nicht vergiften (sonst
				// seedet ein rename-"name" später das add_participant-Feld).
				// Claim-Felder bleiben Pins, wenn das Manifest sie pinnt.
				for (const f of a.fields ?? []) {
					if (cockpit && !(s.pins ?? []).includes(f.key)) continue;
					const v = String(mergedValues[f.key] ?? '').trim();
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
				const before = screenId;
				if (a.goto) go(a.goto);
				// ohne goto ODER goto auf den aktuellen Screen (Cockpit-Modals
				// confirmen auf ihren eigenen Tab): der $effect löst dabei
				// keinen Fetch aus → explizit frisch nachladen (Status sichtbar).
				if (!a.goto || screenId === before) await load(true);
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
		// Verkaufte Items sind serverseitig nicht verlosbar; bereits gemarkte
		// Zeilen (z. B. Vorauswahl alter Wünsche) fallen raus (docs §24).
		const selected = rows.filter((r, i) => isMarked(i) && !truthyFlagVal(r?.for_sale));
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

	// ---- Karten-G: Cockpit-Modal-Handler ------------------------------------
	// Der Vorgänger hält die Esc-Kaskade an, solange ein Modal offen ist.
	$effect(() => {
		modalActive = modal !== null;
	});

	function closeModal() {
		modal = null;
		modalRows = [];
		modalRaw = [];
		modalMarked = [];
		modalLoading = false;
		modalBusy = false;
	}

	function openModal(m: CockpitModal) {
		modal = m;
		void loadModal();
	}

	function mkModal(p: Partial<CockpitModal>): CockpitModal {
		return {
			kind: 'confirm',
			title: '',
			warnings: [],
			target: null,
			action: null,
			chain: null,
			row: null,
			extra: {},
			formSubmit: false,
			inputs: [],
			multi: null,
			multiCtxRow: null,
			multiCaptured: [],
			note: '',
			confirmStage: 'normal',
			...p
		};
	}

	/** Modal-Ziel-Liste frisch fetchen (viewId-Regel wie Screens:
	 *  `<flow>:<screen>`, force = Fresh-Fetch wie beim Tab-Wechsel). */
	async function loadModal() {
		const m = modal;
		const t = m?.target;
		if (!m || !t || !t.endpoint || !isTauri) return;
		modalLoading = true;
		modalMarked = [];
		try {
			const data = await invoke<FetchResultDTO>('connector_fetch', {
				id: entry.id,
				viewId: `${flow.id}:${t.id}`,
				force: true,
				params: bindings(t)
			});
			modalRows = Array.isArray(data.mapped) ? (data.mapped as Record<string, unknown>[]) : [];
			const ent = firstEntityName(t);
			modalRaw = ent ? rawArray(data.body, ent) : [];
			if (m.multi === 'wishes') {
				// Vorauswahl: die Wünsche, die dieser Teilnehmer gerade hält
				// (ROHER Body der Basisscreen-Session, nicht das Modal-Mapping).
				let held: string[] = [];
				const ctx = multiCtxOf(t);
				if (ctx.key && m.multiCtxRow) {
					const cid = String(m.multiCtxRow[ctx.key] ?? '');
					const e = rawArray(res.data?.body, ctx.ent?.array ?? '').find(
						(x) => String(x[ctx.ent?.idField ?? 'id'] ?? '') === cid
					);
					if (e) held = wishIdsOf(e);
				}
				const pinKey = (t.pins ?? [])[0];
				modalMarked = modalRows
					.map((r, i) => (pinKey && held.includes(String(r[pinKey] ?? '')) ? String(i) : ''))
					.filter((v) => v !== '');
			} else if (m.multi === 'assign') {
				// Keine Vorauswahl (Review R1-Punkt 2): Teilnehmer, die das
				// captured Loot bereits halten, sind in assignRows als
				// ineligible deaktiviert (Hinweis im Modal). Eine
				// Vorauswahl genau dieser Zeilen war toter Code —
				// `eligible` impliziert overlap-frei — und waehlte
				// aehnliche Formeln im Zweifel deaktivierte Zeilen vor.
				// Verhalten unveraendert: nichts vorausgewaehlt.
				modalMarked = [];
			}
		} catch (err) {
			modalRows = [];
			modalRaw = [];
			if (modal) modal.note = trError(err);
		}
		modalLoading = false;
	}

	/** Kontext-Key einer modalen Write-Aktion: Body-{Platzhalter}-Key, den der
	 *  Basis-Screen als Pin liefert (z. B. participant_id vom Teilnehmer-Tab),
	 *  NICHT der Ziel-Pin selbst (loot_id des Wünsche-Modals). */
	function multiCtxOf(t: ScreenDTO): { key: string | null; ent: { array: string; idField: string } | null } {
		const write = (t.actions ?? []).find((a) => !isNavigationAction(a));
		const ph = new Set<string>();
		if (write) templateKeys(write.body_template, ph);
		const targetPin = (t.pins ?? [])[0];
		const key = (shown?.pins ?? []).find((k) => k !== targetPin && ph.has(k)) ?? null;
		return { key, ent: key ? entityOf(shown as ScreenDTO, key) : null };
	}

	function isModalMarked(k: string): boolean {
		return modalMarked.includes(k);
	}

	/** harte UI-Grenze: max_path bzw. Session max_wishes (Spec 15); der
	 *  Server-Admin-Pfad prüft sie NICHT — die Grenze ist UI-Pflicht. */
	function toggleModalMark(k: string, eligible: boolean) {
		const m = modal;
		if (!m || !eligible || !k) return;
		if (modalMarked.includes(k)) {
			modalMarked = modalMarked.filter((x) => x !== k);
		} else if (m.multi === 'wishes' && modalCap !== null && modalMarked.length >= modalCap) {
			return;
		} else {
			modalMarked = [...modalMarked, k];
		}
	}

	function openTopPlan(p: TopPlan) {
		const a = p.a;
		if (p.mode === 'nav') {
			if (a.goto) go(a.goto);
			return;
		}
		const t = a.goto ? findScreen(a.goto) : null;
		if (p.mode === 'formmodal' && t?.form) {
			formValues = {};
			formNote = '';
			seedFromState(t, formValues);
			openModal(mkModal({ kind: 'confirm', target: t, formSubmit: true, title: labelOf(t.title, lang) }));
			return;
		}
		if (p.mode === 'confirmmodal') {
			if (t?.form) {
				// Confirm-Submit ohne Nutzerfelder (z. B. regen_codes): Warnung +
				// Bestätigen; Submit via connector_form_submit (Codes werden
				// gepinnt, frischer Fetch zeigt die neuen Codes).
				formValues = {};
				formNote = '';
				seedFromState(t, formValues);
				openModal(
					mkModal({
						kind: 'confirm',
						target: t,
						formSubmit: true,
						title: labelOf(t.title, lang),
						warnings: [warnText(a.id), warnText(t.id)].filter(Boolean)
					})
				);
			} else {
				const warnings = [warnText(a.id)];
				if (doneFlag) warnings.push(warnText('overwrite'));
				openModal(
					mkModal({ kind: 'confirm', action: a, title: labelOf(a.title, lang), warnings: warnings.filter(Boolean) })
				);
			}
			return;
		}
		if (p.mode === 'multimodal' && t) {
			const s = shown;
			const pinKey = s ? (s.pins ?? [])[0] : undefined;
			const captured =
				s && pinKey
					? rows
							.filter((_, i) => isMarked(i))
							.map((r) => {
								const v = r?.[pinKey];
								return v === undefined || v === null ? '' : String(v);
							})
							.filter((v) => v !== '')
					: [];
			openModal(mkModal({ kind: 'multi', multi: 'assign', target: t, title: labelOf(t.title, lang), multiCaptured: captured }));
		}
	}

	function openRowNav(a: ScreenActionDTO, row: Record<string, unknown>) {
		const t = a.goto ? findScreen(a.goto) : null;
		if (!t || !t.multi) return;
		// I-SCLM Cockpit UX R2 (Punkt 4): multi.mode entscheidet die Richtung
		// des Zeilen-Modals. 'assign' pinnt GENAU den Zeilen-Gegenstand
		// (multiCaptured = seine loot_id) und zeigt die Teilnehmerliste mit
		// freien Wunsch-Slots — keine Loot-Mehrfachauswahl mehr. 'wishes'
		// (Default, Bestandsmanifeste ohne mode unverändert): Loot-Auswahl
		// für den Zeilen-Teilnehmer.
		const mode = t.multi.mode ?? 'wishes';
		if (mode === 'assign') {
			const pinKey = (shown?.pins ?? [])[0];
			const v = pinKey ? row?.[pinKey] : undefined;
			const captured = v === undefined || v === null ? [] : [String(v)].filter((x) => x !== '');
			openModal(
				mkModal({
					kind: 'multi',
					multi: 'assign',
					target: t,
					multiCaptured: captured,
					title: `${labelOf(a.title, lang)} — ${formatValue(row?.name ?? row?.title ?? '')}`,
					warnings: []
				})
			);
			return;
		}
		openModal(
			mkModal({
				kind: 'multi',
				multi: 'wishes',
				target: t,
				multiCtxRow: row,
				title: `${labelOf(t.title, lang)} — ${formatValue(row?.name ?? row?.title ?? '')}`,
				warnings: []
			})
		);
	}

	function openRowBtn(p: RowBtn, row: Record<string, unknown>) {
		const a = p.a;
		if (p.kind === 'button') {
			void runAction(a, rowCtx(row));
			return;
		}
		if (p.kind === 'confirm') {
			openModal(
				mkModal({ kind: 'confirm', action: a, row, title: labelOf(a.title, lang), warnings: [warnText(a.id), warnText('remove')].filter((v, i, arr) => v && arr.indexOf(v) === i) })
			);
			return;
		}
		if (p.kind === 'prompt') {
			const src = p.chain
				? (p.chain.fields ?? []).find((f) => f.key === p.fieldKey)
				: (a.fields ?? []).find((f) => f.key === p.fieldKey);
			const inputs: CockpitModal['inputs'] = src
				? [
						{
							key: src.key,
							label: labelOf(src.label, lang),
							// required nur bei EXPLIZIT deklariertem
							// true (Review R1-Punkt 3): Manifest-
							// Contract aus F — required ist optional
							// deklariert; undeklarierte Felder wie
							// sale_revenue sind laut Spec 15 optional
							// und duerfen nicht UI-pflichtig werden.
							required: src.required === true,
							type: src.type === 'select' ? 'select' : src.type === 'number' ? 'number' : 'text',
							options: src.type === 'select' ? src.options : undefined,
							value: String(row?.[src.key] ?? '')
						}
					]
					: [];
			openModal(mkModal({ kind: 'prompt', action: a, chain: p.chain ?? null, row, title: labelOf(a.title, lang), inputs }));
		}
	}

	/** qty ± (delta-Body) und scu ± (absoluter Body) — Zähler-Buttons statt
	 *  Freitextfeld (Spec 15 Punkte 6/7). Absolut: cur+sign, floor 0. */
	async function stepperRun(a: ScreenActionDTO, row: Record<string, unknown>, fieldKey: string | undefined, sign: number) {
		const s = shown;
		if (!s || actionBusy) return;
		const f = (a.fields ?? []).find((x) => x.key === fieldKey) ?? (a.fields ?? [])[0];
		if (!f) return;
		const abs = !!fieldKey && fieldKey in (s.mapping ?? {});
		const val = abs ? String(Math.max(0, Number(row?.[fieldKey] ?? 0) + sign)) : String(sign);
		actionInputs = { ...actionInputs, [`${a.id}.${f.key}`]: val };
		await runAction(a, rowCtx(row));
	}

	async function modalSaveMulti() {
		const m = modal;
		const t = m?.target;
		if (!m || !t || m.kind !== 'multi') return;
		const write = (t.actions ?? []).find((a) => !isNavigationAction(a));
		const targetPin = (t.pins ?? [])[0] ?? 'id';
		const ctx = multiCtxOf(t);
		if (m.multi === 'wishes') {
			const ids = modalRows
				.map((r, i) => (isModalMarked(String(i)) && !truthyFlagVal(r?.for_sale) ? String(r[targetPin] ?? '') : ''))
				.filter((v) => v !== '');
			if (ids.length === 0 && m.confirmStage !== 'empty') {
				// Zweistufig (Spec 15): leere Auswahl = alle Wünsche löschen —
				// erst nach Bestätigung im Modal.
				m.confirmStage = 'empty';
				return;
			}
			const extra: Record<string, unknown> = { [targetPin]: ids };
			if (ctx.key && m.multiCtxRow) {
				const v = m.multiCtxRow[ctx.key];
				if (typeof v === 'string' || typeof v === 'number') extra[ctx.key] = v;
			}
			// Sebo-Befund 2026-11-05 (docs SCLM-FLOW-V2 §23): Der Act haengt an
			// der Modal-ZIEL-Screen (wishes_for), nicht am sichtbaren Tab-
			// Screen (c_participants) — Rust loest die Action in der
			// uebergebenen Screen auf. Vorher: runAction leitete sich aus
			// `shown` (Tab) ab, Rust fand 'confirm' dort nicht und antwortete
			// view_unknown ('Unbekannter View.' im Tab, POST lief nie raus).
			// Zusaetzlich: Modal schliesst erst NACH dem POST (erfolgreiche
			// Uebernahme bleibt sichtbar, Fehlermeldung bleibt im Modal-Kontext
			// des Tabs erhalten). Der assign-Zweig nutzte die Ziel-ID schon
			// direkt — korrekt.
			if (write) await runAction(write, extra, t.id); // full-replace POST
			closeModal();
			return;
		}
		// assign: ein POST je gewähltem Teilnehmer — bestehende Wünsche bleiben,
		// das captured Loot wird ergänzt (dedupliziert).
		const chosen = assignRows.filter((r) => r.eligible && r.id && isModalMarked(r.id));
		const lootIds = m.multiCaptured;
		if (!write || chosen.length === 0) {
			closeModal();
			return;
		}
		modalBusy = true;
		let errNote = '';
		for (const pr of chosen) {
			const e = modalRaw.find((x) => String(x.id ?? '') === pr.id) ?? {};
			const existing = wishIdsOf(e);
			const merged = [...existing, ...lootIds.filter((l) => !existing.includes(l))];
			try {
				const out = await invoke<{ status: number; body: unknown }>('connector_screen_action', {
					id: entry.id,
					screenId: `${flow.id}:${t.id}`,
					actionId: write.id,
					values: { ...actionValues(), [targetPin]: pr.id, ...(ctx.key ? { [ctx.key]: merged } : {}) }
				});
				if (out.status >= 400 && !errNote)
					errNote = httpErrorNote(out.status, out.body, write.method, write.path);
			} catch (err) {
				if (!errNote) errNote = trError(err);
			}
		}
		closeModal();
		actionNote = errNote || tr('connector.flow.creator.assign_done', { n: String(chosen.length) });
		marked = [];
		await load(true);
	}

	async function modalPromptRun() {
		const m = modal;
		const a = m?.action;
		if (!m || !a) return;
		const extra: Record<string, unknown> = { ...m.extra };
		for (const inp of m.inputs) {
			const v = inp.value.trim();
			if (inp.required && !v) {
				m.note = tr('error.connector.param_required', { key: inp.label });
				return;
			}
			if (v) extra[inp.key] = v;
		}
		const rowValues = m.row ? rowCtx(m.row) : {};
		closeModal();
		await runAction(a, { ...rowValues, ...extra });
		const filled = m.inputs.some((i) => String(extra[i.key] ?? '').trim());
		if (m.chain && filled && !actionBusy) {
			// Optional-Kette Verkauf an: zweiter PATCH derselben Zeile mit Wert.
			await runAction(m.chain, { ...rowValues, ...extra });
		}
	}

	async function modalConfirmRun() {
		const m = modal;
		if (!m) return;
		if (m.formSubmit) {
			await modalFormSubmit();
			return;
		}
		const a = m.action;
		if (!a) return;
		const extra = { ...(m.row ? rowCtx(m.row) : {}), ...m.extra };
		closeModal();
		await runAction(a, extra);
	}

	async function modalFormSubmit() {
		const m = modal;
		const t = m?.target;
		if (!m || !t?.form || !isTauri) return;
		modalBusy = true;
		m.note = '';
		const seeded: Record<string, string> = { ...formValues };
		seedFromState(t, seeded);
		for (const f of t.form.fields ?? []) {
			// required nur bei EXPLIZIT true (Review R1-Punkt 3, gleiche
			// Semantik wie im Prompt-Modal): Spec-15-optionale Felder
			// (add_loot size/type/grade/qty, add_commodity scu, settings
			// max_wishes) sind im Manifest undeklariert-required und
			// duerfen den Submit nicht blocken; explizit required:true
			// (name/code/sid) bleibt Pflicht.
			if (f.required === true && !String(seeded[f.key] ?? '').trim()) {
				m.note = tr('error.connector.param_required', { key: labelOf(f.label, lang) });
				modalBusy = false;
				return;
			}
		}
		try {
			const out = await invoke<{ status: number; body: unknown }>('connector_form_submit', {
				id: entry.id,
				formId: `${flow.id}:${t.id}`,
				values: seeded
			});
			if (out.status >= 400) {
				m.note = httpErrorNote(out.status, out.body, t.form?.method, t.form?.path);
				modalBusy = false;
				return;
			}
			// Werte in den Zustand (Regel wie submitForm; Cockpit-Frischfetch
			// bringt ohnehin die Wahrheit — hier Best-effort-Vorfüllen).
			for (const [k, v] of Object.entries(seeded)) if (!(k in fstate)) fstate[k] = v;
			if (out.body && typeof out.body === 'object') {
				const body = out.body as Record<string, unknown>;
				for (const [k, v] of Object.entries(body)) {
					fstate[k] = v;
					if ((t.pins ?? []).includes(k) && typeof v === 'string') pin(k, v);
				}
				for (const [k, path] of Object.entries(t.mapping ?? {})) {
					const v = dotGet(body, path);
					if (typeof v === 'string' || typeof v === 'number') {
						fstate[k] = v;
						if ((t.pins ?? []).includes(k)) pin(k, String(v));
					}
				}
			}
			closeModal();
			const nxt = t.on_submit;
			if (nxt && nxt !== screenId && findScreen(nxt)) go(nxt); // $effect fetcht frisch
			else await load(true);
		} catch (err) {
			m.note = trError(err);
			modalBusy = false;
		}
	}

	/** No-Copy-Regel-Ausnahme (Spec 15): NUR Codes im Creator-Cockpit. */
	async function copyCode(field: string, value: string) {
		try {
			await navigator.clipboard.writeText(value);
			copiedField = field;
			if (copiedTimer) clearTimeout(copiedTimer);
			copiedTimer = setTimeout(() => {
				copiedField = '';
			}, 2000);
		} catch {
			/* Clipboard nicht verfügbar (unsicherer Kontext) — still: der Wert
			   steht ohnehin als Text auf dem Screen. */
		}
	}

	/** Esc im Cockpit: zuerst das Modal schließen (Kaskade Flow→Liste→Modus
	 *  bleibt beim Vorgänger, der über modalActive angehalten wird). */
	function onCockpitKey(e: KeyboardEvent) {
		if (!modal || e.key !== 'Escape' || e.defaultPrevented) return;
		closeModal();
		e.preventDefault();
	}

	// H-SCLM-UX R1: die CockpitSelect-Vorbelegung ist entfallen — mit dem
	// Inline-Select ist auch sein Session-Wert-Seeding entfallen. Die
	// Settings-Modal-Vorbelegung übernimmt seedFromState (formmodal-Öffnung).

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
			// Sebo-Runde 3 Punkt 1+3: Server-seitiger q-Filter wird jetzt
			// (Manifest ?q={q}) genutzt; zusätzlicher Client-Filter, damit
			// die Vorschläge auch bei Backend-/API-Eigenheiten hart nach dem
			// gerade Tippenden gefiltert bleiben — nie dieselben Statik-Listen.
			const needle = text.trim().toLowerCase();
			acItems = {
				...acItems,
				[rule.field]: rows
					.map((r) => ({
						label: typeof r === 'object' && r !== null ? String((r as Record<string, unknown>).name ?? JSON.stringify(r)) : String(r),
						value: (r ?? {}) as Record<string, unknown>
					}))
					.filter((it) => it.label.toLowerCase().includes(needle))
					.slice(0, 8)
			};
			acOpen = { ...acOpen, [rule.field]: true };
		} catch {
			acItems = { ...acItems, [rule.field]: [] };
		}
	}

	function pickAC(rule: AutocompleteRuleDTO, ac: { label: string; value: Record<string, unknown> }) {
		const filled = applyAutofill(ac.value, rule);
		// Sebo-Runde 3 Punkt 2: Treffer-Label immer ins Auslöser-Feld echoen
		// (Standard-Autocomplete-Verhalten), auch wenn das Manifest-Fill
		// fuer dieses Feld fehlt/leer ist oder der Treffer ein roher String.
		if (!(rule.field in filled)) filled[rule.field] = ac.label;
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
			{#if !cockpit}
				<!-- Cockpit hat permanente Sub-Reiter statt Wizard-Zähler. -->
				<span class="fstep">
					{tr('connector.flow.step', {
						n: String(flow.screens.findIndex((x) => x.id === shown.id) + 1),
						total: String(flow.screens.length)
					})}
				</span>
			{/if}
		</header>
	{/if}

	{#if shown === null}
		<p class="fempty">{tr('error.connector.flow_screen_missing')}</p>
	{:else}
		<!-- SCLM v2 Karte C (Requirement 9): Flows ohne tabs sind strikt
		     linear — Flow-Schritte sind KEINE Tabs (participant flow).
		     Karten-G (Spec 15): flow.tabs rendert permanente Sub-Reiter in
		     bestehender Chip-Reiter-Optik (identisch zu den Legacy-ftabs
		     unten); freier Wechsel jederzeit, jeder Tab-Wechsel fetched den
		     Screen frisch ($effect load(true) auf Screen-Änderung, kein
		     Poll-Cache). Flow ohne tabs = bisheriges Verhalten unverändert. -->
		{#if cockpit}
			<div class="ftabs" role="tablist">
				{#each cockpitTabs as t (t.id)}
					<button
						type="button"
						role="tab"
						aria-selected={activeTabId === t.id}
						class="chip"
						class:is-on={activeTabId === t.id}
						onclick={() => tabGo(t)}
					>{labelOf(t.label, lang)}</button>
				{/each}
			</div>
		{:else if !flow.open}
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
												if (rule) pickAC(rule, item);
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
			{:else if cockpit && shown.type === 'list'}
				<!-- Cockpit-Tab-Liste (Spec 15): Name prominent, Meta-Felder
				     dezent, keine ID-/Claim-Spalten (hiddenMappingKeys zieht
				     Cockpit-Id/Code-Keys zusätzlich ab), Button-Cluster je
				     Zeile (Manifest-strukturklassiert: Umbenennen/Löschen/
				     Stepper/Verkauf…). multi-Screens: Zeilen markierbar
				     (Erfassung für „Teilnehmern zuordnen“). -->
				{#if revenueHead}
					<p class="fmeta frevhead">{tr('connector.flow.creator.revenue_head', { total: formatValue(revenueHead.total), per: revenueHead.per === null ? '—' : formatValue(revenueHead.per) })}</p>
				{/if}
				<div class="fbar">
					{#if shown.multi}
						<span class="fmarkinfo">{tr('connector.flow.marked', { n: String(marked.length), max: String(multiMax ?? '∞') })}</span>
					{/if}
					<span class="fspacer"></span>
					<button type="button" class="frefresh" title={tr('connector.flow.refresh')} aria-label={tr('connector.flow.refresh')} onclick={() => void load(true)}>↻</button>
				</div>
				<ul class="flist">
					{#each rows as row, i (i)}
						{@const pinKey = (shown.pins ?? [])[0]}
						{@const wishMeta = cockpitWishMeta.get(String(row?.[pinKey] ?? ''))}
						{@const btns = cockpitRowBtns(shown)}
						{@const visBtns = btns.filter((p) => !p.a.enabled_if || rowActionEnabledFor(p.a, row))}
						<li>
							<div class="frow frow-static" class:is-marked={!!shown.multi && isMarked(i)}>
								{#if shown.multi}
									<button type="button" class="fcheckbtn" aria-pressed={isMarked(i)} onclick={() => toggleMark(i)}>{isMarked(i) ? '☑' : '☐'}</button>
								{/if}
								<span class="fmain">
									<span class="ftitle">{listColumns.length > 0 ? formatValue(row?.[listColumns[0]]) : ''}</span>
									{#if wishMeta && wishMeta.length > 0}
										<span class="fmeta fwishmeta">{wishMeta.length}× {wishMeta.slice(0, 4).join(' · ')}{wishMeta.length > 4 ? ` +${wishMeta.length - 4}` : ''}</span>
									{/if}
									{#if listColumns.length > 1}
										<span class="fmeta">
											{#each listColumns.slice(1) as field (field)}
												<span class="fmetapair"><span class="fk">{metaFieldLabel(field)}</span>{metaValueText(row?.[field])}</span>
											{/each}
										</span>
									{/if}
								</span>
								<span class="frowbtns">
									{#each cockpitRowNavs.filter((a) => !a.enabled_if || rowActionEnabledFor(a, row)) as a (a.id)}
										<button class="btn-ghost fbtnrow" type="button" disabled={actionBusy} onclick={() => openRowNav(a, row)}>{rowBtnLabel(a)}</button>
									{/each}
									{#each visBtns as p (p.a.id)}
										{#if p.kind === 'stepper'}
											<button class="fstepbtn" type="button" title={tr('connector.flow.creator.dec')} aria-label={tr('connector.flow.creator.dec')} disabled={actionBusy} onclick={() => void stepperRun(p.a, row, p.fieldKey, -1)}>−</button>
											{#if p.fieldKey}
												<span class="fqty">{formatValue(row?.[p.fieldKey])}</span>
											{/if}
											<button class="fstepbtn" type="button" title={tr('connector.flow.creator.inc')} aria-label={tr('connector.flow.creator.inc')} disabled={actionBusy} onclick={() => void stepperRun(p.a, row, p.fieldKey, 1)}>+</button>
										{:else}
											<button class="btn-ghost fbtnrow" type="button" disabled={actionBusy} onclick={() => openRowBtn(p, row)}>{rowBtnLabel(p.a)}</button>
										{/if}
									{/each}
								</span>
							</div>
						</li>
					{:else}
						<li class="fempty">{$t('connector.empty')}</li>
					{/each}
				</ul>
			{:else if cockpit && shown.type === 'results' && !resultsPending}
				<!-- Cockpit Verlosungs-Tab (Spec 15): Teilnehmer-Name prominent,
				     gewonnene Items gruppiert ×N (normalizeResults/groupNames
				     weiterverwendet), Statuschip je gesetztem Flag + Ersteller-
				     „Zurücksetzen“-Button; Abgeholt/Verzichtet nur ohne gesetztes
				     Flag. Nach Status-Schreibaktion: frischer Fetch (load(true))
				     — der Reset-Button erscheint per frischem Fetch zurück.
				     Vor der Ziehung: Hinweis + Start-Button (Confirm-Modal,
				     Überschreib-Warnung falls raffle_done — doneFlag-Warnung im
				     Modal-Handler). -->
				{#if cockpitResultRows.length === 0}
					<p class="fempty">{tr('connector.flow.creator.results_none')}</p>
				{:else}
					<div class="fbar">
						<span class="fspacer"></span>
						<button type="button" class="frefresh" title={tr('connector.flow.refresh')} aria-label={tr('connector.flow.refresh')} onclick={() => void load(true)}>↻</button>
					</div>
					<ul class="flist">
						{#each cockpitResultRows as r, i (i)}
							<li>
								<div class="frow frow-static">
									<span class="fmain">
										<span class="ftitle">{r.name}</span>
										{#if r.groups.length > 0}
											<span class="fmeta">
												{#each r.groups as g (g.name)}
													<span class="fmetapair"><span class="fk">×{g.count}</span>{g.name}</span>
												{/each}
											</span>
										{:else}
											<!-- I-SCLM Cockpit UX R2 (Punkt 5): Teilnehmer ohne
											     Gewinn — dezente Zeile, nirgends Codes. -->
											<span class="fmeta">{tr('connector.flow.creator.results_no_win')}</span>
										{/if}
									</span>
									<span class="frowbtns">
										{#if r.flags.length > 0}
											{#each r.flags as fl (fl)}
												<span class="fstatus chip" role="status">{tr(`connector.flow.status_${fl}`)}</span>
											{/each}
										{/if}
										{#each cockpitRowBtns(shown) as p (p.a.id)}
											{#if p.kind === 'button' && rowActionEnabledFor(p.a, r.row) && (p.a.enabled_if ? true : r.flags.length === 0)}
												<button class="btn-ghost fbtnrow" type="button" disabled={actionBusy} onclick={() => openRowBtn(p, r.row)}>{rowBtnLabel(p.a)}</button>
											{/if}
										{/each}
									</span>
								</div>
							</li>
						{:else}
							<li class="fempty">{$t('connector.empty')}</li>
						{/each}
					</ul>
				{/if}
			{:else if cockpit && shown.type === 'detail'}
				<!-- Cockpit-Übersicht (Spec 15, Korrekturrunde 1 Spec-Abschnitt 16):
				     Codes als Zeile mit Wert + Copy-Button (documentierte
				     Ausnahme der No-Copy-Regel: nur Codes im Creator-Cockpit,
				     nicht in Teilnehmer-Flows), drei Statistikkarten im
				     bestehenden Overlay-Kartenstil. Das bisherige
				     max_wishes-Inline-Select ist entfernt (Duplikat zum
				     Einstellungen-Button; native <select> ist im borderlosen
				     Wayland-Overlay mit der Maus unbedienbar — Sebo P2/P3). -->
				{#if cockpitName}<p class="fmeta fcockpitname">{cockpitName}</p>{/if}
				{#if cockpitCodeRows.length > 0}
					<ul class="fcode-list">
						{#each cockpitCodeRows as c (c.field)}
							<li class="fcode-line">
								<span class="fk">{metaFieldLabel(c.field)}</span>
								<span class="fcode-value">{c.value === null || c.value === undefined || c.value === '' ? '—' : String(c.value)}</span>
								{#if c.value !== null && c.value !== undefined && String(c.value) !== ''}
									<button type="button" class="fcopy" title={tr('connector.flow.copy')} onclick={() => void copyCode(c.field, String(c.value))}>{copiedField === c.field ? tr('connector.flow.copied') : '⧉'}</button>
								{/if}
							</li>
						{/each}
					</ul>
				{/if}
				{#if cockpitStatRows.length > 0}
					<div class="fstatcards">
						{#each cockpitStatRows as s (s.field)}
							<div class="fstatcard">
								<span class="fstatval">{formatValue(s.value)}</span>
								<span class="fstatlabel">{metaFieldLabel(s.field)}</span>
							</div>
						{/each}
					</div>
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
								{@const rowSaleOff = truthyFlagVal(row?.for_sale)}
								<button
									class="frow frow-mark"
									class:is-marked={isMarked(i)}
									class:is-off={rowSaleOff}
									type="button"
									aria-pressed={isMarked(i)}
									disabled={rowSaleOff}
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
										{#if rowSaleOff}<span class="fassign-hint">{tr('connector.flow.creator.sale_hint')}</span>{/if}
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
			{#if cockpit && !resultsPending && cockpitTopPlans.length > 0}
				<!-- Cockpit-Top-Buttons (Spec 15): „+ Teilnehmer“, „Wünsche
				     bearbeiten“, „+ Gegenstand“, „Teilnehmern zuordnen“,
				     „Codes neu generieren“, „Verlosung starten“, Einstellungen.
				     Kurze Labels wo Locale-Key existiert (btn_<goto>), sonst
                     Manifest-Titel; Assign-Button deaktiviert ohne erfasste
                     Zeilen (Cockpit-Liste: Mehrfachauswahl per ☐ in der
                     Zeile — Erfassung für das Zuordnungs-Modal). -->
				<div class="factions">
					{#each cockpitTopPlans as p (p.a.id)}
						<button
							class={p.mode === 'nav' ? 'btn-ghost' : 'btn fsubmit'}
							type="button"
							disabled={actionBusy || (p.mode === 'multimodal' && marked.length === 0)}
							onclick={() => openTopPlan(p)}
						>{topBtnLabel(p.a)}</button>
					{/each}
					{#if actionNote}<p class="fnote">{actionNote}</p>{/if}
				</div>
			{/if}
			{#if !cockpit && !resultsPending && (confirmAction || screenActions.length > 0)}
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

{#if modal}
	<!-- Cockpit-Modal (Spec 15): steht ÜBER der unbewegten Liste; Esc und
	     Backdrop schließen zuerst das Modal (die Esc-Kaskade
	     Flow→Liste→Modus in ConnectorViews wird über modalActive
	     angehalten). notes/warnings im Modal, Fehler bleiben im Modal. -->
	<div class="fmodal" role="dialog" aria-modal="true" aria-label={modal.title}>
		<button class="fmodal-backdrop" type="button" tabindex={-1} aria-label={tr('connector.flow.creator.cancel')} onclick={() => { if (!modalBusy) closeModal(); }}></button>
		<div class="fmodal-card">
			<p class="fmodal-title">{modal.title}</p>
			{#each modal.warnings as w (w)}
				<p class="fmodal-warn">{w}</p>
			{/each}
			{#if modalLoading}
				<p class="fempty">{$t('common.loading')}</p>
			{:else if modal.kind === 'multi'}
				{@const cap = modalCap}
				<p class="fmarkinfo">{tr('connector.flow.marked', { n: String(modalMarked.length), max: String(cap ?? '∞') })}</p>
				{#if modal.confirmStage === 'empty'}
					<p class="fmodal-warn">{tr('connector.flow.creator.clear_confirm')}</p>
				{/if}
				<ul class="flist fmodal-list">
					{#if modal.multi === 'wishes'}
						{@const metaKeys = Object.keys(modal?.target?.mapping ?? {}).filter((k) => k !== (modal?.target?.pins ?? [])[0] && k !== 'name' && /(qty|type|grade|size|wish_count)/i.test(k)).slice(0, 3)}
						{#each modalRows as row, i (i)}
						{@const rowSaleOff = truthyFlagVal(row?.for_sale)}
						<li>
							<button class="frow frow-mark" class:is-marked={isModalMarked(String(i))} class:is-off={rowSaleOff} disabled={rowSaleOff} type="button" aria-pressed={isModalMarked(String(i))} onclick={() => toggleModalMark(String(i), !rowSaleOff)}>
									<span class="fcheck">{isModalMarked(String(i)) ? '☑' : '☐'}</span>
									<span class="fmain">
										<span class="ftitle">{formatValue(row?.name ?? row?.title ?? '')}</span>
										{#if metaKeys.length > 0}
											<span class="fmeta">
												{#each metaKeys as field (field)}
													<span class="fmetapair"><span class="fk">{metaFieldLabel(field)}</span>{metaValueText(row?.[field])}</span>
												{/each}
											</span>
										{/if}
										{#if rowSaleOff}<span class="fassign-hint">{tr('connector.flow.creator.sale_hint')}</span>{/if}
									</span>
								</button>
							</li>
						{:else}
							<li class="fempty">{$t('connector.empty')}</li>
						{/each}
					{:else}
						{#each assignRows as ar (ar.id)}
							<li>
								<button class="frow frow-mark" class:is-marked={isModalMarked(ar.id)} class:is-off={!ar.eligible} disabled={!ar.eligible} type="button" aria-pressed={isModalMarked(ar.id)} onclick={() => toggleModalMark(ar.id, ar.eligible)}>
									<span class="fcheck">{isModalMarked(ar.id) ? '☑' : '☐'}</span>
									<span class="fmain">
										<span class="ftitle">{ar.name}</span>
										<span class="fmeta">
											<span class="fmetapair">{ar.count}/{String(cap ?? '∞')}</span>
											{#if ar.reason}<span class="fassign-hint">{ar.reason}</span>{/if}
										</span>
									</span>
								</button>
							</li>
						{:else}
							<li class="fempty">{$t('connector.empty')}</li>
						{/each}
					{/if}
				</ul>
			{:else if modal.kind === 'prompt'}
				{#each modal.inputs as inp (inp.key)}
					<label class="ffield">
						<span class="flabel">{inp.label}{inp.required ? ' *' : ''}</span>
						{#if inp.type === 'select'}
							<!-- H-SCLM-UX R1 (Sebo P3): Button-Optionen statt
							     native <select> — eine Zeile pro Option,
							     selected-Zustand wie Statuschips. -->
							<div class="foptgroup" role="group" aria-label={inp.label}>
								{#each inp.options ?? [] as o (labelOf(o, lang))}
									{@const ov = labelOf(o, lang)}
									<button class="btn fopt" type="button" aria-pressed={String(inp.value ?? '') === ov} onclick={() => (inp.value = ov)}>{ov}</button>
								{/each}
							</div>
						{:else}
							<input class="finput" type={inp.type} bind:value={inp.value} />
						{/if}
					</label>
				{/each}
			{:else if modal.formSubmit && modal.target?.form}
				{@const userKeys = formUserKeys(modal.target)}
				{#each (modal.target.form.fields ?? []).filter((f) => userKeys.has(f.key)) as f (f.key)}
					<label class="ffield">
						<span class="flabel">{labelOf(f.label, lang)}{f.required === true ? ' *' : ''}</span>
						{#if f.type === 'select'}
							<!-- H-SCLM-UX R1 (Sebo P3): type=select-Felder in Modal-
							     Formularen als Overlay-Button-Optionen (eine Zeile
							     pro Option, selected wie Statuschips, Submit wie
							     bisher) — native <select> ist im borderlosen
							     Wayland-Overlay mit der Maus unbedienbar.
							     Generisch für alle Manifest-selects (aktuell
							     max_wishes 1–6 im Settings-Modal). -->
							<div class="foptgroup" role="group" aria-label={labelOf(f.label, lang)}>
								{#each f.options ?? [] as o (labelOf(o, lang))}
									{@const ov = labelOf(o, lang)}
									<button class="btn fopt" type="button" aria-pressed={String(formValues[f.key] ?? '') === ov} onclick={() => (formValues[f.key] = ov)}>{ov}</button>
								{/each}
							</div>
						{:else}
							<input class="finput" type={f.type === 'number' ? 'number' : 'text'} bind:value={formValues[f.key]} autocomplete="off" oninput={() => { const rule = (modal?.target?.autocomplete ?? []).find((a) => a.field === f.key); if (rule) void searchAC(rule, String(formValues[f.key] ?? '')); }} />
							{#if autofilled[f.key]}<span class="fauto">{tr('connector.flow.autofill_applied')}</span>{/if}
						{/if}
					</label>
					{#if acOpen[f.key] && (acItems[f.key] ?? []).length > 0}
						<ul class="facs">
							{#each acItems[f.key] as item, i (i)}
								<li>
									<button class="fac" type="button" onclick={() => { const rule = (modal?.target?.autocomplete ?? []).find((a) => a.field === f.key); if (rule) pickAC(rule, item); }}>{item.label}</button>
								</li>
							{/each}
						</ul>
					{/if}
				{/each}
			{/if}
			{#if modal.note}<p class="fmodal-warn">{modal.note}</p>{/if}
			<div class="fmodal-actions">
				<button class="btn-ghost" type="button" onclick={() => closeModal()}>{tr('connector.flow.creator.cancel')}</button>
				{#if modal.kind === 'multi'}
					<button class="btn fsubmit" type="button" disabled={modalBusy || modalLoading || (modal.multi === 'assign' && modalMarked.length === 0)} onclick={() => void modalSaveMulti()}>{tr('connector.flow.creator.save')}</button>
				{:else if modal.kind === 'prompt'}
					<button class="btn fsubmit" type="button" disabled={modalBusy} onclick={() => void modalPromptRun()}>{tr('connector.flow.creator.confirm')}</button>
				{:else}
					<button class="btn fsubmit" type="button" disabled={modalBusy} onclick={() => void modalConfirmRun()}>{modal.formSubmit ? tr('connector.flow.creator.save') : tr('connector.flow.creator.confirm')}</button>
				{/if}
			</div>
		</div>
	</div>
{/if}

<svelte:window onkeydown={onCockpitKey} />
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
	/* Karten-G (Spec-Abschnitt 15) + H-SCLM-UX R1 (Spec-Abschnitt 16):
	   Cockpit-Modals + Übersicht-Controls.
	   D-B Punkt 3 unverändert: keine neuen Farben — nur bestehende
	   Theme-Variablen; Scrim = color-mix aus --fg (theme-adaptiv, dunkel
	   wie hell). Listenzeilen/Buttons weiterverwendet: .frow/.btn/.chip-Norm.
	   H-SCLM-UX R1 (Sebo P1/P4-P6, Diagnose A): Verankerung am GEZEICHNETEN
	   Panel statt am Viewport. Das Overlay-Fenster spannt per Hyprland-
	   Dispatcher den ganzen Monitor (overlay.rs), die sichtbare Shellbox ist
	   die .shell-Box (app.css .shell, position:relative). position:fixed
	   verankert am VIEWPORT → das Modal wurde am Monitor-Mittelpunkt
	   zentriert und landete unterhalb/außerhalb der Shell (QW-5-Lektion wie
	   .advert-scrim, app.css: Anker an der Shell-Box, Clip-Verlust der Shell
	   kann ein fixes Modal abschneiden). .flow/.connector/.cmodal-Ebene hat
	   KEINE position — next positioned ancestor ist damit .shell selbst:
	   absolute inset:0 = exakt die gezeichnete Panel-Fläche, Modal mittig.
	   Panel-Wachstum statt fester Modal-Max-Höhe: .shell-cockpitmodal hebt
	   die Shell-min-height, solange ein Cockpit-Modal offen ist
	   (app.css, Muster .shell-info/.shell-advert); die Modal-Karte nutzt
	   max-height:100% der Shell-Fläche, die Liste scrollt innen
	   (.fmodal-list flex-Regel), Aktionen/Buttons bleiben immer erreichbar. */
	.fmodal {
		position: absolute; inset: 0; z-index: 1400;
		display: flex; align-items: center; justify-content: center;
		background: color-mix(in srgb, var(--fg) 35%, transparent);
	}
	/* Backdrop-Schließen als echter Button (a11y): fängt Klicks ab, ist
	   nicht tabbar (tabindex -1) — Esc handled onCockpitKey. */
	.fmodal-backdrop {
		position: absolute; inset: 0; border: 0; padding: 0;
		background: transparent; cursor: default;
	}
	.fmodal-card {
		/* KRITISCH: position:relative — ohne eigenes position läge der
		   absolute .fmodal-backdrop im Paint-Order ÜBER der Karte (abs.
		   Layer > normaler Flow) und jede Maus-Klaus würde closeModal()
		   auslösen (Sebo-Runde-2-Defekt: Modals nur per Tastatur bedienbar).
		   cmodal-card in ConnectorViews macht's seit jeher richtig. */
		position: relative; z-index: 1;
		/* Breite/Überlauf an der Shell ausgerichtet (100% = Shell-Breite),
		   kein 100vw mehr (Viewport > Shell). Zoom-Konvention wie .shell:
		   fester px-Anteil dividiert durch --ui-scale → visuell konstant;
		   max-height 100% = volle Shell-Fläche (Panel-Wachstum übernimmt
		   .shell-cockpitmodal). */
		width: min(calc(430px / var(--ui-scale, 1)), calc(100% - 24px));
		max-height: 100%;
		overflow: auto; display: flex; flex-direction: column; gap: 10px;
		padding: 14px 16px 12px; border: 1px solid var(--lineb);
		border-radius: 10px; background: var(--panel2);
	}
	.fmodal-title { margin: 0; font-weight: 700; font-size: 13.5px; color: var(--fg); }
	.fmodal-warn {
		margin: 0; font-size: 11.5px; color: var(--alert);
		border: 1px solid var(--lineb); border-radius: 7px; padding: 6px 9px;
		background: color-mix(in srgb, var(--alert) 12%, transparent);
	}
	/* Liste im Modal: interner Scroll-Bereich (flex-Regel statt vh-Cap) —
	   Titel/Warnungen/Actions der Karte bleiben sichtbar, Liste dahinter
	   reglos. */
	.fmodal-list { flex: 1 1 auto; min-height: 0; max-height: none; overflow-y: auto; }
	.fmodal-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 2px; }
	/* H-SCLM-UX R1 (Sebo P3): Button-Optionen für type=select in Modal-
	   Formularen — Overlay-Buttons (eine Zeile pro Option) mit selected-
	   Zustand nach Chip-Norm (Akzent-Rand + Akzent-Tönung, nur Theme-Variablen). */
	.foptgroup { display: flex; flex-direction: column; gap: 6px; max-width: 460px; }
	.fopt {
		cursor: pointer; width: 100%; text-align: left; padding: 7px 10px;
		border: 1px solid var(--lineb); border-radius: 7px;
		background: transparent; color: var(--fg); font-size: 12px;
	}
	.fopt:hover { border-color: var(--accent); }
	.fopt[aria-pressed='true'] {
		border-color: var(--accent); font-weight: 700;
		background: color-mix(in srgb, var(--accent) 18%, transparent);
	}
	.frowbtns { display: inline-flex; align-items: center; gap: 6px; flex-wrap: wrap; }
	.fbtnrow { padding: 4px 9px; font-size: 11.5px; }
	.fstepbtn {
		width: 26px; height: 22px; flex: none; cursor: pointer;
		border: 1px solid var(--lineb); border-radius: 6px;
		background: transparent; color: var(--fg);
		font: 600 13px/1 var(--font-mono);
	}
	.fstepbtn[disabled] { opacity: 0.45; cursor: default; }
	.fcheckbtn {
		flex: none; width: 26px; height: 26px; cursor: pointer;
		border: 1px solid var(--lineb); border-radius: 6px;
		background: transparent; color: var(--fg); font-size: 13px;
	}
	.frow-mark:disabled, .frow-mark.is-off { opacity: 0.45; cursor: not-allowed; }
	.fassign-hint { color: var(--fgfaint); font-style: italic; }
	.fcode-list {
		list-style: none; margin: 0 0 12px; padding: 0;
		display: flex; flex-direction: column; gap: 6px;
	}
	.fcode-line {
		display: flex; align-items: center; gap: 10px;
		border: 1px solid var(--lineb); border-radius: 8px; padding: 8px 10px;
	}
	.fcode-line .fk { flex: none; }
	.fcode-value { flex: 1; min-width: 0; overflow-wrap: anywhere; font-family: var(--font-mono); color: var(--fg); }
	.fcopy {
		flex: none; cursor: pointer; padding: 3px 8px; font-size: 11px;
		border: 1px solid var(--lineb); border-radius: 6px;
		background: transparent; color: var(--fgdim);
	}
	.fstatcards {
		display: grid; grid-template-columns: repeat(auto-fit, minmax(110px, 1fr));
		gap: 8px; margin: 0 0 12px;
	}
	.fstatcard {
		display: flex; flex-direction: column; gap: 2px;
		border: 1px solid var(--lineb); border-radius: 8px; padding: 10px 12px;
	}
	.fstatval { font: 700 18px/1.1 var(--font-mono); color: var(--fg); }
	.fstatlabel { font-size: 11px; color: var(--fgdim); }
	.fwishmeta { color: var(--fgdim); }
	.frevhead { color: var(--fg); margin-bottom: 4px; }
	.fcockpitname { margin-bottom: 2px; }
</style>
