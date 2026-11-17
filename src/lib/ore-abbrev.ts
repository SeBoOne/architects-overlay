// M7-F3 (t_cf1224c5, Punkt 6): Erz-Kürzel im chemisch-Symbol-Stil (Strata-
// Konvention) — Agricium → „Ag“, Beryl → „Be“, Quartz → „Qu“.
//
// Regel (User-Wortlaut): Klammer-Zusätze wie „(Ore)“/„(Raw)“ fließen NICHT
// ins Kürzel — vor der Kürzel-Bildung wird der Klammerzusatz abgeschnitten
// und die ersten zwei Buchstaben des Basisnamens genommen (1. groß, 2. klein).
// „(Pure)“-Sonderfall: nur relevant, wenn danach NICHTS bleiben würde (Name
// besteht nur aus dem Klammerzusatz) — aktuell nicht nötig, hier aber
// abgesichert: fällt der Basisname leer aus, wird der volle Name genommen.

/// Kürzel: Klammerzusatz abschneiden → erste zwei Buchstaben des
/// Basisnamens, 1. groß, 2. klein (Ag, Be, Qu, Go).
export function oreAbbrev(name: string): string {
	const base = name.replace(/\s*\(.*$/, '').trim();
	const src = base.length > 0 ? base : name.trim();
	const letters = src.replace(/[^A-Za-z]/g, '');
	if (letters.length === 0) return '??';
	const a = letters[0].toUpperCase();
	const b = letters.length > 1 ? letters[1].toLowerCase() : '';
	return a + b;
}
