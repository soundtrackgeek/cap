//! Cooperative interruption for owned terminal sessions and capture phases.
//! Install only after bounded input acquisition, before starting an operation
//! that can poll the token. Signal callbacks never write output or touch SQLite.

use std::sync::{atomic::AtomicBool, atomic::Ordering, Arc, OnceLock};

static TOKEN: OnceLock<Result<Arc<AtomicBool>, String>> = OnceLock::new();

pub fn install() -> Result<Arc<AtomicBool>, String> {
    TOKEN
        .get_or_init(|| {
            let token = Arc::new(AtomicBool::new(false));
            let handler_token = Arc::clone(&token);
            ctrlc::set_handler(move || handler_token.store(true, Ordering::SeqCst))
                .map_err(|error| format!("Unable to install interruption handler: {error}"))?;
            Ok(token)
        })
        .clone()
}

pub fn requested() -> bool {
    TOKEN
        .get()
        .and_then(|result| result.as_ref().ok())
        .is_some_and(|token| token.load(Ordering::SeqCst))
}
