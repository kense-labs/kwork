//! Git changes, index and working tree diffs, branch selection and commit history.

use crate::ui_motion::selection::{Group, SelectionStrip};
use std::path::{Path, PathBuf};

use gpui::{prelude::*, *};
use kui::{
    components::{
        branch_graph::{
            BranchAuthor, BranchCommit, BranchGraph, BranchGraphOptions, BranchLane,
            BranchPullRequest, BranchStrategy,
        },
        code_block::CodeBlock,
        code_suggestion::{Decision, Hunk, PatchReview},
    },
    primitives::pro_icons::{ProIcon, pro_icon},
};

use crate::{
    kar_bridge::KarCommand,
    studio_hub::{GitFileEntry, GitWorkspaceSnapshot, WorkspaceFilePreview},
    types::{OfficeKind, SelectionLocator, SelectionTarget},
    ui_motion::{
        MenuPlacement, Tactile, copy_text_button, menu_caption, menu_check, menu_divider, menu_host, menu_icon_item,
        menu_item, primary_button, soft_button, text_button,
    },
    ui_primitives::{
        FS_CONTROL, FS_META, FS_SECTION, FS_TAG, FS_UI, ICON_SM, LH_CONTROL, LH_META,
        R_PILL, R_SM, StudioPalette, UiLocale, frosted_card, mono_font, render_git_branch_icon,
        render_sample_tag, tnum_features,
    },
    workbench::{GitSubMode, KarWorkbench, Menu},
};

pub fn build_branch_graph_options(snap: &GitWorkspaceSnapshot) -> BranchGraphOptions {
    let base_branch = if snap.default_branch.is_empty() {
        "main".to_owned()
    } else {
        snap.default_branch.clone()
    };
    let active_branch = snap
        .current_branch
        .clone()
        .unwrap_or_else(|| base_branch.clone());

    let mut lanes = vec![BranchLane::new(base_branch.clone())];
    if active_branch != base_branch {
        lanes.push(BranchLane::new(active_branch.clone()));
    }
    for b in &snap.local_branches {
        if b != &base_branch && b != &active_branch && lanes.len() < 4 {
            lanes.push(BranchLane::new(b.clone()));
        }
    }

    let commits: Vec<BranchCommit> = if snap.commits.is_empty() {
        vec![BranchCommit {
            id: "head".into(),
            hash: "HEAD".into(),
            message: "当前代码仓暂无提交历史或正在加载".into(),
            body: None,
            branch: active_branch.clone().into(),
            parents: Vec::new(),
            author: BranchAuthor {
                name: "Workspace".into(),
                avatar: None,
            },
            date: "刚刚".into(),
            additions: None,
            deletions: None,
            files: None,
        }]
    } else {
        snap.commits
            .iter()
            .map(|c| BranchCommit {
                id: c.id.clone().into(),
                hash: c.hash.clone().into(),
                message: c.message.clone().into(),
                body: c.body.clone().map(Into::into),
                branch: c.branch.clone().into(),
                parents: c.parents.iter().cloned().map(Into::into).collect(),
                author: BranchAuthor {
                    name: c.author.clone().into(),
                    avatar: None,
                },
                date: c.date.clone().into(),
                additions: Some(c.additions),
                deletions: Some(c.deletions),
                files: Some(c.files),
            })
            .collect()
    };

    let pr_branch = if active_branch != base_branch {
        active_branch.clone()
    } else {
        lanes
            .get(1)
            .map(|l| l.name.to_string())
            .unwrap_or_else(|| base_branch.clone())
    };

    BranchGraphOptions {
        commits,
        branches: lanes,
        pull_request: BranchPullRequest {
            number: 1,
            title: format!("{pr_branch} → {base_branch}").into(),
            branch: pr_branch.into(),
        },
        strategy: None,
        default_strategy: BranchStrategy::Open,
        strategies: vec![
            BranchStrategy::Open,
            BranchStrategy::Merge,
            BranchStrategy::Rebase,
            BranchStrategy::Squash,
        ],
        title: Some("分支拓扑与合并策略模拟".into()),
        autoplay: false,
        speed: 1.0,
        paused: true,
        accent: None,
        label: Some("Git 分支历史图".into()),
    }
}

impl KarWorkbench {
    pub(crate) fn rebuild_git_branch_graph(&mut self, cx: &mut Context<Self>) {
        let opts = build_branch_graph_options(&self.git_snapshot);
        self.git_branch_graph = Some(cx.new(|cx| BranchGraph::new(opts, cx)));
    }

    pub(crate) fn render_git_workbench_body(
        &mut self,
        pane_w: f32,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.git_snapshot.is_demo && self.git_branch_graph.is_none() {
            self.rebuild_git_branch_graph(cx);
        }

        let snap = self.git_snapshot.clone();
        let ws = self.effective_workspace_dir();
        let loc = self.ui_locale;
        if (!snap.is_repo && !snap.is_demo) || snap.error.is_some() {
            let (title, detail) = match snap.error.as_deref() {
                Some(error) => (loc.pick("无法读取 Git 仓库", "Couldn’t read Git repository"), error),
                None => (
                    loc.pick("当前文件夹没有 Git 仓库", "No Git repository in this folder"),
                    loc.pick("选择已有仓库，查看分支和文件更改。", "Choose a repository to see its branches and changes."),
                ),
            };
            return crate::ui_primitives::pane_empty(render_git_branch_icon(20., s.muted), title, detail, s)
            .child(
                soft_button("git-pick-project", loc.pick("选择项目文件夹…", "Choose project folder…"), s)
                    .mt(px(16.))
                    .on_press(cx, |this, win, cx| this.pick_workspace(win, cx)),
            )
            .into_any_element();
        }
        let branch_name = snap.current_branch.clone().unwrap_or_else(|| "HEAD".into());
        let dirty_count = snap.dirty_count();
        let staged_count = snap.staged_count();

        let commit_open = window.use_keyed_state("git-show-commit-form", cx, |_, _| false);
        let show_commit = *commit_open.read(cx);

        let is_changes = self.git_sub_mode == GitSubMode::Changes;
        let ws_for_forge = ws.clone();
        let sub_mode_pill = SelectionStrip::options(
            "git-views",
            loc.pick("Git 视图", "Git views"),
            Group::Tabs,
            [
                (
                    GitSubMode::Changes,
                    format!("{} {dirty_count}", loc.pick("更改", "Changes")),
                ),
                (
                    GitSubMode::Graph,
                    if snap.is_demo {
                        loc.pick("拓扑", "Demo")
                    } else {
                        loc.pick("历史", "History")
                    }
                    .to_owned(),
                ),
                (GitSubMode::PullRequests, "PR".to_owned()),
                (GitSubMode::Issues, loc.pick("议题", "Issues").to_owned()),
            ],
            self.git_sub_mode,
            s,
            cx,
            move |wb, mode, _, cx| {
                wb.git_sub_mode = mode;
                if matches!(mode, GitSubMode::PullRequests | GitSubMode::Issues) {
                    wb.bridge.send(KarCommand::RefreshForgeData {
                        workspace: ws_for_forge.clone(),
                    });
                }
                cx.notify();
            },
        );

        // ── Branch Picker Trigger + Menu ────────────────────────────────────
        let compact_toolbar = pane_w < 470.;
        let branch_trigger = Tactile::ghost("git-branch-trigger", s)
            .radius(5.)
            .h(px(28.))
            .px(px(6.))
            .gap(px(5.))
            .lit(self.open_menu == Some(Menu::GitBranch))
            .aria(format!("{} · {}", loc.pick("切换分支", "Switch branch"), branch_name))
            .tip(loc.pick("切换或新建本地分支", "Switch or create a local branch"))
            .child(render_git_branch_icon(12., s.text))
            .child(
                div()
                    .max_w(px(if compact_toolbar { 74. } else { 132. }))
                    .truncate()
                    .text_size(px(FS_CONTROL))
                    .line_height(px(LH_CONTROL))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.text)
                    .child(branch_name.clone()),
            )
            .child(pro_icon(ProIcon::ChevronDown, 10., s.text))
            .on_press(cx, |this, _, cx| this.toggle_menu(Menu::GitBranch, cx));

