use std::fmt;
use std::sync::Arc;

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, LayoutBox, LengthSpec,
    MutationQueue, NodeKind, NodeStyle, StableNodeId, UiWorld,
};

const DEFAULT_LABEL: &str = "QR code";

/// Why [`QrCode::from_modules`] or [`QrCode::encode`] rejected a matrix, size, or payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrCodeError {
    InvalidModules,
    NonFiniteSize,
    EncodeFailed,
}

impl fmt::Display for QrCodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidModules => formatter.write_str("QR module matrix is empty or not square"),
            Self::NonFiniteSize => formatter.write_str("QR code size must be finite"),
            Self::EncodeFailed => formatter.write_str("QR payload could not be encoded"),
        }
    }
}

impl std::error::Error for QrCodeError {}

/// Scanner-safe QR display. Holds a square module matrix and paints through
/// the scene as an opaque white quiet zone with black modules. String
/// payloads encode here via [`QrCode::encode`]; pre-built matrices use
/// [`QrCode::from_modules`].
#[derive(Debug, Clone, PartialEq)]
pub struct QrCode {
    pub modules: Arc<[bool]>,
    pub width: usize,
    pub size: f32,
    pub label: Arc<str>,
}

impl QrCode {
    /// Quiet-zone modules on each side. Scanner-safe QR uses four, not theme padding.
    pub const QUIET_ZONE_MODULES: usize = 4;
    pub const DEFAULT_SIZE: f32 = 224.0;
    pub const MIN_SIZE: f32 = 64.0;

    pub fn from_modules(
        modules: impl Into<Arc<[bool]>>,
        width: usize,
        size: f32,
    ) -> Result<Self, QrCodeError> {
        let modules = modules.into();
        let expected = width
            .checked_mul(width)
            .ok_or(QrCodeError::InvalidModules)?;
        if width == 0 || modules.is_empty() || modules.len() != expected {
            return Err(QrCodeError::InvalidModules);
        }
        if !size.is_finite() {
            return Err(QrCodeError::NonFiniteSize);
        }
        Ok(Self {
            modules,
            width,
            size: clamp_size(size),
            label: Arc::from(DEFAULT_LABEL),
        })
    }

    /// Encode `data` (`qrcode::QrCode::new`, dark modules = true), then paint via [`Self::from_modules`].
    /// Quiet-zone modules are added at paint time, not in the matrix.
    pub fn encode(data: impl AsRef<[u8]>, size: f32) -> Result<Self, QrCodeError> {
        let code = qrcode::QrCode::new(data.as_ref()).map_err(|_| QrCodeError::EncodeFailed)?;
        let width = code.width();
        let modules: Arc<[bool]> = code
            .to_colors()
            .into_iter()
            .map(|color| color == qrcode::Color::Dark)
            .collect();
        Self::from_modules(modules, width, size)
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = clamp_size(size);
        self
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        let label = label.into();
        self.label = if label.is_empty() {
            Arc::from(DEFAULT_LABEL)
        } else {
            label
        };
        self
    }

    pub fn module_width(&self) -> usize {
        self.width
    }

    /// Pixel module size and local origin for a laid-out QR field.
    ///
    /// `floor(min(width, height) / (module_width + 8)).max(1)`, then centered
    /// with a floored origin relative to the top-left of `bounds`.
    pub fn module_geometry(bounds: LayoutBox, module_width: usize) -> (f32, (f32, f32)) {
        module_geometry(bounds, module_width)
    }

    fn resolved_size(&self) -> f32 {
        clamp_size(self.size)
    }

    fn resolved_label(&self) -> Arc<str> {
        if self.label.is_empty() {
            Arc::from(DEFAULT_LABEL)
        } else {
            Arc::clone(&self.label)
        }
    }

    fn effective_style(&self) -> NodeStyle {
        let size = self.resolved_size();
        let mut style = NodeStyle::default();
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Px(size));
        layout.height = Some(LengthSpec::Px(size));
        layout.min_width = Some(LengthSpec::Px(Self::MIN_SIZE));
        layout.min_height = Some(LengthSpec::Px(Self::MIN_SIZE));
        style
    }
}

fn clamp_size(size: f32) -> f32 {
    if size.is_finite() {
        size.max(QrCode::MIN_SIZE)
    } else {
        QrCode::MIN_SIZE
    }
}

fn inert() -> InteractionState {
    InteractionState {
        pointer_events: false,
        focusable: false,
    }
}

