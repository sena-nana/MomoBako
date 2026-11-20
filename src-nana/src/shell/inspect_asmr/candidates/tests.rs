//! 补全候选的测试：抓取结果的去向、导入框的错误、候选卡片上列什么、表单跟着文件走。
//! 卡片读详情卡叠层，不跟未保存草稿；正在输入时详情到达不冲掉作品 ID 和导入框。

use std::collections::BTreeMap;

use serde_json::json;

use super::super::AsmrMessage;
use super::super::tests::{audio, detail, open, row, saved, scene};
use super::{Candidate, DLSITE};
use crate::shell::files::FilesMessage;
use crate::shell::{InspectMessage, ShellMessage, ShellPage, ShellViewModel};

const REPO: &str = "acceptance-repo";

fn asmr(model: &mut crate::shell::ShellViewModel, message: AsmrMessage) {
    model.reduce(ShellMessage::Asmr(message));
}

fn lookup_result(asset_id: &str, result: Result<serde_json::Value, String>) -> AsmrMessage {
    AsmrMessage::LookupFinished {
        repo_id: REPO.into(),
        asset_id: asset_id.into(),
        result,
    }
}

/// 插件交回的结果：候选追加进当前文件的 `providerCandidates` 并保存；抓取进行中按钮写「抓取中」，回来以后恢复。
#[test]
fn lookup_results_are_appended_to_the_file_they_were_fetched_for() {
    let files = [("voice.mp3", audio(None, &[("rjCode", json!("RJ01234567"))]))];
    let mut model = scene(&files, "voice.mp3");
    asmr(&mut model, AsmrMessage::Lookup);
    model.admin.take_effects();
    let custom = super::super::display_custom(&model);
    assert!(super::candidates_view(&model, &custom).looking_up);
    asmr(&mut model, AsmrMessage::Lookup);
    assert!(
        model.admin.take_effects().is_empty(),
        "同一个文件还在抓时不再发"
    );

    let payload = json!({
        "provider": "dlsite",
        "id": "RJ01234567",
        "sourceUrl": "https://www.dlsite.com/x",
        "fetchedAt": "2026-10-11T00:00:00Z",
        "candidate": { "source": "dlsite", "confidence": "external-id", "fields": { "workTitle": "夜曲", "price": 1320 } }
    });
    asmr(&mut model, lookup_result("voice.mp3", Ok(payload)));
    let metadata = saved(&mut model).expect("抓到的候选写进当前文件");
    assert_eq!(
        metadata.get("providerCandidates"),
        Some(
            &json!([{ "source": "dlsite", "confidence": "external-id", "fields": { "workTitle": "夜曲", "price": 1320 } }])
        )
    );
    let view = super::candidates_view(&model, &super::super::display_custom(&model));
    assert!(!view.looking_up);
    assert!(view.candidates.is_empty(), "没写回详情叠层的候选不画出来");
}

/// 抓取结果回来时已经换了文件：丢掉，不写到正在看的别的文件上。
#[test]
fn lookup_results_for_a_file_left_behind_are_dropped() {
    let files = [
        ("a.mp3", audio(None, &[("rjCode", json!("RJ01234567"))])),
        ("b.mp3", audio(None, &[])),
    ];
    let mut model = scene(&files, "a.mp3");
    asmr(&mut model, AsmrMessage::Lookup);
    model.admin.take_effects();
    open(&mut model, &files, "b.mp3");
    let payload = json!({ "candidate": { "source": "dlsite", "fields": { "workTitle": "夜曲" } } });
    asmr(&mut model, lookup_result("a.mp3", Ok(payload)));
    assert!(saved(&mut model).is_none(), "不写 b.mp3");
    assert!(
        super::candidates_view(&model, &super::super::display_custom(&model))
            .candidates
            .is_empty()
    );
    assert!(
        model.asmr.candidates.lookups.is_empty(),
        "a.mp3 的抓取已经结束"
    );
}

