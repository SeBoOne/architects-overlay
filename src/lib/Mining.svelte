<script lang="ts">
	// Mining-Modus (M7-K2, t_bb3303d0) — F5-Modus im Rail/F-Keys-System.
	//
	// Datenquelle: Strata Public API via `strata_client` (M7-K1) — die Tauri-
	// Commands `strata_*` liefern cache-first Responses inkl. Pflicht-
	// Attribution. Diese Komponente ist der F5-View:
	//
	//   1. Erz-Suche     — Typeahead (Name/Slug, case-insensitiv), Filter
	//                      category (ship/fps) + Rarity (M7-F1); Liste alphabetisch;
	//                      Pin-Button „Signatur pinnen" pro Erz-Zeile
	//                      (Pin-Content `scan-signature`, Rendering in K3).
	//   2. Ort-Suche     — Typeahead, Filter system + type.
	//   3. Detail        — „Orte pro Erz" (ore-locations), gruppiert nach
	//                      Umgebung (space/surface/cave/exploration). Gruppen
	//                      sind NICHT untereinander vergleichbar (eigene
	//                      Skala je Gruppe — als Gruppentitel sichtbar).
	//                      M7-F3: Kein Dial mehr — Karte = Kürzel-Block
	//                      (1.25:1) + Rang-Balken (kontinuierlich) +
	//                      Konzentrations-Balken (6 feste Stufen, Gitterlinien
	//                      bei 1/4/8/15/25 %), responsives Grid (1–6 Spalten).
	//                      Rang als PLATZIERUNG („Nr. 1“/„#1“), nicht Prozent.
	//
	// M7-F3 (Punkt 8): Die internen Text-Filterfelder sind ENTFERNT — das
	// HAUPT-Suchfeld der Palette filtert als In-Context-Filter die aktuelle
	// Liste (Tab-abhängig, Muster Inventory `filter`-Prop). Kategorie/Typ/
	// Seltenheits-Chips bleiben als Kompakt-Filter-Zeile.
	//
	// Formular-Konvention (Sebo): Typeaheads starten LEER mit Gesamtliste,
	// sind MAUS-klickbar und filtern auf Tasteneingabe.
	//
	// Key-Missing-State: kein `celd_`-Key im Keystore → Erklär-Card mit Link
	// https://strata.celd.space/api-keys. Attribution-Footer IMMER sichtbar,
	// wenn Strata-Daten angezeigt werden (ToS-Pflicht).
	import { t, tr, trError } from './i18n';
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import { oreAbbrev } from './ore-abbrev';
	// M7-F4 (t_c3367dfb): Rarity-Rampe an EINEM Ort (rarity.ts) — Mode-Badges
	// und Pin-Chips/Ort-Kacheln teilen sich dieselbe Skala (Konsistenz P2).
	import { RARITIES, RARITY_COLORS, rarityColor, type Rarity } from './rarity';
	import { sigToggleEntry, type ScanSignatureEntry } from './scan-signature';
	import { PINS_CHANGED_EVENT, type PinEntry, type PinsState } from './pins';

	type Ore = {
		id: string;
		name: string;
		slug: string;
		category: string | null; // 'ship' | 'fps' | 'ground_vehicle' — null-fähig
		// M7-F1: Live-Contract — tier ist String-Rarity (common..legendary),
		// NICHT die i64-Zahl aus dem (stalen) Spec. null-check, nie truthiness.
		tier: string | null;
		scanSignature: number | null; // 0 ist gültige Daten
		instability: number | null; // unbeschränkt (live: 350! / negativ) — kein Clamp
		resistance: number | null; // unbeschränkt — kein Clamp
	};
	type OresResponse = { ores: Ore[]; count: number; attribution: string };
	type Location = { id: string; name: string; slug: string; system: string | null; type: string }; // system live null-fähig (Dungeons)
	type LocationsResponse = { locations: Location[]; count: number; attribution: string };
	type LocationRow = {
		locationId: string;
		location: string;
		system: string;
		rankPct: number;
		rank: number;
		n: number;
		concentration: number;
		tier: number;
		tierName: string;
	};
	type OreLocationsResponse = {
		ore: { id: string; name: string };
		sort: string;
		tierLegend: Record<string, string> | null;
		results: {
			space: LocationRow[];
			surface: LocationRow[];
			cave: LocationRow[];
			exploration: LocationRow[];
		};
		attribution: string;
	};
	// Ort-Detail (M7-F2): `location-ores/{location}` — Erze am Ort, nach
	// Konzentration absteigend (liefert die API bereits). Reine Info, kein Pin.
	type OreRow = {
		oreId: string;
		name: string;
		concentration: number;
		tier: number;
		tierName: string;
		scanSignature: number | null;
	};
	type LocationOresResponse = {
		location: { id: string; name: string; system: string; type: string };
		ores: OreRow[];
		count: number;
		attribution: string;
	};
	type StrataStatus = {
		enabled: boolean;
		key_set: boolean;
		key_ref: string;
		tier_default: number;
	};

	let { onexit, filter = $bindable('') }: { onexit: () => void; filter?: string } = $props();

	// ---- State ---------------------------------------------------------------
	let status = $state<StrataStatus | null>(null);
	let checked = $state(false); // Status-Load abgeschlossen
	let ores = $state<Ore[]>([]);
	let locations = $state<Location[]>([]);
	let attribution = $state('');
	let loading = $state(false);
	let error = $state('');

	let view = $state<'ores' | 'locations'>('ores');
	// M7-F3 (Punkt 8): oreQuery/locQuery ENTFERNT — `filter` (Haupt-Suchfeld)
	// filtert tab-abhängig die aktuelle Liste.
	let catFilter = $state<'all' | 'ship' | 'fps'>('all');
	// M7-F1: Tier-Filter 1–6 war mit String-Rarity SINNLOS — Rarity-Filter.
	let rarityFilter = $state<'all' | Rarity>('all');
	// Ort-Suche (nur Chips-Filter — Textfilter via Haupt-Suchfeld, M7-F3)
	let sysFilter = $state('all');
	let typeFilter = $state('all');
	// Detail
	let detail = $state<OreLocationsResponse | null>(null);
	let detailOre = $state<Ore | null>(null); // Scan-Signatur für das Zentrum-Badge
	let detailLoading = $state(false);
	let detailError = $state('');
	let sortParam = $state<'rank' | 'concentration'>('rank');
	// Ort-Detail (M7-F2)
	let locDetail = $state<LocationOresResponse | null>(null);
	let locDetailLoading = $state(false);
	let locDetailError = $state('');
	// Pin-Feedback (Statuszeile unten, Muster f5Msg)
	let note = $state('');
	let noteTimer: ReturnType<typeof setTimeout> | null = null;

	const ATTRIBUTION_FALLBACK = 'Data from celd.space / Strata Mining Tools';

	// ---- M7-F1: Rarity (Live-Contract) ---------------------------------------
	// ores[].tier ist String-Rarity — die Tier-Zahl 1–6 existiert nur im
	// Detail (ore-locations/location-ores), dort bleibt tierColor(tier: number)
	// unverändert. Badge-Rampe: common grau → legendary mint.

	// 6-Stufen-Farbrampe (Orange→Amber→Oliv→Gelbgrün→Hellgrün→Mint) —
	// absolute Bands (Tier 1–6 der API), überall gleich interpretierbar.
	const TIER_COLORS: Record<number, string> = {
		1: '#d97742',
		2: '#e0a83e',
		3: '#a8a23e',
		4: '#9dc451',
		5: '#6bd694',
		6: '#4be08a'
	};
	// M7-F4 P3: Rarity-String fuer eine OreRow (Ort-Detail) — Lookup in der
	// Erz-Referenzliste (OreRow traegt nur die Tier-Zahl).
	function rarityOfOreId(oreId: string): string | null {
		return ores.find((o) => o.id === oreId)?.tier ?? null;
	}

	function tierColor(tier: number): string {
		return TIER_COLORS[Math.min(6, Math.max(1, tier))] ?? TIER_COLORS[1];
	}

	function flash(msg: string) {
		note = msg;
		if (noteTimer) clearTimeout(noteTimer);
		noteTimer = setTimeout(() => (note = ''), 3000);
	}

	// ---- Load ----------------------------------------------------------------
	async function loadStatus() {
		checked = false;
		error = '';
		try {
			status = await invoke<StrataStatus>('strata_status');
		} catch (e) {
			error = trError(e);
		}
		checked = true;
		if (status?.enabled && status?.key_set) void loadReference();
	}

	async function loadReference() {
		loading = true;
		error = '';
		try {
			const [o, l] = await Promise.all([
				invoke<OresResponse>('strata_ores'),
				invoke<LocationsResponse>('strata_locations')
			]);
			ores = [...o.ores].sort((a, b) => a.name.localeCompare(b.name));
			locations = [...l.locations].sort((a, b) => a.name.localeCompare(b.name));
			attribution = o.attribution || ATTRIBUTION_FALLBACK;
		} catch (e) {
			error = trError(e);
		}
		loading = false;
	}

	$effect(() => {
		void loadStatus();
	});

	// ---- Filter (LEER = Gesamtliste, case-insensitiv Name/Slug; M7-F3: das
	// Haupt-Suchfeld der Palette liefert den Text, Chips bleiben lokal) ----
	const oreMatches = (o: Ore, q: string) =>
		o.name.toLowerCase().includes(q) || o.slug.toLowerCase().includes(q);
	const filteredOres = $derived(
		ores.filter((o) => {
			const q = filter.trim().toLowerCase();
			if (q && !oreMatches(o, q)) return false;
			if (catFilter !== 'all' && o.category !== catFilter) return false;
			if (rarityFilter !== 'all' && o.tier !== rarityFilter) return false;
			return true;
		})
	);
	// M7-F1: system ist live null-fähig — dynamisch aus Daten (inkl. „—“-Slot
	// für null), kein hartkodiertes Spec-Vokabular.
	const systems = $derived([...new Set(locations.map((l) => l.system ?? '—'))].sort());
	// M7-F3 (Punkt 7): locTypes bleibt dynamisch aus den Daten — ein Typ ohne
	// mindestens 1 Ort erscheint gar nicht erst (z. B. station, wenn keine
	// Station Minable-Elemente listet). Anzeigenamen via i18n (type_*).
	const locTypes = $derived([...new Set(locations.map((l) => l.type))].sort());
	const filteredLocations = $derived(
		locations.filter((l) => {
			const q = filter.trim().toLowerCase();
			if (q && !(l.name.toLowerCase().includes(q) || l.slug.toLowerCase().includes(q)))
				return false;
			if (sysFilter !== 'all' && (l.system ?? '—') !== sysFilter) return false;
			if (typeFilter !== 'all' && l.type !== typeFilter) return false;
			return true;
		})
	);
	// M7-F3 (Punkt 7): Anzeigename eines Ort-Typs (Wire-Wert bleibt z. B.
	// `dungeon` für Caves — NUR das Label wird übersetzt).
	const typeName = (ty: string) => tr(`mining.type_${ty}`);
	// M7-F3 (Punkt 7): Seltenheits-Label (EN capitalized, DE Begriffe) —
	// null/leer zeigt „—“ (kein „None“).
	const rarityName = (r: string | null) => (r ? tr(`mining.rarity_${r}`) : '—');
	// M7-F3 (Punkt 7): Tier-Badges/Legende über i18n-Keys (tier_1..6 =
	// Band-Label „Pure 25%+“-Stil) statt rohem API-tierName — konsistent in
	// Legende UND Badges, in beiden Locales.
	const tierName = (tier: number) => tr(`mining.tier_${tier}`);

	// ---- Detail --------------------------------------------------------------
	async function openDetail(ore: Ore) {
		detailOre = ore;
		detailLoading = true;
		detailError = '';
		detail = null;
		try {
			detail = await invoke<OreLocationsResponse>('strata_ore_locations', {
				ore: ore.id,
				sort: sortParam
			});
			attribution = detail.attribution || ATTRIBUTION_FALLBACK;
		} catch (e) {
			detailError = trError(e);
		}
		detailLoading = false;
	}
	function setSort(s: 'rank' | 'concentration') {
		if (sortParam === s || !detailOre) return;
		sortParam = s;
		void openDetail(detailOre);
	}
	function closeDetail() {
		detail = null;
		detailOre = null;
		detailError = '';
	}

	// Ort-Detail (M7-F2): `strata_location_ores` — Erze am Ort (reine Info).
	async function openLocationDetail(loc: Location) {
		locDetailLoading = true;
		locDetailError = '';
		locDetail = null;
		try {
			locDetail = await invoke<LocationOresResponse>('strata_location_ores', {
				location: loc.id
			});
			attribution = locDetail.attribution || ATTRIBUTION_FALLBACK;
		} catch (e) {
			locDetailError = trError(e);
		}
		locDetailLoading = false;
	}
	function closeLocationDetail() {
		locDetail = null;
		locDetailError = '';
	}

	// Gruppen-Reihenfolge wie Strata: Haupt-Ziele → explorable Regions →
	// Caves → Surface. Leere Gruppen entfallen.
	const GROUPS = [
		['space', 'mining.group_space'],
		['exploration', 'mining.group_exploration'],
		['cave', 'mining.group_cave'],
		['surface', 'mining.group_surface']
	] as const;
	const detailGroups = $derived.by(() => {
		const d = detail;
		if (!d) return [];
		return GROUPS.map(([key, label]) => ({ key, label, rows: d.results[key] })).filter(
			(g) => g.rows.length > 0
		);
	});

	// ---- Sammel-Signatur-Pin (M7-F3, Punkt 2) --------------------------------
	// EIN Pin-Content „scan-signatures“ (Plural) sammelt ALLE Signaturen —
	// der Pin-Button am Erz TOGGLET (addet/entfernt, dedupe nach oreId).
	// Payload generisch: {type:'scan-signatures', entries:[{oreId,label,value}]}
	// (KEIN Strata-Vokabular, Universality-Regel). `visibleSlots` ist im
	// Payload-Vertrag nachrüstbar (Pin-Resize-Welle, bis ×10) — siehe
	// scan-signature.ts. Rendering: PinScanSignatures (K3-Registry).
	//
	// Pin-Identität: fester Key `scan-signatures` + box_index-Basis 9903 —
	// pin_add kann bei belegter (view/screen/box)-Identität ablehnen, dann
	// wird der nächste Index probiert (der Screen wird backend-seitig
	// gewählt, das Frontend kann ihn nicht vorwählen).
	const COLLECT_KEY = 'scan-signatures';
	const COLLECT_BOX_BASE = 9903;

	// Gepinnte Pinned-Marker für die Erz-Liste: bei PINS_CHANGED_EVENT
	// nachziehen (gleicher Mechanismus wie PinBoxes/+page pinsMeta).
	let pins = $state<PinEntry[]>([]);
	const pinnedOreIds = $derived(
		new Set(
			pins
				.filter((p) => p.view_id === 'mining' && p.key === COLLECT_KEY)
				.flatMap((p) => {
					const entries = (p.content as { entries?: ScanSignatureEntry[] } | null)?.entries;
					return (entries ?? []).map((e) => e.oreId);
				})
		)
	);
	async function loadPins() {
		try {
			const st = await invoke<PinsState>('pins_state');
			pins = st.pins;
		} catch {
			pins = [];
		}
	}
	$effect(() => {
		void loadPins();
		void listen(PINS_CHANGED_EVENT, () => void loadPins());
	});

	function collectRows(entries: ScanSignatureEntry[]) {
		// rows-Snapshot (Höhen-Schätzung pins.rs: 44 + 34/Zeile) — pro Erz
		// eine Zeile; die reichhaltige ×1–×5-Anzeige macht der Content-Renderer.
		return entries.map((e, i) => ({
			idx: String(i),
			name: oreAbbrev(e.label),
			sub: e.label,
			value: e.value === null ? '—' : String(e.value),
			side: null
		}));
	}

	type SigToggleArg = { id: string; name: string; scanSignature: number | null; tier: string | null };
	async function toggleSignaturePin(ore: SigToggleArg) {
		try {
			const st = await invoke<PinsState>('pins_state');
			const collect = st.pins.find(
				(p) => p.view_id === 'mining' && p.pinned && p.key === COLLECT_KEY
			);
			const current = (
				(collect?.content ?? null) as { entries?: ScanSignatureEntry[] } | null
			)?.entries;
			const entries = sigToggleEntry(current ?? [], {
				oreId: ore.id,
				label: ore.name,
				value: ore.scanSignature,
				// M7-F4 P2: Rarity-String (ores[].tier) in die Payload — der
				// Pin-Renderer faerbt den Chip ueber rarity.ts.
				rarity: ore.tier
			});
			const isAdd = entries.some((e) => e.oreId === ore.id);
			if (isAdd) {
				// M7-F4 P1 (Dedupe): Legacy-Einzelpins (M7-K3, Typ 'scan-signature')
				// desselben Erzes entfernen — der Wert stünde sonst doppelt auf
				// der Pin-Flaeche (alter K3-Pin + Sammel-Pin).
				for (const p of st.pins) {
					if (!p.pinned || p.view_id !== 'mining') continue;
					const legacy = p.content as { type?: string; label?: string } | null;
					if (
						(p.key === 'scan-signature' || legacy?.type === 'scan-signature') &&
						legacy?.label === ore.name
					) {
						await invoke('pin_remove', {
							view_id: 'mining',
							screen: p.screen,
							box_index: p.box_index
						});
					}
				}
			}
			if (entries.length === 0) {
				// leer → Sammel-Pin wird entfernt (Spec Punkt 2)
				if (collect) {
					await invoke('pin_remove', {
						view_id: 'mining',
						screen: collect.screen,
						box_index: collect.box_index
					});
					flash(tr('pins.removed'));
				}
				await loadPins();
				return;
			}
			const content = { type: 'scan-signatures', entries };
			const title = tr('pins.scan_collect_title');
			const rows = collectRows(entries);
			if (collect) {
				// Snapshot-Refresh (M6-F5): Identität bleibt, Inhalt frisch.
				await invoke('pin_update', {
					view_id: 'mining',
					box_index: collect.box_index,
					title,
					kpi: null,
					rows,
					content,
					key: COLLECT_KEY
				});
			} else {
				let ok = false;
				for (let idx = COLLECT_BOX_BASE; idx < COLLECT_BOX_BASE + 5 && !ok; idx++) {
					ok = await invoke<boolean>('pin_add', {
						view_id: 'mining',
						box_index: idx,
						title,
						kpi: null,
						rows,
						content,
						key: COLLECT_KEY
					});
				}
				if (!ok) {
					flash(tr('pins.add_failed'));
					return;
				}
			}
			await loadPins(); // Pinned-Marker sofort nachziehen (Real-App: zusätzlich via PINS_CHANGED_EVENT)
			flash(tr('pins.added'));
		} catch (e) {
			flash(trError(e));
		}
	}

	// ---- Keyboard (Esc-Kaskade wie Inventory: Komponente behandelt selbst) ----
	function onKey(e: KeyboardEvent) {
		if (e.key !== 'Escape') return;
		e.preventDefault();
		e.stopPropagation();
		if (locDetail) {
			closeLocationDetail();
			return;
		}
		if (detail) {
			closeDetail();
			return;
		}
		onexit();
	}

	// M7-F3 (Punkt 6): Kürzel-Funktion ist nach ore-abbrev.ts gezogen
	// (unit-getestet gegen die echte Erzliste) — Klammerzusätze wie „(Ore)“/
	// „(Raw)“ fließen nicht ein (Agricium → „Ag“, nicht „AO“).
	function pct(v: number): number {
		return Math.max(0, Math.min(100, Math.round(v)));
	}
	function concPct(v: number): string {
		return (Math.round(v * 1000) / 10).toFixed(1);
	}
	// M7-F3 (Punkt 5): Rang als PLATZIERUNG („Nr. 1“ DE / „#1“ EN) — die
	// Prozentzahl bleibt nur als Balken-Füllung (rankPct-Datenfeld bleibt).
	const rankPlace = (rank: number) => tr('mining.rank_place', { n: String(rank) });
	const tierLegend = $derived([
		{ tier: 1, band: '< 1 %' },
		{ tier: 2, band: '1–4 %' },
		{ tier: 3, band: '4–8 %' },
		{ tier: 4, band: '8–15 %' },
		{ tier: 5, band: '15–25 %' },
		{ tier: 6, band: '≥ 25 %' }
	]);
