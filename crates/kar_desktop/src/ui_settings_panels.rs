//! Rich Settings sub-panels (`Appearance`, `Keybindings`, `Voice`, `ProjectsGit`, `Devices`,
//! and `Maintenance & About`) built strictly with Pure GPUI + KUI + Kwrok's unified design system.

use gpui::{prelude::*, *};
use kui::{
    ThemeMode,
    components::{
        input::Input,
        shortcut_recorder::ShortcutPlatform,
    },
    primitives::{
        editor::{Editor, InputMode},
        pro_icons::{ProIcon, pro_icon},
    },
};
use uuid::Uuid;

use crate::{
    forge_devices_voice::{
        prune_managed_worktrees,
    },
    kar_bridge::KarCommand,
    prefs::{
        ChatWidthMode, CommitStyleMode, DesktopPreferences, DictationTriggerMode, DiffLayoutMode,
        FollowUpMode, FontScale, KEYBINDING_COMMANDS, PrMergeMethod, ProjectScript,
        SendShortcutMode, VoiceProviderKind, chord_from_keystroke,
    },
    ui_motion::{
        Tactile, danger_button_sm, icon_button_sm, icon_button_xs, outline_button, primary_button,
        selection::{Group, SelectionStrip},
        soft_button, soft_button_sm, text_button_sm,
    },
    ui_primitives::{
        CTRL_H, CTRL_SM_H, FS_META, FS_TAG, FS_TITLE, FS_UI, ICON_XS, R_CTRL, R_PILL, R_SM, R_XS,
        StudioPalette, card_box, mono_font, render_kar_brand_lockup, render_kbd, search_input_box,
        settings_divider, settings_input_action_row, settings_page_shell, settings_row_item, settings_section_title,
        sub_card_box, tnum_features,
    },
    workbench::KarWorkbench,
};

impl KarWorkbench {
    // ═══════════════════════════════════════════════════════════════════════
    // 1. Appearance, Themes (`kantor_theme` 900+ Gallery), Typography & Chat/Diff UX
    // ═══════════════════════════════════════════════════════════════════════

