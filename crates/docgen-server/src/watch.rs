//! The debounced `notify` file watcher. Watches `docs/` recursively; on any
//! debounced change it rebuilds the site and broadcasts a reload. Failures are
//! logged and swallowed so a bad save never tears down the server.

use std::path::Path;

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};

use crate::{rebuild_and_reload, AppState};

/// Spawn a 200ms-debounced recursive watcher on `docs_dir`. The returned
/// `Debouncer` must be kept alive for the watcher to run (dropping it stops
/// watching); `serve` binds it to a `let _watcher` for the process lifetime.
pub fn spawn_watcher(
    state: AppState,
    docs_dir: &Path,
) -> anyhow::Result<Debouncer<notify::RecommendedWatcher>> {
    let mut debouncer = new_debouncer(
        std::time::Duration::from_millis(200),
        move |res: DebounceEventResult| match res {
            Ok(_events) => {
                // Skip the echo of an editor save: put_source already rebuilt +
                // reloaded for this exact change, so avoid a double rebuild/reload.
                if state.take_self_write_suppression() {
                    tracing::debug!("skipping watcher rebuild: editor-initiated write");
                    return;
                }
                if let Err(e) = rebuild_and_reload(&state) {
                    tracing::error!("rebuild after fs change failed: {e:#}");
                }
            }
            Err(e) => tracing::error!("watch error: {e:?}"),
        },
    )?;
    debouncer
        .watcher()
        .watch(docs_dir, RecursiveMode::Recursive)?;
    Ok(debouncer)
}
