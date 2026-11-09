// M2 (t_a931d1bb): Breakpoint-/Kompakt-Logik des Fenstermodus (app-window).
//
// Pures Modul ohne Svelte-/CSS-Abhängigkeit — die Schwellwerte des
// Kompakt/Breit-Schritts (Mockup mockup-fenstermodus-b.html: Breit ~1400 px
// als Referenz, Kompakt ~900 px) sind hier zentral und unit-testbar
// (node --test, Kommentar unten). Alle VISUELLEN Folgen sind strikt
// hinter `html[data-window="app-window"]`-Scoping in app.css bzw. hinter
// die `.app-compact`-Klasse (OVERLAY-REGEL: Spotlight bleibt unverändert).
//
// Eigener Ad-hoc-Test: `node --test src/lib/window-breakpoints.test.ts`
// (node ≥22.7 type-stripping; kein eigener Runner in den CI-Gates, weil
// das Frontend nur `pnpm run check` besitzt — der Test dokumentiert die
// Schwellwerte trotzdem maschinell).

/** Unterhalb dieser Fensterbreite greift der Kompakt-Schritt. */
export const APP_WINDOW_COMPACT_PX = 900;

/** Referenz „Breit“ des Mockups B (volle Ausstattung ab hier). */
export const APP_WINDOW_WIDE_PX = 1400;

/**
 * Kompakt-Schritt des app-window: nur im Fenstermodus, nur bei gemessener
 * (echter) Breite, unterhalb des Schwellwerts. `width <= 0` gilt als
 * „noch nicht gemessen“ (bind:clientWidth initial) → nie kompakt.
 */
export function appCompact(width: number, windowMode: boolean): boolean {
	return windowMode && width > 0 && width < APP_WINDOW_COMPACT_PX;
}

/**
 * „+N mehr“-Anzeige der Hint-Zeile: im Kompakt-Schritt zeigen wir die
 * ersten drei Hints, ALLE weiteren als „+N“ (Mehr-Anzeige statt stiller
 * Kürzung, Mockup-Defekt „stille Kürzung“ wird nicht nachgebaut).
 */
export function hintMore(count: number, compact: boolean): number {
	return compact ? Math.max(0, count - 3) : 0;
}
