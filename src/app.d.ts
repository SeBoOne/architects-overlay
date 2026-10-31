// See https://svelte.dev/docs/kit/types#app.d.ts
// for information about these interfaces
declare global {
	namespace App {
		// interface Error {}
		// interface Locals {}
		// interface PageData {}
		// interface PageState {}
		// interface Platform {}
	}

	// TW (t_f178b084): Build-Zeit-Konstanten (vite.config.ts define) — Version
	// aus src-tauri/tauri.conf.json, Build-Datum zum Build-Zeitpunkt.
	const __APP_VERSION__: string;
	const __BUILD_DATE__: string;
}

export {};
