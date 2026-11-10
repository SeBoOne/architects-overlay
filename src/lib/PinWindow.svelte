<script lang="ts">
	// M4 (t_7bf507e6): Das PIN-FENSTER-Frontend — rendert NUR gepinnte Boxen
	// (Mockup mockup-pin.html: Kopf Griff|Titel|Kennzahl|Deckkraft|lösen|
	// schließen, inhaltsgetriebene Höhe). Interaktion nur, wenn das Overlay
	// sichtbar ist (Backend setzt set_ignore_cursor_events entsprechend —
	// §9.1.6); im Click-Through-Zustand sind die Buttons reine Anzeige.
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { onMount } from 'svelte';
	import { t } from '../lib/i18n';
	import { PINS_CHANGED_EVENT, type PinEntry, clampBoxToViewport, pinBoxEstHeight } from './pins';

	let { label }: { label: string } = $props();

	let pins = $state<PinEntry[]>([]);
	let interactive = $state(false);
	let dragPin: PinEntry | null = $state(null);
	// Viewport des Pin-Fensters (t_01f1a9ed Punkt 3): Basis für den
	// Box-Clamp — das Fenster sollte zwar den ganzen Screen abdecken
	// (Backend-Geometrie), aber verwahrte Koordinaten dürfen die Box nie
	// vollständig aus dem Viewport treiben.
	let vw = $state(0);
	let vh = $state(0);

	function measureViewport() {
		vw = window.innerWidth;
		vh = window.innerHeight;
	}

	const PRESETS = [20, 55, 100];

	async function load() {
		try {
			pins = await invoke('pins_for_screen', { label });
		} catch {
			pins = [];
		}
	}

	onMount(() => {
		measureViewport();
		window.addEventListener('resize', measureViewport);
		void load();
		void listen(PINS_CHANGED_EVENT, () => void load());
		// Overlay-Sichtbarkeit → Input-Modus-Anzeige (Backend steuert die
		// echten Fenster-Events; hier nur für den Cursor-Kontrast im CSS).
		void listen<{ overlay_visible: boolean }>('ao:pins-mode', (e) => {
			interactive = e.payload.overlay_visible;
		});
	});

	// Drag NUR im Overlay-sichtbaren Zustand (§9.1.6 — Backend sperrt den
	// Input ohnehin); nach dem Loslassen Position zurückmelden → Edge-Snap.
	function onGrab(e: MouseEvent, pin: PinEntry) {
		if (!interactive) return;
		e.preventDefault();
		dragPin = pin;
		const move = (ev: MouseEvent) => {
			if (!dragPin) return;
			dragPin = { ...dragPin, x: pin.x + ev.movementX, y: pin.y + ev.movementY };
		};
		const up = () => {
			const p = dragPin;
			dragPin = null;
			window.removeEventListener('mousemove', move);
			window.removeEventListener('mouseup', up);
			if (p) {
				void invoke('pin_move', {
					view_id: p.view_id,
					screen: p.screen,
					box_index: p.box_index,
					drag: {
						x: p.x,
						y: p.y,
						w: 356,
						h: 44 + 34 * p.rows.length,
						screen_w: window.innerWidth,
						screen_h: window.innerHeight
					}
				});
			}
		};
		window.addEventListener('mousemove', move);
		window.addEventListener('mouseup', up);
	}

	function setOpacity(pin: PinEntry, opacity: number) {
		void invoke('pin_set_opacity', {
			view_id: pin.view_id,
			screen: pin.screen,
			box_index: pin.box_index,
			opacity
		});
	}

	function unpin(pin: PinEntry) {
		void invoke('pin_unpin', {
			view_id: pin.view_id,
			screen: pin.screen,
			box_index: pin.box_index
		});
	}

	function remove(pin: PinEntry) {
		void invoke('pin_remove', {
			view_id: pin.view_id,
			screen: pin.screen,
			box_index: pin.box_index
		});
	}
</script>

