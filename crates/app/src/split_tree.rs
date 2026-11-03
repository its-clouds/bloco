//! The pane layout of one tab: a binary tree where each split divides its
//! rectangle in two. This module is geometry only (no GPUI), so the rules have
//! unit tests. The workspace renders the same numbers with flexbox.

use serde::{Deserialize, Serialize};

/// A rectangle in window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    fn right(&self) -> f32 {
        self.x + self.width
    }

    fn bottom(&self) -> f32 {
        self.y + self.height
    }
}

/// How a split divides its rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    /// The two panes sit side by side. Made by a split to the right.
    Horizontal,
    /// The two panes are stacked. Made by a split below.
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Width of the divider between two panes, in pixels. The visible line is 1px.
/// The grab area around it is wider; see `workspace.rs`.
pub const DIVIDER: f32 = 1.0;
/// The share of a split that the first pane can take. Keeps both panes usable.
pub const MIN_RATIO: f32 = 0.1;
pub const MAX_RATIO: f32 = 0.9;

#[derive(Debug, Clone)]
pub enum Node<T> {
    Leaf(T),
    Split {
        axis: Axis,
        /// The share of the space the first child takes, before the divider.
        ratio: f32,
        first: Box<Node<T>>,
        second: Box<Node<T>>,
    },
}

/// The panes of one tab. `T` is the pane handle.
#[derive(Debug, Clone)]
pub struct SplitTree<T> {
    root: Node<T>,
}

/// The rectangles of a split's two children and the divider between them.
pub fn split_rect(area: Rect, axis: Axis, ratio: f32) -> (Rect, Rect) {
    match axis {
        Axis::Horizontal => {
            let first_width = area.width * ratio;
            let first = Rect {
                width: first_width,
                ..area
            };
            let second = Rect {
                x: area.x + first_width + DIVIDER,
                width: (area.width - first_width - DIVIDER).max(0.0),
                ..area
            };
            (first, second)
        }
        Axis::Vertical => {
            let first_height = area.height * ratio;
            let first = Rect {
                height: first_height,
                ..area
            };
            let second = Rect {
                y: area.y + first_height + DIVIDER,
                height: (area.height - first_height - DIVIDER).max(0.0),
                ..area
            };
            (first, second)
        }
    }
}

impl<T: Clone + PartialEq> SplitTree<T> {
    pub fn new(pane: T) -> Self {
        Self {
            root: Node::Leaf(pane),
        }
    }

    /// Wraps a tree built elsewhere, such as one restored from a saved layout.
    pub fn from_root(root: Node<T>) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Node<T> {
        &self.root
    }

    /// The panes, in the order they were made.
    pub fn leaves(&self) -> Vec<T> {
        let mut out = Vec::new();
        collect_leaves(&self.root, &mut out);
        out
    }

    /// Replaces `target` with a split that holds it and `new`, which comes second.
    /// Returns `false` when `target` is not in the tree.
    pub fn split(&mut self, target: &T, axis: Axis, new: T) -> bool {
        split_node(&mut self.root, target, axis, &mut Some(new))
    }

    /// Removes `target`. Its sibling takes the space. Returns `None` when
    /// `target` was the only pane, so the tab is empty. Unchanged when
    /// `target` is not in the tree.
    pub fn remove(self, target: &T) -> Option<Self> {
        match remove_node(self.root, target) {
            Ok(root) => Some(Self { root }),
            Err(root) if matches!(&root, Node::Leaf(pane) if pane == target) => None,
            Err(root) => Some(Self { root }),
        }
    }

    /// The rectangle of each pane inside `area`.
    pub fn layout(&self, area: Rect) -> Vec<(T, Rect)> {
        let mut out = Vec::new();
        layout_node(&self.root, area, &mut out);
        out
    }

    /// The pane next to `from` in `direction`, if there is one inside `area`.
    /// Picks the closest pane that overlaps `from` on the other axis.
    pub fn neighbor(&self, from: &T, direction: Direction, area: Rect) -> Option<T> {
        let panes = self.layout(area);
        let (_, here) = panes.iter().find(|(pane, _)| pane == from)?;
        panes
            .iter()
            .filter(|(pane, _)| pane != from)
            .filter_map(|(pane, there)| gap(here, there, direction).map(|gap| (gap, pane)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, pane)| pane.clone())
    }

