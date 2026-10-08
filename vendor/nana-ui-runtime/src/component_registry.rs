//! Unified Runtime component ABI for builtins and plugins.
//!
//! L3 `create_component`, Vue tags, and `UiExtension` all resolve through
//! [`ComponentRegistry`]. Application business state stays in `AppContext`
//! views; this table only names types and projects generic UI into `UiWorld`.
//!
//! This is **not** the Vue JS host factory table (`nana_ui_vue::NativeComponentRegistry`).
//! Register layout/hit/Scene components here via [`crate::ExtensionRegistrar::register_component`].

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt,
    sync::Arc,
};

use nana_ui_core::{ButtonKind, ControlSize, Icon, LayoutStyle};

use crate::{ComponentView, FrameworkError, MutationQueue, StableNodeId, UiWorld};

/// Stable component identity (`nana.button`, `app.bilibili-user-card`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentTypeId(Arc<str>);

impl ComponentTypeId {
    pub fn new(id: impl Into<String>) -> Result<Self, FrameworkError> {
        let id = id.into();
        let trimmed = id.trim();
        if trimmed.is_empty() {
            return Err(FrameworkError::InvalidComponentType);
        }
        Ok(Self(Arc::from(trimmed)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// So a `HashMap<ComponentTypeId, _>` can be looked up by the `&'static str`
/// constants in `component_descriptors` without building an owned id first.
/// Sound because the newtype's `Hash` and `Eq` are the wrapped `str`'s.
impl std::borrow::Borrow<str> for ComponentTypeId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ComponentTypeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Choice-control option. Application owns identity; this is display/disable only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticOption<'a> {
    pub value: &'a str,
    pub label: &'a str,
    pub disabled: bool,
}

/// Narrow L1/L2 property bag. Not application business state.
#[derive(Debug, Clone)]
pub struct SemanticSpec<'a> {
    pub type_id: &'a ComponentTypeId,
    pub label: &'a str,
    pub value: &'a str,
    pub hint: &'a str,
    pub placeholder: &'a str,
    pub disabled: bool,
    pub loading: bool,
    pub invalid: bool,
    pub active: bool,
    pub toggled: bool,
    pub read_only: bool,
    pub secure: bool,
    pub button_kind: ButtonKind,
    pub size: ControlSize,
    pub layout: &'a Arc<LayoutStyle>,
    pub icon: Option<Icon>,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub number: f32,
    pub options: &'a [SemanticOption<'a>],
    /// Extra Vue/host attributes. Plugins read these instead of extending this struct.
    pub attrs: &'a [(&'a str, &'a str)],
    /// Named child slots (`leading`, `body`, `control`, …). Not HostValue.
    pub slots: &'a [(&'a str, StableNodeId)],
}

impl<'a> SemanticSpec<'a> {
    pub fn from_parts(type_id: &'a ComponentTypeId, layout: &'a Arc<LayoutStyle>) -> Self {
        Self {
            type_id,
            label: "",
            value: "",
            hint: "",
            placeholder: "",
            disabled: false,
            loading: false,
            invalid: false,
            active: false,
            toggled: false,
            read_only: false,
            secure: false,
            button_kind: ButtonKind::Ghost,
            size: ControlSize::Medium,
            layout,
            icon: None,
            min: 0.0,
            max: 1.0,
            step: 0.1,
            number: 0.0,
            options: &[],
            attrs: &[],
            slots: &[],
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(*value))
    }

    pub fn slot(&self, name: &str) -> Option<StableNodeId> {
        self.slots
            .iter()
            .find_map(|(key, id)| key.eq_ignore_ascii_case(name).then_some(*id))
    }

    pub fn display_label(&self) -> &str {
        if !self.label.is_empty() {
            self.label
        } else {
            self.value
        }
    }
}

pub(crate) struct ComponentBindRequest<'a> {
    pub id: StableNodeId,
    pub world: &'a UiWorld,
    pub mutations: &'a mut MutationQueue,
    pub spec: &'a SemanticSpec<'a>,
    pub previous: Option<&'a (dyn Any + Send)>,
    pub retained: Option<Box<dyn Any + Send>>,
    pub finish: Option<fn(&mut crate::AppContext, StableNodeId) -> Result<(), FrameworkError>>,
}

