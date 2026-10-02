//! Windows host-only DirectComposition tree for native content below NanaUI.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::core::Interface;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowsCompositionRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl WindowsCompositionRect {
    fn valid(self) -> bool {
        [
            self.x,
            self.y,
            self.width,
            self.height,
            self.x + self.width,
            self.y + self.height,
        ]
        .into_iter()
        .all(f32::is_finite)
            && self.width >= 0.0
            && self.height >= 0.0
    }
}

#[derive(Debug)]
pub enum WindowsCompositionError {
    WindowHandle(String),
    Native(windows::core::Error),
    InvalidGeometry,
    Removed,
}
impl fmt::Display for WindowsCompositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowHandle(message) => {
                write!(f, "DirectComposition requires a Win32 window: {message}")
            }
            Self::Native(error) => write!(f, "DirectComposition: {error}"),
            Self::InvalidGeometry => {
                f.write_str("DirectComposition geometry must be finite with nonnegative size")
            }
            Self::Removed => f.write_str("DirectComposition visual has been removed"),
        }
    }
}
impl std::error::Error for WindowsCompositionError {}
impl From<windows::core::Error> for WindowsCompositionError {
    fn from(error: windows::core::Error) -> Self {
        Self::Native(error)
    }
}

/// What a composed window's tree has been asked to publish, and how much work
/// it has done. DirectComposition is a retained compositor: an unchanged tree
/// needs no `Commit`, however many GPU frames the UI presents into its visual.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowsCompositionWork {
    /// `IDCompositionDevice::Commit` calls the system accepted. A failed commit
    /// is not counted and leaves the tree dirty for the next frame.
    pub commits: usize,
    /// Changes staged on this composition device since the tree was created:
    /// visuals added or removed, geometry written, visibility flipped, and the
    /// swapchain binding a surface configure leaves behind. Each one is a
    /// reason the next commit has work to publish.
    pub tree_mutations: usize,
}

struct CompositionTree {
    device: IDCompositionDevice,
    target: IDCompositionTarget,
    _root: IDCompositionVisual,
    native_root: IDCompositionVisual,
    ui: IDCompositionVisual,
    hwnd: HWND,
    /// Staged mutations no `Commit` has published yet. A tree nothing touched
    /// since its last commit has nothing to publish, and committing it anyway
    /// is per-frame work with no effect — the thing this flag exists to stop.
    dirty: Cell<bool>,
    work: Cell<WindowsCompositionWork>,
    _window: Arc<dyn winit::window::Window>,
}
impl CompositionTree {
    /// Records one staged mutation. The next commit publishes it.
    fn touch(&self) {
        self.dirty.set(true);
        let mut work = self.work.get();
        work.tree_mutations = work.tree_mutations.saturating_add(1);
        self.work.set(work);
    }
    fn commit(&self) -> Result<(), WindowsCompositionError> {
        if !self.dirty.get() {
            return Ok(());
        }
        // The flag is cleared only once the system has taken the batch: a
        // failed commit leaves the tree dirty, so the next frame publishes the
        // changes instead of forgetting them.
        unsafe {
            self.device.Commit()?;
        }
        self.dirty.set(false);
        let mut work = self.work.get();
        work.commits = work.commits.saturating_add(1);
        self.work.set(work);
        Ok(())
    }
}
impl Drop for CompositionTree {
    fn drop(&mut self) {
        // The retained window remains alive until the target is detached.
        unsafe {
            let _ = self.target.SetRoot(None::<&IDCompositionVisual>);
            let _ = self.device.Commit();
        }
    }
}

/// The mutable half of a composed window's DirectComposition tree.
///
/// This is what a [`RuntimeProgram::native_content_frame`] callback receives,
/// and it is deliberately everything that callback may do: stage visuals and
/// their geometry. It cannot commit. The transaction belongs to the Scene
/// host, which publishes every window's staged changes once per frame, so a
/// backend and the host can never both submit the same tree.
///
/// [`RuntimeProgram::native_content_frame`]: crate::RuntimeProgram::native_content_frame
#[derive(Clone)]
pub struct WindowsCompositionTree {
    tree: Rc<CompositionTree>,
}
impl WindowsCompositionTree {
    /// Borrowed HWND, valid while this handle remains alive.
    pub fn window_handle(&self) -> *mut c_void {
        self.tree.hwnd.0
    }
    /// Starts hidden. Geometry and visibility are staged; the host publishes
    /// them with the rest of the frame's tree changes.
    pub fn create_native_visual(&self) -> Result<WindowsNativeVisual, WindowsCompositionError> {
        let (container, content) = unsafe {
            let container = self.tree.device.CreateVisual()?;
            let content = self.tree.device.CreateVisual()?;
            container.SetClip2(&D2D_RECT_F::default())?;
            container.AddVisual(&content, false, None::<&IDCompositionVisual>)?;
            self.tree
                .native_root
                .AddVisual(&container, true, None::<&IDCompositionVisual>)?;
            (container, content)
        };
        self.tree.touch();
        Ok(WindowsNativeVisual {
            inner: Rc::new(NativeVisual {
                tree: Rc::clone(&self.tree),
                container,
                content,
                state: RefCell::new(VisualState::default()),
            }),
        })
    }
}

