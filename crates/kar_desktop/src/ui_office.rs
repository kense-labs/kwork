//! The document sheet: scrollable tabs, unified selection & status strip, native Kantor editor,
//! and responsive co-editing inspector.

use crate::ui_motion::selection::{Group, Item, SelectionStrip};
use gpui::{prelude::*, *};
use kui::{
    components::pro_chrome::pro_fold,
    primitives::pro_icons::{ProIcon, pro_icon},
};

use crate::{
    coedit::{CoEditProposal, CoEditStatus, ScenarioKey, UserResolutionChoice},
    kar_bridge::KarCommand,
    ui_motion::{
        MenuPlacement, Tactile, close_button, icon_button, menu_divider, menu_hint, menu_host, menu_icon_item,
        primary_button, soft_button, text_button,
    },
    ui_primitives::{
        CTRL_H, CTRL_SM_H, DotTone, FS_META, FS_TAG, FS_UI, HEADER_H, ICON_SM, ICON_XS, LH_META,
        R_CARD, R_CTRL, R_PILL, StudioPalette, TITLEBAR_BUTTON_GAP, TITLEBAR_EDGE_INSET,
        clean_display_doc_name, macos_traffic_pad, mono_font,
        render_file_diff_icon, render_folder_icon,
        render_office_chip, render_sample_tag, tnum_features,
    },
    workbench::{KarWorkbench, Menu, RightPaneSurface, WorkspaceLayoutMode},
};

