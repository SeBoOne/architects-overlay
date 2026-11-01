import json, re, sys

# text -> (key, en, de).  de/en authored for the T2 wave-2 strings.
M = {
 'Trade BEARBEITEN': ('trades.form.edit_caps', 'EDIT TRADE', 'Trade BEARBEITEN'),
 'Trade ERFASSEN': ('trades.form.create_caps', 'RECORD TRADE', 'Trade ERFASSEN'),
 'Ware Type-ahead — Tippen filtert, ↵ übernimmt': ('trades.ph_commodity_typeahead', 'Commodity type-ahead — type to filter, Enter accepts', 'Ware Type-ahead — Tippen filtert, ↵ übernimmt'),
 'Ort/Terminal Type-ahead — Tippen filtert, ↵ übernimmt': ('trades.ph_location_typeahead', 'Location/terminal type-ahead — type to filter, Enter accepts', 'Ort/Terminal Type-ahead — Tippen filtert, ↵ übernimmt'),
 'Speichern …': ('common.saving', 'Saving …', 'Speichern …'),
 'Änderung prüfen': ('trades.review_change', 'Review change', 'Änderung prüfen'),
 'Trade prüfen': ('trades.review_trade', 'Review trade', 'Trade prüfen'),
 'Trade LÖSCHEN?': ('trades.confirm_delete_title', 'DELETE TRADE?', 'Trade LÖSCHEN?'),
 'Trade ÄNDERN?': ('trades.confirm_edit_title', 'EDIT TRADE?', 'Trade ÄNDERN?'),
 'Trade ANLEGEN?': ('trades.confirm_create_title', 'CREATE TRADE?', 'Trade ANLEGEN?'),
 'GEÄNDERT': ('trades.changed_caps', 'CHANGED', 'GEÄNDERT'),
 'Senden …': ('common.sending', 'Sending …', 'Senden …'),
 'Bestätigen': ('common.confirm_btn', 'Confirm', 'Bestätigen'),
 'Inventar filtern — Standort oder Material …': ('search.ph_inventory', 'Filter inventory — location or material …', 'Inventar filtern — Standort oder Material …'),
 'Hangar search — Name, Custom-Name, Manufacturer or Base-Ship …': ('search.ph_hangar', 'Search hangar — name, custom name, manufacturer or base ship …', 'Hangar durchsuchen — Name, Custom-Name, Hersteller oder Basisschiff …'),
 'Handelsrouten filtern — Ware, Start- oder Zielort …': ('search.ph_routes', 'Filter routes — commodity, origin or destination …', 'Handelsrouten filtern — Ware, Start- oder Zielort …'),
 'Marktplatz durchsuchen — Name / Typ / Terminal / Verkäufer …': ('search.ph_market', 'Search marketplace — name / type / terminal / seller …', 'Marktplatz durchsuchen — Name / Typ / Terminal / Verkäufer …'),
 'Commodity, Item oder Terminal suchen …': ('search.ph_default', 'Search commodity, item or terminal …', 'Commodity, Item oder Terminal suchen …'),
 'Modul suchen': ('hangar.search_module_btn', 'Search module', 'Modul suchen'),
 'Basisschiff zuordnen': ('hangar.assign_base_title', 'Assign base ship', 'Basisschiff zuordnen'),
 'Eintrag bearbeiten': ('hangar.edit_entry_title', 'Edit entry', 'Eintrag bearbeiten'),
 'Eintrag anlegen': ('hangar.create_entry_title', 'Create entry', 'Eintrag anlegen'),
 'EINTRAG BEARBEITEN': ('hangar.edit_entry_caps', 'EDIT ENTRY', 'EINTRAG BEARBEITEN'),
 'EINTRAG ANLEGEN': ('hangar.create_entry_caps', 'CREATE ENTRY', 'EINTRAG ANLEGEN'),
 'EINTRAG LÖSCHEN?': ('hangar.confirm_delete_title', 'DELETE ENTRY?', 'EINTRAG LÖSCHEN?'),
 'ÄNDERUNGEN SPEICHERN?': ('hangar.confirm_save_title', 'SAVE CHANGES?', 'ÄNDERUNGEN SPEICHERN?'),
 'EINTRAG ANLEGEN?': ('hangar.confirm_create_title', 'CREATE ENTRY?', 'EINTRAG ANLEGEN?'),
 'Ändern': ('common.change', 'Change', 'Ändern'),
 'Mod-Taste (Strg/Alt/Shift/Win) + normale Taste drücken': ('settings.capture_hint', 'Press a modifier key (Ctrl/Alt/Shift/Win) plus a normal key', 'Mod-Taste (Strg/Alt/Shift/Win) + normale Taste drücken'),
 'wird aus dem Secret-Key ermittelt …': ('settings.username_pending', 'will be detected from the secret key …', 'wird aus dem Secret-Key ermittelt …'),
 'Sync läuft …': ('common.syncing', 'Syncing …', 'Sync läuft …'),
 'Commodity-Preise jetzt aktualisieren': ('settings.refresh_commodity_prices', 'Refresh commodity prices now', 'Commodity-Preise jetzt aktualisieren'),
 'Item-Preise jetzt aktualisieren': ('settings.refresh_item_prices', 'Refresh item prices now', 'Item-Preise jetzt aktualisieren'),
 'Aktualisiere …': ('common.updating', 'Updating …', 'Aktualisiere …'),
 'Modul-Zuweisungen aktualisieren': ('settings.update_module_assignments', 'Update module assignments', 'Modul-Zuweisungen aktualisieren'),
 'Prüfe …': ('settings.checking', 'Checking …', 'Prüfe …'),
 'Jetzt prüfen': ('settings.check_now', 'Check now', 'Jetzt prüfen'),
 'Lade …': ('common.loading', 'Loading …', 'Lade …'),
 'Keine Verbindung': ('settings.no_connection', 'No connection', 'Keine Verbindung'),
 'Trade gelöscht.': ('trades.deleted_note', 'Trade deleted.', 'Trade gelöscht.'),
 '⚠ illegal': ('market.illegal_chip', '⚠ illegal', '⚠ illegal'),
 'Aktueller Kaufort dieser Route': ('routes.current_buy_terminal_title', 'Current buy terminal of this route', 'Aktueller Kaufort dieser Route'),
 'Kaufort im Filter setzen': ('routes.set_buy_terminal_filter', 'Set this buy terminal in the filter', 'Kaufort im Filter setzen'),
 'Aktueller Verkaufsort dieser Route': ('routes.current_sell_terminal_title', 'Current sell terminal of this route', 'Aktueller Verkaufsort dieser Route'),
 'Verkaufsort im Filter setzen': ('routes.set_sell_terminal_filter', 'Set this sell terminal in the filter', 'Verkaufsort im Filter setzen'),
 'Schiff Type-ahead — Tippen filtert, ↵ übernimmt': ('routes.ph_ship_typeahead', 'Ship type-ahead — type to filter, Enter accepts', 'Schiff Type-ahead — Tippen filtert, ↵ übernimmt'),
 'Startort Type-ahead — Tippen filtert, ↵ übernimmt': ('routes.ph_origin_typeahead', 'Origin type-ahead — type to filter, Enter accepts', 'Startort Type-ahead — Tippen filtert, ↵ übernimmt'),
 'Zielort Type-ahead — Tippen filtert, ↵ übernimmt': ('routes.ph_destination_typeahead', 'Destination type-ahead — type to filter, Enter accepts', 'Zielort Type-ahead — Tippen filtert, ↵ übernimmt'),
 'Käufer': ('trades.buyer', 'Buyer', 'Käufer'),
 'Trade abgeschlossen': ('trades.completed', 'Trade completed', 'Trade abgeschlossen'),
 'Trade beendet': ('trades.ended', 'Trade ended', 'Trade beendet'),
 'Keine Verhandlungen/Abschlüsse zu deinen Listings — starte einen Deal auf uexcorp.space.': ('trades.no_deals_note', 'No negotiations/closings on your listings — start a deal on uexcorp.space.', 'Keine Verhandlungen/Abschlüsse zu deinen Listings — starte einen Deal auf uexcorp.space.'),
 'Username wird ermittelt …': ('settings.username_detecting', 'Detecting username …', 'Username wird ermittelt …'),
 'NEU ANLEGEN (Bearbeiten = erst Abmelden, dann Neu)': ('shop.recreate_title', 'CREATE NEW (editing = unlist first, then create new)', 'NEU ANLEGEN (Bearbeiten = erst Abmelden, dann Neu)'),
 'NEUES LISTING ANLEGEN': ('shop.new_listing_caps', 'CREATE NEW LISTING', 'NEUES LISTING ANLEGEN'),
 'Ort Type-ahead — Locations, keine Shops': ('shop.ph_location_typeahead', 'Location type-ahead — locations, no shops', 'Ort Type-ahead — Locations, keine Shops'),
 'Item Type-ahead — Pflicht für Item-Listings': ('shop.ph_item_typeahead', 'Item type-ahead — required for item listings', 'Item Type-ahead — Pflicht für Item-Listings'),
 'Wird gesendet …': ('shop.sending', 'Sending …', 'Wird gesendet …'),
 'Anlegen (2-Schritt)': ('shop.create_2step', 'Create (2-step)', 'Anlegen (2-Schritt)'),
 'Freigabe unbestätigt': ('shop.approval_unconfirmed', 'Approval unconfirmed', 'Freigabe unbestätigt'),
 'wartet auf Freigabe': ('shop.status_pending', 'awaiting approval', 'wartet auf Freigabe'),
 'Kein Username ermittelt — Secret-Key in den Einstellungen (F12) prüfen.': ('shop.no_username_note', 'No username detected — check the secret key in settings (F12).', 'Kein Username ermittelt — Secret-Key in den Einstellungen (F12) prüfen.'),
 'Listing ABMELDEN?': ('shop.confirm_unlist_title', 'UNLIST LISTING?', 'Listing ABMELDEN?'),
 'Listing ANLEGEN?': ('shop.confirm_create_title', 'CREATE LISTING?', 'Listing ANLEGEN?'),
 'JA, ABMELDEN': ('shop.confirm_unlist', 'YES, UNLIST', 'JA, ABMELDEN'),
 'JA, ANLEGEN': ('shop.confirm_create', 'YES, CREATE', 'JA, ANLEGEN'),
 'kein Kauf-Angebot': ('market.no_buy_offer', 'no buy offer', 'kein Kauf-Angebot'),
 'kein Verkauf-Angebot': ('market.no_sell_offer', 'no sell offer', 'kein Verkauf-Angebot'),
 'nur eine Seite vorhanden — kein Rundlauf möglich': ('market.one_side_only', 'only one side present — no round trip possible', 'nur eine Seite vorhanden — kein Rundlauf möglich'),
 'Kein Artikel zum Kauf': ('market.no_item_to_buy', 'No item to buy', 'Kein Artikel zum Kauf'),
 'Kein Terminal kauft diese Ware': ('market.no_terminal_buys', 'No terminal buys this commodity', 'Kein Terminal kauft diese Ware'),
 'Kein Artikel zum Verkauf': ('market.no_item_to_sell', 'No item to sell', 'Kein Artikel zum Verkauf'),
 'Kein Terminal verkauft diese Ware': ('market.no_terminal_sells', 'No terminal sells this commodity', 'Kein Terminal verkauft diese Ware'),
 'POSITION ERFASSEN': ('inventory.create_caps', 'RECORD POSITION', 'POSITION ERFASSEN'),
 'POSITION BEARBEITEN': ('inventory.edit_caps', 'EDIT POSITION', 'POSITION BEARBEITEN'),
 'Config öffnen': ('common.open_config', 'Open config', 'Config öffnen'),
 'Modul-Zuweisungen aktualisiert (module-overrides.json).': ('settings.modules_updated_note', 'Module assignments updated (module-overrides.json).', 'Modul-Zuweisungen aktualisiert (module-overrides.json).'),
 'Aktualisierung läuft bereits.': ('settings.update_already_running', 'An update is already running.', 'Aktualisierung läuft bereits.'),
 'Zuerst eine Mod-Taste (Strg/Alt/Shift/Win) halten, dann die normale Taste.': ('settings.hotkey_needs_mod', 'Hold a modifier key (Ctrl/Alt/Shift/Win) first, then the normal key.', 'Zuerst eine Mod-Taste (Strg/Alt/Shift/Win) halten, dann die normale Taste.'),
 'Mode-Hotkeys auf Standard-Belegung zurückgesetzt.': ('settings.hotkeys_reset', 'Mode hotkeys reset to the default mapping.', 'Mode-Hotkeys auf Standard-Belegung zurückgesetzt.'),
 'Tastenkürzel mit System (Omarchy) synchronisieren': ('settings.sync_shortcuts_system', 'Sync keyboard shortcuts with the system (Omarchy)', 'Tastenkürzel mit System (Omarchy) synchronisieren'),
 'Compositor-Sync aktiv — Kürzel ändern jetzt auch die Hyprland-Binds.': ('settings.compositor_sync_on', 'Compositor sync active — changing shortcuts now also updates the Hyprland binds.', 'Compositor-Sync aktiv — Kürzel ändern jetzt auch die Hyprland-Binds.'),
 'Autocomplete from those already entered in the Hangar': ('hangar.autocomplete_note', 'Autocomplete from those already entered in the Hangar', 'Autovervollständigung aus den im Hangar erfassten Einträgen'),
 'Bitte einen Typ wählen.': ('hangar.err_choose_type', 'Please choose a type.', 'Bitte einen Typ wählen.'),
 'Bitte eine Herkunft wählen.': ('hangar.err_choose_origin', 'Please choose an origin.', 'Bitte eine Herkunft wählen.'),
 'Bitte die Herkunft des Moduls wählen.': ('hangar.err_choose_module_origin', "Please choose the module's origin.", 'Bitte die Herkunft des Moduls wählen.'),
 'API-Key gespeichert (lokal, 0600).': ('settings.apikey_saved', 'API key saved (locally, 0600).', 'API-Key gespeichert (lokal, 0600).'),
 'Secret-Key gespeichert — wird NIE im Klartext angezeigt.': ('settings.secret_saved', 'Secret key saved — it is NEVER shown in plain text.', 'Secret-Key gespeichert — wird NIE im Klartext angezeigt.'),
 'Jede schreibende Aktion wird bestätigt (Default).': ('settings.confirm_mode_every', 'Every write action is confirmed (default).', 'Jede schreibende Aktion wird bestätigt (Default).'),
 'Nur beim ABMELDEN wird bestätigt — Anlegen läuft direkt.': ('settings.confirm_mode_unlist', 'Only UNLISTING is confirmed — creating runs directly.', 'Nur beim ABMELDEN wird bestätigt — Anlegen läuft direkt.'),
 'Username noch nicht ermittelt — Secret-Key in den Einstellungen (F12) prüfen.': ('settings.username_missing', 'Username not detected yet — check the secret key in settings (F12).', 'Username noch nicht ermittelt — Secret-Key in den Einstellungen (F12) prüfen.'),
 'Keine eigenen aktiven Listings. Wartet ein Listing auf Freigabe, liefert die UEX-API es erst NACH der Freigabe (Endpoint listet nur aktive Ads) — es erscheint dann automatisch hier.': ('shop.no_active_listings', 'No active listings of your own. If a listing is awaiting approval, the UEX API only returns it AFTER approval (the endpoint lists active ads only) — it then appears here automatically.', 'Keine eigenen aktiven Listings. Wartet ein Listing auf Freigabe, liefert die UEX-API es erst NACH der Freigabe (Endpoint listet nur aktive Ads) — es erscheint dann automatisch hier.'),
 'Für den Trades-Reiter braucht UEX deinen persönlichen Secret-Key (Einstellungen → „UEX persönlicher Secret-Key“).': ('trades.need_secret_note', 'The Trades tab needs your personal UEX secret key (Settings → “UEX personal secret key”).', 'Für den Trades-Reiter braucht UEX deinen persönlichen Secret-Key (Einstellungen → „UEX persönlicher Secret-Key“).'),
 'Für den Trade Log braucht UEX deinen persönlichen Secret-Key (Einstellungen → „UEX persönlicher Secret-Key“).': ('trades.trade_log_need_secret', 'The Trade Log needs your personal UEX secret key (Settings → “UEX personal secret key”).', 'Für den Trade Log braucht UEX deinen persönlichen Secret-Key (Einstellungen → „UEX persönlicher Secret-Key“).'),
 'Ware wählen (Tippen für Vorschläge).': ('trades.choose_commodity', 'Choose a commodity (type for suggestions).', 'Ware wählen (Tippen für Vorschläge).'),
 'Ort/Terminal wählen (Tippen für Vorschläge).': ('trades.choose_location', 'Choose a location/terminal (type for suggestions).', 'Ort/Terminal wählen (Tippen für Vorschläge).'),
 'Menge (SCU) muss eine positive Ganzzahl sein.': ('trades.err_qty_positive', 'Quantity (SCU) must be a positive integer.', 'Menge (SCU) muss eine positive Ganzzahl sein.'),
 'Preis muss positiv sein.': ('trades.err_price_positive', 'Price must be positive.', 'Preis muss positiv sein.'),
 'Trade abgeschlossen — Verlauf nur lesbar.': ('trades.readonly_note', 'Trade completed — the history is read-only.', 'Trade abgeschlossen — Verlauf nur lesbar.'),
 'Nur JPG/PNG-Bilder sind erlaubt (UEX-Limit).': ('shop.err_image_type', 'Only JPG/PNG images are allowed (UEX limit).', 'Nur JPG/PNG-Bilder sind erlaubt (UEX-Limit).'),
 'Listing Type ist Pflicht.': ('shop.err_listing_type', 'Listing type is required.', 'Listing Type ist Pflicht.'),
 'Titel ist Pflicht.': ('shop.err_title_required', 'Title is required.', 'Titel ist Pflicht.'),
 'Titel max. 140 Zeichen (Doku).': ('shop.err_title_max', 'Title max. 140 characters (docs).', 'Titel max. 140 Zeichen (Doku).'),
 'Beschreibung ist Pflicht.': ('shop.err_description_required', 'Description is required.', 'Beschreibung ist Pflicht.'),
 'Beschreibung max. 65.535 Zeichen (Doku).': ('shop.err_description_max', 'Description max. 65,535 characters (docs).', 'Beschreibung max. 65.535 Zeichen (Doku).'),
 'Bitte eine Kategorie wählen (Pflicht für Item-/Contract-Listings).': ('shop.err_category', 'Please choose a category (required for item/contract listings).', 'Bitte eine Kategorie wählen (Pflicht für Item-/Contract-Listings).'),
 'Bitte ein Item wählen (Pflicht für Item-Listings).': ('shop.err_item', 'Please choose an item (required for item listings).', 'Bitte ein Item wählen (Pflicht für Item-Listings).'),
 'Ort ist Pflicht.': ('shop.err_location', 'Location is required.', 'Ort ist Pflicht.'),
 'Preis muss eine positive Ganzzahl sein.': ('shop.err_price', 'Price must be a positive integer.', 'Preis muss eine positive Ganzzahl sein.'),
 'Menge muss eine positive Ganzzahl sein.': ('shop.err_qty', 'Quantity must be a positive integer.', 'Menge muss eine positive Ganzzahl sein.'),
 'Einheit ist Pflicht.': ('shop.err_unit', 'Unit is required.', 'Einheit ist Pflicht.'),
 'Währung ist Pflicht.': ('shop.err_currency', 'Currency is required.', 'Währung ist Pflicht.'),
 'Quality muss zwischen 0 und 1000 liegen.': ('shop.err_quality', 'Quality must be between 0 and 1000.', 'Quality muss zwischen 0 und 1000 liegen.'),
 'Laufzeit ist Pflicht.': ('shop.err_duration', 'Duration is required.', 'Laufzeit ist Pflicht.'),
 'Maximal 1 Bild (UEX-Limit).': ('shop.err_image_max', 'Max. 1 image (UEX limit).', 'Maximal 1 Bild (UEX-Limit).'),
 'Bitte die markierten Felder korrigieren.': ('shop.err_fix_fields', 'Please correct the highlighted fields.', 'Bitte die markierten Felder korrigieren.'),
 'Mein Shop sync läuft …': ('shop.sync_running', 'My-shop sync running …', 'Mein Shop sync läuft …'),
 'Routen-Sweep läuft …': ('routes.sweep_running', 'Route sweep running …', 'Routen-Sweep läuft …'),
 'Market-Sync läuft …': ('market.sync_running', 'Market sync running …', 'Market-Sync läuft …'),
 'Unbekanntes Problem beim Laden der Preise.': ('market.price_err_unknown', 'Unknown problem loading prices.', 'Unbekanntes Problem beim Laden der Preise.'),
 'Preisdaten unvollständig — der Anbieter lieferte leere Werte. Bitte erneut versuchen.': ('market.price_err_empty', 'Price data incomplete — the provider returned empty values. Please try again.', 'Preisdaten unvollständig — der Anbieter lieferte leere Werte. Bitte erneut versuchen.'),
 'Anfragenlimit erreicht — bitte kurz warten und erneut versuchen.': ('market.price_err_ratelimit', 'Request limit reached — wait a moment and try again.', 'Anfragenlimit erreicht — bitte kurz warten und erneut versuchen.'),
 'Keine Preisdaten für diesen Eintrag gefunden.': ('market.price_err_notfound', 'No price data found for this entry.', 'Keine Preisdaten für diesen Eintrag gefunden.'),
 'Zeitüberschreitung beim Laden — bitte erneut versuchen.': ('market.price_err_timeout', 'Loading timed out — please try again.', 'Zeitüberschreitung beim Laden — bitte erneut versuchen.'),
 'Keine Verbindung zum Preisdienst. Bitte Internet prüfen.': ('market.price_err_network', 'No connection to the price service. Please check your internet.', 'Keine Verbindung zum Preisdienst. Bitte Internet prüfen.'),
 'SYNC LÄUFT': ('market.sync_running_caps', 'SYNC RUNNING', 'SYNC LÄUFT'),
 'Position gelöscht.': ('inventory.deleted_note', 'Position deleted.', 'Position gelöscht.'),
 'Bitte ein Material wählen (Entity-Suche) oder als Freitext übernehmen.': ('inventory.err_material', 'Choose a material (entity search) or enter it as free text.', 'Bitte ein Material wählen (Entity-Suche) oder als Freitext übernehmen.'),
 'Qualität ist Pflicht (0–1000).': ('inventory.err_quality', 'Quality is required (0–1000).', 'Qualität ist Pflicht (0–1000).'),
 'Bitte einen Standort angeben.': ('inventory.err_location', 'Please enter a location.', 'Bitte einen Standort angeben.'),
 'Bitte eine gültige Menge angeben.': ('inventory.err_qty', 'Please enter a valid quantity.', 'Bitte eine gültige Menge angeben.'),
 'Kaufpreis muss eine Zahl sein (aUEC).': ('inventory.err_price', 'Buy price must be a number (aUEC).', 'Kaufpreis muss eine Zahl sein (aUEC).'),
 'Position geändert.': ('inventory.changed_note', 'Position changed.', 'Position geändert.'),
}

