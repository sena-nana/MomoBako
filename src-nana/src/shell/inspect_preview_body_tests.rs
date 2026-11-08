//! PDF 预览框的翻页：第一页画不出来时，原来只剩失败浮层、没有翻页，后面的页都看不到。现在失败的页也带翻页，
//! 「下一页」点得到，翻过去画第二页。

use crate::shell::inspect::{InspectMessage, PreviewBody, native_preview};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellPage, ShellViewModel};
use nana_ui::runtime::LengthSpec;
use nana_ui_core::{SemanticColorMix, SemanticColorRole, StyleModelRef, ThemeAppearance};

const PDF_VIEW: &str = "momobako.preview.pdf";
const PATH: &str = "notes/page.pdf";

/// 预览页打开着这份 PDF（照验收场景 `live-preview` 的做法装进解析结果）。
fn previewing(pdf: &[u8]) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::SelectedFile);
    let repo_id = model.workspace.active_repo_id.clone().unwrap_or_default();
    model.inspect.begin_selection(PATH);
    native_preview::begin(
        &mut model.inspect,
        &repo_id,
        PATH,
        PDF_VIEW.into(),
        "PDF".into(),
    );
    let loaded = native_preview::load(PDF_VIEW, pdf).expect("PDF 能读");
    let generation = model.inspect.generation;
    model.reduce(ShellMessage::Inspect(InspectMessage::NativeLoaded {
        path: PATH.into(),
        generation,
        result: Ok(loaded),
    }));
    model
}

/// 两页的 PDF：第一页的内容流标着 Flate 却不是压缩数据（解不开），第二页是 `(Second) Tj`。
const TWO_PAGES: &[u8] = b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>\nendobj\n\
4 0 obj\n<< /Length 4 /Filter /FlateDecode >>\nstream\nxxxx\nendstream\nendobj\n\
5 0 obj\n<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>\nendobj\n\
6 0 obj\n<< /Length 11 >>\nstream\n(Second) Tj\nendstream\nendobj\n\
trailer\n<< /Root 1 0 R >>\n%%EOF\n";

#[test]
fn a_failed_page_still_offers_the_next_page() {
    let pdf = TWO_PAGES;
    let mut harness = ShellHarness::mount(previewing(pdf));
    assert!(harness.find("无法预览该文档").is_some(), "第一页写失败原因");
    assert!(harness.find("1 / 2").is_some(), "失败的页也写页码");
    assert!(
        harness.keyed("inspect-native-page").is_none(),
        "第一页没有页纸"
    );

    harness.click_label("下一页");
    assert!(
        harness.keyed("inspect-native-page").is_some(),
        "翻到第二页，画出页纸"
    );
    assert!(harness.find("2 / 2").is_some());
    assert!(
        harness.find("无法预览该文档").is_none(),
        "第二页不再是失败浮层"
    );
}

const MODEL_VIEW: &str = "momobako.preview.model";

/// 一个三角形：x、y 跨度都是 1，z 是 0，尺寸按 Vue 收到 0.01。
const TRIANGLE_OBJ: &[u8] = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";

/// 打开模型预览并装上解析结果。扩展名写进文件事实，左下角标签才有字。
fn previewing_model(path: &str, extension: &str, bytes: &[u8]) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::SelectedFile);
    let repo_id = model.workspace.active_repo_id.clone().unwrap_or_default();
    model.inspect.begin_selection(path);
    model.inspect.facts.extension = extension.to_string();
    native_preview::begin(
        &mut model.inspect,
        &repo_id,
        path,
        MODEL_VIEW.into(),
        "模型".into(),
    );
    let loaded = native_preview::load(MODEL_VIEW, bytes).expect("模型能读");
    let generation = model.inspect.generation;
    model.reduce(ShellMessage::Inspect(InspectMessage::NativeLoaded {
        path: path.into(),
        generation,
        result: Ok(loaded),
    }));
    model
}

