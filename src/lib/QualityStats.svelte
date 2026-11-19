<script lang="ts">
	// M8-K6 (t_f7a29367): Echter Qualitäts-Stat-Rechner — UI-Block.
	//
	// M8-R1-A (t_a81b04a6): INDIVIDUELLE Material-Regler — jeder Aspekt wird
	// mit seiner eigenen Aspekt-Qualität (mengengewichtetes Mittel seiner
	// Material-Regler, s. quality-calc.ts Header) interpoliert; Eigenschaften
	// über Aspekte multiplikativ. Der Gesamt-Wert (min über Aspekte) kommt
	// als combined-Quality rein und wird im Planer angezeigt. Die frühere
	// minQuality-Badge-Liste ist an die Regler umgezogen (Crafting.svelte).
	//
	// Ohne Modifier-Daten (Blueprint nicht gematcht, Ranges fehlen) rendert
	// der Block NICHTS behauptendes — kein Hard-Fail, degradiert still.
	import { t } from './i18n';
	import {
		statChangesPerAspect,
		type Aspect,
		type SummaryProperty
	} from './quality-calc';

	let {
		aspects,
		summary = null,
		aspectQualities
	}: {
		aspects: Aspect[];
		summary?: SummaryProperty[] | null;
		aspectQualities: Map<string, number>;
	} = $props();

	const changes = $derived(statChangesPerAspect(aspects ?? [], summary, aspectQualities));
</script>

{#if changes.length > 0}
	<div class="qs">
		<div class="qs-head">{$t('craft.stats_title')}</div>
		<!-- Eigenschaften in summary_properties-Reihenfolge, Ampel via better_when -->
		<div class="qs-props">
			{#each changes as ch (ch.property_key)}
				<div class="qs-prop" class:is-better={ch.better === true} class:is-worse={ch.better === false}>
					<span class="qs-prop-name">{ch.label}</span>
					<span class="qs-prop-val">
						{#if ch.pct === null}—{:else}{ch.pct > 0 ? '+' : ''}{ch.pct} %{/if}
					</span>
				</div>
			{/each}
		</div>
		<p class="qs-note">{$t('craft.stats_note')}</p>
	</div>
{/if}

<style>
	.qs {
		margin-top: 10px;
		border-top: 1px dashed var(--line, #2a2f3a);
		padding-top: 8px;
	}
	.qs-head {
		font-weight: 700;
		font-size: 12px;
		margin-bottom: 6px;
	}
	.qs-props {
		display: flex;
		flex-direction: column;
		gap: 2px;
		margin-bottom: 8px;
	}
	.qs-prop {
		display: flex;
		justify-content: space-between;
		gap: 10px;
		font-size: 12px;
		font-variant-numeric: tabular-nums;
	}
	.qs-prop.is-better .qs-prop-val {
		color: #4be08a;
	}
	.qs-prop.is-worse .qs-prop-val {
		color: #ff6b6b;
	}
	.qs-prop-name {
		opacity: 0.85;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.qs-note {
		margin: 8px 0 0;
		font-size: 11px;
		line-height: 1.45;
		opacity: 0.7;
	}
</style>
