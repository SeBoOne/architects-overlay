//! Tauri-Commands für die generische Listen-Engine (M9-B, t_36778bc3).
//!
//! Dünne Wrapper über `cache::lists` — dieselbe Struktur wie das Inventar
//! (inventory.rs): lokal-first, kein Provider, kein Sync. Fehler gehen als
//! i18n-Fehlerschlüssel über die IPC-Grenze (err_wrap, Fallback-Kette
//! en → raw key im Frontend).

use crate::i18n_err::err_wrap;
use crate::inventory::FlexId;
use crate::trade::TradeState;
use cache::inventory::NewInventoryEntry;
use cache::lists::{ListDetail, ListSummary, NewListItem, ReservedAdd};
use cache::reservations::{NewReservation, Reservation, ReserveAllResult};
use tauri::State;

/// Alle Listen eines Modus (`craft` | `mining`).
#[tauri::command]
pub fn lists_list(state: State<'_, TradeState>, kind: String) -> Result<Vec<ListSummary>, String> {
    state
        .cache
        .lists_list(&kind)
        .map_err(|e| err_wrap("lists.err_load", e.to_string()))
}

/// Neue Liste anlegen, liefert ihre id.
#[tauri::command]
pub fn list_create(
    state: State<'_, TradeState>,
    kind: String,
    name: String,
    // M9-D (t_36f3966c): erzeugende Liste der Brücke — optional, NULL = standalone.
    source_list_id: Option<u64>,
) -> Result<u64, String> {
    state
        .cache
        .list_create(&kind, &name, source_list_id)
        .map_err(|e| err_wrap("lists.err_create", e.to_string()))
}

/// Liste umbenennen.
#[tauri::command]
pub fn list_rename(state: State<'_, TradeState>, list_id: u64, name: String) -> Result<(), String> {
    state
        .cache
        .list_rename(list_id, &name)
        .map_err(|e| err_wrap("lists.err_rename", e.to_string()))
}

/// Liste inkl. Einträge/Snapshots/Abhak-Stand löschen.
#[tauri::command]
pub fn list_delete(state: State<'_, TradeState>, list_id: u64) -> Result<(), String> {
    state
        .cache
        .list_delete(list_id)
        .map_err(|e| err_wrap("lists.err_delete", e.to_string()))
}

/// Blueprint-Eintrag (mit Material-Snapshot inkl. Regler-Qualitäten) anhängen.
#[tauri::command]
pub fn list_add_item(
    state: State<'_, TradeState>,
    list_id: u64,
    item: NewListItem,
) -> Result<u64, String> {
    state
        .cache
        .list_add_item(list_id, item)
        .map_err(|e| err_wrap("lists.err_add", e.to_string()))
}

/// Eintrag aus der Liste nehmen.
#[tauri::command]
pub fn list_remove_item(state: State<'_, TradeState>, item_row: u64) -> Result<(), String> {
    state
        .cache
        .list_remove_item(item_row)
        .map_err(|e| err_wrap("lists.err_remove", e.to_string()))
}

/// copies eines Eintrags ändern (wie oft gecraftet wird).
#[tauri::command]
pub fn list_set_copies(
    state: State<'_, TradeState>,
    item_row: u64,
    copies: f64,
) -> Result<(), String> {
    state
        .cache
        .list_set_copies(item_row, copies)
        .map_err(|e| err_wrap("lists.err_copies", e.to_string()))
}

/// Abhak-Stand eines aggregierten Materials setzen (ganz/teilweise/Reset).
#[tauri::command]
pub fn list_set_done(
    state: State<'_, TradeState>,
    list_id: u64,
    material: String,
    unit: String,
    done_qty: Option<f64>,
) -> Result<(), String> {
    state
        .cache
        .list_set_done(list_id, &material, &unit, done_qty)
        .map_err(|e| err_wrap("lists.err_done", e.to_string()))
}

/// Volle Liste: Kopf + Einträge + Material-Aggregation.
#[tauri::command]
pub fn list_detail(state: State<'_, TradeState>, list_id: u64) -> Result<ListDetail, String> {
    state
        .cache
        .list_detail(list_id)
        .map_err(|e| err_wrap("lists.err_load", e.to_string()))
}

/// M9-E (t_aae8d4c9): Material über eine Liste ins Inventar aufnehmen —
/// Ort + Qualität werden im Listen-Kontext abgefragt, die Position wird
/// „reserviert für Liste X" und eine höhere Qualität hebt die Anforderung
/// der Liste an (`requirement_raised` + neue `requirement`).
#[tauri::command]
pub fn inventory_add_for_list(
    state: State<'_, TradeState>,
    entry: NewInventoryEntry,
    list_id: u64,
    material: String,
    unit: String,
) -> Result<ReservedAdd, String> {
    state
        .cache
        .inventory_add_for_list(entry, list_id, &material, &unit)
        .map_err(|e| err_wrap("lists.err_add_inventory", e.to_string()))
}

/// M9-F (t_bb97da62): Reservierung AUS dem Inventar anlegen — eine Position,
/// eine Menge (partiell erlaubt) und ein Zweck (Liste und/oder freier Text).
#[tauri::command]
pub fn reservation_add(
    state: State<'_, TradeState>,
    new: NewReservation,
) -> Result<Reservation, String> {
    let id = state
        .cache
        .reservation_add(new)
        .map_err(|e| err_wrap("reservations.err_add", e.to_string()))?;
    state
        .cache
        .reservations_list()
        .map_err(|e| err_wrap("reservations.err_add", e.to_string()))?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| err_wrap("reservations.err_add", format!("row {id} vanished")))
}

/// M9-F: Reservierung aufheben.
#[tauri::command]
pub fn reservation_delete(state: State<'_, TradeState>, id: FlexId) -> Result<(), String> {
    state
        .cache
        .reservation_delete(id.0)
        .map_err(|e| err_wrap("reservations.err_delete", e.to_string()))
}

/// M9-J (t_14cc5d4e): „Alles reservieren“ für eine Liste — Greedy best→worst
/// über alle Materialien, Ø-Qualitätsregel je Material, Cap Summe ≤ Bedarf.
/// Re-Click = komplette Neuberechnung (Alt-Reservierungen der Liste ersetzt).
#[tauri::command]
pub fn lists_reserve_all(
    state: State<'_, TradeState>,
    list_id: FlexId,
) -> Result<ReserveAllResult, String> {
    state
        .cache
        .list_reserve_all(list_id.0)
        .map_err(|e| err_wrap("reservations.err_reserve_all", e.to_string()))
}
