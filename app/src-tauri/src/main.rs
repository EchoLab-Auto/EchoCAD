#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;

use commands::{
    add_arc, add_circle, add_constraint, add_extrude_feature, add_line, add_point,
    add_spline, add_ellipse,
    add_fillet_feature, add_fillet_edges_feature, add_chamfer_feature, add_chamfer_edges_feature,
    add_linear_pattern, add_circular_pattern,
    add_mirror_feature, add_sweep_feature, add_shell_feature, add_boolean_feature,
    add_revolve_feature, add_sketch_feature, create_offset_plane,
    clear_document, clear_recent_files, clear_sketch, delete_feature,
    export_obj, export_stl, export_gltf_cmd, check_recovery, generate_plugin_feature, get_features, get_recent_files,
    get_regen_errors, get_all_solid_meshes, get_sketch_entities, get_sketch_constraints,
    get_solid_mesh, load_project_from, preview_extrude, list_generators, list_plugins,
    load_project_cmd, move_point, remove_constraint, save_project_cmd, save_project_to,
    set_active_sketch, solve_sketch, undo, redo, can_undo_redo, update_constraint_value,
    update_entity_prop, delete_entity, update_parameter, rename_feature,
    set_feature_suppressed, set_feature_color,
    measure_distance, measure_angle,
    update_linear_pattern, update_circular_pattern, update_mirror_params,
    AppState,
};

#[allow(unused_imports)]
use tauri::Manager;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust.", name)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState::new())
        .setup(|_app| {
            #[cfg(debug_assertions)]
            {
                let window = _app.get_webview_window("main").unwrap();
                window.open_devtools();
            }

            // Recovery check: if an autosave snapshot exists and is newer than
            // the most recent opened project, set a flag the frontend can query.
            {
                let state: tauri::State<AppState> = _app.state();
                if let Ok(dir) = _app.path().app_config_dir() {
                    let autosave_path = dir.join("autosave.echi");
                    if autosave_path.exists() {
                        if let Ok(meta) = std::fs::metadata(&autosave_path) {
                            if let Ok(mtime) = meta.modified() {
                                let recent_path = dir.join("recent_files.json");
                                let autosave_is_newer = std::fs::read(&recent_path)
                                    .ok()
                                    .and_then(|bytes| serde_json::from_slice::<Vec<String>>(&bytes).ok())
                                    .map(|files| {
                                        files.first().map_or(true, |most_recent| {
                                            std::fs::metadata(most_recent)
                                                .ok()
                                                .and_then(|rmeta| rmeta.modified().ok())
                                                .map_or(true, |rmtime| mtime > rmtime)
                                        })
                                    })
                                    .unwrap_or(true);
                                if autosave_is_newer {
                                    state.has_recovery_file.store(true, std::sync::atomic::Ordering::Relaxed);
                                }
                            }
                        }
                    }
                }
            }

            // Spawn autosave background thread (writes the document to
            // autosave.echi every 30 seconds). Only locks the document, so
            // it respects the lock-doc → lock-sketch ordering invariant.
            let app_handle = _app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(30));
                    let state: tauri::State<AppState> = app_handle.state();
                    if !state.autosave_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                        continue;
                    }
                    let dir = match app_handle.path().app_config_dir() {
                        Ok(d) => d,
                        Err(_) => continue,
                    };
                    let _ = std::fs::create_dir_all(&dir);
                    let path = dir.join("autosave.echi");
                    let doc = state.lock_doc();
                    if let Err(e) = echi_io::save_project(&*doc, &path) {
                        log::error!("Autosave failed: {}", e);
                    }
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_features,
            add_sketch_feature,
            create_offset_plane,
            set_active_sketch,
            add_extrude_feature,
            add_revolve_feature,
            add_fillet_feature,
            add_fillet_edges_feature,
            add_chamfer_feature,
            add_chamfer_edges_feature,
            add_linear_pattern,
            add_circular_pattern,
            add_mirror_feature,
            add_sweep_feature,
            add_shell_feature,
            add_boolean_feature,
            update_parameter,
            rename_feature,
            set_feature_suppressed,
            set_feature_color,
            delete_feature,
            get_sketch_entities,
            get_sketch_constraints,
            remove_constraint,
            add_point,
            add_line,
            add_circle,
            add_arc,
            add_spline,
            add_ellipse,
            add_constraint,
            update_constraint_value,
            solve_sketch,
            update_entity_prop,
            delete_entity,
            move_point,
            clear_sketch,
            get_solid_mesh,
            preview_extrude,
            get_all_solid_meshes,
            get_regen_errors,
            clear_document,
            save_project_cmd,
            save_project_to,
            load_project_cmd,
            load_project_from,
            get_recent_files,
            clear_recent_files,
            export_stl,
            export_obj,
            export_gltf_cmd,
            check_recovery,
            list_plugins,
            list_generators,
            generate_plugin_feature,
            undo,
            redo,
            can_undo_redo,
            measure_distance,
            measure_angle,
            update_linear_pattern,
            update_circular_pattern,
            update_mirror_params,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
