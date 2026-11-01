/**
 * connector — generische Connector-Typen + Helfer (C2, t_91124322).
 *
 * Alles hier ist Manifest-getrieben: KEIN connector-spezifisches Wissen im
 * Overlay-Kern (SCLM/AAWA leben ausschließlich in ihren JSON-Manifesten).
 * Die Rust-Seite (src-tauri/src/connector.rs) liefert die Liste validierter
 * Manifeste + Fetch/Submit mit Header-Auth + TTL-Cache.
 */

export type LocalizedText = { de: string; en: string };

export type PlacementDTO = {
	kind: 'own' | 'replace' | 'embed';
	label?: LocalizedText | null;
	hotkey_slot?: boolean;
	target?: string | null;
};

export type FormFieldDTO = {
	key: string;
	label: LocalizedText;
	type: 'text' | 'number' | 'select';
	required?: boolean;
	options?: LocalizedText[] | null;
	/** Optional: Feldwert zusätzlich als Request-Header senden. */
	header?: string | null;
	/** Optional: Voreinstellung aus der Settings-Whitelist (Backend). */
	default_source?: string | null;
};

export type FormViewDTO = {
	fields: FormFieldDTO[];
	method: 'get' | 'post' | 'patch' | 'put' | 'delete';
	path: string;
	body_template: unknown;
};

export type ParamDTO = {
	key: string;
	label: LocalizedText;
};

export type ViewDTO = {
	id: string;
	type: 'table' | 'cards' | 'detail' | 'calendar' | 'form';
	title: LocalizedText;
	endpoint: string;
	mapping?: Record<string, string> | null;
	poll_seconds?: number | null;
	form?: FormViewDTO | null;
	/** Endpoint-`{param}`-Platzhalter (z. B. Session-Code im Detail-Pfad). */
	params?: ParamDTO[] | null;
	/** Request-Header-Templates (`{param}`) — z. B. X-SCLM-Code pro Aufruf. */
	headers?: Record<string, string> | null;
};

export type ConnectorManifestDTO = {
	manifest_version: number;
	id: string;
	name: LocalizedText;
	base_url: string;
	auth?: {
		type: 'header' | 'none';
		header?: string | null;
		value_source?: string | null;
	} | null;
	placement: PlacementDTO;
	views: ViewDTO[];
	flows?: FlowDTO[] | null;
	sync?: unknown;
};

// ---- SF1 (t_c35bc233): Screen-Flow-Schema — deklarative Screen-Ketten ------
// 1:1 zu crates/connector/src/flow.rs. Alles generisch: KEINE Connector-ID,
// kein connector-spezifisches Wissen im Renderer.

export type ConditionDTO = {
	on: string;
	op: 'equals' | 'not_equals' | 'exists' | 'missing';
	value?: unknown | null;
	then?: string | null;
	otherwise?: string | null;
};

export type AutocompleteRuleDTO = {
	field: string;
	endpoint: string;
	query_param?: string | null;
	params?: ParamDTO[] | null;
	headers?: Record<string, string> | null;
	fill?: Record<string, string> | null;
};

export type ScreenDTO = {
	id: string;
	type: 'code_entry' | 'list' | 'detail' | 'form' | 'wait' | 'results';
	title: LocalizedText;
	endpoint?: string | null;
	mapping?: Record<string, string> | null;
	params?: ParamDTO[] | null;
	headers?: Record<string, string> | null;
	poll_seconds?: number | null;
	form?: FormViewDTO | null;
	pins?: string[] | null;
	when?: ConditionDTO | null;
	on_submit?: string | null;
	on_select?: string | null;
	exit?: ConditionDTO | null;
	autocomplete?: AutocompleteRuleDTO[] | null;
};

export type FlowDTO = {
	id: string;
	title: LocalizedText;
	entry: string;
	screens: ScreenDTO[];
};

export type ConnectorEntry = {
	id: string;
	manifest: ConnectorManifestDTO;
	enabled: boolean;
	auth_set: Record<string, boolean>;
	hotkey: string | null;
};

export type FetchResultDTO = {
	status: number;
	body: unknown;
	mapped: unknown;
	cached: boolean;
	fetched_at_ms: number;
};

