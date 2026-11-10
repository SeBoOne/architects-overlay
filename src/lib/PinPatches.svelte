<script lang="ts">
// M4-E (t_34efb781): Spotlight-PATCHES — unsichtbare Interaktionsflächen im
// SPOTLIGHT-Webview für die gepinnten Boxen des eigenen Screens.
//
// Vertrag (§9.1.6 präzisiert + docs/FENSTERMODUS-PLAN.md §6):
// - Das PIN-FENSTER ist REIN ANZEIGE (dauerhaft input-transparent); das
//   Spotlight hebt sich beim show() über gleichrangige always-on-top-
//   Fenster und verschluckte früher alle Klicks auf den Pin-Boxen.
// - Patch pro aktivem Pin, NUR bei sichtbarem Overlay UND laufendem SC UND
//   nicht click-through, an der persistierten Box-Position in Monitor-
//   Viewport-CSS-Px (der Spotlight deckt seinen Screen 1:1 ab — dieselbe
//   Viewport-Basis, die PinWindow.svelte für die Boxen nutzt).
// - Nur Pins mit `pin.screen === spotlight_screen` bekommen eine Fläche.
// - DOM-Z-Ordnung: die Patches liegen als Geschwister VOR dem .shell-Band,
//   Shell-/UI-Elemente gewinnen jeden Überlapp (visuell + Hit-Testing).
//
// Interaktion: Drag → pin_move (Sub-Args wie das alte Pin-Fenster-Drag:
// {x,y,w,h,screen_w,screen_h}, Edge-Snapping bleibt Backend); Klick (ohne
// Drag-Bewegung) → Mini-Controller-Popover (Deckkraft-Presets + ±5-Schritte,
// Lösen = pin_unpin, Schließen = pin_remove). ESC/Klick außerhalb schließt.
import { onMount } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { t } from './i18n';
import { listen } from '@tauri-apps/api/event';
import { PINS_CHANGED_EVENT, PIN_BOX_W, pinBoxEstHeight, type PinEntry } from './pins';
import { patchPos, popoverPos } from './pin-patches';

type PatchOverlay = {
	visible: boolean;
	game_running: boolean;
	click_through: boolean;
};

let { overlay }: { overlay: PatchOverlay | null } = $props();

let pins = $state<PinEntry[]>([]);
let spotlightScreen = $state('');
let vw = $state(0);
let vh = $state(0);
let dragPin: PinEntry | null = $state(null);
let openKey = $state<string | null>(null);

const PRESETS = [20, 55, 100];

// Patch-Gate: sichtbares Overlay UND laufendes SC UND nicht click-through
// (§9.1.6 präzisiert M4-E — der Overlay-Click-Through-Modus hält die Patches
// zurück, damit Klicks im Spiel nicht von unsichtbaren Flächen gefangen).
const active = $derived(
	overlay !== null && overlay.visible && overlay.game_running && !overlay.click_through
);

function patchKey(pin: PinEntry): string {
	return pin.view_id + ':' + pin.screen + ':' + pin.box_index;
}

// Patch-Filter: nur der eigene Spotlight-Screen — die Spot-Viewport-
// Koordinaten decken exakt diesen Monitor ab (Pins anderer Screens bleiben
// ohne Fläche; das Spot-Overlay sitzt nicht dort).
const patchPins = $derived(pins.filter((p) => p.pinned && p.screen === spotlightScreen));

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

function patchPosFor(pin: PinEntry) {
	return patchPos(pin, vw, vh);
}

// Popover-Position: neben der Box (Klick-Position), an den Screen-Rand
// geclampt (keine fixed-Positionierung — Modal-Regeln, absolute im Patch).
function popoverPosFor(pin: PinEntry) {
	return popoverPos(pin, vw, vh);
}

