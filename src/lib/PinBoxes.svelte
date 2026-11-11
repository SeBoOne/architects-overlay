<script lang="ts">
// M5 (t_d10a0d84): Pins als DOM-Boxen IM SPOTLIGHT-Webview — die früheren
// pins-<idx>-Fenster (M4, verworfen: Wayland/GTK3 input regions unzuverlässig,
// Stacking-Fight) sind entfernt. Diese Komponente migriert den Rendering-Look
// von PinWindow.svelte UND die Interaktion von PinPatches.svelte (M4-E) in
// einen einzigen Layer über der Palette-Ebene.
//
// Vertrag (docs/FENSTERMODUS-PLAN.md §6-Revision M5):
// - Position x/y screen-relativ = Spotlight-Viewport-Koordinaten 1:1 (gleiche
//   Viewport-Basis wie zuvor; patchPos clamp nie vollständig aus dem Screen).
// - INTERAKTIV sobald das Spotlight interaktiv ist (Palette offen, kein
//   Click-Through): Drag auf dem Kopf → pin_move (Edge-Snapping im Backend),
//   Klick → Controller-Popover (Deckkraft-Presets + ±5, Lösen, Schließen —
//   M4-E-Mechanik, Anker-Regeln: Popover als GESCHWISTER der Box unter dem
//   Viewport-Anker-Layer, nie als Kind der Box).
// - Pins-only (Palette verborgen, Pins aktiv): reine Anzeige — die Boxen
//   bleiben sichtbar, pointer-events:none (das Fenster ist komplett
//   durchlässig, `set_ignore_cursor_events(true)` im Backend).
// - Nur Pins mit `pin.screen === spotlight_screen` rendern (der Spotlight
//   deckt exakt diesen Monitor ab).
import { onMount } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { t } from './i18n';
import { listen } from '@tauri-apps/api/event';
import { PINS_CHANGED_EVENT, PIN_BOX_W, pinBoxEstHeight, type PinEntry } from './pins';
import { patchPos, popoverPos } from './pin-patches';

type OverlayState = {
	visible: boolean;
	game_running: boolean;
	click_through: boolean;
};

let { overlay, pinsOnly = false }: { overlay: OverlayState | null; pinsOnly?: boolean } = $props();

let pins = $state<PinEntry[]>([]);
let spotlightScreen = $state('');
let vw = $state(0);
let vh = $state(0);
let dragPin: PinEntry | null = $state(null);
let openKey = $state<string | null>(null);

const PRESETS = [20, 55, 100];

// Interaktions-Gate: Palette offen UND laufendes SC UND kein Click-Through
// UND nicht pins-only (M4-E-Mechanik übernommen + Pins-only-Gate M5).
const interactive = $derived(
	!pinsOnly &&
		overlay !== null &&
		overlay.visible &&
		overlay.game_running &&
		!overlay.click_through
);

// Render-Gate: laufendes SC ist die §9.1.5-Grundbedingung (kein Kanal ohne
// Spiel); im Palette-Zustand zeigen die Boxen ebenfalls (dort interaktiv).
const showBoxes = $derived(overlay !== null && overlay.game_running && pins.length > 0);

function boxKey(pin: PinEntry): string {
	return pin.view_id + ':' + pin.screen + ':' + pin.box_index;
}

// Box-Filter: nur der eigene Spotlight-Screen — die Spot-Viewport-
// Koordinaten decken exakt diesen Monitor ab (Pins anderer Screens bleiben
// ohne Box; das Spot-Overlay sitzt nicht dort).
const boxPins = $derived(pins.filter((p) => p.pinned && p.screen === spotlightScreen));

async function load() {
	try {
		const st = await invoke<{ pins: PinEntry[]; spotlight_screen: string }>('pins_state');
		pins = st.pins;
		spotlightScreen = st.spotlight_screen;
	} catch {
		/* Pins-Backend fehlt — best effort. */
	}
}

function measure() {
	vw = window.innerWidth;
	vh = window.innerHeight;
}

function boxPosFor(pin: PinEntry) {
	return patchPos(pin, vw, vh);
}

// Popover-Position: neben der Box, an den Screen-Rand geclampt (keine
// fixed-Positionierung — Modal-Regeln). Die Koordinaten sind VIEWPORT-
// absolut; das Popover wird deshalb als Geschwister der .pin-Boxen direkt
// unter .pin-layer gerendert (position:absolute; inset:0 = Viewport-Anker),
// NIE als Kind einer Box — sonst doppelter Offset (M4-E Review R1).
function popoverPosFor(pin: PinEntry) {
	return popoverPos(pin, vw, vh);
}