/// 有网格时左下角写出包围盒三轴尺寸；没有网格的 FBX 只留扩展名，不编尺寸。
#[test]
fn model_hud_shows_the_mesh_bounding_box() {
    let harness = ShellHarness::mount(previewing_model("models/tri.obj", "obj", TRIANGLE_OBJ));
    assert!(harness.find("OBJ").is_some(), "扩展名标签");
    assert!(harness.find("1.00 x 1.00 x 0.01").is_some(), "包围盒尺寸");
    assert!(
        harness.keyed("inspect-model-hud-size").is_some(),
        "尺寸标签在左下角"
    );

    let header = ShellHarness::mount(previewing_model(
        "models/cube.fbx",
        "fbx",
        b"; FBX 7.4\nModel: \"Cube\"",
    ));
    assert!(header.find("FBX").is_some(), "没有网格时仍写扩展名");
    assert!(
        header.keyed("inspect-model-hud-size").is_none(),
        "没有网格不编尺寸"
    );
    assert!(
        header.find("1.00 x 1.00 x 0.01").is_none(),
        "文件头预览不出现三角形的尺寸"
    );
}

/// 有包围盒时，左下角尺寸标签的底是 Vue `.model-preview__hud span` 的抬升表面 88%。
/// 没有网格就不画这枚标签，也不编 `1.00 x 1.00 x 0.01`。文案仍是三轴尺寸。
#[test]
fn model_size_label_uses_the_vue_fill() {
    let harness = ShellHarness::mount(previewing_model("models/tri.obj", "obj", TRIANGLE_OBJ));
    assert_eq!(
        chip_text(&harness, "inspect-model-hud-size"),
        "1.00 x 1.00 x 0.01"
    );
    assert_size_fill(&harness);

    let header = ShellHarness::mount(previewing_model(
        "models/cube.fbx",
        "fbx",
        b"; FBX 7.4\nModel: \"Cube\"",
    ));
    assert!(header.find("FBX").is_some(), "没有网格时仍写扩展名");
    assert!(
        header.keyed("inspect-model-hud-size").is_none(),
        "没有尺寸不画空标签"
    );
    assert!(
        header.find("1.00 x 1.00 x 0.01").is_none(),
        "没有尺寸不编数字"
    );
}

/// 尺寸标签只铺抬升表面的 88%，不再另铺不透明的 bg-elev。比例写死。
fn assert_size_fill(harness: &ShellHarness) {
    let style = node_style(harness, "inspect-model-hud-size");
    assert_eq!(style.background, None, "尺寸标签不能再铺不透明的 bg-elev");
    let mix = SemanticColorMix::alpha(SemanticColorRole::Surface, 0.88);
    assert_eq!(style.interaction.base.background_mix, Some(mix));
    assert_eq!(style.border, Some(SemanticColorRole::Border));
    for (name, mode) in [
        ("深色", ThemeAppearance::Dark),
        ("浅色", ThemeAppearance::Light),
    ] {
        let model = StyleModelRef::new(mode);
        let painted = mix.resolve(model).as_rgba_array();
        let elev = model.color(SemanticColorRole::Surface).as_rgba_array();
        assert!(
            (painted[3] - elev[3] * 0.88).abs() < 1e-4,
            "{name} 尺寸标签透明度应是抬升表面的 88%"
        );
        assert!(
            (painted[0] - elev[0]).abs() < 1e-4
                && (painted[1] - elev[1]).abs() < 1e-4
                && (painted[2] - elev[2]).abs() < 1e-4,
            "{name} 尺寸标签色相应是抬升表面"
        );
    }
}

/// 打开一份还没数出页的 Office 预览。种类名来自 `builtin_view`，大小写进文件事实。
fn previewing_office(path: &str, extension: &str, size: &str) -> ShellViewModel {
    let (view, label) = native_preview::builtin_view(extension)
        .unwrap_or_else(|| panic!("{extension} 应是 Office 预览"));
    let mut model = ShellViewModel::for_page(ShellPage::SelectedFile);
    let repo_id = model.workspace.active_repo_id.clone().unwrap_or_default();
    model.inspect.begin_selection(path);
    model.inspect.facts.extension = extension.to_string();
    model.inspect.facts.size_label = size.to_string();
    native_preview::begin(
        &mut model.inspect,
        &repo_id,
        path,
        view.into(),
        label.into(),
    );
    model
}

