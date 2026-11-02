//! (see module docs)
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

//! H5-Fallback: Low-Level-Keyboard-Hook für Global-Shortcuts, wenn Star
//! Citizen fokussiert ist (t_ed01e8f7).
//!
//! Befund (Sebo, VM): SC fokussiert → `WM_HOTKEY` erreicht den
//! global-shortcut-Handler nicht mehr; SC im Hintergrund → funktioniert.
//! Ursache: der SC-/Anti-Cheat-Low-Level-Keyboard-Hook verschluckt die
//! Taste, BEVOR das System `WM_HOTKEY` generiert (LL-Hooks laufen in
//! umgekehrter Installationsreihenfolge — der zuletzt installierte Hook
//! sieht die Taste zuerst).
//!
//! Fallback: eigener `WH_KEYBOARD_LL`-Hook, installiert NACH dem SC-Start
//! (synchron zum Game-Poller via `sync_game_running`), der NUR die
//! registrierten Combos erkennt, an denselben Dispatch-Pfad wie `WM_HOTKEY`
//! weiterleitet (`shortcut_diag::dispatch_kind`, mit Dedupe gegen den
//! Normal-Pfad) und alle KeyEvents ungehindert durchreicht
//! (`CallNextHookEx` — das Spiel sieht die Taste weiter).
//!
//! Lifecycle: install bei `game_running=true`, unhook bei `false` und bei
//! App-Shutdown (`RunEvent::Exit`). Der Hook lebt auf einem eigenen
//! Message-Loop-Thread (Pflicht für `WH_KEYBOARD_LL`).
//!
//! Die Combo-Erkennung ist die pure Schicht (`Combo`/`ComboMatcher`/
//! `parse_combo`) — plattformunabhängig unit-getestet.

/// Eine globales Shortcut-Combo in LL-Hook-Normalform (virtuelle Keycodes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub win: bool,
    /// Virtueller Keycode des Nicht-Modifier-Taste (z. B. `0x55` = U).
    pub vk: u32,
}

/// Modifier-VKs (inkl. Links/Rechts-Varianten) → Bitmaske.
const MOD_SHIFT: u32 = 1;
const MOD_CTRL: u32 = 2;
const MOD_ALT: u32 = 4;
const MOD_WIN: u32 = 8;

fn modifier_mask(vk: u32) -> u32 {
    match vk {
        0x10 | 0xA0 | 0xA1 => MOD_SHIFT, // VK_SHIFT / L / R
        0x11 | 0xA2 | 0xA3 => MOD_CTRL,  // VK_CONTROL / L / R
        0x12 | 0xA4 | 0xA5 => MOD_ALT,   // VK_MENU / L / R
        0x5B | 0x5C => MOD_WIN,          // VK_LWIN / VK_RWIN
        _ => 0,
    }
}

/// Buchstaben, Ziffern, F1–F12, Minus, Gleich, Komma, Punkt, Slash → VK.
/// Reicht für die kanonischen Settings-Combos (`ctrl+shift+u` etc.); ein
/// unmappbares Token ist ein plausibler Fehler (UI-Validierung).
pub fn vk_from_key(token: &str) -> Option<u32> {
    let lower = token.to_ascii_lowercase();
    let vk = match lower.as_str() {
        "f1" => 0x70,
        "f2" => 0x71,
        "f3" => 0x72,
        "f4" => 0x73,
        "f5" => 0x74,
        "f6" => 0x75,
        "f7" => 0x76,
        "f8" => 0x77,
        "f9" => 0x78,
        "f10" => 0x79,
        "f11" => 0x7A,
        "f12" => 0x7B,
        "minus" | "-" => 0xBD,
        "equal" | "=" => 0xBB,
        "comma" | "," => 0xBC,
        "period" | "." => 0xBE,
        "slash" | "/" => 0xBF,
        "backslash" | "\\" => 0xDC,
        _ => return None,
    };
    Some(vk)
}

/// Einzelzeichen (a–z, 0–9) → VK.
pub fn vk_from_char(ch: char) -> Option<u32> {
    let upper = ch.to_ascii_uppercase();
    let c = upper as u32;
    if upper.is_ascii_uppercase() || upper.is_ascii_digit() {
        Some(c) // 'A'..'Z' == 0x41..0x5A == VK_A..VK_Z, '0'..'9' == 0x30..0x39
    } else {
        None
    }
}

/// Kanonischen Combo-String (`"ctrl+shift+u"`, exakt das Persistenzformat)
/// in die LL-Hook-Normalform übersetzen. Alle Token müssen bekannt sein —
/// ein Teiltreffer ist `None` (Registrierungsfehler, kein stiller Bug).
/// WK-R3 (t_ed01e8f7): Token im JS-KeyboardEvent-Format ("KeyU", "Digit1",
/// "Numpad1") auf den Rohbuchstaben abbilden und dem normalen vk_from_key-
/// Pfad zuführen — Slot-Settings persistieren auf Windows genau so.
fn vk_from_token(t: &str) -> Option<u32> {
    let lower = t.to_ascii_lowercase();
    for prefix in ["key", "digit", "numpad"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if rest.chars().count() == 1 {
                return vk_from_char(rest.chars().next().expect("count==1"));
            }
        }
    }
    vk_from_key(t)
}