/// 插件报错、结果里没有字段，原因写在导入框里，不存空候选。
#[test]
fn lookup_failures_are_shown_next_to_the_form() {
    let files = [("voice.mp3", audio(None, &[("rjCode", json!("RJ01234567"))]))];
    let mut model = scene(&files, "voice.mp3");
    asmr(&mut model, AsmrMessage::ToggleImport);
    asmr(&mut model, AsmrMessage::Lookup);
    asmr(
        &mut model,
        lookup_result(
            "voice.mp3",
            Err("provider returned HTTP 404 Not Found".into()),
        ),
    );
    let view = super::candidates_view(&model, &super::super::display_custom(&model));
    assert_eq!(
        view.import_error,
        "抓取失败：provider returned HTTP 404 Not Found"
    );
    assert!(!view.looking_up);

    asmr(&mut model, AsmrMessage::Lookup);
    asmr(
        &mut model,
        lookup_result(
            "voice.mp3",
            Ok(json!({ "candidate": { "source": "dlsite", "fields": {} } })),
        ),
    );
    let view = super::candidates_view(&model, &super::super::display_custom(&model));
    assert_eq!(view.import_error, "抓取结果没有可读取字段");
    assert!(saved(&mut model).is_none());
}

/// 没有作品 ID 时不发请求，写「缺少作品 ID」。
#[test]
fn lookup_without_a_work_id_says_so() {
    let files = [("voice.mp3", audio(None, &[]))];
    let mut model = scene(&files, "voice.mp3");
    asmr(&mut model, AsmrMessage::Lookup);
    assert!(model.admin.take_effects().is_empty());
    assert_eq!(
        super::candidates_view(&model, &super::super::display_custom(&model)).import_error,
        "缺少作品 ID"
    );
}

/// 导入框的 JSON 照 Vue `parseCandidateJson` 检查：空、坏 JSON、不是对象、没有字段都报原因，不写入。
#[test]
fn import_rejects_bad_json_without_writing() {
    let files = [("voice.mp3", audio(None, &[]))];
    let mut model = scene(&files, "voice.mp3");
    for (draft, error) in [
        ("", "候选 JSON 为空"),
        ("{", "候选 JSON 格式不正确"),
        ("[1]", "候选 JSON 必须是对象"),
        (r#"{"source":"x"}"#, "候选 JSON 没有可读取字段"),
    ] {
        asmr(&mut model, AsmrMessage::SetImportDraft(draft.into()));
        asmr(&mut model, AsmrMessage::ImportCandidate);
        assert_eq!(
            super::candidates_view(&model, &super::super::display_custom(&model)).import_error,
            error,
            "{draft}"
        );
        assert!(saved(&mut model).is_none());
    }
}

/// 卡片上列能写的字段和跳过的键；没有内容的值（null、空白、空数组）既不写也不列，免得清掉文件上已有的值。
#[test]
fn candidate_cards_skip_empty_values_and_list_skipped_keys() {
    let candidate = json!({ "source": " ", "fields": { "circle": "社团", "series": null, "workTitle": "  ", "voiceActors": [], "rating": 5, "unknown": 1 } });
    let files = [(
        "voice.mp3",
        audio(None, &[("providerCandidates", json!([candidate]))]),
    )];
    let mut model = scene(&files, "voice.mp3");
    let view = super::candidates_view(&model, &super::super::display_custom(&model));
    assert_eq!(view.candidates.len(), 1);
    let card = &view.candidates[0];
    assert_eq!(card.title, "provider 候选");
    assert_eq!(card.fields, [("circle".to_string(), "社团".to_string())]);
    assert_eq!(card.skipped, ["rating", "unknown"]);
    asmr(&mut model, AsmrMessage::Apply(0));
    assert_eq!(
        saved(&mut model).expect("应用"),
        [("circle".to_string(), json!("社团"))]
            .into_iter()
            .collect()
    );
}

/// 有没处理的版本冲突时候选锁住，不写入（写进去也会被「采用服务器版本」冲掉）。
#[test]
fn candidates_are_locked_while_a_conflict_waits() {
    let files = [(
        "voice.mp3",
        audio(
            None,
            &[(
                "providerCandidates",
                json!([{ "source": "dlsite", "fields": { "circle": "社团" } }]),
            )],
        ),
    )];
    let mut model = scene(&files, "voice.mp3");
    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "本地".into(),
    )));
    model.reduce(ShellMessage::Inspect(InspectMessage::SaveMetadata));
    model.inspect.take_effects();
    let server = detail("voice.mp3", &files[0].1, 3);
    model.reduce(ShellMessage::Inspect(InspectMessage::MetadataSaved(Ok((
        "conflict".into(),
        server,
    )))));
    assert!(super::candidates_view(&model, &super::super::display_custom(&model)).locked);
    asmr(&mut model, AsmrMessage::Apply(0));
    assert!(saved(&mut model).is_none());
}