        let branch_menu =
            self.render_git_branch_menu(Menu::GitBranch, &snap, &branch_name, MenuPlacement::BelowStart, s, cx);

        let ws_refresh = ws.clone();
        let ws_pull = ws.clone();
        let ws_push = ws.clone();

        let ws_stage = ws.clone();
        let ws_unstage = ws.clone();
        let index_paths: Vec<_> = snap
            .files
            .iter()
            .filter(|f| f.is_staged())
            .map(|f| f.path.clone())
            .collect();
        let mut remote_rows = vec![
            menu_icon_item(
                "git-menu-stage-all",
                ProIcon::Plus,
                loc.pick("全部暂存", "Stage all changes"),
                s,
            )
            .disabled(snap.is_demo || !snap.files.iter().any(|f| f.has_unstaged()))
            .on_press(cx, move |wb, _, cx| {
                wb.open_menu = None;
                wb.bridge.send(KarCommand::GitStageAll {
                    workspace: ws_stage.clone(),
                });
                cx.notify();
            })
            .into_any_element(),
            menu_icon_item(
                "git-menu-unstage-all",
                ProIcon::Minus,
                loc.pick("全部取消暂存", "Unstage all changes"),
                s,
            )
            .disabled(snap.is_demo || index_paths.is_empty())
            .on_press(cx, move |wb, _, cx| {
                wb.open_menu = None;
                wb.bridge.send(KarCommand::GitUnstagePaths {
                    workspace: ws_unstage.clone(),
                    paths: index_paths.clone(),
                });
                cx.notify();
            })
            .into_any_element(),
            menu_divider(s).into_any_element(),
            menu_icon_item("git-pull-btn", ProIcon::Download, loc.pick("拉取上游更改", "Pull from upstream"), s)
                .disabled(snap.is_demo || snap.upstream.is_none())
                .when(snap.upstream.is_none(), |item| item.tip(loc.pick("当前分支未设置上游", "This branch has no upstream")))
                .on_press(cx, move |this, _, cx| {
                    this.open_menu = None;
                    this.bridge.send(KarCommand::GitPull {
                        workspace: ws_pull.clone(),
                    });
                    cx.notify();
                })
                .into_any_element(),
            menu_icon_item("git-push-btn", ProIcon::Upload, loc.pick("推送当前分支", "Push current branch"), s)
                .disabled(snap.is_demo || !snap.has_origin || snap.current_branch.is_none())
                .when(!snap.has_origin, |item| item.tip(loc.pick("尚未配置 origin 远端", "No origin remote configured")))
                .on_press(cx, move |this, _, cx| {
                    this.open_menu = None;
                    this.bridge.send(KarCommand::GitPush {
                        workspace: ws_push.clone(),
                    });
                    cx.notify();
                })
                .into_any_element(),
            menu_divider(s).into_any_element(),
            menu_icon_item(
                "git-open-external-editor",
                ProIcon::ExternalArrow,
                loc.pick("在外部编辑器中打开仓库", "Open in external editor"),
                s,
            )
            .on_press(cx, |this, _, cx| {
                this.open_menu = None;
                this.open_workspace_in_preferred_editor(None, cx);
            })
            .into_any_element(),
        ];
        if compact_toolbar {
            let workspace = ws.clone();
            let refresh = menu_icon_item(
                "git-menu-refresh",
                ProIcon::Refresh,
                loc.pick("刷新变更", "Refresh changes"),
                s,
            )
            .on_press(cx, move |this, _, cx| {
                this.open_menu = None;
                this.bridge.send(KarCommand::RefreshGitWorkspace {
                    workspace: workspace.clone(),
                });
                cx.notify();
            });
            remote_rows.insert(0, refresh.into_any_element());
        }
        let remote_menu = menu_host(
            crate::ui_motion::icon_button(
                "git-more",
                ProIcon::MoreHorizontal,
                loc.pick("更多 Git 操作", "More Git actions"),
                s,
            )
            .lit(self.open_menu == Some(Menu::GitMore))
            .on_press(cx, |this, _, cx| this.toggle_menu(Menu::GitMore, cx)),
            self.render_menu(
                Menu::GitMore,
                MenuPlacement::BelowEnd,
                192.,
                remote_rows,
                s,
                cx,
            ),
        );

