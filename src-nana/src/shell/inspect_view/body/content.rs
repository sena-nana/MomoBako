//! 内容引用的视图构建、预览内容区与文本插件视图。

use super::*;

impl ContentRef<'_> {
    /// 身份：画面读到的每样东西。大块像素和文本只认缓冲区地址和长度。
    pub(super) fn identity(&self, hasher: &mut DefaultHasher) {
        std::mem::discriminant(self).hash(hasher);
        match self {
            Self::Still { pixels, cover } => {
                pixels.identity(hasher);
                cover.hash(hasher);
            }
            Self::Text {
                markdown,
                text,
                truncated_at,
                extension,
            } => {
                (
                    markdown,
                    text.as_ptr() as usize,
                    text.len(),
                    truncated_at,
                    extension,
                )
                    .hash(hasher);
            }
            Self::ImageTexture | Self::Pending => {}
            Self::ImagePixels(pixels) => pixels.identity(hasher),
            Self::Video { cover } => cover.hash(hasher),
            Self::VideoPixels { pixels, cover } => {
                pixels.identity(hasher);
                cover.hash(hasher);
            }
            Self::Audio {
                name,
                viewport,
                lines,
                active,
                pending,
                caption,
                cover_path,
                accent,
            } => {
                (
                    name,
                    viewport.to_bits(),
                    active,
                    pending,
                    accent,
                    &caption.title,
                    &caption.subtitle,
                    &caption.album,
                    cover_path,
                )
                    .hash(hasher);
                if let Some(path) = cover_path {
                    super::super::audio_stage::cover_stamp(path).hash(hasher);
                }
                for line in lines.iter() {
                    line.text.hash(hasher);
                    line.time_ms.hash(hasher);
                }
            }
            Self::Archive {
                content,
                picked,
                stage_title,
                stage_detail,
                stage_failed,
                name,
                shown,
            } => {
                (
                    content,
                    picked,
                    stage_title,
                    stage_detail,
                    stage_failed,
                    name,
                )
                    .hash(hasher);
                shown.iter().for_each(|shown| shown.identity(hasher));
            }
            Self::Model {
                extension,
                raster,
                content,
                mesh,
                extent,
            } => {
                (extension, content, mesh, extent).hash(hasher);
                raster.iter().for_each(|raster| raster.identity(hasher));
            }
            Self::Document {
                toolbar,
                nav,
                page,
                error,
                text,
            } => {
                (toolbar, nav, error, text).hash(hasher);
                page.iter().for_each(|raster| raster.identity(hasher));
            }
            Self::Failed {
                title,
                message,
                toolbar,
                document,
                retry,
            } => (title, message, toolbar, document, retry).hash(hasher),
            Self::Loading(loading) => match loading {
                LoadingRef::Pill {
                    title,
                    detail,
                    progress,
                    toolbar,
                } => (0u8, title, detail, progress, toolbar).hash(hasher),
                LoadingRef::Archive { detail } => (1u8, detail).hash(hasher),
                LoadingRef::Media { name, video } => (2u8, name, video).hash(hasher),
            },
            Self::Fallback(fallback) => match fallback {
                FallbackRef::Texture(slot) => slot.hash(hasher),
                FallbackRef::Icon(icon) => (icon.as_ptr() as usize).hash(hasher),
            },
        }
    }

    /// 抄成自有的内容。只在身份变了时调用。
    pub(super) fn to_owned(self) -> Body {
        match self {
            Self::Still { pixels, cover } => Body::Still {
                pixels: pixels.to_owned(),
                cover,
            },
            Self::Text {
                markdown,
                text,
                truncated_at,
                extension,
            } => Body::Text {
                markdown,
                text: text.into(),
                truncated_at,
                extension: extension.to_string(),
            },
            Self::ImageTexture => Body::ImageTexture,
            Self::ImagePixels(pixels) => Body::ImagePixels(pixels.to_owned()),
            Self::Video { cover } => Body::Video { cover },
            Self::VideoPixels { pixels, cover } => Body::VideoPixels {
                pixels: pixels.to_owned(),
                cover,
            },
            Self::Audio {
                viewport,
                lines,
                active,
                pending,
                caption,
                cover_path,
                accent,
                ..
            } => Body::Audio {
                viewport,
                lines: lines.to_vec(),
                active,
                pending,
                caption,
                accent,
                cover: cover_path.and_then(super::super::audio_stage::cover_data_url),
            },
            Self::Archive {
                content,
                picked,
                stage_title,
                stage_detail,
                stage_failed,
                name,
                shown,
            } => Body::Archive {
                content: content.to_string(),
                picked: picked.map(str::to_string),
                stage_title: stage_title.to_string(),
                stage_detail: stage_detail.to_string(),
                stage_failed,
                name: name.to_string(),
                shown: shown.map(ArchiveShown::to_owned),
            },
            Self::Model {
                extension,
                raster,
                content,
                mesh,
                extent,
            } => Body::Model {
                extension: extension.to_string(),
                raster: raster.map(PixelsRef::to_owned),
                content: content.to_string(),
                mesh,
                extent: extent.to_string(),
            },
            Self::Document {
                toolbar,
                nav,
                page,
                error,
                text,
            } => Body::Document {
                toolbar,
                nav,
                page: page.map(PixelsRef::to_owned),
                error: error.map(str::to_string),
                text: text.to_string(),
            },
            Self::Failed {
                title,
                message,
                toolbar,
                document,
                retry,
            } => Body::Failed {
                title,
                message: message.to_string(),
                toolbar,
                document,
                retry,
            },
            Self::Loading(LoadingRef::Pill {
                title,
                detail,
                progress,
                toolbar,
            }) => Body::Pill {
                title,
                detail,
                progress,
                toolbar,
            },
            Self::Loading(LoadingRef::Archive { detail }) => Body::ArchiveLoading { detail },
            Self::Loading(LoadingRef::Media { name, video }) => Body::MediaLoading {
                name: name.to_string(),
                video,
            },
            Self::Fallback(FallbackRef::Texture(slot)) => Body::FallbackTexture(slot.to_string()),
            Self::Fallback(FallbackRef::Icon(icon)) => Body::FallbackIcon(icon),
            Self::Pending => Body::Pending,
        }
    }
}