    pub(crate) fn render_appearance_settings(
        &self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let stacked = f32::from(window.viewport_size().width) < 620.;

        let selected_mode = (!self.follows_system_appearance()).then_some(self.theme_mode);
        let mode_picker = SelectionStrip::options(
            "settings-appearance-mode",
            loc.pick("亮暗模式", "Color mode"),
            Group::Choices,
            [
                (None, loc.pick("跟随系统", "System")),
                (Some(ThemeMode::Light), loc.pick("浅色", "Light")),
                (Some(ThemeMode::Dark), loc.pick("深色", "Dark")),
            ],
            selected_mode,
            s,
            cx,
            |wb, mode, _, cx| match mode {
                Some(m) => wb.select_appearance(m, cx),
                None => wb.set_follow_system_appearance(cx),
            },
        );

        let gallery_open_state =
            window.use_keyed_state("appearance-gallery-open", cx, |_, _| false);
        let gallery_open = *gallery_open_state.read(cx);
        let gallery_toggle_state = gallery_open_state.clone();

        let theme_search = window.use_keyed_state("appearance-theme-search", cx, |win, cx| {
            let mut ed = Editor::new(
                loc.pick("搜索配色主题…", "Search color themes…"),
                InputMode::Search,
                win,
                cx,
            );
            ed.set_placeholder(loc.pick("搜索主题名称（如 Catppuccin、Tokyo Night、Gruvbox、Nord）…", "Search Catppuccin, Tokyo Night, Nord…"));
            ed.font_size = FS_UI;
            ed
        });
        let theme_q = theme_search.read(cx).value().trim().to_lowercase();
        let theme_focused = theme_search.focus_handle(cx).is_focused(window);

        let all_themes = window
            .use_keyed_state("kantor-theme-catalog", cx, |_, _| {
                kantor_theme::catalog::all()
            })
            .read(cx)
            .clone();

        let active_named = self.prefs.named_theme_id.clone();
        let mut swatch_grid = div().w_full().flex().flex_wrap().gap(px(8.));

        // Classic Kwrok Architectural Monochrome card
        let mono_selected = active_named.is_none();
        swatch_grid = swatch_grid.child(
            Tactile::soft("theme-card-monochrome", s)
                .w(px(204.))
                .h(px(56.))
                .px(px(10.))
                .radius(R_CTRL)
                .outlined(
                    if mono_selected { s.ink } else { s.line_soft },
                    s.line_strong,
                )
                .justify_between()
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(s.font_size(FS_META))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(s.ink)
                                        .child(loc.pick("经典极简黑白（默认）", "Kwrok Monochrome")),
                                )
                                .when(mono_selected, |d| {
                                    d.child(pro_icon(ProIcon::Check, ICON_XS, s.pro.green_ink))
                                }),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(3.))
                                .children(
                                    [0x18181bu32, 0x3f3f46, 0x71717a, 0xa1a1aa, 0xe4e4e7, 0xfafafa]
                                        .into_iter()
                                        .map(|hex| {
                                            div()
                                                .size(px(10.))
                                                .rounded(px(R_XS))
                                                .bg(rgb(hex))
                                                .border_1()
                                                .border_color(s.line_soft)
                                        }),
                                ),
                        ),
                )
                .on_press(cx, |wb, _, cx| {
                    wb.prefs.named_theme_id = None;
                    wb.save_prefs();
                    crate::apply_theme_and_typography(wb.theme_mode, &wb.prefs, cx);
                    cx.notify();
                }),
        );

        let featured_zh: &[(&str, &str)] = &[
            ("Catppuccin Latte", "柔白拿铁 · Latte"),
            ("Catppuccin Mocha", "深邃夜幕 · Mocha"),
            ("Tokyo Night", "东京夜色 · Tokyo Night"),
            ("Nord", "北欧冰蓝 · Nord"),
            ("Rose Pine", "柔光松木 · Rose Pine"),
        ];
        let expanded_curated = [
            "Catppuccin Latte",
            "Catppuccin Mocha",
            "Tokyo Night",
            "Tokyo Night Light",
            "Nord",
            "Rose Pine",
            "Rose Pine Dawn",
            "Gruvbox Dark",
            "Gruvbox Light",
            "Kanagawa Wave",
            "Solarized Dark - Patched",
            "Solarized Light",
            "Dracula",
            "Everforest Dark - Hard",
            "One Half Dark",
            "GitHub Light Default",
        ];

        let filtered_themes: Vec<_> = if !gallery_open {
            let mut list = Vec::new();
            for &(raw_name, _) in featured_zh {
                if let Some(found) = all_themes
                    .iter()
                    .find(|t| t.name.eq_ignore_ascii_case(raw_name))
                {
                    list.push(found.clone());
                }
            }
            if let Some(active_id) = active_named.as_deref()
                && !list.iter().any(|t| t.id.to_setting() == active_id)
                && let Some(active_entry) = all_themes
                    .iter()
                    .find(|t| t.id.to_setting() == active_id)
            {
                list.push(active_entry.clone());
            }
            list
        } else if theme_q.is_empty() {
            all_themes
                .iter()
                .filter(|t| expanded_curated.iter().any(|c| t.name.eq_ignore_ascii_case(c)))
                .take(20)
                .cloned()
                .collect()
        } else {
            all_themes
                .iter()
                .filter(|t| t.name.to_lowercase().contains(&theme_q))
                .take(30)
                .cloned()
                .collect()
        };

        for (ix, entry) in filtered_themes.into_iter().enumerate() {
            let setting_id = entry.id.to_setting();
            let chosen = active_named.as_deref() == Some(setting_id.as_str());
            let sw = entry.swatch();
            let bg_hex = entry.palette.background;
            let fg_hex = entry.palette.foreground;
            let is_dark = entry.palette.dark;
            let set_clone = setting_id.clone();
            let display_name = if matches!(loc, crate::ui_primitives::UiLocale::Zh) {
                featured_zh
                    .iter()
                    .find(|(raw, _)| entry.name.eq_ignore_ascii_case(raw))
                    .map(|(_, zh)| (*zh).to_owned())
                    .unwrap_or_else(|| entry.name.clone())
            } else {
                entry.name.clone()
            };
            swatch_grid = swatch_grid.child(
                Tactile::soft(("theme-card", ix), s)
                    .w(px(204.))
                    .h(px(56.))
                    .px(px(10.))
                    .radius(R_CTRL)
                    .outlined(
                        if chosen { s.ink } else { s.line_soft },
                        s.line_strong,
                    )
                    .justify_between()
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(5.))
                                    .child(
                                        div()
                                            .px(px(4.))
                                            .rounded(px(R_XS))
                                            .bg(rgb(bg_hex))
                                            .border_1()
                                            .border_color(s.line_soft)
                                            .text_size(s.font_size(FS_TAG))
                                            .font_weight(FontWeight::BOLD)
                                            .text_color(rgb(fg_hex))
                                            .child("Aa"),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(s.font_size(FS_META))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child(display_name),
                                    )
                                    .when(chosen, |d| {
                                        d.child(pro_icon(ProIcon::Check, ICON_XS, s.pro.green_ink))
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(3.))
                                    .children(sw.into_iter().map(|c| {
                                        div().size(px(10.)).rounded(px(R_XS)).bg(rgb(c))
                                    })),
                            ),
                    )
                    .on_press(cx, move |wb, _, cx| {
                        wb.prefs.named_theme_id = Some(set_clone.clone());
                        wb.save_prefs();
                        wb.select_appearance(
                            if is_dark {
                                ThemeMode::Dark
                            } else {
                                ThemeMode::Light
                            },
                            cx,
                        );
                    }),
            );
        }

        let total_themes = all_themes.len();
        let theme_box = card_box(s).flex().flex_col().child(
            div()
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .flex_wrap()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .text_size(s.font_size(FS_UI))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(s.ink)
                                        .child(loc.pick("精选主题配色", "Curated color themes")),
                                )
                                .child(
                                    div()
                                        .font_features(tnum_features())
                                        .text_size(s.font_size(FS_META))
                                        .text_color(s.muted)
                                        .child(loc.pick(
                                            "默认展示 6 款护眼精选配色，点击右侧按钮可搜索全部主题库。",
                                            "Showing 6 curated schemes; expand to search the full theme catalog.",
                                        )),
                                ),
                        )
                        .child(
                            soft_button_sm(
                                "appearance-toggle-gallery",
                                if gallery_open {
                                    loc.pick("收起全部主题 ▴", "Collapse gallery ▴").to_owned()
                                } else {
                                    format!(
                                        "{} ({total_themes}) ▾",
                                        loc.pick("浏览全部配色主题", "Browse all themes")
                                    )
                                },
                                s,
                            )
                            .on_press(cx, move |_, _, cx| {
                                gallery_toggle_state.update(cx, |v, cx| {
                                    *v = !*v;
                                    cx.notify();
                                });
                            }),
                        ),
                )
                .when(gallery_open, |d| {
                    d.child(
                        div().w(px(300.)).max_w_full().child(search_input_box(
                            "appearance-theme-search-box",
                            theme_search,
                            theme_focused,
                            true,
                            s,
                        )),
                    )
                })
                .child(swatch_grid),
        );

        // Typography & Layout Card
        let ui_scale_strip = SelectionStrip::options(
            "appearance-ui-scale",
            loc.pick("界面字号缩放", "UI font scale"),
            Group::Choices,
            FontScale::ALL.map(|sc| (sc, sc.label(loc))),
            self.prefs.ui_font_scale,
            s,
            cx,
            |wb, scale, _, cx| {
                wb.prefs.ui_font_scale = scale;
                wb.save_prefs();
                crate::apply_theme_and_typography(wb.theme_mode, &wb.prefs, cx);
                cx.notify();
            },
        );

        let mut code_sizes = vec![12., 13., 14., 15., 16.];
        if !code_sizes.contains(&self.prefs.code_font_size) {
            code_sizes.push(self.prefs.code_font_size);
            code_sizes.sort_by(f32::total_cmp);
        }
        let code_size_strip = SelectionStrip::options(
            "appearance-code-size",
            loc.pick("代码与终端字号", "Code & terminal font size"),
            Group::Choices,
            code_sizes.into_iter().map(|size| (size, format!("{size}px"))),
            self.prefs.code_font_size,
            s,
            cx,
            |wb, sz, _, cx| {
                wb.prefs.code_font_size = sz;
                wb.save_prefs();
                crate::apply_theme_and_typography(wb.theme_mode, &wb.prefs, cx);
                cx.notify();
            },
        );

        let code_font_current = cx.global::<crate::ui_primitives::ActiveTypography>()
            .code_font_family.clone();
        let mut code_fonts = vec![(None, SharedString::from(loc.pick("系统默认", "System")))];
        let candidates = [
            "SF Mono", "JetBrains Mono", "Menlo", "Monaco", "Cascadia Mono", "Consolas",
            "Noto Sans Mono", "DejaVu Sans Mono", "Liberation Mono", "Courier New",
        ];
        for family in candidates.iter()
            .filter_map(|name| crate::installed_code_font(name, cx))
            .take(3)
        {
            code_fonts.push((Some(family.clone()), family));
        }
        if let Some(current) = &code_font_current
            && !code_fonts.iter().any(|(family, _)| family.as_ref() == Some(current))
        {
            code_fonts.push((Some(current.clone()), current.clone()));
        }
        let code_font_strip = SelectionStrip::options(
            "appearance-code-family",
            loc.pick("等宽代码字体", "Monospace font"),
            Group::Choices,
            code_fonts,
            code_font_current,
            s,
            cx,
            |wb, fam, _, cx| {
                wb.prefs.code_font_family = fam.map(|family| family.to_string());
                wb.save_prefs();
                crate::apply_theme_and_typography(wb.theme_mode, &wb.prefs, cx);
                cx.notify();
            },
        );

        let chat_width_strip = SelectionStrip::options(
            "appearance-chat-width",
            loc.pick("对话阅读区宽度", "Conversation width"),
            Group::Choices,
            ChatWidthMode::ALL.map(|m| (m, m.label(loc))),
            self.prefs.chat_width_mode,
            s,
            cx,
            |wb, mode, _, cx| {
                wb.prefs.chat_width_mode = mode;
                wb.save_prefs();
                cx.notify();
            },
        );

        let system_mono = &cx.global::<crate::SystemCodeFont>().0;
        let font_description = match loc {
            crate::ui_primitives::UiLocale::Zh => format!("用于代码块、文件差异和终端。系统默认使用 {system_mono}。"),
            crate::ui_primitives::UiLocale::En => format!("Used for code blocks, diffs, and the terminal. System default is {system_mono}."),
        };
        let typo_box = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("亮暗外观模式", "Appearance mode"),
                loc.pick(
                    "跟随电脑系统自动切换，或固定使用浅色 / 深色界面。",
                    "Follow macOS appearance or lock Light / Dark mode.",
                ),
                mode_picker.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("界面文字大小", "Interface text scale"),
                loc.pick("一键调节全局菜单、设置与对话正文的阅读字号。", "Scale navigation, settings, and reading text."),
                ui_scale_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("对话阅读宽度", "Thread reading width"),
                loc.pick("适中宽度更适合专注阅读，宽屏显示器也可铺满窗口。", "Balanced column or full-width layout."),
                chat_width_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("代码与终端字体", "Code & terminal font"),
                font_description,
                code_font_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("代码字号", "Code font size"),
                loc.pick("调节代码块、文件差异和终端的字号。", "Text size in code blocks, diffs, and the terminal."),
                code_size_strip.into_any_element(),
                stacked,
                s,
            ));

        // Chat & Diff Interaction Card
        let diff_layout_strip = SelectionStrip::options(
            "appearance-diff-layout",
            loc.pick("代码对比布局", "Diff layout"),
            Group::Choices,
            DiffLayoutMode::ALL.map(|m| (m, m.label(loc))),
            self.prefs.diff_layout,
            s,
            cx,
            |wb, mode, _, cx| {
                wb.prefs.diff_layout = mode;
                wb.save_prefs();
                cx.notify();
            },
        );

        let send_strip = SelectionStrip::options(
            "appearance-send-key",
            loc.pick("发送消息快捷键", "Send shortcut"),
            Group::Choices,
            SendShortcutMode::ALL.map(|m| (m, m.label(loc))),
            self.prefs.send_shortcut,
            s,
            cx,
            |wb, mode, _, cx| {
                wb.prefs.send_shortcut = mode;
                wb.save_prefs();
                cx.notify();
            },
        );

        let followup_strip = SelectionStrip::options(
            "appearance-followup-mode",
            loc.pick("运行中追问行为", "Follow-up behavior"),
            Group::Choices,
            FollowUpMode::ALL.map(|m| (m, m.label(loc))),
            self.prefs.follow_up_mode,
            s,
            cx,
            |wb, mode, _, cx| {
                wb.prefs.follow_up_mode = mode;
                wb.save_prefs();
                cx.notify();
            },
        );

        let ignore_ws_sw = Self::settings_switch(
            "appearance-diff-ws",
            loc.pick("忽略空白字符差异", "Ignore whitespace in diffs"),
            self.prefs.diff_ignore_whitespace,
            |wb, cx| {
                wb.prefs.diff_ignore_whitespace = !wb.prefs.diff_ignore_whitespace;
                wb.save_prefs();
                wb.inline_git_diffs.clear();
                cx.notify();
            },
            s,
            window,
            cx,
        );

        let collapse_tools_sw = Self::settings_switch(
            "appearance-collapse-tools",
            loc.pick("折叠连续工具调用", "Group consecutive tool calls"),
            self.prefs.default_collapse_tools,
            |wb, cx| {
                wb.prefs.default_collapse_tools = !wb.prefs.default_collapse_tools;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );

        let suggestions_sw = Self::settings_switch(
            "appearance-prompt-suggestions",
            loc.pick("显示智能追问建议气泡", "Show follow-up prompt suggestions"),
            self.prefs.prompt_suggestions_enabled,
            |wb, cx| {
                wb.prefs.prompt_suggestions_enabled = !wb.prefs.prompt_suggestions_enabled;
                if !wb.prefs.prompt_suggestions_enabled {
                    wb.live_prompt_suggestion = None;
                }
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );

        let chat_diff_box = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("发送消息按键", "Send message key"),
                loc.pick("选择按 Enter 直接发送，或按 ⌘Enter 发送（Enter 换行）。", "Send on Enter or ⌘Enter."),
                send_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("运行中追问方式", "Running turn follow-ups"),
                loc.pick("智能体执行时发送新消息：排队等待本轮结束，或立即插话引导。", "Queue behind active turn or steer immediately."),
                followup_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("代码差异（Diff）视图", "Diff view layout"),
                loc.pick("在单列统一视图与左右双栏并排对比之间切换。", "Switch between unified and split side-by-side diffs."),
                diff_layout_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("忽略空白字符差异", "Ignore whitespace changes"),
                loc.pick("审查 Git 文件改动时自动过滤纯缩进与空格变化。", "Hide indentation-only changes in Git diffs."),
                ignore_ws_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("折叠连续工具调用", "Group consecutive tool calls"),
                loc.pick("在执行过程中将连续的同类调用收起为一行。", "Show consecutive similar calls as one expandable step."),
                collapse_tools_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("智能下一步建议", "Follow-up suggestions"),
                loc.pick("回合完成后在输入框上方展示可一键采纳的追问建议。", "Show suggested next prompts above the composer."),
                suggestions_sw,
                stacked,
                s,
            ));

        div()
            .id("settings-appearance-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                settings_page_shell()
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("外观模式与阅读字号", "Mode & Typography"), s))
                            .child(typo_box),
                    )
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("发送消息与代码查看习惯", "Chat & Diff Experience"), s))
                            .child(chat_diff_box),
                    )
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("配色主题方案", "Theme & Palette"), s))
                            .child(theme_box),
                    ),
            )
            .into_any_element()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 2. Keybindings Center (KUI Keycaps + Live Chord Recording + Conflict Check)
    // ═══════════════════════════════════════════════════════════════════════

    pub(crate) fn render_keybindings_settings(
        &self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let kb_search = window.use_keyed_state("kb-search-editor", cx, |win, cx| {
            let mut ed = Editor::new(
                loc.pick("搜索命令或按键…", "Search commands or keys…"),
                InputMode::Search,
                win,
                cx,
            );
            ed.set_placeholder(loc.pick("搜索快捷键（如 新建对话、终端、cmd+k）…", "Search commands or shortcuts…"));
            ed.font_size = FS_UI;
            ed
        });
        let recording_cmd = window.use_keyed_state("kb-recording-cmd", cx, |_, _| None::<String>);
        let rec_focus = window
            .use_keyed_state("kb-recorder-focus", cx, |_, cx| cx.focus_handle())
            .read(cx)
            .clone();

        let q = kb_search.read(cx).value().trim().to_lowercase();
        let search_focused = kb_search.focus_handle(cx).is_focused(window);
        let active_rec = recording_cmd.read(cx).clone();
        let platform = ShortcutPlatform::detected();

        let kb_json_path = DesktopPreferences::keybindings_json_path(&self.config.chat_dir);
        let has_custom = !self.prefs.custom_keybindings.is_empty();

        let top_bar = div()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap(px(10.))
            .child(
                div().w(px(320.)).max_w_full().child(search_input_box(
                    "kb-search-box",
                    kb_search,
                    search_focused,
                    true,
                    s,
                )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        outline_button(
                            "kb-open-json",
                            loc.pick("打开 keybindings.json", "Open keybindings.json"),
                            s,
                        )
                        .child(pro_icon(ProIcon::Code, ICON_XS, s.muted))
                        .on_press(cx, move |wb, _, cx| {
                            wb.prefs.save_keybindings_json(&wb.config.chat_dir);
                            wb.open_workspace_in_preferred_editor(Some(kb_json_path.clone()), cx);
                        }),
                    )
                    .child(
                        soft_button(
                            "kb-reset-all",
                            loc.pick("全部恢复默认", "Reset all"),
                            s,
                        )
                        .disabled(!has_custom)
                        .on_press(cx, |wb, _, cx| {
                            wb.prefs.custom_keybindings.clear();
                            wb.save_prefs();
                            wb.notify_toast("已将全部快捷键恢复为默认方案", cx);
                        }),
                    ),
            );

        let mut list_card = card_box(s).overflow_hidden().flex().flex_col();
        let mut last_cat = "";
        let mut shown = 0usize;

        for (ix, def) in KEYBINDING_COMMANDS.iter().enumerate() {
            let chord = self.prefs.effective_chord(def.id).to_owned();
            if !q.is_empty()
                && !def.label_zh.to_lowercase().contains(&q)
                && !def.label_en.to_lowercase().contains(&q)
                && !def.category_zh.to_lowercase().contains(&q)
                && !chord.to_lowercase().contains(&q)
            {
                continue;
            }
            let cat = def.category(loc);
            if cat != last_cat {
                if shown > 0 {
                    list_card = list_card.child(settings_divider(s));
                }
                list_card = list_card.child(
                    div()
                        .px(px(16.))
                        .py(px(8.))
                        .bg(s.tray)
                        .text_size(s.font_size(FS_META))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(s.muted)
                        .child(cat),
                );
                last_cat = cat;
            } else {
                list_card = list_card.child(settings_divider(s));
            }
            shown += 1;

            let is_rec = active_rec.as_deref() == Some(def.id);
            let is_modified = self.prefs.custom_keybindings.contains_key(def.id);
            let conflict = self.prefs.find_conflict(def.id, &chord);
            let cmd_id = def.id.to_owned();
            let cmd_for_reset = def.id.to_owned();
            let rec_state_click = recording_cmd.clone();
            let rec_focus_click = rec_focus.clone();

            list_card = list_card.child(
                div()
                    .min_h(px(48.))
                    .px(px(16.))
                    .py(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(s.font_size(FS_UI))
                                    .text_color(s.ink)
                                    .child(def.label(loc)),
                            )
                            .when(is_modified, |d| {
                                d.child(
                                    div()
                                        .px(px(6.))
                                        .py(px(1.))
                                        .rounded(px(R_PILL))
                                        .bg(s.pro.accent_soft)
                                        .text_size(s.font_size(FS_TAG))
                                        .text_color(s.pro.accent_info)
                                        .child(loc.pick("已自定义", "Modified")),
                                )
                            })
                            .when_some(conflict, |d, other| {
                                d.child(
                                    div()
                                        .px(px(6.))
                                        .py(px(1.))
                                        .rounded(px(R_PILL))
                                        .bg(s.pro.amber_soft)
                                        .text_size(s.font_size(FS_TAG))
                                        .text_color(s.pro.amber_ink)
                                        .child(format!(
                                            "{}: {}",
                                            loc.pick("冲突", "Conflict"),
                                            other.label(loc)
                                        )),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .when(is_modified, |row| {
                                row.child(
                                    text_button_sm(
                                        ("kb-row-reset", ix),
                                        loc.pick("重置", "Reset"),
                                        s,
                                    )
                                    .on_press(cx, move |wb, _, cx| {
                                        wb.prefs.custom_keybindings.remove(&cmd_for_reset);
                                        wb.save_prefs();
                                        cx.notify();
                                    }),
                                )
                            })
                            .child(if is_rec {
                                soft_button_sm(
                                    ("kb-rec-active", ix),
                                    loc.pick("请按下新组合键（Esc 取消）…", "Press shortcut (Esc cancels)…"),
                                    s,
                                )
                                .lit(true)
                                .on_press(cx, {
                                    let rec_cancel = recording_cmd.clone();
                                    move |_, _, cx| {
                                        rec_cancel.update(cx, |v, cx| {
                                            *v = None;
                                            cx.notify();
                                        });
                                    }
                                })
                                .into_any_element()
                            } else {
                                Tactile::soft(("kb-record-btn", ix), s)
                                    .h(px(28.))
                                    .px(px(8.))
                                    .radius(R_SM)
                                    .outlined(s.line_soft, s.line_strong)
                                    .tip(loc.pick("点击录制新快捷键", "Click to record shortcut"))
                                    .child(platform.keycaps(&chord, true, Modifiers::default(), cx))
                                    .on_press(cx, move |_, win, cx| {
                                        rec_state_click.update(cx, |v, cx| {
                                            *v = Some(cmd_id.clone());
                                            cx.notify();
                                        });
                                        win.focus(&rec_focus_click, cx);
                                    })
                                    .into_any_element()
                            }),
                    ),
            );
        }

        if shown == 0 {
            list_card = list_card.child(
                div()
                    .p(px(24.))
                    .text_size(s.font_size(FS_META))
                    .text_color(s.muted)
                    .child(loc.pick("没有匹配的快捷键条目", "No matching shortcuts")),
            );
        }

        let rec_capture = recording_cmd.clone();
        div()
            .id("settings-keybindings-scroll")
            .track_focus(&rec_focus)
            .capture_key_down(cx.listener(move |wb, ev: &KeyDownEvent, _, cx| {
                let Some(target_cmd) = rec_capture.read(cx).clone() else {
                    return;
                };
                if ev.keystroke.key == "escape" {
                    rec_capture.update(cx, |v, cx| {
                        *v = None;
                        cx.notify();
                    });
                    cx.stop_propagation();
                    return;
                }
                if let Some(new_chord) = chord_from_keystroke(&ev.keystroke) {
                    wb.prefs.custom_keybindings.insert(target_cmd, new_chord);
                    wb.save_prefs();
                    rec_capture.update(cx, |v, cx| {
                        *v = None;
                        cx.notify();
                    });
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .size_full()
            .overflow_y_scroll()
            .child(
                settings_page_shell()
                    .child(top_bar)
                    .child(list_card),
            )
            .into_any_element()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 3. Voice Input & Dictation Settings
    // ═══════════════════════════════════════════════════════════════════════

    pub(crate) fn render_voice_settings(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let stacked = f32::from(window.viewport_size().width) < 620.;

        let provider = self.prefs.voice_provider;
        let provider_strip = SelectionStrip::options(
            "voice-provider-strip",
            loc.pick("语音识别引擎", "Speech engine"),
            Group::Choices,
            VoiceProviderKind::ALL.map(|p| (p, p.label(loc))),
            provider,
            s,
            cx,
            |wb, p, _, cx| {
                wb.prefs.voice_provider = p;
                wb.save_prefs();
                wb.refresh_voice();
                cx.notify();
            },
        );

        let api_key_id = crate::hardware::provider_id(provider);
        let configured = self.voice_env.keys.get(api_key_id) == Some(&true);
        let key_input = window.use_keyed_state(
            SharedString::from(format!("voice-key-inp-{api_key_id}")),
            cx,
            |win, cx| {
                let mut edit = Editor::new("语音服务密钥", InputMode::Password, win, cx);
                edit.set_placeholder(loc.pick("输入新的 API 密钥", "Enter a new API key"));
                edit
            },
        );

        let mode_strip = SelectionStrip::options(
            "voice-trigger-mode",
            loc.pick("听写触发方式", "Dictation trigger"),
            Group::Choices,
            DictationTriggerMode::ALL.map(|m| (m, m.label(loc))),
            self.prefs.voice_dictation_mode,
            s,
            cx,
            |wb, m, _, cx| {
                wb.prefs.voice_dictation_mode = m;
                wb.save_prefs();
                cx.notify();
            },
        );

        let lang_strip = SelectionStrip::options(
            "voice-lang-strip",
            loc.pick("识别语言", "Recognition language"),
            Group::Choices,
            [
                (
                    "auto".to_owned(),
                    loc.pick("中英自动识别", "Auto").to_owned(),
                ),
                ("zh".to_owned(), "简体中文".to_owned()),
                ("en".to_owned(), "English".to_owned()),
                ("ja".to_owned(), "日本語".to_owned()),
            ],
            self.prefs.voice_language.clone(),
            s,
            cx,
            |wb, l, _, cx| {
                wb.prefs.voice_language = l;
                wb.save_prefs();
                cx.notify();
            },
        );

        let filler_sw = Self::settings_switch(
            "voice-remove-fillers",
            loc.pick("自动过滤口头禅与语气词", "Remove filler words"),
            self.prefs.voice_remove_fillers,
            |wb, cx| {
                wb.prefs.voice_remove_fillers = !wb.prefs.voice_remove_fillers;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );

        let is_recording = self.active_voice_rec.is_some();
        let dictation = self.composer_dictation();
        let finishing = dictation.is_some() && !is_recording;
        let status = &self.voice_env.status;
        let env_summary = if let Some(issue) = crate::hardware::LocalVoiceIssue::from_status(status) {
            issue.message(loc).to_owned()
        } else if status["supported"] == true {
            let state = if status["state"] == "transcribing" {
                loc.pick("正在转写", "Transcribing")
            } else {
                loc.pick("已就绪", "Ready")
            };
            format!("{} · {state}", status["model"].as_str().unwrap_or(""))
        } else {
            loc.pick("正在检查本机语音引擎…", "Checking local speech engine…").to_owned()
        };

        let mut engine_box = card_box(s).flex().flex_col().child(settings_row_item(
            loc.pick("识别服务提供商", "Transcription provider"),
            if provider == VoiceProviderKind::Local {
                env_summary.as_str()
            } else {
                loc.pick("结束听写时，录音会发给所选服务转写。", "When you finish, the selected provider transcribes the recording.")
            },
            provider_strip.into_any_element(),
            stacked,
            s,
        ));

        if provider != VoiceProviderKind::Local {
            let k_inp = key_input.clone();

            engine_box = engine_box.child(settings_divider(s)).child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .flex()
                    .items_center()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_1()
                            .flex_basis(px(140.))
                            .min_w_0()
                            .h(px(CTRL_H))
                            .px(px(10.))
                            .bg(s.soft)
                            .border_1()
                            .border_color(s.line_soft)
                            .rounded(px(R_CTRL))
                            .flex()
                            .items_center()
                            .child(key_input),
                    )
                    .when(configured, |row| {
                        row.child(
                            soft_button(
                                "voice-clear-key",
                                loc.pick("清除密钥", "Remove key"),
                                s,
                            )
                            .on_press(cx, move |wb, _, _| {
                                wb.hardware(crate::hardware::Command::VoiceKey(
                                    provider,
                                    crate::hardware::Key(None),
                                ));
                            }),
                        )
                    })
                    .child(
                        primary_button("voice-save-key", loc.pick("保存密钥", "Save key"), s)
                            .on_press(cx, move |wb, win, cx| {
                                let key = k_inp.read(cx).value().trim().to_owned();
                                if !key.is_empty() {
                                    wb.hardware(crate::hardware::Command::VoiceKey(
                                        provider,
                                        crate::hardware::Key(Some(key)),
                                    ));
                                    k_inp.update(cx, |e, cx| e.set_value("", win, cx));
                                }
                            }),
                    ),
            );
        }

        let models = if provider == VoiceProviderKind::Local {
            &self.voice_env.models
        } else {
            &self.voice_env.cloud_models
        };
        if matches!(
            provider,
            VoiceProviderKind::Local | VoiceProviderKind::OpenRouter
        ) && !models.is_empty()
        {
            let mut list = div()
                .id("voice-models")
                .max_h(px(132.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(3.))
                .px(px(14.))
                .py(px(6.));
            for (ix, model) in models.iter().enumerate() {
                let Some(id) = model["id"].as_str() else {
                    continue;
                };
                let active = if provider == VoiceProviderKind::Local {
                    model["active"] == true && model["state"] == "installed"
                } else {
                    self.prefs.voice_openrouter_model.as_deref() == Some(id)
                };
                let download =
                    provider == VoiceProviderKind::Local && model["state"] != "installed";
                let name = model["name"].as_str().unwrap_or(id).to_owned();
                let action = if active {
                    loc.pick("已选择", "Selected")
                } else if download {
                    loc.pick("下载", "Download")
                } else {
                    loc.pick("选择", "Select")
                };
                let id = id.to_owned();
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(6.))
                        .child(div().text_size(s.font_size(FS_META)).text_color(s.text).child(name))
                        .child(
                            soft_button_sm(("voice-model", ix), action, s)
                                .disabled(active)
                                .on_press(cx, move |wb, _, cx| {
                                    if provider == VoiceProviderKind::Local {
                                        wb.hardware(crate::hardware::Command::VoiceModel(
                                            if download { "download" } else { "select" }.into(),
                                            id.clone(),
                                        ));
                                    } else {
                                        wb.prefs.voice_openrouter_model = Some(id.clone());
                                        wb.save_prefs();
                                        cx.notify();
                                    }
                                }),
                        ),
                );
            }
            engine_box = engine_box.child(settings_divider(s)).child(list);
        }

        if provider == VoiceProviderKind::OpenRouter
            && let Some(reason) = &self.voice_env.cloud_model_error
        {
            engine_box = engine_box
                .child(settings_divider(s))
                .child(settings_row_item(
                    loc.pick("OpenRouter 模型目录", "OpenRouter model catalog"),
                    reason.clone(),
                    soft_button_sm("voice-refresh-models", loc.pick("重试", "Retry"), s)
                        .on_press(cx, |wb, _, _| wb.refresh_voice())
                        .into_any_element(),
                    stacked,
                    s,
                ));
        }
        engine_box = engine_box
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("麦克风检测与试音", "Microphone & live test"),
                dictation.filter(|text| !text.is_empty())
                    .or(self.microphone_name.as_deref())
                    .unwrap_or(loc.pick("无输入设备", "No input device")).to_owned(),
                primary_button(
                    "voice-test-record",
                    if is_recording {
                        loc.pick("完成听写", "Finish dictation")
                    } else if finishing {
                        loc.pick("正在整理听写…", "Finishing dictation…")
                    } else {
                        loc.pick("开始麦克风试音", "Test microphone")
                    },
                    s,
                )
                .disabled(finishing)
                .on_press(cx, |wb, win, cx| wb.toggle_voice_dictation(win, cx))
                .into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("识别语种", "Language"),
                loc.pick(
                    "自动混合识别中英文技术术语，或锁定单一语言。",
                    "Auto-detect bilingual tech terms or lock language.",
                ),
                lang_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("快捷键触发模式", "Trigger behavior"),
                format!(
                    "{} ({})",
                    loc.pick(
                        "设置听写快捷键的行为",
                        "Choose how the dictation shortcut works"
                    ),
                    self.prefs.effective_badge("chat.voice_dictate")
                ),
                mode_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("净化口头禅与停顿词", "Clean up filler words"),
                loc.pick(
                    "自动移除「嗯、啊、那个、um、uh」等口语杂质。",
                    "Strip spoken fillers automatically.",
                ),
                filler_sw,
                stacked,
                s,
            ));

        // Custom Vocabulary Card
        let vocab_input = window.use_keyed_state("voice-vocab-inp", cx, |win, cx| {
            Input::new(
                "voice-vocab-add",
                loc.pick(
                    "添加专属词汇或同音纠错规则",
                    "Add custom term or replacement",
                ),
                win,
                cx,
            )
            .with_placeholder(
                loc.pick(
                    "例如：GPUI 或 kwrok => Kwrok",
                    "e.g. GPUI or kwrok => Kwrok",
                ),
                cx,
            )
        });
        let vocab_for_add = vocab_input.clone();
        let mut vocab_chips = div().flex().flex_wrap().gap(px(6.));
        for (ix, word) in self.prefs.voice_custom_words.iter().cloned().enumerate() {
            vocab_chips = vocab_chips.child(
                div()
                    .h(px(CTRL_SM_H))
                    .pl(px(10.))
                    .pr(px(3.))
                    .rounded(px(R_PILL))
                    .bg(s.tray)
                    .border_1()
                    .border_color(s.line_soft)
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .text_size(s.font_size(FS_META))
                    .text_color(s.ink)
                    .child(word)
                    .child(
                        icon_button_xs(
                            ("vocab-del", ix),
                            ProIcon::Close,
                            loc.pick("移除", "Remove"),
                            s,
                        )
                        .on_press(cx, move |wb, _, cx| {
                            if ix < wb.prefs.voice_custom_words.len() {
                                wb.prefs.voice_custom_words.remove(ix);
                                wb.save_prefs();
                                cx.notify();
                            }
                        }),
                    ),
            );
        }

        let vocab_box = card_box(s)
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                settings_input_action_row()
                    .flex_wrap()
                    .child(div().flex_1().flex_basis(px(144.)).min_w_0().child(vocab_input))
                    .child(
                        soft_button("voice-vocab-submit", loc.pick("添加词条", "Add term"), s)
                            .on_press(cx, move |wb, win, cx| {
                                let v = vocab_for_add.read(cx).value(cx).trim().to_owned();
                                if !v.is_empty() && !wb.prefs.voice_custom_words.contains(&v) {
                                    wb.prefs.voice_custom_words.push(v);
                                    wb.save_prefs();
                                    vocab_for_add.update(cx, |inp, cx| inp.set_value("", win, cx));
                                    cx.notify();
                                }
                            }),
                    ),
            )
            .when(!self.prefs.voice_custom_words.is_empty(), |d| {
                d.child(vocab_chips)
            });

        div()
            .id("settings-voice-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                settings_page_shell()
                    .child(
                        div()
                            .child(settings_section_title(
                                loc.pick("语音听写与转写", "Voice Dictation"),
                                s,
                            ))
                            .child(engine_box),
                    )
                    .child(
                        div()
                            .child(settings_section_title(
                                loc.pick("个人技术词库与纠错", "Custom Vocabulary"),
                                s,
                            ))
                            .child(vocab_box),
                    ),
            )
            .into_any_element()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 4. Projects, Source Control, Forge Tokens & Managed Worktrees
    // ═══════════════════════════════════════════════════════════════════════

    pub(crate) fn render_projects_git_settings(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let stacked = f32::from(window.viewport_size().width) < 620.;

        // Registered projects list + Clone Git URL + Preferred external editor
        let clone_url_inp = window.use_keyed_state("proj-clone-url", cx, |win, cx| {
            Input::new("proj-clone-url", loc.pick("克隆远程 Git 仓库地址", "Clone Git repository URL"), win, cx)
                .with_placeholder("https://github.com/org/repo.git", cx)
        });
        let clone_for_btn = clone_url_inp.clone();

        let mut projects_card = card_box(s).flex().flex_col();
        let proj_len = self.project_registry.projects.len();
        for (ix, proj) in self.project_registry.projects.clone().into_iter().enumerate() {
            if ix > 0 {
                projects_card = projects_card.child(settings_divider(s));
            }
            let pid_up = proj.id.clone();
            let pid_down = proj.id.clone();
            let pid_rm = proj.id.clone();
            let root_open = proj.root_path.clone();
            let root_switch = proj.root_path.clone();
            projects_card = projects_card.child(
                div()
                    .px(px(16.))
                    .py(px(10.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(s.font_size(FS_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(crate::studio_hub::project_display_name(&proj.name).to_owned()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .font_family(mono_font(cx))
                                    .text_size(s.font_size(FS_META))
                                    .text_color(s.muted)
                                    .child(proj.root_path.display().to_string()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                soft_button_sm(("proj-new-chat", ix), loc.pick("新建对话", "New chat"), s)
                                    .on_press(cx, move |wb, win, cx| {
                                        wb.select_project_and_new_chat(root_switch.clone(), win, cx);
                                    }),
                            )
                            .child(
                                soft_button_sm(("proj-open-ed", ix), loc.pick("在编辑器打开", "Open in editor"), s)
                                    .on_press(cx, move |wb, _, cx| {
                                        wb.open_workspace_in_preferred_editor(Some(root_open.clone()), cx);
                                    }),
                            )
                            .when(proj_len > 1, |row| {
                                row.child(
                                    icon_button_sm(("proj-up", ix), ProIcon::ChevronUp, loc.pick("上移", "Move up"), s)
                                        .disabled(ix == 0)
                                        .on_press(cx, move |wb, _, cx| {
                                            if wb.project_registry.move_project(&pid_up, true) {
                                                let _ = wb.project_registry.save(&wb.config.chat_dir);
                                                cx.notify();
                                            }
                                        }),
                                )
                                .child(
                                    icon_button_sm(("proj-down", ix), ProIcon::ChevronDown, loc.pick("下移", "Move down"), s)
                                        .disabled(ix + 1 >= proj_len)
                                        .on_press(cx, move |wb, _, cx| {
                                            if wb.project_registry.move_project(&pid_down, false) {
                                                let _ = wb.project_registry.save(&wb.config.chat_dir);
                                                cx.notify();
                                            }
                                        }),
                                )
                                .child(
                                    danger_button_sm(("proj-rm", ix), loc.pick("移除", "Remove"), s)
                                        .on_press(cx, move |wb, _, cx| {
                                            if wb.project_registry.remove_project(&pid_rm) {
                                                let _ = wb.project_registry.save(&wb.config.chat_dir);
                                                wb.notify_toast("已从项目列表移除（本地磁盘文件未删除）", cx);
                                            }
                                        }),
                                )
                            }),
                    ),
            );
        }

        // Preferred External Editor Row + Clone Row
        let mut editor_opts = vec![("auto".to_owned(), loc.pick("自动检测", "Auto").to_owned())];
        for ed in &self.installed_editors {
            editor_opts.push((ed.id.clone(), ed.label.clone()));
        }
        let editor_strip = SelectionStrip::options(
            "proj-preferred-editor",
            loc.pick("首选外部编辑器", "Preferred external editor"),
            Group::Choices,
            editor_opts,
            self.prefs.preferred_external_editor.clone(),
            s,
            cx,
            |wb, ed, _, cx| {
                wb.prefs.preferred_external_editor = ed;
                wb.save_prefs();
                cx.notify();
            },
        );

        projects_card = projects_card
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("默认外部编辑器", "Default external editor"),
                loc.pick("点击「在编辑器打开」或按 ⌥⌘E 时调用的本机开发工具。", "Used by 'Open in editor' and ⌥⌘E."),
                editor_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .flex()
                    .items_end()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(div().flex_1().flex_basis(px(180.)).min_w_0().child(clone_url_inp))
                    .child(
                        primary_button("proj-clone-btn", loc.pick("克隆并导入", "Clone"), s)
                            .on_press(cx, move |wb, win, cx| {
                                let url = clone_for_btn.read(cx).value(cx).trim().to_owned();
                                if url.is_empty() {
                                    wb.notify_warning("请输入 Git 仓库地址", cx);
                                    return;
                                }
                                let parent = wb
                                    .effective_workspace_dir()
                                    .parent()
                                    .map(std::path::Path::to_path_buf)
                                    .unwrap_or_else(|| wb.effective_workspace_dir());
                                clone_for_btn.update(cx, |inp, cx| inp.set_value("", win, cx));
                                wb.notify_toast("正在后台克隆 Git 仓库…", cx);
                                wb.bridge.send(KarCommand::CloneGitRepo { url, parent_dir: parent });
                            }),
                    )
                    .child(
                        soft_button("proj-pick-local", loc.pick("添加本地目录…", "Add folder…"), s)
                            .on_press(cx, |wb, win, cx| wb.pick_workspace(win, cx)),
                    ),
            );

        // Quick Scripts for active workspace
        let ws_key = self.effective_workspace_dir().display().to_string();
        let scripts = self
            .prefs
            .project_scripts
            .get(&ws_key)
            .cloned()
            .unwrap_or_default();
        let sc_name_inp = window.use_keyed_state("proj-script-name", cx, |win, cx| {
            Input::new("proj-sc-name", loc.pick("脚本名称", "Script name"), win, cx)
                .with_placeholder(loc.pick("例如：启动开发服", "e.g. Dev server"), cx)
        });
        let sc_cmd_inp = window.use_keyed_state("proj-script-cmd", cx, |win, cx| {
            Input::new("proj-sc-cmd", loc.pick("Shell 命令", "Shell command"), win, cx)
                .with_placeholder("cargo check --workspace", cx)
        });
        let sc_n = sc_name_inp.clone();
        let sc_c = sc_cmd_inp.clone();
        let ws_key_add = ws_key.clone();

        let mut scripts_card = card_box(s).p(px(16.)).flex().flex_col().gap(px(10.));
        for (ix, sc) in scripts.into_iter().enumerate() {
            let cmd_run = sc.command.clone();
            let ws_key_del = ws_key.clone();
            scripts_card = scripts_card.child(
                sub_card_box(s)
                    .px(px(12.))
                    .py(px(8.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .flex_basis(px(176.))
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(s.font_size(FS_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(sc.name),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .font_family(mono_font(cx))
                                    .text_size(s.font_size(FS_META))
                                    .text_color(s.muted)
                                    .child(sc.command),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                soft_button_sm(("sc-run", ix), loc.pick("在终端运行", "Run"), s)
                                    .on_press(cx, move |wb, win, cx| {
                                        wb.close_all_overlays(cx);
                                        wb.run_terminal_command_str(cmd_run.clone(), win, cx);
                                    }),
                            )
                            .child(
                                danger_button_sm(("sc-del", ix), loc.pick("删除", "Delete"), s)
                                    .on_press(cx, move |wb, _, cx| {
                                        if let Some(vec) = wb.prefs.project_scripts.get_mut(&ws_key_del)
                                            && ix < vec.len()
                                        {
                                            vec.remove(ix);
                                            wb.save_prefs();
                                            cx.notify();
                                        }
                                    }),
                            ),
                    ),
            );
        }
        scripts_card = scripts_card.child(
            div()
                .flex()
                .items_end()
                .flex_wrap()
                .gap(px(8.))
                .child(div().flex_1().flex_basis(px(132.)).min_w_0().child(sc_name_inp))
                .child(div().flex_1().flex_basis(px(180.)).min_w_0().child(sc_cmd_inp))
                .child(
                    soft_button("sc-add-btn", loc.pick("保存快捷脚本", "Add script"), s).on_press(
                        cx,
                        move |wb, win, cx| {
                            let name = sc_n.read(cx).value(cx).trim().to_owned();
                            let cmd = sc_c.read(cx).value(cx).trim().to_owned();
                            if name.is_empty() || cmd.is_empty() {
                                return;
                            }
                            wb.prefs
                                .project_scripts
                                .entry(ws_key_add.clone())
                                .or_default()
                                .push(ProjectScript {
                                    id: Uuid::new_v4().to_string(),
                                    name,
                                    command: cmd,
                                    run_on_worktree_create: false,
                                });
                            wb.save_prefs();
                            sc_n.update(cx, |i, cx| i.set_value("", win, cx));
                            sc_c.update(cx, |i, cx| i.set_value("", win, cx));
                            cx.notify();
                        },
                    ),
                ),
        );

        // Forge Tokens, Commit Style & PR Merge Strategy
        let gh_inp = window.use_keyed_state("forge-gh-token", cx, |win, cx| {
            kui::components::password_field::PasswordField::new("forge-gh", "GitHub Personal Access Token", win, cx)
                .with_placeholder("输入新令牌 · 留空保留现有配置", cx)
        });
        let gl_inp = window.use_keyed_state("forge-gl-token", cx, |win, cx| {
            kui::components::password_field::PasswordField::new("forge-gl", "GitLab Private Token", win, cx)
                .with_placeholder("输入新令牌 · 留空保留现有配置", cx)
        });
        let lin_inp = window.use_keyed_state("forge-linear-token", cx, |win, cx| {
            kui::components::password_field::PasswordField::new("forge-linear", "Linear API Key", win, cx)
                .with_placeholder("输入新密钥 · 留空保留现有配置", cx)
        });
        let gh_save = gh_inp.clone();
        let gl_save = gl_inp.clone();
        let lin_save = lin_inp.clone();

        let commit_style_strip = SelectionStrip::options(
            "forge-commit-style",
            loc.pick("提交说明写作风格", "Commit message style"),
            Group::Choices,
            [
                CommitStyleMode::RepoConventions,
                CommitStyleMode::ConventionalCommits,
                CommitStyleMode::Custom,
            ]
            .map(|m| (m, m.label(loc))),
            self.prefs.commit_writing_style,
            s,
            cx,
            |wb, m, _, cx| {
                wb.prefs.commit_writing_style = m;
                wb.save_prefs();
                cx.notify();
            },
        );

        let merge_strip = SelectionStrip::options(
            "forge-pr-merge",
            loc.pick("默认 PR 合并策略", "Default PR merge method"),
            Group::Choices,
            [PrMergeMethod::Squash, PrMergeMethod::Merge, PrMergeMethod::Rebase]
                .map(|m| (m, m.label())),
            self.prefs.pr_default_merge_method,
            s,
            cx,
            |wb, m, _, cx| {
                wb.prefs.pr_default_merge_method = m;
                wb.save_prefs();
                cx.notify();
            },
        );

        let wt_origin_sw = Self::settings_switch(
            "wt-from-origin",
            loc.pick("隔离对话基于最新主分支创建", "Base worktrees on origin default branch"),
            self.prefs.new_worktrees_from_origin,
            |wb, cx| {
                wb.prefs.new_worktrees_from_origin = !wb.prefs.new_worktrees_from_origin;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );
        let wt_prefix_inp = window.use_keyed_state("wt-prefix-inp", cx, |win, cx| {
            Input::new("wt-prefix", loc.pick("隔离分支名前缀", "Worktree branch prefix"), win, cx)
                .with_value(self.prefs.worktree_branch_prefix.clone(), win, cx)
        });
        let wt_pref_save = wt_prefix_inp.clone();

        let git_adv_state =
            window.use_keyed_state("projects-git-adv-open", cx, |_, _| false);
        let git_adv_open = *git_adv_state.read(cx);
        let git_adv_toggle = git_adv_state.clone();

        let git_everyday_card = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("AI 生成提交说明风格", "AI commit message style"),
                loc.pick("在 Git 改动面板点击「生成提交说明」时使用的格式。", "Used when generating commit messages from Git changes."),
                commit_style_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("默认合并分支（PR）方式", "Default PR merge strategy"),
                loc.pick("在审查面板一键合并 Pull Request 时默认使用的方式。", "Default merge strategy in the PR panel."),
                merge_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("隔离对话基于最新主分支创建", "Start worktrees from origin/HEAD"),
                loc.pick("新建独立分支对话前自动同步远端最新代码，避免冲突。", "Fetch origin before creating isolated chat worktrees."),
                wt_origin_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("清理已结束对话遗留的临时目录", "Clean finished chat worktrees"),
                loc.pick("安全清理已完成对话留下的无未提交改动临时分支目录。", "Prune clean temporary worktrees left by finished chats."),
                outline_button("wt-prune-orphans", loc.pick("一键清理闲置目录", "Clean unused"), s)
                    .on_press(cx, |wb, _, cx| {
                        let ws = wb.effective_workspace_dir();
                        match prune_managed_worktrees(&ws, true) {
                            Ok(count) => {
                                wb.notify_toast(
                                    format!("已清理 {count} 个无未提交改动的临时目录"),
                                    cx,
                                );
                            }
                            Err(err) => wb.notify_warning(err.to_string(), cx),
                        }
                    })
                    .into_any_element(),
                stacked,
                s,
            ));

        let forge_advanced_card = card_box(s)
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(gh_inp)
                    .child(gl_inp)
                    .child(lin_inp)
                    .child(
                        div().flex().justify_end().child(
                            primary_button("forge-save-tokens", loc.pick("保存访问令牌", "Save tokens"), s)
                                .on_press(cx, move |wb, win, cx| {
                                    let fields = [&gh_save, &gl_save, &lin_save];
                                    let values = fields.map(|field| field.read(cx).value(cx).to_owned());
                                    if values.iter().all(|v| v.trim().is_empty()) { return; }
                                    wb.bridge.send(KarCommand::StoreForgeCredentials(crate::kar_bridge::ForgeCredentials(values)));
                                    for field in fields { field.update(cx, |field, cx| field.set_value("", win, cx)); }
                                }),
                        ),
                    ),
            )
            .child(settings_divider(s))
            .child(
                settings_input_action_row()
                    .px(px(16.))
                    .py(px(12.))
                    .flex_wrap()
                    .child(div().flex_1().flex_basis(px(144.)).min_w_0().child(wt_prefix_inp))
                    .child(
                        soft_button("wt-save-prefix", loc.pick("保存分支前缀", "Save prefix"), s)
                            .on_press(cx, move |wb, _, cx| {
                                let v = wt_pref_save.read(cx).value(cx).trim().to_owned();
                                wb.prefs.worktree_branch_prefix =
                                    if v.is_empty() { "kar/wt-".into() } else { v };
                                wb.save_prefs();
                                wb.notify_toast("已更新隔离分支名前缀", cx);
                            }),
                    ),
            );

        div()
            .id("settings-projects-git-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                settings_page_shell()
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("项目文件夹与外部编辑器", "Projects & External Editor"), s))
                            .child(projects_card),
                    )
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("Git 提交与临时分支清理", "Git Commits & Cleanup"), s))
                            .child(git_everyday_card),
                    )
                    .child(
                        div()
                            .child(settings_section_title(loc.pick("当前项目常用命令", "Project Quick Scripts"), s))
                            .child(scripts_card),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                soft_button(
                                    "projects-toggle-forge-adv",
                                    loc.pick("访问令牌与分支前缀", "Access tokens & branch prefix"),
                                    s,
                                )
                                .child(pro_icon(
                                    if git_adv_open { ProIcon::ChevronUp } else { ProIcon::ChevronDown },
                                    ICON_XS,
                                    s.muted,
                                ))
                                .on_press(cx, move |_, _, cx| {
                                    git_adv_toggle.update(cx, |v, cx| {
                                        *v = !*v;
                                        cx.notify();
                                    });
                                }),
                            )
                            .when(git_adv_open, |d| d.child(forge_advanced_card)),
                    ),
            )
            .into_any_element()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 5. Mobile Simulators, SSH Devices & Local Dev Preview (`localhost:PORT`)
    // ═══════════════════════════════════════════════════════════════════════

    pub(crate) fn render_devices_preview_settings(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let stacked = f32::from(window.viewport_size().width) < 620.;

        // Local web preview card
        let pill_sw = Self::settings_switch(
            "dev-preview-pill-sw",
            loc.pick(
                "检测到本地服务端口时在对话顶栏显示预览按钮",
                "Show localhost preview pill in chat header",
            ),
            self.prefs.auto_show_dev_preview_pill,
            |wb, cx| {
                wb.prefs.auto_show_dev_preview_pill = !wb.prefs.auto_show_dev_preview_pill;
                wb.save_prefs();
                cx.notify();
            },
            s,
            window,
            cx,
        );

        let mut ports_row = div()
            .font_features(tnum_features())
            .flex()
            .flex_wrap()
            .gap(px(8.));
        if self.detected_dev_ports.is_empty() {
            ports_row = ports_row.child(div().text_size(s.font_size(FS_META)).text_color(s.muted).child(
                loc.pick(
                    "暂未检测到正在运行的本地网页服务（启动开发服务后可点右侧按钮扫描）",
                    "No active local dev ports detected yet.",
                ),
            ));
        } else {
            for &port in &self.detected_dev_ports {
                let url = format!("http://localhost:{port}");
                ports_row = ports_row.child(
                    soft_button_sm(
                        ("open-dev-port", port as usize),
                        format!("localhost:{port} ↗"),
                        s,
                    )
                    .on_press(cx, move |wb, win, cx| {
                        wb.open_url_in_browser(&url, false, win, cx);
                    }),
                );
            }
        }

        let preview_card = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("自动提示本地网页预览", "Auto-detect local dev servers"),
                loc.pick(
                    "终端输出或本机监听出现 localhost:3000/5173/8080 等端口时一键打开预览。",
                    "Surface one-click browser preview when localhost ports open.",
                ),
                pill_sw,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(
                div()
                    .px(px(16.))
                    .py(px(12.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(div().flex_1().min_w_0().child(ports_row))
                    .child(
                        outline_button(
                            "scan-local-ports",
                            loc.pick("扫描本机端口", "Scan ports"),
                            s,
                        )
                        .on_press(cx, |wb, _, cx| {
                            wb.detected_dev_ports =
                                crate::forge_devices_voice::probe_local_dev_ports(&[]);
                            cx.notify();
                        }),
                    ),
            );

        use crate::hardware::DeviceAction;
        let enabled = self.devices.settings["enabled"].as_bool().unwrap_or(false);
        let agent_access = self.devices.settings["agent_access_enabled"]
            .as_bool()
            .unwrap_or(false);
        let toggle = Self::settings_switch(
            "kar-devices-enabled",
            loc.pick("启用设备发现", "Enable device discovery"),
            enabled,
            move |wb, cx| {
                wb.device_command(Some(DeviceAction::Configure(serde_json::json!({"enabled":!wb.devices.settings["enabled"].as_bool().unwrap_or(false)}))));
                cx.notify();
            },
            s,
            window,
            cx,
        );
        let access = Self::settings_switch(
            "devices-agent-access",
            loc.pick("允许智能体调试设备", "Allow agent device access"),
            agent_access,
            move |wb, cx| {
                wb.device_command(Some(DeviceAction::Configure(serde_json::json!({"agent_access_enabled":!wb.devices.settings["agent_access_enabled"].as_bool().unwrap_or(false)}))));
                cx.notify();
            },
            s,
            window,
            cx,
        );
        let mut dev_card = card_box(s)
            .flex()
            .flex_col()
            .child(settings_row_item(
                loc.pick("设备发现", "Device discovery"),
                loc.pick(
                    "查看本机及已连接主机的模拟器和真机。",
                    "Discover devices on this machine and configured hosts.",
                ),
                toggle,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("智能体设备调试", "Agent device access"),
                loc.pick(
                    "允许智能体通过当前对话调试设备。",
                    "Allow the current chat to inspect devices.",
                ),
                access,
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("已连接设备列表", "Connected devices"),
                format!(
                    "{} {} {}",
                    loc.pick("当前发现", "Detected"),
                    self.devices.devices.len(),
                    loc.pick("台模拟器或真机", "simulators or devices")
                ),
                soft_button_sm("refresh-simulators", loc.pick("刷新列表", "Refresh"), s)
                    .on_press(cx, |wb, _, _| wb.device_command(None))
                    .into_any_element(),
                stacked,
                s,
            ));
        for host in &self.devices.hosts {
            if enabled {
                for platform in host["platforms"].as_array().into_iter().flatten() {
                    if let Some(reason) = platform["reason"].as_str() {
                        dev_card = dev_card.child(
                            div()
                                .px(px(16.))
                                .py(px(3.))
                                .text_size(s.font_size(FS_META))
                                .text_color(s.muted)
                                .child(format!(
                                    "{} · {} · {reason}",
                                    host["label"].as_str().unwrap_or(""),
                                    platform["platform"].as_str().unwrap_or("")
                                )),
                        );
                    }
                }
            }
        }
        for (ix, dev) in self.devices.devices.clone().into_iter().enumerate() {
            let screenshot = dev.clone();
            let preview = dev.clone();
            let power = dev.clone();
            let chat = self.current_chat_id();
            let port = self.detected_dev_ports.first().copied();
            let mut actions = div().flex().items_center().gap(px(6.)).flex_wrap();
            if dev.booted {
                actions = actions.child(
                    soft_button_sm(("device-screenshot", ix), loc.pick("截图", "Screenshot"), s)
                        .on_press(cx, move |wb, _, _| {
                            wb.device_command(Some(DeviceAction::Screenshot(
                                screenshot.clone(),
                                wb.config.chat_dir.clone(),
                            )))
                        }),
                );
                if let Some(port) = port.filter(|_| dev.host_id == "local" && !dev.physical) {
                    actions = actions.child(
                        soft_button_sm(
                            ("device-preview", ix),
                            loc.pick("打开预览", "Open preview"),
                            s,
                        )
                        .on_press(cx, move |wb, _, _| {
                            wb.device_command(Some(DeviceAction::Preview(preview.clone(), port)))
                        }),
                    );
                }
            }
            if !dev.physical {
                let label = if dev.booted {
                    loc.pick("关机", "Shut down")
                } else {
                    loc.pick("启动", "Boot")
                };
                actions = actions.child(
                    outline_button(("device-power", ix), label, s)
                        .h(px(CTRL_SM_H))
                        .px(px(10.))
                        .disabled(!dev.booted && chat.is_none())
                        .on_press(cx, move |wb, _, _| {
                            if power.booted {
                                wb.device_command(Some(DeviceAction::Shutdown(power.clone())));
                            } else if let Some(id) = chat {
                                wb.device_command(Some(DeviceAction::Boot(power.clone(), id)));
                            }
                        }),
                );
            }
            dev_card = dev_card.child(settings_divider(s)).child(settings_row_item(
                dev.name,
                format!("{} · {} · {}", dev.host_id, dev.platform, dev.version),
                actions.into_any_element(),
                stacked,
                s,
            ));
        }

        // SSH Host Config
        let ssh_label_inp = window.use_keyed_state("dev-ssh-label", cx, |win, cx| {
            Input::new(
                "dev-ssh-lbl",
                loc.pick("远端设备备注", "Host label"),
                win,
                cx,
            )
            .with_placeholder("Mac mini CI", cx)
        });
        let ssh_dest_inp = window.use_keyed_state("dev-ssh-dest", cx, |win, cx| {
            Input::new(
                "dev-ssh-dst",
                loc.pick("SSH 地址 (user@host)", "SSH destination"),
                win,
                cx,
            )
            .with_placeholder("dev@192.168.1.20", cx)
        });
        let sl = ssh_label_inp.clone();
        let sd = ssh_dest_inp.clone();
        let mut ssh_card = card_box(s)
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_end()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(div().flex_1().flex_basis(px(132.)).min_w_0().child(ssh_label_inp))
                    .child(div().flex_1().flex_basis(px(180.)).min_w_0().child(ssh_dest_inp))
                    .child(
                        soft_button("dev-ssh-add", loc.pick("添加 SSH 主机", "Add SSH host"), s)
                            .on_press(cx, move |wb, win, cx| {
                                let label = sl.read(cx).value(cx).trim().to_owned();
                                let dest = sd.read(cx).value(cx).trim().to_owned();
                                if label.is_empty() || dest.is_empty() {
                                    return;
                                }
                                wb.device_command(Some(DeviceAction::AddSsh(vec![serde_json::json!({"id":Uuid::new_v4(),"label":label,"target":dest,"port":22})])));
                                sl.update(cx, |i, cx| i.set_value("", win, cx));
                                sd.update(cx, |i, cx| i.set_value("", win, cx));
                                cx.notify();
                            }),
                    ),
            );

        if !self.prefs.device_ssh_hosts.is_empty() {
            let title = format!(
                "{} ({})",
                loc.pick("导入已保存的主机", "Import saved hosts"),
                self.prefs.device_ssh_hosts.len()
            );
            ssh_card = ssh_card.child(soft_button_sm("devices-import-hosts", title, s).on_press(
                cx,
                |wb, _, _| {
                    let hosts = wb
                        .prefs
                        .device_ssh_hosts
                        .iter()
                        .map(|h| h.backend_host())
                        .collect();
                    wb.device_command(Some(DeviceAction::AddSsh(hosts)));
                },
            ));
        }

        let ssh_adv_state = window.use_keyed_state("devices-ssh-open", cx, |_, _| {
            !self.prefs.device_ssh_hosts.is_empty()
        });
        let ssh_adv_open = *ssh_adv_state.read(cx);
        let ssh_adv_toggle = ssh_adv_state.clone();

        div()
            .id("settings-devices-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                settings_page_shell()
                    .child(
                        div()
                            .child(settings_section_title(
                                loc.pick("本地网页实时预览", "Local Dev Server Previews"),
                                s,
                            ))
                            .child(preview_card),
                    )
                    .child(
                        div()
                            .child(settings_section_title(
                                loc.pick(
                                    "手机模拟器与真机调试",
                                    "Mobile Simulators & Emulators",
                                ),
                                s,
                            ))
                            .child(dev_card),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(10.))
                            .child(
                                soft_button(
                                    "devices-toggle-ssh",
                                    if ssh_adv_open {
                                        loc.pick("收起 SSH 远端测试主机设置 ▴", "Hide SSH device hosts ▴")
                                    } else {
                                        loc.pick("连接远端 Mac / Linux 测试主机（SSH 高级） ▾", "Connect SSH device hosts ▾")
                                    },
                                    s,
                                )
                                .on_press(cx, move |_, _, cx| {
                                    ssh_adv_toggle.update(cx, |v, cx| {
                                        *v = !*v;
                                        cx.notify();
                                    });
                                }),
                            )
                            .when(ssh_adv_open, |d| d.child(ssh_card)),
                    ),
            )
            .into_any_element()
    }

    // ═══════════════════════════════════════════════════════════════════════
    // 6. Maintenance, Logs, Legacy Transcript Import & About (`kantor_update`)
    // ═══════════════════════════════════════════════════════════════════════

    pub(crate) fn render_maintenance_about_settings(
        &mut self,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let stacked = f32::from(window.viewport_size().width) < 620.;
        let sub_tab_state = window.use_keyed_state("maint-sub-tab", cx, |_, _| 0u8);
        let sub_tab = *sub_tab_state.read(cx);

        let st_for_strip = sub_tab_state.clone();
        let nav_strip = SelectionStrip::options(
            "maint-subtab-strip",
            loc.pick("维护与关于分区", "Section"),
            Group::Choices,
            [
                (0u8, loc.pick("实时诊断日志", "Diagnostics")),
                (1u8, loc.pick("存储、防休眠与导入", "Storage & Import")),
                (2u8, loc.pick("关于与版本更新", "About & Updates")),
            ],
            sub_tab,
            s,
            cx,
            move |wb, next, _, cx| {
                if next == 1 {
                    wb.bridge.send(KarCommand::InspectStorage { clean: false });
                    if wb.external_transcripts.is_empty() {
                        wb.external_transcripts =
                            crate::forge_devices_voice::scan_external_transcripts();
                    }
                }
                st_for_strip.update(cx, |v, _| *v = next);
                cx.notify();
            },
        );

        let top_bar = div()
            .w_full()
            .px(px(24.))
            .pt(px(14.))
            .pb(px(10.))
            .border_b_1()
            .border_color(s.line_soft)
            .flex()
            .items_center()
            .justify_between()
            .child(nav_strip);

        if sub_tab == 0 {
            self.sync_log_stream_widget(window, cx);
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(top_bar)
                .child(div().flex_1().min_h_0().child(self.render_logs_panel()))
                .into_any_element();
        }

        if sub_tab == 1 {
            let storage_label = self.storage_breakdown.as_ref().map(|summary| {
                let mb = |n: u64| format!("{:.1} MB", n as f64 / 1_048_576.);
                format!("{} {} · {} {} · {} {}",
                    loc.pick("总计", "Total"), mb(summary.store_bytes),
                    loc.pick("项目目录", "Catalog"), mb(summary.catalog_bytes),
                    loc.pick("过期上传与旧日志", "Expired uploads and old logs"), mb(summary.expired_upload_bytes + summary.rotated_log_bytes))
            }).unwrap_or_else(|| loc.pick("正在读取工作台存储…", "Reading workbench storage…").into());
            let awake_sw = Self::settings_switch(
                "maint-keep-awake",
                loc.pick("智能体执行长任务期间防止电脑休眠", "Prevent sleep while agents are working"),
                self.prefs.keep_awake_while_working,
                |wb, cx| {
                    wb.prefs.keep_awake_while_working = !wb.prefs.keep_awake_while_working;
                    wb.caffeinate_guard
                        .sync(wb.prefs.keep_awake_while_working && wb.turn_in_progress());
                    wb.save_prefs();
                    cx.notify();
                },
                s,
                window,
                cx,
            );

            let storage_card = card_box(s)
                .flex()
                .flex_col()
                .child(settings_row_item(
                    loc.pick("长任务运行期间防止电脑休眠", "Keep Mac awake while working"),
                    loc.pick("当智能体正在执行长耗时任务或定时任务时，自动阻止电脑合盖/息屏断网。", "Keep system awake during active agent turns."),
                    awake_sw,
                    stacked,
                    s,
                ))
                .child(settings_divider(s))
                .child(settings_row_item(
                    loc.pick("本地缓存与日志空间", "Local disk footprint"),
                    storage_label,
                    outline_button("maint-clean-cache", loc.pick("清理过期缓存与旧日志", "Clean expired cache and logs"), s)
                        .on_press(cx, |wb, _, _| wb.bridge.send(KarCommand::InspectStorage { clean: true }))
                        .into_any_element(),
                    stacked,
                    s,
                ));

            let mut import_card = card_box(s)
                .flex()
                .flex_col()
                .child(
                    div()
                        .px(px(16.))
                        .py(px(12.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_size(s.font_size(FS_META))
                                .text_color(s.muted)
                                .child(loc.pick(
                                    "自动扫描本机 ~/.claude、~/.codex、~/.pi 历史对话记录并一键导入接力",
                                    "Scan ~/.claude, ~/.codex, and ~/.pi session logs for 1-click import",
                                )),
                        )
                        .child(
                            soft_button_sm("maint-rescan-ext", loc.pick("重新扫描", "Rescan"), s)
                                .on_press(cx, |wb, _, cx| {
                                    wb.external_transcripts =
                                        crate::forge_devices_voice::scan_external_transcripts();
                                    cx.notify();
                                }),
                        ),
                );

            for (ix, item) in self.external_transcripts.clone().into_iter().take(12).enumerate() {
                let src = item.source_agent.clone();
                let p = item.path.clone();
                import_card = import_card.child(settings_divider(s)).child(
                    div()
                        .px(px(16.))
                        .py(px(10.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(s.font_size(FS_UI))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(s.ink)
                                        .child(format!("[{}] {}", item.source_agent, item.title)),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .font_family(mono_font(cx))
                                        .text_size(s.font_size(FS_META))
                                        .text_color(s.muted)
                                        .child(item.path.display().to_string()),
                                ),
                        )
                        .child(
                            soft_button_sm(("ext-imp", ix), loc.pick("导入为本地对话", "Import"), s)
                                .on_press(cx, move |wb, _, cx| {
                                    let rt = wb.selected_runtime.clone();
                                    let ws = wb.effective_workspace_dir();
                                    wb.close_all_overlays(cx);
                                    wb.bridge.send(KarCommand::ImportExternalTranscript {
                                        source_agent: src.clone(),
                                        path: p.clone(),
                                        workspace: ws,
                                        target_runtime: rt,
                                    });
                                }),
                        ),
                );
            }

            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(top_bar)
                .child(
                    div()
                        .id("maint-storage-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(
                            settings_page_shell()
                                .child(
                                    div()
                                        .child(settings_section_title(loc.pick("存储治理与防休眠", "Storage & Power"), s))
                                        .child(storage_card),
                                )
                                .child(
                                    div()
                                        .child(settings_section_title(loc.pick("导入外部历史对话记录", "Import External Chat History"), s))
                                        .child(import_card),
                                ),
                        ),
                )
                .into_any_element();
        }

        // Sub-tab 2: About & Software Updates (`kantor_update`)
        let channel_strip = SelectionStrip::options(
            "about-update-channel",
            loc.pick("软件更新通道", "Update channel"),
            Group::Choices,
            [
                ("stable".to_owned(), loc.pick("稳定版 (Stable)", "Stable").to_owned()),
                ("nightly".to_owned(), loc.pick("每夜预览 (Nightly)", "Nightly").to_owned()),
            ],
            self.prefs.update_channel.clone(),
            s,
            cx,
            |wb, ch, _, cx| {
                wb.prefs.update_channel = ch;
                wb.save_prefs();
                cx.notify();
            },
        );

        let report = self.desktop_update_report.clone();
        let install = report.as_ref().and_then(|r| r.request_id);
        let releases = report.as_ref().and_then(|r| r.notes_url.clone());

        let about_card = card_box(s)
            .flex()
            .flex_col()
            .child(
                div()
                    .p(px(20.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(14.))
                            .child(render_kar_brand_lockup(s, cx))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .font_features(tnum_features())
                                            .text_size(s.font_size(FS_TITLE))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(s.ink)
                                            .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                                    )
                                    .child(
                                        div()
                                            .text_size(s.font_size(FS_META))
                                            .text_color(s.muted)
                                            .child(format!(
                                                "Pure Rust + GPUI Native Workbench · {}-{}",
                                                std::env::consts::OS,
                                                std::env::consts::ARCH
                                            )),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(
                                soft_button("about-check-update", loc.pick("检查桌面端更新", "Check for updates"), s)
                                    .on_press(cx, |wb, _, _| {
                                        wb.bridge.send(KarCommand::CheckDesktopUpdate {
                                            channel: wb.prefs.update_channel.clone(),
                                            install: None,
                                        });
                                    }),
                            )
                            .when_some(releases, |row, url| row.child(soft_button("about-releases", loc.pick("查看发布版本", "View releases"), s).on_press(cx, move |_, _, cx| cx.open_url(&url))))
                            .when_some(install, |row, id| {
                                row.child(
                                    primary_button("about-apply-update", loc.pick("安装并重新启动", "Install & relaunch"), s)
                                        .on_press(cx, move |wb, _, _| {
                                            wb.bridge.send(KarCommand::CheckDesktopUpdate {
                                                channel: wb.prefs.update_channel.clone(),
                                                install: Some(id),
                                            });
                                        }),
                                )
                            }),
                    ),
            )
            .when_some(report, |card, rep| {
                card.child(settings_divider(s)).child(
                    div()
                        .px(px(20.))
                        .py(px(10.))
                        .text_size(s.font_size(FS_META))
                        .text_color(s.text)
                        .child(rep.status_message),
                )
            })
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("发布通道", "Release channel"),
                loc.pick("通过 Ed25519 数字签名与 SHA-256 校验安全获取更新包。", "Verified via Ed25519 manifest signatures and SHA-256."),
                channel_strip.into_any_element(),
                stacked,
                s,
            ))
            .child(settings_divider(s))
            .child(settings_row_item(
                loc.pick("隐私与开源组件声明", "Privacy & Open-Source Licenses"),
                loc.pick("本地优先架构：所有对话、文档与配对密钥均保存在本机目录，零云端中转收集。基于 GPUI、KUI、Kar SDK 与 Kantor 引擎构建。", "Local-first architecture: all journals, Office/PDF docs, and HMAC keys remain on your device."),
                render_kbd("100% Local Rust", s).into_any_element(),
                stacked,
                s,
            ));

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(top_bar)
            .child(
                div()
                    .id("maint-about-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        settings_page_shell().child(
                            div()
                                .child(settings_section_title(loc.pick("关于 Kwrok 与软件更新", "About Kwrok & Updates"), s))
                                .child(about_card),
                        ),
                    ),
            )
            .into_any_element()
    }
}
