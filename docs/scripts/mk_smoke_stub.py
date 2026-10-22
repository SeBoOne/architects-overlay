#!/usr/bin/env python3
"""
mk_smoke_stub.py — (Re)generiert die kontrakt- und datentreuen Location-Daten in
docs/scripts/smoke-stub.js aus den echten UEX-Daten (/tmp/uex_terminals.json
+ /tmp/admin436.json).

Damit spiegelt smoke-stub.js exakt das Produktions-Backend (incl. der
Location-Synthese, Kriterium 2) wider:
  - search('checkmate'): eine synthetische `kind='location'`-Zeile (id = 436,
    Name = Checkmate Station) + alle 18 echten Checkmate-Terminals.
  - get_prices kind='terminal' id=436: die 43 echten Admin-Preise.
  - get_prices kind='location' id=436: Aggregat, je Terminal eine Gruppe
    (nur 436 hat Preise).
  - item -> prices:[], commodity -> unverändert (5-Locations-Fixture).

Die Screenshot-fixturen (Gold-Pool, Marketplace, math_eval) bleiben unberührt.

Usage:  python3 docs/scripts/mk_smoke_stub.py [--stub PATH] [--write]
"""
import json, os, sys, argparse, re

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
TERMS = "/tmp/uex_terminals.json"
ADMIN436 = "/tmp/admin436.json"
STUB = os.path.join(SCRIPT_DIR, "smoke-stub.js")

CHAIN = "Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station"
LOCATION_NAME = "Checkmate Station"
MONEY_ID = 436  # Commodity Shop - Admin - Checkmate (Handels-Terminal, id=436)
MONEY_TERM_NAME = "Commodity Shop - Admin - Checkmate"


def chain_of(r):
    parts = [r.get(k) for k in (
        "star_system_name", "planet_name", "moon_name",
        "orbit_name", "space_station_name", "city_name",
        "outpost_name", "poi_name",
    )]
    d = []
    for x in parts:
        if x and (not d or d[-1] != x):
            d.append(str(x))
    return " > ".join(d)


def js_str(s):
    return "'" + str(s).replace("'", "\\'") + "'"


def load_terms():
    t = json.load(open(TERMS))["data"]
    return sorted([r for r in t if chain_of(r) == CHAIN], key=lambda r: r["id"])


def load_admin():
    return json.load(open(ADMIN436))["data"]


def admin_rows():
    return [r for r in load_admin() if r["id_terminal"] == MONEY_ID]


# ---- fragments ----------------------------------------------------------

def terminal_row(t):
    code = js_str(t.get("code")) if t.get("code") else "null"
    return (
        "\t\t\t\t\t\t\t{ id: %d, name: %s, kind: 'terminal', "
        "subtitle: %s, code: %s }"
        % (t["id"], js_str(t["name"]), js_str(CHAIN), code)
    )


def location_pool_block(terms):
    """Replaces the original 3-entry `location: [ ... ]` block in `const pool`."""
    entries = []
    # location row first (id = money terminal 436), then all 18 real terminals.
    entries.append(
        "\t\t\t\t\t\t{ id: %d, name: %s, kind: 'location', "
        "subtitle: %s, code: null }"
        % (MONEY_ID, js_str(LOCATION_NAME), js_str(CHAIN))
    )
    for t in terms:
        entries.append(terminal_row(t))
    return "\t\t\t\t\tlocation: [\n" + ",\n".join(entries) + "\n\t\t\t\t\t]"


def byterminal_rows():
    """Plain `{commodity,buy,sell,q,code,commodity_id}` rows for byTerminal[436]."""
    out = []
    for r in admin_rows():
        out.append(
            "\t\t\t\t\t\t\t{ commodity: %s, buy: %s, sell: %s, q: %s, code: %s, commodity_id: %s }"
            % (
                js_str(r["commodity_name"]),
                r["price_buy"],
                r["price_sell"],
                r.get("quality", 0),
                js_str(r.get("commodity_code", "")),
                r.get("id_commodity", 0),
            )
        )
    return ",\n".join(out)


def location_price_calls():
    """`price('Checkmate — Admin Terminal', 436, buy, sell, q, {code,commodity_id})`
    calls for the location group of terminal 436 (used inside `price(...)`."""
    out = []
    for r in admin_rows():
        out.append(
            "\t\t\t\t\t\t\t\tprice('Checkmate — Admin Terminal', %d, %s, %s, %s, "
            "{ code: %s, commodity_id: %s })"
            % (
                MONEY_ID,
                r["price_buy"],
                r["price_sell"],
                r.get("quality", 0),
                js_str(r.get("commodity_code", "")),
                r.get("id_commodity", 0),
            )
        )
    return ",\n".join(out)


