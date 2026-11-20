//! Office 转换设置块的视图：面板、标题、操作按钮、缓存区、仓库选择与状态行。

use super::*;

/// 设置区里的状态和缓存清理。不是 Office 转换时整块不占布局。
pub(in super::super) fn settings_block(status: Signal<Option<OfficeConvertView>>) -> AnyView {
    let shown = status;
    nana_ui::runtime::view::dynamic(status, move |view: &Option<OfficeConvertView>| {
        view.as_ref()
            .map(panel)
            .unwrap_or_else(|| widget(column(0.0)).into_any())
    })
    .visible(move || shown.with(Option::is_some))
    .key("admin-office-status")
    .into_any()
}

/// 整张设置卡：运行状态在上，资源库缓存清理在下，通用配置字段仍在这块下面。
pub(super) fn panel(view: &OfficeConvertView) -> AnyView {
    let mut body = vec![head(view.loading)];
    if !view.error.is_empty() {
        body.push(note(&view.error, "admin-office-error"));
    }
    if !view.message.is_empty() {
        body.push(note(&view.message, "admin-office-message"));
    }
    body.push(note(CONFIG_NOTE, "admin-office-config"));
    if view.rows.is_empty() {
        body.push(note(WAITING, "admin-office-waiting"));
    } else {
        body.push(rows(&view.rows, "status"));
    }
    body.push(actions(view));
    body.push(cache_section(view));
    let fill = style::SoftFill::new(
        style::Soft::RoleMix {
            role: Role::AccentSoft,
            ratio: 0.42,
            base: Role::Subtle,
        },
        Some(RadiusTier::Md),
    )
    .bordered(Role::BorderSoft, 1.0);
    widget(
        pad(column(10.0), 10.0, 10.0, 10.0, 10.0)
            .painter(fill)
            .with_layout(|layout| {
                layout.width = Some(LengthSpec::Fill);
            }),
    )
    .children(body)
    .key("admin-office-card")
    .into_any()
}

/// 页头：眉题、标题和刷新。读取中按钮写「刷新中」并禁用。
pub(super) fn head(loading: bool) -> AnyView {
    widget(style::spread(10.0, AlignSpec::Start))
        .children((
            widget(
                column(0.0)
                    .width(LengthSpec::Shrink)
                    .min_width(LengthSpec::Px(0.0))
                    .grow(1.0)
                    .shrink(1.0),
            )
            .children((
                widget(style::eyebrow_text("Office Convert")).key("admin-office-eyebrow"),
                widget(pad(column(0.0), 2.0, 0.0, 0.0, 0.0)).children((widget(style::label_lh(
                    TITLE,
                    14.0,
                    700,
                    Role::Text,
                    1.25,
                ))
                .key("admin-office-title"),)),
            )),
            widget(action(
                if loading { "刷新中" } else { "刷新" },
                None,
                Tone::Plain,
                loading,
            ))
            .prop::<bool, ActionDisabled>(loading)
            .key("admin-office-refresh")
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::RefreshOfficeStatus));
            }),
        ))
        .key("admin-office-head")
        .into_any()
}

pub(super) fn actions(view: &OfficeConvertView) -> AnyView {
    let shutdown_disabled = view.stopping || !view.daemon_running;
    widget(row(8.0).align(AlignSpec::Center).wrap(true))
        .children((
            widget(action(
                if view.self_checking {
                    "自检中"
                } else {
                    "运行自检"
                },
                None,
                Tone::Plain,
                view.self_checking,
            ))
            .prop::<bool, ActionDisabled>(view.self_checking)
            .key("admin-office-self-check")
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::RunOfficeSelfCheck));
            }),
            widget(action(
                if view.stopping {
                    "关闭中"
                } else {
                    "关闭守护进程"
                },
                None,
                Tone::Plain,
                shutdown_disabled,
            ))
            .prop::<bool, ActionDisabled>(shutdown_disabled)
            .key("admin-office-shutdown")
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ShutdownOfficeDaemon));
            }),
        ))
        .key("admin-office-actions")
        .into_any()
}

