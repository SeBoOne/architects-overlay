<script lang="ts">
	// M7-K3 (t_cfea5bea): generischer Pin-Content-Renderer „scan-signature".
	// Zeigt Label (z. B. Ore-Name), Scan-Signatur-Wert („—" bei fehlenden
	// Daten), Orientierungsbalken relativ zum Chart-Max (payload.max) und
	// optionalen Tier-Badge der gemappten Quelle.
	// Vertrag (M4-E/M5/M6-F1): reine Anzeige — KEINE Interaktion hier, der
	// Renderer hat denselben Gate wie DealChat (`interactive` nur für die
	// Registrierungs-Schnittstelle; scan-signature rendert keinen Editor).
	// Kein Strata-Vokabular: Payload ist {label, value, tier?, max?} generisch
	// (scan-signature.ts).
	import { t } from './i18n';
	import type { PinEntry } from './pins';
	import { sigBarPct, sigTierBadge, sigValueDisplay, type ScanSignaturePayload } from './scan-signature';

	let { pin }: { pin: PinEntry; interactive?: boolean } = $props();

	const payload = $derived((pin.content ?? {}) as Partial<ScanSignaturePayload>);
	const label = $derived(payload.label ?? pin.title);
	const valueText = $derived(sigValueDisplay(payload.value));
	const barPct = $derived(sigBarPct(payload.value, payload.max));
	const tier = $derived(sigTierBadge(payload.tier));
</script>

<div class="sig" aria-label={$t('pins.scan_title')}>
	<div class="sig-row">
		<span class="sig-label">{label}</span>
		{#if tier}<span class="sig-tier">{tier}</span>{/if}
	</div>
	<div class="sig-row">
		<span class="sig-value">{valueText}</span>
		{#if barPct !== null}
			<span
				class="sig-bar"
				title={$t('pins.scan_bar_title')}
				role="img"
				aria-label={$t('pins.scan_bar_title')}
			>
				<i style={`width:${barPct}%`}></i>
			</span>
		{/if}
	</div>
</div>

<style>
	.sig {
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 2px 0 4px;
	}
	.sig-row {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}
	.sig-label {
		flex: 1;
		min-width: 0;
		font: 600 12.5px/1.3 var(--font-mono, monospace);
		color: var(--fg);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.sig-tier {
		flex: 0 0 auto;
		font: 700 9.5px/1 var(--font-mono, monospace);
		color: var(--accent);
		border: 1px solid var(--lineb);
		padding: 2px 5px;
	}
	.sig-value {
		flex: 0 0 auto;
		font: 600 12.5px/1.15 var(--font-mono, monospace);
		color: var(--accent);
		min-width: 3ch;
	}
	/* Orientierungsbalken: Spur = Panel2, Füllung = Akzent; Breite in % vom
	   Chart-Max (renderer-seitig geclampt, scan-signature.ts sigBarPct). */
	.sig-bar {
		flex: 1;
		min-width: 40px;
		height: 8px;
		background: var(--panel3);
		border: 1px solid var(--lineb);
		position: relative;
		overflow: hidden;
	}
	.sig-bar i {
		position: absolute;
		inset: 0 auto 0 0;
		background: var(--accent);
		display: block;
	}
</style>
