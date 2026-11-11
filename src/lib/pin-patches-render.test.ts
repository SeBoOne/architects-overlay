// M5 (t_d10a0d84): Rendering-Struktur-Test für PinBoxes.svelte (Nachfolger
// von PinPatches.svelte, M4-E t_34efb781 — die Pin-Boxen leben jetzt als DOM
// IM Spotlight; die pins-<idx>-Fenster sind entfernt). Der Test parst die
// KOMPONENTE mit dem svelte-Compiler (AST = gerenderte DOM-Struktur) und
// belegt:
//  1. .pin-popover ist KEIN Nachfahre eines .pin-Elements (Containing-Block
//     wäre sonst die absolute Box, nicht der Viewport — Review R1-Defect),
//  2. der nächste positionierende Vorfahre ist .pin-layer (position:absolute;
//     inset:0 = Viewport-Anker, auf den popoverPos() rechnet),
//  3. das Popover-Style-Binding nutzt popoverPosFor (viewport-absolute
//     Koordinaten) und
//  4. .pin-popover hat pointer-events:auto (die Layer ist pointer-events:none —
//     ohne das Flag wäre das Popover unklickbar),
//  5. M5-Anker-Vertrag: die Boxen selbst bekommen pointer-events NUR über
//     .pin.interactive (Palette offen) — der Pins-only-Zustand (class
//     display-only an der Layer) bleibt reine Anzeige.
// Laufbar mit `node --test src/lib/pin-patches-render.test.ts`.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import { parse } from 'svelte/compiler';

// Eigener rekursiver AST-Walker (svelte/compiler exportiert kein walk mehr,
// estree-walker ist keine direkte Dependency — bewusst ohne neues Paket).
// Liefert die KIND-KNOTEN (Objekte mit type): direkt gesetzte Knoten-Werte
// sowie Elemente von Knoten-Arrays.
function childNodes(node: unknown): Node[] {
	if (!node || typeof node !== 'object' || Array.isArray(node)) return [];
	const kids: Node[] = [];
	for (const value of Object.values(node as Record<string, unknown>)) {
		if (Array.isArray(value)) {
			for (const el of value) {
				if (el && typeof el === 'object' && typeof (el as Record<string, unknown>).type === 'string') {
					kids.push(el as Node);
				}
			}
		} else if (value && typeof value === 'object' && typeof (value as Record<string, unknown>).type === 'string') {
			kids.push(value as Node);
		}
	}
	return kids;
}

function walkNodes(root: Node, visit: (n: Node, path: Node[]) => void, path: Node[] = []): void {
	visit(root, path);
	for (const child of childNodes(root)) walkNodes(child, visit, [...path, root]);
}

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(join(here, 'PinBoxes.svelte'), 'utf8');

type Node = { type: string; attributes?: unknown[]; children?: Node[]; name?: string; start: number; end: number };

function classesOf(node: Node): string[] {
	const attr = (node.attributes ?? []).find(
		// @ts-expect-error AST shape
		(a) => a.type === 'Attribute' && a.name === 'class'
	);
	// class:pin / class:pin-popover Direktiven und class="..."-Attribut abbilden
	const dirs = (node.attributes ?? [])
		// @ts-expect-error AST shape
		.filter((a) => a.type === 'ClassDirective')
		// @ts-expect-error AST shape
		.map((a) => a.name);
	// @ts-expect-error AST shape
	const rawVal = attr ? (typeof attr.value === 'string' ? attr.value : (attr.value?.[0]?.data ?? '')) : '';
	const plain = rawVal.split(/\s+/);
	return [...dirs, ...plain.filter(Boolean)];
}

function findByClass(ast: Node, cls: string): Node[] {
	const found: Node[] = [];
	walkNodes(ast, (node) => {
		if ((node.type === 'Element' || node.type === 'RegularElement') && classesOf(node).includes(cls)) {
			found.push(node);
		}
	});
	return found;
}

// Elternkette im AST: mit explizitem Pfad-Tracking (walk liefert keinen Pfad)
function elementAncestors(node: Node, ast: Node): Node[] {
	let chain: Node[] = [];
	walkNodes(ast, (n, path) => {
		if (n === node) chain = path;
	});
	return chain.filter((n) => n.type === 'Element' || n.type === 'RegularElement');
}