def location_groups_block(terms):
    """`const groups = [...]` für den `kind='location'`-Case (alle 18 Terminals)."""
    out = ["\t\t\t\t\tconst groups = ["]
    for t in terms:
        out.append("\t\t\t\t\t\t{")
        out.append("\t\t\t\t\t\t\tterminal_id: %d," % t["id"])
        out.append("\t\t\t\t\t\t\tterminal_name: %s," % js_str(t["name"]))
        if t["id"] == MONEY_ID:
            out.append("\t\t\t\t\t\t\tprices: [")
            out.append(location_price_calls().replace("}\n", "}\n\t\t\t\t\t\t\t\t").rstrip())
            # re-indent each call line under prices[]
            lines = [l for l in location_price_calls().split("\n")]
            out = out[:-1] + ["\t\t\t\t\t\t\t\t" + l for l in lines]
            out.append("\t\t\t\t\t\t\t]")
        else:
            out.append("\t\t\t\t\t\t\tprices: []")
        out.append("\t\t\t\t\t\t},")
    if out[-1].endswith(","):
        out[-1] = out[-1][:-1]
    out.append("\t\t\t\t\t];")
    return "\n".join(out)


# ---- original fragments kept verbatim -----------------------------------

ORIGINAL_COMMODITY_PRICES = """\t\t\t\t\t\t{ commodity: name, commodity_id: 1, location: 'ARC-L1 Wide Forest Station', terminal_id: 11, sell_price: 0, buy_price: 4812, sell_stock: 0, buy_stock: 1200, sell_status: 1, buy_status: 1, quality: 3, updated: now - 240 },
\t\t\t\t\t\t{ commodity: name, commodity_id: 1, location: 'Area18 — TDD', terminal_id: 12, sell_price: 0, buy_price: 4930, sell_stock: 0, buy_stock: 900, sell_status: 1, buy_status: 1, quality: 3, updated: now - 300 },
\t\t\t\t\t\t{ commodity: name, commodity_id: 1, location: 'Orison — Providence', terminal_id: 13, sell_price: 6305, buy_price: 0, sell_stock: 2200, buy_stock: 0, sell_status: 1, buy_status: 1, quality: 3, updated: now - 180 },
\t\t\t\t\t\t{ commodity: name, commodity_id: 1, location: 'Levski — Grand Barter', terminal_id: 14, sell_price: 6118, buy_price: 0, sell_stock: 1800, buy_stock: 0, sell_status: 1, buy_status: 1, quality: 3, updated: now - 360 },
\t\t\t\t\t\t{ commodity: name, commodity_id: 1, location: 'Lorville — Tammany & Sons', terminal_id: 15, sell_price: 5980, buy_price: 0, sell_stock: 1500, buy_stock: 0, sell_status: 1, buy_status: 1, quality: 3, updated: now - 540 }"""


