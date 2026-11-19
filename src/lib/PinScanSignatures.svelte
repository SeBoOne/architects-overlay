<script lang="ts">
	// M7-F3 (t_cf1224c5) + M7-F4 (t_c3367dfb): generischer Pin-Content-Renderer
	// „scan-signatures“ (Plural) — EIN Sammel-Pin zeigt ALLE angepinnten
	// Signaturen. Pro Erz EIN Chip (Rarity-Border + translucent Fill — Spec
	// M7-F4 P2): Kürzel + Basis-Signatur GENAU EINMAL (kein Kopf-KPI-Doppelrender,
	// keine Singular/Plural-Splitting — Spec P1), daneben die Multiplikatoren
	// ×1..×5 (Basis·n, Strata-Signatur-Chart-Konvention). Payload-Vertrag
	// (scan-signature.ts): { entries: [{oreId,label,value,rarity?}], visibleSlots? }
	// — generisch, KEIN Strata-Vokabular. `visibleSlots` ist nachrüstbar
	// (Pin-Resize-Welle, bis ×10) — Default 5.
	// Vertrag (M4-E/M5/M6-F1): reine Anzeige, keine Interaktion hier.
	import { t } from './i18n';
	import type { PinEntry } from './pins';
	import {
		sigEntries,
		sigMultiples,
		sigValueDisplay,
		type ScanSignaturesPayload
	} from './scan-signature';
	import { oreAbbrev } from './ore-abbrev';
	import { rarityColor } from './rarity';

	let { pin }: { pin: PinEntry; interactive?: boolean } = $props();

	const payload = $derived((pin.content ?? {}) as Partial<ScanSignaturesPayload>);
	const entries = $derived(sigEntries(payload));
	const slots = $derived(payload.visibleSlots ?? 5);
</script>

<div class="sigs" aria-label={$t('pins.scan_collect_title')}>
	{#if entries.length === 0}
		<div class="sigs-empty">{$t('pins.scan_collect_empty')}</div>
	{:else}
		{#each entries as e (e.oreId)}
			<div class="sigs-row" style={`--rc:${rarityColor(e.rarity) ?? 'var(--lineb)'}`} title={e.rarity ?? ''}>
				<!-- M7-F5d (User-Mockup 2026-11-19): Kürzel + Basis-Signatur in EINEM
				     Rarity-Kasten; jede weitere Stufe (×2..×n) eigener Kasten,
				     verbunden mit einfachen Strichen. Die Basis IST ×1 — kein Doppel. -->
				<span class="sigs-chip" title={e.label}>
					<span class="sigs-abbr">{oreAbbrev(e.label)}</span>
					<span class="sigs-base">{sigValueDisplay(e.value)}</span>
				</span>
				{#each sigMultiples(e.value, slots) as m (m.n)}
					<span class="sigs-link" aria-hidden="true"></span>
					<span class="sigs-cell">{m.display}</span>
				{/each}
			</div>
		{/each}
	{/if}
</div>

<style>
	.sigs {
		display: flex;
		flex-direction: column;
		gap: 3px;
		padding: 2px 0 4px;
	}
	.sigs-empty {
		font: 11px/1.3 var(--font-mono, monospace);
		color: var(--fgdim, #9aa3b2);
	}
	/* M7-F4 P2: Rarity-Chip — Umrandung bevorzugt (Pin ist transparent), dezenter
	   translucent Fill als zweiter Layer. Farbe aus der gemeinsamen Rampe
	   (rarity.ts), konsistent mit den Mode-Badges. */
	.sigs-row {
		display: flex;
		align-items: center;
		gap: 5px;
		min-width: 0;
		font: 600 12px/1.3 var(--font-mono, monospace);
	}
	/* Basis-Kasten: Kürzel + Basis-Signatur zusammen, Rarity-Border + Fill. */
	.sigs-chip {
		flex: 0 0 auto;
		display: flex;
		align-items: baseline;
		gap: 5px;
		padding: 2px 6px;
		border: 1px solid var(--rc, var(--lineb));
		background: color-mix(in srgb, var(--rc, var(--lineb)) 14%, transparent);
		border-radius: 3px;
	}
	.sigs-abbr {
		color: var(--fg);
		font-weight: 700;
	}
	.sigs-base {
		color: var(--accent);
	}
	/* Verbindung: einfacher Strich zwischen den Kästen (gedimmt Rarity-Farbe). */
	.sigs-link {
		flex: 0 0 9px;
		height: 1px;
		background: color-mix(in srgb, var(--rc, var(--lineb)) 45%, transparent);
	}
	/* Stufen-Kasten: eigener Rahmen (gedimmt), transparenter Fill. */
	.sigs-cell {
		flex: 0 0 auto;
		padding: 2px 5px;
		border: 1px solid color-mix(in srgb, var(--rc, var(--lineb)) 45%, transparent);
		border-radius: 3px;
		color: var(--fgdim, #9aa3b2);
		white-space: nowrap;
	}
</style>
