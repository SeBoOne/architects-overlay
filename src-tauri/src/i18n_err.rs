//! i18n error transport (T2, t_c5904bcb) — Re-Export der gemeinsamen
//! `i18n-err`-Crate, damit auch die Workspace-Crates (cache, settings,
//! updater) dieselben `{error_key, params}`-Nachrichten erzeugen können.
pub use i18n_err::{err_key, err_keyp, err_wrap};
