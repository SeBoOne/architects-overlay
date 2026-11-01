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
};

export type FormViewDTO = {
	fields: FormFieldDTO[];
	method: 'get' | 'post' | 'patch' | 'put' | 'delete';
	path: string;
	body_template: unknown;
};

export type ViewDTO = {
	id: string;
	type: 'table' | 'cards' | 'detail' | 'calendar' | 'form';
	title: LocalizedText;
	endpoint: string;
	mapping?: Record<string, string> | null;
	poll_seconds?: number | null;
	form?: FormViewDTO | null;
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
	sync?: unknown;
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
	if (Array.isArray(v)) return v.length === 1 ? formatValue(v[0]) : `[${v.length}]`;
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
export function formValue(raw: string, type: FormFieldDTO['type']): string | number {
	return type === 'number' && raw.trim() !== '' ? Number(raw) : raw;
}
