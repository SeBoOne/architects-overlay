<script lang="ts">
	// MD (t_10be9255, D2): Dunkles, mausbedienbares Dropdown mit Typeahead —
	// ersetzt die nativen <select> im Listing-Formular (Listing Type, Currency,
	// Einheit, Kategorie, Laufzeit, Item Source, Availability). Kombiniert:
	// - MAUS: Klick auf Eingabefeld/Chevron klappt auf, Klick auf Eintrag wählt.
	// - TASTATUR: Tippen filtert (Typeahead), Pfeile navigieren, Enter wählt,
	//   Esc schließt.
	// - Optik im Overlay-Stil: dunkel, gleiche Schrift, Hover/Selection wie
	//   RoutesTypeahead (rta-dd).
	// Kontrakt wie RoutesTypeahead: `value` bindbar, `onselect(value,label)`.
	const uid = $props.id();
	let {
		value = $bindable(''),
		options = [] as { value: string; label: string }[],
		placeholder = '',
		ariaLabel = '',
		disabled = false,
		onselect = (_value: string, _label: string) => {}
	} = $props();

	let open = $state(false);
	let hi = $state(0);
	let query = $state('');

	// Synchronisiere das Eingabefeld mit dem gewählten Wert (Label), wenn von
	// außen gesetzt (z. B. Vorbefüllung beim Bearbeiten oder Unit-Wechsel).
	let lastExternal = value;
	$effect(() => {
		if (value !== lastExternal) {
			lastExternal = value;
			const o = options.find((c) => c.value === value);
			query = o ? o.label : value;
		}
	});
	// Initial-Label beim Mount/Option-Wechsel (z. B. CURRENCY_CHOICES laden).
	$effect(() => {
		const o = options.find((c) => c.value === value);
		if (value !== '' && query === '') query = o ? o.label : value;
	});

	let filtered = $derived.by(() => {
		const q = query.trim().toLowerCase();
		// Der aktuell gewählte Wert bleibt sichtbar (auch wenn er nicht dem
		// Filtertext entspricht), sonst alles per Query gefiltert.
		if (q === '') return options;
		return options.filter((c) => c.label.toLowerCase().includes(q) || c.value === value);
	});

	$effect(() => {
		if (hi >= filtered.length) hi = Math.max(0, filtered.length - 1);
	});

	function pick(i: number) {
		if (i < 0 || i >= filtered.length) return;
		const o = filtered[i];
		value = o.value;
		lastExternal = o.value;
		query = o.label;
		open = false;
		onselect(o.value, o.label);
	}

	function onkey(e: KeyboardEvent) {
		const n = filtered.length;
		if (e.key === 'ArrowDown') {
			open = true;
			hi = n ? (hi + 1) % n : 0;
			e.preventDefault();
			e.stopPropagation();
			return;
		}
		if (e.key === 'ArrowUp') {
			open = true;
			hi = n ? (hi - 1 + n) % n : 0;
			e.preventDefault();
			e.stopPropagation();
			return;
		}
		if (e.key === 'Enter') {
			if (open && n) {
				e.preventDefault();
				e.stopPropagation();
				pick(hi);
			}
			return;
		}
		if (e.key === 'Escape') {
			if (open) {
				open = false;
				e.preventDefault();
				e.stopPropagation();
			}
			return;
		}
		// jede andere Taste: Dropdown offen halten (Filter via oninput)
		open = true;
		hi = 0;
	}

	function oninput(e: Event) {
		query = (e.currentTarget as HTMLInputElement).value;
		open = true;
		hi = 0;
	}

	let blurTimer: ReturnType<typeof setTimeout> | null = null;
	function onfocus() {
		if (disabled) return;
		open = true;
		hi = 0;
	}
	function onblur() {
		blurTimer = setTimeout(() => {
			open = false;
			// Text ohne Auswahl zurück auf das Label des gewählten Werts.
			const o = options.find((c) => c.value === value);
			query = o ? o.label : value;
		}, 140);
	}
	function toggle(e: MouseEvent) {
		if (disabled) return;
		e.stopPropagation();
		if (blurTimer) clearTimeout(blurTimer);
		open = !open;
		hi = 0;
		if (open) (e.currentTarget as HTMLElement).parentElement?.querySelector('input')?.focus();
	}
</script>

<div class="scb" class:is-disabled={disabled}>
	<input
		class="rtext scb-input"
		type="text"
		{placeholder}
		aria-label={ariaLabel}
		aria-disabled={disabled}
		role="combobox"
		aria-expanded={open}
		aria-controls={open ? uid + '-dd' : undefined}
		autocomplete="off"
		bind:value={query}
		oninput={oninput}
		onkeydown={onkey}
		onfocus={onfocus}
		onblur={onblur}
		onclick={() => (open = true)}
	/>
	<button type="button" class="scb-chev" tabindex="-1" aria-label="Ausklappen" onclick={toggle}>▾</button>
	{#if open && !disabled && filtered.length}
		<div class="scb-dd" role="listbox" id={uid + '-dd'}>
			{#each filtered as o, i (o.value + '|' + o.label)}
				<button
					type="button"
					class="scb-item"
					class:is-hi={i === hi}
					role="option"
					aria-selected={o.value === value}
					onmousedown={(e) => e.preventDefault()}
					onclick={() => pick(i)}
				>{o.label}</button>
			{/each}
		</div>
	{/if}
</div>

<style>
	.scb {
		position: relative;
		display: inline-flex;
		align-items: stretch;
	}
	.scb.is-disabled {
		opacity: 0.55;
	}
	.scb-input {
		width: 150px;
		border-radius: 0;
	}
	.scb-input:focus {
		border-color: var(--accent);
	}
	.scb-chev {
		border: 1px solid var(--lineb);
		border-left: 0;
		background: var(--panel);
		color: var(--fgdim);
		font: 700 10px/1 var(--font-mono);
		padding: 0 7px;
		cursor: pointer;
	}
	.scb-chev:hover {
		color: var(--accent);
		border-color: var(--accent);
	}
	.scb-dd {
		position: absolute;
		top: 100%;
		left: 0;
		z-index: 60;
		min-width: 100%;
		max-width: 340px;
		max-height: 240px;
		overflow-y: auto;
		background: var(--panel3);
		border: 1px solid var(--lineb);
		margin-top: 2px;
		display: flex;
		flex-direction: column;
		scrollbar-width: thin;
	}
	.scb-item {
		text-align: left;
		border: 0;
		background: transparent;
		color: var(--fg);
		font: 500 12px/1.4 var(--font-mono);
		padding: 5px 8px;
		cursor: pointer;
		white-space: nowrap;
	}
	.scb-item:hover,
	.scb-item.is-hi {
		background: var(--accent-dim, var(--lineb));
		color: var(--accent);
	}
</style>