/// Pixel module size and local origin for a laid-out QR field.
///
/// Module size and origin:
/// `floor(min(width, height) / (module_width + 8)).max(1)`, then centered
/// with a floored origin relative to the top-left of `bounds`.
pub(crate) fn module_geometry(bounds: LayoutBox, module_width: usize) -> (f32, (f32, f32)) {
    let module_count = module_width + QUIET_ZONE_MODULES * 2;
    let shortest = bounds.width.min(bounds.height);
    let module_size = if module_count == 0 || !shortest.is_finite() {
        1.0
    } else {
        (shortest / module_count as f32).floor().max(1.0)
    };
    let rendered_size = module_size * module_count as f32;
    let origin = (
        ((bounds.width - rendered_size) / 2.0).floor(),
        ((bounds.height - rendered_size) / 2.0).floor(),
    );
    (module_size, origin)
}

pub(crate) const QUIET_ZONE_MODULES: usize = QrCode::QUIET_ZONE_MODULES;

impl ComponentView for QrCode {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "qr-code".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        // scene: QrCode { modules, width }
        let visual = crate::StandardVisual::QrCode {
            modules: Arc::clone(&self.modules),
            width: self.width,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        project_common(
            id,
            world,
            mutations,
            &self.effective_style(),
            inert(),
            AccessibilityState {
                role: AccessibilityRole::Image,
                label: Some(self.resolved_label()),
                ..AccessibilityState::default()
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framework::AppContext;
    use crate::{DocumentId, LayoutViewport};

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn sample_modules() -> Arc<[bool]> {
        [true, false, false, true].into_iter().collect()
    }

    #[test]
    fn from_modules_rejects_empty_or_mismatched_matrix() {
        assert_eq!(
            QrCode::from_modules(Arc::<[bool]>::from([]), 0, QrCode::DEFAULT_SIZE),
            Err(QrCodeError::InvalidModules)
        );
        assert_eq!(
            QrCode::from_modules(Arc::<[bool]>::from([]), 1, QrCode::DEFAULT_SIZE),
            Err(QrCodeError::InvalidModules)
        );
        assert_eq!(
            QrCode::from_modules(vec![true, false, true], 2, QrCode::DEFAULT_SIZE),
            Err(QrCodeError::InvalidModules)
        );
        assert_eq!(
            QrCode::from_modules(sample_modules(), 3, QrCode::DEFAULT_SIZE),
            Err(QrCodeError::InvalidModules)
        );
        assert!(QrCode::from_modules(sample_modules(), 2, QrCode::DEFAULT_SIZE).is_ok());
    }

    #[test]
    fn encode_builds_square_matrix_from_payload() {
        let code = QrCode::encode(
            "lilia-remote://pair?v=1&ticket=abc&challenge=def&bridge=http%3A%2F%2F10.0.0.2%3A41478",
            QrCode::DEFAULT_SIZE,
        )
        .unwrap();
        assert!(code.module_width() >= 21);
        assert_eq!(code.modules.len(), code.width * code.width);
        assert!(code.modules.iter().any(|module| *module));
        assert!(code.modules.iter().any(|module| !*module));
        assert_eq!(code.size, QrCode::DEFAULT_SIZE);
    }

    #[test]
    fn encode_rejects_oversized_payload_and_non_finite_size() {
        assert_eq!(
            QrCode::encode(vec![0u8; 16_384], QrCode::DEFAULT_SIZE),
            Err(QrCodeError::EncodeFailed)
        );
        assert_eq!(
            QrCode::encode("nana://pair", f32::NAN),
            Err(QrCodeError::NonFiniteSize)
        );
    }

    #[test]
    fn from_modules_rejects_non_finite_size() {
        assert_eq!(
            QrCode::from_modules(sample_modules(), 2, f32::NAN),
            Err(QrCodeError::NonFiniteSize)
        );
        assert_eq!(
            QrCode::from_modules(sample_modules(), 2, f32::INFINITY),
            Err(QrCodeError::NonFiniteSize)
        );
        assert_eq!(
            QrCode::from_modules(sample_modules(), 2, f32::NEG_INFINITY),
            Err(QrCodeError::NonFiniteSize)
        );
    }

    #[test]
    fn size_clamps_to_minimum() {
        let constructed = QrCode::from_modules(sample_modules(), 2, 12.0).unwrap();
        assert_eq!(constructed.size, QrCode::MIN_SIZE);
        assert_eq!(constructed.resolved_size(), QrCode::MIN_SIZE);

        let shrunk = constructed.size(0.0).size(-8.0);
        assert_eq!(shrunk.size, QrCode::MIN_SIZE);

        let grown = shrunk.size(320.0);
        assert_eq!(grown.size, 320.0);
        assert_eq!(grown.size(f32::NAN).size, QrCode::MIN_SIZE);
    }

    #[test]
    fn layout_uses_size_and_minimum() {
        let mut context = AppContext::new();
        let code = context
            .create_component(
                document(),
                QrCode::from_modules(sample_modules(), 2, QrCode::DEFAULT_SIZE).unwrap(),
            )
            .unwrap();
        let id = code.stable_id();

        assert!(matches!(
            context.world().node(id).unwrap().kind,
            NodeKind::Element { tag } if tag == "qr-code"
        ));
        let style = context.world().node_style(id).unwrap();
        assert_eq!(
            style.layout.width,
            Some(LengthSpec::Px(QrCode::DEFAULT_SIZE))
        );
        assert_eq!(
            style.layout.height,
            Some(LengthSpec::Px(QrCode::DEFAULT_SIZE))
        );
        assert_eq!(
            style.layout.min_width,
            Some(LengthSpec::Px(QrCode::MIN_SIZE))
        );
        assert_eq!(
            style.layout.min_height,
            Some(LengthSpec::Px(QrCode::MIN_SIZE))
        );
        assert_eq!(style.background, None);
        assert_eq!(style.foreground, None);
        assert_eq!(context.world().interaction(id), Some(inert()));
        assert!(matches!(
            context.world().standard_visual(id),
            Some(crate::StandardVisual::QrCode { width: 2, .. })
        ));

        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        let layout = context.world().layout_box(id).unwrap();
        assert_eq!(layout.width, QrCode::DEFAULT_SIZE);
        assert_eq!(layout.height, QrCode::DEFAULT_SIZE);

        context
            .update_component(code, |code, _| {
                *code = code.clone().size(10.0);
            })
            .unwrap();
        let style = context.world().node_style(id).unwrap();
        assert_eq!(style.layout.width, Some(LengthSpec::Px(QrCode::MIN_SIZE)));
        assert_eq!(style.layout.height, Some(LengthSpec::Px(QrCode::MIN_SIZE)));
        assert_eq!(
            style.layout.min_width,
            Some(LengthSpec::Px(QrCode::MIN_SIZE))
        );
        assert_eq!(
            style.layout.min_height,
            Some(LengthSpec::Px(QrCode::MIN_SIZE))
        );

        context
            .layout_document(document(), LayoutViewport::new(800.0, 600.0))
            .unwrap();
        let layout = context.world().layout_box(id).unwrap();
        assert_eq!(layout.width, QrCode::MIN_SIZE);
        assert_eq!(layout.height, QrCode::MIN_SIZE);
    }

    #[test]
    fn accessibility_uses_image_role_and_label() {
        let mut context = AppContext::new();
        let code = context
            .create_component(
                document(),
                QrCode::from_modules(sample_modules(), 2, QrCode::DEFAULT_SIZE).unwrap(),
            )
            .unwrap();
        let id = code.stable_id();
        let accessibility = context.world().accessibility(id).unwrap();
        assert_eq!(accessibility.role, AccessibilityRole::Image);
        assert_eq!(accessibility.label.as_deref(), Some(DEFAULT_LABEL));

        context
            .update_component(code, |code, _| {
                *code = code.clone().label("Pairing code");
            })
            .unwrap();
        let accessibility = context.world().accessibility(id).unwrap();
        assert_eq!(accessibility.label.as_deref(), Some("Pairing code"));

        context
            .update_component(code, |code, _| {
                *code = code.clone().label("");
            })
            .unwrap();
        let accessibility = context.world().accessibility(id).unwrap();
        assert_eq!(accessibility.label.as_deref(), Some(DEFAULT_LABEL));
    }

    #[test]
    fn module_geometry_matches_quiet_zone_math() {
        let bounds = LayoutBox {
            x: 10.0,
            y: 20.0,
            width: 224.0,
            height: 224.0,
        };
        let (module_size, origin) = module_geometry(bounds, 21);
        // module_count = 21 + 8, floor(224 / 29) = 7, origin floor((224 - 203) / 2) = 10
        assert_eq!(module_size, 7.0);
        assert_eq!(origin, (10.0, 10.0));
        assert!(module_size * QrCode::QUIET_ZONE_MODULES as f32 >= 4.0);

        let (module_size, origin) = module_geometry(
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 220.0,
                height: 220.0,
            },
            21,
        );
        assert_eq!(module_size, 7.0);
        assert_eq!(origin, (8.0, 8.0));

        let (module_size, origin) = module_geometry(
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 100.0,
            },
            21,
        );
        assert_eq!(module_size, 3.0);
        assert_eq!(origin, (106.0, 6.0));
        assert!(module_size >= 1.0);
    }

    #[test]
    fn idle_project_does_not_dirty() {
        let mut context = AppContext::new();
        let code = context
            .create_component(
                document(),
                QrCode::from_modules(sample_modules(), 2, QrCode::DEFAULT_SIZE).unwrap(),
            )
            .unwrap();
        let _ = context.take_system_work();
        context.reproject_component(code).unwrap();
        assert!(context.take_system_work().is_empty());
    }
}
