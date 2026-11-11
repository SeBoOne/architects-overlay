<script lang="ts" module>
	// M6-F1 (t_58d46923): Content-Renderer-Registry — DealChat als erster
	// registrierter Content-Typ (Proof of Concept, Universality-Regel:
	// keine SCLM-/DealChat-Semantik in der Pin-Box selbst).
	import PinChat from './PinChat.svelte';
	import { registerPinContent } from './pin-content';
	registerPinContent('deal-chat', PinChat);
</script>

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
import { PINS_CHANGED_EVENT, PIN_BOX_W, SCALE_STEP, clampScale, pinBoxEstHeight, pinBoxSize, type PinEntry, type PinsState } from './pins';
import { dragBasePos, patchPos, popoverPos } from './pin-patches';
import { pinContentFor } from './pin-content';

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
let resizePin: { pin: PinEntry; scale: number } | null = $state(null);
let openKey = $state<string | null>(null);

// M6-F3: die vordefinierten Werte kommen aus den Settings (editierbar);
// Default bis zum ersten pins_state-Load = der M4-Vertrag.
let presets = $state<number[]>([20, 55, 100]);

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
		const st = await invoke<PinsState>('pins_state');
		pins = st.pins;
		spotlightScreen = st.spotlight_screen ?? '';
		// M6-F3: editierbare Presets aus den Settings übernehmen (normiert vom
		// Backend — genau 3 Werte; defensive Fallback bei leerer Liste).
		if (st.presets?.length) presets = st.presets;
		dbg(
			`load pins=${pins.length} boxPins=${pins.filter((p) => p.pinned && p.screen === spotlightScreen).length} screen=${spotlightScreen} vw=${window.innerWidth}x${window.innerHeight} dpr=${window.devicePixelRatio}`
		);
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

// M6-F2 (t_3ff5e552): effektiver Größenfaktor — beim Corner-Resize die
// Live-Skalierung, sonst der persistierte Wert (Legacy: undefined → 100).
function liveScale(pin: PinEntry): number {
	if (resizePin && boxKey(resizePin.pin) === boxKey(pin)) return resizePin.scale;
	return clampScale(pin.scale ?? 100);
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

// M5-R3 (t_12e6bd06) Diagnose: der komplette Drag-Pfad loggt über die
// Backend-Brücke `pins_debug_log` (Debug-Builds loggen, Release verwirft
// stumm) — ein Satz Zahlen pro Drag: Start, je Move dx/dy/Live-Position,
// beim Drop die gesendeten pin_move-Args. Kandidaten-Check: DPR/Viewport-
// Offset (hyprctl-Befund Spotlight at [0,-15]) gegen clientX/Y.
function dbg(line: string) {
	void invoke('pins_debug_log', { line }).catch(() => {});
}

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
	//
	// M6-R1 (t_325456dc): Anker = GERENDERTE (geclampte) Position
	// (dragBasePos), nicht die rohe Persistenz-Koordinate — sonst klebt der
	// erste Drag am Edge, wenn die gespeicherte Position außerhalb des
	// Viewport-Clamps liegt (Nachzieh-Verzögerung/Versatz; Details
	// pin-patches.ts dragBasePos). Ab dem zweiten Move war das wegen des
	// pin_move-Snap-Persistenz schon sauber — jetzt ist jeder Move es.
	const startX = e.clientX;
	const startY = e.clientY;
	const base = dragBasePos(pin, window.innerWidth, window.innerHeight);
	dbg(
		`grab start=(${startX},${startY}) pin=(${pin.x},${pin.y}) base=(${base.x},${base.y}) vw=${window.innerWidth}x${window.innerHeight} dpr=${window.devicePixelRatio}`
	);
	let dx = 0;
	let dy = 0;
	const move = (ev: PointerEvent) => {
		if (!dragPin) return;
		dx = ev.clientX - startX;
		dy = ev.clientY - startY;
		dragPin = { ...pin, x: base.x + dx, y: base.y + dy };
		dbg(`move client=(${ev.clientX},${ev.clientY}) d=(${dx},${dy}) live=(${dragPin.x},${dragPin.y})`);
	};
	const up = (ev: PointerEvent) => {
		const p = dragPin;
		window.removeEventListener('pointermove', move);
		window.removeEventListener('pointerup', up);
		if (!p) return;
		const moved = Math.abs(dx) + Math.abs(dy);
		if (moved > DRAG_THRESHOLD) {
			const dragArgs = {
				// M5-R3 (t_12e6bd06): runde an der Grenze Frontend/Backend —
				// Subpixel-clientX/Y (reale Maus) durften den pin_move-Invoke
				// als i32-Contract still verwerfen lassen → Drop-Ort verlor
				// sich. Backend akzeptiert zusätzlich f64 (Runden im Rust).
				// M6-F2: w/h sind die SKALIERTEN Box-Maße (zoom-Layer) — dieselbe
				// Mathe wie pinBoxSize/patchPos, sonst snappt das Backend an der
				// unskalierten Größe.
				x: Math.round(p.x),
				y: Math.round(p.y),
				w: pinBoxSize(p.rows.length, p.scale).w,
				h: pinBoxSize(p.rows.length, p.scale).h,
				screen_w: window.innerWidth,
				screen_h: window.innerHeight
			};
			dbg(`up moved=${moved} pin_move=${JSON.stringify(dragArgs)}`);
			void invoke('pin_move', {
				view_id: p.view_id,
				screen: p.screen,
				box_index: p.box_index,
				drag: dragArgs
			}).catch((err) => dbg(`pin_move FAILED ${String(err)}`));
			// M6-R1 (t_325456dc): Flash-Back-Fix — dragPin NICHT sofort
			// löschen. Vorher renderte die Box zwischen Release und dem
			// Bestätigungs-Reload wieder die STALE pins-Position (alter
			// Stand) und sprang danach auf das Backend-Ergebnis. dragPin
			// bleibt Render-Quelle, bis load() die bestätigte (gesnapte)
			// Position geliefert hat; erst dann wird geleert.
			void (async () => {
				await load();
				if (dragPin && boxKey(dragPin) === boxKey(p)) dragPin = null;
			})();
		} else {
			dragPin = null;
			openKey = boxKey(p);
		}
	};
	window.addEventListener('pointermove', move);
	window.addEventListener('pointerup', up);
}

