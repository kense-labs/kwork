//! Browser-local Session ordering shared by flat and Workspace lists.

use crate::sessions::SessionSummary;
use gpui::{Bounds, Pixels, Point};
use std::collections::{HashMap, HashSet};

pub(crate) const FLAT_SESSION_ORDER_KEY: &str = "__flat_session_order__";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DropHalf {
    Before,
    After,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionDrop {
    pub id: String,
    pub half: DropHalf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionRow {
    pub id: String,
    pub pinned: bool,
    pub blank: bool,
    pub archived: bool,
}

fn by_recency(
    members: &[&SessionSummary],
    recency: &impl Fn(&SessionSummary) -> u64,
) -> Vec<String> {
    let mut members = members.to_vec();
    members.sort_by(|a, b| recency(b).cmp(&recency(a)).then_with(|| a.id.cmp(&b.id)));
    members
        .into_iter()
        .map(|session| session.id.clone())
        .collect()
}

/// Reconcile saved positions without forgetting hidden or temporarily archived members.
pub(crate) fn reconcile_manual_order(
    members: &[SessionSummary],
    saved_order: &[String],
    pinned: &[String],
    archived: &HashSet<String>,
    recency: impl Fn(&SessionSummary) -> u64,
) -> Vec<String> {
    let by_id = members
        .iter()
        .map(|session| (session.id.as_str(), session))
        .collect::<HashMap<_, _>>();
    let mut included = HashSet::new();
    let mut ordered = Vec::new();
    for id in saved_order {
        if by_id.contains_key(id.as_str()) && included.insert(id.clone()) {
            ordered.push(id.clone());
        }
    }
    let pins = pinned
        .iter()
        .filter(|id| {
            by_id.contains_key(id.as_str())
                && !archived.contains(*id)
                && included.insert((*id).clone())
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut missing = members
        .iter()
        .filter(|session| !included.contains(&session.id))
        .collect::<Vec<_>>();
    missing.sort_by(|a, b| recency(b).cmp(&recency(a)).then_with(|| a.id.cmp(&b.id)));
    let mut ordinary = Vec::new();
    let mut archives = Vec::new();
    for session in missing {
        if archived.contains(&session.id) {
            archives.push(session.id.clone());
        } else {
            ordinary.push(session.id.clone());
        }
    }
    ordered.splice(0..0, pins);
    ordered.extend(ordinary);
    ordered.extend(archives);
    ordered
}

fn pin_current_blank(order: Vec<String>, current_blank: Option<&str>) -> Vec<String> {
    let Some(blank) = current_blank else {
        return order;
    };
    std::iter::once(blank.to_owned())
        .chain(order.into_iter().filter(|id| id != blank))
        .collect()
}

pub(crate) fn move_session_to_front(mut order: Vec<String>, id: &str) -> Vec<String> {
    order.retain(|candidate| candidate != id);
    order.insert(0, id.to_owned());
    order
}

/// Project one account in the same order used to initialize a later Manual view.
pub(crate) fn ordered_session_ids(
    members: &[SessionSummary],
    saved_order: Option<&[String]>,
    current_blank: Option<&str>,
    pinned: &[String],
    archived: &HashSet<String>,
    recency: impl Fn(&SessionSummary) -> u64,
) -> Vec<String> {
    let order = match saved_order {
        Some(saved) => reconcile_manual_order(members, saved, pinned, archived, recency),
        None => by_recency(&members.iter().collect::<Vec<_>>(), &recency),
    };
    let current_blank = current_blank.filter(|id| {
        members
            .iter()
            .any(|session| session.id == *id && session.blank)
    });
    pin_current_blank(order, current_blank)
}

pub(crate) fn smallest_row_at<'a, T>(
    rows: impl IntoIterator<Item = (&'a T, &'a Bounds<Pixels>)>,
    position: Point<Pixels>,
) -> Option<(&'a T, &'a Bounds<Pixels>)> {
    rows.into_iter()
        .filter(|(_, bounds)| bounds.contains(&position))
        .min_by(|(_, a), (_, b)| {
            let area = |bounds: &Bounds<Pixels>| {
                f32::from(bounds.size.width) * f32::from(bounds.size.height)
            };
            area(a).total_cmp(&area(b))
        })
}

pub(crate) fn session_drag_order(
    order: &[String],
    rows: &[SessionRow],
    source_id: &str,
    source_pinned: bool,
    over: &SessionDrop,
) -> Option<Vec<String>> {
    let source = rows.iter().find(|row| row.id == source_id)?;
    let target = rows.iter().find(|row| row.id == over.id)?;
    if source.blank
        || source.archived
        || source.pinned != source_pinned
        || target.pinned != source_pinned
        || source.id == target.id
        || !order.contains(&source.id)
    {
        return None;
    }
    let half = if target.blank {
        DropHalf::After
    } else {
        over.half
    };
    let section = rows
        .iter()
        .filter(|row| row.pinned == source_pinned)
        .collect::<Vec<_>>();
    let source_index = section.iter().position(|row| row.id == source.id)?;
    let without_source = section
        .iter()
        .filter(|row| row.id != source.id)
        .collect::<Vec<_>>();
    let insert_at = without_source.iter().position(|row| row.id == target.id)?
        + usize::from(half == DropHalf::After);
    if insert_at == source_index {
        return None;
    }
    let mut next = order
        .iter()
        .filter(|id| id.as_str() != source.id)
        .cloned()
        .collect::<Vec<_>>();
    let target_index = next.iter().position(|id| *id == target.id)?;
    next.insert(
        target_index + usize::from(half == DropHalf::After),
        source.id.clone(),
    );
    let current_blank = rows
        .iter()
        .find(|row| row.blank && next.contains(&row.id))
        .map(|row| row.id.clone());
    Some(pin_current_blank(next, current_blank.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{bounds, point, px, size};

    #[test]
    fn overlapping_rows_hit_the_smallest_row() {
        let rows = [
            ("container", 340., 70.),
            ("source", 340., 32.),
            ("target", 374., 32.),
        ]
        .map(|(id, top, height)| {
            (
                id,
                bounds(point(px(12.), px(top)), size(px(258.), px(height))),
            )
        });
        let rows = rows.iter().map(|(id, bounds)| (id, bounds));
        assert_eq!(
            smallest_row_at(rows.clone(), point(px(145.), px(389.)))
                .map(|(id, bounds)| (*id, bounds.origin.y)),
            Some(("target", px(374.)))
        );
        assert_eq!(
            smallest_row_at(rows, point(px(145.), px(350.)))
                .map(|(id, bounds)| (*id, bounds.origin.y)),
            Some(("source", px(340.)))
        );
    }

    fn session(id: &str, at: u64) -> SessionSummary {
        SessionSummary {
            id: id.to_owned(),
            cwd: "/work".to_owned(),
            title: None,
            blank: id == "blank",
            created_at_ms: Some(at),
        }
    }

    fn row(id: &str, pinned: bool) -> SessionRow {
        SessionRow {
            id: id.to_owned(),
            pinned,
            blank: id == "blank",
            archived: id == "archived",
        }
    }

    fn at(session: &SessionSummary) -> u64 {
        session.created_at_ms.unwrap_or(0)
    }

    fn ids(items: &[&str]) -> Vec<String> {
        items.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn reconcile_keeps_saved_slots_and_adds_missing_members_by_category() {
        let members = vec![
            session("new", 50),
            session("old", 10),
            session("pin", 30),
            session("archived", 40),
            session("saved", 20),
        ];
        let saved = ids(&["saved", "saved", "removed"]);
        let pinned = ids(&["pin"]);
        let archived = HashSet::from(["archived".to_owned()]);
        assert_eq!(
            reconcile_manual_order(&members, &saved, &pinned, &archived, at),
            ids(&["pin", "saved", "new", "old", "archived"])
        );
    }

    #[test]
    fn selected_blank_leads_the_complete_order_without_moving_pinned_slots() {
        let members = vec![
            session("a", 10),
            session("pin", 40),
            session("blank", 30),
            session("b", 20),
        ];
        let pinned = ids(&["pin"]);
        let archived = HashSet::new();
        assert_eq!(
            ordered_session_ids(
                &members,
                Some(&ids(&["b", "a", "pin"])),
                Some("blank"),
                &pinned,
                &archived,
                at,
            ),
            ids(&["blank", "b", "a", "pin"])
        );
        assert_eq!(
            ordered_session_ids(&members, None, Some("blank"), &pinned, &archived, at),
            ids(&["blank", "pin", "b", "a"])
        );
    }

    #[test]
    fn missing_pins_follow_pin_order_instead_of_recency() {
        let members = [
            session("recent", 100),
            session("first-pin", 10),
            session("normal", 50),
        ];
        let pinned = ids(&["first-pin", "recent"]);
        assert_eq!(
            reconcile_manual_order(&members, &[], &pinned, &HashSet::new(), at),
            ids(&["first-pin", "recent", "normal"])
        );
    }

    #[test]
    fn pinning_moves_to_the_front_without_a_duplicate_slot() {
        assert_eq!(
            move_session_to_front(ids(&["pin", "a", "b"]), "b"),
            ids(&["b", "pin", "a"])
        );
    }

    #[test]
    fn pinned_drag_updates_complete_positions_without_regrouping_other_slots() {
        let order = ids(&["first", "pin-a", "archived", "second", "pin-b"]);
        let rows = [
            row("pin-a", true),
            row("pin-b", true),
            row("first", false),
            row("second", false),
        ];
        let target = SessionDrop {
            id: "pin-a".to_owned(),
            half: DropHalf::Before,
        };
        assert_eq!(
            session_drag_order(&order, &rows, "pin-b", true, &target),
            Some(ids(&["first", "pin-b", "pin-a", "archived", "second"]))
        );
    }

    #[test]
    fn visible_drop_preserves_hidden_archive_and_restores_blank_to_first() {
        let order = ids(&["pin", "a", "archived", "b", "c", "blank"]);
        let rows = vec![
            row("blank", false),
            row("pin", true),
            row("a", false),
            row("b", false),
            row("c", false),
        ];
        assert_eq!(
            session_drag_order(
                &order,
                &rows,
                "c",
                false,
                &SessionDrop {
                    id: "a".to_owned(),
                    half: DropHalf::Before,
                },
            ),
            Some(ids(&["blank", "pin", "c", "a", "archived", "b"]))
        );
    }

    #[test]
    fn dragging_does_not_resurrect_a_closed_blank_placeholder() {
        let order = ids(&["a", "b", "c"]);
        let rows = vec![row("blank", false), row("a", false), row("b", false)];
        assert_eq!(
            session_drag_order(
                &order,
                &rows,
                "b",
                false,
                &SessionDrop {
                    id: "a".to_owned(),
                    half: DropHalf::Before,
                },
            ),
            Some(ids(&["b", "a", "c"]))
        );
    }

    #[test]
    fn pinned_rows_cannot_cross_the_partition_or_move_archived_rows() {
        let rows = vec![
            row("pin", true),
            row("ordinary", false),
            row("archived", false),
        ];
        let order = ids(&["pin", "ordinary", "archived"]);
        for (source, pinned, target) in [
            ("pin", true, "ordinary"),
            ("ordinary", true, "pin"),
            ("archived", false, "ordinary"),
        ] {
            assert_eq!(
                session_drag_order(
                    &order,
                    &rows,
                    source,
                    pinned,
                    &SessionDrop {
                        id: target.to_owned(),
                        half: DropHalf::Before,
                    },
                ),
                None
            );
        }
    }

    #[test]
    fn ordinary_rows_can_land_after_the_blank_or_use_a_visible_archive_as_an_anchor() {
        let rows = [
            row("blank", false),
            row("a", false),
            row("archived", false),
            row("b", false),
        ];
        let order = ids(&["blank", "a", "archived", "b"]);
        let drop = |target: &str, half| {
            session_drag_order(
                &order,
                &rows,
                "b",
                false,
                &SessionDrop {
                    id: target.to_owned(),
                    half,
                },
            )
        };
        assert_eq!(
            drop("blank", DropHalf::Before),
            Some(ids(&["blank", "b", "a", "archived"]))
        );
        assert_eq!(
            drop("archived", DropHalf::Before),
            Some(ids(&["blank", "a", "b", "archived"]))
        );
    }

    #[test]
    fn adjacent_or_invalid_drops_do_not_write_a_new_order() {
        let rows = vec![row("a", false), row("b", false), row("c", false)];
        let order = ids(&["a", "b", "c"]);
        for (source, target, half) in [
            ("a", "a", DropHalf::Before),
            ("a", "b", DropHalf::Before),
            ("b", "a", DropHalf::After),
            ("missing", "a", DropHalf::Before),
            ("a", "missing", DropHalf::Before),
        ] {
            assert_eq!(
                session_drag_order(
                    &order,
                    &rows,
                    source,
                    false,
                    &SessionDrop {
                        id: target.to_owned(),
                        half,
                    },
                ),
                None
            );
        }
    }

    #[test]
    fn before_and_after_insert_into_the_complete_account() {
        let rows = vec![row("a", false), row("b", false), row("c", false)];
        let order = ids(&["a", "hidden", "b", "c"]);
        for (half, expected) in [
            (DropHalf::Before, ids(&["c", "a", "hidden", "b"])),
            (DropHalf::After, ids(&["a", "c", "hidden", "b"])),
        ] {
            assert_eq!(
                session_drag_order(
                    &order,
                    &rows,
                    "c",
                    false,
                    &SessionDrop {
                        id: "a".to_owned(),
                        half,
                    },
                ),
                Some(expected)
            );
        }
    }
}
