//! Office 转换设置的测试：打开时读取状态、仓库选择、状态行、失败保留、自检关闭清理调用与重复刷新。

use super::*;
use crate::backend::services::repository::RepositoryBackendSummary;
use serde_json::json;

fn open(model: &mut ShellViewModel) {
    model.reduce(ShellMessage::Admin(AdminMessage::ToggleSettings(
        PLUGIN_ID.into(),
    )));
}

fn repo(id: &str, name: &str) -> RepositorySummary {
    RepositorySummary {
        repo_id: id.into(),
        name: name.into(),
        path: format!("C:/{id}"),
        backend: RepositoryBackendSummary {
            plugin_id: "momobako.local-filesystem".into(),
            kind: "filesystem".into(),
            name: "本地文件系统".into(),
            capabilities: vec!["read".into()],
        },
        status: "ready".into(),
        asset_count: 0,
        updated_at: "2026-10-08T08:00:00Z".into(),
        local_cache: None,
        authentication: None,
    }
}

fn status_calls(model: &mut ShellViewModel) -> usize {
    model
        .admin
        .take_effects()
        .into_iter()
        .filter(|effect| {
            matches!(
                effect,
                AdminEffect::CallPlugin {
                    origin: PluginCallOrigin::OfficeConvert(OfficeCall::Status),
                    ..
                }
            )
        })
        .count()
}

#[test]
fn opening_requests_status_without_inventing_rows() {
    let mut model = ShellViewModel::default();
    open(&mut model);
    let effects = model.admin.take_effects();
    assert!(effects.iter().any(|effect| matches!(
        effect,
        AdminEffect::CallPlugin { plugin_id, method, payload, repository_id, origin }
            if plugin_id == PLUGIN_ID
                && method == "officeConvert.getRuntimeStatus"
                && payload == &json!({})
                && repository_id.is_none()
                && *origin == PluginCallOrigin::OfficeConvert(OfficeCall::Status)
    )));
    let view = OfficeConvertView::project(&model);
    assert!(view.loading);
    assert!(view.rows.is_empty(), "还没有状态，不画守护进程");
    assert_eq!(view.selected_repo_name, NO_REPO);
    assert!(!view.daemon_running);
    assert!(view.error.is_empty());
}

#[test]
fn known_repositories_are_selectable_and_other_plugins_do_not_request() {
    let mut model = ShellViewModel::default();
    model.admin.note_backends(&[repo("repo-a", "默认资源库")]);
    open(&mut model);
    assert_eq!(status_calls(&mut model), 1);
    let view = OfficeConvertView::project(&model);
    assert_eq!(view.selected_repo_id, "repo-a");
    assert_eq!(view.selected_repo_name, "默认资源库");
    assert_eq!(view.repos, vec![("repo-a".into(), "默认资源库".into())]);

    let mut other = ShellViewModel::default();
    other.reduce(ShellMessage::Admin(AdminMessage::ToggleSettings(
        "momobako.local-filesystem".into(),
    )));
    assert!(other.admin.take_effects().iter().all(|effect| !matches!(
        effect,
        AdminEffect::CallPlugin {
            origin: PluginCallOrigin::OfficeConvert(_),
            ..
        }
    )));
}