// M6-F2 (t_3ff5e552): Resize am Corner-Handle (unten rechts). Der Griff
// zieht die Box-Ecke; der Faktor ergibt sich aus dem Pointer-Abstand zur
// Box-Ecke (oben links) relativ zum Layoutmaß — horizontal UND vertikal
// gemittelt, clamp 50–200. Beim Release: auf SCALE_STEP runden, dann
// `pin_resize` (Größe + neu gesnappte Position, Persistenz im Backend).
function onResizeStart(e: PointerEvent, pin: PinEntry) {
	if (!interactive) return;
	e.preventDefault();
	e.stopPropagation();
	openKey = null;
	const origin = boxPosFor(pin);
	const startRows = pin.rows.length;
	resizePin = { pin, scale: liveScale(pin) };
	dbg(`resize start scale=${resizePin.scale} origin=(${origin.x},${origin.y})`);
	const move = (ev: PointerEvent) => {
		if (!resizePin) return;
		const sx = ((ev.clientX - origin.x) / PIN_BOX_W) * 100;
		const sy = ((ev.clientY - origin.y) / pinBoxEstHeight(startRows)) * 100;
		resizePin = { pin, scale: clampScale((sx + sy) / 2) };
	};
	const up = () => {
		const r = resizePin;
		resizePin = null;
		window.removeEventListener('pointermove', move);
		window.removeEventListener('pointerup', up);
		if (!r) return;
		const scale = Math.round(r.scale / SCALE_STEP) * SCALE_STEP;
		const { w, h } = pinBoxSize(startRows, scale);
		dbg(`resize end scale=${scale} w=${w} h=${h}`);
		void invoke('pin_resize', {
			view_id: r.pin.view_id,
			screen: r.pin.screen,
			box_index: r.pin.box_index,
			scale,
			drag: {
				x: Math.round(r.pin.x),
				y: Math.round(r.pin.y),
				w,
				h,
				screen_w: window.innerWidth,
				screen_h: window.innerHeight
			}
		}).catch((err) => dbg(`pin_resize FAILED ${String(err)}`));
		void load();
	};
	window.addEventListener('pointermove', move);
	window.addEventListener('pointerup', up);
}

