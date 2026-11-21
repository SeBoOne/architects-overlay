<script lang="ts">
	// M8-R3 (t_399b10d8): Custom-Quality-Slider — ersetzt den nativen
	// <input type="range"> der Material-Regler (M8-R1-A). Der native Slider
	// kann die drei Anforderungen nicht:
	//
	//   1. **500 immer die optische Mitte** — der Track bleibt FIX 0–1000
	//      (kein min/max-Skalieren auf bands[0]–1000): 500 sitzt immer exakt
	//      in der Track-Mitte. Die Band-Grenzen werden im Pointer-Handler
	//      geclampt, nicht durch min/max-Attribute.
	//   2. **Magnet beim Ziehen (Hysterese)** — der gerenderte Thumb steht
	//      NUR auf Snap-Punkten. Gewechselt wird erst, wenn der Pointer die
	//      HALBE DISTANZ zwischen dem AKTUELLEN gerasteten Punkt und dem
	//      Nachbar-Punkt überschreitet — und zurück erst wieder unter
	//      denselben Halbweg (striktes > / < am Halbweg: exakt AUF dem
	//      Halbweg bleibt der aktuelle Punkt stehen → JITTER-sicher, die
	//      Anzeige springt nicht beim Wackeln um die Mitte).
	//   3. **Harte Grenzen** — unter bounds[0] / über bounds[1] ist physisch
	//      Schluss: der Pointer darf den Thumb nicht über die Grenze ziehen
	//      (Clamp im Pointer-Handler; bands[0]/bands[-1] sind erreichbare
	//      Anschläge). Snap-aus = 0–1000 frei (Wert folgt dem Pointer exakt).
	//
	// Pointer-Events werden selbst verwaltet (pointerdown/move/up + Cancel,
	// setPointerCapture best-effort — synthetische Probe-Events haben keine
	// aktive Pointer-Id). Tastatur: Pfeile = nächster/vorheriger Rasterpunkt
	// (Snap-an) bzw. ±10 (Snap-aus), Home/End = Anschlag. ARIA: role=slider
	// mit min/max/now (die ARIA-Grenzen spiegeln die HARDEN Grenzen, nicht
	// den Track — Screenreader hören dieselben Grenzen wie der Pointer).
	// KEIN DOM-Write-in-Value-Gefummel mehr (0.5.85–0.5.87 WebKit-Drag-Zirkus):
	// der Thumb ist ein div, dessen Position rein aus dem State folgt.
	const MIN = 0;
	const MAX = 1000;

	let {
		value = $bindable(500),
		snaps = [],
		snapOn = false,
		bounds = [MIN, MAX],
		ariaLabel = ''
	}: {
		value?: number;
		snaps?: number[];
		snapOn?: boolean;
		bounds?: [number, number];
		ariaLabel?: string;
	} = $props();

	let trackEl = $state<HTMLElement | null>(null);
	let dragging = $state(false);

	// Raster (sortiert, dedup) — der Thumb steht NUR auf diesen Punkten.
	const sorted = $derived([...new Set(snaps)].sort((a, b) => a - b));
	// Index des aktuell gerasteten Punkts (Anker der Hysterese).
	const curIdx = $derived.by(() => {
		let best = 0;
		for (let i = 0; i < sorted.length; i++) {
			if (Math.abs(sorted[i] - value) < Math.abs(sorted[best] - value)) best = i;
		}
		return best;
	});

	/** Pointer-Rohwert 0–1000 aus der Track-Geometrie (Thumb-Mitte = Wert). */
	function rawFromPointer(e: PointerEvent): number {
		const el = trackEl;
		if (el === null) return value;
		const r = el.getBoundingClientRect();
		if (r.width <= 0) return value;
		const frac = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
		return Math.round(frac * (MAX - MIN));
	}

	/** Harte Grenzen: Snap-an → bounds (physisch Schluss, Anschläge
	 *  erreichbar), Snap-aus → 0–1000 frei. */
	function clampValue(v: number): number {
		const [lo, hi] = snapOn ? bounds : [MIN, MAX];
		return Math.min(Math.max(lo, v), Math.max(lo, hi));
	}

	/** Pointer-Rohwert anwenden. `initial` = Klick/Jump (direkt auf den
	 *  nächsten Rasterpunkt), sonst Drag mit Hysterese. */
	function applyPointer(raw: number, initial: boolean) {
		const clamped = clampValue(raw);
		if (!snapOn || sorted.length === 0) {
			// Snap-aus (oder kein Raster): frei 0–1000, Wert folgt exakt.
			value = clamped;
			return;
		}
		if (initial) {
			// Klick direkt auf den nächsten Punkt (innerhalb der Grenzen).
			let best = 0;
			for (let i = 0; i < sorted.length; i++) {
				if (Math.abs(sorted[i] - clamped) < Math.abs(sorted[best] - clamped)) best = i;
			}
			value = sorted[best];
			return;
		}
		// Hysterese relativ zum AKTUELLEN gerasteten Punkt: gewechselt wird
		// erst STRENG hinter dem Halbweg zum Nachbarn (und zurück erst wieder
		// STRENG unter denselben Halbweg — exakt auf dem Halbweg bleibt der
		// aktuelle Punkt: kein Flackern). Schnelle Drags, deren Pointer in
		// einem Move-Event mehrere Punkte überspringt, laufen die Zwischen-
		// punkte im selben Durchlauf ab — der Wert bleibt IMMER im Raster.
		let i = curIdx;
		for (;;) {
			const cur = sorted[i];
			if (clamped > cur) {
				const next = sorted[i + 1];
				if (next === undefined || clamped <= (cur + next) / 2) break;
				i++;
			} else if (clamped < cur) {
				const prev = sorted[i - 1];
				if (prev === undefined || clamped >= (prev + cur) / 2) break;
				i--;
			} else {
				break;
			}
		}
		value = sorted[i];
		}

	function onPointerDown(e: PointerEvent) {
		e.preventDefault();
		dragging = true;
		// Best-effort: synthetische Probe-Events haben keine aktive Pointer-Id.
		try {
			trackEl?.setPointerCapture(e.pointerId);
		} catch {
			/* ohne Capture arbeiten die Track-Handler trotzdem */
		}
		applyPointer(rawFromPointer(e), true);
	}
	function onPointerMove(e: PointerEvent) {
		if (!dragging) return;
		applyPointer(rawFromPointer(e), false);
	}
	function onPointerEnd() {
		dragging = false;
	}

	/** Tastatur: Pfeile = nächster/vorheriger Rasterpunkt (Snap-an) bzw.
	 *  ±10 (Snap-aus), Home/End = Anschlag. */
	function onKey(e: KeyboardEvent) {
		let next: number | null = null;
		switch (e.key) {
			case 'ArrowRight':
			case 'ArrowUp':
				next = step(1);
				break;
			case 'ArrowLeft':
			case 'ArrowDown':
				next = step(-1);
				break;
			case 'Home':
				next = clampValue(MIN);
				break;
			case 'End':
				next = clampValue(MAX);
				break;
			default:
				return;
		}
		e.preventDefault();
		if (next !== null) value = next;
	}
	function step(dir: 1 | -1): number | null {
		if (snapOn && sorted.length > 0) {
			return sorted[curIdx + dir] ?? null; // ohne Rasterpunkt → kein Move
		}
		return clampValue(value + dir * 10);
	}

	const pct = (v: number) => ((v - MIN) / (MAX - MIN)) * 100;
