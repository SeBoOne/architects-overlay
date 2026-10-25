<script lang="ts">
	// Rail — linke Modusleiste des Overlays (Karte t_315a26cc).
	//
	// Schmale vertikale Leiste am linken Rand der Shell: ein Eintrag je Modus
	// mit Icon + F-Key-Label, aktiver Modus in der Palette-Akzentfarbe. Klick
	// und die F-Taste tun dasselbe (setMode in +page.svelte).
	//
	// Die Rail gehört NUR zum Overlay — sie liegt innerhalb der Shell, der
	// transparente Hintergrund des Spotlight-Fensters bleibt unberührt.
	// Bei schmaler Shell kollabiert sie auf reine Icons (`compact` vom
	// Elternteil per bind:clientWidth gemessen).
	type Mode = 'search' | 'market' | 'routes' | 'inventory' | 'hangar' | 'settings';
	type Entry = {
		id: Mode;
		key: string;
		label: string;
		title: string;
		icon: 'search' | 'market' | 'routes' | 'inventory' | 'hangar' | 'settings';
	};

	let { mode, compact = false, onselect }: { mode: Mode; compact?: boolean; onselect: (m: Mode) => void } =
		$props();

	// Inventar/Hangar: eigene Karten — hier nur Rail-Eintrag + Platzhalter-View.
	const ENTRIES: Entry[] = [
		{ id: 'search', key: 'F1', label: 'Suche', title: 'Suche (F1)', icon: 'search' },
		{ id: 'market', key: 'F2', label: 'Marktplatz', title: 'Marktplatz (F2)', icon: 'market' },
		{ id: 'routes', key: 'F6', label: 'Routen', title: 'Handelsrouten (F6)', icon: 'routes' },
		{ id: 'inventory', key: 'F3', label: 'Inventar', title: 'Inventar — kommt bald (F3)', icon: 'inventory' },
		{ id: 'hangar', key: 'F4', label: 'Hangar', title: 'Hangar — kommt bald (F4)', icon: 'hangar' },
		{ id: 'settings', key: 'F12', label: 'Einstellungen', title: 'Einstellungen (F12)', icon: 'settings' }
	];
</script>

<nav class="rail" class:is-compact={compact} aria-label="Modi">
	{#each ENTRIES as en (en.id)}
		<button
			type="button"
			class="rentry"
			class:is-on={mode === en.id}
			onclick={() => onselect(en.id)}
			title={en.title}
			aria-label={en.title}
			aria-current={mode === en.id ? 'true' : undefined}
		>
			<span class="ric">
				{#if en.icon === 'search'}
					<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round">
						<circle cx="10.5" cy="10.5" r="6.5" />
						<path d="M15.4 15.4 21 21" />
					</svg>
				{:else if en.icon === 'market'}
					<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
						<path d="M3 6h18l-1.5 11a2 2 0 0 1-2 1.7H6.5a2 2 0 0 1-2-1.7L3 6Z" />
						<path d="M8.5 6V5a3.5 3.5 0 0 1 7 0v1" />
					</svg>
				{:else if en.icon === 'routes'}
					<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
						<circle cx="5" cy="19" r="2.2" />
						<circle cx="19" cy="5" r="2.2" />
						<path d="M7 19h8a3 3 0 0 0 3-3V7.2" />
						<path d="m15.9 9.3 2.1-2.1 2.1 2.1" />
					</svg>
				{:else if en.icon === 'inventory'}
					<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
						<path d="M21 8.2 12 13 3 8.2V5.8L12 10.6l9-4.8v2.4Z" />
						<path d="M21 13.2 12 18 3 13.2" />
					</svg>
				{:else if en.icon === 'hangar'}
					<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
						<path d="M3 21V9l9-5 9 5v12" />
						<path d="M8 21v-8h8v8" />
						<path d="M12 4v5" />
					</svg>
				{:else}
					<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
						<circle cx="12" cy="12" r="3" />
						<path
							d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
						/>
					</svg>
				{/if}
			</span>
			<span class="rkey">{en.key}</span>
			<span class="rlbl">{en.label}</span>
		</button>
	{/each}
</nav>
