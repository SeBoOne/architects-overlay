# Berichtskorrektur — Location-Expansion / Kriterium 2

**Status:** Korrigiert (`feature/location-detail-fix`, Worktree `t_e9831026`).
**Kontext:** Review-Verdikt `t_9608f33c` → ÄNDERUNGEN NÖTIG; Kriterium 2 nicht erfüllt.

## Korrigierte Berichtsaussage

> FALSCH (wörtlich aus früheren Befunden):
> „Terminal **Landing Services** zeigt EIGENE Kauf-Preise.“

> KORRIGIERT (live, aus `uex_client --test live_api` / Cache-Seed `admin436.json`):
> **Landing Services (428) ist leer** — *„Kein Terminal kauft/verkauft“*.
> Es **zeigt keine Preise eigenständig**; das Aggregat über die Kette kommt
> ausschließlich über das monetäre Terminal **436** (`Commodity Shop - Admin - Checkmate`).

## Real unterscheidbare Fälle (nach Terminal)

| Terminal (id) | Name                          | Kauf-Preise | Verkauf-Preise | Bemerkung                       |
|---------------|-------------------------------|-------------|----------------|---------------------------------|
| **436**       | Commodity Shop - Admin - Checkmate | **43**      | 0              | monetäres Terminal (Handel) — Quelle für das Location-Aggregat |
| 428           | Landing Services - Checkmate  | 0           | 0              | leer — „Kein Terminal kauft/verkauft“ |
| 431           | Refinement Center - Checkmate | 0           | 0              | leer                            |
| 435           | Medical Shop - Checkmate      | 0           | 0              | leer                            |
| 437           | Guns - Checkmate              | 0           | 0              | leer                            |
| 438           | Sharp Shooters - Checkmate    | 0           | 0              | leer                            |
| 440–442, 490–494, 596–597, 600, 689 | weitere Checkmate-Terminals | 0 | 0 | leer |

## Was "EigenPreis" bedeutet (Kriterium 1, gültig)

Für **436** zeigt das Terminal-Panel **43 eigene Preise ohne Ketten-Header**
(d.h. nur die Preise, die 436 tatsächlich liste — nicht die Summe der Kette).
Für **428** zeigt das Panel **keine Zeilen** (leer), weil 428 live 0 Preise
hat — das ist NICHT ein Aggregations-Bug, sondern die Fakt (Landing Services
kauft/verkauft nicht am Stubenmarkt). Damit ist Kriterium 1 korrekt erfüllt:
jedes Terminal sendet `kind='terminal'` + seine echte id; 436 und 428 sind
**unterschiedlich** (436 ≠ leer), nicht identisch.

## Live-Beweis (Auszug)

```
LANDING@428=0 GUNS@437=0 ADMIN@436=43
CHECKMATE: location=Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station groups=18 prices@436=43
test result: ok. 6 passed; 0 failed; 0 ignored; ... finished in 88.12s
```

## Zusammenhang zu Kriterium 2 (Dieser Fix)

Da 428 leer ist, war vor dem Fix **kein** Terminal-Klick mehr das Aggregat —
`h.kind === 'location'` im Frontend (`+page.svelte`) war toter Code. Der Fix
synthétisiert pro Kette eine `kind='location'`-Zeile (id = 436) direkt im
`search`-Befehl (`trade.rs::with_location_rows`), sodass der Standort-Klick das
Aggregat der 18 Terminals erreicht — unabhängig davon, dass Einzel-Terminals
wie 428 leer sind.