<div class="pins-root">
	{#each pins as pin (pin.view_id + ':' + pin.screen + ':' + pin.box_index)}
		{@const pos = clampBoxToViewport(pin.x, pin.y, 356, pinBoxEstHeight(pin.rows.length), vw, vh)}
		<article
			class="pin"
			class:dragging={dragPin === pin}
			style={`left:${pos.x}px; top:${pos.y}px; opacity:${pin.opacity / 100}`}
		>
			<header class="pin-head" title={$t('pins.drag_title')}>
				<button
					type="button"
					class="pin-grip"
					aria-label={$t('pins.drag_title')}
					onmousedown={(e) => onGrab(e, pin)}
				><i></i><i></i><i></i></button>
				<span class="pin-title">{pin.title}</span>
				{#if pin.kpi}<span class="pin-kpi">{pin.kpi}</span>{/if}
				<button
					type="button"
					class="pin-btn"
					title={$t('pins.opacity_title')}
					aria-label={$t('pins.opacity_title')}
					onclick={() => setOpacity(pin, pin.opacity >= 100 ? 20 : pin.opacity >= 55 ? 100 : 55)}
				>◐</button>
				<button
					type="button"
					class="pin-btn"
					title={$t('pins.unpin_title')}
					aria-label={$t('pins.unpin_title')}
					onclick={() => unpin(pin)}
				>⌐</button>
				<button
					type="button"
					class="pin-btn"
					title={$t('pins.close_title')}
					aria-label={$t('pins.close_title')}
					onclick={() => remove(pin)}
				>✕</button>
			</header>
			<div class="pin-body">
				{#each pin.rows as row (row.idx)}
					<div class="pin-step">
						<span class="idx">{row.idx}</span>
						<span class="nm">
							{row.name}
							{#if row.sub}<small>{row.sub}</small>{/if}
						</span>
						<span class="pr {row.side ?? ''}">{row.value}</span>
					</div>
				{/each}
			</div>
			{#if interactive}
				<footer class="pin-foot">
					<span>{$t('pins.opacity')}</span>
					<input
						type="range"
						min="20"
						max="100"
						step="1"
						value={pin.opacity}
						aria-label={$t('pins.opacity_title')}
						oninput={(e) => setOpacity(pin, Number((e.currentTarget as HTMLInputElement).value))}
					/>
					{#each PRESETS as p (p)}
						<button
							type="button"
							class="pin-btn"
							class:active={pin.opacity === p}
							onclick={() => setOpacity(pin, p)}
						>{p}</button>
					{/each}
					<span class="sp"></span>
					<span class="pin-val">{pin.opacity} %</span>
				</footer>
			{/if}
		</article>
	{/each}
	{#if pins.length === 0}
		<p class="pins-empty">{$t('pins.empty')}</p>
	{/if}
</div>

<style>
	/* Token-Erbe: die Shell-Tokens gelten (app.html lädt app.css im selben
	   Webview) — hier nur die Pin-Box selbst, exakt mockup-pin.html. */
	.pins-root {
		position: absolute;
		inset: 0;
		pointer-events: none; /* Fenster-Ebene ist click-through via Backend */
	}
	.pin {
		position: absolute;
		width: 356px;
		max-width: calc(100% - 16px);
		pointer-events: auto;
		background: var(--panel);
		border: 1px solid var(--lineb);
		border-left: 3px solid var(--accent);
		clip-path: polygon(
			12px 0,
			100% 0,
			100% calc(100% - 12px),
			calc(100% - 12px) 100%,
			0 100%,
			0 12px
		);
		box-shadow: 0 16px 40px rgba(0, 0, 0, 0.72);
		transition: opacity 0.12s linear;
	}
	.pin.dragging {
		outline: 1px dashed var(--accent);
	}
	.pin-head {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 7px 8px;
		background: var(--panel2);
		border-bottom: 1px solid var(--accent);
		cursor: grab;
	}
	.pin-grip {
		display: flex;
		gap: 2px;
		flex: 0 0 auto;
		cursor: grab;
		padding: 0;
		background: none;
		border: 0;
	}
	.pin-grip i {
		width: 2px;
		height: 14px;
		background: var(--lineb);
		display: block;
	}
	.pin-title {
		flex: 1;
		min-width: 0;
		font: 700 10.5px/1 var(--font-mono, monospace);
		letter-spacing: 0.14em;
		text-transform: uppercase;
		color: var(--accent);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.pin-kpi {
		font: 700 10.5px/1 var(--font-mono, monospace);
		color: var(--ok);
		white-space: nowrap;
	}
	.pin-btn {
		width: 22px;
		height: 20px;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		background: var(--panel3);
		border: 1px solid var(--lineb);
		color: var(--fgdim);
		font: 700 10px/1 var(--font-mono, monospace);
		cursor: pointer;
	}
	.pin-btn:hover {
		color: var(--fg);
		border-color: var(--fgdim);
	}
	.pin-btn.active {
		color: var(--on-accent);
		background: var(--accent);
		border-color: var(--accent);
	}
	.pin-body {
		padding: 9px 11px 4px;
	}
	.pin-step {
		display: grid;
		grid-template-columns: 22px 1fr auto;
		gap: 9px;
		align-items: baseline;
		padding: 6px 0;
		border-bottom: 1px solid var(--line);
	}
	.pin-step:last-of-type {
		border-bottom: 0;
	}
	.pin-step .idx {
		font: 700 9.5px/1.5 var(--font-mono, monospace);
		color: var(--accent);
		border: 1px solid var(--lineb);
		text-align: center;
	}
	.pin-step .nm {
		min-width: 0;
		font-size: 12.5px;
		font-weight: 600;
		color: var(--fg);
	}
	.pin-step .nm small {
		display: block;
		margin-top: 2px;
		font: 500 10px/1.2 var(--font-mono, monospace);
		color: var(--fgdim);
	}
	.pin-step .pr {
		font: 600 12.5px/1.15 var(--font-mono, monospace);
		white-space: nowrap;
	}
	.pin-step .pr.buy {
		color: var(--buy);
	}
	.pin-step .pr.sell {
		color: var(--sell);
	}
	.pin-foot {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 7px 10px;
		background: var(--panel3);
		border-top: 1px solid var(--lineb);
		font: 700 9.5px/1 var(--font-mono, monospace);
		letter-spacing: 0.1em;
		text-transform: uppercase;
		color: var(--fgfaint);
	}
	.pin-foot .sp {
		flex: 1;
	}
	.pins-empty {
		display: none;
	}
</style>
