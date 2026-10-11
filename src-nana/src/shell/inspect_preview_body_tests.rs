//! PDF 预览框的翻页：第一页画不出来时，原来只剩失败浮层、没有翻页，后面的页都看不到。现在失败的页也带翻页，
//! 「下一页」点得到，翻过去画第二页。

use crate::shell::inspect::{native_preview, InspectMessage};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellPage, ShellViewModel};

const PDF_VIEW: &str = "momobako.preview.pdf";
const PATH: &str = "notes/page.pdf";

/// 预览页打开着这份 PDF（照验收场景 `live-preview` 的做法装进解析结果）。
fn previewing(pdf: &[u8]) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::SelectedFile);
    let repo_id = model.workspace.active_repo_id.clone().unwrap_or_default();
    model.inspect.begin_selection(PATH);
    native_preview::begin(&mut model.inspect, &repo_id, PATH, PDF_VIEW.into(), "PDF".into());
    let loaded = native_preview::load(PDF_VIEW, pdf).expect("PDF 能读");
    let generation = model.inspect.generation;
    model.reduce(ShellMessage::Inspect(InspectMessage::NativeLoaded { path: PATH.into(), generation, result: Ok(loaded) }));
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
    assert!(harness.keyed("inspect-native-page").is_none(), "第一页没有页纸");

    harness.click_label("下一页");
    assert!(harness.keyed("inspect-native-page").is_some(), "翻到第二页，画出页纸");
    assert!(harness.find("2 / 2").is_some());
    assert!(harness.find("无法预览该文档").is_none(), "第二页不再是失败浮层");
}