/// UI-thread backend access; ordinary Runtime controls never receive this handle.
/// Native visuals are children of a layer permanently below the WGPU UI visual.
///
/// Held by the Scene host. It owns the commit — see [`Self::commit`] — and
/// hands backends the [`WindowsCompositionTree`], which cannot.
#[derive(Clone)]
pub struct WindowsComposition {
    tree: WindowsCompositionTree,
}
impl std::ops::Deref for WindowsComposition {
    type Target = WindowsCompositionTree;
    fn deref(&self) -> &Self::Target {
        &self.tree
    }
}
impl WindowsComposition {
    pub(crate) fn new(
        window: Arc<dyn winit::window::Window>,
    ) -> Result<Self, WindowsCompositionError> {
        let handle = window
            .window_handle()
            .map_err(|error| WindowsCompositionError::WindowHandle(error.to_string()))?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err(WindowsCompositionError::WindowHandle(
                "non-Win32 handle".to_owned(),
            ));
        };
        let hwnd = HWND(handle.hwnd.get() as *mut c_void);
        // No D3D device is created here: WGPU supplies the UI swapchain.
        let (device, target, root, native_root, ui) = unsafe {
            let device: IDCompositionDevice = DCompositionCreateDevice(None::<&IDXGIDevice>)?;
            let target = device.CreateTargetForHwnd(hwnd, true)?;
            let root = device.CreateVisual()?;
            let native_root = device.CreateVisual()?;
            let ui = device.CreateVisual()?;
            root.AddVisual(&native_root, false, None::<&IDCompositionVisual>)?;
            root.AddVisual(&ui, true, Some(&native_root))?;
            target.SetRoot(&root)?;
            device.Commit()?;
            (device, target, root, native_root, ui)
        };
        Ok(Self {
            tree: WindowsCompositionTree {
                tree: Rc::new(CompositionTree {
                    device,
                    target,
                    _root: root,
                    native_root,
                    ui,
                    hwnd,
                    // The construction above committed the root tree itself.
                    dirty: Cell::new(false),
                    work: Cell::new(WindowsCompositionWork {
                        commits: 1,
                        tree_mutations: 0,
                    }),
                    _window: window,
                }),
            },
        })
    }
    pub(crate) fn ui_visual(&self) -> *mut c_void {
        self.tree.tree.ui.as_raw()
    }
    /// The handle a native backend is given: staging without commit.
    pub const fn tree(&self) -> &WindowsCompositionTree {
        &self.tree
    }
    /// Publishes this window's staged tree, geometry and native-engine content
    /// changes, and nothing else.
    ///
    /// A tree with nothing staged since its last commit does no work and no
    /// system call: DirectComposition is retained, so the visuals stay exactly
    /// as they were while the UI presents new GPU frames into them. This is
    /// what keeps a 120 FPS host texture from synchronising a static visual
    /// tree 120 times a second.
    pub fn commit(&self) -> Result<(), WindowsCompositionError> {
        self.tree.tree.commit()
    }

    /// Publishes changes something other than this tree staged on the same
    /// composition device.
    ///
    /// WGPU binds a configured swapchain to the UI visual with
    /// `IDCompositionVisual::SetContent`, which stages work on this device that
    /// the tree's own dirty flag cannot see. Skipping that commit would leave
    /// the UI visual holding no content at all, so a surface configure has to
    /// say so rather than rely on the flag.
    pub(crate) fn commit_external(&self) -> Result<(), WindowsCompositionError> {
        self.tree.tree.touch();
        self.tree.tree.commit()
    }
    /// Commits and tree mutations this window has done since it was created.
    pub fn work(&self) -> WindowsCompositionWork {
        self.tree.tree.work.get()
    }
}

#[derive(Default)]
struct VisualState {
    bounds: WindowsCompositionRect,
    clip: Option<WindowsCompositionRect>,
    visible: bool,
    removed: bool,
}
struct NativeVisual {
    tree: Rc<CompositionTree>,
    container: IDCompositionVisual,
    content: IDCompositionVisual,
    state: RefCell<VisualState>,
}
impl Drop for NativeVisual {
    fn drop(&mut self) {
        if !self.state.get_mut().removed {
            unsafe {
                let _ = self.tree.native_root.RemoveVisual(&self.container);
            }
            // Staged, not published: the host's next commit takes it, like
            // every other tree change. A drop that committed on its own would
            // be a second transaction authority.
            self.tree.touch();
        }
    }
}

