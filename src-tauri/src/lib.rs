mod addons;
mod commands;
mod config;
mod db;
mod environment;
mod service;
mod ssl;
mod state;
mod workspace;

use commands::addon_commands::{
    enable_addon, get_addon_states, install_native_addon, list_addons, restart_addon,
    set_addon_dashboard_visibility, start_addon, stop_addon,
};
use commands::config_commands::{
    check_port_availability, enable_long_paths, get_config, get_long_paths_enabled, set_ports,
    set_preferred_editor, set_show_recovery_in_dashboard, set_tld, set_update_checks_enabled,
    suggest_available_web_port,
};
use commands::database_tools_commands::{
    create_database_user, database_backup_all, database_repair_all, database_restore_backup,
    get_database_tool_status, list_database_backups, set_database_root_password,
    set_database_version,
};
use commands::dev_tools_commands::{
    get_workspace_debug_context, get_wp_tool_status, get_wp_version, repair_workspace, run_wp_cli,
    wp_cache_flush, wp_core_reinstall, wp_core_update, wp_db_size, wp_performance_analysis,
    wp_plugin_activate, wp_plugin_deactivate, wp_plugin_delete, wp_plugin_list, wp_search_replace,
    wp_security_audit, wp_security_harden, wp_site_health, wp_theme_activate, wp_theme_list,
    wp_transient_cleanup, wp_update_all, wp_workspace_info,
};
use commands::port_commands::{kill_process, list_known_ports};
use commands::service_commands::{
    get_service_statuses, get_services, restart_service, start_service, stop_service,
};
use commands::service_control_commands::{
    get_service_config_paths, get_service_log_paths, graceful_restart_service,
    list_installed_web_apps, read_service_log, reload_service, test_service_config,
};
use commands::ssl_commands::{
    finish_domain_setup, get_ca_trusted, list_devpanel_hosts_entries, remove_hosts_entry,
    sync_workspace_hosts, trust_local_ca,
};
use commands::stack_commands::{
    get_active_stack, get_stacks, set_active_stack, start_stack, stop_stack,
};
use commands::update_commands::check_for_update;
use commands::workspace_commands::{
    create_workspace, delete_workspace_all, delete_workspace_config, delete_workspace_data,
    discover_workspace_folders,
    get_php_extensions, get_runtime_catalog, get_site_presets, get_workspace_paths,
    get_xdebug_mode, install_xdebug, launch_heidisql, launch_workspace_editor,
    launch_workspace_tool, list_workspaces, list_xdebug_output, open_workspace_folder,
    open_xdebug_output_folder, refresh_runtime_detection, retry_database_setup,
    set_php_extension, set_workspace_runtime_profile, set_xdebug_mode, start_workspace,
    stop_workspace, uninstall_workspace_keep_data,
};
use config::ConfigManager;
use service::ServiceManager;
use state::AppState;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, RunEvent, Runtime,
};

fn toggle_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            window.hide().ok();
        } else {
            window.show().ok();
            window.set_focus().ok();
        }
    }
}

fn show_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        window.show().ok();
        window.unminimize().ok();
        window.set_focus().ok();
    }
}