/// 预览框里的内容。
pub(in super::super) fn content_view(content: &PreviewContent) -> AnyView {
    let scope = &content.scope;
    match &content.body {
        Body::Still { pixels, cover } => pixel_image(pixels, *cover, "inspect-player-still"),
        Body::Text {
            markdown,
            text,
            truncated_at,
            extension,
        } => text_plugin(*markdown, text, *truncated_at, extension, scope),
        Body::ImageTexture => {
            // 底色是 bg：留白处是 bg，不透出预览框的插件光晕。
            widget(Stack::fill_column(0.0).surface(SemanticColorRole::Background))
                .children((
                    widget(GpuTextureView::new("file-preview").contain()).key("inspect-image"),
                ))
                .key("inspect-image-frame")
                .into_any()
        }
        Body::ImagePixels(pixels) => pixel_image(pixels, false, "inspect-image"),
        Body::Video { cover } => host_video(*cover),
        Body::VideoPixels { pixels, cover } => {
            frame_pixels(pixels, *cover, "inspect-video-pixels", true)
        }
        Body::Audio {
            viewport,
            lines,
            active,
            pending,
            caption,
            cover,
            accent,
        } => {
            super::super::audio_stage::lyric_lift::note_accent(*accent);
            super::super::audio_stage::audio_stage(
                *viewport,
                lines,
                *active,
                caption,
                cover.as_deref(),
                *pending,
            )
        }
        Body::Archive {
            content,
            picked,
            stage_title,
            stage_detail,
            stage_failed,
            name,
            shown,
        } => {
            let opened = shown
                .as_ref()
                .map(|shown| archive_entry::view(shown, scope));
            inspect_library::archive_list(
                content,
                picked.as_deref(),
                inspect_library::ArchiveStage {
                    title: stage_title,
                    detail: stage_detail,
                    failed: *stage_failed,
                },
                name,
                opened,
            )
        }
        Body::Model {
            extension,
            raster,
            content,
            mesh,
            extent,
        } => model_view(extension, raster.as_ref(), content, *mesh, extent),
        Body::Document {
            toolbar,
            nav,
            page,
            error,
            text,
        } => document_view(
            toolbar.as_ref(),
            *nav,
            page.as_ref(),
            error.as_deref(),
            text,
            scope,
        ),
        Body::Failed {
            title,
            message,
            toolbar,
            document,
            retry,
        } => {
            let overlay = if *retry {
                retry_overlay(title, message)
            } else {
                overlay(title, message, "inspect-failed")
            };
            if !document {
                return overlay;
            }
            let mut rows = toolbar
                .iter()
                .map(inspect_library::document_toolbar_view)
                .collect::<Vec<_>>();
            rows.push(overlay);
            widget(Stack::fill_column(0.0)).children(rows).into_any()
        }
        Body::Pill {
            title,
            detail,
            progress,
            toolbar,
        } => status_layer(
            status_pill(title, detail, *progress),
            toolbar.as_ref().map(inspect_library::document_toolbar_view),
        ),
        Body::ArchiveLoading { detail } => centered_status("读取压缩包", detail),
        Body::MediaLoading { name, video } => super::super::audio_stage::loading_card(name, *video),
        Body::FallbackTexture(slot) => {
            let mut thumb = Thumbnail::new(super::super::super::thumbs::thumbnail_slot(slot))
                .fit(ContentFit::Contain);
            {
                let layout = std::sync::Arc::make_mut(&mut thumb.style.layout);
                layout.width = Some(LengthSpec::Fill);
                layout.height = Some(LengthSpec::Fill);
            }
            widget(Stack::fill_column(0.0).surface(SemanticColorRole::Background))
                .children((widget(thumb).key("inspect-thumbnail-texture"),))
                .key("inspect-thumbnail")
                .into_any()
        }
        Body::FallbackIcon(icon) => widget(
            Stack::fill_column(0.0)
                .align(AlignSpec::Center)
                .justify(JustifySpec::Center),
        )
        .children((widget(
            IconGlyph::new(*icon)
                .size(54.0)
                .role(SemanticColorRole::Faint),
        )
        .key("inspect-fallback-icon"),))
        .key("inspect-fallback")
        .into_any(),
        Body::Pending => widget(Stack::fill_column(0.0))
            .key("inspect-pending")
            .into_any(),
    }
}