#[test]
fn status_rows_use_the_payload_and_missing_fields_use_fallbacks() {
    let mut model = ShellViewModel::default();
    open(&mut model);
    model.admin.take_effects();
    model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
        call: OfficeCall::Status,
        result: Ok(json!({
            "converterMode": "libreoffice",
            "autoDownloadLibreOffice": true,
            "microsoftOffice": { "available": false, "reason": "未探测到系统 Microsoft Office 安装。" },
            "libreofficeSystem": { "available": true, "path": "C:/LibreOffice/program/soffice.exe", "version": "25.8.3" },
            "libreofficeBundle": { "available": false, "reason": "未发现已下载的 LibreOffice 运行时。" },
            "bundledDownloadUrl": "https://example.invalid/lo.msi",
            "daemon": {
                "running": true,
                "pid": 99,
                "sofficeReady": true,
                "updatedAt": "2026-10-13T00:00:00Z",
                "helperType": "bundled",
                "port": 18181,
                "baseUrl": "http://127.0.0.1:18181",
                "healthy": true,
                "sofficePid": 100,
                "unoAvailable": true,
                "pythonValid": false,
                "pythonPath": "C:/lo/python.exe",
                "control": { "health": "http-local", "shutdown": "plugin-call" },
                "lastConvert": { "phase": "completed", "conversionMode": "uno", "sourcePath": "a.docx", "updatedAt": "2026-10-13T00:01:00Z" },
                "lastSelfCheck": {
                    "ok": true,
                    "converter": "LibreOffice",
                    "converterVersion": "25.8.3",
                    "conversionMode": "uno",
                    "pdfSizeBytes": 1200,
                    "durationMs": 40,
                    "completedAt": "2026-10-13T00:02:00Z",
                    "samplePath": "C:/tmp/self-check.docx",
                    "pdfPath": "C:/tmp/self-check.pdf",
                    "converterPath": "C:/lo/soffice.exe"
                }
            }
        })),
    }));
    let rows = OfficeConvertView::project(&model).rows;
    assert_eq!(
        rows,
        vec![
            ("模式".into(), "libreoffice".into()),
            ("自动下载".into(), "开启".into()),
            (
                "Microsoft Office".into(),
                "未探测到系统 Microsoft Office 安装。".into()
            ),
            (
                "系统 LibreOffice".into(),
                "C:/LibreOffice/program/soffice.exe | 25.8.3".into()
            ),
            (
                "自带 LibreOffice".into(),
                "未发现已下载的 LibreOffice 运行时。".into()
            ),
            (
                "自带下载地址".into(),
                "https://example.invalid/lo.msi".into()
            ),
            (
                "守护进程".into(),
                "PID 99 | Soffice 已就绪 | 更新于 2026-10-13T00:00:00Z".into()
            ),
            ("Helper 类型".into(), "bundled".into()),
            ("Helper 端口".into(), "18181".into()),
            ("Helper 地址".into(), "http://127.0.0.1:18181".into()),
            ("健康检查".into(), "通过".into()),
            ("Soffice 就绪".into(), "已就绪".into()),
            ("Soffice PID".into(), "100".into()),
            ("UNO 可用".into(), "是".into()),
            ("Python 有效".into(), "否".into()),
            ("Python 路径".into(), "C:/lo/python.exe".into()),
            (
                "控制方式".into(),
                "health=http-local | shutdown=plugin-call".into()
            ),
            (
                "最近转换".into(),
                "completed | uno | a.docx | 2026-10-13T00:01:00Z".into()
            ),
            (
                "最近自检".into(),
                "通过 | LibreOffice | 25.8.3 | uno | 1200 bytes | 40 ms | 2026-10-13T00:02:00Z"
                    .into()
            ),
            ("自检样本".into(), "C:/tmp/self-check.docx".into()),
            ("自检输出".into(), "C:/tmp/self-check.pdf".into()),
            (
                "自检转换器".into(),
                "LibreOffice | 25.8.3 | C:/lo/soffice.exe".into()
            ),
            ("自检错误".into(), "无".into()),
        ]
    );
    assert!(OfficeConvertView::project(&model).daemon_running);

    model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
        call: OfficeCall::Status,
        result: Ok(json!({})),
    }));
    let sparse = OfficeConvertView::project(&model).rows;
    assert_eq!(
        sparse
            .iter()
            .find(|(name, _)| name == "守护进程")
            .map(|(_, value)| value.as_str()),
        Some("未知")
    );
    assert_eq!(
        sparse
            .iter()
            .find(|(name, _)| name == "Helper 端口")
            .map(|(_, value)| value.as_str()),
        Some("未分配")
    );
    assert_eq!(
        sparse
            .iter()
            .find(|(name, _)| name == "最近自检")
            .map(|(_, value)| value.as_str()),
        Some("暂无记录")
    );
    assert!(
        sparse.iter().all(|(_, value)| !value.contains("PID")),
        "空状态对象不编进程号"
    );
}

#[test]
fn failure_keeps_the_previous_status_and_closing_drops_a_late_result() {
    let mut model = ShellViewModel::default();
    open(&mut model);
    model.admin.take_effects();
    model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
        call: OfficeCall::Status,
        result: Ok(json!({ "converterMode": "auto" })),
    }));
    model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
        call: OfficeCall::Status,
        result: Err("转换服务没有响应".into()),
    }));
    assert_eq!(model.admin.office.error, "转换服务没有响应");
    assert!(model.admin.office.status.is_some(), "失败保留上一次的状态");
    assert!(!model.admin.office.loading);

    open(&mut model);
    model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
        call: OfficeCall::Status,
        result: Ok(json!({ "converterMode": "libreoffice" })),
    }));
    assert!(model.admin.office.status.is_none());
    assert!(!model.admin.office.loading);
}