FILES = ['src/routes/+page.svelte', 'src/lib/Inventory.svelte', 'src/lib/Rail.svelte', 'src/lib/SelectCombo.svelte', 'src/lib/RoutesTypeahead.svelte']

def regions(src):
    s = src.find('<script')
    e = src.find('</script>')
    return s, e  # script spans [s,e]; markup outside

report = {'script': 0, 'markup': 0, 'unresolved': []}
new_en = {}
new_de = {}
for f in FILES:
    src = open(f).read()
    s0, e0 = regions(src)
    prot = []
    for m in re.finditer(r'<!--[\s\S]*?-->', src):
        if not (m.start() >= s0 and m.end() <= e0):
            prot.append((m.start(), m.end()))
    edits = []
    for text, (key, en, de) in M.items():
        lit = "'" + text + "'"
        lit2 = '"' + text + '"'
        for L, repl in ((lit, None), (lit2, None)):
            start = 0
            while True:
                i = src.find(L, start)
                if i < 0:
                    break
                start = i + 1
                if any(a <= i < b for a, b in prot):
                    continue
                in_script = s0 <= i < e0
                q = L[0]
                if in_script:
                    new = "t('%s')" % key
                else:
                    new = "$t('%s')" % key
                edits.append((i, i + len(L), new, in_script, text, key))
                new_en[key] = en
                new_de[key] = de
                break  # one occurrence per file per text is the rule; others handled by iteration below
    # apply edits in reverse
    edits.sort(key=lambda x: x[0], reverse=True)
    for i, j, new, ins, text, key in edits:
        src = src[:i] + new + src[j:]
        if ins:
            report['script'] += 1
        else:
            report['markup'] += 1
    open(f, 'w').write(src)

# leftover check
left = []
for f in FILES:
    src = open(f).read()
    s0, e0 = regions(src)
    for text, (key, en, de) in M.items():
        lit = "'" + text + "'"
        lit2 = '"' + text + '"'
        for i in range(len(src)):
            i = src.find(lit, i)
            if i < 0:
                break
            if not (s0 <= i < e0):
                left.append((f, text))
            i += 1
        i = 0
        for i in range(len(src)):
            i = src.find(lit2, i)
            if i < 0:
                break
            if not (s0 <= i < e0):
                left.append((f, text))
            i += 1
report['leftover'] = left
json.dump({'new_en': new_en, 'new_de': new_de}, open('_prep/wave2-strings.json', 'w'), ensure_ascii=False, indent=1)
print('script', report['script'], 'markup', report['markup'], 'leftover', len(left))
for l in left[:20]:
    print('LEFT', l)
