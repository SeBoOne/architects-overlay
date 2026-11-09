<script lang="ts">
	// M1 (t_90cb34ee, Scope 2): Titelband Variante B (Sebo-Beschluss §9.1.1,
	// mockup-fenstermodus-b.html) — eigenes Client-side-Chrome für das
	// Fenstermodus-Fenster (decorations: false), im Stil des restlichen
	// Overlays (Tokens aus app.css — Optik-Vertrag; KEIN neues Design).
	//
	// Zonen (Mockup §3): Emblem+Wordmark | Modus-Umschalter Overlay|Fenster |
	// Drag (ganze Bandfläche) | Menü (Einstellungen F12, Beenden rot) |
	// Minimieren, In Tray. KEIN × — Schließen ist nur-ins-Tray (Backend
	// verhindert den Close, M1 Scope 3); die dashed „kein Schließen"-Chips
	// des Mockups sind ANNOTATIONEN und werden bewusst NICHT gebaut.
	//
	// Drag: das ganze Band ist der Griff (onmousedown ohne Buttons/Interaktive
	// Elemente → window_start_drag, Backend start_dragging).
	import { invoke } from '@tauri-apps/api/core';
	import { t } from '../lib/i18n';
	import logoUrl from '../lib/assets/logo.svg';
	import { APP_WINDOW_COMPACT_PX } from './window-breakpoints';

	/** Aktiver Modus der Shell-Darstellung (kein Restart, M1 Scope 2). */
	let mode: 'window' | 'overlay' = $state('window');
	/** Menü-Popover offen. */
	let menuOpen = $state(false);

	function setMode(next: 'window' | 'overlay') {
		if (next === mode) return;
		mode = next;
		if (next === 'overlay') {
			// Overlay-Modus = in-Game-Spotlight-Fenster einblenden; dieses
			// Fenster versteckt sich (Backend window_hide). Der Rückweg ist
			// der Tray-Punkt „Fenstermodus zeigen/verstecken".
			void invoke('overlay_set_visible', { visible: true });
			void invoke('window_hide');
		} else {
			void invoke('window_show');
		}
	}

	function quit() {
		menuOpen = false;
		void invoke('window_quit');
	}

	function openSettings() {
		menuOpen = false;
		// Einstellungen (F12): gleicher In-App-Pfad wie der Rail-Zahnrad —
		// das Event erreicht den Parent-Shell-Code über ein Custom-Event.
		window.dispatchEvent(new CustomEvent('ao-window-open-config'));
	}

	function minimize() {
		void invoke('window_minimize');
	}

	function toTray() {
		void invoke('window_hide');
	}

	/** Drag-Griff: ganze Bandfläche, außer klickbaren Zonen. */
	function bandDown(e: MouseEvent) {
		const target = e.target as HTMLElement;
		if (target.closest('button, a, input, select, textarea')) return;
		e.preventDefault();
		void invoke('window_start_drag');
	}

	// M2 (t_a931d1bb Scope 4): Kompakt-Schritt des Titelbands — gemessene
	// Bandbreite (bind:clientWidth). Unter dem Fenstermodus-Schwellwert
	// (Mockup B ~900 px) kollabieren die rechtsseitigen Buttons auf reine
	// Icons (Tooltips tragen die Aktion, Wordmark skaliert kleiner —
	// mockup-Annotation „emblem+abbreviation“ bewusst NICHT nachgebaut).
	let tbW = $state(0);
	const compact = $derived(tbW > 0 && tbW < APP_WINDOW_COMPACT_PX);
</script>