/** Lokalisiertes Label: gewählte Sprache, Fallback auf die andere Seite. */
export function labelOf(text: LocalizedText | null | undefined, lang: string): string {
	if (!text) return '';
	if (lang === 'de') return text.de || text.en;
	return text.en || text.de;
}

/** Aktivierte Connectors (Placement-Rendering-Quelle). */
export function activeConnectors(list: ConnectorEntry[] | null): ConnectorEntry[] {
	return (list ?? []).filter((c) => c.enabled);
}

/** placement.kind=own — eigene Top-Level-Bereiche. */
export function ownConnectors(list: ConnectorEntry[] | null): ConnectorEntry[] {
	return activeConnectors(list).filter((c) => c.manifest.placement?.kind === 'own');
}

/** placement.kind=replace mit Ziel-Sektion (z. B. `hangar`). */
export function replaceFor(list: ConnectorEntry[] | null, target: string): ConnectorEntry | null {
	return (
		activeConnectors(list).find(
			(c) => c.manifest.placement?.kind === 'replace' && c.manifest.placement?.target === target
		) ?? null
	);
}

/** placement.kind=embed mit Ziel-Sektion (z. B. `inventory`). */
export function embedFor(list: ConnectorEntry[] | null, target: string): ConnectorEntry | null {
	return (
		activeConnectors(list).find(
			(c) => c.manifest.placement?.kind === 'embed' && c.manifest.placement?.target === target
		) ?? null
	);
}

/** Werte für die generischen Views formatieren (Arrays/Objekte kompakt). */
export function formatValue(v: unknown): string {
	if (v === null || v === undefined) return '—';
	if (typeof v === 'boolean') return v ? '✓' : '✗';
	if (Array.isArray(v)) {
		// Kurze Listen (Namen/IDs) lesbar auflösen — lange als Anzahl.
		if (v.length <= 4 && v.every((x) => typeof x === 'string' || typeof x === 'number')) {
			return v.map((x) => formatValue(x)).join(', ');
		}
		return `[${v.length}]`;
	}
	if (typeof v === 'object') {
		const o = v as Record<string, unknown>;
		if (typeof o.name === 'string') return o.name;
		if (typeof o.title === 'string') return o.title;
		if (typeof o.code === 'string') return o.code;
		return JSON.stringify(v);
	}
	return String(v);
}

/** Spaltenreihenfolge der Tabelle = Manifest-Mapping-Reihenfolge. */
export function tableColumns(view: ViewDTO): string[] {
	return Object.keys(view.mapping ?? {});
}

/** Kalender-Gruppierung: `start`-Mapping-Feld → ISO-Präfix (YYYY-MM-DD). */
export function calendarDay(v: unknown): string {
	const s = formatValue(v);
	return /^\d{4}-\d{2}-\d{2}/.test(s) ? s.slice(0, 10) : s.slice(0, 16);
}

/** Zahlenfelder → Number, Text → String (für form_submit-Werte). */
export function formValue(raw: unknown, type: FormFieldDTO['type']): string | number {
	// number-Inputs binden als number|null (Svelte-Konvention) — String-Felder
	// als string. Beide Formen landen hier; trim darf niemals auf Number knallen.
	const s = typeof raw === 'string' ? raw : String(raw ?? '');
	return type === 'number' && s.trim() !== '' ? Number(s) : s;
}

// ---- C3 (t_d5ca9277): Pin-Speicher für View-Parameter -----------------------
//
// Session-Codes (admin/join/pcode) sind KEINE Auth im Settings-Sinne — sie
// werden pro Connector/View im UI gepinnt. Bewusst NICHT in den Settings
// (Sebo-Regel: Session-Codes sind Werte im Connector-Kontext). Speicher:
// localStorage (`connector.pin.<connector>.<view>.<key>`), in-Memory-Spiegel
// für reaktives Rendern.

const PIN_PREFIX = 'connector.pin.';

export function loadPins(connectorId: string, viewId: string): Record<string, string> {
	if (typeof localStorage === 'undefined') return {};
	try {
		const raw = localStorage.getItem(`${PIN_PREFIX}${connectorId}.${viewId}`);
		return raw ? (JSON.parse(raw) as Record<string, string>) : {};
	} catch {
		return {};
	}
}

