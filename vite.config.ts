import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

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