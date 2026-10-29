//! Importing conversations other apps saved.

use super::*;

#[tauri::command]
pub(crate) async fn scan_imports(server: Srv<'_>, kind: ImportSource) -> Res<ImportScan> {
    let server = server.inner().clone();
    blocking(move || server.scan_imports(kind)).await
}

#[tauri::command]
pub(crate) async fn import_threads(server: Srv<'_>, kinds: Vec<ImportSource>) -> Res<u32> {
    let server = server.inner().clone();
    blocking(move || server.import_threads(&kinds)).await?
}
