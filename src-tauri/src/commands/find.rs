use tauri::{AppHandle, Emitter, Runtime, State};

use crate::cancel::SearchCancels;
use crate::error::AppResult;
use crate::fs::search::{self, SearchResult};

/// Fuzzy-find inside `dir`. `id` identifies the search so a superseded
/// keystroke can be cancelled; reusing the same id automatically stops
/// the previous run.
#[tauri::command]
pub async fn fuzzy_find(
    id: String,
    dir: String,
    query: String,
    recursive: bool,
    limit: Option<usize>,
    cancels: State<'_, SearchCancels>,
) -> AppResult<SearchResult> {
    let token = cancels.0.register(&id);
    let result = search::search(
        dir,
        query,
        recursive,
        limit.unwrap_or_else(search::default_limit),
        token.clone(),
    )
    .await;
    cancels.0.finish(&id, &token);
    result
}

#[tauri::command]
pub fn cancel_search(id: String, cancels: State<'_, SearchCancels>) {
    cancels.0.cancel(&id);
}

// --- Checksums ---------------------------------------------------------------

use crate::cancel::HashCancels;
use crate::fs::hash::{self, HashAlgo, HashResult};

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HashFinished {
    id: String,
    cancelled: bool,
}

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct HashItem {
    id: String,
    result: HashResult,
}

/// Start hashing `paths`. Results arrive as `hash:result` events and the
/// run ends with `hash:finished`, so a long batch streams into the dialog
/// instead of appearing all at once at the end.
///
/// Generic over the runtime so the command can be exercised against
/// Tauri's mock runtime in tests.
#[tauri::command]
pub fn start_hash<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    paths: Vec<String>,
    algos: Vec<HashAlgo>,
    cancels: State<'_, HashCancels>,
) -> AppResult<()> {
    let token = cancels.0.register(&id);
    let job_id = id.clone();

    // Hashing is CPU and IO bound; keep it off the async runtime.
    std::thread::spawn(move || {
        let emit_app = app.clone();
        let progress_id = job_id.clone();
        let cancelled = hash::hash_batch(
            &job_id,
            &paths,
            &algos,
            token,
            |progress| {
                let _ = emit_app.emit("hash:progress", progress);
            },
            |result| {
                let _ = app.emit(
                    "hash:result",
                    HashItem {
                        id: progress_id.clone(),
                        result,
                    },
                );
            },
        );
        let _ = app.emit(
            "hash:finished",
            HashFinished {
                id: job_id,
                cancelled,
            },
        );
    });

    Ok(())
}

#[tauri::command]
pub fn cancel_hash(id: String, cancels: State<'_, HashCancels>) {
    cancels.0.cancel(&id);
}
