mod config;
mod cores;
mod models;
mod parser;
mod service;
mod storage;
mod windows;
use serde_json::{json, Value};
use service::Service;
use std::sync::{atomic::Ordering, Arc};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager,
};

#[tauri::command]
fn runtime_info() -> Value {
    json!({"version":env!("CARGO_PKG_VERSION"),"platform":std::env::consts::OS,"architecture":std::env::consts::ARCH})
}
#[tauri::command]
fn proxy_snapshot(state: tauri::State<'_, Arc<Service>>) -> models::Snapshot {
    state.snapshot()
}
#[tauri::command]
async fn proxy_action(
    state: tauri::State<'_, Arc<Service>>,
    action: String,
    args: Value,
) -> Result<Value, String> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let text = |key: &str| args[key].as_str().unwrap_or_default().to_string();
        match action.as_str() {
            "import" => state.import(&text("payload")),
            "select" => {
                state.select(&text("id"))?;
                Ok(Value::Null)
            }
            "renameNode" => {
                state.rename_node(&text("id"), &text("name"))?;
                Ok(Value::Null)
            }
            "setNodePin" => {
                state.set_node_pin(&text("id"), &text("fingerprint"))?;
                Ok(Value::Null)
            }
            "deleteNode" => {
                state.delete_node(&text("id"))?;
                Ok(Value::Null)
            }
            "saveSettings" => {
                let s = serde_json::from_value(args["settings"].clone())
                    .map_err(|_| "设置数据格式无效")?;
                state.save_settings(s)?;
                Ok(Value::Null)
            }
            "saveSubscription" => {
                let sub = serde_json::from_value(args["subscription"].clone())
                    .map_err(|_| "订阅数据格式无效")?;
                state.save_subscription(sub)?;
                Ok(Value::Null)
            }
            "deleteSubscription" => {
                state.delete_subscription(&text("id"))?;
                Ok(Value::Null)
            }
            "updateSubscription" => {
                let id = text("id");
                state.start_job("subscription", 1, move |s| s.update_subscription(&id))?;
                Ok(Value::Null)
            }
            "connect" => {
                let id = args["id"].as_str().map(str::to_string);
                state.start_job("connect", 1, move |s| s.connect(id))?;
                Ok(Value::Null)
            }
            "disconnect" => {
                state.disconnect()?;
                Ok(Value::Null)
            }
            "test" => {
                let mode = text("mode");
                let ids: Vec<String> =
                    serde_json::from_value(args.get("ids").cloned().unwrap_or(json!([])))
                        .map_err(|_| "节点列表无效")?;
                state.start_job("test", ids.len(), move |s| s.test_nodes(ids, mode))?;
                Ok(Value::Null)
            }
            "cancel" => {
                state.cancel();
                Ok(Value::Null)
            }
            "diagnostics" => Ok(json!(state.diagnostics()?)),
            "exportDiagnostics" => Ok(json!(state.export_diagnostics()?)),
            "clearLogs" => {
                state.clear_logs()?;
                Ok(Value::Null)
            }
            "restoreBackup" => {
                state.restore_backup()?;
                Ok(Value::Null)
            }
            "checkUpdate" => {
                let kind = text("kind");
                state.start_job("updateCheck", 1, move |s| {
                    let r = s.check_release(&kind)?;
                    let tag = r["tag_name"].as_str().unwrap_or("未知");
                    let result = if kind == "app" {
                        let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
                        match semver::Version::parse(tag.trim_start_matches('v')) {
                            Ok(v) if v > current => {
                                format!("发现新版 {tag}；请在发布页面手动下载，不会自动安装")
                            }
                            Ok(_) => format!("当前已是最新版本（发布版本 {tag}）"),
                            Err(_) => format!("发布版本：{tag}（无法进行版本比较）"),
                        }
                    } else {
                        format!("{kind} 最新版本：{tag}")
                    };
                    Ok(result)
                })?;
                Ok(Value::Null)
            }
            "updateCore" => {
                let kind = text("kind");
                if !["xray", "singbox"].contains(&kind.as_str()) {
                    return Err("核心类型无效".into());
                }
                let geo = args["geoOnly"].as_bool().unwrap_or(false);
                state.start_job("coreUpdate", 1, move |s| s.update_core(&kind, geo))?;
                Ok(Value::Null)
            }
            "openData" => {
                std::process::Command::new("explorer.exe")
                    .arg(&state.root)
                    .spawn()
                    .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            "openRelease" => {
                let repo = state.data.lock().unwrap().settings.update_repo.clone();
                if repo.is_empty() {
                    return Err("请先填写新版发布源".into());
                }
                let url = format!("https://github.com/{repo}/releases/latest");
                std::process::Command::new("rundll32.exe")
                    .args(["url.dll,FileProtocolHandler", &url])
                    .spawn()
                    .map_err(|e| e.to_string())?;
                Ok(Value::Null)
            }
            _ => Err("未知应用操作".into()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .setup(|app| {
            let isolated = std::env::var_os("MYRAY_TEST_ROOT").is_some();
            let root = std::env::var_os("MYRAY_TEST_ROOT")
                .map(std::path::PathBuf::from)
                .unwrap_or(app.path().app_data_dir()?);
            let resources = if cfg!(debug_assertions) {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources")
            } else {
                app.path().resource_dir()?.join("cores")
            };
            let state = Service::new(root, resources, isolated).map_err(std::io::Error::other)?;
            let show = MenuItem::with_id(app, "show", "打开 MyRay Lite", true, None::<&str>)?;
            let disconnect = MenuItem::with_id(app, "disconnect", "断开连接", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &disconnect, &quit])?;
            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("MyRay Lite · 双核心代理客户端")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                    "disconnect" => {
                        let s = app.state::<Arc<Service>>().inner().clone();
                        std::thread::spawn(move || {
                            if let Err(e) = s.disconnect() {
                                s.log("ERROR", "connection", &e);
                            }
                        });
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        if let Some(w) = tray.app_handle().get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.unminimize();
                            let _ = w.set_focus();
                        }
                    }
                })
                .build(app)?;
            let settings = state.data.lock().unwrap().settings.clone();
            state.background();
            app.manage(state.clone());
            if !isolated {
                if settings.auto_connect
                    || (settings.auto_check_updates && !settings.update_repo.is_empty())
                {
                    let _ = state.start_job("startup", 1, move |s| {
                        let mut messages = Vec::new();
                        if settings.auto_connect {
                            match s.connect(None) {
                                Ok(m) => messages.push(m),
                                Err(e) => {
                                    s.log("ERROR", "startup", &e);
                                    messages.push(e);
                                }
                            }
                        }
                        if settings.auto_check_updates && !settings.update_repo.is_empty() {
                            s.cancelled()?;
                            match s.check_release("app") {
                                Ok(r) => messages.push(format!(
                                    "最新发布版本：{}",
                                    r["tag_name"].as_str().unwrap_or("未知")
                                )),
                                Err(e) => {
                                    s.log("ERROR", "startup", &e);
                                    messages.push(e);
                                }
                            }
                        }
                        Ok(messages.join("；"))
                    });
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<Arc<Service>>();
                if state.data.lock().unwrap().settings.minimize_to_tray
                    && !state.exit.load(Ordering::SeqCst)
                {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            runtime_info,
            proxy_snapshot,
            proxy_action
        ])
        .build(tauri::generate_context!())
        .expect("failed to initialize MyRay Lite");
    app.run(|app, event| {
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            if let Some(s) = app.try_state::<Arc<Service>>() {
                if !s.exit.load(Ordering::SeqCst) {
                    s.shutdown();
                }
            }
        }
    });
}