/// 文本插件：上面一条种类和行数，下面是等宽原文。Markdown 也按原文显示。
pub(super) fn text_plugin(
    markdown: bool,
    source: &str,
    truncated_at: Option<u64>,
    extension: &str,
    scope: &str,
) -> AnyView {
    let extension = extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_uppercase();
    let kind = if markdown {
        "Markdown".to_string()
    } else if extension.is_empty() {
        "TEXT".to_string()
    } else {
        extension
    };
    let lines = if source.is_empty() {
        0
    } else {
        source
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .split('\n')
            .count()
    };
    let mut chips = vec![
        chip(&kind, true, "inspect-text-kind"),
        chip(&format!("{lines} 行"), false, "inspect-text-lines"),
    ];
    if let Some(read) = truncated_at {
        chips.push(chip(
            &format!("仅显示前 {}", format_byte_count(read)),
            false,
            "inspect-text-truncated",
        ));
    }
    let shown = if source.is_empty() {
        "空文件"
    } else {
        source
    };
    let mut body = Text::new(shown)
        .color(SemanticColorRole::Text)
        .role(nana_ui_core::TypeRole::Body);
    {
        let layout = std::sync::Arc::make_mut(&mut body.style.layout);
        // 等宽原文 13px、1.65 倍行高，按正文字号档的倍数写。
        crate::theme_map::tiers::line_ratio(layout, nana_ui_core::TypeRole::Body, 1.65);
        layout.text_ext.font_family = Some("monospace".into());
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.text_ext.line_break = Some(nana_ui_core::LineBreakSpec::Anywhere);
        layout.text_ext.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
        // `white-space: pre-wrap`：保留原文的换行和空格，长行在框内折行。
        layout.white_space = nana_ui_core::WhiteSpaceSpec::PreWrap;
    }
    // 全局 `pre`：bg-subtle 底、border-soft 发丝边、`Md` 圆角、上下 `Xl` 左右 `Xxl` 内边距，至少铺满内容区。
    let sheet = widget(
        super::super::super::files_view::style::hairline(
            Stack::column(0.0)
                .width(LengthSpec::Fill)
                .min_width(LengthSpec::Px(0.0))
                .surface(SemanticColorRole::Subtle),
            SemanticColorRole::BorderSoft,
        )
        .radius(RadiusTier::Md)
        .with_layout(|layout| {
            crate::theme_map::tiers::padding_xy(
                layout,
                nana_ui_core::SpacingStep::Xxl,
                nana_ui_core::SpacingStep::Xl,
            );
            layout.min_height = Some(LengthSpec::Percent(100.0));
            layout.flex_grow = Some(1.0);
        }),
    )
    .children((widget(body).key("inspect-text"),))
    .key("inspect-text-sheet");
    let content = widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
        crate::theme_map::tiers::padding_xy(
            layout,
            nana_ui_core::SpacingStep::Page,
            nana_ui_core::SpacingStep::PageTight,
        );
    }))
    .children((
        widget(Stack::fill_column(0.0).min_height(LengthSpec::Percent(100.0))).children((sheet,)),
    ))
    .key(format!("preview-text-{scope}"));
    // 插件根自带 bg 底，盖住预览框顶部的插件光晕。
    widget(
        Stack::fill_column(0.0)
            .min_height(LengthSpec::Px(0.0))
            .surface(SemanticColorRole::Background),
    )
    .children((toolbar(chips, "inspect-text-toolbar"), content.into_any()))
    .key("inspect-text-plugin")
    .into_any()
}
