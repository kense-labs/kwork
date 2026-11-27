//! Place thread controls outside the reading column at every panel width.
use crate::{
    types::ChatAccessMode,
    ui_motion::{MenuPlacement, Tactile, menu_host},
    ui_primitives::{
        CTRL_SM_H, DETAILS_CARD_W, FS_META, FS_UI, StudioPalette, frosted_card,
        render_file_diff_icon, render_git_branch_icon, render_machine_icon, tnum_features,
    },
    workbench::{KarWorkbench, Menu},
};
use gpui::{prelude::*, *};
use kui::{
    components::pro_chrome::pro_spring_f32,
    primitives::pro_icons::{ProIcon, pro_icon},
};

const EDGE: f32 = 12.;
const STRIP_H: f32 = 40.;

#[derive(Clone, Copy)]
pub(super) enum Placement {
    Margin { width: f32 },
    Strip { width: f32 },
}

impl Placement {
    pub(super) fn for_thread(pane: f32, reading: f32, font_scale: f32) -> Self {
        // The outer gutter is symmetric. Keep clearance on both sides of the card.
        let space = (pane - reading) / 2. - 2. * EDGE;
        let min_width = 208. * font_scale;
        if space >= min_width {
            Self::Margin {
                width: space.min(DETAILS_CARD_W.max(min_width)),
            }
        } else {
            Self::Strip { width: pane }
        }
    }

    pub(super) fn strip_height(self, reveal: f32) -> f32 {
        if matches!(self, Self::Strip { .. }) {
            STRIP_H * reveal
        } else {
            0.
        }
    }
}

impl KarWorkbench {
    pub(super) fn thread_details_reveal(&self, window: &mut Window, cx: &mut Context<Self>) -> f32 {
        pro_spring_f32(
            "thread-details-reveal",
            f32::from(self.thread_details_open),
            0.15,
            window,
            cx,
        )
        .clamp(0., 1.)
    }

    pub(super) fn thread_body_with_details(
        &self,
        body: AnyElement,
        placement: Placement,
        reveal: f32,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .map(|pane| match placement {
                Placement::Strip { width } => pane.child(self.details_strip(width, reveal, s, cx)),
                Placement::Margin { .. } => pane,
            })
            .child(div().w_full().flex_1().min_h_0().child(body))
            .when(reveal > 0.001, |pane| match placement {
                Placement::Margin { width } => pane.child(
                    div()
                        .absolute()
                        .right(px(EDGE))
                        .top(px(4. + (1. - reveal) * -3.))
                        .w(px(width))
                        .opacity(reveal)
                        .child(self.details_controls(placement, s, cx)),
                ),
                Placement::Strip { .. } => pane,
            })
            .into_any_element()
    }

    pub(crate) fn compact_document_details(
        &self,
        width: f32,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let reveal = self.thread_details_reveal(window, cx);
        self.details_strip(width, reveal, s, cx)
    }

    fn details_strip(
        &self,
        width: f32,
        reveal: f32,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .w_full()
            .flex_none()
            .h(px(STRIP_H * reveal))
            .overflow_hidden()
            .when(reveal > 0.001, |slot| {
                slot.child(
                    div()
                        .w_full()
                        .mx_auto()
                        .max_w(px(
                            self.prefs.chat_width_mode.width_for_pane(width) + 2. * EDGE
                        ))
                        .h(px(STRIP_H))
                        .flex()
                        .items_center()
                        .px(px(EDGE))
                        .opacity(reveal)
                        .child(self.details_controls(Placement::Strip { width }, s, cx)),
                )
            })
    }

