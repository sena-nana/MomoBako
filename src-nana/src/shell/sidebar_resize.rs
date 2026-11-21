//! 拖动侧栏边缘改宽度：读回工作区资源区的尺寸，换成侧栏宽度，决定什么时候写入。

/// 分隔条一帧。拖动中只夹取；松手，或没有指针捕获的键盘/双击变化，才写入。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SidebarResizeFrame {
    pub width: f32,
    pub dragging: bool,
    pub persist: bool,
    pub dirty: bool,
}

/// `extent` 是工作区资源区的尺寸。资源区比侧栏宽度少一条发丝间隙，这里加回来，
/// 存下的是侧栏本身的宽度（`momobako.sidebarWidth`）。
pub fn note_sidebar_resize(
    was_dragging: bool,
    dragging: bool,
    extent: f32,
    shell_width: f32,
    dirty: bool,
) -> SidebarResizeFrame {
    let width = super::workspace::clamp_sidebar_width(extent + super::render::WORKBENCH_GAP_PX);
    let changed = (width - shell_width).abs() > 0.5;
    if dragging {
        return SidebarResizeFrame {
            width,
            dragging: true,
            persist: false,
            dirty: dirty || changed,
        };
    }
    if was_dragging {
        return SidebarResizeFrame {
            width,
            dragging: false,
            persist: dirty || changed,
            dirty: false,
        };
    }
    SidebarResizeFrame {
        width,
        dragging: false,
        persist: changed,
        dirty: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_resize_persists_on_release_and_on_a_settled_change() {
        // 资源区比侧栏宽度少一条发丝间隙，读回时加上。
        let gap = super::super::render::WORKBENCH_GAP_PX;
        let dragging = note_sidebar_resize(false, true, 420.0 - gap, 276.0, false);
        assert!(!dragging.persist);
        assert!(dragging.dirty);
        assert_eq!(dragging.width, 420.0);
        let released = note_sidebar_resize(true, false, 420.0 - gap, 420.0, true);
        assert!(released.persist);
        assert!(!released.dirty);
        let settled = note_sidebar_resize(false, false, 420.0 - gap, 420.0, false);
        assert!(
            !settled.persist,
            "静止时资源区加间隙正好等于侧栏宽度，不再写入"
        );
        let keyed = note_sidebar_resize(false, false, 428.0 - gap, 420.0, false);
        assert!(keyed.persist);
        assert_eq!(keyed.width, 428.0);
        let clamped = note_sidebar_resize(false, false, 1000.0, 276.0, false);
        assert_eq!(clamped.width, crate::theme_map::SIDEBAR_MAX_PX);
        let reset = note_sidebar_resize(false, false, 276.0 - gap, 480.0, false);
        assert!(reset.persist);
        assert_eq!(reset.width, 276.0);
    }
}
