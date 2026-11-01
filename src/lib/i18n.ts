// i18n — T1 (t_530e5f59): Overlay-Lokalisierung mit Svelte-Store, bewusst
// OHNE svelte-i18n-Package (Sebo-Beschluss: transparente eigene t()-Funktion).
//
// Konzept:
//   - Built-in Sprachen (en, de) werden STATISCH importiert (Vite bündelt
//     sie in den JS-Chunk) — der Live-Switch braucht keinen IPC-Roundtrip
//     und funktioniert auch ohne Tauri (SSR/dev/probe).
//   - Zusätzlich lokale (Community-)Dateien: Nutzer importiert per
//     Datei-Button in der Settings-UI; der Text geht an `locale_import`
//     (Rust validiert + persistiert unter <app-local-data>/locales/),
//     `locale_list` liefert sie beim Boot zurück.
//   - Fallback-Kette: gewählte Sprache → en → raw Key. Fehlende Keys sind
//     KEIN Fehler — sie werden im Debug-Log notiert (einmal pro Key) und
//     die Fallback-Übersetzung geliefert.
//   - Live-Wechsel: `t` ist ein abgeleiteter Store über [locales, lang];
//     jede `$t(...)`-Benutzung im Markup zieht beim Umschalten sofort nach.
//     Persistiert wird der Code via `settings_set_language` (app_settings).
//
// Key-Format: flach mit Punktnamen ("settings.language.title"), Interpolation
// über {name}-Platzhalter (t('key', { name: 'Deutsch' })). Keine Plural-
// Forms-Engine — bei Bedarf Key-Suffixe `_one`/`_other` (Spec-Beschluss).

import en from '../../resources/locales/en.json';
import de from '../../resources/locales/de.json';
import { invoke } from '@tauri-apps/api/core';
import { derived, get, writable } from 'svelte/store';

export type LocaleMeta = {
	code: string;
	name: string;
	builtin: boolean;
};

export type LocaleEntry = {
	meta: LocaleMeta;
	/** flaches Key→Text-Map ("settings.language.title" → "Language") */
	strings: Record<string, string>;
};

export type TranslateParams = Record<string, string | number>;

/** Fallback-Sprache (letzte echte Stufe vor dem raw Key). */
export const FALLBACK_LANG = 'en';

const BUILTINS: LocaleEntry[] = [en, de].map((file) => ({
	meta: { code: file.meta.code, name: file.meta.name, builtin: true },
	strings: file.strings
}));

/** Alle bekannten Sprachen (built-ins + lokal importierte), keyed by code. */
const locales = writable<Record<string, LocaleEntry>>(
	Object.fromEntries(BUILTINS.map((l) => [l.meta.code, l]))
);

/** Aktive Sprache (Persistenz via settings_set_language). */
const lang = writable<string>(FALLBACK_LANG);

/**
 * Die t-Funktion als abgeleiteter Store: `$t('nav.trade')` im Markup.
 * Reaktiv — Sprache umschalten wirkt ohne Neustart auf ALLE Views.
 */
export const t = derived([locales, lang], ([$locales, $lang]) => {
	const warned = new Set<string>();
	return function translate(key: string, params?: TranslateParams): string {
		const resolve = (code: string): string | undefined =>
			$locales[code]?.strings?.[key];
		let text = resolve($lang);
		if (text === undefined && $lang !== FALLBACK_LANG) {
			// Stufe 2: en — fehlende Keys im gewählten Locale sind normal,
			// KEIN Fehler (Spec T1).
			text = resolve(FALLBACK_LANG);
			if (text !== undefined && !warned.has(key)) {
				warned.add(key);
				console.debug(`[i18n] missing "${key}" in "${$lang}" → en fallback`);
			}
		}
		if (text === undefined) {
			// Stufe 3: raw Key (auch für en fehlend) — Debug-Log statt Fehler.
			if (!warned.has(key)) {
				warned.add(key);
				console.debug(`[i18n] missing "${key}" everywhere → raw key shown`);
			}
			text = key;
		}
		if (!params) return text;
		return text.replace(/\{(\w+)\}/g, (m, p: string) =>
			Object.prototype.hasOwnProperty.call(params, p) ? String(params[p]) : m
		);
	};
});

