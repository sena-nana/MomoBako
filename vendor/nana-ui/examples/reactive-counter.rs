//! `application-counter` written with the declarative view layer and its
//! `view!` template, plus a keyed list, a conditional block and two-way
//! bindings.
//!
//! `cargo run -p nana-ui --features hosted,bundled-fonts,view-macro --example reactive-counter`

use std::collections::HashMap;

use nana_ui::runtime::view::{IntoView, Signal, signal};
use nana_ui::runtime::{FrameworkError, LengthSpec, MountedView, Stack, view};
use nana_ui::{
    ApplicationIdentity, ApplicationState, ApplicationWindow, DiagnosticsConfig, NanaApplication,
    RuntimeApplication, RuntimeProgramContext, RuntimeProgramUpdate, WindowDescriptor,
};
use nana_ui_platform::WindowId;

#[derive(Clone)]
struct Todo {
    id: u32,
    title: String,
}

fn counter() -> impl IntoView {
    let count = signal(0u64);
    view! {
        <Row gap=8>
            <Text>"计数 {count}"</Text>
            <Button @activate={count.update(|c| *c += 1)}>"增加"</Button>
        </Row>
    }
}

fn todo_row(todo: Todo, list: Signal<Vec<Todo>>) -> impl IntoView {
    let id = todo.id;
    view! {
        <Row gap=8>
            <Text>{todo.title}</Text>
            <Button @activate={list.update(|list| list.retain(|t| t.id != id))}>"删除"</Button>
        </Row>
    }
}

fn todos() -> impl IntoView {
    let draft = signal(String::new());
    let list: Signal<Vec<Todo>> = signal(Vec::new());
    let next_id = signal(1u32);
    let add = move || {
        let title = draft.get_untracked().trim().to_owned();
        if title.is_empty() {
            return;
        }
        let id = next_id.get_untracked();
        next_id.set(id + 1);
        list.update(|list| list.push(Todo { id, title }));
        draft.set(String::new());
    };
    view! {
        <Column gap=8>
            <TextInput label="新任务" placeholder="新任务" v-model={draft} />
            <Button disabled={draft.with(|d| d.trim().is_empty())} @activate={add}>"添加"</Button>
            <TodoRow v-for={todo in list} key={todo.id} todo={todo} list={list} />
            <Text v-if={list.with(Vec::is_empty)}>"还没有任务"</Text>
            <Text v-else>{format!("共 {} 项", list.with(Vec::len))}</Text>
        </Column>
    }
}

fn volume() -> impl IntoView {
    let level = signal(0.5);
    view! {
        <Column gap=4>
            <Text>{format!("音量 {:.0}%", level.get() * 100.0)}</Text>
            <Slider min=0 max=1 step=0.05 label="音量" v-model={level} />
        </Column>
    }
}

fn app() -> impl IntoView {
    // The top inset clears the title bar the window draws over its content.
    let page = Stack::fill_column(16.0)
        .padding(16.0)
        .with_layout(|layout| layout.padding_top = Some(LengthSpec::Px(48.0)));
    view! {
        <Widget of={page}>
            {counter()}
            {volume()}
            {todos()}
        </Widget>
    }
}

#[derive(Default)]
struct Reactive {
    views: HashMap<WindowId, MountedView>,
}

impl ApplicationState for Reactive {
    type Message = ();
    type Error = FrameworkError;

    fn initialize(_: &RuntimeProgramContext<Self::Message>) -> Result<Self, Self::Error> {
        Ok(Self::default())
    }

    fn window_closed(&mut self, id: WindowId) {
        self.views.remove(&id);
    }

    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), Self::Error> {
        let document = window.document.document();
        let view = window
            .document
            .context_mut()
            .mount_view_root(document, app)?;
        self.views.insert(context.window_id(), view);
        Ok(())
    }

    fn update(
        &mut self,
        (): (),
        _: &mut HashMap<WindowId, ApplicationWindow>,
        _: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        RuntimeProgramUpdate::default()
    }
}

fn main() -> Result<(), nana_ui::HostedRunError> {
    let identity = ApplicationIdentity::new(
        "dev.nanaui.reactive-counter",
        "NanaUI Reactive Counter",
        env!("CARGO_PKG_VERSION"),
    );
    NanaApplication::builder(identity)
        .diagnostics(DiagnosticsConfig::default())
        .run::<RuntimeApplication<Reactive>>(
            WindowDescriptor::new("NanaUI Reactive Counter").initial_size(480.0, 420.0),
        )
}
