<script lang="ts">
	// RB1 (Karte t_723939ff): Kombinierte Handelsrouten-Filter — EIN Eingabefeld
	// pro Auswahl (Schiff / Startort / Zielort) mit inline Typeahead-Dropdown.
	// Tippen filtert, Pfeiltasten navigieren, Enter übernimmt, Esc schließt —
	// Ergonomie wie im Hangar-P10, aber mit echtem Keyboard-Combobox.
	//
	// `value`  = freie Texteingabe (Query + übernommener Wert, bindbar).
	// `options`= gefilterte Kandidaten [{label, value}]; bereits VORHER eingegrenzt
	//           (Schiff: hangarShips/shipMatches; Ort: location_suggest).
	// `onselect(value,label)` = Auswahl übernommen (Schiff → applyShipScu usw.).
	// `onclose` = optionales Signal, wenn das Dropdown schließt (z. B. Fokus-Reset).
	let {
		value = $bindable(''),
		options = [] as { label: string; value: string }[],
		placeholder = '',
		ariaLabel = '',
		fullWidth = false,
		onselect = (_value: string, _label: string) => {},
		onclose = () => {}
	} = $props();

	let open = $state(false);
	let hi = $state(0);

	function clampHi() {
		if (hi >= options.length) hi = Math.max(0, options.length - 1);
	}
	$effect(() => {
		options; // when candidates shrink, keep highlight in range
		if (!open) return;
		clampHi();
	});

	$effect(() => {
		if (value.trim() === '') {
			open = false;
			hi = 0;
		}
	});

	function pick(i: number) {
		if (i < 0 || i >= options.length) return;
		const o = options[i];
		open = false;
		value = o.label;
		onselect(o.value, o.label);
	}

	function onkey(e: KeyboardEvent) {
		const n = options.length;
		if (e.key === 'ArrowDown') {
			if (n) {
				open = true;
				clampHi();
				hi = (hi + 1) % n;
				e.preventDefault();
				e.stopPropagation();
			}
			return;
		}
		if (e.key === 'ArrowUp') {
			if (n) {
				open = true;
				clampHi();
				hi = (hi - 1 + n) % n;
				e.preventDefault();
				e.stopPropagation();
			}
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
				onclose();
				e.preventDefault();
				e.stopPropagation();
			}
			return;
		}
		// Jede andere Taste (Tippen) → Dropdown öffnen/filtern, Cursor ans Ende.
		if (value.trim() !== '') {
			open = true;
			hi = 0;
		}
	}

	// Beim Fokus die Kandidatenliste zeigen (Schiff: alle Hangar-Schiffe;
	// Orte: sobald per Backend geladen). Blur schließt verzögert, damit der
	// Klick auf einen Eintrag noch vor dem Close registriert wird.
	let blurTimer: ReturnType<typeof setTimeout> | null = null;
	function onfocus() {
		open = options.length > 0;
		hi = 0;
	}
	function onblur() {
		blurTimer = setTimeout(() => {
			open = false;
			onclose();
		}, 140);
	}
	function oninput(e: Event) {
		value = (e.currentTarget as HTMLInputElement).value;
		if (value.trim() !== '') {
			open = true;
			hi = 0;
		}
	}
</script>

<div class="rta" style={fullWidth ? 'display:block;width:100%' : ''}>
	<input
		class="rtext rta-input"
		type="text"
		{placeholder}
		aria-label={ariaLabel}
		value={value}
		autocomplete="off"
		role="combobox"
		aria-expanded={open}
		style={fullWidth ? 'width:100%' : ''}
		oninput={oninput}
		onkeydown={onkey}
		onfocus={onfocus}
		onblur={onblur}
	/>
	{#if open && options.length}
		<div class="rta-dd" role="listbox">
			{#each options as o, i (o.value + i)}
				<button
					type="button"
					class="rta-item"
					class:is-hi={i === hi}
					role="option"
					aria-selected={i === hi}
					onclick={() => pick(i)}
				>{o.label}</button>
			{/each}
		</div>
	{/if}
</div>

<style>
	.rta {
		position: relative;
		display: inline-block;
	}
	.rta-input {
		width: 130px;
	}
	.rta-dd {
		position: absolute;
		top: 100%;
		left: 0;
		z-index: 60;
		min-width: 100%;
		max-width: 320px;
		max-height: 280px;
		overflow-y: auto;
		background: var(--panel3);
		border: 1px solid var(--lineb);
		margin-top: 2px;
		display: flex;
		flex-direction: column;
	}
	.rta-item {
		text-align: left;
		border: 0;
		background: transparent;
		color: var(--fg);
		font: 500 12px/1.4 var(--font-mono);
		padding: 5px 8px;
		cursor: pointer;
	}
	.rta-item:hover,
	.rta-item.is-hi {
		background: var(--accent-dim, var(--lineb));
		color: var(--accent);
	}
</style>