//! 缩略图取样色板：没有元数据色板时，详情预览下面画出从像素取出的颜色；
//! 元数据「调色板」一行仍只认元数据，有元数据色板时不用取样结果。

use super::{sample_rgba, swatch_colors};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{FilesMessage, ShellMessage, ThumbnailFrame};

/// 一排不透明像素。宽是像素个数，高是 1，取样格和源图对齐。
fn row(pixels: &[[u8; 4]]) -> Vec<u8> {
    pixels.iter().flat_map(|pixel| *pixel).collect()
}

fn solid(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
    pixel.as_slice().repeat((width * height) as usize)
}

fn scene() -> crate::shell::ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(name, _)| *name == "live-files")
        .map(|(_, model)| model)
        .expect("没有 live-files 场景")
}

fn red_frame(path: &str) -> ThumbnailFrame {
    let width = 40;
    let height = 20;
    ThumbnailFrame {
        path: path.into(),
        natural_width: width,
        natural_height: height,
        width,
        height,
        rgba: solid(width, height, [255, 0, 0, 255]),
    }
}

/// 元数据没有色板时，单击选中的文件在详情预览下画出取样色；元数据行不出现。
/// 写上元数据色板以后改画元数据的颜色，取样的红不再出现。
#[test]
fn sampled_thumbnail_palette_shows_under_the_preview_and_not_in_metadata() {
    assert_eq!(sample_rgba(1, 1, &[255, 0, 0, 255]), ["#FF0000"]);
    assert!(
        sample_rgba(1, 1, &[255, 0, 0, 10]).is_empty(),
        "透明像素不取样"
    );
    assert!(
        sample_rgba(2, 2, &[255, 0, 0, 255]).is_empty(),
        "字节不够不编造颜色"
    );

    let mut near = [[255, 0, 0, 255]; 40];
    for pixel in &mut near[30..] {
        *pixel = [230, 0, 0, 255];
    }
    assert_eq!(
        sample_rgba(40, 1, &row(&near)),
        ["#FF0000"],
        "距离小于 30 的邻近色并进主色"
    );

    let mut split = [[255, 0, 0, 255]; 40];
    for pixel in &mut split[30..] {
        *pixel = [0, 0, 255, 255];
    }
    assert_eq!(
        sample_rgba(40, 1, &row(&split)),
        ["#FF0000", "#0000FF"],
        "差得远的颜色按出现次数保留"
    );

    let mut model = scene();
    let row = model
        .files
        .rows
        .iter_mut()
        .find(|row| row.path == "cover.png")
        .expect("列表里有 cover.png");
    row.thumbnail_path = Some("thumbs/cover.png".into());
    row.palette.clear();
    row.metadata.remove("palette");
    row.metadata.remove("thumbnailPalette");
    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(
        "cover.png".into(),
    )));
    let _ = model.files.take_effects();
    model.reduce(ShellMessage::ThumbnailPixels(vec![red_frame(
        "thumbs/cover.png",
    )]));
    assert_eq!(
        model.files.sampled_palette(Some("thumbs/cover.png")),
        ["#FF0000"]
    );
    assert_eq!(
        swatch_colors(&[], Some("thumbs/cover.png"), &model.files),
        ["#FF0000"],
        "没有元数据色板时用取样颜色"
    );
    assert_eq!(
        swatch_colors(&["#112233".into()], Some("thumbs/cover.png"), &model.files),
        ["#112233"],
        "元数据色板优先，不用取样的红"
    );

    let mut harness = ShellHarness::mount(model);
    assert!(
        harness.keyed("file-detail-palette-0-FF0000").is_some(),
        "详情预览下应该有取样出的红"
    );
    assert!(
        harness.find("调色板").is_none(),
        "元数据「调色板」不显示取样颜色"
    );

    let cover = harness
        .model
        .files
        .rows
        .iter_mut()
        .find(|row| row.path == "cover.png")
        .expect("cover.png");
    cover.palette = vec!["#112233".into()];
    harness.sync();
    harness.flush();
    assert!(
        harness.keyed("file-detail-palette-0-112233").is_some(),
        "有元数据色板时画元数据的颜色"
    );
    assert!(
        harness.keyed("file-detail-palette-0-FF0000").is_none(),
        "取样的红让位给元数据色板"
    );
    assert!(
        harness.find("调色板").is_some(),
        "元数据色板写进「调色板」一行"
    );
}