// Drag NUR bei sichtbarem Overlay (Patch-Flächen existieren sowieso nur dann);
// ohne Drag-Bewegung (Klick) öffnet das Controller-Popover.
function onGrab(e: PointerEvent, pin: PinEntry) {
	if (!active) return;
	e.preventDefault();
	openKey = null;
	dragPin = pin;
	const move = (ev: PointerEvent) => {
		if (!dragPin) return;
		dragPin = { ...dragPin, x: pin.x + ev.movementX, y: pin.y + ev.movementY };
	};
	const up = (ev: PointerEvent) => {
		const p = dragPin;
		dragPin = null;
		window.removeEventListener('pointermove', move);
		window.removeEventListener('pointerup', up);
		if (!p) return;
		const moved = Math.abs(ev.movementX) + Math.abs(ev.movementY);
		if (moved > 0) {
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
			openKey = patchKey(p);
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

function unpin(pin: PinEntry) {
	void invoke('pin_unpin', {
		view_id: pin.view_id,
		screen: pin.screen,
		box_index: pin.box_index
	});
	void load();
}

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
	// Klick außerhalb des Popovers (im Shell-UI, im Spiel oder auf einem
	// anderen Patch) schließt — Klicks im Popover selbst schließen nicht.
	const el = e.target as Element | null;
	if (el && el.closest('.pin-popover')) return;
	openKey = null;
}

onMount(() => {
	measure();
	void load();
	void listen(PINS_CHANGED_EVENT, () => void load());
});
</script>

<svelte:window onkeydown={onKeydown} onpointerdown={onWindowPointerdown} />

{#if active}
	<div class="patch-layer" class:dragging={dragPin !== null}>
		{#each patchPins as pin (patchKey(pin))}
			{@const pos = patchPosFor(pin)}
			<div
				class="patch"
				class:dragging={dragPin === pin}
				style={`left:${pos.x}px; top:${pos.y}px; width:${PIN_BOX_W}px; height:${pinBoxEstHeight(pin.rows.length)}px`}
				role="button"
				aria-label={$t('pins.drag_title')}
				tabindex="-1"
				onpointerdown={(e) => onGrab(e, pin)}
			>
				{#if openKey === patchKey(pin)}
					<div
						class="pin-popover"
						style={`left:${popoverPosFor(pin).x}px; top:${popoverPosFor(pin).y}px`}
					>
						<button type="button" class="pbtn" onclick={() => cycleOpacity(pin)} title="{$t('pins.opacity_title')}">◐</button>
						<button type="button" class="pbtn" onclick={() => stepOpacity(pin, -5)} title="{$t('pins.opacity_title')}">−</button>
						<button type="button" class="pbtn" onclick={() => stepOpacity(pin, 5)} title="{$t('pins.opacity_title')}">+</button>
						{#each PRESETS as p (p)}
							<button type="button" class="pbtn" class:active={pin.opacity === p} onclick={() => stepOpacity(pin, p - pin.opacity)} title="{$t('pins.opacity_title')}">{p}</button>
						{/each}
						<button type="button" class="pbtn" onclick={() => unpin(pin)} title="{$t('pins.unpin_title')}">⌐</button>
						<button type="button" class="pbtn" onclick={() => remove(pin)} title="{$t('pins.close_title')}">✕</button>
					</div>
				{/if}
			</div>
		{/each}
	</div>
{/if}

<style>
	/* Patch-Flächen: UNSICHTBAR (kein Hintergrund/Border) — die Interaktions-
	   Fläche nur für die Box-Position. Die Ebene ist pointer-events:none,
	   nur die Patch-Divs selbst fangen Pointer (die Shell oben gewinnt). */
	.patch-layer {
		position: absolute;
		inset: 0;
		pointer-events: none;
	}
	.patch {
		position: absolute;
		pointer-events: auto;
		cursor: grab;
		/* Unsichtbar: nur Interaktionsfläche (kein eigener Hintergrund). */
		background: transparent;
	}
	.patch.dragging {
		cursor: grabbing;
	}
	/* Mini-Controller-Popover: Overlay-Tokens, absolute im Patch-Anker
	   (kein position:fixed — Modal-Regeln). */
	.pin-popover {
		position: absolute;
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
