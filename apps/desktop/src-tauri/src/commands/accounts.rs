//! The optional Patchwork account. Each command calls the account service, so it runs off the
//! main thread.

use super::*;

#[tauri::command]
pub(crate) async fn get_account(server: Srv<'_>) -> Res<Option<Account>> {
    let server = server.inner().clone();
    blocking(move || server.account()).await?
}

#[tauri::command]
pub(crate) async fn account_register(server: Srv<'_>, email: String) -> Res<()> {
    let server = server.inner().clone();
    blocking(move || server.account_register(&email)).await?
}

#[tauri::command]
pub(crate) async fn account_register_finish(
    server: Srv<'_>,
    email: String,
    code: String,
    password: String,
    name: Option<String>,
) -> Res<Account> {
    let server = server.inner().clone();
    blocking(move || server.account_register_finish(&email, &code, &password, name)).await?
}

#[tauri::command]
pub(crate) async fn account_login(
    server: Srv<'_>,
    email: String,
    password: String,
) -> Res<LoginStep> {
    let server = server.inner().clone();
    blocking(move || server.account_login(&email, &password)).await?
}

#[tauri::command]
pub(crate) async fn account_login_verify(
    server: Srv<'_>,
    challenge: String,
    code: String,
    remember: bool,
) -> Res<Account> {
    let server = server.inner().clone();
    blocking(move || server.account_login_verify(&challenge, &code, remember)).await?
}

#[tauri::command]
pub(crate) async fn account_reset_password(server: Srv<'_>, email: String) -> Res<()> {
    let server = server.inner().clone();
    blocking(move || server.account_reset_password(&email)).await?
}

#[tauri::command]
pub(crate) async fn account_reset_password_finish(
    server: Srv<'_>,
    email: String,
    code: String,
    password: String,
) -> Res<()> {
    let server = server.inner().clone();
    blocking(move || server.account_reset_password_finish(&email, &code, &password)).await?
}

#[tauri::command]
pub(crate) async fn account_rename(server: Srv<'_>, name: Option<String>) -> Res<Account> {
    let server = server.inner().clone();
    blocking(move || server.account_rename(name)).await?
}

#[tauri::command]
pub(crate) async fn account_devices(server: Srv<'_>) -> Res<Vec<AccountDevice>> {
    let server = server.inner().clone();
    blocking(move || server.account_devices()).await?
}

#[tauri::command]
pub(crate) async fn account_remove_device(server: Srv<'_>, id: String) -> Res<()> {
    let server = server.inner().clone();
    blocking(move || server.account_remove_device(&id)).await?
}

#[tauri::command]
pub(crate) async fn account_sign_out(server: Srv<'_>) -> Res<()> {
    let server = server.inner().clone();
    blocking(move || server.account_sign_out()).await?
}

#[tauri::command]
pub(crate) async fn account_sign_out_others(server: Srv<'_>) -> Res<()> {
    let server = server.inner().clone();
    blocking(move || server.account_sign_out_others()).await?
}

#[tauri::command]
pub(crate) async fn account_delete(server: Srv<'_>, password: String) -> Res<()> {
    let server = server.inner().clone();
    blocking(move || server.account_delete(&password)).await?
}
