# Pending Listings in „Mein Shop" — Root-Cause & Live-Belege (QW-3, t_28bfd027)

**Befund: UEX API 2.0 liefert NUR freigegebene Listings. Ein Listing auf
Freigabe (pending) ist über keinen Endpunkt abrufbar — kein Filter-Bug im
Overlay.**

## Live-Messung (2026-10-29, api.uexcorp.uk/2.0, App-Key + Secret-Key von
Sebos Account AAWA_SeBoOne / id_user 1713, Werte nicht protokolliert)

| Request | Ergebnis |
|---|---|
| `GET /marketplace_listings?username=AAWA_SeBoOne` (+secret-key) | **0 Rows** |
| `GET /marketplace_listings?user_username=…` / `id_user=…` | Parameter ignoriert → globaler Feed (500/1000 Rows) |
| Globaler Feed, 10 × 100 Rows gescannt auf `user_username` = *sebo* | **0 Treffer** |
| `GET /marketplace_listings?username=annabae` (Kontrolle) | 10 Rows — Filter funktioniert korrekt |
| `GET /user/marketplace_listings` (user-scoped via Secret-Key) | 200 OK, **0 Rows** |
| `GET /user_notifications` | nur Verhandlungs-Events, keine Listing-Approval-Events |

## Doku-Beleg
`https://uexcorp.space/api/documentation/id/get_marketplace_listings/`:
> „GET /marketplace_listings — **List all active** marketplace advertisements,
> limited by 100"

Output-Feld `date_approved` = „date when the listing was approved by UEX
staff". Es gibt in API 2.0 KEINEN Endpunkt für eigene (auch wartende) Listings:
`/marketplace_listings_own`, `/user_marketplace_listings` → 404.

## Konsequenz (implementiert)
- Der Listing-Pull (`get_my_offers` → `/marketplace_listings?username=`) ist
  korrekt; der `approved`-Chip (date_approved = 0) bleibt als Guard, falls UEX
  künftig Rows vor Freigabe ausliefert.
- „Mein Shop" zeigt bei leerem Pull einen erklärenden Hinweis (wartende
  Listings erscheinen erst nach Freigabe) + den bestehenden
  Create-Deep-Link (`shopCreatedUrl`), der das wartende Listing direkt zeigt.

## Punkt-6-Nebenbefund (Tracking)
`GET /user_trades` antwortet per DEFAULT mit **5 Rows** (live belegt; mit
`limit=500` kommen alle verfügbaren). Fix: QW-6, limit=500 + Pagination.
