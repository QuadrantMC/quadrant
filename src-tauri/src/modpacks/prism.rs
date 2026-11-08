use quadrant_core::prism::{PrismInstance, PrismSyncPlan};
use quadrant_host::QuadrantHost;
use tauri::{AppHandle, Manager};

#[tauri::command]
pub async fn get_prism_instances(app: AppHandle) -> Vec<PrismInstance> {
    let host = app.state::<QuadrantHost>().inner().clone();
    // Reading every instance folder is filesystem work, so it stays off the
    // main thread.
    tauri::async_runtime::spawn_blocking(move || host.get_prism_instances())
        .await
        .and_then(|instances| instances.map_err(crate::command_error))
        .unwrap_or_else(|error| {
            // The renderer treats this as "no instances"; leave a trace so an
            // empty list caused by an unreadable Prism folder is diagnosable.
            log::error!("Failed to list Prism instances: {error}");
            Vec::new()
        })
}

host_commands! {
    get_prism_sync_plans(name: String) -> Vec<PrismSyncPlan>;
    apply_modpack_to_prism_instance(name: String, instance_id: String) -> ();
}

#[tauri::command]
pub async fn detach_prism_instance(
    instance_id: String,
    app: AppHandle,
) -> Result<(), tauri::Error> {
    let host = app.state::<QuadrantHost>().inner().clone();
    // Restoring the set-aside `mods` folder moves directories, so it stays off
    // the main thread.
    tauri::async_runtime::spawn_blocking(move || host.detach_prism_instance(instance_id))
        .await?
        .map_err(crate::command_error)
}