impl KarWorkbench {
    pub(crate) fn render_office_pane(
        &mut self,
        pane_w: f32,
        single_dock: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let s = StudioPalette::get(cx);

        let is_office_surface = self.right_surface == RightPaneSurface::Office;
        let active_tab = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .cloned();
        let active_is_demo =
            is_office_surface && active_tab.as_ref().is_some_and(|t| t.is_demo);
        let active_dirty =
            is_office_surface && active_tab.as_ref().is_some_and(|t| t.is_dirty(cx));
        let wide_canvas = self.layout_mode == WorkspaceLayoutMode::CanvasWide;

        // ── Unified Right-Dock Strip (`变更` · `文件` · `终端` + Open Docs/Code) ──
        let loc = self.ui_locale;
        let mut tabs = Vec::new();
        let git_sel = self.right_surface == RightPaneSurface::GitWorkbench;
        let files_sel = self.right_surface == RightPaneSurface::Files;
        let term_sel = self.right_surface == RightPaneSurface::Terminal;
        let tab_count = self.office_tabs.len()
            + self.code_tab_order.len()
            + usize::from(self.git_tab_open)
            + usize::from(self.files_tab_open)
            + usize::from(self.terminal_tab_open);
        let name_max_w = match tab_count {
            0 | 1 => 200.,
            2 => 148.,
            3 => 116.,
            _ => 96.,
        };
        for (surface, is_open, selected, label, glyph, key) in [
            (
                RightPaneSurface::GitWorkbench,
                self.git_tab_open,
                git_sel,
                loc.pick("变更", "Diff"),
                ProIcon::GitBranch,
                "dock-git",
            ),
            (
                RightPaneSurface::Files,
                self.files_tab_open,
                files_sel,
                loc.pick("文件", "Files"),
                ProIcon::Folder,
                "dock-files",
            ),
            (
                RightPaneSurface::Terminal,
                self.terminal_tab_open,
                term_sel,
                loc.pick("终端", "Terminal"),
                ProIcon::Terminal,
                "dock-terminal",
            ),
        ] {
            if !is_open && !selected {
                continue;
            }
            let closed = surface.clone();
            let icon = if surface == RightPaneSurface::GitWorkbench {
                render_file_diff_icon(13., s.text)
            } else {
                pro_icon(glyph, 13., s.text)
            };
            let close_label = format!("{} {label}", loc.pick("关闭", "Close"));
            tabs.push(
                Item::new(key, label, selected, cx, move |wb, _, cx| {
                    wb.open_surface(&surface, cx)
                })
                .content(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .child(icon)
                        .child(label),
                )
                .close(
                    close_button(SharedString::from(format!("{key}-close")), close_label, &s)
                        .on_press(cx, move |wb, _, cx| wb.close_surface(&closed, cx)),
                ),
            );
        }
        for (idx, tab) in self.office_tabs.iter().enumerate() {
            let selected = is_office_surface && self.active_office_tab == Some(idx);
            let name = clean_display_doc_name(&tab.doc_name(cx));
            let tab_id = tab.id;
            let label = div()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(render_office_chip(tab.kind, &s))
                .child(div().max_w(px(name_max_w)).truncate().child(name.clone()))
                .when(tab.is_dirty(cx), |d| {
                    d.child(div().size(px(6.)).rounded_full().bg(s.pro.amber))
                })
                .when(tab.is_demo && selected, |d| d.child(render_sample_tag(&s)));
            let close_label = format!("{} {name}", loc.pick("关闭", "Close"));
            tabs.push(
                Item::new(
                    format!("office-tab-{tab_id}"),
                    name,
                    selected,
                    cx,
                    move |wb, _, cx| wb.activate_office_tab(idx, cx),
                )
                .content(label)
                .close(
                    close_button(SharedString::from(format!("office-tab-close-{tab_id}")), close_label, &s)
                        .on_press(cx, move |wb, win, cx| wb.close_office_tab(tab_id, win, cx)),
                ),
            );
        }
        for rel_path in &self.code_tab_order {
            let selected = self.right_surface == RightPaneSurface::CodeFile(rel_path.clone());
            let label = rel_path
                .file_name()
                .unwrap_or(rel_path.as_os_str())
                .to_string_lossy()
                .into_owned();
            let select = rel_path.clone();
            let close = rel_path.clone();
            let key = format!("code-tab-{}", rel_path.display());
            let close_label = format!("{} {label}", loc.pick("关闭", "Close"));
            tabs.push(
                Item::new(
                    key.clone(),
                    label.clone(),
                    selected,
                    cx,
                    move |wb, _, cx| {
                        wb.right_surface = RightPaneSurface::CodeFile(select.clone());
                        cx.notify();
                    },
                )
                .content(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .child(pro_icon(ProIcon::Code, 13., s.pro.purple))
                        .child(div().max_w(px(name_max_w)).truncate().child(label)),
                )
                .close(
                    close_button(SharedString::from(format!("{key}-close")), close_label, &s)
                        .on_press(cx, move |wb, _, cx| wb.close_workspace_code_tab(&close, cx)),
                ),
            );
        }
        let tabs_scroll = SelectionStrip::new(
            "office-tabs-scroll",
            loc.pick("工作区标签页", "Workspace tabs"),
            Group::Documents,
            tabs,
            &s,
        );

        let add_doc_menu = menu_host(
            icon_button(
                "office-add-tab",
                ProIcon::Plus,
                loc.pick("打开面板", "Open a surface"),
                &s,
            )
            .lit(self.open_menu == Some(Menu::DocAdd))
            .on_press(cx, |this, _, cx| this.toggle_menu(Menu::DocAdd, cx)),
            self.render_menu(
                Menu::DocAdd,
                MenuPlacement::BelowStart,
                196.,
                self.surface_choices(&s, cx),
                &s,
                cx,
            ),
        );

        let more_doc_menu = menu_host(
            icon_button("office-more", ProIcon::MoreHorizontal, "文档操作", &s)
                .lit(self.open_menu == Some(Menu::DocMore))
                .on_press(cx, |this, _, cx| this.toggle_menu(Menu::DocMore, cx)),
            self.render_office_more_menu(&s, cx),
        );

        let header = div()
            .h(px(HEADER_H))
            .relative()
            .pl(px(if single_dock {
                macos_traffic_pad(window) + CTRL_SM_H + TITLEBAR_BUTTON_GAP
            } else { 10. }))
            .pr(px(TITLEBAR_EDGE_INSET + CTRL_SM_H + TITLEBAR_BUTTON_GAP))
            .bg(s.surface)
            .window_control_area(WindowControlArea::Drag)
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(tabs_scroll)
                    .child(add_doc_menu),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .when(active_is_demo, |d| {
                        d.child(
                            soft_button(
                                "office-coedit-toggle",
                                if self.coedit_panel_open {
                                    "收起演练"
                                } else {
                                    "协同演练"
                                },
                                &s,
                            )
                            .child(pro_icon(ProIcon::GitBranch, ICON_XS, s.muted))
                            .on_press(cx, |this, _, cx| {
                                this.coedit_panel_open = !this.coedit_panel_open;
                                cx.notify();
                            }),
                        )
                    })
                    .when(active_dirty && !active_is_demo, |d| {
                        d.child(
                            primary_button("office-save", "保存", &s)
                                .mr(px(2.))
                                .tip("保存修改  ⌘S")
                                .on_press(cx, |this, window, cx| {
                                    this.save_active_document(window, cx)
                                }),
                        )
                    })
                    .when(!single_dock, |d| {
                        d.child(icon_button(
                            "office-wide-toggle",
                            ProIcon::Expand,
                            if wide_canvas {
                                "恢复均衡分栏"
                            } else {
                                "加宽右侧面板"
                            },
                            &s,
                        )
                        .lit(wide_canvas)
                        .on_press(cx, |this, _, cx| this.toggle_wide_document(cx)))
                    })
                    .when(is_office_surface && active_tab.is_some() && !active_is_demo, |d| {
                        d.child(more_doc_menu)
                    }),
            )
            .child(div().absolute().left_0().right_0().bottom_0().h(px(1.)).bg(s.line));

