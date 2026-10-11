//! 设置页的外观主题和插件目录：主题能回到「跟随系统」，保存和读取设置文件的结果晚到时不把刚改的主题
//! 改回去；插件「打开目录」等真的打开以后才说已打开，打不开时写在插件面板上。

use crate::settings::ApplicationSettings;
use crate::shell::admin::{AdminEffect, AdminMessage};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellPage, ShellViewModel};

/// 取走排下的副作用，数一数里面有几次写设置文件。
fn saves(model: &mut ShellViewModel) -> usize {
    model.admin.take_effects().into_iter().filter(|effect| matches!(effect, AdminEffect::SaveSettings)).count()
}

fn with_theme(theme: &str) -> ApplicationSettings {
    ApplicationSettings { theme: theme.into(), ..ApplicationSettings::default() }
}

/// 外观卡有「跟随系统」一档：点过暗色、浅色以后还能回到跟随系统。
#[test]
fn the_theme_can_go_back_to_following_the_system() {
    let mut harness = ShellHarness::mount(ShellViewModel::for_page(ShellPage::Settings));
    assert_eq!(harness.model.settings.theme, "system", "默认跟随系统");
    harness.click_label("暗色");
    assert_eq!(harness.model.settings.theme, "dark");
    harness.click_label("浅色");
    assert_eq!(harness.model.settings.theme, "light");
    harness.click_label("跟随系统");
    assert_eq!(harness.model.settings.theme, "system", "点过暗色、浅色以后回不到跟随系统");
    harness.assert_same_as_fresh_mount();
}

/// 连着切两次主题：第二次等第一次写完再写，第一次的结果晚到也不把主题改回去。
#[test]
fn a_late_save_result_does_not_revert_a_newer_theme() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::SettingsThemeChanged("light".into()));
    assert_eq!(saves(&mut model), 1);
    model.reduce(ShellMessage::SettingsThemeChanged("dark".into()));
    assert_eq!(saves(&mut model), 0, "上一次还在写，等它写完再写");
    model.reduce(ShellMessage::SettingsSaved(Ok(with_theme("light"))));
    assert_eq!(model.settings.theme, "dark", "晚到的保存结果把主题改回去了");
    assert_eq!(saves(&mut model), 1, "写完以后接着写最新的");
    model.reduce(ShellMessage::SettingsSaved(Ok(with_theme("dark"))));
    assert_eq!(saves(&mut model), 0);
    assert_eq!(model.settings.theme, "dark");
}

/// 打开设置页时读设置文件；读回来之前改了主题，读到的旧内容不盖掉刚改的。读的时候没改过就照文件换上。
#[test]
fn a_settings_read_from_before_a_change_does_not_overwrite_it() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::OpenSettings);
    assert!(model.admin.take_effects().iter().any(|effect| matches!(effect, AdminEffect::LoadAppSettings)));
    model.reduce(ShellMessage::SettingsThemeChanged("dark".into()));
    model.reduce(ShellMessage::SettingsLoaded(Ok((with_theme("light"), None))));
    assert_eq!(model.settings.theme, "dark", "读到的旧设置把刚改的主题盖掉了");

    model.reduce(ShellMessage::SettingsSaved(Ok(with_theme("dark"))));
    model.reduce(ShellMessage::OpenSettings);
    model.reduce(ShellMessage::SettingsLoaded(Ok((with_theme("light"), None))));
    assert_eq!(model.settings.theme, "light", "读的这段时间没改过，照文件换上");
}

/// 插件「打开目录」：查到目录时还没打开，不先说已打开；打不开时原因写在插件面板上，不进状态区；
/// 打开了才写「已打开…设置目录。」（Vue `openPluginDataDirectoryInWorkspace` 等 `openRepositoryPath`）。
#[test]
fn the_data_directory_notice_waits_for_the_open() {
    let mut model = ShellViewModel::default();
    let finished = || ShellMessage::Admin(AdminMessage::DataDirectoryFinished { name: "user.one".into(), result: Ok("C:/plugins/user.one".into()) });
    model.reduce(ShellMessage::Admin(AdminMessage::OpenDataDirectory("user.one".into())));
    model.reduce(finished());
    assert!(model.admin.action_message.is_empty(), "还没打开就说已打开：{}", model.admin.action_message);
    crate::host_bridge::perform(&mut model, |_, _| Err("路径不存在".into()), |_| Ok(()), || Ok(()));
    assert_eq!(model.admin.action_error, "插件设置目录打开失败：路径不存在");
    assert!(model.admin.action_message.is_empty());
    assert!(model.status.failure().is_none(), "插件面板上已经写了，不再进状态区");

    model.reduce(ShellMessage::Admin(AdminMessage::OpenDataDirectory("user.one".into())));
    model.reduce(finished());
    let mut opened = Vec::new();
    crate::host_bridge::perform(
        &mut model,
        |target, reveal| {
            opened.push((target.to_string(), reveal));
            Ok(())
        },
        |_| Ok(()),
        || Ok(()),
    );
    assert_eq!(opened, [("C:/plugins/user.one".to_string(), true)]);
    assert_eq!(model.admin.action_message, "已打开“user.one”设置目录。");
    assert!(model.admin.action_error.is_empty());
}