        let status_strip = div()
            .min_h(px(40.))
            .px(px(8.))
            .py(px(4.))
            .border_b_1()
            .border_color(s.line_soft)
            .bg(s.surface)
            .flex_none()
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
                    .child(sub_mode_pill)
                    .child(menu_host(branch_trigger, branch_menu))
                    .when(snap.ahead > 0 || snap.behind > 0, |row| {
                        row.child(
                            div()
                                .h(px(20.))
                                .px(px(6.))
                                .rounded(px(5.))
                                .bg(s.soft)
                                .font_features(tnum_features())
                                .text_size(px(FS_TAG))
                                .text_color(s.text)
                                .flex()
                                .items_center()
                                .child(format!("↑{} ↓{}", snap.ahead, snap.behind)),
                        )
                    })
                    .when(snap.is_demo, |row| row.child(render_sample_tag(s))),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.))
                    .when(!compact_toolbar, |row| {
                        row.child(crate::ui_motion::icon_button(
                            "git-refresh-btn",
                            ProIcon::Refresh,
                            loc.pick("刷新变更", "Refresh changes"),
                            s,
                        )
                        .on_press(cx, move |this, _, _| {
                            this.bridge.send(KarCommand::RefreshGitWorkspace {
                                workspace: ws_refresh.clone(),
                            });
                        }))
                    })
                    .child(remote_menu)
                    .when(is_changes && dirty_count > 0 && !snap.is_demo, |d| {
                        d.child(
                            crate::ui_motion::icon_button(
                                "git-toggle-commit",
                                ProIcon::Check,
                                loc.pick("编写提交说明", "Write commit message"),
                                s,
                            )
                            .lit(show_commit)
                            .on_press(cx, move |this, window, cx| {
                                commit_open.update(cx, |open, cx| {
                                    *open = !*open;
                                    cx.notify();
                                });
                                if *commit_open.read(cx) {
                                    window.focus(&this.git_commit_input.focus_handle(cx), cx);
                                }
                            }),
                        )
                    })
                    .child(
                        text_button(
                            "git-ai-review-btn",
                            loc.pick("起草审查请求", "Draft review"),
                            s,
                        )
                        .tip(loc.pick("将当前分支和改动文件填入输入框，确认发送后由智能体审查", "Add the current branch and changed files to a review draft you can edit and send"))
                        .disabled(
                            dirty_count == 0
                                || snap.is_demo
                                || !(self.access_mode.is_writable()
                                    || matches!(
                                        self.access_mode,
                                        crate::types::ChatAccessMode::NoneSelected
                                    )),
                        )
                        .on_press(cx, move |this, win, cx| {
                            this.pin_git_changes_to_composer(win, cx);
                        }),
                    ),
            );

        let body = match self.git_sub_mode {
            GitSubMode::Changes => self.render_git_changes_section(
                &snap,
                &ws,
                staged_count,
                dirty_count,
                show_commit,
                s,
                cx,
            ),
            GitSubMode::Graph => self.render_git_graph_section(&snap, s, cx),
            GitSubMode::PullRequests => self.render_git_prs_section(&snap, &ws, s, window, cx),
            GitSubMode::Issues => self.render_git_issues_section(&ws, s, window, cx),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .child(status_strip)
            .child(
                div()
                    .id("kar-git-workbench-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(8.))
                    .child(body),
            )
            .into_any_element()
    }

    pub(crate) fn render_git_branch_menu(
        &self,
        menu: Menu,
        snap: &GitWorkspaceSnapshot,
        current: &str,
        placement: MenuPlacement,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let ws = self.effective_workspace_dir();
        let mut rows: Vec<AnyElement> =
            vec![menu_caption(loc.pick("切换本地分支", "Switch branch"), s).into_any_element()];

        for (ix, br) in snap.local_branches.iter().enumerate() {
            let is_cur = br == current;
            let br_target = br.clone();
            let ws_sw = ws.clone();
            rows.push(
                menu_item(
                    ("git-br-item", ix),
                    Some(render_git_branch_icon(
                        13.,
                        if is_cur { s.ink } else { s.muted },
                    )),
                    br.clone(),
                    s,
                )
                .when(is_cur, |r| r.child(menu_check(s)))
                .on_press(cx, move |this, _, cx| {
                    this.open_menu = None;
                    if !is_cur {
                        this.bridge.send(KarCommand::GitSwitchBranch {
                            workspace: ws_sw.clone(),
                            branch: br_target.clone(),
                            create: false,
                        });
                    }
                    cx.notify();
                })
                .into_any_element(),
            );
        }

        rows.push(menu_divider(s).into_any_element());
        let ws_new = ws;
        rows.push(
            menu_icon_item(
                "git-br-create",
                ProIcon::Plus,
                loc.pick("新建分支…", "Create new branch…"),
                s,
            )
            .disabled(snap.is_demo)
            .on_press(cx, move |this, window, cx| {
                this.open_menu = None;
                this.branch_workspace = Some(ws_new.clone());
                this.branch_input.update(cx, |field, cx| {
                    field.set_value("", window, cx);
                    field.set_error("", cx);
                });
                this.branch_dialog
                    .update(cx, |dialog, cx| dialog.set_open(true, window, cx));
                cx.notify();
            })
            .into_any_element(),
        );

        self.render_menu(menu, placement, 228., rows, s, cx)
    }

    #[allow(clippy::too_many_arguments)]
    fn render_git_changes_section(
        &mut self,
        snap: &GitWorkspaceSnapshot,
        ws: &Path,
        staged_count: usize,
        dirty_count: usize,
        show_commit: bool,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let ws_commit = ws.to_path_buf();
        let committing = self.committing_workspaces.contains(ws);
        let stage_paths: Vec<_> = snap
            .files
            .iter()
            .filter(|_| staged_count == 0)
            .map(|f| f.path.clone())
            .collect();
        let commit_label = if committing {
            loc.pick("正在提交…", "Committing…").to_owned()
        } else if staged_count > 0 {
            format!("{} ({staged_count})", loc.pick("提交暂存", "Commit staged"))
        } else if dirty_count > 0 {
            format!(
                "{} ({dirty_count})",
                loc.pick("全部暂存并提交", "Stage all and commit")
            )
        } else {
            loc.pick("提交", "Commit").to_owned()
        };

        // ── 1. Top Commit Composer Box (with ✨ AI Commit Writer) ───────────
        let ws_ai_msg = ws.to_path_buf();
        let commit_card = div()
            .w_full()
            .pb(px(12.))
            .border_b_1()
            .border_color(s.line_soft)
            .flex()
            .items_center()
            .gap(px(6.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(self.git_commit_input.clone()),
            )
            .child(
                crate::ui_motion::icon_button(
                    "git-ai-commit-msg",
                    ProIcon::Sparkles,
                    loc.pick("根据当前改动自动生成提交说明", "Generate commit message from diff"),
                    s,
                )
                .disabled(dirty_count == 0 || snap.is_demo || self.generating_commits.contains_key(&ws_ai_msg))
                .on_press(cx, move |this, _, cx| {
                    let draft = this.git_commit_input.read(cx).value(cx).to_string();
                    this.generating_commits.insert(ws_ai_msg.clone(), draft);
                    cx.notify();
                    this.bridge.send(KarCommand::GenerateAiCommitMessage {
                        workspace: ws_ai_msg.clone(),
                        style: this.prefs.commit_writing_style,
                        custom_instructions: this.prefs.commit_custom_instructions.clone(),
                    });
                }),
            )
            .child(
                primary_button("git-commit-submit", commit_label, s)
                    .h(px(32.))
                    .px(px(12.))
                    .disabled(
                        dirty_count == 0
                            || committing
                            || snap.is_demo
                            || self.git_commit_input.read(cx).value(cx).trim().is_empty(),
                    )
                    .on_press(cx, move |this, _, cx| {
                        let msg = this.git_commit_input.read(cx).value(cx).trim().to_owned();
                        if msg.is_empty() {
                            this.notify_warning("请先输入提交说明（例如 feat: ...）", cx);
                            return;
                        }
                        if !this.committing_workspaces.insert(ws_commit.clone()) {
                            return;
                        }
                        cx.notify();
                        this.bridge.send(KarCommand::GitCommit {
                            stage_paths: stage_paths.clone(),
                            workspace: ws_commit.clone(),
                            message: msg,
                        });
                    }),
            );

        if snap.files.is_empty() {
            return div()
                .w_full()
                .pt(px(56.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.))
                .text_size(px(FS_META))
                .text_color(s.muted)
                .child(
                    self.ui_locale
                        .pick("工作树干净，没有未提交更改", "No uncommitted changes"),
                )
                .child(
                    text_button(
                        "git-jump-graph",
                        self.ui_locale.pick("查看提交历史", "View history"),
                        s,
                    )
                    .on_press(cx, |wb, _, cx| {
                        wb.git_sub_mode = GitSubMode::Graph;
                        cx.notify();
                    }),
                )
                .into_any_element();
        }

        let staged_files: Vec<GitFileEntry> = snap
            .files
            .iter()
            .filter(|f| f.is_staged())
            .cloned()
            .collect();
        let unstaged_files: Vec<GitFileEntry> = snap
            .files
            .iter()
            .filter(|f| f.has_unstaged())
            .cloned()
            .collect();

        let mixed = !staged_files.is_empty() && !unstaged_files.is_empty();
        let mut col = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(12.))
            .when(show_commit, |d| d.child(commit_card));

        // ── 2. Staged Files (`已暂存的更改` + Unstage!) ─────────────────────
        if !staged_files.is_empty() {
            let all_staged_paths: Vec<PathBuf> =
                staged_files.iter().map(|f| f.path.clone()).collect();
            let ws_unstage_all = ws.to_path_buf();
            let mut st_box = div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(1.))
                .when(mixed, |rows| {
                    rows.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(FS_META))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child(self.ui_locale.pick("已暂存", "Staged changes")),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .rounded(px(R_PILL))
                                            .bg(s.soft)
                                            .font_features(tnum_features())
                                            .text_size(px(FS_TAG))
                                            .text_color(s.muted)
                                            .child(format!("{}", staged_files.len())),
                                    ),
                            )
                            .child(
                                text_button(
                                    "git-unstage-all",
                                    self.ui_locale.pick("全部取消暂存", "Unstage all"),
                                    s,
                                )
                                .disabled(snap.is_demo)
                                .on_press(cx, move |this, _, _| {
                                    this.bridge.send(KarCommand::GitUnstagePaths {
                                        workspace: ws_unstage_all.clone(),
                                        paths: all_staged_paths.clone(),
                                    });
                                }),
                            ),
                    )
                });

            for (ix, file) in staged_files.iter().take(self.git_staged_limit).enumerate() {
                st_box =
                    st_box.child(self.render_git_file_row(file, ix, true, ws, snap.is_demo, s, cx));
            }
            if staged_files.len() > self.git_staged_limit {
                let next = (staged_files.len() - self.git_staged_limit).min(32);
                st_box = st_box.child(
                    soft_button("more-git_staged_limit", format!("再显示 {next} 个文件"), s)
                        .on_press(cx, |this, _, cx| {
                            this.git_staged_limit += 32;
                            cx.notify();
                        }),
                );
            }
            col = col.child(st_box);
        }

        // ── 3. Unstaged / Untracked Files (`未暂存的更改` + Stage!) ─────────
        if !unstaged_files.is_empty() {
            let ws_stage_all = ws.to_path_buf();
            let mut un_box = div()
                .w_full()
                .flex()
                .flex_col()
                .gap(px(1.))
                .when(mixed, |rows| {
                    rows.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .child(
                                        div()
                                            .text_size(px(FS_META))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(s.ink)
                                            .child(self.ui_locale.pick("工作树", "Working tree")),
                                    )
                                    .child(
                                        div()
                                            .px(px(6.))
                                            .rounded(px(R_PILL))
                                            .bg(s.soft)
                                            .font_features(tnum_features())
                                            .text_size(px(FS_TAG))
                                            .text_color(s.muted)
                                            .child(format!("{}", unstaged_files.len())),
                                    ),
                            )
                            .child(
                                text_button(
                                    "git-stage-all-btn",
                                    self.ui_locale.pick("全部暂存", "Stage all"),
                                    s,
                                )
                                .disabled(snap.is_demo)
                                .child(pro_icon(ProIcon::Plus, 10., s.ink))
                                .on_press(cx, move |this, _, _| {
                                    this.bridge.send(KarCommand::GitStageAll {
                                        workspace: ws_stage_all.clone(),
                                    });
                                }),
                            ),
                    )
                });

            for (ix, file) in unstaged_files
                .iter()
                .take(self.git_unstaged_limit)
                .enumerate()
            {
                un_box = un_box.child(self.render_git_file_row(
                    file,
                    staged_files.len() + ix,
                    false,
                    ws,
                    snap.is_demo,
                    s,
                    cx,
                ));
            }
            if unstaged_files.len() > self.git_unstaged_limit {
                let next = (unstaged_files.len() - self.git_unstaged_limit).min(32);
                un_box = un_box.child(
                    soft_button(
                        "more-git_unstaged_limit",
                        format!("再显示 {next} 个文件"),
                        s,
                    )
                    .on_press(cx, |this, _, cx| {
                        this.git_unstaged_limit += 32;
                        cx.notify();
                    }),
                );
            }
            col = col.child(un_box);
        }

        col.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_git_file_row(
        &mut self,
        f: &GitFileEntry,
        ix: usize,
        staged: bool,
        ws: &Path,
        is_demo: bool,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> Div {
        let badge = if staged {
            f.index_status
        } else {
            f.worktree_status
        };
        let badge_color = match badge {
            'A' | '?' => s.pro.green_ink,
            'D' => s.pro.red_ink,
            _ => s.pro.amber_ink,
        };
        let file_name = f
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| f.path.display().to_string());
        let folder = f
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_string_lossy().into_owned());
        let file_key = (f.path.clone(), staged);
        let expanded = self.expanded_git_diff_file.as_ref() == Some(&file_key);
        let rel_toggle = f.path.clone();
        let rel_open = self
            .git_snapshot
            .repo_root
            .as_deref()
            .unwrap_or(ws)
            .join(&f.path)
            .strip_prefix(ws)
            .ok()
            .map(Path::to_path_buf)
            .filter(|_| f.worktree_status != 'D' && !(staged && f.index_status == 'D'));
        let rel_stage = f.path.clone();
        let ws_toggle = ws.to_path_buf();
        let ws_stage = ws.to_path_buf();

        let inline_patch = if expanded {
            let cache_key = format!("inline-diff-{staged}-{}", f.path.display());
            if let Some(hunks_data) = self.inline_git_diffs.get(&file_key).cloned() {
                if hunks_data.is_empty() {
                    Some(
                        div()
                            .px(px(10.))
                            .py(px(8.))
                            .text_size(px(FS_META))
                            .text_color(s.muted)
                            .child("无可显示的文本行差异（可能为二进制文件或仅权限变更）。")
                            .into_any_element(),
                    )
                } else if self.prefs.diff_layout == crate::prefs::DiffLayoutMode::Split {
                    Some(render_split_diff_hunks(&hunks_data, s, cx).into_any_element())
                } else {
                    let pr = self
                        .patch_review_cache
                        .get_mut()
                        .entry(cache_key)
                        .or_insert_with(|| {
                            let hunks = hunks_data
                                .iter()
                                .map(|h| Hunk {
                                    line: h.start_line,
                                    before: h.before.clone(),
                                    after: h.after.clone(),
                                    decision: Decision::Pending,
                                })
                                .collect();
                            cx.new(|cx| {
                                PatchReview::new(
                                    format!("inline-pr-{}", f.path.display()),
                                    f.path.display().to_string(),
                                    hunks,
                                    cx,
                                )
                                .read_only(mono_font(cx))
                            })
                        })
                        .clone();
                    Some(div().pt(px(6.)).w_full().child(pr).into_any_element())
                }
            } else {
                Some(
                    div()
                        .px(px(10.))
                        .py(px(8.))
                        .text_size(px(FS_META))
                        .text_color(s.muted)
                        .child("正在读取差异块…")
                        .into_any_element(),
                )
            }
        } else {
            None
        };

        div()
            .w_full()
            .rounded(px(5.))
            .when(expanded, |d| d.border_1().border_color(s.line_soft))
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(28.))
                    .px(px(4.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(6.))
                    .child(
                        Tactile::ghost(("git-file-expand", ix), s)
                            .flex_1()
                            .min_w_0()
                            .h(px(26.))
                            .px(px(4.))
                            .gap(px(6.))
                            .tip("展开或收起文件差异")
                            .child(pro_icon(
                                if expanded {
                                    ProIcon::ChevronDown
                                } else {
                                    ProIcon::ChevronRight
                                },
                                10.,
                                s.faint,
                            ))
                            .child(
                                div()
                                    .size(px(14.))
                                    .rounded(px(3.))
                                    .border_1()
                                    .border_color(badge_color)
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .font_family(mono_font(cx))
                                    .text_size(px(FS_TAG))
                                    .line_height(px(12.))
                                    .text_color(badge_color)
                                    .child(match badge {
                                        'A' | '?' => "+",
                                        'D' => "−",
                                        'R' => "R",
                                        _ => "~",
                                    }),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(FS_CONTROL))
                                    .text_color(s.ink)
                                    .child(file_name),
                            )
                            .when_some(folder, |d, folder| {
                                d.child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(FS_META))
                                        .text_color(s.faint)
                                        .child(folder),
                                )
                            })
                            .on_press(cx, move |this, _, cx| {
                                if this.expanded_git_diff_file.as_ref() == Some(&file_key) {
                                    this.expanded_git_diff_file = None;
                                } else {
                                    this.expanded_git_diff_file = Some(file_key.clone());
                                    if !this.inline_git_diffs.contains_key(&file_key) {
                                        this.bridge.send(KarCommand::LoadGitFileDiff {
                                            staged,
                                            workspace: ws_toggle.clone(),
                                            rel_path: rel_toggle.clone(),
                                            is_demo,
                                        });
                                    }
                                }
                                cx.notify();
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .when(
                                (f.added_lines != 0 || f.deleted_lines != 0)
                                    && !(f.is_staged() && f.has_unstaged()),
                                |d| {
                                    d.child(
                                        div()
                                            .font_family(mono_font(cx))
                                            .font_features(tnum_features())
                                            .text_size(px(FS_TAG))
                                            .text_color(s.pro.green_ink)
                                            .child(format!("+{}", f.added_lines)),
                                    )
                                    .child(
                                        div()
                                            .font_family(mono_font(cx))
                                            .font_features(tnum_features())
                                            .text_size(px(FS_TAG))
                                            .text_color(s.pro.red_ink)
                                            .child(format!("−{}", f.deleted_lines)),
                                    )
                                },
                            )
                            .child(
                                Tactile::ghost(("git-file-tab-open", ix), s)
                                    .size(px(24.))
                                    .radius(R_SM)
                                    .justify_center()
                                    .disabled(rel_open.is_none())
                                    .tip("在新标签页打开文件")
                                    .child(pro_icon(ProIcon::ExternalArrow, 11., s.muted))
                                    .on_press(cx, move |this, _, cx| {
                                        if let Some(path) = rel_open.clone() {
                                            this.open_workspace_code_tab(path, cx);
                                        }
                                    }),
                            )
                            .child(if staged {
                                Tactile::ghost(("git-unstage-one", ix), s)
                                    .size(px(24.))
                                    .radius(R_SM)
                                    .justify_center()
                                    .tip(self.ui_locale.pick("取消暂存此文件", "Unstage file"))
                                    .child(pro_icon(ProIcon::Minus, 13., s.muted))
                                    .disabled(is_demo)
                                    .on_press(cx, move |this, _, _| {
                                        this.bridge.send(KarCommand::GitUnstagePaths {
                                            workspace: ws_stage.clone(),
                                            paths: vec![rel_stage.clone()],
                                        });
                                    })
                            } else {
                                Tactile::ghost(("git-stage-one", ix), s)
                                    .size(px(24.))
                                    .radius(R_SM)
                                    .justify_center()
                                    .tip(self.ui_locale.pick("暂存此文件", "Stage file"))
                                    .child(pro_icon(ProIcon::Plus, 13., s.muted))
                                    .disabled(is_demo)
                                    .on_press(cx, move |this, _, _| {
                                        this.bridge.send(KarCommand::GitStagePaths {
                                            workspace: ws_stage.clone(),
                                            paths: vec![rel_stage.clone()],
                                        });
                                    })
                            }),
                    ),
            )
            .when_some(inline_patch, |d, patch| d.child(patch))
    }

    fn render_git_graph_section(
        &self,
        snap: &GitWorkspaceSnapshot,
        s: &StudioPalette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut commits_card = frosted_card(s)
            .w_full()
            .p(px(14.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(FS_META))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(s.ink)
                    .child(self.ui_locale.pick("最近提交记录", "Recent commits")),
            );

        if snap.commits.is_empty() {
            return crate::ui_primitives::pane_empty(
                render_git_branch_icon(20., s.muted),
                self.ui_locale.pick("还没有提交", "No commits yet"),
                self.ui_locale.pick(
                    "提交暂存的更改后，可以在这里查看历史记录。",
                    "Commit your staged changes to start the history.",
                ),
                s,
            )
            .into_any_element();
        }
        for c in &snap.commits {
            commits_card = commits_card.child(
                div()
                    .py(px(5.))
                    .border_t_1()
                    .border_color(s.line_soft)
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
                            .gap(px(8.))
                            .child(
                                div()
                                    .px(px(5.))
                                    .py(px(1.))
                                    .rounded(px(R_SM))
                                    .bg(s.soft)
                                    .font_family(mono_font(cx))
                                    .text_size(px(FS_TAG))
                                    .text_color(s.muted)
                                    .child(c.hash.clone()),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(FS_META))
                                    .text_color(s.ink)
                                    .child(c.message.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(FS_TAG))
                            .text_color(s.faint)
                            .child(format!("{} · {}", c.author, c.date)),
                    ),
            );
        }

        if !snap.is_demo {
            return commits_card.into_any_element();
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                frosted_card(s)
                    .w_full()
                    .p(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(FS_SECTION))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child("分支演进与合并策略预览（KUI BranchGraph）"),
                            )
                            .child(
                                div()
                                    .text_size(px(FS_META))
                                    .line_height(px(LH_META))
                                    .text_color(s.muted)
                                    .child("切换「合并前 / 合并 / 变基 / 压缩」可实时观察提交拓扑重排动画。"),
                            ),
                    )
                    .when_some(self.git_branch_graph.clone(), |d, g| d.child(g)),
            )
            .child(commits_card)
            .into_any_element()
    }

    pub(crate) fn render_workspace_code_tab_body(
        &mut self,
        rel_path: &PathBuf,
        pane_width: f32,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(preview) = self.open_code_tabs.get(rel_path) else {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(FS_UI))
                .text_color(s.muted)
                .child("正在加载工作区文件…")
                .into_any_element();
        };

        let key = preview.rel_path.to_string_lossy().into_owned();
        let code_entity = self
            .code_block_cache
            .entry(key.clone())
            .or_insert_with(|| {
                cx.new(|_| CodeBlock::source_only(preview.content.clone()))
            })
            .clone();

        let patch_entity = if !preview.diff_hunks.is_empty() {
            Some(
                self.patch_review_cache
                    .get_mut()
                    .entry(format!("tab-patch-{key}"))
                    .or_insert_with(|| {
                        let hunks = preview
                            .diff_hunks
                            .iter()
                            .map(|h| Hunk {
                                line: h.start_line,
                                before: h.before.clone(),
                                after: h.after.clone(),
                                decision: Decision::Pending,
                            })
                            .collect();
                        cx.new(|cx| {
                            PatchReview::new(
                                format!("patch-{}", preview.rel_path.display()),
                                preview.rel_path.display().to_string(),
                                hunks,
                                cx,
                            )
                            .read_only(mono_font(cx))
                            .within_file_preview()
                        })
                    })
                    .clone(),
            )
        } else {
            None
        };

        let file_to_pin = rel_path.clone();
        let loc = self.ui_locale;
        let pin_label = loc.pick("引用到对话", "Add to chat");
        let copy = copy_text_button(
            format!("file-preview-copy-{key}"), loc.pick("复制文件内容", "Copy file contents"),
            code_entity.read(cx).source(), window, cx,
        );
        let subbar = div()
            .h(px(36.))
            .px(px(14.))
            .border_b_1()
            .border_color(s.line_soft)
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(10.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .max_w(relative(0.32))
                            .truncate()
                            .px(px(6.))
                            .py(px(1.))
                            .rounded(px(4.))
                            .bg(s.soft)
                            .font_family(mono_font(cx))
                            .text_size(px(FS_TAG))
                            .text_color(s.text)
                            .child(preview.language.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_family(mono_font(cx))
                            .text_size(px(FS_META))
                            .text_color(s.ink)
                            .child(preview.rel_path.display().to_string()),
                    )
                    .when(pane_width >= 450., |row| row.child(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .font_features(tnum_features())
                            .text_size(px(FS_META))
                            .text_color(s.muted)
                            .child(format!("{} {}", preview.total_lines, loc.pick(
                                "行", if preview.total_lines == 1 { "line" } else { "lines" },
                            ))),
                    )),
            )
            .child(copy)
            .child(
                Tactile::ghost("code-pin-to-chat", s)
                    .flex_none()
                    .h(px(28.))
                    .map(|d| if pane_width >= 390. { d.px(px(6.)) } else { d.w(px(28.)) })
                    .justify_center()
                    .tip(pin_label)
                    .gap(px(5.))
                    .text_size(px(FS_META))
                    .text_color(s.ink)
                    .child(pro_icon(ProIcon::Pin, ICON_SM, s.muted))
                    .when(pane_width >= 390., |d| d.child(pin_label))
                    .on_press(cx, move |this, win, cx| {
                        if let Some(preview) = this.open_code_tabs.get(&file_to_pin).cloned() {
                            this.pin_code_file_to_composer(&preview, win, cx);
                        }
                    }),
            );

        div()
            .size_full()
            .flex()
            .flex_col()
            .child(subbar)
            .child(
                div()
                    .id("kar-code-preview-scroll")
                    .w_full()
                    .min_w_0()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .when_some(patch_entity, |col, patch| col.child(patch))
                    .child(code_entity),
            )
            .into_any_element()
    }

    pub fn pin_git_changes_to_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let snap = &self.git_snapshot;
        let branch = snap.current_branch.as_deref().unwrap_or("HEAD");
        let mut files = snap
            .files
            .iter()
            .take(10)
            .map(|f| format!("{} {}", f.badge_char(), f.path.display()))
            .collect::<Vec<_>>()
            .join(", ");
        if snap.files.len() > 10 {
            let remaining = snap.files.len() - 10;
            files.push_str(
                match self.ui_locale {
                    UiLocale::Zh => format!("，另有 {remaining} 个改动文件"),
                    UiLocale::En => format!(", and {remaining} more changed files"),
                }
                .as_str(),
            );
        }
        let prompt = match (self.ui_locale, files.is_empty()) {
            (UiLocale::Zh, true) => {
                format!("当前工作树干净，请审查分支 `{branch}` 的最新一次提交。")
            }
            (UiLocale::En, true) => {
                format!("The working tree is clean. Review the latest commit on `{branch}`.")
            }
            (UiLocale::Zh, false) => format!(
                "请审查当前分支 `{branch}` 的 Git 变更（{files}），检查潜在边界缺陷并给出一段清晰的提交说明。"
            ),
            (UiLocale::En, false) => format!(
                "Review the Git changes on `{branch}` ({files}). Check for edge cases and suggest a commit message."
            ),
        };
        self.fill_starter_prompt(&prompt, window, cx);
        self.notify_toast(
            self.ui_locale.pick(
                "审查请求已填入输入框，可编辑后发送",
                "Review draft added. Edit it before sending.",
            ),
            cx,
        );
    }

    /// Bug #9 Fix: Pins source code using `SelectionLocator::CodeLines` (`[CODE]` badge & accurate
    /// jump back to Right-Dock `CodeFile` tab) instead of pretending to be a Word document!
    pub fn pin_code_file_to_composer(
        &mut self,
        preview: &WorkspaceFilePreview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown_lines = preview.total_lines.clamp(1, 36);
        let excerpt: String = preview
            .content
            .lines()
            .take(shown_lines)
            .collect::<Vec<_>>()
            .join("\n");
        self.pinned_target = Some(SelectionTarget {
            tab_id: 0,
            file_path: Some(preview.abs_path.clone()),
            doc_name: preview.rel_path.display().to_string(),
            kind: OfficeKind::Word,
            base_revision: None,
            locator: SelectionLocator::CodeLines {
                rel_path: preview.rel_path.clone(),
                language: preview.language.clone(),
                start_line: 1,
                end_line: shown_lines,
                total_lines: preview.total_lines,
            },
            excerpt,
        });
        self.reveal_chat_composer(window, cx);
        self.notify_toast(
            format!("已引用源码「{}」到对话", preview.rel_path.display()),
            cx,
        );
        cx.notify();
    }

    pub fn expand_first_git_diff(&mut self, cx: &mut Context<Self>) {
        let Some(first) = self
            .git_snapshot
            .files
            .iter()
            .find(|f| !f.is_untracked())
            .or_else(|| self.git_snapshot.files.first())
            .cloned()
        else {
            return;
        };
        let ws = self.effective_workspace_dir();
        let is_demo = self.git_snapshot.is_demo;
        let staged = first.is_staged();
        let file_key = (first.path.clone(), staged);
        self.expanded_git_diff_file = Some(file_key.clone());
        if !self.inline_git_diffs.contains_key(&file_key) {
            self.bridge.send(KarCommand::LoadGitFileDiff {
                staged,
                workspace: ws,
                rel_path: first.path,
                is_demo,
            });
        }
        cx.notify();
    }

    fn render_git_prs_section(
        &mut self,
        snap: &GitWorkspaceSnapshot,
        ws: &Path,
        s: &StudioPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let current_workspace = self.forge_workspace.as_deref() == Some(ws);
        let prs = if current_workspace { self.forge_prs.as_slice() } else { &[] };
        let chat = match self.access_mode {
            crate::types::ChatAccessMode::WriteController {chat_id} => Some(chat_id),
            _ => None,
        };
        let can_create=chat.is_some() && self.forge_pending.is_none() && !snap.is_demo && snap.current_branch.is_some();
        let field_key = |field: &str| (SharedString::from(format!("git-pr/{field}/{chat:?}/{}",ws.display())),self.forge_draft_epoch);
        let pr_title_inp = window.use_keyed_state(field_key("title"), cx, |win, cx| {
            kui::components::input::Input::new(
                "git-pr-title-inp",
                loc.pick("PR 标题", "Pull Request title"),
                win,
                cx,
            )
            .with_placeholder("feat: ...", cx)
        });
        let pr_body_inp = window.use_keyed_state(field_key("body"), cx, |win, cx| {
            kui::components::input::Input::new(
                "git-pr-body-inp",
                loc.pick("变更摘要", "Summary"),
                win,
                cx,
            )
            .with_placeholder(loc.pick("描述主要改动与验证结果…", "Summary & test plan…"), cx)
        });
        let pr_draft_state = window.use_keyed_state(field_key("draft"), cx, |_, _| false);
        let is_draft = *pr_draft_state.read(cx);

        let ws_refresh = ws.to_path_buf();
        let head_for_create=snap.current_branch.clone().unwrap_or_default();
        let mut col = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.muted)
                            .child(format!(
                                "{} ({})",
                                loc.pick("开启中的 Pull Requests", "Open Pull Requests"),
                                prs.len()
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                crate::ui_motion::soft_button_sm(
                                    "git-pr-refresh",
                                    loc.pick("刷新", "Refresh"),
                                    s,
                                )
                                .on_press(cx, move |wb, _, _| {
                                    wb.bridge.send(KarCommand::RefreshForgeData {
                                        workspace: ws_refresh.clone(),
                                    });
                                }),
                            )
                            .child(
                                crate::ui_motion::primary_button_sm(
                                    "git-pr-toggle-create",
                                    if self.forge_create_pr_open {
                                        loc.pick("取消创建", "Cancel")
                                    } else {
                                        loc.pick("新建 PR", "New PR")
                                    },
                                    s,
                                )
                                .disabled(!can_create)
                                .on_press(cx, |wb, _, cx| {
                                    wb.forge_create_pr_open = !wb.forge_create_pr_open;
                                    cx.notify();
                                }),
                            ),
                    ),
            );

        if self.forge_create_pr_open {
            let t_inp = pr_title_inp.clone();
            let b_inp = pr_body_inp.clone();
            let d_st = pr_draft_state.clone();
            let ws_create = ws.to_path_buf();
            let base_br = snap.default_branch.clone();
            col = col.child(
                crate::ui_primitives::card_box(s)
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(pr_title_inp)
                    .child(pr_body_inp)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                crate::ui_motion::soft_button_sm(
                                    "git-pr-draft-toggle",
                                    if is_draft {
                                        loc.pick("✓ 草稿 PR (Draft)", "✓ Draft PR")
                                    } else {
                                        loc.pick("设为草稿 PR", "Mark as draft")
                                    },
                                    s,
                                )
                                .lit(is_draft)
                                .on_press(cx, move |_, _, cx| {
                                    d_st.update(cx, |v, _| *v = !*v);
                                    cx.notify();
                                }),
                            )
                            .child(
                                crate::ui_motion::primary_button_sm(
                                    "git-pr-submit",
                                    format!("推送并创建 PR → {base_br}"),
                                    s,
                                )
                                .disabled(!can_create)
                                .on_press(cx, move |wb, _, cx| {
                                    let Some(chat)=chat else{return;};
                                    let title = t_inp.read(cx).value(cx).trim().to_owned();
                                    let body = b_inp.read(cx).value(cx).trim().to_owned();
                                    if title.is_empty() {
                                        wb.notify_warning("请填写 PR 标题", cx);
                                        return;
                                    }
                                    let key=serde_json::json!(["create", chat, ws_create, base_br, head_for_create, title, body, is_draft]).to_string();
                                    let operation=wb.forge_operation(&key);
                                    wb.bridge.send(KarCommand::CreatePullRequest {workspace:ws_create.clone(),chat,operation,title,body,base:base_br.clone(),draft:is_draft});
                                    cx.notify();
                                }),
                            ),
                    ),
            );
        }

        if let Some(ref err) = self.forge_error
            && current_workspace
        {
            col = col.child(
                crate::ui_primitives::card_box(s)
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .text_color(s.muted)
                            .child(err.clone()),
                    )
                    .child(
                        div().flex().justify_end().child(
                            crate::ui_motion::soft_button_sm(
                                "git-pr-cfg-token",
                                loc.pick("前往设置配置 GitHub / GitLab 令牌", "Configure Token"),
                                s,
                            )
                            .on_press(cx, |wb, _, cx| {
                                wb.open_settings(crate::ui_settings::SettingsTab::ProjectsGit, cx)
                            }),
                        ),
                    ),
            );
        }

        for (ix, pr) in prs.iter().enumerate() {
            let pr_url = pr.url.clone();
            let pr_num = pr.number;
            let sha=pr.head_sha.clone();
            let pr_title = pr.title.clone();
            let pr_head = pr.head_branch.clone();
            let ws_merge = ws.to_path_buf();
            col = col.child(
                crate::ui_primitives::card_box(s)
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .font_features(tnum_features())
                                    .text_size(px(FS_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(format!("#{} {}", pr.number, pr.title)),
                            )
                            .when(!pr.checks_summary.is_empty(), |d| {
                                d.child(
                                    div()
                                        .px(px(6.))
                                        .rounded(px(R_PILL))
                                        .bg(s.tray)
                                        .text_size(px(FS_TAG))
                                        .text_color(s.muted)
                                        .child(pr.checks_summary.clone()),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .text_color(s.muted)
                            .child(format!("{} → {} · @{}", pr.head_branch, pr.base_branch, pr.author)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(6.))
                            .child(
                                crate::ui_motion::soft_button_sm(
                                    ("pr-ai-review", ix),
                                    loc.pick("交给智能体审查", "AI Review"),
                                    s,
                                )
                                .on_press(cx, move |wb, win, cx| {
                                    let prompt = format!(
                                        "请审查 Pull Request #{pr_num}「{pr_title}」（分支 `{pr_head}`），检查代码风险与测试覆盖。"
                                    );
                                    wb.fill_starter_prompt(&prompt, win, cx);
                                }),
                            )
                            .when(url::Url::parse(&pr_url).is_ok_and(|u| matches!(u.scheme(), "https" | "http")), |row| {
                                row.child(
                                    crate::ui_motion::soft_button_sm(
                                        ("pr-open-web", ix),
                                        loc.pick("网页查看 ↗", "Open ↗"),
                                        s,
                                    )
                                    .on_press(cx, move |_, _, cx| cx.open_url(&pr_url)),
                                )
                            })
                            .child(
                                crate::ui_motion::primary_button_sm(
                                    ("pr-merge", ix),
                                    loc.pick("合并 PR", "Merge"),
                                    s,
                                )
                                .disabled(chat.is_none() || self.forge_pending.is_some() || sha.is_empty() || pr.is_draft)
                                .on_press(cx, move |wb, _, cx| {
                                    let Some(chat)=chat else{return;};
                                    let key=format!("merge/{chat}/{}/{pr_num}/{sha}/{:?}",ws_merge.display(),wb.prefs.pr_default_merge_method);
                                    wb.confirm_or_run(key.clone(),true,format!("再次点击以合并 PR #{pr_num}"),cx,|wb,cx| {
                                        let operation=wb.forge_operation(&key);
                                        wb.bridge.send(KarCommand::MergePullRequest{workspace:ws_merge.clone(),chat,operation,number:pr_num,method:wb.prefs.pr_default_merge_method,expected_head_sha:sha.clone()});
                                        cx.notify();
                                    });
                                }),
                            ),
                    ),
            );
        }

        col.into_any_element()
    }

    fn render_git_issues_section(
        &mut self,
        ws: &Path,
        s: &StudioPalette,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let loc = self.ui_locale;
        let issues = if self.forge_workspace.as_deref() == Some(ws) { self.forge_issues.as_slice() } else { &[] };
        let ws_refresh = ws.to_path_buf();
        let mut col = div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(s.muted)
                            .child(format!(
                                "{} ({})",
                                loc.pick("待处理议题 (GitHub / GitLab / Linear)", "Open Issues"),
                                issues.len()
                            )),
                    )
                    .child(
                        crate::ui_motion::soft_button_sm(
                            "git-issues-refresh",
                            loc.pick("刷新议题", "Refresh"),
                            s,
                        )
                        .on_press(cx, move |wb, _, _| {
                            wb.bridge.send(KarCommand::RefreshForgeData {
                                workspace: ws_refresh.clone(),
                            });
                        }),
                    ),
            );

        if let Some(error)=self.forge_error.as_ref().filter(|_|self.forge_workspace.as_deref()==Some(ws)) {
            col=col.child(div().text_size(px(FS_META)).text_color(s.muted).child(error.clone()));
        }
        if issues.is_empty() {
            col = col.child(
                crate::ui_primitives::card_box(s)
                    .p(px(14.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(FS_META))
                            .text_color(s.muted)
                            .child(loc.pick(
                                "暂无待办议题（可在「设置 → 项目与代码托管」中配置 GitHub / GitLab / Linear 令牌同步团队任务）。",
                                "No open issues found. Configure GitHub / GitLab / Linear tokens in Settings.",
                            )),
                    )
                    .child(
                        div().flex().justify_end().child(
                            crate::ui_motion::soft_button_sm(
                                "git-issue-cfg-btn",
                                loc.pick("配置 Linear / GitHub", "Configure"),
                                s,
                            )
                            .on_press(cx, |wb, _, cx| {
                                wb.open_settings(crate::ui_settings::SettingsTab::ProjectsGit, cx)
                            }),
                        ),
                    ),
            );
        }

        for (ix, issue) in issues.iter().enumerate() {
            let i_url = issue.url.clone();
            let i_num = issue.number_label.clone();
            let i_title = issue.title.clone();
            let i_body = issue.body.clone();
            col = col.child(
                crate::ui_primitives::card_box(s)
                    .p(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .truncate()
                                    .font_features(tnum_features())
                                    .text_size(px(FS_UI))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(s.ink)
                                    .child(format!("{} {}", issue.number_label, issue.title)),
                            )
                            .child(
                                div()
                                    .px(px(6.))
                                    .rounded(px(R_PILL))
                                    .bg(s.tray)
                                    .text_size(px(FS_TAG))
                                    .text_color(s.muted)
                                    .child(issue.source.clone()),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(6.))
                            .when(url::Url::parse(&i_url).is_ok_and(|u| matches!(u.scheme(), "https" | "http")), |row| {
                                row.child(
                                    crate::ui_motion::soft_button_sm(
                                        ("issue-open-web", ix),
                                        loc.pick("网页打开 ↗", "Open ↗"),
                                        s,
                                    )
                                    .on_press(cx, move |_, _, cx| cx.open_url(&i_url)),
                                )
                            })
                            .child(
                                crate::ui_motion::primary_button_sm(
                                    ("issue-to-chat", ix),
                                    loc.pick("转为新对话处理", "Start thread"),
                                    s,
                                )
                                .on_press(cx, move |wb, win, cx| {
                                    wb.start_new_chat(win, cx);
                                    let excerpt: String = i_body.chars().take(600).collect();
                                    let prompt = if excerpt.trim().is_empty() {
                                        format!("请帮我分析并实现议题 {i_num}：{i_title}")
                                    } else {
                                        format!("请帮我分析并实现议题 {i_num}：{i_title}\n\n背景描述：\n{excerpt}")
                                    };
                                    wb.fill_starter_prompt(&prompt, win, cx);
                                }),
                            ),
                    ),
            );
        }

        col.into_any_element()
    }
}