/// Clones share one native visual; dropping the last handle removes its subtree.
/// The engine attaches to the content visual; host geometry lives on its parent.
#[derive(Clone)]
pub struct WindowsNativeVisual {
    inner: Rc<NativeVisual>,
}
impl WindowsNativeVisual {
    /// Borrowed IDCompositionVisual pointer. Do not take ownership of this COM
    /// reference; a native API retaining it must perform its own AddRef.
    pub fn as_raw(&self) -> *mut c_void {
        self.inner.content.as_raw()
    }
    pub fn window_handle(&self) -> *mut c_void {
        self.inner.tree.hwnd.0
    }
    /// Bounds and clip use physical client-area coordinates. The clip is
    /// intersected with bounds and translated into visual-local space.
    pub fn set_geometry(
        &self,
        bounds: WindowsCompositionRect,
        clip: Option<WindowsCompositionRect>,
    ) -> Result<(), WindowsCompositionError> {
        let mut state = self.inner.state.borrow_mut();
        if state.removed {
            return Err(WindowsCompositionError::Removed);
        }
        let rect = local_clip(bounds, clip, state.visible)?;
        if state.bounds == bounds && state.clip == clip {
            return Ok(());
        }
        unsafe {
            self.inner.container.SetOffsetX2(bounds.x)?;
            self.inner.container.SetOffsetY2(bounds.y)?;
            self.inner.container.SetClip2(&rect)?;
        }
        self.inner.tree.touch();
        state.bounds = bounds;
        state.clip = clip;
        Ok(())
    }
    pub fn set_visible(&self, visible: bool) -> Result<(), WindowsCompositionError> {
        let mut state = self.inner.state.borrow_mut();
        if state.removed {
            return Err(WindowsCompositionError::Removed);
        }
        if state.visible == visible {
            return Ok(());
        }
        let rect = local_clip(state.bounds, state.clip, visible)?;
        unsafe {
            self.inner.container.SetClip2(&rect)?;
        }
        self.inner.tree.touch();
        state.visible = visible;
        Ok(())
    }
    /// Detaches from the pending tree. Idempotent; the host's frame commit
    /// publishes it.
    pub fn remove(&self) -> Result<(), WindowsCompositionError> {
        let mut state = self.inner.state.borrow_mut();
        if !state.removed {
            unsafe {
                self.inner
                    .tree
                    .native_root
                    .RemoveVisual(&self.inner.container)?;
            }
            self.inner.tree.touch();
            state.removed = true;
        }
        Ok(())
    }
}

fn local_clip(
    bounds: WindowsCompositionRect,
    clip: Option<WindowsCompositionRect>,
    visible: bool,
) -> Result<D2D_RECT_F, WindowsCompositionError> {
    if !bounds.valid() || clip.is_some_and(|clip| !clip.valid()) {
        return Err(WindowsCompositionError::InvalidGeometry);
    }
    if !visible {
        return Ok(D2D_RECT_F::default());
    }
    let clip = clip.unwrap_or(bounds);
    let left = (clip.x - bounds.x).clamp(0.0, bounds.width);
    let top = (clip.y - bounds.y).clamp(0.0, bounds.height);
    let right = ((clip.x + clip.width) - bounds.x).clamp(left, bounds.width);
    let bottom = ((clip.y + clip.height) - bounds.y).clamp(top, bounds.height);
    Ok(D2D_RECT_F {
        left,
        top,
        right,
        bottom,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_geometry_clips_in_client_pixels_and_never_escapes_bounds() {
        let bounds = WindowsCompositionRect {
            x: 100.0,
            y: 50.0,
            width: 80.0,
            height: 60.0,
        };
        let clip = WindowsCompositionRect {
            x: 90.0,
            y: 70.0,
            width: 50.0,
            height: 80.0,
        };
        let local = local_clip(bounds, Some(clip), true).unwrap();
        assert_eq!(
            (local.left, local.top, local.right, local.bottom),
            (0.0, 20.0, 40.0, 60.0)
        );
        let hidden = local_clip(bounds, Some(clip), false).unwrap();
        assert_eq!(
            (hidden.left, hidden.top, hidden.right, hidden.bottom),
            (0.0, 0.0, 0.0, 0.0)
        );
        let disjoint = local_clip(
            bounds,
            Some(WindowsCompositionRect { x: 200.0, ..clip }),
            true,
        )
        .unwrap();
        assert_eq!(disjoint.left, disjoint.right);
        assert!(
            local_clip(
                WindowsCompositionRect {
                    width: -1.0,
                    ..bounds
                },
                None,
                true
            )
            .is_err()
        );
        assert!(
            local_clip(
                WindowsCompositionRect {
                    x: f32::NAN,
                    ..bounds
                },
                None,
                false
            )
            .is_err()
        );
    }
}