const DRAG_THRESHOLD = 4; // px — darunter gilt der Grab als Klick (Popover)

// Drag NUR bei interaktivem Spotlight (Palette offen); ohne Drag-Bewegung
// (Klick auf den Kopf) öffnet das Controller-Popover.
function onGrab(e: PointerEvent, pin: PinEntry) {
	if (!interactive) return;
	// Kopf-Buttons (◐/⌐/✕) sind keine Drag-Griffe — deren Klick-Aktionen
	// feuern direkt.
	if ((e.target as Element | null)?.closest('.pin-btn')) return;
	e.preventDefault();
	openKey = null;
	dragPin = pin;
	// Delta über clientX/Y (Start-Position merken): ev.movementX/Y ist ein
	// Pointer-Lock-Feature und liefert ohne Pointer-Lock (Wayland/WebKitGTK)
	// konstant 0 — der Drag starb still und jeder Grab öffnete das Popover
	// (Sebo-Befund 2026-11-10). Plus Schwelle: < DRAG_THRESHOLD ist ein Klick.
	const startX = e.clientX;
	const startY = e.clientY;
	let dx = 0;
	let dy = 0;
	const move = (ev: PointerEvent) => {
		if (!dragPin) return;
		dx = ev.clientX - startX;
		dy = ev.clientY - startY;
		dragPin = { ...pin, x: pin.x + dx, y: pin.y + dy };
	};
	const up = (ev: PointerEvent) => {
		const p = dragPin;
		dragPin = null;
		window.removeEventListener('pointermove', move);
		window.removeEventListener('pointerup', up);
		if (!p) return;
		const moved = Math.abs(dx) + Math.abs(dy);
		if (moved > DRAG_THRESHOLD) {
			void invoke('pin_move', {
				view_id: p.view_id,
				screen: p.screen,
				box_index: p.box_index,
				drag: {
					x: p.x,
					y: p.y,
					w: PIN_BOX_W,
					h: pinBoxEstHeight(p.rows.length),
					screen_w: window.innerWidth,
					screen_h: window.innerHeight
				}
			});
			void load();
		} else {
			openKey = boxKey(p);
		}
	};
	window.addEventListener('pointermove', move);
	window.addEventListener('pointerup', up);
}

function cycleOpacity(pin: PinEntry) {
	const next = pin.opacity >= 100 ? 20 : pin.opacity >= 55 ? 100 : 55;
	void invoke('pin_set_opacity', {
		view_id: pin.view_id,
		screen: pin.screen,
		box_index: pin.box_index,
		opacity: next
	});
	void load();
}

function stepOpacity(pin: PinEntry, delta: number) {
	const next = Math.min(100, Math.max(20, pin.opacity + delta));
	void invoke('pin_set_opacity', {
		view_id: pin.view_id,
		screen: pin.screen,
		box_index: pin.box_index,
		opacity: next
	});
	void load();
}

// „Lösen“ (unpin) entfernt (Sebo-Befund 2026-11-10): semantisch identisch
// zu „Schließen“ (pin entfernt, Plugin/View läuft weiter) — zwei Buttons
// für denselben Effekt verwirrten. pin_unpin bleibt im Backend bestehen
// (API-Vertrag), wird im UI aber nicht mehr angeboten.

function remove(pin: PinEntry) {
	void invoke('pin_remove', {
		view_id: pin.view_id,
		screen: pin.screen,
		box_index: pin.box_index
	});
	void load();
}

function onKeydown(e: KeyboardEvent) {
	if (e.key === 'Escape') openKey = null;
}

function onWindowPointerdown(e: PointerEvent) {
	if (!openKey) return;
	// Klick außerhalb des Popovers (im Shell-UI, im Spiel oder auf einer
	// anderen Box) schließt — Klicks im Popover selbst schließen nicht.
	const el = e.target as Element | null;
	if (el && el.closest('.pin-popover')) return;
	openKey = null;
}

onMount(() => {
	measure();
	window.addEventListener('resize', measure);
	void load();
	void listen(PINS_CHANGED_EVENT, () => void load());
});
</script>

<svelte:window onkeydown={onKeydown} onpointerdown={onWindowPointerdown} />

