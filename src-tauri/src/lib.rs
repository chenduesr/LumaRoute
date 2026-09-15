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
    Emitter, Manager,
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
                Ok(json!(state.save_subscription(sub)?))
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
            "updateAllSubscriptions" => {
                let ids: Vec<String> = state
                    .data
                    .lock()
                    .unwrap()
                    .subscriptions
                    .iter()
                    .map(|subscription| subscription.id.clone())
                    .collect();
                if ids.is_empty() {
                    return Err("还没有可更新的订阅".into());
                }
                state.start_job("subscription", ids.len(), move |s| {
                    s.update_subscriptions(ids)
                })?;
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
    let mut builder = tauri::Builder::default().plugin(tauri_plugin_clipboard_manager::init());
    if std::env::var_os("MYRAY_TEST_ROOT").is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }));
    }
    let app = builder
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
            let status = MenuItem::with_id(app, "status", "↑ 0 B/s  ↓ 0 B/s", false, None::<&str>)?;
            let current =
                MenuItem::with_id(app, "current", "当前节点：未选择", false, None::<&str>)?;
            let connect = MenuItem::with_id(app, "connect-toggle", "连接", true, None::<&str>)?;
            let recent0 = MenuItem::with_id(app, "recent-0", "最近节点 1", false, None::<&str>)?;
            let recent1 = MenuItem::with_id(app, "recent-1", "最近节点 2", false, None::<&str>)?;
            let recent2 = MenuItem::with_id(app, "recent-2", "最近节点 3", false, None::<&str>)?;
            let update = MenuItem::with_id(
                app,
                "update-subscriptions",
                "更新全部订阅",
                true,
                None::<&str>,
            )?;
            let test = MenuItem::with_id(app, "test-current", "测试当前节点", true, None::<&str>)?;
            let simple =
                MenuItem::with_id(app, "toggle-simple", "切换简洁模式", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[
                    &show, &status, &current, &connect, &recent0, &recent1, &recent2, &update,
                    &test, &simple, &quit,
                ],
            )?;
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
                    "connect-toggle" => {
                        let s = app.state::<Arc<Service>>().inner().clone();
                        std::thread::spawn(move || {
                            let result = if s.snapshot().connection.status == "connected" {
                                s.disconnect()
                            } else {
                                s.start_job("connect", 1, |service| service.connect(None))
                            };
                            if let Err(e) = result {
                                s.log("ERROR", "connection", &e);
                            }
                        });
                    }
                    "update-subscriptions" => {
                        let s = app.state::<Arc<Service>>().inner().clone();
                        let ids = s
                            .data
                            .lock()
                            .unwrap()
                            .subscriptions
                            .iter()
                            .map(|item| item.id.clone())
                            .collect::<Vec<_>>();
                        let _ = s.start_job("subscription", ids.len(), move |service| {
                            service.update_subscriptions(ids)
                        });
                    }
                    "test-current" => {
                        let s = app.state::<Arc<Service>>().inner().clone();
                        let selected = s.data.lock().unwrap().active_node_id.clone();
                        if let Some(id) = selected {
                            let _ = s.start_job("test", 1, move |service| {
                                service.test_nodes(vec![id], "http".into())
                            });
                        }
                    }
                    "toggle-simple" => {
                        let _ = app.emit("toggle-simple-mode", ());
                    }
                    id if id.starts_with("recent-") => {
                        let index = id
                            .trim_start_matches("recent-")
                            .parse::<usize>()
                            .unwrap_or(99);
                        let s = app.state::<Arc<Service>>().inner().clone();
                        let selected = s.data.lock().unwrap().recent_node_ids.get(index).cloned();
                        if let Some(id) = selected {
                            let _ = s.select(&id);
                        }
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
            let tray_state = state.clone();
            std::thread::spawn(move || {
                let recent_items = [recent0, recent1, recent2];
                while !tray_state.exit.load(Ordering::SeqCst) {
                    let snapshot = tray_state.snapshot();
                    let speed = |bytes: u64| {
                        if bytes >= 1_048_576 {
                            format!("{:.1} MiB/s", bytes as f64 / 1_048_576.0)
                        } else {
                            format!("{:.0} KiB/s", bytes as f64 / 1024.0)
                        }
                    };
                    let _ = status.set_text(format!(
                        "↑ {}  ↓ {}",
                        speed(snapshot.traffic.upload_speed),
                        speed(snapshot.traffic.download_speed)
                    ));
                    let active = snapshot
                        .data
                        .active_node_id
                        .as_ref()
                        .and_then(|id| snapshot.data.nodes.iter().find(|node| &node.id == id));
                    let _ = current.set_text(format!(
                        "当前节点：{}",
                        active.map(|node| node.name.as_str()).unwrap_or("未选择")
                    ));
                    let connected = snapshot.connection.status == "connected";
                    let _ = connect.set_text(if connected { "断开连接" } else { "连接" });
                    let _ = connect.set_enabled(
                        active.is_some() || snapshot.data.settings.proxy_mode == "direct",
                    );
                    let _ = update.set_enabled(
                        !snapshot.data.subscriptions.is_empty() && !snapshot.job.running,
                    );
                    let _ = test.set_enabled(active.is_some() && !snapshot.job.running);
                    for (index, item) in recent_items.iter().enumerate() {
                        let node =
                            snapshot.data.recent_node_ids.get(index).and_then(|id| {
                                snapshot.data.nodes.iter().find(|node| &node.id == id)
                            });
                        let _ = item.set_text(format!(
                            "最近：{}",
                            node.map(|node| node.name.as_str()).unwrap_or("—")
                        ));
                        let _ =
                            item.set_enabled(node.is_some() && !connected && !snapshot.job.running);
                    }
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            });
            let settings = state.data.lock().unwrap().settings.clone();
            state.background();
            app.manage(state.clone());
            if !isolated {
                let startup_subscription_ids: Vec<String> = state
                    .data
                    .lock()
                    .unwrap()
                    .subscriptions
                    .iter()
                    .map(|subscription| subscription.id.clone())
                    .collect();
                if (settings.update_subscriptions_on_launch && !startup_subscription_ids.is_empty())
                    || settings.auto_connect
                    || (settings.auto_check_updates && !settings.update_repo.is_empty())
                {
                    let startup_total = startup_subscription_ids.len()
                        + usize::from(settings.auto_connect)
                        + usize::from(
                            settings.auto_check_updates && !settings.update_repo.is_empty(),
                        );
                    let _ = state.start_job("startup", startup_total, move |s| {
                        let mut messages = Vec::new();
                        if settings.update_subscriptions_on_launch
                            && !startup_subscription_ids.is_empty()
                        {
                            match s.update_subscriptions(startup_subscription_ids) {
                                Ok(message) => messages.push(message),
                                Err(error) => {
                                    s.log("ERROR", "startup", &error);
                                    messages.push(error);
                                }
                            }
                        }
                        if settings.auto_connect {
                            s.cancelled()?;
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
