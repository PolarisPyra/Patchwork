//! Scheduled tasks: the list, saving, running, and removing them.

use super::*;

#[tauri::command]
pub(crate) fn list_scheduled_tasks(server: Srv) -> Vec<ScheduledTask> {
    server.store.scheduled_tasks()
}

/// Adds or replaces a task, and returns it as saved, with its next run worked out.
#[tauri::command]
pub(crate) fn save_scheduled_task(server: Srv, task: ScheduledTask) -> Res<ScheduledTask> {
    server.save_scheduled_task(task)
}

#[tauri::command]
pub(crate) fn remove_scheduled_task(server: Srv, id: String) -> Res<()> {
    server.store.remove_scheduled_task(&id)
}

/// Starts a task now, whatever its schedule says.
#[tauri::command]
pub(crate) async fn run_scheduled_task(server: Srv<'_>, id: String) -> Res<Thread> {
    server.run_scheduled_task(&id)
}
