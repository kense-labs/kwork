//! Searchable workspace skills, commands, MCP servers, and tool schemas.

use crate::ui_motion::selection::{Group, SelectionStrip};
use gpui::{prelude::*, *};
use kui::{
    components::{
        code_block::CodeBlock,
        json_viewer::JsonViewer,
    },
    primitives::pro_icons::{ProIcon, pro_icon},
};

use crate::{
    studio_hub::InventoryTab,
    ui_motion::{Tactile, icon_button, pulse_beacon, soft_button},
    ui_primitives::{
        CTRL_H, FS_CONTROL, FS_META, FS_TAG, FS_UI, R_PILL, SkillCardGlyph,
        StudioPalette, card_box, mono_font, search_input_box, settings_page_shell,
    },
    workbench::KarWorkbench,
};

fn scope_label(scope: &str, locale: crate::ui_primitives::UiLocale) -> String {
    match scope {
        "项目技能" => locale.pick("项目技能", "Project"),
        "用户全局" => locale.pick("用户全局", "User"),
        "内置技能" => locale.pick("内置技能", "Built-in"),
        "托管插件" => locale.pick("托管插件", "Managed"),
        "已发现" => locale.pick("已发现", "Discovered"),
        other => other,
    }.to_owned()
}

fn skill_accent_for(ix: usize, s: &StudioPalette) -> Hsla {
    let tones = [
        s.pro.blue,
        s.pro.purple,
        s.pro.teal,
        s.pro.orange,
        s.pro.green,
        s.pro.cyan,
    ];
    tones[ix % tones.len()]
}

impl KarWorkbench {
    pub(crate) fn render_skills_mcp_panel(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let narrow = f32::from(window.viewport_size().width) < 900.;
        let loc = self.ui_locale;
        let inv = &self.workspace_inventory;
        let query = self.skills_filter_query.trim().to_lowercase();

        // ── 1. Gliding Category Tab Strip + Search Input ────────────────────
        let options = InventoryTab::ALL.into_iter().map(|tab| {
            let (english, count) = match tab {
                InventoryTab::All => (
                    "All",
                    inv.skills.len() + inv.mcp_servers.len() + inv.commands.len() + inv.tools.len(),
                ),
                InventoryTab::Skills => ("Skills", inv.skills.len()),
                InventoryTab::McpServers => ("MCP", inv.mcp_servers.len()),
                InventoryTab::Commands => ("Commands", inv.commands.len()),
                InventoryTab::Tools => ("Tools", inv.tools.len()),
            };
            (tab, format!("{}  {count}", loc.pick(tab.label(), english)))
        });
        let tabs_row = SelectionStrip::options(
            "inventory-views",
            loc.pick("技能与工具类别", "Inventory categories"),
            Group::Tabs,
            options,
            self.active_inventory_tab,
            s,
            cx,
            |wb, tab, _, cx| {
                wb.selected_skill_inspector = None;
                wb.active_inventory_tab = tab;
                cx.notify();
            },
        );

        let filter_bar = div()
            .px(px(20.))
            .py(px(10.))
            .border_b_1()
            .border_color(s.line_soft)
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(8.))
            .flex_wrap()
            .child(tabs_row)
            .child(
                search_input_box(
                    "skills-search-box",
                    self.skills_search_editor.clone(),
                    self.skills_search_editor.read(cx).focus.is_focused(window),
                    true,
                    s,
                )
                .flex_1()
                .min_w(px(140.))
                .max_w(px(216.)),
            );

        let show_skills = matches!(
            self.active_inventory_tab,
            InventoryTab::All | InventoryTab::Skills
        );
        let show_mcp = matches!(
            self.active_inventory_tab,
            InventoryTab::All | InventoryTab::McpServers
        );
        let show_cmds = matches!(
            self.active_inventory_tab,
            InventoryTab::All | InventoryTab::Commands
        );
        let show_tools = matches!(
            self.active_inventory_tab,
            InventoryTab::Tools | InventoryTab::All
        );

        let filtered_skills: Vec<_> = if show_skills {
            inv.skills
                .iter()
                .filter(|sk| {
                    query.is_empty()
                        || sk.name.to_lowercase().contains(&query)
                        || sk.description.to_lowercase().contains(&query)
                })
                .cloned()
                .collect()
        } else {
            Vec::new()
        };

        let filtered_tools: Vec<_> = if show_tools {
            inv.tools
                .iter()
                .filter(|t| {
                    query.is_empty()
                        || t.name.to_lowercase().contains(&query)
                        || t.description.to_lowercase().contains(&query)
                        || t.server_id.to_lowercase().contains(&query)
                })
                .cloned()
                .collect()
        } else {
            Vec::new()
        };