/// 顶栏某一枚芯片上的字。
fn chip_text(harness: &ShellHarness, key: &str) -> String {
    let context = harness.document().context();
    harness
        .nodes()
        .into_iter()
        .find_map(|node| {
            let path = context.assembly_path(node.id)?;
            path.split('/')
                .any(|part| part == key)
                .then(|| node.label.as_deref().unwrap_or("").to_string())
                .filter(|text| !text.is_empty())
        })
        .unwrap_or_else(|| panic!("{key} 没有文字"))
}

/// Office 顶栏种类照 Vue `officeKindLabel`：Word、Excel、PowerPoint。
/// 还没有页数时第三项是条目大小，没有大小写「准备文档」。有页的 PDF 第三项仍是「N 页 PDF」。
#[test]
fn office_toolbar_uses_vue_kind_labels() {
    for (extension, label) in [
        ("docx", "Word"),
        ("docm", "Word"),
        ("doc", "Word"),
        ("dotx", "Word"),
        ("dotm", "Word"),
        ("dot", "Word"),
        ("xlsx", "Excel"),
        ("xlsm", "Excel"),
        ("xlsb", "Excel"),
        ("xls", "Excel"),
        ("xltx", "Excel"),
        ("xltm", "Excel"),
        ("xlt", "Excel"),
        ("pptx", "PowerPoint"),
        ("pptm", "PowerPoint"),
        ("ppt", "PowerPoint"),
        ("ppsx", "PowerPoint"),
        ("ppsm", "PowerPoint"),
        ("pps", "PowerPoint"),
        ("potx", "PowerPoint"),
        ("potm", "PowerPoint"),
        ("pot", "PowerPoint"),
    ] {
        assert_eq!(
            native_preview::builtin_view(extension).map(|(_, kind)| kind),
            Some(label),
            "{extension}"
        );
    }

    let word = ShellHarness::mount(previewing_office("notes/a.docx", "docx", "   "));
    assert_eq!(chip_text(&word, "inspect-doc-kind"), "Word");
    assert_eq!(chip_text(&word, "inspect-doc-ext"), "DOCX");
    assert_eq!(
        chip_text(&word, "inspect-doc-pages"),
        "准备文档",
        "没有大小不编数字"
    );
    assert!(word.find("文档").is_none(), "顶栏不再笼统写「文档」");

    let sheet = ShellHarness::mount(previewing_office("notes/a.xlsx", "xlsx", " 12 KB "));
    assert_eq!(chip_text(&sheet, "inspect-doc-kind"), "Excel");
    assert_eq!(chip_text(&sheet, "inspect-doc-ext"), "XLSX");
    assert_eq!(
        chip_text(&sheet, "inspect-doc-pages"),
        "12 KB",
        "第三项用已经读到的大小"
    );
    assert!(sheet.find("准备文档").is_none());

    let deck = ShellHarness::mount(previewing_office("notes/a.pot", "pot", ""));
    assert_eq!(chip_text(&deck, "inspect-doc-kind"), "PowerPoint");
    assert_eq!(chip_text(&deck, "inspect-doc-ext"), "POT");

    let mut pdf = previewing(TWO_PAGES);
    pdf.inspect.facts.extension = "pdf".into();
    pdf.inspect.facts.size_label = "9 KB".into();
    let pdf = ShellHarness::mount(pdf);
    assert_eq!(chip_text(&pdf, "inspect-doc-kind"), "PDF");
    assert_eq!(
        chip_text(&pdf, "inspect-doc-pages"),
        "2 页 PDF",
        "有页数时不改写成大小"
    );
    assert!(pdf.find("准备文档").is_none());
}