export function savePin(
	connectorId: string,
	viewId: string,
	key: string,
	value: string | null
): Record<string, string> {
	if (typeof localStorage === 'undefined') return {};
	const storeKey = `${PIN_PREFIX}${connectorId}.${viewId}`;
	let pins: Record<string, string> = {};
	try {
		pins = loadPins(connectorId, viewId);
		if (value === null || value.trim() === '') delete pins[key];
		else pins[key] = value.trim();
		localStorage.setItem(storeKey, JSON.stringify(pins));
	} catch {
		/* Store voll/gesperrt: Pin bleibt nur für diese Sitzung im State. */
	}
	return pins;
}

// ---- SF1 (t_c35bc233): Flow-Engine-Helfer (Spiegel zu crates/connector/flow.rs) --

/** Dot-Path im JS-Zustand auflösen (`.key`, `[i]`, `[]`-Arrays). */
export function dotPath(state: unknown, path: string): unknown {
	let cur: unknown = state;
	for (const seg of path.split('.')) {
		if (cur === null || cur === undefined) return undefined;
		const m = /^(.*?)\[(\d*)\]$/.exec(seg);
		if (m && m[1]) cur = (cur as Record<string, unknown>)[m[1]];
		if (cur === null || cur === undefined) return undefined;
		if (Array.isArray(cur)) {
			if (m && m[2] !== '') {
				cur = cur[Number(m[2])];
			} else {
				// Iterate: erster Treffer reicht für Bedingungen.
				cur = cur.length > 0 ? cur[0] : undefined;
			}
		}
		if (cur === null || cur === undefined) return undefined;
	}
	return cur;
}

/** Bedingung gegen den Flow-Zustand auswerten (tolerant: fehlend = falsch). */
export function evaluateCondition(c: ConditionDTO, state: Record<string, unknown>): boolean {
	let v: unknown;
	try {
		v = dotPath(state, c.on);
	} catch {
		return false;
	}
	const isNull = v === null || v === undefined;
	switch (c.op) {
		case 'exists':
			return !isNull;
		case 'missing':
			return isNull;
		case 'equals':
			return v === c.value;
		case 'not_equals':
			return v !== c.value;
	}
}

/** Branch-Ziel: `then` wenn wahr, sonst `otherwise`. */
export function branchTarget(c: ConditionDTO, state: Record<string, unknown>): string | null {
	return evaluateCondition(c, state) ? (c.then ?? null) : (c.otherwise ?? null);
}

/** Auto-Fill-Mapping: Treffer-Felder → Formular-Werte (fehlt/nul = nicht füllen). */
export function applyAutofill(item: Record<string, unknown>, rule: AutocompleteRuleDTO): Record<string, string> {
	const out: Record<string, string> = {};
	for (const [field, path] of Object.entries(rule.fill ?? {})) {
		const v = dotPath(item, path);
		if (v !== null && v !== undefined) out[field] = String(v);
	}
	return out;
}

/** `{param}`-Platzhalter auflösen; Unbekannte bleiben stehen (kein Still-Schweigen). */
export function resolvePlaceholders(input: string, bindings: Record<string, string>): string {
	return input.replace(/\{(\w+)\}/g, (full, key) => (key in bindings ? bindings[key] : full));
}

/** Flow-Pins pro Connector+Flow (Zustands-Pinning pro Session, nicht global). */
const FLOW_PIN_PREFIX = 'connector.flow.pin.';
export function loadFlowPins(connectorId: string, flowId: string): Record<string, string> {
	if (typeof localStorage === 'undefined') return {};
	try {
		const raw = localStorage.getItem(`${FLOW_PIN_PREFIX}${connectorId}.${flowId}`);
		return raw ? (JSON.parse(raw) as Record<string, string>) : {};
	} catch {
		return {};
	}
}

export function saveFlowPin(
	connectorId: string,
	flowId: string,
	key: string,
	value: string | null
): Record<string, string> {
	const pins = loadFlowPins(connectorId, flowId);
	if (value === null || value.trim() === '') delete pins[key];
	else pins[key] = value.trim();
	if (typeof localStorage !== 'undefined') {
		try {
			localStorage.setItem(`${FLOW_PIN_PREFIX}${connectorId}.${flowId}`, JSON.stringify(pins));
		} catch {
			/* Store voll/gesperrt: in-memory reicht für die Session. */
		}
	}
	return pins;
}
