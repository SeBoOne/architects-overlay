<script lang="ts">
	// M7-F3 (t_cf1224c5, Punkt 2): generischer Pin-Content-Renderer
	// „scan-signatures“ (Plural) — EIN Sammel-Pin zeigt ALLE angepinnten
	// Signaturen: pro Zeile Kürzel + Basis-Signatur, daneben die Multiplikatoren
	// ×1..×5 (Basis·n, Strata-Signatur-Chart-Konvention: 2 Vorkommen = 2× Basis).
	// Payload-Vertrag (scan-signature.ts): { entries: [{oreId,label,value}],
	// visibleSlots? } — generisch, KEIN Strata-Vokabular. `visibleSlots` ist
	// nachrüstbar (Pin-Resize-Welle, bis ×10) — Default 5.
	// Vertrag (M4-E/M5/M6-F1): reine Anzeige, keine Interaktion hier.
	import { t } from './i18n';
	import type { PinEntry } from './pins';
	import {
		sigMultiples,
		sigValueDisplay,
		type ScanSignaturesPayload
	} from './scan-signature';
	import { oreAbbrev } from './ore-abbrev';

	let { pin }: { pin: PinEntry; interactive?: boolean } = $props();

	const payload = $derived((pin.content ?? {}) as Partial<ScanSignaturesPayload>);
	const entries = $derived(payload.entries ?? []);
	const slots = $derived(payload.visibleSlots ?? 5);
</script>

<div class="sigs" aria-label={$t('pins.scan_collect_title')}>
	{#if entries.length === 0}
		<div class="sigs-empty">{$t('pins.scan_collect_empty')}</div>
	{:else}
		{#each entries as e (e.oreId)}
			<div class="sigs-row">
				<span class="sigs-abbr" title={e.label}>{oreAbbrev(e.label)}</span>
				<span class="sigs-base">{sigValueDisplay(e.value)}</span>
				<span class="sigs-mult">
					{#each sigMultiples(e.value, slots) as m, i (m.n)}
						<span class="sigs-mult-cell" class:is-first={i === 0}>{m.display}</span>
					{/each}
				</span>
			</div>
		{/each}
	{/if}
</div>

<style>
	.sigs {
		display: flex;
		flex-direction: column;
		gap: 2px;
		padding: 2px 0 4px;
	}
	.sigs-empty {
		font: 11px/1.3 var(--font-mono, monospace);
		color: var(--fgdim, #9aa3b2);
	}
	.sigs-row {
		display: flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
		font: 600 12px/1.3 var(--font-mono, monospace);
	}
	.sigs-abbr {
		flex: 0 0 auto;
		min-width: 2.4ch;
		color: var(--fg);
		font-weight: 700;
	}
	.sigs-base {
		flex: 0 0 auto;
		min-width: 4ch;
		text-align: right;
		color: var(--accent);
	}
	.sigs-mult {
		flex: 1;
		display: flex;
		gap: 6px;
		min-width: 0;
		overflow: hidden;
	}
	.sigs-mult-cell {
		color: var(--fgdim, #9aa3b2);
		white-space: nowrap;
	}
	.sigs-mult-cell.is-first {
		color: var(--fg);
	}
</style>
