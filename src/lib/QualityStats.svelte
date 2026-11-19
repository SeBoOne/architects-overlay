<script lang="ts">
	// M8-K6 (t_f7a29367) + M8-R1-C (t_b6ee9f14): Stats-Vollanzeige.
	//

	// M8-R1-A (t_a81b04a6): INDIVIDUELLE Material-Regler — jeder Aspekt wird
	// mit seiner eigenen Aspekt-Qualität (mengengewichtetes Mittel seiner
	// Material-Regler, s. quality-calc.ts Header) interpoliert; Eigenschaften
	// über Aspekte multiplikativ. Der Gesamt-Wert (min über Aspekte) kommt
	// als combined-Quality rein und wird im Planer angezeigt. Die frühere
	// minQuality-Badge-Liste ist an die Regler umgezogen (Crafting.svelte).

	// R1-C-Spec: ALLE Stats untereinander — nicht nur die Q-unbeeinflussten.
	// Pro Statzeile: DEFAULT-Wert aus der API (vehicle_weapon/ammunition)
	// daneben der GEÄNDERTE Wert bei der aktuellen Ziel-Qualität (via
	// quality-calc: Modifier-Faktor linear zwischen at_min/at_max_quality).
	// Stats, deren Prop nicht in den Blueprint-Modifiern auftaucht, bleiben
	// neu = default und werden sauber als „unaffected“ ausgewiesen.
	// Modifier-Eigenschaften OHNE API-Basiswert (z. B. health_maxhealth —
	// das Item-DTO trägt keine Integrity-Zahl) zeigen „—“ als default und
	// die %-Abweichung — nichts wird erfunden.

	//
	// Key-Mapping (belegt, nicht geraten): `weapon_damage` skaliert die
	// vehicle_weapon-Damage-Werte (damage_per_shot + damages-Liste —
	// damage_per_shot ist die Summe der Typen, dieselbe Wertfamilie).
	// Alle anderen Modifier-Keys laufen als eigene Zeilen mit „—“ default.
	//
	// Ohne SC-Wiki-Match (keine Modifier, keine API-Stats) rendert der Block
	// NICHTS behauptendes — kein Hard-Fail, degradiert still.
	import { t } from './i18n';
	import {
		statChangesPerAspect,
		scaledStat,

		type Aspect,
		type SummaryProperty
	} from './quality-calc';

	type WeaponDamage = { damage_type: string; damage: number | null };
	type VehicleWeapon = {
		capacity: number | null;
		range: number | null;
		damage_per_shot: number | null;
		rpm: number | null;
		damages: WeaponDamage[];
	};
	type Ammunition = {
		size: number | null;
		capacity: number | null;
		speed: number | null;
		range: number | null;
		lifetime: number | null;
	};

	let {
		aspects,
		summary = null,
		aspectQualities,
		weapon = null,
		ammo = null
	}: {
		aspects: Aspect[];
		summary?: SummaryProperty[] | null;
		aspectQualities: Map<string, number>;
		weapon?: VehicleWeapon | null;
		ammo?: Ammunition | null;
	} = $props();

	// Aggregierte Faktoren je property_key: Produkt über die Aspekte bei
	// der JEWILS individuellen Aspekt-Qualität (M8-R1-A-Modell), plus
	// Modifier-Keys ohne API-Basiswert für die eigenen Zeilen.
	const changes = $derived(statChangesPerAspect(aspects ?? [], summary, aspectQualities));
	const factorByKey = $derived(new Map(changes.map((c) => [c.property_key, c])));
	const WEAPON_DAMAGE_KEY = 'weapon_damage';

	const fmt = (v: number | null | undefined, suffix = '') =>
		v === null || v === undefined ? '—' : `${Math.round(v * 100) / 100}${suffix ? ' ' + suffix : ''}`;

	type StatRow = {
		label: string;
		def: string;
		next: string;
		pct: number | null;
		better: boolean | null;
		affected: boolean;
	};

	// Statzeile mit API-Basiswert: default | ×Faktor = neu.
	function apiRow(
		label: string,
		base: number | null | undefined,
		unit: string,
		key: string | null
	): StatRow {
		const ch = key ? factorByKey.get(key) : undefined;
		const factor = ch?.factor ?? null;
		const scaled = scaledStat(base, factor);
		return {
			label,
			def: fmt(base, unit),
			next: scaled === null ? fmt(base, unit) : fmt(scaled, unit),
			pct: ch?.pct ?? null,
			better: ch?.better ?? null,
			affected: scaled !== null && scaled !== base
		};
	}

	// Modifier-only-Zeile (kein API-Basiswert): default „—“, neu als %.
	function modRow(key: string): StatRow {
		const ch = factorByKey.get(key);
		return {
			label: ch?.label ?? key,
			def: '—',
			next: ch?.pct === null || ch?.pct === undefined ? '—' : `${ch.pct > 0 ? '+' : ''}${ch.pct} %`,
			pct: ch?.pct ?? null,
			better: ch?.better ?? null,
			affected: true
		};
	}

	// Zeilen: API-Stats (weapon/ammo) zuerst, dann Modifier-Keys ohne
	// Basiswert (Erstauftretens-Reihenfolge aus summary_properties).
	const statRows = $derived.by(() => {
		const out: StatRow[] = [];
		const consumed = new Set<string>();
		if (weapon) {
			if (weapon.damage_per_shot !== null && weapon.damage_per_shot !== undefined) {
				out.push(apiRow($t('craft.dmg_per_shot'), weapon.damage_per_shot, '', WEAPON_DAMAGE_KEY));
				consumed.add(WEAPON_DAMAGE_KEY);
			}
			for (const d of weapon.damages ?? []) {
				out.push(apiRow(`${d.damage_type}`, d.damage, '', WEAPON_DAMAGE_KEY));
			}
			// M8-R1-C: ALLE Stats — auch ohne Wert als „—“ + unaffected.
			out.push(apiRow($t('craft.rpm'), weapon.rpm, '', null));
			out.push(apiRow($t('craft.range'), weapon.range, 'm', null));
			out.push(apiRow($t('craft.capacity'), weapon.capacity, '', null));
		}
		if (ammo) {
			out.push(apiRow($t('craft.ammo_speed'), ammo.speed, 'm/s', null));
			out.push(apiRow($t('craft.range'), ammo.range, 'm', null));
			out.push(apiRow($t('craft.capacity'), ammo.capacity, '', null));
			out.push(apiRow($t('craft.ammo_lifetime'), ammo.lifetime, 's', null));
		}
		for (const ch of changes) {
			if (!consumed.has(ch.property_key)) out.push(modRow(ch.property_key));
		}
		return out;
	});

	const show = $derived(
		((weapon ?? null) !== null || (ammo ?? null) !== null || changes.length > 0) === true
	);

