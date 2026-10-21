<script lang="ts">
	// Overlay homepage skeleton. Real search/marketplace layout lands with the
	// UEX-Client + Overlay-Fenster tasks.

	// runes mode: stateful value must be $state so updates stay reactive.
	let query = $state('');
</script>

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