    /// The axis and rectangle of the split at `path`. A path is the child
    /// indices from the root: 0 for the first child, 1 for the second.
    pub fn split_at(&self, path: &[usize], area: Rect) -> Option<(Axis, Rect)> {
        let mut node = &self.root;
        let mut area = area;
        for &step in path {
            let Node::Split {
                axis,
                ratio,
                first,
                second,
            } = node
            else {
                return None;
            };
            let (a, b) = split_rect(area, *axis, *ratio);
            (node, area) = if step == 0 { (first, a) } else { (second, b) };
        }
        match node {
            Node::Split { axis, .. } => Some((*axis, area)),
            Node::Leaf(_) => None,
        }
    }

    /// Sets the ratio of the split at `path`, clamped to [`MIN_RATIO`, `MAX_RATIO`].
    /// Returns `false` when there is no split there.
    pub fn set_ratio(&mut self, path: &[usize], ratio: f32) -> bool {
        set_ratio_node(&mut self.root, path, ratio.clamp(MIN_RATIO, MAX_RATIO))
    }
}

/// How far `there` lies from `here` in `direction`, when it is in that
/// direction and overlaps `here` on the other axis. `None` otherwise.
fn gap(here: &Rect, there: &Rect, direction: Direction) -> Option<f32> {
    let overlap_x = there.x < here.right() && there.right() > here.x;
    let overlap_y = there.y < here.bottom() && there.bottom() > here.y;
    match direction {
        Direction::Left if overlap_y && there.right() <= here.x + 0.5 => Some(here.x - there.right()),
        Direction::Right if overlap_y && there.x >= here.right() - 0.5 => Some(there.x - here.right()),
        Direction::Up if overlap_x && there.bottom() <= here.y + 0.5 => Some(here.y - there.bottom()),
        Direction::Down if overlap_x && there.y >= here.bottom() - 0.5 => Some(there.y - here.bottom()),
        _ => None,
    }
}

fn collect_leaves<T: Clone>(node: &Node<T>, out: &mut Vec<T>) {
    match node {
        Node::Leaf(pane) => out.push(pane.clone()),
        Node::Split { first, second, .. } => {
            collect_leaves(first, out);
            collect_leaves(second, out);
        }
    }
}

fn layout_node<T: Clone>(node: &Node<T>, area: Rect, out: &mut Vec<(T, Rect)>) {
    match node {
        Node::Leaf(pane) => out.push((pane.clone(), area)),
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            let (a, b) = split_rect(area, *axis, *ratio);
            layout_node(first, a, out);
            layout_node(second, b, out);
        }
    }
}

fn split_node<T: Clone + PartialEq>(
    node: &mut Node<T>,
    target: &T,
    axis: Axis,
    new: &mut Option<T>,
) -> bool {
    match node {
        Node::Leaf(pane) if pane == target => {
            let old = Node::Leaf(pane.clone());
            let Some(new) = new.take() else {
                return false;
            };
            *node = Node::Split {
                axis,
                ratio: 0.5,
                first: Box::new(old),
                second: Box::new(Node::Leaf(new)),
            };
            true
        }
        Node::Leaf(_) => false,
        Node::Split { first, second, .. } => {
            split_node(first, target, axis, new) || split_node(second, target, axis, new)
        }
    }
}

/// Removes the leaf `target`. `Ok` holds the new tree. `Err` gives the tree
/// back unchanged, because `target` was not found (or was the root leaf).
fn remove_node<T: PartialEq>(node: Node<T>, target: &T) -> Result<Node<T>, Node<T>> {
    match node {
        Node::Leaf(_) => Err(node),
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => {
            if matches!(&*first, Node::Leaf(pane) if pane == target) {
                return Ok(*second);
            }
            if matches!(&*second, Node::Leaf(pane) if pane == target) {
                return Ok(*first);
            }
            match remove_node(*first, target) {
                Ok(first) => Ok(Node::Split {
                    axis,
                    ratio,
                    first: Box::new(first),
                    second,
                }),
                Err(first) => match remove_node(*second, target) {
                    Ok(second) => Ok(Node::Split {
                        axis,
                        ratio,
                        first: Box::new(first),
                        second: Box::new(second),
                    }),
                    Err(second) => Err(Node::Split {
                        axis,
                        ratio,
                        first: Box::new(first),
                        second: Box::new(second),
                    }),
                },
            }
        }
    }
}