/// 按资源库清理预览缓存。下拉框只列宿主已经拿到的资源库。
pub(super) fn cache_section(view: &OfficeConvertView) -> AnyView {
    let clear_disabled = view.clearing || view.selected_repo_id.is_empty();
    widget(pad(column(8.0), 12.0, 0.0, 0.0, 0.0).width(LengthSpec::Fill))
        .children((
            widget(column(0.0)).children((
                widget(style::eyebrow_text("Preview Cache")).key("admin-office-cache-eyebrow"),
                widget(pad(column(0.0), 2.0, 0.0, 0.0, 0.0)).children((widget(style::label_lh(
                    CACHE_TITLE,
                    14.0,
                    700,
                    Role::Text,
                    1.25,
                ))
                .key("admin-office-cache-title"),)),
            )),
            rows(
                &[("当前资源库".into(), view.selected_repo_name.clone())],
                "repo",
            ),
            widget(row(8.0).align(AlignSpec::Center).width(LengthSpec::Fill)).children((
                repo_select(view),
                clear_button(view.clearing, clear_disabled),
            )),
            note(CACHE_NOTE, "admin-office-cache-note"),
        ))
        .key("admin-office-cache")
        .into_any()
}

pub(super) fn clear_button(clearing: bool, disabled: bool) -> AnyView {
    widget(action(
        if clearing {
            "清理中"
        } else {
            "清理缓存"
        },
        None,
        Tone::Plain,
        disabled,
    ))
    .prop::<bool, ActionDisabled>(disabled)
    .key("admin-office-clear")
    .on_cx(move |_, _: &Activate, cx| {
        cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ClearOfficeCache));
    })
    .into_any()
}

pub(super) fn repo_select(view: &OfficeConvertView) -> AnyView {
    let options = repo_options(&view.repos);
    let selected = view.selected_repo_id.clone();
    let mut select = Select::new(Some(selected.clone())).options(options.clone());
    {
        let layout = Arc::make_mut(&mut select.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.height = Some(LengthSpec::Px(32.0));
        layout.font_size = Some(14.0);
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
    }
    let value = selected.clone();
    widget(select)
        .prop::<Vec<SelectOption>, fields::select::options>(move || options.clone())
        .prop::<Option<Arc<str>>, fields::select::value>(move || Some(Arc::from(value.as_str())))
        .key("admin-office-repo")
        .on_cx(move |_, event: &SelectChanged, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::SelectOfficeRepository(
                event.value.to_string(),
            )));
        })
        .into_any()
}

pub(super) fn repo_options(repos: &[(String, String)]) -> Vec<SelectOption> {
    let mut options = vec![SelectOption::new("", "选择资源库")];
    options.extend(
        repos
            .iter()
            .map(|(id, name)| SelectOption::new(id.clone(), name.clone())),
    );
    options
}

/// 警告色说明。错误、结果、空状态和页底说明都用警告色浅底。
pub(super) fn note(text: &str, key: &'static str) -> AnyView {
    let fill = style::SoftFill::new(
        style::Soft::Alpha {
            role: Role::Warning,
            alpha: 0.12,
        },
        Some(RadiusTier::Sm),
    );
    widget(
        pad(column(0.0), 8.0, 10.0, 8.0, 10.0)
            .painter(fill)
            .with_layout(|layout| {
                layout.width = Some(LengthSpec::Fill);
            }),
    )
    .children((widget(style::wrapping(style::label_lh(
        text,
        12.0,
        400,
        Role::Warning,
        1.45,
    )))
    .key(key),))
    .into_any()
}

/// 状态行：上为 11 号大写弱标签，下为右对齐的值。第一行没有上边线。
pub(super) fn rows(items: &[(String, String)], prefix: &str) -> AnyView {
    let lines = items
        .iter()
        .enumerate()
        .map(|(index, (name, value))| {
            let mut line = column(6.0).width(LengthSpec::Fill);
            if index > 0 {
                line = style::top_rule(line)
                    .with_layout(|layout| layout.padding_top = Some(LengthSpec::Px(12.0)));
            }
            widget(line)
                .children((
                    widget(field_label(name)).key(format!("admin-office-{prefix}-label-{index}")),
                    widget(value_text(value)).key(format!("admin-office-{prefix}-value-{index}")),
                ))
                .key(format!("admin-office-{prefix}-row-{index}"))
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(column(8.0).width(LengthSpec::Fill))
        .children(lines)
        .key(format!("admin-office-{prefix}-rows"))
        .into_any()
}

pub(super) fn field_label(text: &str) -> nana_ui::runtime::Text {
    let mut text = label(text.to_uppercase(), 11.0, 700, Role::Faint);
    Arc::make_mut(&mut text.style.layout)
        .text_ext
        .letter_spacing = Some(0.4);
    text
}

pub(super) fn value_text(text: &str) -> nana_ui::runtime::Text {
    style::align_end(style::wrapping(label(text, 13.0, 400, Role::Text)))
}