/// A registry projection and its optional typed interaction state. Finish only
/// after the caller has committed the projection's mutations successfully.
pub struct PreparedSemanticBinding {
    pub(crate) id: StableNodeId,
    pub(crate) type_id: ComponentTypeId,
    pub(crate) kind: ComponentBindKind,
    pub(crate) retained: Option<Box<dyn Any + Send>>,
    pub(crate) finish:
        Option<fn(&mut crate::AppContext, StableNodeId) -> Result<(), FrameworkError>>,
}

impl PreparedSemanticBinding {
    pub fn id(&self) -> StableNodeId {
        self.id
    }

    pub fn type_id(&self) -> &ComponentTypeId {
        &self.type_id
    }

    pub fn kind(&self) -> ComponentBindKind {
        self.kind
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentBindKind {
    /// `from_semantic` projected a real control; skip generic Vue style.
    Projected,
    /// Tag-only / layout primitive; keep Vue generic style/a11y.
    Layout,
}

type Binder = Arc<
    dyn Fn(&mut ComponentBindRequest<'_>) -> Result<ComponentBindKind, FrameworkError>
        + Send
        + Sync,
>;

pub(crate) struct RegisteredComponentType {
    pub id: ComponentTypeId,
    rust_type: Option<TypeId>,
    binder: Binder,
    /// Whether this component reads the [`SemanticSpec`] fields a host has to
    /// derive by scanning the widget's children -- `slots` and `icon`. A
    /// component that ignores them makes every one of its instances pay those
    /// scans for nothing, and layout boxes, which ignore them, are exactly the
    /// widgets with the most children. Registered per component, so the layout
    /// aliases inherit it.
    reads_child_derived_spec: bool,
    /// Type-erased `AppContext::reproject_component`, for views a host bound
    /// through this registry rather than created through the context.
    pub(crate) reproject: Option<crate::framework::ReprojectFn>,
    /// Installs the type's hooks (activation, assembler) for a view bound
    /// through this registry. `None` for identity-only entries.
    pub(crate) register: Option<fn(&mut crate::AppContext)>,
}

/// Types that builtins and plugins register through the same ABI.
pub trait RegisterableComponent: ComponentView {
    const TYPE_ID: &'static str;
    const TAGS: &'static [&'static str];
    /// Layout primitives keep Vue generic style writeback (`Layout`).
    const BIND_KIND: ComponentBindKind = ComponentBindKind::Projected;
    fn from_semantic(spec: &SemanticSpec<'_>) -> Self;
    /// Opt in when interaction uses typed component state rather than only
    /// UiWorld fields. Builtins and extensions use the same retention path.
    const RETAIN_SEMANTIC_STATE: bool = false;
    /// Whether [`Self::from_semantic`] reads the [`SemanticSpec`] fields a host
    /// derives by scanning the widget's children: `slots` and `icon`.
    ///
    /// Leave it `true` unless the component genuinely ignores both: a host is
    /// entitled to skip those scans entirely when this is `false`, so a
    /// component that lies here silently loses its slot children and its icon.
    const READS_CHILD_DERIVED_SPEC: bool = true;
    fn reconcile_semantic(spec: &SemanticSpec<'_>, _previous: Option<&Self>) -> Self {
        Self::from_semantic(spec)
    }
    fn project_semantic(
        &self,
        _spec: &SemanticSpec<'_>,
        id: StableNodeId,
        world: &UiWorld,
        mutations: &mut MutationQueue,
    ) {
        self.project(id, world, mutations);
    }
    fn finish_semantic(
        _context: &mut crate::AppContext,
        _entity: crate::Entity<Self>,
    ) -> Result<(), FrameworkError> {
        Ok(())
    }
}

#[derive(Default)]
pub struct ComponentRegistry {
    by_id: HashMap<ComponentTypeId, RegisteredComponentType>,
    by_tag: HashMap<String, ComponentTypeId>,
    by_rust: HashMap<TypeId, ComponentTypeId>,
}

impl ComponentRegistry {
    pub(crate) fn get_by_rust(&self, type_id: TypeId) -> Option<&RegisteredComponentType> {
        self.by_rust.get(&type_id).and_then(|id| self.by_id.get(id))
    }

    pub fn resolve_tag(&self, tag: &str) -> Option<&ComponentTypeId> {
        self.by_tag.get(&normalize_tag(tag))
    }

    /// Resolve an already-normalized tag (see [`normalize_tag`]). Callers that
    /// probe several candidate spellings can normalize each candidate once and
    /// dedupe before looking up.
    pub fn resolve_normalized(&self, normalized_tag: &str) -> Option<&ComponentTypeId> {
        self.by_tag.get(normalized_tag)
    }

    /// Whether the component registered under `type_id` reads the
    /// [`SemanticSpec`] fields derived by scanning a widget's children
    /// (`slots`, `icon`).
    ///
    /// This lets a host skip those scans entirely for a component that ignores
    /// them. Unknown types answer `true`: never silently drop a slot or an icon
    /// for a component this registry has not been told about.
    pub fn reads_child_derived_spec(&self, type_id: &ComponentTypeId) -> bool {
        self.by_id
            .get(type_id)
            .is_none_or(|entry| entry.reads_child_derived_spec)
    }

    pub(crate) fn insert(&mut self, entry: RegisteredComponentType) -> Result<(), FrameworkError> {
        if self.by_id.contains_key(&entry.id) {
            return Err(FrameworkError::DuplicateComponentType(
                entry.id.as_str().to_owned(),
            ));
        }
        if let Some(rust_type) = entry.rust_type
            && self.by_rust.contains_key(&rust_type)
        {
            return Err(FrameworkError::DuplicateComponentType(
                entry.id.as_str().to_owned(),
            ));
        }
        if let Some(rust_type) = entry.rust_type {
            self.by_rust.insert(rust_type, entry.id.clone());
        }
        self.by_id.insert(entry.id.clone(), entry);
        Ok(())
    }

    pub(crate) fn insert_with_tags(
        &mut self,
        entry: RegisteredComponentType,
        tags: impl IntoIterator<Item = String>,
    ) -> Result<(), FrameworkError> {
        let tags = tags
            .into_iter()
            .filter(|tag| !tag.is_empty())
            .collect::<Vec<_>>();
        for tag in &tags {
            if self.by_tag.contains_key(tag) {
                return Err(FrameworkError::DuplicateComponentTag(tag.clone()));
            }
        }
        let id = entry.id.clone();
        self.insert(entry)?;
        for tag in tags {
            self.by_tag.insert(tag, id.clone());
        }
        Ok(())
    }

    pub(crate) fn extend(&mut self, other: ComponentRegistry) -> Result<(), FrameworkError> {
        for tag in other.by_tag.keys() {
            if self.by_tag.contains_key(tag) {
                return Err(FrameworkError::DuplicateComponentTag(tag.clone()));
            }
        }
        for id in other.by_id.keys() {
            if self.by_id.contains_key(id) {
                return Err(FrameworkError::DuplicateComponentType(
                    id.as_str().to_owned(),
                ));
            }
        }
        for (rust_type, id) in &other.by_rust {
            if self.by_rust.contains_key(rust_type) {
                return Err(FrameworkError::DuplicateComponentType(
                    id.as_str().to_owned(),
                ));
            }
        }
        self.by_rust.extend(other.by_rust);
        self.by_tag.extend(other.by_tag);
        self.by_id.extend(other.by_id);
        Ok(())
    }

    pub(crate) fn bind(
        &self,
        request: &mut ComponentBindRequest<'_>,
    ) -> Result<ComponentBindKind, FrameworkError> {
        let Some(entry) = self.by_id.get(request.spec.type_id) else {
            return Err(FrameworkError::MissingComponentType(
                request.spec.type_id.as_str().to_owned(),
            ));
        };
        (entry.binder)(request)
    }
}

fn normalized_tags(tags: &[&str]) -> Vec<String> {
    tags.iter()
        .map(|tag| normalize_tag(tag))
        .filter(|tag| !tag.is_empty())
        .collect()
}

fn bind_registerable<C: RegisterableComponent>() -> Binder {
    Arc::new(|request| {
        let component = C::reconcile_semantic(
            request.spec,
            request.previous.and_then(|value| value.downcast_ref::<C>()),
        );
        component.project_semantic(request.spec, request.id, request.world, request.mutations);
        if C::RETAIN_SEMANTIC_STATE {
            request.retained = Some(Box::new(component));
            request.finish =
                Some(|context, id| C::finish_semantic(context, crate::Entity::from_stable_id(id)));
        }
        Ok(C::BIND_KIND)
    })
}

pub(crate) fn registerable_entry<C: RegisterableComponent>()
-> Result<(RegisteredComponentType, Vec<String>), FrameworkError> {
    Ok((
        RegisteredComponentType {
            id: ComponentTypeId::new(C::TYPE_ID)?,
            rust_type: Some(TypeId::of::<C>()),
            binder: bind_registerable::<C>(),
            reads_child_derived_spec: C::READS_CHILD_DERIVED_SPEC,
            reproject: Some(crate::framework::reproject_erased::<C>),
            register: Some(crate::AppContext::register_view_type::<C>),
        },
        normalized_tags(C::TAGS),
    ))
}

pub(crate) fn tag_entry(
    type_id: &'static str,
    tags: &'static [&'static str],
) -> Result<(RegisteredComponentType, Vec<String>), FrameworkError> {
    Ok((
        RegisteredComponentType {
            id: ComponentTypeId::new(type_id)?,
            rust_type: None,
            binder: Arc::new(|_| Ok(ComponentBindKind::Layout)),
            reads_child_derived_spec: true,
            reproject: None,
            register: None,
        },
        normalized_tags(tags),
    ))
}

/// The binder of a type registered for identity only: it cannot be built
/// from a tag and attributes.
fn unbound() -> Binder {
    Arc::new(|_| Err(FrameworkError::InvalidComponentType))
}

/// A type's identity without its semantic binder or reprojector: nodes
/// created from the Rust type are stamped with its id and tags resolve to
/// it, but no code that builds it from a tag is referenced, so a binary that
/// never creates it links none of it. A layout alias has no `rust_type`.
pub(crate) fn identity_entry<C: RegisterableComponent>(
    type_id: &'static str,
    rust_type: Option<TypeId>,
    tags: &'static [&'static str],
) -> Result<(RegisteredComponentType, Vec<String>), FrameworkError> {
    Ok((
        RegisteredComponentType {
            id: ComponentTypeId::new(type_id)?,
            rust_type,
            binder: unbound(),
            reads_child_derived_spec: C::READS_CHILD_DERIVED_SPEC,
            reproject: None,
            register: None,
        },
        normalized_tags(tags),
    ))
}

/// Same binder as [`registerable_entry`], different type id / tags (layout aliases).
pub(crate) fn alias_entry<C: RegisterableComponent>(
    type_id: &'static str,
    tags: &'static [&'static str],
) -> Result<(RegisteredComponentType, Vec<String>), FrameworkError> {
    Ok((
        RegisteredComponentType {
            id: ComponentTypeId::new(type_id)?,
            rust_type: None,
            binder: bind_registerable::<C>(),
            reads_child_derived_spec: C::READS_CHILD_DERIVED_SPEC,
            reproject: None,
            register: None,
        },
        normalized_tags(tags),
    ))
}

pub fn normalize_tag(tag: &str) -> String {
    tag.trim().trim_start_matches("nana-").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_id_rejects_empty() {
        assert_eq!(
            ComponentTypeId::new("  ").unwrap_err(),
            FrameworkError::InvalidComponentType
        );
    }

    #[test]
    fn tags_normalize_nana_prefix() {
        assert_eq!(normalize_tag("nana-button"), "button");
        assert_eq!(normalize_tag("Button"), "button");
    }

    #[test]
    fn slot_matches_case_insensitive_name() {
        let type_id = ComponentTypeId::new("nana.list-item").unwrap();
        let layout = std::sync::Arc::new(LayoutStyle::default());
        let leading = StableNodeId::new(7).unwrap();
        let slots = [("Leading", leading)];
        let spec = SemanticSpec {
            slots: &slots,
            ..SemanticSpec::from_parts(&type_id, &layout)
        };
        assert_eq!(spec.slot("leading"), Some(leading));
        assert_eq!(spec.slot("LEADING"), Some(leading));
        assert_eq!(spec.slot("content"), None);
        assert_eq!(
            SemanticSpec::from_parts(&type_id, &layout).slot("leading"),
            None
        );
    }
}
