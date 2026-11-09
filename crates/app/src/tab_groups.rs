//! Tab order and tab groups as plain list operations (no GPUI). The workspace
//! keeps the tabs in one `Vec`, and each tab names its group. The rule that
//! holds everywhere: the tabs of one group are contiguous, so a group is a run
//! of tabs in the tab bar and a labelled section in the sidebar.

/// A group's identity. Unique within one workspace, never reused.
pub type GroupId = u64;

/// A named group of tabs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
}

/// What the list operations need from a tab: which group it is in.
pub trait Grouped {
    fn group(&self) -> Option<GroupId>;
    fn set_group(&mut self, group: Option<GroupId>);
}

/// Moves the item at `from` to `to`, shifting the items between them. Returns
/// `to`. Panics when either index is out of range.
pub fn move_item<T>(items: &mut Vec<T>, from: usize, to: usize) -> usize {
    let item = items.remove(from);
    items.insert(to, item);
    to
}

/// Where the index `active` ends up after `move_item(items, from, to)`. Use it
/// to keep the selected tab selected when tabs move.
pub fn follow(active: usize, from: usize, to: usize) -> usize {
    if active == from {
        to
    } else if from < active && active <= to {
        active - 1
    } else if to <= active && active < from {
        active + 1
    } else {
        active
    }
}

/// Moves the item at `from` to `to` one neighbour at a time. Each step swaps
/// with the neighbour, and the moved item takes that neighbour's group. So a
/// tab that crosses a group's edge joins the group, and one that leaves a
/// group's edge leaves it, and no group is split. Returns `to`.
pub fn move_tab<T: Grouped>(items: &mut [T], from: usize, to: usize) -> usize {
    let mut at = from;
    while at != to {
        let next = if to > at { at + 1 } else { at - 1 };
        let group = items[next].group();
        items.swap(at, next);
        items[next].set_group(group);
        at = next;
    }
    to
}

/// Puts the item at `index` in `group` (`None` takes it out of its group).
/// A tab joins at the end of the group's run. A tab that leaves a group goes
/// right after that group's run. When the group has no other tabs, the item
/// keeps its position. Returns the item's new index.
pub fn set_group<T: Grouped>(items: &mut Vec<T>, index: usize, group: Option<GroupId>) -> usize {
    let old = items[index].group();
    if old == group {
        return index;
    }
    // The group whose run the item joins (new group) or leaves (old group).
    let run_group = group.or(old);
    // The last other item of that group. The moved item is not counted.
    let last = run_group.and_then(|id| {
        (0..items.len()).rfind(|&i| i != index && items[i].group() == Some(id))
    });
    items[index].set_group(group);
    // Positions in the list without the moved item: the run ends just before `to`.
    let to = match last {
        Some(last) => (if last > index { last - 1 } else { last }) + 1,
        None => index,
    };
    move_item(items, index, to)
}

/// Moves the item at `from` into a group and returns its new index. With
/// `onto`, the item takes the place of the item at that index and its group,
/// so `group` is not used: dragged down it lands right after that item,
/// dragged up right before it. So any item can reach the first and the last
/// place. Without `onto`, the item joins `group` as `set_group` does. A drop
/// on the item it already is changes nothing. Keeps the groups contiguous.
/// Panics when `from` is out of range.
pub fn move_to_group<T: Grouped>(
    items: &mut Vec<T>,
    from: usize,
    group: Option<GroupId>,
    onto: Option<usize>,
) -> usize {
    let Some(target) = onto else {
        return set_group(items, from, group);
    };
    if target == from || target >= items.len() {
        return from;
    }
    let group = items[target].group();
    items[from].set_group(group);
    move_item(items, from, target)
}