pub(crate) fn render_split_diff_hunks(
    hunks: &[crate::studio_hub::DiffHunkData],
    s: &StudioPalette,
    cx: &App,
) -> Div {
    let mono = mono_font(cx);
    let mut box_col = div()
        .mt(px(6.))
        .w_full()
        .rounded(px(crate::ui_primitives::R_CTRL))
        .bg(s.card)
        .border_1()
        .border_color(s.line_soft)
        .overflow_hidden()
        .font_family(mono)
        .text_size(px(crate::ui_primitives::effective_code_font_size(cx)))
        .line_height(relative(1.5))
        .flex()
        .flex_col();

    for (hix, hunk) in hunks.iter().enumerate() {
        if hix > 0 {
            box_col = box_col.child(div().h(px(1.)).w_full().bg(s.line_soft));
        }
        box_col = box_col.child(
            div()
                .px(px(8.))
                .py(px(3.))
                .bg(s.tray)
                .font_features(tnum_features())
                .text_size(px(FS_TAG))
                .text_color(s.muted)
                .child(format!("@@ +{} @@", hunk.start_line)),
        );
        let max_rows = hunk.before.len().max(hunk.after.len());
        for r in 0..max_rows {
            let left = hunk.before.get(r);
            let right = hunk.after.get(r);
            box_col = box_col.child(
                div()
                    .w_full()
                    .grid()
                    .grid_cols(2)
                    .border_t_1()
                    .border_color(s.line_soft.opacity(0.45))
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(2.))
                            .min_w_0()
                            .truncate()
                            .bg(if left.is_some() {
                                s.pro.red_soft.opacity(0.55)
                            } else {
                                gpui::transparent_black()
                            })
                            .text_color(s.pro.red_ink)
                            .child(left.map_or_else(String::new, |l| format!("− {l}"))),
                    )
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(2.))
                            .min_w_0()
                            .truncate()
                            .border_l_1()
                            .border_color(s.line_soft)
                            .bg(if right.is_some() {
                                s.pro.green_soft.opacity(0.55)
                            } else {
                                gpui::transparent_black()
                            })
                            .text_color(s.pro.green_ink)
                            .child(right.map_or_else(String::new, |l| format!("+ {l}"))),
                    ),
            );
        }
    }
    box_col
}