pub fn parse_combo(canonical: &str) -> Option<Combo> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut win = false;
    let mut vk = None;
    for token in canonical.split('+') {
        match token.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" | "option" => alt = true,
            "super" | "meta" | "cmd" | "command" | "win" => win = true,
            t if t.chars().count() == 1 => {
                let ch = t.chars().next().expect("len==1");
                vk = Some(vk_from_char(ch)?);
            }
            // WK-R3 (t_ed01e8f7): Slot-Settings persistieren auf Windows im
            // JS-KeyboardEvent-Format ("shift+control+KeyU") — parse_combo
            // kannte nur "ctrl+shift+u" und warf die Targets stumm weg
            // (Diag-Befund: 32 Keydowns, 0 Matches). "KeyU"/"Digit1"/
            // "Numpad1" etc. sind jetzt äquivalent gültig.
            t => vk = Some(vk_from_token(t)?),
        }
    }
    // Combo ohne Taste („ctrl+shift“) ist ein Validierungsfehler.
    let vk = vk?;
    Some(Combo {
        ctrl,
        shift,
        alt,
        win,
        vk,
    })
}

/// Stateful Kombo-Erkennung aus LL-KeyEvents. Nur Zustände der registrierten
/// Combos und der Modifier werden geführt — keine Historie, kein blocking.
#[derive(Debug, Default)]
pub struct ComboMatcher {
    targets: Vec<Combo>,
    mods: u32,
}

impl ComboMatcher {
    pub fn new(targets: Vec<Combo>) -> Self {
        Self { targets, mods: 0 }
    }

    /// Einem KeyEvent (Virtueller Keycode, down/up) folgen. Liefert den
    /// Index des Treffers, wenn eine registrierte Nicht-Modifier-Taste
    /// HERUNTERgedrückt wird und die aktuellen Modifier exakt passen.
    /// `None` sonst (auch für alle Ups — Repeat-Filter macht der Hook-Thread
    /// über den `is_key_down`-Filter).
    pub fn on_key(&mut self, vk: u32, is_key_down: bool) -> Option<usize> {
        let modbit = modifier_mask(vk);
        if modbit != 0 {
            match is_key_down {
                true => self.mods |= modbit,
                false => self.mods &= !modbit,
            }
            return None;
        }
        if !is_key_down {
            return None;
        }
        self.targets
            .iter()
            .position(|t| t.vk == vk && self.mods == Self::mask_of(t))
    }

    fn mask_of(t: &Combo) -> u32 {
        let mut m = 0;
        if t.ctrl {
            m |= MOD_CTRL;
        }
        if t.shift {
            m |= MOD_SHIFT;
        }
        if t.alt {
            m |= MOD_ALT;
        }
        if t.win {
            m |= MOD_WIN;
        }
        m
    }
}

/// Dedupe-Fenster: ein Combo, das der LL-Hook gerade an `dispatch_kind`
/// weitergeleitet hat, darf vom WM_HOTKEY-Normalpfad (der bei SC im
/// Hintergrund parallel feuert) nicht doppelt auslösen — und umgekehrt.
const DEDUPE_WINDOW: std::time::Duration = std::time::Duration::from_millis(600);

#[cfg(target_os = "windows")]
use crate::shortcut_diag;
/// Hook-Laufzeit auf dem Message-Loop-Thread. Der Hook-Proc läuft IM
/// Kontext dieses Threads — daher die thread_local-Verkabelung.
// WK-R1: Windows-Scope braucht AppHandle + den Sibling shortcut_diag —
// auf Linux wird das Modul nie kompiliert, deshalb fielen die fehlenden
// Imports nur im Windows-Target auf (CI-Check, E0425/E0433).
#[cfg(target_os = "windows")]
use tauri::AppHandle;

#[cfg(target_os = "windows")]
struct HookRuntime {
    matcher: std::sync::Mutex<ComboMatcher>,
    /// WK-R1 Defekt 2: Kind am Combo (deckungsgleicher Index), nicht geraten.
    kinds: Vec<crate::overlay::ShortcutKind>,
    /// Das konkrete Handle wird bei `install::<R>` in die Closure gefangen —
    /// der thread_local-Runtime-Slot kann nicht generisch sein.
    dispatch: Box<dyn Fn(crate::overlay::ShortcutKind)>,
}