/// The name the user typed, trimmed. Empty text means no name (`None`), so the
/// automatic title comes back.
pub fn clean_name(text: &str) -> Option<String> {
    let name = text.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Clears the group of every item in `group`. Their order does not change.
pub fn remove_group<T: Grouped>(items: &mut [T], group: GroupId) {
    for item in items.iter_mut().filter(|item| item.group() == Some(group)) {
        item.set_group(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The run of contiguous items in `group`, as a range of indices.
    fn run_of<T: Grouped>(items: &[T], group: GroupId) -> Option<std::ops::Range<usize>> {
        let start = items.iter().position(|item| item.group() == Some(group))?;
        let end = items[start..]
            .iter()
            .position(|item| item.group() != Some(group))
            .map_or(items.len(), |offset| start + offset);
        Some(start..end)
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Item {
        name: &'static str,
        group: Option<GroupId>,
    }

    impl Grouped for Item {
        fn group(&self) -> Option<GroupId> {
            self.group
        }
        fn set_group(&mut self, group: Option<GroupId>) {
            self.group = group;
        }
    }

    fn item(name: &'static str, group: Option<GroupId>) -> Item {
        Item { name, group }
    }

    fn names(items: &[Item]) -> Vec<&'static str> {
        items.iter().map(|i| i.name).collect()
    }

    #[test]
    fn move_item_shifts_the_items_between() {
        let mut items = vec![item("a", None), item("b", None), item("c", None)];
        assert_eq!(move_item(&mut items, 0, 2), 2);
        assert_eq!(names(&items), ["b", "c", "a"]);
    }

    #[test]
    fn follow_tracks_the_selected_item() {
        assert_eq!(follow(0, 0, 2), 2);
        assert_eq!(follow(1, 0, 2), 0);
        assert_eq!(follow(2, 0, 2), 1);
        assert_eq!(follow(2, 2, 0), 0);
        assert_eq!(follow(0, 2, 0), 1);
        assert_eq!(follow(1, 2, 0), 2);
        assert_eq!(follow(3, 0, 2), 3);
    }

    #[test]
    fn move_tab_matches_move_item_for_ungrouped_tabs() {
        let mut items = vec![item("a", None), item("b", None), item("c", None), item("d", None)];
        move_tab(&mut items, 0, 3);
        assert_eq!(names(&items), ["b", "c", "d", "a"]);
        move_tab(&mut items, 3, 1);
        assert_eq!(names(&items), ["b", "a", "c", "d"]);
    }

    #[test]
    fn a_tab_crossing_a_group_edge_joins_the_group() {
        // [a] [g1 g2] -> moving a right takes it into the group's edge.
        let mut items = vec![item("a", None), item("g1", Some(1)), item("g2", Some(1))];
        move_tab(&mut items, 0, 1);
        assert_eq!(names(&items), ["g1", "a", "g2"]);
        assert_eq!(items[1].group, Some(1));
        assert_eq!(run_of(&items, 1), Some(0..3));
    }

    #[test]
    fn a_tab_leaving_a_group_edge_leaves_the_group() {
        let mut items = vec![item("g1", Some(1)), item("g2", Some(1)), item("a", None)];
        move_tab(&mut items, 1, 2);
        assert_eq!(names(&items), ["g1", "a", "g2"]);
        assert_eq!(items[2].group, None);
        assert_eq!(run_of(&items, 1), Some(0..1));
    }

    #[test]
    fn reordering_inside_a_group_keeps_it_together() {
        let mut items = vec![item("g1", Some(1)), item("g2", Some(1)), item("g3", Some(1))];
        move_tab(&mut items, 0, 2);
        assert_eq!(names(&items), ["g2", "g3", "g1"]);
        assert_eq!(run_of(&items, 1), Some(0..3));
    }

    #[test]
    fn run_of_finds_the_contiguous_tabs() {
        let items = vec![item("a", Some(1)), item("b", None), item("c", Some(1)), item("d", Some(2))];
        assert_eq!(run_of(&items, 1), Some(0..1));
        assert_eq!(run_of(&items, 2), Some(3..4));
        assert_eq!(run_of(&items, 9), None);
    }

    #[test]
    fn joining_a_group_goes_to_the_end_of_its_run() {
        let mut items = vec![item("g1", Some(1)), item("g2", Some(1)), item("a", None), item("b", None)];
        let to = set_group(&mut items, 3, Some(1));
        assert_eq!(to, 2);
        assert_eq!(names(&items), ["g1", "g2", "b", "a"]);
        assert_eq!(items[2].group, Some(1));
        assert_eq!(run_of(&items, 1), Some(0..3));
    }

    #[test]
    fn leaving_a_group_goes_after_its_run() {
        let mut items = vec![item("g1", Some(1)), item("g2", Some(1)), item("a", None)];
        let to = set_group(&mut items, 0, None);
        assert_eq!(to, 1);
        assert_eq!(names(&items), ["g2", "g1", "a"]);
        assert_eq!(items[1].group, None);
        assert_eq!(run_of(&items, 1), Some(0..1));
    }

    #[test]
    fn a_new_group_keeps_the_tabs_position() {
        let mut items = vec![item("a", None), item("b", None), item("c", None)];
        assert_eq!(set_group(&mut items, 1, Some(7)), 1);
        assert_eq!(names(&items), ["a", "b", "c"]);
        assert_eq!(items[1].group, Some(7));
    }

    #[test]
    fn setting_the_same_group_changes_nothing() {
        let mut items = vec![item("a", Some(1)), item("b", None)];
        assert_eq!(set_group(&mut items, 0, Some(1)), 0);
        assert_eq!(names(&items), ["a", "b"]);
    }

    #[test]
    fn a_blank_name_means_no_name() {
        assert_eq!(clean_name("  build  "), Some("build".into()));
        assert_eq!(clean_name("   "), None);
        assert_eq!(clean_name(""), None);
    }

        /// Panics unless every group's tabs are one run, with no other tab inside it.
    fn assert_contiguous(items: &[Item]) {
        let mut ids: Vec<GroupId> = items.iter().filter_map(|i| i.group).collect();
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            let count = items.iter().filter(|i| i.group == Some(id)).count();
            let run = run_of(items, id).expect("the group has tabs");
            assert_eq!(run.len(), count, "group {id} is split in {:?}", names(items));
        }
    }

    #[test]
    fn dropping_down_onto_a_tab_of_another_group_joins_that_group_after_it() {
        let mut items = vec![item("x", Some(1)), item("y", Some(1)), item("a", None), item("z", Some(2)), item("w", Some(2))];
        let to = move_to_group(&mut items, 2, None, Some(4));
        assert_eq!(to, 4);
        assert_eq!(names(&items), ["x", "y", "z", "w", "a"]);
        assert_eq!(items[4].group, Some(2));
        assert_contiguous(&items);
    }

    #[test]
    fn dropping_onto_an_ungrouped_tab_leaves_the_group() {
        let mut items = vec![item("g1", Some(1)), item("g2", Some(1)), item("a", None), item("b", None)];
        let to = move_to_group(&mut items, 1, None, Some(3));
        assert_eq!(to, 3);
        assert_eq!(names(&items), ["g1", "a", "b", "g2"]);
        assert_eq!(items[3].group, None);
        assert_contiguous(&items);
    }

    #[test]
    fn dropping_an_ungrouped_tab_onto_a_grouped_tab_joins_the_group() {
        let mut items = vec![item("a", None), item("g1", Some(1)), item("g2", Some(1))];
        let to = move_to_group(&mut items, 0, None, Some(2));
        assert_eq!(to, 2);
        assert_eq!(names(&items), ["g1", "g2", "a"]);
        assert_eq!(items[2].group, Some(1));
        assert_contiguous(&items);
    }

    #[test]
    fn dropping_onto_a_tab_of_its_own_group_keeps_the_group() {
        let mut items = vec![item("g1", Some(1)), item("g2", Some(1)), item("g3", Some(1))];
        let to = move_to_group(&mut items, 0, None, Some(2));
        assert_eq!(to, 2);
        assert_eq!(names(&items), ["g2", "g3", "g1"]);
        assert_eq!(items[2].group, Some(1));
        assert_contiguous(&items);
    }

    #[test]
    fn dropping_a_tab_on_itself_changes_nothing() {
        let mut items = vec![item("a", None), item("b", Some(1))];
        assert_eq!(move_to_group(&mut items, 0, None, Some(0)), 0);
        assert_eq!(names(&items), ["a", "b"]);
        assert_eq!(items[0].group, None);
        assert_contiguous(&items);
    }

    #[test]
    fn dropping_onto_the_next_tab_swaps_with_it() {
        let mut items = vec![item("a", None), item("b", Some(1)), item("c", Some(1))];
        assert_eq!(move_to_group(&mut items, 0, None, Some(1)), 1);
        assert_eq!(names(&items), ["b", "a", "c"]);
        assert_eq!(items[1].group, Some(1));
        assert_contiguous(&items);
    }

    #[test]
    fn the_first_tab_can_reach_the_last_place_and_back() {
        let mut items = vec![item("a", None), item("b", None), item("c", None)];
        assert_eq!(move_to_group(&mut items, 0, None, Some(2)), 2);
        assert_eq!(names(&items), ["b", "c", "a"]);
        assert_eq!(move_to_group(&mut items, 2, None, Some(0)), 0);
        assert_eq!(names(&items), ["a", "b", "c"]);
    }

    #[test]
    fn the_last_tab_of_a_full_group_can_leave_it() {
        let mut items = vec![item("a", Some(1)), item("b", Some(1)), item("c", Some(1))];
        let to = move_to_group(&mut items, 0, None, None);
        assert_eq!(to, 2);
        assert_eq!(names(&items), ["b", "c", "a"]);
        assert_eq!(items[2].group, None);
        assert_contiguous(&items);
    }

    #[test]
    fn dropping_on_a_group_header_joins_the_end_of_its_run() {
        let mut items = vec![item("a", None), item("g1", Some(1)), item("g2", Some(1))];
        let to = move_to_group(&mut items, 0, Some(1), None);
        assert_eq!(to, 2);
        assert_eq!(names(&items), ["g1", "g2", "a"]);
        assert_eq!(items[2].group, Some(1));
        assert_contiguous(&items);
    }

    #[test]
    fn dropping_on_the_header_of_its_own_group_changes_nothing() {
        let mut items = vec![item("g1", Some(1)), item("g2", Some(1)), item("a", None)];
        assert_eq!(move_to_group(&mut items, 0, Some(1), None), 0);
        assert_eq!(names(&items), ["g1", "g2", "a"]);
        assert_contiguous(&items);
    }

    #[test]
    fn dropping_on_the_no_group_header_takes_a_tab_out_of_its_group() {
        let mut items = vec![item("g1", Some(1)), item("g2", Some(1)), item("a", None)];
        let to = move_to_group(&mut items, 1, None, None);
        assert_eq!(to, 1);
        assert_eq!(names(&items), ["g1", "g2", "a"]);
        assert_eq!(items[1].group, None);
        assert_contiguous(&items);
    }

    #[test]
    fn deleting_a_group_keeps_its_tabs_in_order() {
        let mut items = vec![item("a", Some(1)), item("b", Some(1)), item("c", Some(2))];
        remove_group(&mut items, 1);
        assert_eq!(names(&items), ["a", "b", "c"]);
        assert_eq!(items[0].group, None);
        assert_eq!(items[1].group, None);
        assert_eq!(items[2].group, Some(2));
    }
}
