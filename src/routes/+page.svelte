<script lang="ts">
	// Overlay homepage skeleton. Two views share this route:
	//  - main window: full search shell (one `search(entities, query)` per the
	//    DataProvider seam, filled by the UEX-Client task)
	//  - spotlight window: compact, click-through-aware palette that sits over
	//    Star Citizen (toggled with Strg+Shift+U, click-through via Strg+Shift+O)
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';

	type OverlayState = { visible: boolean; game_running: boolean; click_through: boolean };

	// Mirrors OVERLAY_STATE_EVENT in src-tauri/src/overlay.rs — keep in sync.
	const OVERLAY_STATE_EVENT = 'overlay-state-changed';

	let query = $state('');
	let isSpotlight = $state(false);
	let overlay = $state<OverlayState | null>(null);

	/** Recognise which Tauri window we run in (browser dev tabs stay on main). */
	async function boot() {
		try {
			const label = getCurrentWindow().label;
			if (label === 'spotlight') {
				isSpotlight = true;
				// Initial snapshot, then stay live via push events from Rust.
				overlay = await invoke<OverlayState>('overlay_get_state');
				await listen<OverlayState>(OVERLAY_STATE_EVENT, (e) => {
					overlay = e.payload;
				});
			}
		} catch {
			// Not running inside Tauri (plain vite dev tab) — keep main shell.
		}
	}
	void boot();

	async function toggleClickThrough() {
		if (!overlay) return;
		overlay = await invoke<OverlayState>('overlay_get_state');
		const ignore = !overlay.click_through;
		await invoke<boolean>('overlay_set_click_through', { ignore });
		overlay = await invoke<OverlayState>('overlay_get_state');
	}
</script>

{#if isSpotlight}
	<main class="spotlight">
		<header class="spotlight-header">
			<span class="spotlight-title">SC ARCHITECTS</span>
			<span
				class="spotlight-dot"
				class:live={overlay?.game_running}
				title={overlay?.game_running ? 'Star Citizen erkannt' : 'Star Citizen nicht erkannt'}
			></span>
		</header>

		<section class="spotlight-body">
			{#if overlay?.game_running}
				<p class="muted">Bereit. Warensuche folgt mit dem UEX-Client.</p>
			{:else}
				<p class="faint">Warte auf Star Citizen … (Prozess-Scan aktiv)</p>
			{/if}
		</section>

		<footer class="spotlight-footer">
			{#if overlay?.click_through}
				<span class="faint">Klick-frei · Maus geht an das Spiel</span>
			{:else}
				<span class="muted">Fokus · Overlay ist klickbar</span>
			{/if}
			<button onclick={toggleClickThrough} disabled={!overlay?.game_running}>
				{overlay?.click_through ? 'Fokus an (Strg+Shift+O)' : 'Klick-frei an (Strg+Shift+O)'}
			</button>
		</footer>
	</main>
{:else}
	<main class="app-root">
		<header class="app-header">SC Architects Overlay</header>

		<section class="app-body">
			<input
				bind:value={query}
				type="text"
				placeholder="Suche nach Ware, Item, Ort …  (Strg+Shift+U)"
			/>

			<!--
				Ergebnisliste: Platz für `search(entities, query)` aus dem
				DataProvider. Wird mit dem UEX-Client-Task gefüllt.
			-->
			{#if query}
				<ul>
					<li>…</li>
				</ul>
			{:else}
				<p class="muted">Kein Suchbegriff — tippe, um zu suchen.</p>
			{/if}
		</section>

		<footer class="app-footer">
			<span class="faint">Offline · Provider: –</span>
		</footer>
	</main>
{/if}