#[cfg(target_os = "windows")]
std::thread_local! {
    static HOOK_CTX: std::cell::RefCell<Option<HookRuntime>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(target_os = "windows")]
static HOOK_THREAD_ID: std::sync::Mutex<Option<u32>> = std::sync::Mutex::new(None);

#[cfg(target_os = "windows")]
/// Sync mit dem Game-Poller: SC gestartet → Hook installieren (NACH dem
/// Anti-Cheat-Hook → der sieht unsere Taste zuerst), SC beendet → unhook.
pub fn sync_game_running<R: tauri::Runtime>(app: &AppHandle<R>, running: bool) {
    if running {
        install(app.clone());
    } else {
        uninstall();
    }
}

#[cfg(target_os = "windows")]
pub fn install<R: tauri::Runtime>(app: AppHandle<R>) {
    let already = *HOOK_THREAD_ID.lock().expect("hook thread id lock");
    if already.is_some() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("arch-llhotkey".into())
        .spawn(move || hook_thread(app));
    match spawned {
        Ok(_) => log::info!("LL-keyboard-hook thread spawned (SC running)"),
        Err(err) => log::warn!("LL-keyboard-hook thread spawn failed: {err}"),
    }
}

#[cfg(target_os = "windows")]
pub fn uninstall() {
    let thread_id = HOOK_THREAD_ID.lock().expect("hook thread id lock").take();
    if let Some(id) = thread_id {
        unsafe {
            // WM_QUIT beendet die GetMessage-Schleife; Unhook macht der
            // Thread selbst (aus demselben Kontext, wie Windows es verlangt).
            windows_sys::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                id, 0x0012, /* WM_QUIT */
                0, 0,
            );
        }
        log::info!("LL-keyboard-hook thread stopping (SC not running)");
    }
}

/// App-Shutdown-Häkchen (RunEvent::Exit): Hook sauber entfernen.
#[cfg(target_os = "windows")]
pub fn shutdown() {
    uninstall();
}

#[cfg(target_os = "windows")]
fn hook_thread<R: tauri::Runtime>(app: AppHandle<R>) {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetMessageW, MSG, SetWindowsHookExW, WH_KEYBOARD_LL,
    };

    let targets = read_targets(&app);
    log::info!(
        "LL-keyboard-hook registering for targets: {:?}",
        targets
            .iter()
            .map(|(c, k)| format!("{:?}={}", k, format_combo(c)))
            .collect::<Vec<_>>()
    );
    let kinds: Vec<crate::overlay::ShortcutKind> = targets.iter().map(|(_, k)| *k).collect();

    HOOK_CTX.with(|ctx| {
        *ctx.borrow_mut() = Some(HookRuntime {
            matcher: std::sync::Mutex::new(ComboMatcher::new(
                targets.into_iter().map(|(c, _)| c).collect(),
            )),
            kinds,
            dispatch: Box::new(move |kind| {
                shortcut_diag::dispatch_kind(&app, kind);
            }),
        });
    });

    unsafe {
        let hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(ll_hook_proc),
            GetModuleHandleW(std::ptr::null()),
            0,
        );
        if hook.is_null() {
            log::warn!("SetWindowsHookExW(WH_KEYBOARD_LL) failed — Fallback-Hook inaktiv");
            return;
        }
        // Thread-ID für den Unhook-Pfad registrieren.
        let tid = windows_sys::Win32::System::Threading::GetCurrentThreadId();
        *HOOK_THREAD_ID.lock().expect("hook thread id lock") = Some(tid);

        // Message-Loop-Pflicht: der LL-Hook wird nur abgeliefert, solange
        // dieser Thread Nachrichten pumpt.
        let mut msg: MSG = std::mem::zeroed();
        loop {
            if GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) <= 0 {
                break;
            }
        }
        windows_sys::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(hook);
        *HOOK_THREAD_ID.lock().expect("hook thread id lock") = None;
        log::info!("LL-keyboard-hook unhooked");
    }
}

/// Pure Schicht (plattformunabhängig unit-getestet): Slot-Settings in
/// (Combo, Kind)-Paare überführen. Parsebare Slots behalten IHR Kind —
/// unparsebare fallen nur als PAAR weg, sodass keine Index-Verrutschung
/// entsteht (Defekt 2, WK-R1: vorher fielen Combos einzeln raus und der
/// Hook-Proc riet das Kind aus der Position).
fn build_targets(
    slots: [(crate::overlay::ShortcutKind, Option<String>); 2],
) -> Vec<(Combo, crate::overlay::ShortcutKind)> {
    slots
        .into_iter()
        .filter_map(|(kind, s)| {
            let s = s?;
            let combo = parse_combo(&s)?;
            Some((combo, kind))
        })
        .collect()
}