fn set_ratio_node<T>(node: &mut Node<T>, path: &[usize], ratio: f32) -> bool {
    match node {
        Node::Leaf(_) => false,
        Node::Split {
            ratio: current,
            first,
            second,
            ..
        } => match path {
            [] => {
                *current = ratio;
                true
            }
            [0, rest @ ..] => set_ratio_node(first, rest, ratio),
            [_, rest @ ..] => set_ratio_node(second, rest, ratio),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 804.0,
        height: 400.0,
    };

    #[test]
    fn a_single_pane_fills_the_area() {
        let tree = SplitTree::new(1);
        assert_eq!(tree.layout(AREA), vec![(1, AREA)]);
    }

    #[test]
    fn horizontal_split_puts_the_new_pane_on_the_right() {
        let mut tree = SplitTree::new(1);
        assert!(tree.split(&1, Axis::Horizontal, 2));
        let layout = tree.layout(AREA);
        assert_eq!(layout[0].0, 1);
        assert_eq!(layout[1].0, 2);
        assert_eq!(layout[0].1.width, 402.0);
        assert_eq!(layout[1].1.x, 403.0);
        assert_eq!(layout[1].1.width, 401.0);
    }

    #[test]
    fn vertical_split_puts_the_new_pane_below() {
        let mut tree = SplitTree::new(1);
        tree.split(&1, Axis::Vertical, 2);
        let layout = tree.layout(AREA);
        assert_eq!(layout[1].1.y, 201.0);
        assert_eq!(layout[1].1.height, 199.0);
        assert_eq!(layout[1].1.width, AREA.width);
    }

    #[test]
    fn splitting_an_unknown_pane_changes_nothing() {
        let mut tree = SplitTree::new(1);
        assert!(!tree.split(&9, Axis::Horizontal, 2));
        assert_eq!(tree.leaves(), vec![1]);
    }

    #[test]
    fn removing_a_pane_gives_its_space_to_the_sibling() {
        let mut tree = SplitTree::new(1);
        tree.split(&1, Axis::Horizontal, 2);
        tree.split(&2, Axis::Vertical, 3);
        let tree = tree.remove(&2).unwrap();
        assert_eq!(tree.leaves(), vec![1, 3]);
        assert_eq!(tree.layout(AREA)[1].1.height, AREA.height);
    }

    #[test]
    fn removing_the_only_pane_empties_the_tree() {
        assert!(SplitTree::new(1).remove(&1).is_none());
    }

    #[test]
    fn removing_an_unknown_pane_keeps_the_tree() {
        let mut tree = SplitTree::new(1);
        tree.split(&1, Axis::Horizontal, 2);
        let tree = tree.remove(&7).unwrap();
        assert_eq!(tree.leaves(), vec![1, 2]);
    }

    #[test]
    fn neighbors_in_a_two_by_two_grid() {
        // 1 | 2
        // -----
        // 3 | 4
        let mut tree = SplitTree::new(1);
        tree.split(&1, Axis::Horizontal, 2);
        tree.split(&1, Axis::Vertical, 3);
        tree.split(&2, Axis::Vertical, 4);
        assert_eq!(tree.leaves(), vec![1, 3, 2, 4]);

        assert_eq!(tree.neighbor(&1, Direction::Right, AREA), Some(2));
        assert_eq!(tree.neighbor(&1, Direction::Down, AREA), Some(3));
        assert_eq!(tree.neighbor(&4, Direction::Left, AREA), Some(3));
        assert_eq!(tree.neighbor(&4, Direction::Up, AREA), Some(2));
        assert_eq!(tree.neighbor(&1, Direction::Left, AREA), None);
        assert_eq!(tree.neighbor(&2, Direction::Down, AREA), Some(4));
        assert_eq!(tree.neighbor(&2, Direction::Up, AREA), None);
    }

    #[test]
    fn ratio_is_clamped_and_moves_the_divider() {
        let mut tree = SplitTree::new(1);
        tree.split(&1, Axis::Horizontal, 2);
        assert!(tree.set_ratio(&[], 0.25));
        assert_eq!(tree.layout(AREA)[0].1.width, 201.0);
        assert!(tree.set_ratio(&[], 2.0));
        assert_eq!(tree.layout(AREA)[0].1.width, AREA.width * MAX_RATIO);
        assert!(!tree.set_ratio(&[0], 0.5), "a leaf has no ratio");
    }

    #[test]
    fn split_at_reports_the_rect_of_nested_splits() {
        let mut tree = SplitTree::new(1);
        tree.split(&1, Axis::Horizontal, 2);
        tree.split(&2, Axis::Vertical, 3);
        let (axis, rect) = tree.split_at(&[1], AREA).unwrap();
        assert_eq!(axis, Axis::Vertical);
        assert_eq!(rect.x, 403.0);
        assert_eq!(rect.width, 401.0);
        assert_eq!(tree.split_at(&[1, 0], AREA), None);
    }
}