</script>

<div class="cqs" class:is-snap={snapOn} class:is-drag={dragging}>
	<div
		class="cqs-track"
		bind:this={trackEl}
		role="slider"
		tabindex="0"
		aria-valuemin={snapOn ? bounds[0] : MIN}
		aria-valuemax={snapOn ? bounds[1] : MAX}
		aria-valuenow={value}
		aria-label={ariaLabel}
		onpointerdown={onPointerDown}
		onpointermove={onPointerMove}
		onpointerup={onPointerEnd}
		onpointercancel={onPointerEnd}
		onlostpointercapture={onPointerEnd}
		onkeydown={onKey}
	>
		<div class="cqs-fill" style={`width:${pct(value)}%`}></div>
		{#each sorted as t (t)}
			<span class="cqs-tick" style={`left:${pct(t)}%`}></span>
		{/each}
		<div class="cqs-thumb" style={`left:${pct(value)}%`}></div>
	</div>
</div>

<style>
	.cqs {
		/* t_24961acd (User-P2) + M8-R4 (t_e8f4f57d): Der Balken nutzt die
		   verfügbare Zeilenbreite VOLL aus (User-Korrektur 2026-11-21) — die
		   feste 19rem-Breite war zu schmal. Alle Zeilen sind gleich aufgebaut
		   (Balken + Wert + Toggle; Effekttext lebt in einer EIGENEN Zeile
		   darunter, Crafting.svelte) → die Tracks bleiben trotzdem pixel-gleich
		   und die 500-Mitte auf derselben x-Position (Geometrie-Vertrag ±1px). */
		flex: 1;
		min-width: 0;
		display: flex;
		align-items: center;
	}
	.cqs-track {
		position: relative;
		flex: 1;
		height: 16px;
		cursor: pointer;
		touch-action: none;
		outline: none;
	}
	/* Rail */
	.cqs-track::before {
		content: '';
		position: absolute;
		left: 0;
		right: 0;
		top: 50%;
		height: 4px;
		transform: translateY(-50%);
		border-radius: 2px;
		background: var(--line, #2a2f3a);
	}
	.cqs-track:focus-visible {
		border-radius: 4px;
		box-shadow: 0 0 0 1px var(--acc, #e0a83e);
	}
	.cqs-fill {
		position: absolute;
		left: 0;
		top: 50%;
		height: 4px;
		transform: translateY(-50%);
		border-radius: 2px;
		background: var(--acc, #e0a83e);
		opacity: 0.45;
		pointer-events: none;
	}
	/* Ticks für jeden Snap-Punkt (bands + 500) */
	.cqs-tick {
		position: absolute;
		top: 50%;
		width: 1px;
		height: 9px;
		transform: translate(-50%, -50%);
		background: rgba(255, 255, 255, 0.32);
		pointer-events: none;
	}
	.cqs-thumb {
		position: absolute;
		top: 50%;
		width: 12px;
		height: 12px;
		border-radius: 50%;
		background: var(--acc, #e0a83e);
		transform: translate(-50%, -50%);
		pointer-events: none;
		box-shadow: 0 0 0 2px rgba(0, 0, 0, 0.45);
	}
	.cqs.is-snap .cqs-thumb {
		box-shadow:
			0 0 0 2px rgba(0, 0, 0, 0.45),
			0 0 6px rgba(224, 168, 62, 0.55);
	}
	.cqs.is-drag {
		cursor: grabbing;
	}
</style>