/**
 * Skript-seitige Übersetzung (Event-Handler, Toasts): liest den Store-Stand
 * im Augenblick des Aufrufs (get(t)) — im Markup `$t(...)` verwenden, dort
 * ist der Wechsel reaktiv.
 */
export function tr(key: string, params?: TranslateParams): string {
	return get(t)(key, params);
}

/**
 * Fehlertext aus Rust (T1-Muster: error_key + params): Rust liefert als
 * `Err` den String `{"error_key": "...", "params": {...}}`; Klartext-Fehler
 * (alter Stil, T2 migriert sie schrittweise) fallen als Rohstring durch.
 */
export function trError(e: unknown): string {
	const raw = typeof e === 'string' ? e : e instanceof Error ? e.message : String(e);
	try {
		const parsed = JSON.parse(raw);
		if (parsed && typeof parsed === 'object' && typeof parsed.error_key === 'string') {
			return get(t)(parsed.error_key, parsed.params ?? {});
		}
	} catch {
		/* kein JSON → Klartext durchreichen */
	}
	return raw;
}

/** Metas der geladenen Sprachen für die Auswahl-UI (Built-ins zuerst). */
export const localeList = derived([locales, lang], ([$locales, $lang]) => {
	const all = Object.values($locales);
	all.sort((a, b) =>
		a.meta.builtin === b.meta.builtin
			? a.meta.code.localeCompare(b.meta.code)
			: a.meta.builtin
				? -1
				: 1
	);
	return { locales: all, current: $lang };
});

/** Sprach-Code im Store wechseln (LIVE — kein Neustart). Persistenz ruft die
 *  UI separat via `setLanguagePersisted` (invoke zuerst, Store danach). */
export function setLanguage(code: string): void {
	lang.set(code);
}

/** Sprache persistieren + Store umschalten (UI-Ablauf: invoke zuerst —
 *  scheitert der Setter, bleibt die alte Sprache aktiv). */
export async function setLanguagePersisted(code: string): Promise<void> {
	await invoke('settings_set_language', { code });
	setLanguage(code);
}

/**
 * Lokale Sprachdatei importieren: JSON-Text aus dem File-Input an
 * `locale_import` (Rust validiert: Größe/JSON/meta/strings, built-in-Codes
 * geschützt) und im Store registrieren. Fehler sind locale.key-basiert
 * (`LocaleError`-JSON) → `trError` übersetzt sie.
 */
export async function importLocaleFromText(text: string): Promise<LocaleMeta> {
	// locale_import liefert das validierte FLAT LocaleMeta ({code, name,
	// builtin}) — die Strings hat Rust denselben Text bereits aus validiert;
	// für den Store parsen wir sie client-seitig aus demselben Text (ein
	// zweiter IPC wäre Roundtrip für Daten, die schon hier sind).
	const meta: LocaleMeta = await invoke('locale_import', { text });
	let strings: Record<string, string> = {};
	try {
		const doc = JSON.parse(text);
		if (doc && typeof doc.strings === 'object' && doc.strings !== null) {
			strings = doc.strings;
		}
	} catch {
		/* Rust hat den Text bereits validiert — defensiv leer lassen. */
	}
	locales.update((map) => ({
		...map,
		[meta.code]: { meta: { ...meta, builtin: false }, strings }
	}));
	return meta;
}

/**
 * Boot-Initialisierung: persistierte Sprache + lokale Dateien aus dem
 * Backend holen. Ohne Tauri (SSR/dev) bleibt der Stand: en + Built-ins.
 * Wird aus `boot()` der +page.svelte nach `loadSettings()` gerufen.
 */
export async function initI18n(persistedLang?: string): Promise<void> {
	try {
		const local = await invoke<{ meta: LocaleMeta; strings: Record<string, string> }[]>(
			'locale_list'
		);
		if (local.length) {
			locales.update((map) => {
				const next = { ...map };
				for (const entry of local) {
					next[entry.meta.code] = {
						meta: { ...entry.meta, builtin: false },
						strings: entry.strings
					};
				}
				return next;
			});
		}
	} catch {
		/* Ohne Tauri/Backend → nur Built-ins. */
	}
	if (persistedLang) lang.set(persistedLang);
}

/** Hilfsfunktion für Probe-Tests: direkter Store-Zugriff (nicht UI-Flow). */
export function currentLang(): string {
	let cur = '';
	lang.subscribe((v) => (cur = v))();
	return cur;
}