{#if showBoxes}
	<div class="pin-layer" class:display-only={pinsOnly} class:dragging={dragPin !== null}>
		{#each boxPins as pin (boxKey(pin))}
			<!-- Drag-Live-Feedback: der gezogene Pin rendert aus dragPin (clientX-Delta),
			     sonst würde die Box erst beim Release/Reload springen (Sebo-Befund). -->
			{@const live = dragPin && boxKey(dragPin) === boxKey(pin) ? dragPin : pin}
			{@const pos = boxPosFor(live)}
			<article
				class="pin"
				class:interactive
				class:dragging={dragPin === pin}
				style={`left:${pos.x}px; top:${pos.y}px; opacity:${pin.opacity / 100}`}
			>
				<header
					class="pin-head"
					title={$t('pins.drag_title')}
					role="button"
					tabindex="-1"
					aria-label={$t('pins.drag_title')}
					onpointerdown={(e) => onGrab(e, pin)}
				>
					<span class="pin-grip" aria-hidden="true"><i></i><i></i><i></i></span>
					<span class="pin-title">{pin.title}</span>
					{#if pin.kpi}<span class="pin-kpi">{pin.kpi}</span>{/if}
					{#if interactive}
						<button
							type="button"
							class="pin-btn"
							title={$t('pins.opacity_title')}
							aria-label={$t('pins.opacity_title')}
							onclick={() => cycleOpacity(pin)}
						>◐</button>
						<button
							type="button"
							class="pin-btn"
							title={$t('pins.close_title')}
							aria-label={$t('pins.close_title')}
							onclick={() => remove(pin)}
						>✕</button>
					{/if}
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
			</article>
		{/each}
		{#each boxPins as pin (boxKey(pin))}
			{#if openKey === boxKey(pin)}
				<!-- Geschwister der .pin-Boxen unter .pin-layer (Viewport-Anker) —
				     NICHT Kind einer Box, sonst doppelter Offset (M4-E Review R1). -->
				<div
					class="pin-popover"
					style={`left:${popoverPosFor(pin).x}px; top:${popoverPosFor(pin).y}px`}
				>
					<button type="button" class="pbtn" onclick={() => cycleOpacity(pin)} title={$t('pins.opacity_title')}>◐</button>
					<button type="button" class="pbtn" onclick={() => stepOpacity(pin, -5)} title={$t('pins.opacity_title')}>−</button>
					<button type="button" class="pbtn" onclick={() => stepOpacity(pin, 5)} title={$t('pins.opacity_title')}>+</button>
					{#each PRESETS as p (p)}
						<button type="button" class="pbtn" class:active={pin.opacity === p} onclick={() => stepOpacity(pin, p - pin.opacity)} title={$t('pins.opacity_title')}>{p}</button>
					{/each}
					<button type="button" class="pbtn" onclick={() => remove(pin)} title={$t('pins.close_title')}>✕</button>
				</div>
			{/if}
		{/each}
	</div>
{/if}

<style>
	/* Pin-Boxen-Layer: liegt ÜBER der Palette-Ebene (Geschwister VOR .shell —
	   im Markup vor dem Shell-Band), deckt den ganzen Viewport ab, ist selbst
	   pointer-events:none. Interaktive Boxen fangen Pointer selbst; im
	   Pins-only-Zustand (display-only) ist ALLES reine Anzeige. */
	.pin-layer {
		position: absolute;
		inset: 0;
		pointer-events: none;
	}
	.pin {
		position: absolute;
		width: 356px;
		max-width: calc(100% - 16px);
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
	.pin.interactive {
		pointer-events: auto;
	}
	.pin-head {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 7px 8px;
		background: var(--panel2);
		border-bottom: 1px solid var(--accent);
	}
	.pin.interactive .pin-head {
		cursor: grab;
	}
	.pin.interactive.dragging .pin-head {
		cursor: grabbing;
	}
	.pin-grip {
		display: flex;
		gap: 2px;
		flex: 0 0 auto;
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
		padding: 0;
	}
	.pin-btn:hover {
		color: var(--fg);
		border-color: var(--fgdim);
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
	/* Mini-Controller-Popover (M4-E-Mechanik): Overlay-Tokens; Geschwister
	   der .pin-Boxen unter .pin-layer → Viewport-Anker (position:absolute;
	   inset:0) passt zu den viewport-absoluten popoverPos-Koordinaten (kein
	   position:fixed). pointer-events:auto — die Layer selbst ist none, das
	   Popover muss aber klickbar sein. */
	.pin-popover {
		position: absolute;
		pointer-events: auto;
		display: flex;
		gap: 4px;
		align-items: center;
		padding: 4px;
		background: var(--panel);
		border: 1px solid var(--lineb);
		box-shadow: 0 4px 12px rgba(0, 0, 0, 0.5);
		z-index: 10;
	}
	.pbtn {
		width: 24px;
		height: 24px;
		display: grid;
		place-items: center;
		background: var(--panel2);
		border: 1px solid var(--lineb);
		color: var(--fg);
		cursor: pointer;
	}
	.pbtn:hover {
		border-color: var(--accent);
	}
	.pbtn.active {
		border-color: var(--accent);
		color: var(--accent);
	}
</style>