/// Live-Bindings → (Combo, Kind)-Paare (kanonische Strings der Slot-Settings).
#[cfg(target_os = "windows")]
fn read_targets<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Vec<(Combo, crate::overlay::ShortcutKind)> {
    use tauri::Manager;
    app.try_state::<crate::overlay::ShortcutBindings>()
        .map(|b| {
            build_targets([
                (
                    crate::overlay::ShortcutKind::Toggle,
                    b.current(crate::overlay::ShortcutKind::Toggle)
                        .map(|s| s.to_string()),
                ),
                (
                    crate::overlay::ShortcutKind::ClickThrough,
                    b.current(crate::overlay::ShortcutKind::ClickThrough)
                        .map(|s| s.to_string()),
                ),
            ])
        })
        .unwrap_or_default()
}

#[cfg(target_os = "windows")]
fn format_combo(c: &Combo) -> String {
    let mut s = String::new();
    if c.ctrl {
        s.push_str("ctrl+");
    }
    if c.shift {
        s.push_str("shift+");
    }
    if c.alt {
        s.push_str("alt+");
    }
    if c.win {
        s.push_str("win+");
    }
    s.push_str(&format!("{:#04x}", c.vk));
    s
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn ll_hook_proc(n_code: i32, w_param: usize, l_param: isize) -> isize {
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CallNextHookEx, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
        };

        // Regelpunkt: KeyEvents NORMAL durchreichen — nur die Combo-Erkennung
        // lauscht mit. Ein Blocken (kein CallNextHookEx) käme einem Tastendiebstahl
        // gleich und wäre exakt der SC-Bug, den wir umgehen.
        let pass_through = || CallNextHookEx(std::ptr::null_mut(), n_code, w_param, l_param);
        if n_code < 0 {
            return pass_through();
        }
        let kb = l_param as *const KBDLLHOOKSTRUCT;
        if kb.is_null() {
            return pass_through();
        }
        let msg_code = w_param as u32;
        let is_key_down = matches!(
            msg_code,
            0x0100 /* WM_KEYDOWN */ | 0x0104 /* WM_SYSKEYDOWN */
        );
        let is_key_up = matches!(
            msg_code,
            0x0101 /* WM_KEYUP */ | 0x0105 /* WM_SYSKEYUP */
        );
        if !is_key_down && !is_key_up {
            return pass_through();
        }
        let kb = unsafe { &*kb };

        HOOK_CTX.with(|ctx| {
            if let Some(rt) = ctx.borrow().as_ref() {
                // Repeat: WM_KEYDOWN-Autorepeat-Wiederholungen bekommen wir als
                // mehrfache Down-Events — der Melder zählt jede als frisch. Der
                // echte Filter: modiere niemals auf Repeat-Messagecodes (unsere
                // Combos sind Down-getriggert; Autorepeat des Ziels wiederholt
                // den Dispatch — akzeptabel fürs Overlay, da der Toggle idempotent
                // wäre? NEIN — Toggle flippt. Autorepeat-Dedupe über den
                // gemeinsamen Dispatch-Zeitstempel unten.)
                let matched = rt
                    .matcher
                    .lock()
                    .expect("ll matcher lock")
                    .on_key(kb.vkCode as u32, is_key_down);
                if let Some(idx) = matched {
                    let Some(&kind) = rt.kinds.get(idx) else {
                        log::warn!("LL-hook matched idx={idx} ohne Kind — übersprungen");
                        return;
                    };
                    log::info!(
                        "LL-hook matched combo idx={idx} vk={:#04x} (down={is_key_down})",
                        kb.vkCode
                    );
                    // Dedupe gegen den WM_HOTKEY-Normalpfad macht dispatch_kind.
                    (rt.dispatch)(kind);
                }
            }
        });

        pass_through()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WK-R3-Regression (Sebo-Diag-Snapshot): persistierte Combos sind im
    /// KeyboardEvent.code-Format — der LL-Hook-Matcher muss sie genauso
    /// auflösen wie das global-shortcut-Plugin, sonst: 32 Keydowns, 0 Matches.
    #[test]
    fn parse_combo_accepts_keycode_format() {
        let c = parse_combo("shift+control+KeyU").expect("KeyU-Combo muss parsen");
        assert!(c.shift && c.ctrl);
        assert_eq!(c.vk, 0x55);

        let c2 = parse_combo("ctrl+KeyO").expect("KeyO-Combo muss parsen");
        assert!(c2.ctrl);
        assert_eq!(c2.vk, 0x4F);

        let c3 = parse_combo("Digit1").expect("Digit1 muss parsen");
        assert_eq!(c3.vk, 0x31);

        let c4 = parse_combo("Numpad1").expect("Numpad1 muss parsen");
        assert_eq!(c4.vk, 0x31);
    }

    #[test]
    fn parse_combo_still_rejects_garbage() {
        assert!(parse_combo("ctrl+NotAKey").is_none());
        assert!(parse_combo("ctrl+shift").is_none());
    }
}
