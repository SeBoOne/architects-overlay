import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { readFileSync } from 'node:fs';
import { defineConfig } from 'vite';

// TW (t_f178b084): Build-Zeit-Konstanten für das Info-Popup. Version kommt
// aus src-tauri/tauri.conf.json (eine Quelle, keine Doppel-Pflege — die UI
// holt die Laufzeit-Version zusätzlich per getVersion()); das Build-Datum
// wird beim vite-Build festgenagelt (nicht Laufzeit).
const tauriConf = JSON.parse(
	readFileSync(new URL('./src-tauri/tauri.conf.json', import.meta.url), 'utf-8')
);

// Static adapter: the frontend is bundled to ./build and served from inside the
// Tauri webview. No SSR — this is a desktop overlay app.
export default defineConfig({
	// Tauri serves the SvelteKit output; bind to localhost only and ignore the
	// auto-opened browser during `tauri dev`.
	server: {
		port: 5173,
		strictPort: true,
		host: '127.0.0.1'
	},
	clearScreen: false,
	envPrefix: ['VITE_', 'TAURI_ENV_'],
	define: {
		__APP_VERSION__: JSON.stringify(tauriConf.version ?? 'dev'),
		__BUILD_DATE__: JSON.stringify(new Date().toISOString().slice(0, 10))
	},
	plugins: [
		sveltekit({
			compilerOptions: {
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},
			adapter: adapter({ fallback: 'index.html' })
		})
	]
});