        let body = match self.right_surface.clone() {
            RightPaneSurface::GitWorkbench => self.render_git_workbench_body(pane_w, &s, window, cx),
            RightPaneSurface::Files => self.render_files_dock_body(&s, cx),
            RightPaneSurface::Terminal => self.render_terminal_dock_body(&s, window, cx),
            RightPaneSurface::CodeFile(rel) => self.render_workspace_code_tab_body(&rel, pane_w, &s, window, cx),
            RightPaneSurface::Office => match active_tab {
                None => self.render_office_launcher(&s, cx),
                Some(tab) => {
                    let selection = tab.capture_selection(cx);
                    let already_pinned = self.pinned_target.as_ref().is_some_and(|p| {
                        p.tab_id == selection.tab_id && p.locator == selection.locator
                    });
                    let place = selection.locator.target_label();
                    let excerpt = selection.excerpt.replace('\n', " ");
                    let view_status = tab.status(cx);

                    let quote_btn = if already_pinned {
                        Tactile::ghost("office-quote", &s)
                            .flex_none()
                            .h(px(24.))
                            .px(px(8.))
                            .gap(px(5.))
                            .text_size(px(FS_META))
                            .text_color(s.pro.green_ink)
                            .tip("已关联在对话输入框上方，点击可更新快照")
                            .child(pro_icon(ProIcon::Check, ICON_XS, s.pro.green_ink))
                            .child(loc.pick("已引用", "Added to chat"))
                            .on_press(cx, |this, window, cx| {
                                this.bind_active_selection_to_chat(window, cx)
                            })
                    } else {
                        Tactile::soft("office-quote", &s)
                            .flex_none()
                            .h(px(24.))
                            .pl(px(8.))
                            .pr(px(10.))
                            .gap(px(5.))
                            .text_size(px(FS_META))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .tip(loc.pick("将当前选中的段落、单元格、幻灯片对象或 PDF 页放入对话", "Add the selected paragraph, cell, slide object, or PDF page to the chat draft"))
                            .child(pro_icon(ProIcon::Pin, ICON_XS, s.muted))
                            .child(loc.pick("引用到对话", "Add to chat"))
                            .on_press(cx, |this, window, cx| {
                                this.bind_active_selection_to_chat(window, cx)
                            })
                    };

                    let doc_key = clean_display_doc_name(&tab.doc_name(cx));
                    let live_rev = self.fence_ledger.current_revision(&doc_key);
                    let live_conflicts: Vec<_> = self
                        .fence_ledger
                        .conflicts
                        .iter()
                        .filter(|c| c.doc_key == doc_key)
                        .cloned()
                        .collect();

                    let strip = div()
                        .h(px(32.))
                        .pl(px(12.))
                        .pr(px(8.))
                        .border_b_1()
                        .border_color(s.line_soft)
                        .bg(s.stage.opacity(0.55))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex_none()
                                .px(px(6.))
                                .py(px(1.))
                                .rounded(px(R_PILL))
                                .bg(s.soft)
                                .font_family(mono_font(cx))
                                .font_features(tnum_features())
                                .text_size(px(FS_TAG))
                                .text_color(s.muted)
                                .child(format!("rev #{live_rev}")),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(FS_META))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(s.ink)
                                .child(place),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(FS_META))
                                .text_color(s.muted)
                                .child(if !view_status.is_empty() && view_status.as_ref() != "就绪" {
                                    format!("{view_status} · {excerpt}")
                                } else if excerpt.trim().is_empty() {
                                    "选中内容后可一键引用到对话".to_owned()
                                } else {
                                    excerpt
                                }),
                        )
                        .child(quote_btn);