#[test]
fn self_check_shutdown_and_clear_call_the_plugin_methods() {
    let mut model = ShellViewModel::default();
    model.admin.note_backends(&[repo("repo-a", "默认资源库")]);
    open(&mut model);
    model.admin.take_effects();
    model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
        call: OfficeCall::Status,
        result: Ok(json!({ "daemon": { "running": true, "pid": 7 } })),
    }));

    model.reduce(ShellMessage::Admin(AdminMessage::RunOfficeSelfCheck));
    assert!(model.admin.take_effects().iter().any(|effect| matches!(
        effect,
        AdminEffect::CallPlugin { method, payload, origin, .. }
            if method == "officeConvert.runRuntimeSelfCheck" && payload == &json!({}) && *origin == PluginCallOrigin::OfficeConvert(OfficeCall::SelfCheck)
    )));
    model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
        call: OfficeCall::SelfCheck,
        result: Ok(json!({ "ok": true })),
    }));
    assert_eq!(model.admin.office.message, "运行时自检通过");
    assert_eq!(status_calls(&mut model), 1, "自检成功后再读一次状态");
    assert_eq!(model.admin.office.message, "运行时自检通过");

    model.reduce(ShellMessage::Admin(AdminMessage::ShutdownOfficeDaemon));
    assert!(model.admin.take_effects().iter().any(|effect| matches!(
        effect,
        AdminEffect::CallPlugin { method, origin, .. }
            if method == "officeConvert.shutdownDaemon" && *origin == PluginCallOrigin::OfficeConvert(OfficeCall::Shutdown)
    )));

    model.reduce(ShellMessage::Admin(AdminMessage::ClearOfficeCache));
    assert!(model.admin.take_effects().iter().any(|effect| matches!(
        effect,
        AdminEffect::CallPlugin { method, payload, origin, .. }
            if method == "officeConvert.clearPreviewCache"
                && payload == &json!({ "repoId": "repo-a" })
                && *origin == PluginCallOrigin::OfficeConvert(OfficeCall::ClearCache)
    )));
    model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
        call: OfficeCall::ClearCache,
        result: Ok(json!({ "repoId": "repo-a", "removed": 3 })),
    }));
    assert_eq!(model.admin.office.message, "已清理 3 个缓存文件");
}

#[test]
fn missing_daemon_missing_repo_and_call_errors_do_not_pretend_success() {
    let mut model = ShellViewModel::default();
    open(&mut model);
    model.admin.take_effects();
    model.reduce(ShellMessage::Admin(AdminMessage::ShutdownOfficeDaemon));
    assert!(
        model.admin.take_effects().is_empty(),
        "没有运行中的守护进程，不调用关闭"
    );
    assert!(model.admin.office.message.is_empty());

    model.reduce(ShellMessage::Admin(AdminMessage::ClearOfficeCache));
    assert!(
        model.admin.take_effects().is_empty(),
        "没有资源库，不调用清理"
    );
    assert!(model.admin.office.message.is_empty());

    model.reduce(ShellMessage::Admin(AdminMessage::RunOfficeSelfCheck));
    model.admin.take_effects();
    model.reduce(ShellMessage::Admin(AdminMessage::OfficeCallFinished {
        call: OfficeCall::SelfCheck,
        result: Err("soffice 没有响应".into()),
    }));
    assert_eq!(model.admin.office.error, "soffice 没有响应");
    assert!(model.admin.office.message.is_empty());
    assert!(!model.admin.office.self_checking);
    assert!(
        model.admin.take_effects().is_empty(),
        "自检失败不再读状态，也不写通过"
    );
}

#[test]
fn refresh_while_loading_does_not_request_twice() {
    let mut model = ShellViewModel::default();
    open(&mut model);
    assert_eq!(status_calls(&mut model), 1);
    model.reduce(ShellMessage::Admin(AdminMessage::RefreshOfficeStatus));
    assert!(model.admin.take_effects().is_empty());
}
