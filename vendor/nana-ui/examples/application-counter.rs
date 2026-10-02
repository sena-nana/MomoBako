use std::collections::HashMap;

use nana_ui::runtime::view::{button, entity_ref, text, widget, with_refs};
use nana_ui::runtime::{Activate, Entity, FrameworkError, List, Text};
use nana_ui::{
    ApplicationIdentity, ApplicationState, ApplicationWindow, DiagnosticsConfig, NanaApplication,
    RuntimeApplication, RuntimeProgramContext, RuntimeProgramUpdate, WindowDescriptor,
};
use nana_ui_platform::WindowId;

#[derive(Default)]
struct Counter {
    values: HashMap<WindowId, (u64, Entity<Text>)>,
}

impl ApplicationState for Counter {
    type Message = WindowId;
    type Error = FrameworkError;

    fn initialize(_: &RuntimeProgramContext<Self::Message>) -> Result<Self, Self::Error> {
        Ok(Self::default())
    }

    fn window_closed(&mut self, id: WindowId) {
        self.values.remove(&id);
    }

    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), Self::Error> {
        let id = context.window_id();
        let document = window.document.document();
        let (_, label) = window
            .document
            .context_mut()
            .mount_view_root(document, || {
                let label = entity_ref::<Text>();
                let counter = widget(List::new()).children((
                    text("0").entity_ref(label),
                    // `dispatch_program_all`, not `dispatch_program`: the message
                    // type is `WindowId`, so coalescing by type would drop every
                    // click but the last whenever two land in the same frame.
                    button("增加").on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(id)),
                ));
                with_refs(counter, label)
            })?;
        self.values.insert(id, (0, label));
        Ok(())
    }

    fn update(
        &mut self,
        id: WindowId,
        windows: &mut HashMap<WindowId, ApplicationWindow>,
        _: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        if let Some(window) = windows.get_mut(&id)
            && let Some((count, label)) = self.values.get_mut(&id)
        {
            *count += 1;
            window
                .document
                .context_mut()
                .update_component(*label, |text, _| {
                    text.value = count.to_string();
                })
                .expect("counter label belongs to this window");
            return RuntimeProgramUpdate::redraw(id);
        }
        RuntimeProgramUpdate::default()
    }
}

fn main() -> Result<(), nana_ui::HostedRunError> {
    // Identity names the per-user directories; diagnostics write `.nlog`
    // session logs under `ApplicationPaths::logs`.
    let identity = ApplicationIdentity::new(
        "dev.nanaui.counter",
        "NanaUI Counter",
        env!("CARGO_PKG_VERSION"),
    );
    NanaApplication::builder(identity)
        .diagnostics(DiagnosticsConfig::default())
        .run::<RuntimeApplication<Counter>>(
            WindowDescriptor::new("NanaUI Counter").initial_size(480.0, 320.0),
        )
}