        let active_inspector_key = self
            .selected_skill_inspector
            .as_ref()
            .filter(|selected| {
                filtered_skills
                    .iter()
                    .any(|s| **selected == format!("skill:{}", s.name))
                    || filtered_tools
                        .iter()
                        .any(|t| **selected == format!("tool:{}:{}", t.server_id, t.name))
            })
            .cloned();
        let show_list = !narrow || active_inspector_key.is_none();
        let can_inject = self.access_mode.is_writable()
            || matches!(
                self.access_mode,
                crate::types::ChatAccessMode::NoneSelected
            );

        let mut matched = 0usize;
        let mut body = settings_page_shell();

        // ── Section 1: MCP Servers (`integrations.rs` Sculpted Cards) ────────
        if show_mcp {
            let mcps: Vec<_> = inv
                .mcp_servers
                .iter()
                .filter(|m| {
                    query.is_empty()
                        || m.name.to_lowercase().contains(&query)
                        || m.id.to_lowercase().contains(&query)
                })
                .cloned()
                .collect();
            matched += mcps.len();
            if !mcps.is_empty() {
                let mut sec = div().w_full().flex().flex_col().gap(px(8.)).child(
                    div()
                        .text_size(px(FS_META))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(s.muted)
                        .child(loc.pick("已连接的 MCP 服务", "Connected MCP servers")),
                );
                for (m_ix, srv) in mcps.into_iter().enumerate() {
                    let accent = skill_accent_for(m_ix + 2, s);
                    sec = sec.child(
                        card_box(s)
                            .p(px(12.))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(10.))
                                    .child(
                                        SkillCardGlyph::from_name(&srv.name).render_tile(32., accent),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap(px(2.5))
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap(px(6.))
                                                    .child(pulse_beacon(
                                                        if srv.error.is_some() {
                                                            s.pro.amber
                                                        } else {
                                                            s.pro.green
                                                        },
                                                        6.,
                                                        false,
                                                        window,
                                                        cx,
                                                    ))
                                                    .child(
                                                        div()
                                                            .text_size(px(FS_UI))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(s.ink)
                                                            .truncate()
                                                            .child(srv.name),
                                                    )
                                                    .child(
                                                        div()
                                                            .px(px(6.))
                                                            .py(px(1.))
                                                            .rounded(px(R_PILL))
                                                            .bg(s.tray)
                                                            .font_family(mono_font(cx))
                                                            .text_size(px(FS_TAG))
                                                            .text_color(s.muted)
                                                            .child(srv.id.clone()),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(FS_META))
                                                    .text_color(s.muted)
                                                    .truncate()
                                                    .child(format!(
                                                        "{} · {}",
                                                        if srv.builtin_office {
                                                            loc.pick("可连接", "Available")
                                                        } else if srv.status == "已配置" {
                                                            loc.pick("已配置", "Configured")
                                                        } else { &srv.status },
                                                        if srv.builtin_office {
                                                            loc.pick("Kwrok 桌面端", "Kwrok desktop")
                                                        } else { &srv.source }
                                                    )),
                                            ),
                                    ),
                            )
                            .child(
                                soft_button(
                                    SharedString::from(format!("mcp-view-tools-{}", srv.id)),
                                    format!("{} {}", srv.tool_count, loc.pick("个工具", "tools")),
                                    s,
                                )
                                .disabled(!inv.tools.iter().any(|t| t.server_id == srv.id))
                                .on_press(
                                    cx,
                                    move |this, window, cx| {
                                        this.active_inventory_tab = InventoryTab::Tools;
                                        this.skills_search_editor.update(cx, |ed, cx| {
                                            ed.set_value(srv.id.clone(), window, cx)
                                        });
                                        this.skills_filter_query = srv.id.clone();
                                        cx.notify();
                                    },
                                ),
                            ),
                    );
                }
                body = body.child(sec);
            }
        }

        // ── Section 2: 2-Column Sculpted Skills Grid (`agent_skills.rs`) ─────
        if !filtered_skills.is_empty() {
            matched += filtered_skills.len();
            let mut sec = div().w_full().flex().flex_col().gap(px(8.)).child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.muted)
                            .child(loc.pick("工作区与全局技能", "Workspace and global skills")),
                    )
                    .child(
                        div()
                            .text_size(px(FS_TAG))
                            .text_color(s.faint)
                            .child(loc.pick("选择技能查看详情", "Select a skill for details")),
                    ),
            );

            for (r_ix, pair) in filtered_skills
                .chunks(if narrow { 1 } else { 2 })
                .enumerate()
            {
                let mut row = div().w_full().flex().gap(px(10.));
                for (c_ix, sk) in pair.iter().cloned().enumerate() {
                    let ix = r_ix * 2 + c_ix;
                    let sk_key = format!("skill:{}", sk.name);
                    let is_sel = active_inspector_key.as_deref() == Some(sk_key.as_str());
                    let accent = skill_accent_for(ix, s);
                    let sk_token = format!("${} ", sk.name);
                    let sk_key_click = sk_key.clone();
                    let can_use = sk.enabled && can_inject;

                    row = row.child(
                        card_box(s)
                            .id(("skill-sculpted-card", ix))
                            .flex_1()
                            .min_w_0()
                            .p(px(12.))
                            .border_color(if is_sel {
                                s.ink.opacity(0.42)
                            } else {
                                s.line_soft
                            })
                            .cursor_pointer()
                            .hover(|st| st.bg(s.hover).border_color(s.line_strong))
                            .active(|st| st.bg(s.active))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected_skill_inspector = Some(sk_key_click.clone());
                                cx.notify();
                            }))
                            .flex()
                            .flex_col()
                            .justify_between()
                            .gap(px(9.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(7.))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .gap(px(8.))
                                            .child(
                                                SkillCardGlyph::from_name(&sk.name)
                                                    .render_tile(32., accent),
                                            )
                                            .child(
                                                div()
                                                    .min_w_0()
                                                    .max_w(px(132.))
                                                    .px(px(6.))
                                                    .py(px(1.5))
                                                    .rounded(px(R_PILL))
                                                    .bg(s.tray)
                                                    .font_family(mono_font(cx))
                                                    .text_size(px(FS_TAG))
                                                    .text_color(s.muted)
                                                    .truncate()
                                                    .child(format!("${}", sk.name)),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(FS_UI))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(s.ink)
                                            .truncate()
                                            .child(sk.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .line_clamp(2)
                                            .text_size(px(FS_META))
                                            .line_height(px(16.5))
                                            .text_color(s.muted)
                                            .child(sk.description.clone()),
                                    ),
                            )
                            .child(
                                div()
                                    .pt(px(4.))
                                    .border_t_1()
                                    .border_color(s.line_soft)
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(FS_TAG))
                                            .text_color(s.faint)
                                            .truncate()
                                            .child(scope_label(&sk.scope_label, loc)),
                                    )
                                    .child(
                                        soft_button(
                                            ("skill-use", ix),
                                            if sk.enabled {
                                                loc.pick("填入草稿", "Add to draft")
                                            } else {
                                                loc.pick("已停用", "Disabled")
                                            },
                                            s,
                                        )
                                        .disabled(!can_use)
                                        .on_press(cx, move |wb, win, cx| {
                                            wb.inject_token_into_composer(&sk_token, win, cx);
                                        }),
                                    ),
                            ),
                    );
                }
                if !narrow && pair.len() == 1 {
                    row = row.child(div().flex_1().min_w_0());
                }
                sec = sec.child(row);
            }
            body = body.child(sec);
        }

        // ── Section 3: Slash Commands ────────────────────────────────────────
        if show_cmds {
            let cmds: Vec<_> = inv
                .commands
                .iter()
                .filter(|c| {
                    query.is_empty()
                        || c.name.to_lowercase().contains(&query)
                        || c.description.to_lowercase().contains(&query)
                })
                .cloned()
                .collect();
            matched += cmds.len();
            if !cmds.is_empty() {
                let mut sec = div().w_full().flex().flex_col().gap(px(6.)).child(
                    div()
                        .text_size(px(FS_META))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(s.muted)
                        .child(loc.pick("快捷斜杠指令", "Slash commands")),
                );
                for (ix, cmd) in cmds.into_iter().enumerate() {
                    let token = format!("{} ", cmd.name);
                    sec = sec.child(
                        card_box(s)
                            .px(px(12.))
                            .py(px(8.5))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .font_family(mono_font(cx))
                                            .text_size(px(FS_META))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(s.ink)
                                            .child(cmd.name),
                                    )
                                    .when_some(cmd.hint, |r, h| {
                                        r.child(
                                            div()
                                                .text_size(px(FS_TAG))
                                                .text_color(s.faint)
                                                .child(h),
                                        )
                                    })
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(FS_META))
                                            .text_color(s.muted)
                                            .child(cmd.description),
                                    ),
                            )
                            .child(soft_button(("cmd-inject", ix), loc.pick("填入", "Insert"), s).disabled(!can_inject).on_press(
                                cx,
                                move |this, win, cx| {
                                    this.inject_token_into_composer(&token, win, cx);
                                },
                            )),
                    );
                }
                body = body.child(sec);
            }
        }

        // ── Section 4: Tool Schemas (selects into Right Inspector!) ──────────
        if !filtered_tools.is_empty() {
            matched += filtered_tools.len();
            let mut sec = div().w_full().flex().flex_col().gap(px(7.)).child(
                div()
                    .text_size(px(FS_META))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(s.muted)
                    .child(loc.pick("MCP 与宿主工具参数定义", "Tool schemas")),
            );
            for (ix, t) in filtered_tools.iter().cloned().enumerate() {
                let tkey = format!("tool:{}:{}", t.server_id, t.name);
                let is_sel = active_inspector_key.as_deref() == Some(tkey.as_str());
                let tkey_click = tkey.clone();
                sec = sec.child(
                    card_box(s)
                        .id(("tool-schema-card", ix))
                        .p(px(10.))
                        .border_color(if is_sel {
                            s.ink.opacity(0.42)
                        } else {
                            s.line_soft
                        })
                        .cursor_pointer()
                        .hover(|st| st.bg(s.hover).border_color(s.line_strong))
                        .active(|st| st.bg(s.active))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.selected_skill_inspector = Some(tkey_click.clone());
                            cx.notify();
                        }))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(6.))
                                        .child(
                                            div()
                                                .font_family(mono_font(cx))
                                                .text_size(px(FS_META))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(s.ink)
                                                .truncate()
                                                .child(t.name),
                                        )
                                        .child(
                                            div()
                                                .px(px(6.))
                                                .py(px(1.))
                                                .rounded(px(R_PILL))
                                                .bg(s.tray)
                                                .text_size(px(FS_TAG))
                                                .text_color(s.muted)
                                                .child(t.server_id),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(FS_META))
                                        .text_color(s.muted)
                                        .truncate()
                                        .child(t.description),
                                ),
                        )
                        .child(pro_icon(ProIcon::ChevronRight, 12.5, s.muted)),
                );
            }
            body = body.child(sec);
        }

        if matched == 0 {
            let (title, detail) = if query.is_empty() {
                (
                    loc.pick("暂无可用项目", "Nothing available"),
                    loc.pick("当前智能体尚未提供这个分类的内容。", "This agent has no entries in this category."),
                )
            } else {
                (
                    loc.pick("没有匹配结果", "No matches"),
                    loc.pick("可以缩短关键词，或切换到「全部」搜索其他分类。", "Try a shorter term or search All."),
                )
            };
            body = body.child(crate::ui_primitives::pane_empty(
                pro_icon(ProIcon::Search, crate::ui_primitives::ICON, s.muted),
                title,
                detail,
                s,
            ));
        }

        // ── Right-Hand Master-Detail Inspector (`w(258px)`) ──────────────────
        let inspector: Option<Div> = active_inspector_key.and_then(|key| {
            if let Some(sk_name) = key.strip_prefix("skill:") {
                let sk = inv.skills.iter().find(|item| item.name == sk_name)?;
                let md_text = sk
                    .preview_markdown
                    .clone()
                    .unwrap_or_else(|| format!("# {}\n\n{}", sk.name, sk.description));
                let cache_key = format!("skill-md-{}", sk.name);
                let md_block = self
                    .code_block_cache
                    .entry(cache_key)
                    .or_insert_with(|| {
                        cx.new(|cx| {
                            CodeBlock::new("SKILL.md", "Markdown", md_text, cx).max_lines(14)
                        })
                    })
                    .clone();
                let sk_token = format!("${} ", sk.name);
                let can_use = sk.enabled && can_inject;
                Some(
                    div()
                        .map(|d| if narrow { d.w_full() } else { d.w(px(258.)) })
                        .h_full()
                        .flex_none()
                        .border_l_1()
                        .border_color(s.line_soft)
                        .bg(s.tray.opacity(if s.dark { 0.42 } else { 0.55 }))
                        .p(px(15.))
                        .flex()
                        .flex_col()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .id("kar-skill-inspector-scroll")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .flex()
                                .flex_col()
                                .gap(px(11.))
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_size(px(FS_TAG))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(s.faint)
                                                .child("SKILL INSPECTOR"),
                                        )
                                        .child(
                                            icon_button(
                                                "skill-ins-close",
                                                ProIcon::Close,
                                                loc.pick("收起详情面板", "Close details"),
                                                s,
                                            )
                                            .on_press(cx, |this, _, cx| {
                                                this.selected_skill_inspector =
                                                    None;
                                                cx.notify();
                                            }),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(9.))
                                        .child(
                                            SkillCardGlyph::from_name(&sk.name)
                                                .render_tile(34., s.pro.accent_info),
                                        )
                                        .child(
                                            div()
                                                .min_w_0()
                                                .flex_1()
                                                .flex()
                                                .flex_col()
                                                .gap(px(2.))
                                                .child(
                                                    div()
                                                        .text_size(px(FS_UI))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(s.ink)
                                                        .truncate()
                                                        .child(sk.name.clone()),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(FS_TAG))
                                                        .text_color(s.muted)
                                                        .truncate()
                                                        .child(scope_label(&sk.scope_label, loc)),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(FS_META))
                                        .line_height(px(17.))
                                        .text_color(s.text)
                                        .child(sk.description.clone()),
                                )
                                .child(md_block),
                        )
                        .child(
                            Tactile::primary("skill-ins-inject", s)
                                .disabled(!can_use)
                                .w_full()
                                .h(px(CTRL_H))
                                .px(px(10.))
                                .justify_center()
                                .gap(px(6.))
                                .child(pro_icon(ProIcon::Sparkles, 12.5, s.on_primary))
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(FS_META))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(format!(
                                            "{} ${}",
                                            loc.pick("引用", "Insert"),
                                            sk.name
                                        )),
                                )
                                .on_press(cx, move |wb, win, cx| {
                                    wb.inject_token_into_composer(&sk_token, win, cx);
                                }),
                        ),
                )
            } else if let Some(rest) = key.strip_prefix("tool:") {
                let (srv_id, t_name) = rest.split_once(':')?;
                let t = inv
                    .tools
                    .iter()
                    .find(|item| item.server_id == srv_id && item.name == t_name)?;
                let jv = (!t.schema_json.is_null()).then(|| {
                    let cache_key = format!("tool-schema-{:?}", (&t.server_id, &t.name));
                    self.tool_json_viewers
                        .entry(cache_key)
                        .or_insert_with(|| {
                            cx.new(|cx| {
                                JsonViewer::new(t.name.clone(), t.schema_json.clone(), window, cx)
                                    .with_max_height(260.)
                            })
                        })
                        .clone()
                });
                Some(
                    div()
                        .map(|d| if narrow { d.w_full() } else { d.w(px(258.)) })
                        .h_full()
                        .flex_none()
                        .border_l_1()
                        .border_color(s.line_soft)
                        .bg(s.tray.opacity(if s.dark { 0.42 } else { 0.55 }))
                        .p(px(15.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(FS_TAG))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(s.faint)
                                        .child("TOOL SCHEMA"),
                                )
                                .child(
                                    icon_button(
                                        "tool-ins-close",
                                        ProIcon::Close,
                                        loc.pick("收起详情面板", "Close details"),
                                        s,
                                    )
                                    .on_press(cx, |this, _, cx| {
                                        this.selected_skill_inspector = None;
                                        cx.notify();
                                    }),
                                ),
                        )
                        .child(
                            div()
                                .font_family(mono_font(cx))
                                .text_size(px(FS_CONTROL))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(s.ink)
                                .child(t.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(FS_META))
                                .line_height(px(16.5))
                                .text_color(s.muted)
                                .child(t.description.clone()),
                        )
                        .when_some(jv, |d, viewer| d.child(viewer)),
                )
            } else {
                None
            }
        });

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(filter_bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .flex_row()
                    .when(show_list, |row| row.child(
                        div()
                            .id("kar-skills-mcp-scroll")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .child(body),
                    ))
                    .when_some(inspector, |d, ins| d.child(ins)),
            )
            .into_any_element()
    }

    pub fn inject_token_into_composer(
        &mut self,
        prefix_token: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.access_mode.is_writable()
            && !matches!(self.access_mode, crate::types::ChatAccessMode::NoneSelected)
        {
            return;
        }
        self.reveal_chat_composer(window, cx);
        let current = self.composer_editor.read(cx).value().to_owned();
        let next = if current.trim().is_empty() {
            prefix_token.to_owned()
        } else {
            format!("{prefix_token}{current}")
        };
        self.composer_editor
            .update(cx, |ed, cx| ed.set_value(next, window, cx));
        let focus = self.composer_editor.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }
}
