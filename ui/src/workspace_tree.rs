use crate::sessions::SessionSummary;
use std::collections::HashMap;

pub(crate) struct WorkspaceGroup {
    pub cwd: String,
    pub sessions: Vec<SessionSummary>,
}

pub(crate) struct WorkspaceTree {
    parents: Vec<Option<usize>>,
    pub children: Vec<Vec<usize>>,
    pub roots: Vec<usize>,
}

impl WorkspaceTree {
    pub fn new(groups: &[WorkspaceGroup]) -> Self {
        let paths = groups
            .iter()
            .map(|group| group.cwd.as_str())
            .collect::<Vec<_>>();
        let parents = paths
            .iter()
            .map(|path| owning_parent_folder(path, &paths))
            .collect::<Vec<_>>();
        let mut children = vec![Vec::new(); groups.len()];
        let mut roots = Vec::new();
        for (index, parent) in parents.iter().enumerate() {
            if let Some(parent) = parent {
                children[*parent].push(index);
            } else {
                roots.push(index);
            }
        }
        Self {
            parents,
            children,
            roots,
        }
    }

    pub fn contains(&self, root: usize, mut node: usize) -> bool {
        loop {
            if node == root {
                return true;
            }
            let Some(parent) = self.parents[node] else {
                return false;
            };
            node = parent;
        }
    }

    pub fn descendants(&self, root: usize, groups: &[WorkspaceGroup]) -> Vec<String> {
        (0..groups.len())
            .filter(|node| self.contains(root, *node))
            .map(|node| groups[node].cwd.clone())
            .collect()
    }
}

fn folder_path(path: &str) -> String {
    let windows = path.as_bytes().get(..3).is_some_and(|prefix| {
        prefix[0].is_ascii_alphabetic() && prefix[1] == b':' && matches!(prefix[2], b'/' | b'\\')
    }) || path.starts_with(r"\\");
    let path = if windows {
        path.replace('\\', "/")
    } else {
        path.to_owned()
    };
    path.trim_end_matches('/').to_owned()
}

pub(crate) fn owning_parent_folder(path: &str, parents: &[&str]) -> Option<usize> {
    let child = folder_path(path);
    let mut owner = None;
    let mut length = None;
    for (index, parent) in parents.iter().enumerate() {
        // The empty cwd is Ungrouped, not a registered filesystem ancestor.
        if parent.is_empty() {
            continue;
        }
        let root = folder_path(parent);
        if length.is_none_or(|length| root.len() > length)
            && child != root
            && child.starts_with(&format!("{root}/"))
        {
            owner = Some(index);
            length = Some(root.len());
        }
    }
    owner
}

pub(crate) fn clear_workspace_watermarks(
    watermarks: &mut HashMap<String, usize>,
    descendants: &[String],
) {
    for cwd in descendants {
        watermarks.remove(cwd);
    }
}

pub(crate) fn reconcile_workspace_order(saved: &[String], known: &[String]) -> Vec<String> {
    let known_ids = known
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    let mut included = std::collections::HashSet::new();
    let mut ordered = Vec::new();
    for id in saved {
        if known_ids.contains(id.as_str()) && included.insert(id.clone()) {
            ordered.push(id.clone());
        }
    }
    // Newly discovered Workspaces follow the saved sequence in discovery order.
    for id in known {
        if included.insert(id.clone()) {
            ordered.push(id.clone());
        }
    }
    ordered
}

fn workspace_parent(path: &str, paths: &[String], nested: bool) -> Option<String> {
    if !nested || path.is_empty() {
        return None;
    }
    let paths = paths.iter().map(String::as_str).collect::<Vec<_>>();
    owning_parent_folder(path, &paths).map(|index| paths[index].to_owned())
}

/// Normalize a descendant hit to the nearest ancestor accepted by the source's parent.
pub(crate) fn workspace_drop_target(
    order: &[String],
    hierarchy: &[String],
    source: &str,
    target: &str,
    nested: bool,
) -> Option<String> {
    if source.is_empty()
        || target.is_empty()
        || !order.iter().any(|id| id == source)
        || !order.iter().any(|id| id == target)
        || !hierarchy.iter().any(|id| id == source)
        || !hierarchy.iter().any(|id| id == target)
    {
        return None;
    }
    let source_parent = workspace_parent(source, hierarchy, nested);
    let mut target = target.to_owned();
    loop {
        let parent = workspace_parent(&target, hierarchy, nested);
        if parent == source_parent {
            return Some(target);
        }
        target = parent?;
    }
}