/// 导入框和作品 ID 属于当时的文件：换了文件回到初始状态，上一个文件填的作品 ID 不会拿去抓别的文件；选的来源留着。
#[test]
fn the_form_belongs_to_the_file() {
    let files = [
        ("a.mp3", audio(None, &[("rjCode", json!("RJ01234567"))])),
        ("b.mp3", audio(None, &[("rjCode", json!("RJ07654321"))])),
    ];
    let mut model = scene(&files, "a.mp3");
    asmr(&mut model, AsmrMessage::ToggleImport);
    asmr(&mut model, AsmrMessage::SetLookupId("RJ99999999".into()));
    asmr(
        &mut model,
        AsmrMessage::SetProvider("momobako.service.provider.asmr-one".into()),
    );
    asmr(&mut model, AsmrMessage::SetImportDraft("{".into()));
    asmr(&mut model, AsmrMessage::ImportCandidate);
    open(&mut model, &files, "b.mp3");
    let custom = super::super::display_custom(&model);
    let view = super::candidates_view(&model, &custom);
    assert!(!view.import_open && view.import_error.is_empty());
    assert_eq!(super::lookup_id_shown(&model), "RJ07654321");
    assert_eq!(view.provider, "momobako.service.provider.asmr-one");
    asmr(&mut model, AsmrMessage::Lookup);
    let payloads = model
        .admin
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            crate::shell::admin::AdminEffect::CallPlugin {
                plugin_id, payload, ..
            } => Some((plugin_id, payload)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        payloads,
        [(
            "momobako.service.provider.asmr-one".to_string(),
            json!({ "id": "RJ07654321" })
        )]
    );
}

/// 候选对象的三种写法（`fields`、`metadata`、平铺）都读得出；插件结果没有 `candidate` 时整个结果就是候选。
#[test]
fn candidate_shapes_follow_vue() {
    for raw in [
        json!({ "fields": { "circle": "社团" } }),
        json!({ "metadata": { "circle": "社团" } }),
        json!({ "circle": "社团", "confidence": "高" }),
    ] {
        let candidate = Candidate::read(&raw).expect("读得出");
        assert_eq!(
            candidate.fields.get("circle"),
            Some(&json!("社团")),
            "{raw}"
        );
        assert!(!candidate.fields.contains_key("confidence"));
    }
    assert!(Candidate::read(&json!("文字")).is_none());
    assert!(Candidate::read(&json!({ "source": "x" })).is_none());
    let flat = Candidate::looked_up(&json!({ "provider": "asmr-one", "workTitle": "夜曲" }))
        .expect("平铺的结果");
    assert_eq!(
        (flat.source.as_str(), flat.confidence.as_deref()),
        ("asmr-one", None)
    );
    assert_eq!(super::provider_options().len(), 2);
    assert_eq!(
        super::super::AsmrUi::default().candidates.provider(),
        DLSITE
    );
}

