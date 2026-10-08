//! A large static view builds within a 1 MB stack in a debug build.
//!
//! A static tree is a value: every element holds its component and, until
//! it is built, its children, and each builder method moves the element on.
//! This page (a settings page's worth of sections, rows and controls) is
//! built and mounted on a thread with the 1 MB stack a Windows main thread
//! has.
#![cfg(feature = "view-macro")]

use nana_ui_runtime::view::{IntoView, signal};
use nana_ui_runtime::{AppContext, DocumentId, view};

fn section(title: &'static str) -> impl IntoView {
    let on = signal(false);
    let level = signal(0.5f64);
    view! {
        <Column gap=8>
            <Text>{title}</Text>
            <Row gap=8>
                <Text>"启用"</Text>
                <Switch checked={on}>"开关"</Switch>
                <Button>"重置"</Button>
            </Row>
            <Row gap=8>
                <Text>"强度"</Text>
                <Slider min=0 max=1 step=0.05 label="强度" v-model={level}/>
                <Text>{format!("{:.2}", level.get())}</Text>
            </Row>
            <Row gap=8>
                <Checkbox checked={on}>"同步"</Checkbox>
                <TextInput placeholder="名称" label="名称"/>
                <Button disabled={on}>"应用"</Button>
            </Row>
            <Row gap=8>
                <Text>"说明"</Text>
                <Text>"这一行只有文字"</Text>
                <Divider/>
            </Row>
        </Column>
    }
}

fn page() -> impl IntoView {
    view! {
        <Column gap=16>
            <Row gap=8>
                <Text>"设置"</Text>
                <Button>"返回"</Button>
            </Row>
            <Column gap=12>
                {section("播放")}
                {section("弹幕")}
                {section("下载")}
                {section("缓存")}
            </Column>
            <Column gap=12>
                {section("快捷键")}
                {section("账号")}
                {section("外观")}
                {section("关于")}
            </Column>
        </Column>
    }
}

#[test]
fn a_large_static_view_builds_within_a_one_megabyte_stack() {
    std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(|| {
            let mut cx = AppContext::new();
            let document = DocumentId::new(1).unwrap();
            let view = cx.mount_view_root(document, page).unwrap();
            assert_eq!(view.roots().len(), 1);
        })
        .unwrap()
        .join()
        .expect("the view builds without overflowing its stack");
}