// M6-F2: Größe ± im Controller-Popover (Schritt SCALE_STEP, Clamp im Backend;
// Anker oben links — Position bleibt, die Box wächst nach rechts/unten).
function stepScale(pin: PinEntry, delta: number) {
	const next = clampScale(liveScale(pin) + delta);
	void invoke('pin_set_scale', {
		view_id: pin.view_id,
		screen: pin.screen,
		box_index: pin.box_index,
		scale: next
	});
	void load();
}

// M6-F3: Preset-Zyklus über die (editierbare) Presets-Liste — der nächste
// Wert ÜBER dem aktuellen, Wrap zum kleinsten (Verhalten wie M4 bei 20/55/100).
function cycleOpacity(pin: PinEntry) {
	const next = presets.find((p) => p > pin.opacity) ?? presets[0];
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
			     sonst würde die Box erst beim Release/Reload springen (Sebo-Befund).
			     Resize-Live-Feedback analog: resizePin überschreibt den Faktor →
			     die Patch-Geometrie (boxPosFor → patchPos) rechnet sofort mit der
			     neuen Größe. -->
			{@const live =
				dragPin && boxKey(dragPin) === boxKey(pin)
					? dragPin
					: resizePin && boxKey(resizePin.pin) === boxKey(pin)
						? { ...pin, scale: resizePin.scale }
						: pin}
			{@const scale = liveScale(pin)}
			{@const pos = boxPosFor(live)}
			{@const Content = pinContentFor(pin.content?.type)}
			<article
				class="pin"
				class:interactive
				class:dragging={dragPin === pin}
				class:resizing={resizePin !== null && boxKey(resizePin.pin) === boxKey(pin)}
				style={`left:${pos.x}px; top:${pos.y}px; opacity:${pin.opacity / 100}; zoom:${scale / 100}`}
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
					{#if Content}
						<!-- M6-F1: kontext-adaptiver Content-Renderer (z. B. DealChat).
						     interactive = Overlay-aktiv-Gate (M4-E/M5-Vertrag) — der
						     Inline-Composer rendert nur bei interaktivem Spotlight. -->
						<Content {pin} {interactive} />
					{:else}
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
					{/if}
				</div>
				{#if interactive}
					<!-- M6-F2: Corner-Resize-Griff (unten rechts, Winkel-Marker). -->
					<button
						type="button"
						class="pin-resize"
						title={$t('pins.resize_title')}
						aria-label={$t('pins.resize_title')}
						onpointerdown={(e) => onResizeStart(e, pin)}
					><i></i></button>
				{/if}
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
					{#each presets as p (p)}
						<button type="button" class="pbtn" class:active={pin.opacity === p} onclick={() => stepOpacity(pin, p - pin.opacity)} title={$t('pins.opacity_title')}>{p}</button>
					{/each}
					<!-- M6-F2: Größen-Step ± (50–200 %, Backend-Clamp). -->
					<button type="button" class="pbtn" onclick={() => stepScale(pin, -SCALE_STEP)} title={$t('pins.size_title')}>⤡−</button>
					<button type="button" class="pbtn" onclick={() => stepScale(pin, SCALE_STEP)} title={$t('pins.size_title')}>⤢+</button>
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
	/* M6-F2: Corner-Resize-Griff — unten rechts über der clip-path-Ecke.
	   Position absolute im Box-Kontext: das zoom der Box skaliert den Griff
	   mit, was ok ist (Trefferfläche wächst mit der Box). */
	.pin-resize {
		position: absolute;
		right: 2px;
		bottom: 2px;
		width: 16px;
		height: 16px;
		display: none;
		background: transparent;
		border: 0;
		padding: 0;
		cursor: nwse-resize;
	}
	.pin-resize i {
		position: absolute;
		right: 2px;
		bottom: 2px;
		width: 9px;
		height: 9px;
		border-right: 2px solid var(--fgdim);
		border-bottom: 2px solid var(--fgdim);
		display: block;
	}
	.pin.interactive .pin-resize {
		display: block;
	}
	.pin.interactive .pin-resize:hover i,
	.pin.resizing .pin-resize i {
		border-color: var(--accent);
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
