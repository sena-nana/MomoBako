//! 标签菜单的位置和点外关闭。坐标夹取和侧栏弹层用同一条规则。

use super::InspectState;

impl InspectState {
    pub fn tag_menu_open(&self) -> bool {
        self.tag_menu
    }

    pub fn close_tag_menu(&mut self) {
        self.tag_menu = false;
    }

    pub fn open_tag_menu(&mut self, x: f32, y: f32, width: f32, height: f32, viewport_w: f32, viewport_h: f32) {
        if self.virtual_asset || self.saving || self.asset_id.is_none() {
            eprintln!("Nana 当前不能打开标签菜单");
            return;
        }
        let (x, y) = super::super::sidebar::clamp_anchored(x, y, width, height, viewport_w, viewport_h);
        self.tag_menu = true;
        self.tag_menu_x = x;
        self.tag_menu_y = y;
    }

    pub fn dismiss_tag_menu_outside(&mut self, inside: bool) {
        if self.tag_menu && !inside {
            self.tag_menu = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::InspectState;

    #[test]
    fn tag_menu_clamps_like_vue_and_closes_on_an_outside_click() {
        let mut state = InspectState::default();
        state.asset_id = Some("asset".into());
        state.open_tag_menu(-10.0, 900.0, 180.0, 80.0, 400.0, 300.0);
        assert!(state.tag_menu_open());
        assert_eq!(state.tag_menu_x, 4.0);
        assert_eq!(state.tag_menu_y, 216.0);
        state.dismiss_tag_menu_outside(true);
        assert!(state.tag_menu_open());
        state.dismiss_tag_menu_outside(false);
        assert!(!state.tag_menu_open());
    }
}