/// 失败浮层铺 Vue `.media-preview__overlay` 的遮罩：`color-mix(in srgb, var(--bg) 74%, transparent)`，铺满预览框。
/// 能预览或还在读取时不画。标题和原因仍是原来的原文。
#[test]
fn failure_overlay_masks_with_the_vue_background() {
    let failed = ShellHarness::mount(scene("preview-audio-failed"));
    assert!(failed.find("无法预览该音频").is_some(), "失败标题还在");
    assert!(failed.find("没有原生解码器").is_some(), "失败原因还在");
    assert!(
        failed.keyed("inspect-audio-stage").is_none(),
        "失败时不留唱片舞台"
    );
    assert_mask(&failed, "inspect-failed-overlay");

    let retry = ShellHarness::mount(scene("online-preview-failed"));
    assert!(retry.find("无法预览该音频").is_some());
    assert!(
        retry.find("网易云登录已失效，请重新登录").is_some(),
        "来源原话还在"
    );
    assert!(retry.find("重试").is_some());
    assert_mask(&retry, "inspect-failed-overlay");

    let page = ShellHarness::mount(previewing(TWO_PAGES));
    assert!(
        page.find("无法预览该文档").is_some(),
        "画不出的页仍写原来的失败标题"
    );
    assert_mask(&page, "inspect-native-error-overlay");

    let playing = ShellHarness::mount(scene("preview-audio"));
    assert!(
        playing.keyed("inspect-audio-stage").is_some(),
        "能播的音频仍是唱片舞台"
    );
    assert_no_mask(&playing, "音频已经画出");

    let image = ShellHarness::mount(scene("preview-image"));
    assert_no_mask(&image, "图片已经画出");

    let preparing = ShellHarness::mount(scene("online-preview-preparing"));
    assert!(preparing.find("准备音频").is_some(), "准备卡还在");
    assert_no_mask(&preparing, "准备中不是失败");

    let mut loading = scene("preview-image");
    loading.preview_pixels = None;
    loading.preview_token = None;
    loading.inspect.body = PreviewBody::Empty;
    loading.inspect.loading = true;
    loading.reduce(ShellMessage::TaskProgressLoaded(Vec::new()));
    let loading = ShellHarness::mount(loading);
    assert!(loading.find("准备播放").is_some(), "图片读取胶囊还在");
    assert!(loading.find("准备媒体").is_some());
    assert_no_mask(&loading, "图片还在读");
}

/// 浮层底是背景色的 74%，铺满这一层，不再另铺不透明的 bg。
fn assert_mask(harness: &ShellHarness, key: &str) {
    let style = node_style(harness, key);
    assert_eq!(style.background, None, "{key} 不能再铺不透明的背景");
    let mix = SemanticColorMix::alpha(SemanticColorRole::Background, 0.74);
    assert_eq!(style.interaction.base.background_mix, Some(mix), "{key}");
    assert_eq!(
        style.layout.width,
        Some(LengthSpec::Fill),
        "{key} 要铺满预览框"
    );
    assert_eq!(style.layout.height, Some(LengthSpec::Fill), "{key}");
    assert_eq!(style.layout.gap, Some(LengthSpec::Px(8.0)), "{key}");
    assert_eq!(
        style.layout.padding_top,
        Some(LengthSpec::Px(24.0)),
        "{key}"
    );
    assert_eq!(
        style.layout.padding_right,
        Some(LengthSpec::Px(24.0)),
        "{key}"
    );
    assert_eq!(
        style.layout.padding_bottom,
        Some(LengthSpec::Px(24.0)),
        "{key}"
    );
    assert_eq!(
        style.layout.padding_left,
        Some(LengthSpec::Px(24.0)),
        "{key}"
    );
    for (name, mode) in [
        ("深色", ThemeAppearance::Dark),
        ("浅色", ThemeAppearance::Light),
    ] {
        let model = StyleModelRef::new(mode);
        let painted = mix.resolve(model).as_rgba_array();
        let bg = model.color(SemanticColorRole::Background).as_rgba_array();
        assert!(
            (painted[3] - bg[3] * 0.74).abs() < 1e-4,
            "{name} 遮罩透明度应是背景色的 74%"
        );
        assert!(
            (painted[0] - bg[0]).abs() < 1e-4
                && (painted[1] - bg[1]).abs() < 1e-4
                && (painted[2] - bg[2]).abs() < 1e-4,
            "{name} 遮罩色相应是背景色"
        );
    }
}

fn assert_no_mask(harness: &ShellHarness, why: &str) {
    assert!(
        harness.keyed("inspect-failed-overlay").is_none(),
        "{why} 不该画失败遮罩"
    );
    assert!(
        harness.keyed("inspect-native-error-overlay").is_none(),
        "{why} 不该画文档失败遮罩"
    );
}

fn node_style(harness: &ShellHarness, key: &str) -> nana_ui::runtime::NodeStyle {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    harness
        .document()
        .context()
        .world()
        .node_style(id)
        .unwrap_or_else(|| panic!("{key} 没有样式"))
        .clone()
}

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}
