// M4-E (t_34efb781): Rendering-Struktur-Test für den Mini-Controller-Popover
// in PinPatches.svelte (Review R1-Defect: Popover war Kind von .patch →
// doppelter Offset, bei Default-Pins komplett off-screen). Der Test parst die
// KOMPONENTE mit dem svelte-Compiler (AST = gerenderte DOM-Struktur) und
// belegt:
//  1. .pin-popover ist KEIN Nachfahre eines .patch-Elements (Containing-Block
//     wäre sonst das absolute .patch, nicht der Viewport),
//  2. der nächste positionierende Vorfahre ist .patch-layer (position:absolute;
//     inset:0 = Viewport-Anker, auf den popoverPos() rechnet),
//  3. das Popover-Style-Binding nutzt popoverPosFor (viewport-absolute
//     Koordinaten) und
//  4. .pin-popover hat pointer-events:auto (die Layer ist pointer-events:none —
//     ohne das Flag wäre das Popover unklickbar).
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
const src = readFileSync(join(here, 'PinPatches.svelte'), 'utf8');

type Node = { type: string; attributes?: unknown[]; children?: Node[]; name?: string; start: number; end: number };

function classesOf(node: Node): string[] {
	const attr = (node.attributes ?? []).find(
		// @ts-expect-error AST shape
		(a) => a.type === 'Attribute' && a.name === 'class'
	);
	// class:patch / class:pin-popover Direktiven und class="..."-Attribut abbilden
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

function findPopovers(ast: Node): Node[] {
	const found: Node[] = [];
	walkNodes(ast, (node) => {
		if ((node.type === 'Element' || node.type === 'RegularElement') && node.name === 'div' && classesOf(node).includes('pin-popover')) {
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

test('Popover: gerenderte DOM-Struktur — Geschwister von .patch, direkt unter .patch-layer', () => {
	const ast = parse(src, { modern: true }) as Node;
	const pops = findPopovers(ast);
	assert.equal(pops.length, 1, 'genau ein .pin-popover im Template');

	const ancestors = elementAncestors(pops[0], ast);
	const ancestorClasses = ancestors.map((a) => classesOf(a).join(' '));
	assert.ok(
		ancestorClasses.some((c) => c.includes('patch-layer')),
		`nächster positionierender Vorfahre ist .patch-layer (Kette: ${ancestorClasses.join(' > ')})`
	);
	assert.ok(
		!ancestorClasses.some((c) => c.includes('patch') && !c.includes('patch-layer')),
		'.pin-popover darf NICHT innerhalb eines .patch gerendert sein (doppelter Offset, Review R1)'
	);
});

test('Popover: Style-Binding nutzt viewport-absolute popoverPosFor-Koordinaten', () => {
	const ast = parse(src, { modern: true }) as Node;
	const pop = findPopovers(ast)[0];
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