                    let fence_banner = (!live_conflicts.is_empty())
                        .then(|| self.render_live_fence_conflicts(&live_conflicts, &s, cx));
                    let inspector = active_is_demo.then(|| self.render_coedit_inspector(&s, cx));
                    let inspector_open = self.coedit_panel_open && active_is_demo;

                    div()
                        .size_full()
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .child(strip)
                        .when_some(fence_banner, |d, fb| d.child(fb))
                        .child(pro_fold(
                            "kar-coedit-fold",
                            inspector_open,
                            236.,
                            0.,
                            move || inspector.unwrap_or_else(|| div().into_any_element()),
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .overflow_hidden()
                                .child(tab.any_view()),
                        )
                        .into_any_element()
                }
            },
        };

        div()
            .size_full()
            .bg(s.surface)
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(header)
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .into_any_element()
    }

    fn render_files_dock_body(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let loc = self.ui_locale;
        let ws = self.effective_workspace_dir();
        let ws_name = ws
            .file_name()
            .and_then(|n| n.to_str())
            .map_or("Kwork", crate::studio_hub::project_display_name)
            .to_owned();
        let show_hidden = self.tree_show_hidden;
        let ws_refresh = ws.clone();
        let ws_toggle = ws;

        let subbar = div()
            .h(px(36.))
            .px(px(12.))
            .border_b_1()
            .border_color(s.line_soft)
            .bg(s.stage.opacity(0.45))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(render_folder_icon(ICON_SM, s.muted))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .font_family(mono_font(cx))
                            .text_size(px(FS_META))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(s.ink)
                            .child(ws_name),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(
                        soft_button(
                            "files-dock-hidden-toggle",
                            if show_hidden {
                                loc.pick("隐藏点文件", "Hide dotfiles")
                            } else {
                                loc.pick("显示隐藏文件", "Show dotfiles")
                            },
                            s,
                        )
                        .lit(show_hidden)
                        .h(px(23.))
                        .px(px(7.))
                        .text_size(px(FS_TAG))
                        .on_press(cx, move |this, _, cx| {
                            this.tree_show_hidden = !this.tree_show_hidden;
                            this.prefs.tree_show_hidden = this.tree_show_hidden;
                            this.save_prefs();
                            this.bridge.send(KarCommand::RefreshWorkspaceTree {
                                workspace: ws_toggle.clone(),
                                show_hidden: this.tree_show_hidden,
                            });
                            cx.notify();
                        }),
                    )
                    .child(
                        icon_button(
                            "files-dock-refresh",
                            ProIcon::Refresh,
                            loc.pick("刷新文件树", "Refresh files"),
                            s,
                        )
                        .on_press(cx, move |this, _, _| {
                            this.bridge.send(KarCommand::RefreshWorkspaceTree {
                                workspace: ws_refresh.clone(),
                                show_hidden,
                            });
                        }),
                    ),
            );

        let tree_body = if self.workspace_tree_view.is_some() && self.workspace_tree.is_empty() {
            crate::ui_primitives::pane_empty(
                render_folder_icon(20., s.muted),
                loc.pick("没有可显示的文件", "No files to show"),
                if show_hidden {
                    loc.pick(
                        "添加文件后，点击刷新即可查看。",
                        "Add a file and refresh the list.",
                    )
                } else {
                    loc.pick(
                        "可以显示隐藏文件，或添加文件后刷新。",
                        "Show dotfiles or add a file and refresh.",
                    )
                },
                s,
            )
            .into_any_element()
        } else if let Some(tree_entity) = self.workspace_tree_view.clone() {
            div()
                .id("kar-files-dock-scroll")
                .size_full()
                .overflow_y_scroll()
                .px(px(6.))
                .py(px(4.))
                .child(tree_entity)
                .into_any_element()
        } else {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(FS_META))
                .text_color(s.muted)
                .child(loc.pick("正在读取文件…", "Reading workspace files…"))
                .into_any_element()
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(subbar)
            .child(div().flex_1().min_h_0().overflow_hidden().child(tree_body))
            .into_any_element()
    }

    fn render_office_more_menu(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        let active_id = self
            .active_office_tab
            .and_then(|i| self.office_tabs.get(i))
            .map(|t| t.id);
        let rows: Vec<AnyElement> = vec![
            menu_icon_item("doc-more-save", ProIcon::Check, "保存", s)
                .child(menu_hint("⌘S", s))
                .on_press(cx, |this, window, cx| {
                    this.open_menu = None;
                    this.save_active_document(window, cx);
                })
                .into_any_element(),
            menu_icon_item("doc-more-save-as", ProIcon::Folder, "另存为…", s)
                .on_press(cx, |this, window, cx| {
                    this.prompt_save_as_active_document(window, cx);
                })
                .into_any_element(),
            menu_icon_item("doc-more-copy", ProIcon::Copy, "快速保存副本", s)
                .on_press(cx, |this, window, cx| {
                    this.save_copy_of_active_document(window, cx)
                })
                .into_any_element(),
            menu_icon_item("doc-more-quick-edit", ProIcon::Sliders, "按位置精确修改…", s)
                .on_press(cx, |this, window, cx| this.open_quick_edit_modal(window, cx))
                .into_any_element(),
            menu_divider(s).into_any_element(),
            menu_icon_item("doc-more-close", ProIcon::Close, "关闭当前文档", s)
                .on_press(cx, move |this, window, cx| {
                    this.open_menu = None;
                    if let Some(id) = active_id {
                        this.close_office_tab(id, window, cx);
                    }
                })
                .into_any_element(),
        ];
        self.render_menu(Menu::DocMore, MenuPlacement::BelowEnd, 212., rows, s, cx)
    }

    fn surface_choices(&self, s: &StudioPalette, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let loc = self.ui_locale;
        let mut rows: Vec<AnyElement> = [
            (
                RightPaneSurface::Terminal,
                ProIcon::Terminal,
                loc.pick("终端", "Terminal"),
            ),
            (
                RightPaneSurface::Files,
                ProIcon::Folder,
                loc.pick("文件", "Files"),
            ),
            (
                RightPaneSurface::GitWorkbench,
                ProIcon::GitBranch,
                loc.pick("变更", "Diff"),
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (surface, icon, name))| {
            menu_icon_item(("choose-surface", i), icon, name, s)
                .on_press(cx, move |this, _, cx| this.open_surface(&surface, cx))
                .into_any_element()
        })
        .collect();
        rows.push(
            menu_icon_item(
                "surface-doc-library",
                ProIcon::PageDoc,
                loc.pick("文档", "Documents"),
                s,
            )
            .on_press(cx, |this, _, cx| {
                this.close_all_overlays(cx);
                this.open_office_gallery_stage(cx);
            })
            .into_any_element(),
        );
        rows.push(
            menu_icon_item(
                "surface-open-document",
                ProIcon::Folder,
                loc.pick("打开文件…", "Open file…"),
                s,
            )
            .on_press(cx, |this, win, cx| {
                this.close_all_overlays(cx);
                this.pick_documents(win, cx);
            })
            .into_any_element(),
        );
        rows.push(menu_divider(s).into_any_element());
        rows.push(
            menu_icon_item(
                "surface-chat-commands",
                ProIcon::Bolt,
                loc.pick("对话指令…", "Chat commands…"),
                s,
            )
            .disabled(
                !self.access_mode.is_writable()
                    && !matches!(self.access_mode, crate::types::ChatAccessMode::NoneSelected),
            )
            .on_press(cx, |this, win, cx| this.open_slash_picker(win, cx))
            .into_any_element(),
        );
        rows
    }

    fn render_office_launcher(&self, s: &StudioPalette, cx: &mut Context<Self>) -> AnyElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .max_w(px(328.))
                    .px(px(20.))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .pb(px(12.))
                            .text_center()
                            .text_size(px(FS_UI))
                            .font_weight(FontWeight::MEDIUM)
                            .child(self.ui_locale.pick("打开面板", "Open a surface")),
                    )
                    .children(self.surface_choices(s, cx)),
            )
            .into_any_element()
    }

    /// Co-editing sample drawer: responsive `flex_wrap` tiles so narrow split panes never crush text.
    pub(crate) fn render_coedit_inspector(
        &mut self,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sc = self.active_coedit_scenario;
        if let Some(live) = self.read_live_coedit_state(sc, cx) {
            let _ = self.coedit_engine.sync_live_state(&live);
        }
        let Some(prop): Option<CoEditProposal> = self
            .coedit_engine
            .proposals()
            .iter()
            .find(|pr| pr.scenario == sc)
            .cloned()
        else {
            return div().into_any_element();
        };
        let op_id = prop.operation_id;

        let moves: Vec<(&'static str, &'static str, &'static str, bool)> = match sc {
            ScenarioKey::WordSameParagraph => vec![
                ("sim-word-date", "改日期 → 12-01", "word_human_date", false),
                (
                    "sim-word-mid",
                    "解决中途再改一次",
                    "word_mid_conflict_edit",
                    false,
                ),
            ],
            ScenarioKey::ExcelSameCellConflict => vec![
                ("sim-excel-b3", "改 B3 → 135,000", "excel_human_b3", false),
                (
                    "sim-excel-mid",
                    "解决中途再改一次",
                    "excel_mid_conflict_edit",
                    false,
                ),
            ],
            ScenarioKey::PptMoveAndText => vec![
                ("sim-ppt-move", "移位文本框", "ppt_human_move", false),
                (
                    "sim-ppt-mid",
                    "解决中途再移一次",
                    "ppt_mid_conflict_move",
                    false,
                ),
                ("sim-ppt-del", "删除文本框", "ppt_human_delete", true),
            ],
        };

        let mut sim_row = div().flex().flex_wrap().items_center().gap(px(6.)).child(
            div()
                .mr(px(2.))
                .text_size(px(FS_META))
                .text_color(s.muted)
                .child("模拟人工操作"),
        );
        for (id, label, step, danger) in moves {
            let btn = if danger {
                Tactile::soft(id, s)
                    .fills(s.pro.red_soft, s.pro.red.opacity(0.16))
                    .h(px(26.))
                    .px(px(9.))
                    .text_size(px(FS_META))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.pro.red_ink)
                    .child(label)
            } else {
                Tactile::soft(id, s)
                    .fills(s.card, s.hover)
                    .h(px(26.))
                    .px(px(9.))
                    .text_size(px(FS_META))
                    .text_color(s.ink)
                    .child(label)
            };
            sim_row = sim_row.child(btn.on_press(cx, move |this, window, cx| {
                this.trigger_coedit_human_step(step, window, cx);
            }));
        }

        let is_deleted = prop.status == CoEditStatus::TargetDeletedConflict;
        let merged = prop.merged_preview.clone().filter(|_| !is_deleted);
        let (third_title, third_tone) = if is_deleted {
            ("智能体提议", DotTone::Danger)
        } else if merged.is_some() {
            ("合并预览", DotTone::Success)
        } else {
            ("智能体提议", DotTone::Warning)
        };
        let third_value = merged.unwrap_or_else(|| prop.proposed_value.clone());

        let versions = div()
            .w_full()
            .flex()
            .flex_wrap()
            .items_stretch()
            .gap(px(8.))
            .child(render_version_tile(
                "基准内容",
                &prop.base_value,
                DotTone::Neutral,
                false,
                s,
            ))
            .child(render_version_tile(
                "当前画布",
                &prop.current_value,
                if is_deleted {
                    DotTone::Danger
                } else {
                    DotTone::Info
                },
                false,
                s,
            ))
            .child(render_version_tile(
                third_title,
                &third_value,
                third_tone,
                true,
                s,
            ));

        let summary = self
            .coedit_feedback
            .clone()
            .or_else(|| prop.conflict_reason.clone())
            .unwrap_or_else(|| "两处修改互不冲突，可同时保留。".into());

        let decision = match prop.status {
            CoEditStatus::StaleNeedsReconfirm {
                latest_revision, ..
            } => status_strip(
                DotTone::Warning,
                "解决期间画布又发生了修改，请先核对上方最新内容。".into(),
                s,
            )
            .child(
                primary_button("btn-coedit-reconfirm", "已核对最新内容", s).on_press(
                    cx,
                    move |this, _, cx| {
                        let _ = this
                            .coedit_engine
                            .acknowledge_latest_revision(op_id, latest_revision);
                        this.coedit_feedback = Some("已确认最新版本，可以继续选择裁决方式。".into());
                        cx.notify();
                    },
                ),
            ),
            CoEditStatus::ReadyDualKeep => div()
                .w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(180.))
                        .text_size(px(FS_META))
                        .line_height(px(LH_META))
                        .text_color(s.text)
                        .child(summary),
                )
                .child(
                    primary_button("btn-coedit-apply-dual", "两项都保留", s)
                        .child(pro_icon(ProIcon::Check, ICON_XS, s.on_primary))
                        .on_press(cx, move |this, window, cx| {
                            this.submit_coedit_choice(
                                op_id,
                                UserResolutionChoice::ApplyDualKeep,
                                window,
                                cx,
                            );
                        }),
                ),
            CoEditStatus::ConflictNeedsChoice => div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(FS_META))
                        .line_height(px(LH_META))
                        .text_color(s.pro.amber_ink)
                        .child(summary),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    soft_button("btn-coedit-keep", "保留当前值", s)
                                        .fills(s.card, s.hover)
                                        .on_press(cx, move |this, window, cx| {
                                            this.submit_coedit_choice(
                                                op_id,
                                                UserResolutionChoice::KeepCurrent,
                                                window,
                                                cx,
                                            );
                                        }),
                                )
                                .child(
                                    soft_button("btn-coedit-adopt", "采用智能体提议", s)
                                        .fills(s.card, s.hover)
                                        .on_press(cx, move |this, window, cx| {
                                            this.submit_coedit_choice(
                                                op_id,
                                                UserResolutionChoice::AdoptProposal,
                                                window,
                                                cx,
                                            );
                                        }),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .child(
                                    div()
                                        .w(px(124.))
                                        .h(px(CTRL_H))
                                        .px(px(8.))
                                        .rounded(px(R_CTRL))
                                        .bg(s.card)
                                        .border_1()
                                        .border_color(s.line)
                                        .flex()
                                        .items_center()
                                        .child(self.coedit_manual_editor.clone()),
                                )
                                .child(
                                    primary_button("btn-coedit-manual-submit", "手工合并", s)
                                        .on_press(cx, move |this, window, cx| {
                                            let val = this
                                                .coedit_manual_editor
                                                .read(cx)
                                                .value()
                                                .trim()
                                                .to_owned();
                                            this.submit_coedit_choice(
                                                op_id,
                                                UserResolutionChoice::ManualMerge {
                                                    merged_value: val,
                                                },
                                                window,
                                                cx,
                                            );
                                        }),
                                ),
                        ),
                ),
            CoEditStatus::TargetDeletedConflict => {
                status_strip(DotTone::Danger, summary, s).child(self.coedit_dismiss_morph.clone())
            }
            CoEditStatus::OutcomeUnknown { reason, .. } => status_strip(
                DotTone::Warning,
                format!("写入回执未确认（OutcomeUnknown）：{reason} · 请先核对内容，切勿盲目自动重放"),
                s,
            ),
            CoEditStatus::Resolved { summary } => status_strip(DotTone::Success, summary, s),
        };

        div()
            .id("coedit-inspector-panel")
            .max_h(px(280.))
            .overflow_y_scroll()
            .px(px(14.))
            .py(px(10.))
            .bg(s.stage)
            .border_b_1()
            .border_color(s.line)
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .text_size(px(FS_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child("人机并发双写裁决"),
                            )
                            .child(render_sample_tag(s)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(SelectionStrip::options(
                                "coedit-scenario-strip",
                                "示例场景",
                                Group::Choices,
                                [
                                    (ScenarioKey::WordSameParagraph, "Word"),
                                    (ScenarioKey::ExcelSameCellConflict, "Excel"),
                                    (ScenarioKey::PptMoveAndText, "PPT"),
                                ],
                                self.active_coedit_scenario,
                                s,
                                cx,
                                |wb, sc, win, cx| {
                                    wb.switch_coedit_scenario(sc, win, cx);
                                },
                            ))
                            .child(
                                text_button("coedit-reset-all", "重置", s)
                                    .child(pro_icon(ProIcon::Refresh, ICON_XS, s.muted))
                                    .on_press(cx, |this, window, cx| {
                                        this.trigger_coedit_human_step("reset_all", window, cx);
                                    }),
                            ),
                    ),
            )
            .child(sim_row)
            .child(versions)
            .child(decision)
            .into_any_element()
    }

    fn render_live_fence_conflicts(
        &self,
        conflicts: &[crate::coedit::LiveFencedConflict],
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut col = div()
            .w_full()
            .px(px(12.))
            .py(px(8.))
            .bg(s.pro.amber_soft)
            .border_b_1()
            .border_color(s.line)
            .flex()
            .flex_col()
            .gap(px(6.));
        for c in conflicts {
            let op_keep = c.operation_id;
            let op_apply = c.operation_id;
            col = col.child(
                div()
                    .w_full()
                    .p(px(10.))
                    .rounded(px(R_CARD))
                    .bg(s.card)
                    .border_1()
                    .border_color(s.line)
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(200.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .text_size(px(FS_META))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.pro.amber_ink)
                                    .child(pro_icon(ProIcon::Shield, ICON_XS, s.pro.amber))
                                    .child(format!(
                                        "Revision Fence 拦截（期望 rev #{} · 当前 rev #{}）· {}",
                                        c.expected_revision, c.live_revision, c.target_desc
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(px(FS_META))
                                    .text_color(s.text)
                                    .child(format!(
                                        "当前内容：“{}”  →  智能体提案：“{}”",
                                        c.current_summary, c.proposed_summary
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                soft_button(
                                    SharedString::from(format!("fence-keep-{op_keep}")),
                                    "保留人工版本",
                                    s,
                                )
                                .on_press(cx, move |this, window, cx| {
                                    this.resolve_fenced_conflict(op_keep, false, window, cx);
                                }),
                            )
                            .child(
                                primary_button(
                                    SharedString::from(format!("fence-apply-{op_apply}")),
                                    "采用智能体修改",
                                    s,
                                )
                                .on_press(cx, move |this, window, cx| {
                                    this.resolve_fenced_conflict(op_apply, true, window, cx);
                                }),
                            ),
                    ),
            );
        }
        col.into_any_element()
    }
}

fn status_strip(tone: DotTone, text: String, s: &StudioPalette) -> Div {
    let (bg, ink, icon) = match tone {
        DotTone::Success => (s.pro.green_soft, s.pro.green_ink, ProIcon::Check),
        DotTone::Danger => (s.pro.red_soft, s.pro.red_ink, ProIcon::AlertTriangle),
        _ => (s.pro.amber_soft, s.pro.amber_ink, ProIcon::AlertTriangle),
    };
    div()
        .w_full()
        .px(px(10.))
        .py(px(7.))
        .rounded(px(R_CTRL))
        .bg(bg)
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.))
        .child(pro_icon(icon, ICON_SM, ink))
        .child(
            div()
                .flex_1()
                .min_w(px(160.))
                .text_size(px(FS_META))
                .line_height(px(LH_META))
                .text_color(ink)
                .child(text),
        )
}

fn render_version_tile(
    heading: &'static str,
    value: &str,
    tone: DotTone,
    outcome: bool,
    s: &StudioPalette,
) -> Div {
    let bg = if outcome {
        match tone {
            DotTone::Success => s.pro.green_soft,
            DotTone::Warning => s.pro.amber_soft,
            DotTone::Danger => s.pro.red_soft,
            _ => s.card,
        }
    } else {
        s.card
    };

    div()
        .flex_1()
        .min_w(px(148.))
        .p(px(10.))
        .rounded(px(R_CTRL))
        .bg(bg)
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(FS_META))
                .font_weight(FontWeight::MEDIUM)
                .text_color(tone.text_color(s))
                .child(div().size(px(6.)).rounded_full().bg(tone.dot_color(s)))
                .child(heading),
        )
        .child(
            div()
                .text_size(px(FS_META))
                .line_height(px(LH_META))
                .text_color(s.ink)
                .child(if value.is_empty() {
                    "（已删除）".to_owned()
                } else {
                    value.to_owned()
                }),
        )
}
