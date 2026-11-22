<script lang="ts">
	// Globale Reiter-Komponente (M9-R1, einheitliche Reiter-Optik):
	// EINE Quelle für ALLE Tab-Zeilen im Overlay (Mining, Crafting, Flow,
	// Connector, Market) — Segmented-Control-Stil: zusammenhängender Block,
	// aktiver Reiter = gefüllte Akzent-Fläche. Kein Button-Look mehr.
	// M9-R2: Formensprache „Mini-Bevel" — Container mit --bevel-lg-Schräge
	// (Echo der Shellbox), Reiter pur eckig mit Button-Bevel (global).
	type TabItem = { id: string; label: string };
	let {
		items,
		active,
		onSelect,
		ariaLabel = undefined
	}: {
		items: TabItem[];
		active: string;
		onSelect: (id: string) => void;
		ariaLabel?: string;
	} = $props();
</script>

<div class="segtabs" role="tablist" aria-label={ariaLabel}>
	{#each items as it (it.id)}
		<button
			type="button"
			class="segtab"
			class:is-on={active === it.id}
			role="tab"
			aria-selected={active === it.id}
			onclick={() => onSelect(it.id)}>{it.label}</button
		>
	{/each}
</div>

<style>
	/* Segmented Control: zusammenhängender Block, aktiver Reiter gefüllt.
	   Farben NUR aus Shell-Variablen (kein hartes Grau/Akzent). */
	.segtabs {
		display: flex;
		gap: 0;
		flex: 0 0 auto;
		background: color-mix(in srgb, var(--fg, #e8ecf3) 7%, transparent);
		border: 1px solid color-mix(in srgb, var(--fg, #e8ecf3) 12%, transparent);
		padding: 2px;
		width: fit-content;
		max-width: 100%;
		/* M9-R2: Container-Bevel — dieselbe Schrägenrichtung wie die Shellbox */
		clip-path: polygon(
			var(--bevel-lg, 5px) 0,
			100% 0,
			100% calc(100% - var(--bevel-lg, 5px)),
			calc(100% - var(--bevel-lg, 5px)) 100%,
			0 100%,
			0 var(--bevel-lg, 5px)
		);
	}
	.segtab {
		flex: 1;
		border: none;
		background: transparent;
		color: var(--fgdim, #9aa3b2);
		padding: 5px 12px;
		font: inherit;
		font-size: 11.5px;
		letter-spacing: 0.05em;
		text-transform: uppercase;
		cursor: pointer;
		border-radius: 0;
		white-space: nowrap;
	}
	.segtab.is-on {
		background: var(--accent, #4aa3ff);
		color: var(--on-accent, #0b0f14);
		font-weight: 600;
	}
</style>
