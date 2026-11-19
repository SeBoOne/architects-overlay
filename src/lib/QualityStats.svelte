<script lang="ts">
	// M8-K6 (t_f7a29367): Echter Qualitäts-Stat-Rechner — UI-Block.
	//
	// Zeigt pro Aspekt (Frame/Cycler/…) die betroffenen Eigenschaften mit
	// der berechneten %-Änderung bei der gewählten Ziel-Qualität (grün/rot
	// je better_when-Richtung) und pro Material dessen min_quality-Badge
	// samt Erreichbarkeits-Check. Die Rechenlogik liegt in quality-calc.ts
	// (live gegen die SC-Wiki-API verifiziertes Modell: Modifier-Faktor
	// linear zwischen at_min_quality/at_max_quality über quality_range).
	//
	// Ohne Modifier-Daten (Blueprint nicht gematcht, Ranges fehlen) rendert
	// der Block NICHTS behauptendes — kein Hard-Fail, degradiert still.
	import { t } from './i18n';
	import {
		statChanges,
		qualityReachable,
		type Aspect,
		type SummaryProperty
	} from './quality-calc';

	let {
		aspects,
		summary = null,
		targetQuality
	}: {
		aspects: Aspect[];
		summary?: SummaryProperty[] | null;
		targetQuality: number;
	} = $props();

	const changes = $derived(statChanges(aspects ?? [], summary, targetQuality));

	// Pro Aspekt: berechenbare Modifier (für die Anzeige) + Material-Check.
	const rows = $derived(
		(aspects ?? []).map((a) => ({
			aspect: a,
			stats: statChanges([a], summary, targetQuality),
			reachable: qualityReachable(targetQuality, a.input?.min_quality ?? null)
		}))
	);
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
		<!-- Pro Aspekt: Material + min_quality-Badge + Erreichbarkeit -->
		<div class="qs-aspects">
			{#each rows as r (r.aspect.key)}
				<div class="qs-aspect">
					<span class="qs-aspect-name">{r.aspect.name}</span>
					{#if r.aspect.input?.name}
						<span class="qs-mat">{r.aspect.input.name}</span>
					{/if}
					<span
						class="qs-mq"
						class:is-ok={r.reachable}
						class:is-fail={!r.reachable}
						title={r.aspect.input?.min_quality == null
							? $t('craft.min_quality_none')
							: $t('craft.min_quality') + ' ' + r.aspect.input.min_quality}
					>
						{r.aspect.input?.min_quality == null ? '—' : String(r.aspect.input.min_quality)}
					</span>
					<span class="qs-reach" class:is-ok={r.reachable} class:is-fail={!r.reachable}>
						{r.reachable ? $t('craft.stats_reachable') : $t('craft.stats_unreachable')}
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
	.qs-aspects {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}
	.qs-aspect {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 11.5px;
	}
	.qs-aspect-name {
		font-weight: 600;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.qs-mat {
		opacity: 0.7;
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.qs-mq {
		flex: none;
		width: 38px;
		text-align: center;
		font-size: 10.5px;
		padding: 1px 0;
		border-radius: 6px;
		border: 1px solid var(--line, #2a2f3a);
		opacity: 0.8;
		font-variant-numeric: tabular-nums;
	}
	.qs-mq.is-ok {
		border-color: #4be08a;
		opacity: 1;
	}
	.qs-mq.is-fail {
		border-color: #ff6b6b;
		opacity: 1;
	}
	.qs-reach {
		flex: none;
		font-size: 10.5px;
	}
	.qs-reach.is-ok {
		color: #4be08a;
	}
	.qs-reach.is-fail {
		color: #ff6b6b;
	}
	.qs-note {
		margin: 8px 0 0;
		font-size: 11px;
		line-height: 1.45;
		opacity: 0.7;
	}
</style>
