import json

E = [
 # import json! macro in trade.rs
 ('src-tauri/src/trade.rs', '/// Map a seam error to a German, user-facing message (design contract).',
  '/// Map a seam error to an error_key+params JSON message (T2 t_c5904bcb:\n/// the frontend translates; the fallback chain resolves en, then raw key).', 1),
 # the three friendly tests -> key-based assertions
 ('src-tauri/src/trade.rs',
  """    #[test]
    fn friendly_maps_delete_doku_codes_to_german() {
        let err = |body: &str| ProviderError::Http {
            status: 400,
            body: body.to_string(),
        };
        assert_eq!(
            friendly(err(
                r#"{"status":"trade_not_found","http_code":400,"data":null}"#
            )),
            "Der Trade existiert nicht (trade_not_found) — vermutlich bereits gelöscht oder fremde ID."
        );
        assert_eq!(
            friendly(err(
                r#"{"status":"missing_secret_key","http_code":400,"data":null}"#
            )),
            "Secret-Key fehlt — siehe Einstellungen („UEX persönlicher Secret-Key“)."
        );
        assert_eq!(
            friendly(err(
                r#"{"status":"missing_id","http_code":400,"data":null}"#
            )),
            "Trade-ID fehlt (missing_id)."
        );
        // Kein Fehl-Mapping: missing_id_category (Trades-Add) bleibt generisch.
        assert_eq!(
            friendly(err(
                r#"{"status":"missing_id_category","http_code":400,"data":[]}"#
            )),
            "HTTP 400 · UEX nicht erreichbar"
        );
    }""",
  """    #[test]
    fn friendly_maps_delete_doku_codes_to_keys() {
        // T2 (t_c5904bcb): Doku-Codes kommen als error_key + params an; der
        // Code bleibt im Schlüsseltext sichtbar (Key-Text nennt ihn).
        let err = |body: &str| ProviderError::Http {
            status: 400,
            body: body.to_string(),
        };
        let key_of = |s: String| {
            let v: serde_json::Value = serde_json::from_str(&s).unwrap();
            v["error_key"].as_str().unwrap().to_string()
        };
        assert_eq!(
            key_of(friendly(err(
                r#"{"status":"trade_not_found","http_code":400,"data":null}"#
            ))),
            "error.uex.trade_not_found"
        );
        assert_eq!(
            key_of(friendly(err(
                r#"{"status":"missing_secret_key","http_code":400,"data":null}"#
            ))),
            "error.uex.secret_missing"
        );
        assert_eq!(
            key_of(friendly(err(
                r#"{"status":"missing_id","http_code":400,"data":null}"#
            ))),
            "error.uex.missing_id"
        );
        // Kein Fehl-Mapping: missing_id_category (Trades-Add) bleibt generisch.
        assert_eq!(
            key_of(friendly(err(
                r#"{"status":"missing_id_category","http_code":400,"data":[]}"#
            ))),
            "error.uex.http"
        );
    }""", 1),
 ('src-tauri/src/trade.rs',
  """    #[test]
    fn friendly_rate_limit_is_german_and_actionable() {
        assert_eq!(
            friendly(ProviderError::RateLimit {
                retry_after: Some(34)
            }),
            "HTTP 429 · Rate-Limit (Retry in 34 s)"
        );
        assert!(friendly(ProviderError::Timeout).contains("UEX"));
        assert!(
            friendly(ProviderError::Http {
                status: 500,
                body: "x".into()
            })
            .contains("500")
        );
    }""",
  """    #[test]
    fn friendly_rate_limit_is_a_key_with_params() {
        // T2 (t_c5904bcb): Rate-Limit kommt als Key + {secs}-Param an.
        let s = friendly(ProviderError::RateLimit {
            retry_after: Some(34)
        });
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["error_key"], "error.uex.rate_limit_retry");
        assert_eq!(v["params"]["secs"], 34);
        assert!(
            friendly(ProviderError::Timeout)
                .contains("error.uex.timeout"),
            "timeout maps to a key"
        );
        assert!(
            friendly(ProviderError::Http {
                status: 500,
                body: "x".into()
            })
            .contains("500"),
            "http status rides as a param"
        );
    }""", 1),
]
fails = []
for f, old, new, cnt in E:
    src = open(f).read()
    n = src.count(old)
    if n != cnt:
        fails.append((f, old[:70], n, cnt)); continue
    open(f, 'w').write(src.replace(old, new))
print('applied', len(E) - len(fails), 'of', len(E))
for x in fails: print('FAIL', x)
