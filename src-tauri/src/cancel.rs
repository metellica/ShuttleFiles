//! Cancellation tokens for long-running commands, keyed by an id the
//! frontend chooses.
//!
//! Searching a deep tree or hashing a multi-gigabyte file can outlive the
//! user's interest in the answer, and a superseded keystroke must not
//! keep a core busy. Both features want the same bookkeeping, so it lives
//! here once.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct CancelRegistry {
    tokens: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl CancelRegistry {
    /// Claim `id`, replacing any token already registered under it. The
    /// old one is cancelled, so firing a new search per keystroke
    /// implicitly stops the previous one.
    pub fn register(&self, id: &str) -> Arc<AtomicBool> {
        let token = Arc::new(AtomicBool::new(false));
        if let Some(previous) = self.tokens.lock().unwrap().insert(id.to_string(), token.clone()) {
            previous.store(true, Ordering::Relaxed);
        }
        token
    }

    pub fn cancel(&self, id: &str) {
        if let Some(token) = self.tokens.lock().unwrap().remove(id) {
            token.store(true, Ordering::Relaxed);
        }
    }

    /// Drop the token once the work is over, so the map cannot grow
    /// without bound over a long session.
    pub fn finish(&self, id: &str, token: &Arc<AtomicBool>) {
        let mut tokens = self.tokens.lock().unwrap();
        // Only clear our own entry: a newer run may already own this id.
        if tokens.get(id).is_some_and(|current| Arc::ptr_eq(current, token)) {
            tokens.remove(id);
        }
    }
}

/// Newtypes so both features can be managed by Tauri, which keys state by type.
#[derive(Default)]
pub struct SearchCancels(pub CancelRegistry);

#[derive(Default)]
pub struct HashCancels(pub CancelRegistry);
