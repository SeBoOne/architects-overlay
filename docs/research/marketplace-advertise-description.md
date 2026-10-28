# Faktencheck: description mehrzeilig (POST /marketplace_advertise)

Task: MQ (t_d6ff5212) — Punkt 2, Description-Textarea.

## Beleg (UEX API 2.0 Doku, live abgerufen 2026-10-28)

Quelle: https://uexcorp.space/api/documentation/id/post_marketplace_advertise/

> **description** _string(65535)_

- Der Typ ist `string(65535)` — es gibt KEINE Einschränkung der Zeichen
  (kein "alphanumeric only" wie bei `title` string(140)).
- Newlines sind nirgends ausgeschlossen; die Response-Fehlerliste
  (`invalid_*`/`missing_*`) enthält keinen Newline-/Encoding-Fehler für
  description (nur `missing_description`).
- Der Overlay-Backend-Pfad reicht die Description als Rust-`String` 1:1
  durch (providers.rs → uex.rs, JSON-Body via serde) — `\n` wird vom
  JSON-Encoder sauber escaped und von UEX als Zeilenumbruch ankommen.

## UI-Folge

Description im Anlegen-Popup ist ein mehrzeiliges `textarea` (autogrow,
max. 220 px, dann Scroll). Kein clientseitiges Escaping — Newlines werden
durchgereicht.
