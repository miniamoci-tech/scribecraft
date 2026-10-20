//! System-wide hotkeys through the `global-hotkey` crate: Windows (RegisterHotKey), macOS
//! (Carbon hot keys, no Accessibility permission needed) and Linux on X11. Wayland has no
//! global hotkeys; registration then fails with a message saying so.
//!
//! [`GlobalHotkeys::new`] must run on the thread that runs the window event loop (the main
//! thread for the desktop app). The handler runs on that thread when a key is pressed or
//! released, even while another program has the focus, so it must be quick.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Once};

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use scribecraft_engine::hotkeys::HotkeyBinding;
use scribecraft_engine::pedal::PedalAction;

use crate::PlatformError;

/// Where key events go: hotkey id and pressed/released.
type Route = Arc<dyn Fn(u32, bool) + Send + Sync>;

/// The crate's event handler can be installed only once per process (it is a one-time
/// cell: a later `set_event_handler` call is silently ignored), so ScribeCraft installs one
/// forwarding handler and swaps the route behind it instead.
static ROUTE: Mutex<Option<Route>> = Mutex::new(None);
static INSTALL: Once = Once::new();

fn install_forwarder() {
    INSTALL.call_once(|| {
        GlobalHotKeyEvent::set_event_handler(Some(|e: GlobalHotKeyEvent| dispatch(e.id(), e.state() == HotKeyState::Pressed)));
    });
}

fn set_route(route: Option<Route>) {
    *ROUTE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = route;
}

/// Delivers one key event to the current route (if any). The lock is released before the
/// route runs, so a route may itself change the route.
fn dispatch(id: u32, pressed: bool) {
    let route = ROUTE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
    if let Some(r) = route {
        r(id, pressed);
    }
}

/// Registered system-wide hotkeys. Dropping it unregisters them.
pub struct GlobalHotkeys {
    manager: GlobalHotKeyManager,
    registered: Vec<HotKey>,
}

impl std::fmt::Debug for GlobalHotkeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalHotkeys").field("registered", &self.registered.len()).finish()
    }
}

impl GlobalHotkeys {
    /// Creates the manager. Call on the event-loop thread.
    pub fn new() -> Result<Self, PlatformError> {
        let manager = GlobalHotKeyManager::new().map_err(|e| PlatformError::Device(format!("system-wide hotkeys are not available here ({e})")))?;
        install_forwarder();
        Ok(Self { manager, registered: Vec::new() })
    }

    /// Replaces the registered keys with `bindings` and routes their presses and releases to
    /// `handler`. Returns one message per binding that could not be registered (bad key name,
    /// or already taken by another program); the others still work.
    pub fn set(&mut self, bindings: &[HotkeyBinding], handler: impl Fn(PedalAction, bool) + Send + Sync + 'static) -> Vec<String> {
        self.clear();
        let mut problems = Vec::new();
        let mut actions: HashMap<u32, PedalAction> = HashMap::new();
        for b in bindings {
            let key: HotKey = match b.keys.trim().parse() {
                Ok(k) => k,
                Err(e) => {
                    problems.push(format!("{:?} is not a key name ({e}); use names like F9 or Ctrl+Shift+KeyP", b.keys));
                    continue;
                }
            };
            if actions.contains_key(&key.id()) {
                problems.push(format!("{} is set twice; only the first is used", b.keys));
                continue;
            }
            match self.manager.register(key) {
                Ok(()) => {
                    actions.insert(key.id(), b.action);
                    self.registered.push(key);
                }
                Err(e) => problems.push(format!("{} could not be registered ({e}); another program may be using it", b.keys)),
            }
        }
        set_route(Some(Arc::new(move |id, pressed| {
            if let Some(action) = actions.get(&id) {
                handler(*action, pressed);
            }
        })));
        problems
    }

    /// Unregisters every key.
    pub fn clear(&mut self) {
        set_route(None);
        // Best effort: a key that failed to unregister is released when the process exits.
        let _ = self.manager.unregister_all(&self.registered);
        self.registered.clear();
    }

    /// Number of keys currently registered.
    pub fn len(&self) -> usize {
        self.registered.len()
    }

    pub fn is_empty(&self) -> bool {
        self.registered.is_empty()
    }
}

impl Drop for GlobalHotkeys {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn the_route_can_be_replaced_and_cleared_repeatedly() {
        let hits = Arc::new(AtomicUsize::new(0));
        for round in 1..=3 {
            let h = hits.clone();
            set_route(Some(Arc::new(move |id, pressed| {
                if id == 7 && pressed {
                    h.fetch_add(1, Ordering::SeqCst);
                }
            })));
            dispatch(7, true);
            dispatch(8, true);
            assert_eq!(hits.load(Ordering::SeqCst), round);
            set_route(None);
            dispatch(7, true);
            assert_eq!(hits.load(Ordering::SeqCst), round, "a cleared route receives nothing");
        }
    }
}