    fn details_controls(
        &self,
        placement: Placement,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (strip, width) = match placement {
            Placement::Margin { width } => (false, width),
            Placement::Strip { width } => (true, width),
        };
        let scale = f32::from(s.font_size(FS_UI)) / FS_UI;
        let small = strip && width < 540. * scale;
        let loc = self.ui_locale;
        let host = match &self.access_mode {
            ChatAccessMode::RemoteController { host_name, .. } => host_name.clone(),
            ChatAccessMode::DemoShowcase => "MacStudio".into(),
            _ => loc.pick("本机", "Local computer").to_owned(),
        };
        let label = |text: String| div().min_w_0().truncate().child(text);
        let host_label = div()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(6.))
            .when(strip, |row| row.max_w(px((width * 0.22).min(168.))))
            .when(!strip, |row| row.h(px(32.)).px(px(10.)))
            .child(render_machine_icon(14., s.muted))
            .child(label(host));
        let button = |id, title: &'static str| {
            Tactile::ghost(id, s)
                .h(px(CTRL_SM_H))
                .min_w(px(CTRL_SM_H))
                .gap(px(6.))
                .when(strip, |b| b.px(px(6.)).justify_center())
                .when(!strip, |b| b.w_full().px(px(10.)))
                .tip(title)
                .aria(title)
        };

        let open_label = loc.pick("在编辑器或画布打开", "Open in editor");
        let open = menu_host(
            button("details-open-in-btn", open_label)
                .child(pro_icon(ProIcon::ExternalArrow, 14., s.muted))
                .when(!small, |b| {
                    b.child(if strip {
                        loc.pick("打开", "Open")
                    } else {
                        open_label
                    })
                })
                .lit(self.open_menu == Some(Menu::DetailsOpenIn))
                .on_press(cx, |this, _, cx| this.toggle_menu(Menu::DetailsOpenIn, cx)),
            self.render_details_open_in_menu(s, cx),
        );
        let scripts_label = loc.pick("运行项目脚本", "Run project script");
        let scripts = menu_host(
            button("details-scripts-btn", scripts_label)
                .child(pro_icon(ProIcon::Play, 14., s.muted))
                .when(!small, |b| {
                    b.child(if strip {
                        loc.pick("脚本", "Scripts")
                    } else {
                        scripts_label
                    })
                })
                .lit(self.open_menu == Some(Menu::DetailsScripts))
                .on_press(cx, |this, _, cx| this.toggle_menu(Menu::DetailsScripts, cx)),
            self.render_details_scripts_menu(s, cx),
        );

        let mut controls = if strip {
            div().flex().items_center().gap(px(4.))
        } else {
            frosted_card(s).p(px(5.)).flex().flex_col().gap(px(2.))
        }
        .id("thread-details-card")
        .role(Role::Region)
        .aria_label(loc.pick("会话环境与 Git 详情", "Thread environment and Git details"))
        .w_full()
        .min_w_0()
        .occlude()
        .text_size(s.font_size(if strip { FS_META } else { FS_UI }))
        .text_color(s.text)
        .child(host_label);

        if self.git_snapshot.is_repo {
            let demo = matches!(self.access_mode, ChatAccessMode::DemoShowcase);
            let branch = if demo {
                "feat/offline-sync"
            } else {
                self.current_chat_id()
                    .and_then(|id| self.project_registry.worktree_for(id))
                    .map(|wt| wt.branch.as_str())
                    .or(self.git_snapshot.current_branch.as_deref())
                    .unwrap_or("HEAD")
            };
            let switch_branch = loc.pick("切换分支", "Switch branch");
            let branch_row = menu_host(
                button("details-branch-btn", switch_branch)
                    .child(render_git_branch_icon(14., s.muted))
                    .child(label(branch.to_owned()).max_w(px(if strip {
                        (width * 0.22).min(150.)
                    } else {
                        width - 72.
                    })))
                    .when(!small, |b| {
                        b.child(pro_icon(ProIcon::ChevronDown, 10., s.muted))
                    })
                    .aria(format!("{switch_branch} · {branch}"))
                    .tip(branch.to_owned())
                    .lit(self.open_menu == Some(Menu::DetailsBranch))
                    .on_press(cx, |this, _, cx| this.toggle_menu(Menu::DetailsBranch, cx)),
                self.render_git_branch_menu(
                    Menu::DetailsBranch,
                    &self.git_snapshot,
                    branch,
                    MenuPlacement::BelowEnd,
                    s,
                    cx,
                ),
            );
            let (added, deleted) = if demo {
                (71, 2)
            } else {
                self.git_snapshot.line_stats()
            };
            let changes_title = loc.pick("查看工作区变更", "View working tree changes");
            let count = self.git_snapshot.dirty_count();
            let changes = button("details-open-changes", changes_title)
                .child(render_file_diff_icon(14., s.muted))
                .when(!small, |b| b.child(loc.pick("变更", "Changes")))
                .when(!strip, |b| b.child(div().flex_1()))
                .font_features(tnum_features())
                .when(!strip && (added > 0 || deleted > 0), |b| {
                    b.child(div().text_color(s.pro.green_ink).child(format!("+{added}")))
                        .child(div().text_color(s.pro.red_ink).child(format!("-{deleted}")))
                })
                .when(strip || (added == 0 && deleted == 0), |b| {
                    b.child(count.to_string())
                })
                .on_press(cx, |this, _, cx| this.open_git_workbench_tab(cx));
            controls = controls
                .when(!strip, |r| {
                    r.child(div().mx(px(8.)).my(px(2.)).h(px(1.)).bg(s.line))
                })
                .child(branch_row)
                .child(changes);
        }
        controls
            .when(strip, |row| row.child(div().flex_1()))
            .child(open)
            .child(scripts)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{EDGE, Placement};

    #[test]
    fn side_card_fits_entirely_outside_text_at_every_reading_width() {
        use crate::prefs::ChatWidthMode;
        for pane in [340., 480., 723., 900., 1183., 1200., 1503., 2200.] {
            for mode in ChatWidthMode::ALL {
                let reading = mode.width_for_pane(pane);
                for scale in [0.9, 1., 1.25] {
                    match Placement::for_thread(pane, reading, scale) {
                        Placement::Margin { width } => {
                            let text_end = (pane + reading) / 2.;
                            assert!(pane - EDGE - width >= text_end + EDGE);
                            assert!(width >= 208. * scale);
                        }
                        Placement::Strip { width } => {
                            assert_eq!(width, pane);
                            assert!((pane - reading) / 2. < 208. * scale + 2. * EDGE);
                        }
                    }
                }
            }
        }
    }
}
