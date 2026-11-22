<script lang="ts">
	// M9-K (t_db9b234f, P11) — Custom-Dropdown für den Listen-Umschalter
	// (CraftLists + MiningLists). Ersetzt die Chips-Zeile: eine Zeile zeigt die
	// aktive Liste, Klick öffnet ein DOM-Popover mit allen Listen.
	//
	// KEIN natives <select> — die Overlay-WebView kämpft sonst mit dem
	// Fokus-Duell zwischen WebView und Popup (SelectCombo-Muster, MD t_10be9255).
	// Bedienung: MAUS primär (Klick öffnet, Klick wählt, Klick außerhalb
	// schließt) — das ist der permanente Regressionsfall in den Probes;
	// TASTATUR zusätzlich: Pfeile navigieren, Enter wählt, Esc schließt.
	const uid = $props.id();
	type Item = { id: number; name: string; item_count: number; source_name?: string | null };
	type Props = { items?: Item[]; activeId?: number | null; onopen?: (id: number) => void; emptyHint?: string };
	let {
		items = [],
		activeId = null,
		onopen = (_id: number) => {},
		emptyHint = ''
	}: Props = $props();

	let open = $state(false);
	let hi = $state(0);
	let root = $state<HTMLElement | null>(null);

	const active = $derived(items.find((i) => i.id === activeId) ?? null);

	function toggle() {
		if (!open) hi = Math.max(0, items.findIndex((i) => i.id === activeId));
		open = !open;
	}
	function pick(it: Item) {
		open = false;
		onopen(it.id);
	}
	function onkey(e: KeyboardEvent) {
		if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
			if (!open) {
				open = true;
				hi = Math.max(0, items.findIndex((i) => i.id === activeId));
				e.preventDefault();
				return;
			}
			const n = items.length;
			if (n) {
				hi = e.key === 'ArrowDown' ? (hi + 1) % n : (hi - 1 + n) % n;
				e.preventDefault();
			}
		} else if (e.key === 'Enter') {
			if (open && items[hi]) {
				pick(items[hi]);
				e.preventDefault();
			}
		} else if (e.key === 'Escape') {
			open = false;
		}
	}
	// Klick außerhalb schließt das Popover (Maus-Bedienbarkeit).
	$effect(() => {
		if (!open) return;
		const onDown = (ev: PointerEvent) => {
			if (root && !root.contains(ev.target as Node)) open = false;
		};
		document.addEventListener('pointerdown', onDown, true);
		return () => document.removeEventListener('pointerdown', onDown, true);
	});
</script>

<div
	class="lp"
	bind:this={root}
	role="combobox"
	aria-controls={`${uid}-btn`}
	aria-expanded={open}
	aria-haspopup="listbox"
	onkeydown={onkey}
	tabindex="-1"
>
	<button
		type="button"
		class="lp-btn"
		id={`${uid}-btn`}
		aria-controls={`${uid}-pop`}
		onclick={toggle}
	>
		<span class="lp-label">
			{active ? active.name : emptyHint || '—'}
			{#if active}<span class="lp-count">{active.item_count}</span>{/if}
		</span>
		<span class="lp-chev" class:is-open={open}>▾</span>
	</button>
	{#if open}
		<div class="lp-pop" id={`${uid}-pop`} role="listbox" aria-label={emptyHint}>
			{#each items as it, i (it.id)}
				<button
					type="button"
					role="option"
					aria-selected={it.id === activeId}
					class="lp-row"
					class:is-on={it.id === activeId}
					class:is-hi={i === hi}
					onclick={() => pick(it)}
					title={it.source_name
						? `${it.name} — ${it.source_name}`
						: it.name}
				>
					<span class="lp-row-name">{it.name}</span>
					{#if it.source_name}<span class="lp-row-src">{it.source_name}</span>{/if}
					<span class="lp-count">{it.item_count}</span>
				</button>
			{:else}
				<div class="lp-empty">{emptyHint}</div>
			{/each}
		</div>
	{/if}
</div>

<style>
	.lp {
		position: relative;
		display: inline-flex;
		min-width: 0;
	}
	.lp-btn {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		background: rgba(255, 255, 255, 0.05);
		border: 1px solid rgba(255, 255, 255, 0.14);
		border-radius: 8px;
		color: inherit;
		padding: 5px 10px;
		font: inherit;
		font-size: 12px;
		cursor: pointer;
		max-width: 100%;
		min-width: 0;
	}
	.lp-label {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		min-width: 0;
	}
	.lp-count {
		font-size: 10px;
		padding: 1px 6px;
		border-radius: 999px;
		background: rgba(255, 255, 255, 0.08);
		border: 1px solid rgba(255, 255, 255, 0.12);
	}
	.lp-chev {
		opacity: 0.65;
		font-size: 10px;
		transform: rotate(0deg);
		transition: transform 0.12s ease;
	}
	.lp-chev.is-open {
		transform: rotate(180deg);
	}
	.lp-pop {
		position: absolute;
		top: calc(100% + 4px);
		left: 0;
		z-index: 30;
		display: flex;
		flex-direction: column;
		min-width: 220px;
		max-width: 320px;
		max-height: 60vh;
		overflow-y: auto;
		background: #131922;
		border: 1px solid rgba(255, 255, 255, 0.16);
		border-radius: 8px;
		box-shadow: 0 8px 24px rgba(0, 0, 0, 0.55);
		padding: 4px;
	}
	.lp-row {
		display: flex;
		align-items: center;
		gap: 8px;
		background: transparent;
		border: 0;
		border-radius: 6px;
		color: inherit;
		font: inherit;
		font-size: 12px;
		padding: 6px 8px;
		cursor: pointer;
		text-align: left;
		white-space: nowrap;
	}
	.lp-row.is-hi {
		background: rgba(255, 255, 255, 0.07);
	}
	.lp-row.is-on {
		background: var(--accent, #4aa3ff);
		color: #0b0f14;
	}
	.lp-row-name {
		overflow: hidden;
		text-overflow: ellipsis;
		min-width: 0;
	}
	.lp-row-src {
		opacity: 0.6;
		font-size: 10px;
		overflow: hidden;
		text-overflow: ellipsis;
		min-width: 0;
	}
	.lp-row .lp-count {
		margin-left: auto;
	}
	.lp-row.is-on .lp-count {
		background: rgba(0, 0, 0, 0.18);
		border-color: transparent;
	}
	.lp-empty {
		font-size: 12px;
		opacity: 0.6;
		padding: 8px;
	}
</style>
