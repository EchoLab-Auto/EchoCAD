#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;

use commands::{
    add_arc, add_circle, add_constraint, add_extrude_feature, add_line, add_point,
    add_spline, add_ellipse,
    add_fillet_feature, add_chamfer_feature, add_linear_pattern, add_circular_pattern,
    add_mirror_feature, add_sweep_feature, add_shell_feature, add_boolean_feature,
    add_revolve_feature, add_sketch_feature,
    clear_document, clear_recent_files, clear_sketch, delete_feature,
    export_obj, export_stl, generate_plugin_feature, get_features, get_recent_files,
    get_regen_errors, get_all_solid_meshes, get_sketch_entities, get_sketch_constraints,
    get_solid_mesh, load_project_from, preview_extrude, list_generators, list_plugins,
    load_project_cmd, move_point, remove_constraint, save_project_cmd, save_project_to,
    set_active_sketch, solve_sketch, undo, redo, can_undo_redo, update_constraint_value,
    update_entity_prop, delete_entity, update_parameter, rename_feature,
    set_feature_suppressed, AppState,
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_features,
            add_sketch_feature,
            set_active_sketch,
            add_extrude_feature,
            add_revolve_feature,
            add_fillet_feature,
            add_chamfer_feature,
            add_linear_pattern,
            add_circular_pattern,
            add_mirror_feature,
            add_sweep_feature,
            add_shell_feature,
            add_boolean_feature,
            update_parameter,
            rename_feature,
            set_feature_suppressed,
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
            list_plugins,
            list_generators,
            generate_plugin_feature,
            undo,
            redo,
            can_undo_redo,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