test('Popover: gerenderte DOM-Struktur — Geschwister der .pin-Boxen, direkt unter .pin-layer', () => {
	const ast = parse(src, { modern: true }) as Node;
	const pops = findByClass(ast, 'pin-popover');
	assert.equal(pops.length, 1, 'genau ein .pin-popover im Template');

	const ancestors = elementAncestors(pops[0], ast);
	const ancestorClasses = ancestors.map((a) => classesOf(a).join(' '));
	assert.ok(
		ancestorClasses.some((c) => c.includes('pin-layer')),
		`nächster positionierender Vorfahre ist .pin-layer (Kette: ${ancestorClasses.join(' > ')})`
	);
	assert.ok(
		!ancestorClasses.some((c) => c.includes('pin') && !c.includes('pin-layer')),
		'.pin-popover darf NICHT innerhalb einer .pin-Box gerendert sein (doppelter Offset, Review R1)'
	);
});

test('Popover: Style-Binding nutzt viewport-absolute popoverPosFor-Koordinaten', () => {
	const ast = parse(src, { modern: true }) as Node;
	const pop = findByClass(ast, 'pin-popover')[0];
	// @ts-expect-error AST shape
	const style = (pop.attributes ?? []).find((a) => a.type === 'Attribute' && a.name === 'style');
	assert.ok(style, 'Popover hat ein style-Attribut');
	// style={`...`} → ExpressionTag (einzelnes Objekt) oder Text-Array
	// @ts-expect-error AST shape
	const parts = Array.isArray(style.value) ? style.value : [style.value];
	const raw = parts.map((v: { type: string; data?: string; expression?: { end: number; start: number } }) =>
		v.type === 'Text' ? (v.data ?? '') : src.slice(v.expression?.start ?? 0, v.expression?.end ?? 0)
	).join('');
	assert.ok(raw.includes('popoverPosFor(pin)'), `style nutzt popoverPosFor (gefunden: ${raw})`);
	assert.ok(raw.includes('left:') && raw.includes('top:'), 'left/top gesetzt');
});

test('Popover: pointer-events:auto — klickbar trotz pointer-events:none auf der Layer', () => {
	const cssStart = src.lastIndexOf('<style');
	const css = src.slice(cssStart);
	const rule = css.match(/\.pin-popover\s*\{[^}]*\}/);
	assert.ok(rule, '.pin-popover CSS-Regel vorhanden');
	assert.match(rule[0], /pointer-events:\s*auto/, '.pin-popover braucht pointer-events:auto');
});

// M5 (§6-Revision): der Interaktions-Gate ist an die Box-Klasse gebunden —
// im Pins-only-Zustand (Layer-Klasse display-only, Palette-DOM versteckt)
// gibt es KEINE klickbaren Flächen.
test('M5: Box-Interaktion nur über .pin.interactive — Pins-only bleibt reine Anzeige', () => {
	const ast = parse(src, { modern: true }) as Node;
	const layer = findByClass(ast, 'pin-layer')[0];
	assert.ok(layer, '.pin-layer existiert');
	// @ts-expect-error AST shape
	const layerDirs = (layer.attributes ?? []).filter((a) => a.type === 'ClassDirective').map((a) => a.name);
	assert.ok(layerDirs.includes('display-only'), 'Layer trägt class:display-only (Pins-only-Gate)');
	const pin = findByClass(ast, 'pin')[0];
	assert.ok(pin, '.pin-Box existiert');
	// @ts-expect-error AST shape
	const pinDirs = (pin.attributes ?? []).filter((a) => a.type === 'ClassDirective').map((a) => a.name);
	assert.ok(pinDirs.includes('interactive'), 'Box trägt class:interactive (Interaktions-Gate)');
	const cssStart = src.lastIndexOf('<style');
	const css = src.slice(cssStart);
	const layerRule = css.match(/\.pin-layer\s*\{[^}]*\}/);
	assert.ok(layerRule, '.pin-layer CSS-Regel vorhanden');
	assert.match(layerRule[0], /pointer-events:\s*none/, 'Layer ist pointer-events:none');
	const interactiveRule = css.match(/\.pin\.interactive\s*\{[^}]*\}/);
	assert.ok(interactiveRule, '.pin.interactive CSS-Regel vorhanden');
	assert.match(interactiveRule[0], /pointer-events:\s*auto/, 'nur .pin.interactive fängt Pointer');
});