def new_get_prices_block(terms):
    return (
        "\t\t\tcase 'get_prices': {\n"
        "\t\t\t\tconst name = String(a.commodity || '');\n"
        "\t\t\t\tconst now = Math.floor(Date.now() / 1000);\n"
        "\t\t\t\tconst price = (loc, tid, buy, sell, q, extra) => Object.assign({\n"
        "\t\t\t\t\tcommodity: name, commodity_id: 1, location: loc, terminal_id: tid,\n"
        "\t\t\t\t\tsell_price: sell, buy_price: buy, sell_stock: 0, buy_stock: 0,\n"
        "\t\t\t\t\tsell_status: sell > 0 ? 1 : 0, buy_status: buy > 0 ? 1 : 0,\n"
        "\t\t\t\t\tquality: q, updated: now - 240\n"
        "\t\t\t\t}, extra || {});\n"
        "\t\t\t\t// QA-Auflage C: Kontrakt-treuer Mirror — Location → `location.groups`\n"
        "\t\t\t\t// (je Terminal eine Gruppe, leere Gruppe = \"keine Preisdaten\"),\n"
        "\t\t\t\t// Terminal → EIGENE Preise, Item → prices:[] (niemals Fehler),\n"
        "\t\t\t\t// Commodity → Marktliste (unverändert).\n"
        "\t\t\t\tif (a.kind === 'terminal') {\n"
        "\t\t\t\t\tconst tid = Number(a.id || %d);\n"
        "\t\t\t\t\tconst byTerminal = {\n"
        "\t\t\t\t\t\t%d: [\n"
        "%s\n"
        "\t\t\t\t\t\t]\n"
        "\t\t\t\t\t};\n"
        "\t\t\t\t\tconst rows = byTerminal[tid] || [];\n"
        "\t\t\t\t\tconst prices = rows.map((r) => ({\n"
        "\t\t\t\t\t\tcommodity: r.commodity, commodity_id: r.commodity_id || 1, location: 'Checkmate — ' + tid,\n"
        "\t\t\t\t\t\tterminal_id: tid, sell_price: r.sell, buy_price: r.buy,\n"
        "\t\t\t\t\t\tsell_stock: 0, buy_stock: 0, sell_status: r.sell > 0 ? 1 : 0,\n"
        "\t\t\t\t\t\tbuy_status: r.buy > 0 ? 1 : 0, quality: r.q, updated: now - 240\n"
        "\t\t\t\t\t}));\n"
        "\t\t\t\t\treturn {\n"
        "\t\t\t\t\t\tcommodity: name, source: 'uex', fetched_at: now,\n"
        "\t\t\t\t\t\tlocation: null, prices\n"
        "\t\t\t\t\t};\n"
        "\t\t\t\t}\n"
        "\t\t\t\tif (a.kind === 'location') {\n"
        "%s\n"
        "\t\t\t\t\tconst prices = [].concat(...groups.map((g) => g.prices));\n"
        "\t\t\t\t\treturn {\n"
        "\t\t\t\t\t\tcommodity: name, source: 'uex', fetched_at: now, prices,\n"
        "\t\t\t\t\t\tlocation: {\n"
        "\t\t\t\t\t\t\tlocation: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station',\n"
        "\t\t\t\t\t\t\tterminal: 'Checkmate Station', groups\n"
        "\t\t\t\t\t\t}\n"
        "\t\t\t\t\t};\n"
        "\t\t\t\t}\n"
        "\t\t\t\tif (a.kind === 'item') {\n"
        "\t\t\t\t\treturn { commodity: name, source: 'uex', fetched_at: now, location: null, prices: [] };\n"
        "\t\t\t\t}\n"
        "\t\t\t\treturn {\n"
        "\t\t\t\t\tcommodity: name,\n"
        "\t\t\t\t\tsource: 'uex',\n"
        "\t\t\t\t\tfetched_at: now,\n"
        "\t\t\t\t\tlocation: null,\n"
        "\t\t\t\t\tprices: [\n"
        "\t\t\t\t\t\t%s\n"
        "\t\t\t\t\t]\n"
        "\t\t\t};\n"
        "\t\t\t}\n"
        "\t\t\tcase 'search'"
        % (
        MONEY_ID,
        MONEY_ID,
        byterminal_rows(),
        location_groups_block(terms),
        ORIGINAL_COMMODITY_PRICES,
    ))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--stub", default=STUB)
    ap.add_argument("--write", action="store_true")
    args = ap.parse_args()

    terms = load_terms()
    rows436 = admin_rows()
    print("# Checkmate Station terminals:", len(terms))
    print("# Admin(436) price rows:", len(rows436))

    loc_block = location_pool_block(terms)
    gp_block = new_get_prices_block(terms)

    if not args.write:
        print("\n=== LOCATION POOL BLOCK ===")
        print(loc_block)
        print("\n=== GET_PRICES BLOCK (first/last 12 lines) ===")
        g = gp_block.split("\n")
        print("\n".join(g[:6]))
        print("...")
        print("\n".join(g[-6:]))
        print("\n[--write not set: fragments only, stub untouched]")
        return 0

    s = open(args.stub, encoding="utf-8").read()

    # A) location pool: exact replacement of the original 3-entry block.
    orig_loc = (
        "\t\t\t\t\tlocation: [\n"
        "\t\t\t\t\t\t{ id: 5010, name: 'Checkmate Station', kind: 'location', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: null },\n"
        "\t\t\t\t\t\t{ id: 5001, name: 'Landing Services - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: null },\n"
        "\t\t\t\t\t\t{ id: 5002, name: 'Guns - Checkmate', kind: 'terminal', subtitle: 'Pyro > Monox > Monox Lagrange Point 4 > Checkmate Station', code: null }\n"
        "\t\t\t\t\t]"
    )
    assert orig_loc in s, "original location pool block not found"
    s = s.replace(orig_loc, loc_block)

    # B) get_prices case: replace only the `case 'get_prices'` block. Its close
    # brace sits directly before `case 'search'`; anchoring there (not on
    # `math_eval`) avoids swallowing the search case, which follows get_prices.
    pat = re.compile(r"\t\t\tcase 'get_prices': \{[\s\S]*?\n\t\t\t}\n\t\t\tcase 'search'", re.S)
    assert pat.search(s), "get_prices case block not found"
    s = pat.sub(gp_block, s)

    open(args.stub, "w", encoding="utf-8").write(s)
    print("# wrote", args.stub)
    return 0


if __name__ == "__main__":
    sys.exit(main())