</script>

{#if show}
	<div class="qs">
		<div class="qs-head">{$t('craft.stats_title')}</div>
		<!-- Vollanzeige: default | neu je Zeile (Spalten-Kopf) -->
		<div class="qs-cols">
			<span class="qs-col-l">{$t('craft.stats_default')}</span>
			<span class="qs-col-r">{$t('craft.stats_new')}</span>
		</div>
		<div class="qs-props">
			{#each statRows as r, ri (r.label + ':' + ri)}
				<div class="qs-prop" class:is-better={r.better === true} class:is-worse={r.better === false}>
					<span class="qs-prop-name">{r.label}</span>
					<span class="qs-prop-def">{r.def}</span>
					<span class="qs-prop-arrow">→</span>
					<span class="qs-prop-val">
						{#if r.affected}
							{r.next}{#if r.pct !== null && !r.next.includes('%')}
								&nbsp;({r.pct > 0 ? '+' : ''}{r.pct} %){/if}
						{:else}
							{r.def}<span class="qs-unaffected">{$t('craft.stats_unaffected')}</span>
						{/if}
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
		border: 1px solid var(--line, #2a2f3a);
		border-radius: 10px;
		padding: 10px 12px;
	}
	.qs-head {
		font-weight: 700;
		font-size: 12.5px;
		margin-bottom: 8px;
	}
	/* Spalten-Kopf: default | neu */
	.qs-cols {
		display: flex;
		justify-content: flex-end;
		gap: 10px;
		font-size: 9.5px;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		opacity: 0.6;
		margin-bottom: 4px;
	}
	.qs-props {
		display: flex;
		flex-direction: column;
		gap: 2px;
		margin-bottom: 8px;
	}
	.qs-prop {
		display: flex;
		align-items: baseline;
		gap: 8px;
		font-size: 12px;
		font-variant-numeric: tabular-nums;
	}
	.qs-prop-name {
		flex: 1;
		min-width: 0;
		opacity: 0.85;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.qs-prop-def {
		flex: none;
		min-width: 7ch;
		text-align: right;
		opacity: 0.7;
	}
	.qs-prop-arrow {
		flex: none;
		opacity: 0.45;
	}
	.qs-prop-val {
		flex: none;
		min-width: 9ch;
		text-align: right;
		font-weight: 600;
	}
	/* M8-R1-C: Q-unbeeinflusste Stats sauber ausweisen (Badge) */
	.qs-unaffected {
		margin-left: 6px;
		font-size: 9.5px;
		font-weight: 400;
		text-transform: uppercase;
		letter-spacing: 0.05em;
		opacity: 0.55;
	}
	.qs-prop.is-better .qs-prop-val {
		color: #4be08a;
	}
	.qs-prop.is-worse .qs-prop-val {
		color: #ff6b6b;
	}
	.qs-note {
		margin: 8px 0 0;
		font-size: 11px;
		line-height: 1.45;
		opacity: 0.7;
	}
</style>
