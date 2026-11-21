//! 「选择目标资源库」对话框：来源条目「下载到其他资源库」有几个候选时弹出。
//! 宽 `min(560px, 92vw)`；候选是一张 NanaUI 列表（上下方向键在卡片间移焦点），每个候选一张卡片，名称下面一行路径，都按省略号收尾；
//! 底栏只有「取消」。点卡片开始下载，取消、点外面和 Escape 不下载（`shell/admin/source_target.rs`）。
//!
//! 卡片按内容高画，名称和路径不叠在一起；悬停描强调色，和别的悬停行一致。

use std::sync::Arc;

use nana_ui::ButtonKind;
use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{
    Activate, AlignSpec, LengthSpec, List, ListItem, NodeStyle, RadiusTier, RovingFocusGroup,
    SemanticColorRole, SemanticPaint, Stack, Text,
};
use nana_ui_core::{DialogSize, SpacingStep, TextWeight, TypeRole};

use super::super::view_part_overlay::dialog::{DialogFrame, action, footer};
use super::super::{ShellMessage, ShellViewModel};
use super::AdminMessage;
use super::source_target::TargetCandidate;
use crate::theme_map::tiers::{self, Edge};

/// 对话框宽 `min(560px, 92vw)`。
const PICKER_DIALOG: DialogSize = DialogSize::capped(560.0, 92.0);

fn cancel() -> ShellMessage {
    ShellMessage::Admin(AdminMessage::CancelDownloadRepository)
}

/// 选择目标资源库。没有等着选目标的下载时不占位。候选在打开时定下，打开期间不变（浮层身份带着候选，变了整块换）。
pub(crate) fn target_dialog(model: &ShellViewModel) -> Option<AnyView> {
    let pick = model.admin.download_target.as_ref()?;
    let cards = pick
        .candidates
        .iter()
        .map(candidate_card)
        .collect::<Vec<_>>();
    let list = List::new().label("候选资源库").style(
        Stack::column(0.0)
            .gap_step(SpacingStep::Lg)
            .width(LengthSpec::Fill)
            .node_style(),
    );
    let body = widget(list)
        .roving_focus(RovingFocusGroup::vertical())
        .children(cards)
        .key("download-target-list");
    let buttons = vec![action(
        "取消",
        ButtonKind::Ghost,
        false,
        "download-target-cancel",
        cancel,
    )];
    Some(
        DialogFrame::new(
            "download-target-dialog",
            || "选择目标资源库".to_string(),
            cancel,
        )
        .size(PICKER_DIALOG)
        .dialog(body, footer(None, buttons)),
    )
}

/// 一个候选：内边距 `Xl`、`--radius-lg`、`--bg` 底、`--border-soft` 发丝边，悬停换底色、
/// 描强调色；名称 14px 600，路径 12px 弱色，都是单行省略。点它发选中这个资源库。
fn candidate_card(candidate: &TargetCandidate) -> AnyView {
    let mut style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.direction = Some(nana_ui_core::FlexDirection::Column);
        layout.align_items = AlignSpec::Stretch;
        tiers::gap(layout, SpacingStep::Xs);
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        tiers::padding(layout, &Edge::ALL, SpacingStep::Xl);
        tiers::border(layout, 1);
    }
    style.radius = Some(RadiusTier::Lg);
    style.background = Some(SemanticColorRole::Background);
    style.border = Some(SemanticColorRole::BorderSoft);
    style.foreground = Some(SemanticColorRole::Text);
    style.interaction.hovered = SemanticPaint {
        background: Some(SemanticColorRole::Hover),
        border: Some(SemanticColorRole::Accent),
        ..SemanticPaint::default()
    };
    style.interaction.pressed = style.interaction.hovered;
    let content = widget(
        Stack::column(0.0)
            .gap_step(SpacingStep::Xs)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .children((
        widget(line(
            &candidate.name,
            TypeRole::Section,
            TextWeight::Semibold,
            SemanticColorRole::Text,
        )),
        widget(line(
            &candidate.path,
            TypeRole::Meta,
            TextWeight::Regular,
            SemanticColorRole::Muted,
        )),
    ));
    let repo_id = candidate.repo_id.clone();
    widget(ListItem::new(format!("下载到 {}", candidate.name)).style(style))
        .content(content)
        .key(format!(
            "download-target-{}",
            super::style::key_part(&candidate.repo_id)
        ))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(ShellMessage::Admin(AdminMessage::ChooseDownloadRepository(
                repo_id.clone(),
            )));
        })
        .into_any()
}

/// 单行、超出按省略号收尾的一段字。字号和行盒按主题的字号档。
fn line(value: &str, role: TypeRole, weight: TextWeight, color: SemanticColorRole) -> Text {
    let mut node = Text::new(value.to_string())
        .typography(role)
        .weight(weight)
        .color(color)
        .truncating();
    let layout = Arc::make_mut(&mut node.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    node
}