/// Headless privileged-helper mode: when re-invoked as
/// `devpanel.exe --hosts-op <add|remove> <domain>` or
/// `devpanel.exe --hosts-batch <file>` (always via `ssl::elevate`'s UAC
/// re-launch, never directly by the user), perform just that one hosts-file
/// write and exit — no Tauri app, no window. Keeps the main DevPanel
/// process permanently unprivileged.
fn run_hosts_op_if_requested() {
    let args: Vec<String> = std::env::args().collect();

    if let Some(pos) = args.iter().position(|a| a == "--hosts-batch") {
        let result = match args.get(pos + 1) {
            Some(file) => ssl::hosts::apply_batch_direct(std::path::Path::new(file)),
            None => Err("invalid --hosts-batch arguments".to_string()),
        };
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }

    let Some(pos) = args.iter().position(|a| a == "--hosts-op") else {
        return;
    };
    let op = args.get(pos + 1).map(String::as_str);
    let domain = args.get(pos + 2);

    let result = match (op, domain) {
        (Some("add"), Some(d)) => ssl::hosts::add_entry_direct(d),
        (Some("remove"), Some(d)) => ssl::hosts::remove_entry_direct(d),
        _ => Err("invalid --hosts-op arguments".to_string()),
    };

    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    run_hosts_op_if_requested();

    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(tauri_plugin_log::log::LevelFilter::Debug)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: None,
                    }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                ])
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let config = ConfigManager::new();
            let service_mgr = ServiceManager::new();
            let detected = service_mgr.detect_services(
                &config.get().ports,
                config.get().mysql_version.as_deref(),
                &[],
            );
            service_mgr.set_definitions(detected);
            app.manage(AppState::new(service_mgr, config));

            // Regenerate every site's vhost for the currently active engine
            // at startup — a stale or hand-edited generated vhost self-heals
            // on next launch instead of needing a manual "repair" action.
            // `workspace.json` is the actual source of truth per site; this
            // just recompiles it into whichever engine is active right now.
            {
                let state = app.state::<AppState>();
                tauri::async_runtime::block_on(async {
                    let workspaces = state.workspace_store.lock().await.list();
                    let (root, www_dir, http_port, stack) = {
                        let config = state.config.lock().await;
                        let root = state.service_mgr.root().clone();
                        let www_dir = config.get().www_dir.clone().unwrap_or_else(|| "www".into());
                        let stack_id = config
                            .get()
                            .active_stack_id
                            .as_deref()
                            .unwrap_or(environment::DEFAULT_STACK_ID);
                        let stack = environment::find_stack(stack_id);
                        let http_port = stack
                            .as_ref()
                            .map(|stack| config.get().ports.public_http_port(stack))
                            .unwrap_or(config.get().ports.apache);
                        (root, www_dir, http_port, stack)
                    };
                    if let Ok(stack) = stack {
                        for warning in environment::transition::on_stack_changed(
                            &stack,
                            &workspaces,
                            &root,
                            &www_dir,
                            http_port,
                        ) {
                            log::warn!("vhost resync at startup: {warning}");
                        }
                    }
                });
            }

            let show_item = MenuItem::with_id(app, "show", "Show Panel", true, None::<&str>)?;
            let stop_all_item =
                MenuItem::with_id(app, "stop_all", "Stop Services", true, None::<&str>)?;
            let close_all_item =
                MenuItem::with_id(app, "close_all", "Close All Sites", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(
                app,
                &[
                    &show_item,
                    &stop_all_item,
                    &close_all_item,
                    &separator,
                    &quit_item,
                ],
            )?;

            TrayIconBuilder::with_id("devpanel-tray")
                .icon(
                    app.default_window_icon()
                        .expect("DevPanel window icon must be configured")
                        .clone(),
                )
                .tooltip("DevPanel")
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_window(app),
                    "stop_all" => {
                        let state = app.state::<AppState>();
                        tauri::async_runtime::block_on(state.service_mgr.stop_all());
                    }
                    "close_all" => {
                        let state = app.state::<AppState>();
                        tauri::async_runtime::block_on(async {
                            let mut store = state.workspace_store.lock().await;
                            let ids: Vec<String> =
                                store.list().into_iter().map(|ws| ws.id.clone()).collect();
                            for id in ids {
                                if let Some(mut ws) = store.get(&id) {
                                    ws.running = false;
                                    let _ = store.update(ws);
                                }
                            }
                        });
                        tauri::async_runtime::block_on(state.service_mgr.stop_all());
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_window(tray.app_handle());
                    }
                })
                .build(app)?;

            // The window starts hidden (tauri.conf.json `"visible": false`) so the
            // tray/menu wiring above is ready before it's shown — force it visible now.
            show_window(app.handle());

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Resized(_) = event {
                if window.is_minimized().unwrap_or(false) {
                    window.hide().ok();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_services,
            get_service_statuses,
            start_service,
            stop_service,
            restart_service,
            get_config,
            check_port_availability,
            suggest_available_web_port,
            get_stacks,
            get_active_stack,
            set_active_stack,
            start_stack,
            stop_stack,
            list_workspaces,
            discover_workspace_folders,
            get_workspace_paths,
            get_runtime_catalog,
            refresh_runtime_detection,
            get_site_presets,
            get_php_extensions,
            set_php_extension,
            install_xdebug,
            set_workspace_runtime_profile,
            launch_workspace_tool,
            launch_workspace_editor,
            create_workspace,
            retry_database_setup,
            start_workspace,
            stop_workspace,
            delete_workspace_all,
            open_workspace_folder,
            uninstall_workspace_keep_data,
            delete_workspace_data,
            delete_workspace_config,
            get_ca_trusted,
            trust_local_ca,
            finish_domain_setup,
            sync_workspace_hosts,
            list_devpanel_hosts_entries,
            remove_hosts_entry,
            set_tld,
            set_ports,
            set_show_recovery_in_dashboard,
            set_preferred_editor,
            set_update_checks_enabled,
            check_for_update,
            get_long_paths_enabled,
            enable_long_paths,
            get_database_tool_status,
            set_database_root_password,
            create_database_user,
            database_repair_all,
            database_backup_all,
            list_database_backups,
            database_restore_backup,
            set_database_version,
            run_wp_cli,
            get_wp_version,
            get_wp_tool_status,
            repair_workspace,
            get_workspace_debug_context,
            // WordPress Plugin Management
            wp_plugin_list,
            wp_plugin_activate,
            wp_plugin_deactivate,
            wp_plugin_delete,
            // WordPress Theme Management
            wp_theme_list,
            wp_theme_activate,
            // WordPress Core Management
            wp_core_update,
            wp_core_reinstall,
            wp_update_all,
            wp_cache_flush,
            wp_transient_cleanup,
            wp_search_replace,
            // WordPress Security
            wp_security_audit,
            wp_security_harden,
            // WordPress Performance & Health
            wp_site_health,
            wp_performance_analysis,
            wp_db_size,
            wp_workspace_info,
            reload_service,
            test_service_config,
            get_service_log_paths,
            get_service_config_paths,
            read_service_log,
            graceful_restart_service,
            list_addons,
            install_native_addon,
            enable_addon,
            set_addon_dashboard_visibility,
            get_addon_states,
            start_addon,
            stop_addon,
            restart_addon,
            get_xdebug_mode,
            set_xdebug_mode,
            list_xdebug_output,
            open_xdebug_output_folder,
            launch_heidisql,
            list_installed_web_apps,
            list_known_ports,
            kill_process,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let RunEvent::Exit = event {
            // Graceful stop (mysqladmin/httpd -k stop) then force-kill by
            // tracked PID — only touches processes DevPanel actually spawned.
            let state = app_handle.state::<AppState>();
            tauri::async_runtime::block_on(state.service_mgr.stop_all());
        }
    });
}