</script>

<svelte:window onkeydown={onKey} />

<div class="mining" role="region" aria-label={$t('nav.mining')}>
	{#if !checked}
		<div class="mnote">{$t('mining.loading')}</div>
	{:else if error}
		<div class="merr">{error}</div>
	{:else if status && !status.enabled}
		<!-- Opt-in: Modus ist in der Settings ausgeschaltet -->
		<div class="mcard">
			<div class="mc-h">{$t('mining.disabled_title')}</div>
			<p class="mc-body">{$t('mining.disabled_body')}</p>
		</div>
	{:else if status && !status.key_set}
		<!-- Key-Missing-State: Self-Service-Erklärung (ToS: eigener celd_-Key) -->
		<div class="mcard">
			<div class="mc-h">{$t('mining.key_missing_title')}</div>
			<p class="mc-body">{$t('mining.key_missing_body')}</p>
			<a
				class="mc-link"
				href="https://strata.celd.space/api-keys"
				target="_blank"
				rel="noopener noreferrer">{$t('mining.key_missing_link')}</a
			>
			<div class="mc-foot">{$t('mining.key_missing_settings')}</div>
		</div>
	{:else}
		<div class="mcols">
			<!-- Linke Spalte: Erz-/Ort-Suche -->
			<div class="mcol">
				<div class="mtabs" role="tablist">
					<button
						type="button"
						class="mtab"
						class:is-on={view === 'ores'}
						role="tab"
						aria-selected={view === 'ores'}
						onclick={() => (view = 'ores')}>{$t('mining.tab_ores')}</button
					>
					<button
						type="button"
						class="mtab"
						class:is-on={view === 'locations'}
						role="tab"
						aria-selected={view === 'locations'}
						onclick={() => (view = 'locations')}>{$t('mining.tab_locations')}</button
					>
				</div>

				{#if view === 'ores'}
				<!-- M7-F3 (Punkt 8): kein eigenes Textfeld mehr — das Haupt-Suchfeld -->
				<div class="mfrow">
						<div class="mf">
							<span class="mfl">{$t('mining.filter_category')}</span>
							<div class="mfchips">
								{#each ['all', 'ship', 'fps'] as c (c)}
									<button
										type="button"
										class="mchip"
										class:is-on={catFilter === c}
										onclick={() => (catFilter = c as 'all' | 'ship' | 'fps')}
										>{$t(c === 'all' ? 'mining.cat_all' : c === 'ship' ? 'mining.cat_ship' : 'mining.cat_fps')}</button
									>
								{/each}
							</div>
						</div>
						<div class="mf">
							<span class="mfl">{$t('mining.filter_rarity')}</span>
							<div class="mfchips">
								<button
									type="button"
									class="mchip"
									class:is-on={rarityFilter === 'all'}
									onclick={() => (rarityFilter = 'all')}>{$t('mining.tier_all')}</button
								>
								{#each RARITIES as rv (rv)}
									<button
										type="button"
										class="mchip mtier"
										class:is-on={rarityFilter === rv}
										style={`--tc:${RARITY_COLORS[rv]}`}
										onclick={() => (rarityFilter = rarityFilter === rv ? 'all' : rv)}
										>{rarityName(rv)}</button
									>
								{/each}
							</div>
						</div>
					</div>
					{#if loading}
						<div class="mnote">{$t('mining.loading')}</div>
					{:else}
						<div class="mlist" role="listbox" aria-label={$t('mining.tab_ores')}>
							{#each filteredOres as o (o.id)}
								<div class="mrow" role="option" aria-selected="false" tabindex="0">
									<button
										type="button"
										class="mrow-main"
										onclick={() => void openDetail(o)}
										title={o.name}
									>
										<span class="mrow-name">{o.name}</span>
										<span class="mrow-sub">{o.slug}</span>
										{#if o.tier}
												<span class="mtier-badge" style={`--tc:${rarityColor(o.tier) ?? '#8b949e'}`}
													>{rarityName(o.tier)}</span
												>
											{/if}
										</button>
										<button
											type="button"
											class="mpin"
											class:is-pinned={pinnedOreIds.has(o.id)}
											title={$t('mining.pin_sig_title')}
											aria-label={$t('mining.pin_sig_title')}
											aria-pressed={pinnedOreIds.has(o.id)}
											onclick={() => void toggleSignaturePin(o)}
										>
										<svg
											viewBox="0 0 24 24"
											fill="none"
											stroke="currentColor"
											stroke-width="2"
											stroke-linecap="round"
											stroke-linejoin="round"
										>
											<path d="M12 17v5" />
											<path d="M9 10.8 7 3h10l-2 7.8a5 5 0 0 1-6 0Z" />
										</svg>
									</button>
								</div>
							{:else}
								<div class="mnote">{$t('mining.empty')}</div>
							{/each}
						</div>
					{/if}
				{:else}
				<!-- M7-F3 (Punkt 8): kein eigenes Textfeld mehr — das Haupt-Suchfeld -->
				<div class="mfrow">
						<div class="mf">
							<span class="mfl">{$t('mining.filter_system')}</span>
							<div class="mfchips">
								<button
									type="button"
									class="mchip"
									class:is-on={sysFilter === 'all'}
									onclick={() => (sysFilter = 'all')}>{$t('mining.sys_all')}</button
								>
								{#each systems as sy (sy)}
									<button
										type="button"
										class="mchip"
										class:is-on={sysFilter === sy}
										onclick={() => (sysFilter = sysFilter === sy ? 'all' : sy)}>{sy}</button
									>
								{/each}
							</div>
						</div>
						<div class="mf">
							<span class="mfl">{$t('mining.filter_type')}</span>
							<div class="mfchips">
								<button
									type="button"
									class="mchip"
									class:is-on={typeFilter === 'all'}
									onclick={() => (typeFilter = 'all')}>{$t('mining.type_all')}</button
								>
								{#each locTypes as ty (ty)}
										<button
											type="button"
											class="mchip"
											class:is-on={typeFilter === ty}
											onclick={() => (typeFilter = typeFilter === ty ? 'all' : ty)}
											>{typeName(ty)}</button
										>
									{/each}
							</div>
						</div>
					</div>
					<div class="mlist" role="listbox" aria-label={$t('mining.tab_locations')}>
						{#each filteredLocations as l (l.id)}
							<!-- M7-F2: Ort-Zeile öffnet das Ort-Detail (Maus UND Enter) -->
							<div
								class="lrow"
								role="option"
								aria-selected="false"
								tabindex="0"
								onkeydown={(e) => {
									if (e.key === 'Enter') {
										e.preventDefault();
										void openLocationDetail(l);
									}
								}}
							>
								<button
									type="button"
									class="mrow-main"
									onclick={() => void openLocationDetail(l)}
									title={l.name}
								>
									<span class="mrow-name">{l.name}</span>
									<span class="mrow-sub">{l.system} · {typeName(l.type)}</span>
								</button>
							</div>
						{:else}
							<div class="mnote">{$t('mining.empty')}</div>
						{/each}
					</div>
				{/if}
			</div>

			<!-- Rechte Spalte: Detail (Erz-Detail als Karten-Grid / Ort-Detail M7-F2) -->
			<div class="mdetail">
				{#if detailLoading || locDetailLoading}
					<div class="mnote">{$t('mining.loading')}</div>
				{:else if detailError}
					<div class="merr">{detailError}</div>
				{:else if locDetailError}
					<div class="merr">{locDetailError}</div>
				{:else if locDetail}
					<!-- Ort-Detail: „Erze am Ort", nach Konzentration desc (API) — reine Info, kein Pin (Spec K3) -->
					<div class="md-head">
						<button
							type="button"
							class="mback"
							onclick={closeLocationDetail}
							title={$t('common.back_esc')}
							aria-label={$t('common.back')}>←</button
						>
						<span class="md-title">{locDetail.location.name}</span>
					</div>
					<div class="md-sub">{locDetail.location.system} · {typeName(locDetail.location.type)}</div>
					{#if locDetail.ores.length === 0}
						<div class="mnote">{$t('mining.no_ores')}</div>
					{:else}
						<!-- M7-F4 P3 (t_c3367dfb): Karten-Design wie im Erz-Detail
					     (Kuerzel-Kachel 1.25:1 + Signatur + 6-Segment-Balken) statt
					     Tabelle — und JEDE Karte ist in den Sammel-Signatur-Pin
					     toggelbar (Rarity-Farbe ueber --rc). -->
					<div class="dcards" role="list">
						{#each locDetail.ores as or (or.oreId)}
							<!-- Keyboard konsistent (bestehendes Muster): der Pin-Button
							     ist ein nativer Button (Enter/Space); der Karte selbst keine
							     Non-Interactive-Keyboard-Handler (a11y-Regel svelte-check). -->
							<div
								class="dcard locard"
								role="listitem"
								style={`--rc:${rarityColor(rarityOfOreId(or.oreId)) ?? '#8b949e'}`}
							>
								<div class="locard-top">
									<span
										class="ltile"
										style={`border-color:${rarityColor(rarityOfOreId(or.oreId)) ?? 'var(--lineb)'}`}>{oreAbbrev(or.name)}</span
									>
									<div class="locard-head">
										<span class="dcard-name" title={or.name}>{or.name}</span>
										<span class="mtier-badge" style={`--tc:${tierColor(or.tier)}`}
											>{tierName(or.tier)}</span
										>
									</div>
									<button
										type="button"
										class="mpin"
										class:is-pinned={pinnedOreIds.has(or.oreId)}
										title={$t('mining.pin_sig_title')}
										aria-label={$t('mining.pin_sig_title')}
										aria-pressed={pinnedOreIds.has(or.oreId)}
										onclick={() => void toggleSignaturePin({ id: or.oreId, name: or.name, scanSignature: or.scanSignature, tier: rarityOfOreId(or.oreId) })}
									>
										<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
											<path d="M12 17v5" />
											<path d="M9 10.8 7 3h10l-2 7.8a5 5 0 0 1-6 0Z" />
										</svg>
									</button>
								</div>
								<div class="locard-sig">
									<span class="locard-sig-l">{$t('mining.sig_short')}</span>
									<span class="locard-sig-v">{or.scanSignature ?? '—'}</span>
								</div>
								<div class="dnum">
									<span class="dnum-l">{$t('mining.concentration')}</span>
									<span class="dnum-v">{concPct(or.concentration)} %</span>
									<!-- M7-F4 P5: 6 GLEICHE Segmente, gefuellt = Tier-Stufe
									     (OreRow tier 1-6) — Segment-Fuellmodell, NICHT kontinuierlich. -->
									<span class="segbar" style={`--tc:${tierColor(or.tier)}`} role="img" aria-label={$t('mining.concentration')}>
										{#each [1, 2, 3, 4, 5, 6] as s (s)}<i class:fill={s <= or.tier}></i>{/each}
									</span>
								</div>
							</div>
						{/each}
					</div>
					{/if}
				{:else if detail}
					<div class="md-head">
						<button
							type="button"
							class="mback"
							onclick={closeDetail}
							title={$t('common.back_esc')}
							aria-label={$t('common.back')}>←</button
						>
						<span class="md-title">{detail.ore.name}</span>
						<div class="msort" role="group" aria-label={$t('mining.sort')}>
							<button
								type="button"
								class="mchip"
								class:is-on={sortParam === 'rank'}
								onclick={() => setSort('rank')}>{$t('mining.sort_rank')}</button
							>
							<button
								type="button"
								class="mchip"
								class:is-on={sortParam === 'concentration'}
								onclick={() => setSort('concentration')}>{$t('mining.sort_concentration')}</button
							>
						</div>
					</div>
					<!-- M7-F3 Karten-Redesign (Punkt 1): KEIN Dial mehr — links
					     Kürzel-Block (1.25:1, Kürzel groß + Scan-Signatur), rechts
					     Rang-Balken (kontinuierlich, Anzeige als Platz „Nr. X“) +
					     Konzentrations-Balken (6 feste Stufen, Gitterlinien bei
					     1/4/8/15/25 %). Responsives Grid (1–6 Spalten). -->
					<div class="mdetail-scroll">
					{#each detailGroups as g (g.key)}
						<div class="mgroup">
							<div class="mg-head">
								<span class="mg-title">{$t(g.label)}</span>
								<span class="mg-note">{$t('mining.group_note')}</span>
							</div>
							<div class="dcards">
								{#each g.rows as row (row.locationId)}
									<div class="dcard">
										<div class="dcard-top">
											<span class="dcard-name" title={row.location}>{row.location}</span>
											<span class="mtier-badge" style={`--tc:${tierColor(row.tier)}`}
												>{tierName(row.tier)}</span
											>
										</div>
										<!-- Punkt 4: „N spawn sites“ entfernt — die Orte SIND die Spawn-Orte -->
										<span class="dcard-sub">{row.system ?? '—'}</span>
										<div class="dcard-body">
											<div class="abbr-block" aria-hidden="true">
												<span class="abbr">{oreAbbrev(detail.ore.name)}</span>
												<span class="abbr-sig">{detailOre?.scanSignature ?? '—'}</span>
											</div>
											<div class="dnums">
												<div class="dnum">
													<span class="dnum-l">{$t('mining.rank')}</span>
													<span class="dnum-v">{rankPlace(row.rank)}</span>
													<span class="dbar"
														><i style={`width:${pct(row.rankPct)}%;background:var(--accent)`}></i
													></span>
												</div>
												<div class="dnum">
													<span class="dnum-l">{$t('mining.concentration')}</span>
													<span class="dnum-v">{concPct(row.concentration)} %</span>
<span class="segbar" style={`--tc:${tierColor(row.tier)}`}>
																				{#each [1, 2, 3, 4, 5, 6] as s (s)}<i class:fill={s <= row.tier}></i>{/each}
																			</span>
												</div>
											</div>
										</div>
									</div>
								{/each}
							</div>
						</div>
					{:else}
						<div class="mnote">{$t('mining.no_locations')}</div>
					{/each}
					</div>
					<!-- Legende: 6 absolute Bands (= Tier 1–6), bleibt sichtbar -->
					<div class="mlegend">
						<div class="ml-title">{$t('mining.legend_title')}</div>
						{#each tierLegend as li (li.tier)}
							<div class="ml-item">
								<i style={`background:${tierColor(li.tier)}`}></i>
								<span class="ml-name">{$t(`mining.tier_${li.tier}`)}</span>
								<span class="ml-band">{li.band}</span>
							</div>
						{/each}
					</div>
				{:else}
					<div class="mnote">
						{$t(view === 'ores' ? 'mining.detail_hint' : 'mining.loc_detail_hint')}
					</div>
				{/if}
			</div>
		</div>

		{#if note}
			<div class="mtoast">{note}</div>
		{/if}
	{/if}

	<!-- Attribution-Footer (ToS-Pflicht): überall, wo Strata-Daten sichtbar sind -->
	<div class="mattr">{attribution || ATTRIBUTION_FALLBACK}</div>
</div>

<style>
	.mining {
		display: flex;
		flex-direction: column;
		min-height: 0;
		height: 100%;
		padding: 8px 4px 0 2px;
	}
	.mcols {
		display: flex;
		gap: 12px;
		flex: 1;
		min-height: 0;
	}
	.mcol {
		width: 320px;
		flex: 0 0 auto;
		display: flex;
		flex-direction: column;
		min-height: 0;
		gap: 8px;
	}
	.mdetail {
		flex: 1;
		min-width: 0;
		overflow-y: auto;
		padding-right: 4px;
	}
	/* Scrollbar-Konvention (Overlay) */
	.mdetail::-webkit-scrollbar,
	.mlist::-webkit-scrollbar {
		width: 8px;
	}
	.mdetail::-webkit-scrollbar-thumb,
	.mlist::-webkit-scrollbar-thumb {
		background: color-mix(in srgb, var(--accent) 35%, transparent);
		border-radius: 4px;
	}
	.mtabs {
		display: flex;
		gap: 6px;
	}
	.mtab {
		flex: 1;
		padding: 6px 10px;
		border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
		background: transparent;
		color: var(--fgdim, #9aa3b2);
		text-transform: uppercase;
		font-size: 11px;
		letter-spacing: 0.08em;
		cursor: pointer;
	}
	.mtab.is-on {
		background: color-mix(in srgb, var(--accent) 22%, transparent);
		color: var(--fg, #e8ecf3);
		border-color: var(--accent);
	}
	.mfrow {
		display: flex;
		flex-direction: column;
		gap: 6px;
	}
	.mfl {
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.1em;
		color: var(--fgdim, #9aa3b2);
	}
	.mfchips {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
	}
	.mchip {
		padding: 2px 8px;
		font-size: 11px;
		border: 1px solid color-mix(in srgb, var(--accent) 25%, transparent);
		background: transparent;
		color: var(--fgdim, #9aa3b2);
		cursor: pointer;
	}
	.mchip.is-on {
		background: color-mix(in srgb, var(--accent) 25%, transparent);
		color: var(--fg, #e8ecf3);
		border-color: var(--accent);
	}
	.mchip.mtier.is-on {
		border-color: var(--tc, var(--accent));
		color: var(--tc, var(--accent));
	}
	.mlist {
		overflow-y: auto;
		min-height: 0;
		flex: 1;
		display: flex;
		flex-direction: column;
		gap: 2px;
		padding-right: 4px;
	}
	.mrow,
	.lrow {
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.mrow-main {
		flex: 1;
		display: flex;
		align-items: center;
		gap: 8px;
		background: transparent;
		border: 0;
		color: inherit;
		text-align: left;
		padding: 7px 6px;
		cursor: pointer;
		font: inherit;
		min-width: 0;
	}
	.mrow:hover .mrow-main,
	.mrow-main:hover {
		background: color-mix(in srgb, var(--accent) 14%, transparent);
	}
	.mrow-name {
		font-weight: 600;
		font-size: 13px;
		color: var(--fg, #e8ecf3);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.mrow-sub {
		font-size: 11px;
		color: var(--fgdim, #9aa3b2);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.lrow {
		padding: 7px 6px;
	}
	.lrow:hover {
		background: color-mix(in srgb, var(--accent) 14%, transparent);
	}
	.mtier-badge {
		padding: 1px 7px;
		border: 1px solid var(--tc, var(--accent));
		color: var(--tc, var(--accent));
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		white-space: nowrap;
	}
	.mpin {
		border: 0;
		background: transparent;
		color: var(--fgdim, #9aa3b2);
		cursor: pointer;
		padding: 4px;
		width: 26px;
		height: 26px;
		flex: 0 0 auto;
	}
	.mpin:hover {
		color: var(--accent);
		background: color-mix(in srgb, var(--accent) 18%, transparent);
	}
	.mpin.is-pinned {
		color: var(--accent);
	}
	.mpin.is-pinned svg {
		fill: currentColor;
	}
	.mpin svg {
		width: 15px;
		height: 15px;
	}
	.md-head {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-bottom: 8px;
	}
	.mback {
		border: 0;
		background: transparent;
		color: var(--fgdim, #9aa3b2);
		cursor: pointer;
		font-size: 15px;
		padding: 2px 8px;
	}
	.mback:hover {
		color: var(--accent);
	}
	.md-title {
		font-weight: 700;
		font-size: 15px;
		color: var(--fg, #e8ecf3);
		flex: 1;
	}
	.md-sub {
		font-size: 11px;
		color: var(--fgdim, #9aa3b2);
		margin: -4px 0 8px 30px;
	}
	.msort {
		display: flex;
		gap: 4px;
	}
	.mgroup {
		margin-bottom: 12px;
	}
	.mg-head {
		display: flex;
		align-items: baseline;
		gap: 8px;
		border-bottom: 1px solid color-mix(in srgb, var(--accent) 20%, transparent);
		padding-bottom: 3px;
		margin-bottom: 4px;
	}
	.mg-title {
		font-size: 11px;
		text-transform: uppercase;
		letter-spacing: 0.1em;
		color: var(--accent);
		font-weight: 700;
	}
	.mg-note {
		font-size: 10px;
		color: var(--fgdim, #9aa3b2);
	}
	/* M7-F4 P4 (t_c3367dfb): Grid-DENSITY — minmax so kalibriert, dass bei
	   breiter Shell (70vw @ 3440 → Detailspalte ~2076px) regelmässig 4 Spalten
	   entstehen (4×470+3×8 = 1904 ≤ 2076, 5 Spalten brauchen 2382 > 2076);
	   schmalere Fenster stufen automatisch auf 3/2/1 ab (auto-fill). */
	.mdetail-scroll {
		min-width: 0;
	}
	.dcards {
		display: grid;
		/* F5-Fix (User 2026-11-18): OBERE BREITEN-CAP statt 1fr — eine
		   Einzelkarte (Gruppe mit 1 Eintrag) dehnte sich sonst auf die volle
		   Containerbreite (~47 % Leerraum, Badge einsam in der Ecke). Feste
		   Kartenbreite + justify-content:start = dichte Reihe, letzte
		   unvollständige Reihe bleibt kartenbreit statt aufzudehnen. */
		grid-template-columns: repeat(auto-fill, minmax(360px, 430px));
		justify-content: start;
		gap: 8px;
	}
	.dcard {
		display: flex;
		flex-direction: column;
		gap: 2px;
		padding: 5px 8px;
		border: 1px solid color-mix(in srgb, var(--accent) 22%, transparent);
		background: color-mix(in srgb, var(--panel, #11100d) 60%, transparent);
		border-radius: 3px;
		min-width: 0;
	}
	.dcard-top {
		display: flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
	}
	.dcard-name {
		flex: 1;
		min-width: 0;
		font-weight: 600;
		font-size: 13px;
		color: var(--fg, #e8ecf3);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.dcard-sub {
		font-size: 11px;
		color: var(--fgdim, #9aa3b2);
	}
	/* M7-F3: Körper = links Kürzel-Block (1.25:1 Höhe×Breite — breiter als
	   hoch), rechts die 2 Balken. */
	.dcard-body {
		display: flex;
		align-items: center;
		gap: 10px;
	}
	.abbr-block {
		/* 1.25:1 (Höhe×Breite) — auf Mockup-Proportion getrimmt (User 2026-11-18):
		   Kachel kompakt (56px), keine optische Dominanz — Metriken-Spalte trägt. */
		aspect-ratio: 1 / 1.25;
		flex: 0 0 auto;
		width: 56px;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 2px;
		border: 1px solid color-mix(in srgb, var(--accent) 28%, transparent);
		background: color-mix(in srgb, var(--panel2, #14161b) 70%, transparent);
		border-radius: 3px;
	}
	.abbr {
		font-size: 20px;
		font-weight: 700;
		line-height: 1;
		color: var(--fg, #e8ecf3);
		letter-spacing: 0.02em;
	}
	.abbr-sig {
		font-size: 11px;
		color: var(--fgdim, #9aa3b2);
	}
	/* Zahl+Balken-Zeile: exakte Werte OHNE Hover lesbar (Lesbarkeits-Fallback) */
	.dnums {
		display: flex;
		flex-direction: column;
		gap: 2px;
		margin-top: 0;
	}
	.dnum {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px;
		font-size: 11px;
	}
	.dnum-l {
		color: var(--fgdim, #9aa3b2);
		flex: 0 0 auto;
	}
	.dnum-v {
		color: var(--fg, #e8ecf3);
		font-weight: 600;
		flex: 0 0 auto;
		min-width: 52px;
	}
	.dbar {
		/* Volle Zeilenbreite (eigene Zeile unter Label+Wert) — flex:1 schrumpfte
		   auf 0px, wenn Label+Wert die Zeile füllten (Review-Befund Runde 3). */
		flex-basis: 100%;
		height: 5px;
		background: color-mix(in srgb, var(--fgdim, #9aa3b2) 25%, transparent);
		overflow: hidden;
	}
	.dbar i {
		display: block;
		height: 100%;
	}
	/* M7-F4 P5: 6 GLEICHE Segmente mit sichtbaren Trennlinien — gefüllt =
	   Tier-Stufe (1..6), ungefüllt dezent sichtbar (Grenze + leichte Fläche). */
	.segbar {
		/* F5-Fix: EIGENE volle Zeile wie der Rank-Balken (flex-basis:100%) —
		   flex:1 liess den Balken neben Label+Wert schrumpfen (User: „sollte
		   die selbe Breite wie der Rankbalken beibehalten"). */
		flex-basis: 100%;
		display: flex;
		gap: 2px;
		height: 7px;
	}
	.segbar i {
		flex: 1;
		background: color-mix(in srgb, var(--fgdim, #9aa3b2) 18%, transparent);
		border: 1px solid color-mix(in srgb, var(--fgdim, #9aa3b2) 30%, transparent);
	}
	.segbar i.fill {
		background: var(--tc, var(--accent));
		border-color: var(--tc, var(--accent));
	}
	/* M7-F4 P3: Ort-Detail-Karten — Kürzel-Kachel (1.25:1) mit Rarity-Border */
	.locard-top {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}
	.ltile {
		flex: 0 0 auto;
		width: 40px;
		height: 32px; /* 1.25:1 */
		display: grid;
		place-items: center;
		border: 1px solid var(--lineb); /* Rarity-Farbe via inline border-color */
		background: color-mix(in srgb, var(--rc, var(--lineb)) 10%, transparent);
		font: 700 10.5px/1 var(--font-mono, monospace);
		letter-spacing: 0.06em;
		color: var(--fg, #e8ecf3);
	}
	.locard-head {
		flex: 1;
		min-width: 0;
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.locard-sig {
		display: flex;
		align-items: baseline;
		gap: 6px;
		font-size: 11px;
		color: var(--fgdim, #9aa3b2);
	}
	.locard-sig-v {
		font: 600 12.5px/1.15 var(--font-mono, monospace);
		color: var(--fg, #e8ecf3);
	}
	/* Legende */
	.mlegend {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: 3px 10px;
		padding: 8px;
		border: 1px solid color-mix(in srgb, var(--accent) 18%, transparent);
		margin-top: 4px;
	}
	.ml-title {
		grid-column: 1 / -1;
		font-size: 10px;
		text-transform: uppercase;
		letter-spacing: 0.1em;
		color: var(--fgdim, #9aa3b2);
	}
	.ml-item {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 11px;
	}
	.ml-item i {
		width: 10px;
		height: 10px;
		flex: 0 0 auto;
	}
	.ml-name {
		color: var(--fg, #e8ecf3);
	}
	.ml-band {
		color: var(--fgdim, #9aa3b2);
		margin-left: auto;
	}
	/* Cards/States */
	.mcard {
		border: 1px solid color-mix(in srgb, var(--accent) 30%, transparent);
		padding: 16px;
		max-width: 460px;
		margin: 24px auto;
		background: color-mix(in srgb, var(--panel, #11100d) 80%, transparent);
	}
	.mc-h {
		font-weight: 700;
		color: var(--fg, #e8ecf3);
		margin-bottom: 6px;
	}
	.mc-body {
		color: var(--fgdim, #9aa3b2);
		font-size: 12px;
		line-height: 1.5;
		margin: 0 0 10px 0;
	}
	.mc-link {
		color: var(--accent);
		font-size: 12px;
	}
	.mc-foot {
		margin-top: 10px;
		font-size: 11px;
		color: var(--fgdim, #9aa3b2);
	}
	.mnote {
		color: var(--fgdim, #9aa3b2);
		font-size: 12px;
		padding: 8px 4px;
	}
	.merr {
		color: var(--warn, #ffcb63);
		font-size: 12px;
		padding: 8px 4px;
	}
	.mtoast {
		position: absolute;
		bottom: 52px; /* über dem Attribution-Footer — kein Überlappen (Probe-Beleg 03) */
		left: 50%;
		transform: translateX(-50%);
		background: var(--panel2, #1b1813);
		border: 1px solid color-mix(in srgb, var(--accent) 40%, transparent);
		color: var(--fg, #e8ecf3);
		font-size: 12px;
		padding: 5px 12px;
		z-index: 8;
	}
	.mattr {
		border-top: 1px solid color-mix(in srgb, var(--fgdim, #9aa3b2) 20%, transparent);
		color: var(--fgdim, #9aa3b2);
		font-size: 10px;
		padding: 5px 2px 6px;
		text-align: center;
	}
</style>