<div class="tb" class:is-compact={compact} bind:clientWidth={tbW} onmousedown={bandDown} role="toolbar" aria-label={$t('titlebar.aria')} tabindex="-1">
	<span class="tb-logo">
		<img src={logoUrl} alt="" />
		<b>Architects Overlay</b>
		<i>{$t('titlebar.badge')}</i>
	</span>
	<span class="tb-mode" role="radiogroup" aria-label={$t('titlebar.mode_aria')}>
		<button
			type="button"
			class:on={mode === 'overlay'}
			aria-pressed={mode === 'overlay'}
			onclick={() => setMode('overlay')}
		>{$t('titlebar.mode_overlay')}</button>
		<button
			type="button"
			class:on={mode === 'window'}
			aria-pressed={mode === 'window'}
			onclick={() => setMode('window')}
		>{$t('titlebar.mode_window')}</button>
	</span>
	<span class="tb-spacer"></span>
	<div class="tb-menu-wrap">
		<button type="button" class="tb-menu" aria-haspopup="menu" aria-expanded={menuOpen} onclick={() => (menuOpen = !menuOpen)}>
			&#9776;<span class="bl"> {$t('titlebar.menu')}</span>
		</button>
		{#if menuOpen}
			<!-- klick außerhalb schließt -->
			<!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
			<div class="tb-scrim" onclick={() => (menuOpen = false)}></div>
			<div class="tb-pop" role="menu">
				<button type="button" role="menuitem" onclick={openSettings}>
					{$t('titlebar.settings')}<span class="k">F12</span>
				</button>
				<div class="msep"></div>
				<button type="button" role="menuitem" class="danger" onclick={quit}>
					{$t('titlebar.quit')}&nbsp;&nbsp;&#9166;
				</button>
			</div>
		{/if}
	</div>
	<span class="tb-btns">
		<button type="button" class="tb-b" title={$t('titlebar.minimize')} onclick={minimize}>
			<span>&#8211;</span> <span class="bl">{$t('titlebar.minimize')}</span>
		</button>
		<button type="button" class="tb-b" title={$t('titlebar.to_tray')} onclick={toTray}>
			<span>&#9635;</span> <span class="bl">{$t('titlebar.to_tray')}</span>
		</button>
	</span>
</div>

<style>
	/* Tokens + Geometrie 1:1 aus dem Mockup (Variante B) — kein neuer Ton,
	   kein neues Design (Optik-Vertrag: „Optik wie zuvor"). */
	.tb {
		display: flex;
		align-items: center;
		gap: 12px;
		min-height: 46px;
		padding: 0 10px 0 14px;
		background: var(--panel2);
		border-bottom: 2px solid var(--accent);
		user-select: none;
	}
	.tb-logo {
		display: flex;
		align-items: center;
		gap: 9px;
	}
	.tb-logo img {
		width: 22px;
		height: 22px;
		display: block;
	}
	.tb-logo b {
		font: 700 11px/1 var(--font-mono);
		letter-spacing: 0.2em;
		text-transform: uppercase;
		color: var(--accent);
	}
	.tb-logo i {
		font-style: normal;
		font: 700 9px/1 var(--font-mono);
		letter-spacing: 0.14em;
		color: var(--fgfaint);
		border: 1px solid var(--lineb);
		padding: 3px 5px;
		margin-left: 2px;
	}
	.tb-mode {
		display: flex;
		gap: 0;
		margin-left: 8px;
	}
	.tb-mode button {
		background: var(--panel3);
		border: 1px solid var(--lineb);
		color: var(--fgdim);
		font: 700 10px/1 var(--font-mono);
		letter-spacing: 0.12em;
		text-transform: uppercase;
		padding: 6px 10px;
		cursor: pointer;
	}
	.tb-mode button + button {
		border-left: 0;
	}
	.tb-mode button:hover {
		color: var(--fg);
	}
	.tb-mode button.on {
		background: color-mix(in srgb, var(--accent) 20%, transparent);
		border-color: var(--accent);
		color: var(--accent-h);
	}
	.tb-spacer {
		flex: 1;
	}
	.tb-menu-wrap {
		position: relative;
	}
	.tb-menu {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		background: transparent;
		border: 1px solid var(--lineb);
		color: var(--fgdim);
		cursor: pointer;
		font: 700 10px/1 var(--font-mono);
		letter-spacing: 0.12em;
		text-transform: uppercase;
		padding: 6px 9px;
	}
	.tb-menu:hover {
		color: var(--fg);
		border-color: var(--fgdim);
	}
	.tb-scrim {
		position: fixed;
		inset: 0;
		z-index: 30;
	}
	.tb-pop {
		position: absolute;
		top: calc(100% + 4px);
		right: 0;
		z-index: 31;
		min-width: 240px;
		background: var(--panel2);
		border: 1px solid var(--lineb);
		box-shadow: var(--shadow-shell);
		padding: 4px;
		display: flex;
		flex-direction: column;
	}
	.tb-pop button {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		background: transparent;
		border: 0;
		color: var(--fgdim);
		font: 600 11px/1 var(--font-ui);
		padding: 9px 10px;
		cursor: pointer;
		text-align: left;
	}
	.tb-pop button:hover {
		background: color-mix(in srgb, var(--accent) 12%, transparent);
		color: var(--fg);
	}
	.tb-pop button .k {
		font: 700 9px/1 var(--font-mono);
		letter-spacing: 0.12em;
		color: var(--fgfaint);
		border: 1px solid var(--lineb);
		padding: 3px 5px;
	}
	.tb-pop button.danger {
		color: var(--alert);
	}
	.tb-pop button.danger:hover {
		background: color-mix(in srgb, var(--alert) 14%, transparent);
		color: var(--alert);
	}
	.tb-pop .msep {
		height: 1px;
		background: var(--line);
		margin: 3px 2px;
	}
	.tb-btns {
		display: flex;
		gap: 6px;
	}
	.tb-b {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		background: transparent;
		border: 1px solid var(--lineb);
		color: var(--fgdim);
		font: 700 10px/1 var(--font-mono);
		letter-spacing: 0.1em;
		text-transform: uppercase;
		padding: 6px 9px;
		cursor: pointer;
	}
	.tb-b:hover {
		color: var(--fg);
		border-color: var(--fgdim);
	}

	/* M2 (t_a931d1bb Scope 4): Kompakt-Schritt — rechts-Seite kollabiert auf
	   Icon-Buttons (≡ / – / ▣) mit Tooltips (title-Attribute existieren
	   bereits); Wordmark bleibt sichtbar, skaliert nur kleiner (einfach
	   halten, „emblem+abbreviation“-Inkonsistenz des Mockups nicht
	   nachgebaut). Bind:clientWidth-Gate oben; Kompakt nur unten schmal. */
	.tb.is-compact .bl {
		display: none;
	}
	.tb.is-compact .tb-logo b {
		font-size: 9px;
	}
</style>
