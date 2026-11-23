use std::time::Duration;

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_updater::UpdaterExt;

#[derive(Default)]
pub struct UpdateCheckLock(tokio::sync::Mutex<()>);

pub fn start_periodic_checks(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            check(&app, false).await;
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
        }
    });
}

pub fn start_check(app: AppHandle, manual: bool) {
    tauri::async_runtime::spawn(async move { check(&app, manual).await });
}

async fn check(app: &AppHandle, manual: bool) {
    let state = app.state::<UpdateCheckLock>();
    let Ok(_guard) = state.0.try_lock() else {
        return;
    };
    if option_env!("QUOTAFLOAT_UPDATER_PUBLIC_KEY")
        .unwrap_or("")
        .trim()
        .is_empty()
    {
        if manual {
            app.dialog()
                .message(
                    "This build has no update signing key configured. / 此版本未配置更新签名公钥。",
                )
                .title("QuotaPulse")
                .show(|_| {});
        }
        return;
    }

    let mut installing = false;
    let result = async {
        let updater = app
            .updater_builder()
            .timeout(Duration::from_secs(30))
            .build()?;
        let Some(update) = updater.check().await? else {
            if manual {
                app.dialog()
                    .message("You are up to date. / 已是最新版本。")
                    .title("QuotaPulse")
                    .show(|_| {});
            }
            return Ok::<(), tauri_plugin_updater::Error>(());
        };
        let (sender, receiver) = tokio::sync::oneshot::channel();
        app.dialog()
            .message(format!(
                "QuotaPulse {} is available. Install it now?\n发现新版本 {}，现在安装吗？",
                update.version, update.version
            ))
            .title("QuotaPulse")
            .buttons(MessageDialogButtons::OkCancel)
            .show(move |confirmed| {
                let _ = sender.send(confirmed);
            });
        if receiver.await.unwrap_or(false) {
            installing = true;
            update.download_and_install(|_, _| {}, || {}).await?;
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        eprintln!("update check or installation failed: {error}");
        if manual || installing {
            app.dialog().message("Unable to check or install updates. Please check your connection and try again. / 无法检查或安装更新，请检查网络后重试。").title("QuotaPulse").show(|_| {});
        }
    }
}