/// Apply DOM insertBefore semantics while preserving descendants and hidden Workspace slots.
pub(crate) fn workspace_drag_order(
    order: &[String],
    hierarchy: &[String],
    source: &str,
    target: &str,
    after: bool,
    nested: bool,
) -> Option<Vec<String>> {
    let target = workspace_drop_target(order, hierarchy, source, target, nested)?;
    let source_parent = workspace_parent(source, hierarchy, nested);
    let siblings = hierarchy
        .iter()
        .filter(|id| workspace_parent(id, hierarchy, nested) == source_parent)
        .cloned()
        .collect::<Vec<_>>();
    let row_index = siblings.iter().position(|id| *id == target)?;
    let anchor = if after {
        siblings.get(row_index + 1).cloned()
    } else {
        Some(target)
    };
    if anchor.as_deref() == Some(source) {
        return None;
    }
    let source_index = siblings.iter().position(|id| id == source)?;
    let anchor_index = anchor
        .as_ref()
        .and_then(|anchor| siblings.iter().position(|id| id == anchor))
        .unwrap_or(siblings.len());
    if anchor_index == source_index || anchor_index == source_index + 1 {
        return None;
    }
    let mut next = order
        .iter()
        .filter(|id| id.as_str() != source)
        .cloned()
        .collect::<Vec<_>>();
    if let Some(anchor) = anchor {
        let index = next.iter().position(|id| *id == anchor)?;
        next.insert(index, source.to_owned());
    } else {
        next.push(source.to_owned());
    }
    Some(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups(paths: &[&str]) -> Vec<WorkspaceGroup> {
        paths
            .iter()
            .map(|cwd| WorkspaceGroup {
                cwd: (*cwd).to_owned(),
                sessions: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn nearest_ancestors_respect_path_boundaries() {
        let groups = groups(&["/work", "/work/app", "/work/app/deep", "/workshop"]);
        let tree = WorkspaceTree::new(&groups);
        assert_eq!(tree.parents, vec![None, Some(0), Some(1), None]);
        assert_eq!(tree.children[0], vec![1]);
        assert_eq!(
            tree.descendants(0, &groups),
            ["/work", "/work/app", "/work/app/deep"]
        );
        assert!(!tree.contains(0, 3));
    }

    #[test]
    fn missing_intermediate_folders_reattach_to_the_nearest_rendered_parent() {
        let groups = groups(&["/work/app/deep", "/work", "/elsewhere"]);
        let tree = WorkspaceTree::new(&groups);
        assert_eq!(tree.parents, vec![Some(1), None, None]);
        assert_eq!(tree.roots, vec![1, 2]);
    }

    #[test]
    fn windows_separators_normalize_but_case_stays_significant() {
        let groups = groups(&[r"C:\Work", r"C:/Work\App", r"c:\work\lower"]);
        let tree = WorkspaceTree::new(&groups);
        assert_eq!(tree.parents, vec![None, Some(0), None]);
        assert_eq!(folder_path(r"\\server\share\"), "//server/share");
        assert_eq!(folder_path(r"/work\literal"), r"/work\literal");
    }

    #[test]
    fn ungrouped_is_not_an_ancestor_but_a_filesystem_root_is() {
        let paths = ["", "/"];
        assert_eq!(owning_parent_folder("/work", &paths), Some(1));
        assert_eq!(owning_parent_folder("/work", &[""]), None);
        assert_eq!(owning_parent_folder("/work/", &["/work/"]), None);
    }

    #[test]
    fn folding_clears_the_whole_descendant_subtree_only() {
        let groups = groups(&["/work", "/work/app", "/work/app/deep", "/other"]);
        let tree = WorkspaceTree::new(&groups);
        let mut watermarks = HashMap::from([
            ("/work".to_owned(), 6),
            ("/work/app".to_owned(), 7),
            ("/work/app/deep".to_owned(), 8),
            ("/other".to_owned(), 9),
        ]);
        clear_workspace_watermarks(&mut watermarks, &tree.descendants(0, &groups));
        assert_eq!(watermarks, HashMap::from([("/other".to_owned(), 9)]));
    }

    fn strings(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    #[test]
    fn workspace_order_reconciles_stale_duplicates_and_discoveries() {
        assert_eq!(
            reconcile_workspace_order(&strings(&["b", "stale", "b"]), &strings(&["a", "b", "c"])),
            strings(&["b", "a", "c"])
        );
    }

    #[test]
    fn flat_workspace_drag_supports_both_halves_and_skips_no_ops() {
        let order = strings(&["a", "b", "c"]);
        assert_eq!(
            workspace_drag_order(&order, &order, "b", "a", false, false),
            Some(strings(&["b", "a", "c"]))
        );
        assert_eq!(
            workspace_drag_order(&order, &order, "b", "c", true, false),
            Some(strings(&["a", "c", "b"]))
        );
        for (source, target, after) in [("a", "a", false), ("b", "a", true), ("a", "b", false)] {
            assert_eq!(
                workspace_drag_order(&order, &order, source, target, after, false),
                None
            );
        }
    }

    #[test]
    fn descendant_drop_normalizes_to_the_whole_sibling_group() {
        let order = strings(&["outside", "/root", "/root/team", "/root/team/child", "tail"]);
        assert_eq!(
            workspace_drop_target(&order, &order, "outside", "/root/team/child", true),
            Some("/root".to_owned())
        );
        assert_eq!(
            workspace_drag_order(&order, &order, "outside", "/root/team/child", true, true),
            Some(strings(&[
                "/root",
                "/root/team",
                "/root/team/child",
                "outside",
                "tail"
            ]))
        );
    }

    #[test]
    fn moving_a_parent_preserves_descendant_order() {
        let order = strings(&["/root", "/root/child", "other", "outside"]);
        assert_eq!(
            workspace_drag_order(&order, &order, "/root", "outside", true, true),
            Some(strings(&["/root/child", "other", "outside", "/root"]))
        );
    }

    #[test]
    fn incompatible_nested_branches_do_not_reorder() {
        let order = strings(&["/root", "/root/team", "/other", "/other/child"]);
        assert_eq!(
            workspace_drag_order(&order, &order, "/root/team", "/other/child", true, true),
            None
        );
    }

    #[test]
    fn hidden_workspace_slots_survive_a_visible_reorder() {
        let order = strings(&["a", "hidden", "b", "c"]);
        let hierarchy = strings(&["a", "b", "c"]);
        assert_eq!(
            workspace_drag_order(&order, &hierarchy, "c", "a", false, false),
            Some(strings(&["c", "a", "hidden", "b"]))
        );
    }
}