/// 补全候选跟 Vue `readMetadataCandidates(currentFileEntry.metadata)`：详情卡叠对得上的素材详情，
/// 预览页只用列表条目。没保存的补丁不进卡片。正在输入时详情到达不冲掉作品 ID 和导入框。
/// 两边都没有的候选不编造；详情没带的键留着列表上的。
#[test]
fn candidates_follow_the_detail_overlay_not_the_draft() {
    let path = "voice.mp3";
    let list = audio(
        None,
        &[
            ("rjCode", json!("RJ11111111")),
            (
                "providerCandidates",
                json!([{ "source": "list", "fields": { "circle": "列表社团" } }]),
            ),
        ],
    );
    let typed = "{\"circle\":\"手填\"}";
    let mut model = click_card(path, list.clone());
    assert!(
        model.inspect.current_asset_id().is_none(),
        "单击以后详情还没回来"
    );
    asmr(&mut model, AsmrMessage::SetLookupId("正在输入".into()));
    asmr(&mut model, AsmrMessage::SetImportDraft(typed.into()));

    let overlaid = audio(
        None,
        &[
            ("rjCode", json!("RJ22222222")),
            (
                "providerCandidates",
                json!([{ "source": "detail", "confidence": "external-id", "fields": { "workTitle": "详情作品" } }]),
            ),
        ],
    );
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        path, &overlaid, 2,
    ))));
    assert!(
        !super::super::super::files_view::previewing(&model),
        "单击停在详情卡"
    );
    assert_eq!(
        super::lookup_id_shown(&model),
        "正在输入",
        "正在输入时详情不冲掉作品 ID"
    );
    assert_eq!(
        super::import_draft(&model),
        typed,
        "正在输入时详情不冲掉导入框"
    );
    assert_card(&model, "detail external-id", &[("workTitle", "详情作品")]);

    let patched = model.inspect.apply_metadata_patch(
        "voice.mp3",
        BTreeMap::from([(
            "providerCandidates".into(),
            json!([{ "source": "draft", "fields": { "circle": "草稿社团" } }]),
        )]),
    );
    assert!(patched.is_ok(), "补丁应写进草稿：{patched:?}");
    assert_card(&model, "detail external-id", &[("workTitle", "详情作品")]);
    assert_eq!(
        super::lookup_id_shown(&model),
        "正在输入",
        "没保存的补丁不冲掉作品 ID"
    );

    let later = audio(
        None,
        &[
            ("rjCode", json!("RJ33333333")),
            (
                "providerCandidates",
                json!([{ "source": "later", "fields": { "circle": "后到的社团" } }]),
            ),
        ],
    );
    // 仍是单击读详情：不把 select_only 清掉，否则会进预览页，候选改跟列表条目。
    model.files.click_load = Some(path.into());
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(path, &later, 3))));
    assert!(
        !super::super::super::files_view::previewing(&model),
        "再次到达的详情仍停在详情卡"
    );
    assert_eq!(super::lookup_id_shown(&model), "正在输入");
    assert_eq!(super::import_draft(&model), typed);
    assert_card(&model, "later 候选", &[("circle", "后到的社团")]);

    let mut kept = click_card(path, list.clone());
    let only_code = audio(None, &[("rjCode", json!("RJ22222222"))]);
    kept.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        path, &only_code, 2,
    ))));
    assert_eq!(
        super::lookup_id_shown(&kept),
        "RJ22222222",
        "没手填时用叠层上的 RJ 号"
    );
    assert_card(&kept, "list 候选", &[("circle", "列表社团")]);

    let mut cleared = click_card(path, list.clone());
    let empty = audio(None, &[("providerCandidates", json!([]))]);
    cleared.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(path, &empty, 2))));
    assert!(
        cards(&cleared).is_empty(),
        "详情给了空数组就不留列表上的候选，也不另编一条"
    );

    let mut bare = click_card(path, audio(None, &[]));
    bare.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        path,
        &audio(None, &[]),
        2,
    ))));
    assert!(cards(&bare).is_empty(), "两边都没有候选就不编造");

    let mut mismatched = click_card(path, list.clone());
    let mut wrong = detail(path, &overlaid, 2);
    wrong.summary.asset_id = "other-asset".into();
    mismatched.reduce(ShellMessage::AssetDetailLoaded(Ok(wrong)));
    assert_card(&mismatched, "list 候选", &[("circle", "列表社团")]);
    assert_eq!(
        super::lookup_id_shown(&mismatched),
        "RJ11111111",
        "编号对不上时不用详情里的 RJ 号"
    );

    let mut preview = scene(&[(path, list)], path);
    assert!(
        super::super::super::files_view::previewing(&preview),
        "双击停在预览页"
    );
    preview.reduce(ShellMessage::AssetDetailLoaded(Ok(detail(
        path, &overlaid, 2,
    ))));
    assert!(super::super::super::files_view::previewing(&preview));
    assert_card(&preview, "list 候选", &[("circle", "列表社团")]);
}

/// 单击选中，详情还没回来。插件用验收底子里的资源库扩展。
fn click_card(path: &str, metadata: BTreeMap<String, serde_json::Value>) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.files.rows = vec![row(path, &metadata)];
    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(path.into())));
    model
}

fn cards(model: &ShellViewModel) -> Vec<super::CandidateCard> {
    super::candidates_view(model, &super::super::display_custom(model)).candidates
}

fn assert_card(model: &ShellViewModel, title: &str, fields: &[(&str, &str)]) {
    let cards = cards(model);
    assert_eq!(cards.len(), 1, "候选条数：{cards:?}");
    assert_eq!(cards[0].title, title, "{cards:?}");
    let expect = fields
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect::<Vec<_>>();
    assert_eq!(cards[0].fields, expect, "{cards:?}");
    assert!(
        cards[0].fields.iter().all(|(key, _)| key != "series"),
        "没有的字段不编造：{cards:?}"
    );
}
