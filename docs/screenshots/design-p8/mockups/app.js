/* ===========================================================================
   P8 — gemeinsames Mockup-Skript: Daten-Ableitung, Rendering je Variante,
   Tastatur-Navigation (↑↓ / ↵) und die Dev-Leiste (Palette / Datensatz / Höhe).
   Kein Produktionscode — reine Design-Demonstration mit echten Cache-Daten.
   =========================================================================== */
(() => {
	const SETS = window.P8DATA.sets;
	const VARIANT = document.body.dataset.variant;

	const state = { set: 'laranite', pane: 'real', sel: 0, chosen: null };

	/* ---------- Formatierung (wie fmt() der App: de-DE-Gruppierung) ------- */
	const fmt = (n) => (n > 0 ? Math.round(n).toLocaleString('de-DE') : '—');

	/* Bestands-/Nachfrage-Unterzeile: 0 heißt bei UEX "keine Meldung",
	   nicht "0 SCU" — deshalb ein Satz statt eines Striches. */
	const stockTxt = (r, kind) =>
		kind === 'buy'
			? r.bs > 0
				? `Bestand ${fmt(r.bs)} SCU`
				: 'kein Bestand gemeldet'
			: r.ss > 0
				? `Nachfrage ${fmt(r.ss)} SCU`
				: 'keine Nachfrage gemeldet';

	/* ---------- Ableitung: Kauf aufsteigend (günstigster zuerst) ----------
	   Bestehende App sortiert BEIDE Richtungen absteigend (dRowsFor) und der
	   Kopf-Chip nimmt max(buy) — hier bewusst: Kauf = günstigster zuerst,
	   Kopf-Chip = niedrigster Kaufpreis / höchster Verkaufspreis.           */
	function derive(key) {
		const s = SETS[key];
		const buys = s.rows.filter((r) => r.b > 0).sort((a, b) => a.b - b.b);
		const sells = s.rows.filter((r) => r.s > 0).sort((a, b) => b.s - a.s);
		return {
			meta: s,
			buys,
			sells,
			bestBuy: buys.length ? buys[0] : null,
			bestSell: sells.length ? sells[0] : null,
			// Terminal-Matrix: jedes Terminal genau eine Zeile, Kauf/Verkauf als
			// Spalten. Reihenfolge = "beste Gelegenheit oben": erst Terminals mit
			// Kaufpreis aufsteigend, dann reine Verkaufsterminals absteigend.
			matrix: s.rows
				.map((r) => ({ t: r.t, buy: r.b > 0 ? r.b : 0, sell: r.s > 0 ? r.s : 0, bs: r.bs, ss: r.ss }))
				.sort((a, b) => {
					const ab = a.buy > 0 ? 0 : 1;
					const bb = b.buy > 0 ? 0 : 1;
					if (ab !== bb) return ab - bb;
					if (ab === 0) return a.buy - b.buy || b.sell - a.sell || a.t.localeCompare(b.t);
					return b.sell - a.sell || a.t.localeCompare(b.t);
				})
		};
	}

	/* ---------------------------- Kopfzeile ------------------------------- */
	function head(d) {
		const title = d.meta.title;
		const sub = d.meta.sub || '';
		return `
		<div class="d-head">
			<button class="d-back" aria-label="zurück zur Liste">←</button>
			<div class="d-title">
				<h2>${title}</h2>
				<span class="c">${sub}</span>
			</div>
			<div class="d-price">
				<span class="buy"><i>Best Kauf</i>${d.bestBuy ? fmt(d.bestBuy.b) : '—'}</span>
				<span class="sell"><i>Best Verkauf</i>${d.bestSell ? fmt(d.bestSell.s) : '—'}</span>
			</div>
		</div>`;
	}

	function filterHint(d) {
		if (!d.meta.filter) return '';
		const n = d.buys.length + d.sells.length;
		return `<div class="lock-hint"><span class="dot"></span>Filter aktiv: <b>${d.meta.filter}</b> · ${n} Zeilen · Feld leeren = alles zeigen</div>`;
	}

	function foot(rows) {
		if (!rows) {
			return `<div class="d-foot"><span><kbd>Esc</kbd> zurück</span></div>`;
		}
		return `<div class="d-foot">
			<span><kbd>↑↓</kbd> Zeile (<b id="pos">${state.sel + 1}</b>/${rows})</span>
			<span class="sep">·</span>
			<span><kbd>↵</kbd> Auswahl</span>
			<span class="sep">·</span>
			<span><kbd>Esc</kbd> zurück</span>
			<span class="sep">·</span>
			<span>Suchfeld = Filter</span>
		</div>`;
	}

	/* ============================ Variante A ===============================
	   Arkanis-artige Zeilenbox: jede Preiszeile eine eigene Box. Kauf und
	   Verkauf als gruppierte Abschnitte; leere Sektionen werden komplett
	   weggeblendet und durch EINE Statuszeile ersetzt.                      */
	function renderA(d) {
		const box = (r, kind, i, isBest) => `
			<div class="vA-row nav-row${isBest ? ' is-best' : ''}" data-i="${i}">
				<span class="vA-ic">${kind === 'buy' ? 'K' : 'V'}</span>
				<span class="vA-name">${r.t}<small>${stockTxt(r, kind)}</small></span>
				${isBest ? '<span class="vA-tag">Best</span>' : ''}
				<span class="vA-price ${kind === 'buy' ? 'buy' : 'sell'}">${fmt(kind === 'buy' ? r.b : r.s)}<small>aUEC/SCU</small></span>
			</div>`;

		let i = 0;
		let nav = 0;
		const kauf = d.buys.length
			? `<div class="vA-band"><span>Kauf</span><em>${d.buys.length} Terminal · günstigster zuerst</em></div>
			   <div class="vA-rows">${d.buys.map((r) => box(r, 'buy', i++, r === d.buys[0])).join('')}</div>`
			: `<div class="vA-empty"><b class="buy">Kauf</b><span>0 Terminal · kein Terminal verkauft diese Ware</span></div>`;
		nav += d.buys.length;

		const verkauf = d.sells.length
			? `<div class="vA-band sell"><span>Verkauf</span><em>${d.sells.length} Terminal · höchster zuerst</em></div>
			   <div class="vA-rows">${d.sells.map((r) => box(r, 'sell', i++, r === d.sells[0])).join('')}</div>`
			: `<div class="vA-empty"><b class="sell">Verkauf</b><span>0 Terminal · kein Terminal kauft diese Ware</span></div>`;

		return {
			body: `<div class="d-body vA">${kauf}${verkauf}</div>`,
			rows: nav
		};
	}

	/* ============================ Variante B ===============================
	   Kompakte Preis-Matrix: EINE Tabelle, Spalten Terminal | Kauf | Verkauf.
	   Leere Zellen grau/durchgestrichen statt leerer Sektionen. Nutzt das
	   bestehende Zeilenraster (minimal-invasiv).                            */
	function renderB(d) {
		const bestBuy = d.bestBuy ? d.bestBuy.b : 0;
		const bestSell = d.bestSell ? d.bestSell.s : 0;
		let nav = 0;
		const rows = d.matrix
			.map((r) => {
				const isBB = r.buy > 0 && r.buy === bestBuy;
				const isBS = r.sell > 0 && r.sell === bestSell;
				nav++;
				return `
				<div class="vB-r nav-row" data-i="${nav - 1}">
					<span class="vB-t">${r.t}<small>${r.buy > 0 ? stockTxt({ bs: r.bs }, 'buy') : r.sell > 0 ? stockTxt({ ss: r.ss }, 'sell') : 'keine Preisdaten'}</small></span>
					<span class="vB-c buy${isBB ? ' is-best' : ''}${r.buy ? '' : ' nil'}">${r.buy ? fmt(r.buy) : '–'}</span>
					<span class="vB-c sell${isBS ? ' is-best' : ''}${r.sell ? '' : ' nil'}">${r.sell ? fmt(r.sell) : '–'}</span>
				</div>`;
			})
			.join('');

		/* EINE Statuszeile unter der Matrix: Zählung je Richtung + Klartext für
		   eine leere Richtung. Kein Spaltenkopf, keine zweite Hinweiszeile. */
		const legend = [];
		legend.push(`<span><b class="buy">${d.buys.length}</b> Kauf-Terminal</span>`);
		legend.push('<span class="sep">·</span>');
		legend.push(`<span><b class="sell">${d.sells.length}</b> Verkauf-Terminal</span>`);
		if (!d.buys.length || !d.sells.length) {
			legend.push('<span class="sep">·</span>');
			legend.push(
				`<span class="vB-warn">${
					!d.buys.length ? 'kein Terminal verkauft diese Ware' : 'kein Terminal kauft diese Ware'
				}</span>`
			);
		}
		legend.push('<span class="sep">·</span>');
		legend.push('<span>„–" = wird hier nicht gehandelt</span>');

		return {
			body: `
			<div class="d-body vB">
				<div class="vB-tab">
					<div class="vB-head"><span>Terminal</span><span>Kauf</span><span>Verkauf</span></div>
					${rows}
				</div>
				<div class="vB-legend">${legend.join('')}</div>
			</div>`,
			rows: nav
		};
	}

	/* ============================ Variante C ===============================
	   Eigenständiger Vorschlag: Bestpreis-Hero oben (günstigster Kauf,
	   höchster Verkauf, Marge/SCU) + kompakte Terminal-Liste darunter.      */
	function renderC(d) {
		const bb = d.bestBuy;
		const bs = d.bestSell;
		const margin = bb && bs ? bs.s - bb.b : 0;
		const kpi = (cls, k, v, u, who) => `
			<div class="vC-kpi ${cls}">
				<span class="k">${k}</span>
				<span class="v">${v}</span>
				${u ? `<span class="u">${u}</span>` : ''}
				<span class="w">${who}</span>
			</div>`;

		let nav = 0;
		const rows = d.matrix
			.map((r) => {
				nav++;
				return `
				<div class="vC-r nav-row" data-i="${nav - 1}">
					<span class="vC-t">${r.t}<small>${r.buy > 0 ? stockTxt({ bs: r.bs }, 'buy') : r.sell > 0 ? stockTxt({ ss: r.ss }, 'sell') : 'keine Preisdaten'}</small></span>
					<span class="vC-c buy${r.buy ? '' : ' nil'}">${r.buy ? fmt(r.buy) : '–'}</span>
					<span class="vC-c sell${r.sell ? '' : ' nil'}">${r.sell ? fmt(r.sell) : '–'}</span>
				</div>`;
			})
			.join('');

		return {
			body: `
			<div class="d-body vC">
				<div class="vC-hero">
					${kpi('buy', 'Günstigster Kauf', bb ? fmt(bb.b) : '—', 'aUEC / SCU', bb ? `${bb.t} · ${stockTxt(bb, 'buy')}` : 'kein Kauf-Terminal')}
					${kpi('sell', 'Höchster Verkauf', bs ? fmt(bs.s) : '—', 'aUEC / SCU', bs ? `${bs.t} · ${stockTxt(bs, 'sell')}` : 'kein Verkauf-Terminal')}
					${kpi(
						'margin',
						'Marge / SCU',
						bb && bs ? fmt(margin) : '—',
						bb && bs ? `aUEC / SCU · ${fmt(margin * 100)} je 100 SCU` : '',
						bb && bs ? `${bb.t} → ${bs.t}` : 'nur eine Seite vorhanden — kein Rundlauf möglich'
					)}
				</div>
				<div class="vC-listhead"><span>Terminal</span><span>Kauf</span><span>Verkauf</span></div>
				<div class="vC-list">${rows}</div>
			</div>`,
			rows: nav
		};
	}

	const RENDER = { A: renderA, B: renderB, C: renderC };

	/* ---------------------------- Navigation ------------------------------ */
	function markSel() {
		document.querySelectorAll('.nav-row').forEach((el) => {
			const i = Number(el.dataset.i);
			el.classList.toggle('is-sel', i === state.sel);
			el.classList.toggle('is-chosen', i === state.chosen);
		});
		const pos = document.getElementById('pos');
		if (pos) pos.textContent = String(state.sel + 1);
		const cur = document.querySelector('.nav-row.is-sel');
		if (cur) cur.scrollIntoView({ block: 'nearest' });
	}

	/* ---------------------------- Aufbau ---------------------------------- */
	function paint() {
		const d = derive(state.set);
		const r = RENDER[VARIANT](d);
		if (state.sel > r.rows - 1) state.sel = 0;
		document.getElementById('dhead').innerHTML = head(d);
		document.getElementById('dhint').innerHTML = filterHint(d);
		document.getElementById('dbody').innerHTML = r.body;
		document.getElementById('dfoot').innerHTML = foot(r.rows);
		document.getElementById('q').value = state.set === 'levski' ? 'levski' : '';
		document.querySelectorAll('.nav-row').forEach((el) => {
			el.addEventListener('mouseenter', () => {
				state.sel = Number(el.dataset.i);
				markSel();
			});
		});
		document.querySelectorAll('.devbar button[data-set]').forEach((b) => {
			b.classList.toggle('on', b.dataset.set === state.set);
		});
		markSel();
	}

	function wire() {
		document.querySelectorAll('.devbar button[data-set]').forEach((b) =>
			b.addEventListener('click', () => {
				state.set = b.dataset.set;
				state.sel = 0;
				state.chosen = null;
				paint();
			})
		);
		document.querySelectorAll('.devbar button[data-pane]').forEach((b) =>
			b.addEventListener('click', () => {
				document.body.dataset.pane = b.dataset.pane;
				document.querySelectorAll('.devbar button[data-pane]').forEach((x) =>
					x.classList.toggle('on', x.dataset.pane === b.dataset.pane)
				);
			})
		);
		document.querySelectorAll('.devbar button[data-pal]').forEach((b) =>
			b.addEventListener('click', () => {
				document.documentElement.dataset.palette = b.dataset.pal;
				document.querySelectorAll('.devbar button[data-pal]').forEach((x) =>
					x.classList.toggle('on', x.dataset.pal === b.dataset.pal)
				);
			})
		);
		window.addEventListener('keydown', (e) => {
			const n = document.querySelectorAll('.nav-row').length;
			if (e.key === 'ArrowDown') {
				state.sel = Math.min(state.sel + 1, n - 1);
				markSel();
				e.preventDefault();
			} else if (e.key === 'ArrowUp') {
				state.sel = Math.max(state.sel - 1, 0);
				markSel();
				e.preventDefault();
			} else if (e.key === 'Enter') {
				state.chosen = state.sel;
				markSel();
				e.preventDefault();
			}
		});
	}

	window.addEventListener('DOMContentLoaded', () => {
		wire();
		paint();
		window.P8 = { state, paint, markSel };
	});
})();
