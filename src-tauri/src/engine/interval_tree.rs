//! Interval Tree - AVL-balanced tree for efficient interval queries.
//!
//! # Complexity Guarantees
//!
//! | Operation | Complexity |
//! |-----------|------------|
//! | insert | O(log n) |
//! | remove | O(log n) |
//! | query_point | O(log n + k) |
//! | query_range | O(log n + k) |
//!
//! Where k = number of results returned.
//!
//! # Design
//!
//! This is an augmented AVL tree where:
//! - Nodes are sorted by interval start time
//! - Each node stores `max_end`: the maximum end time in its subtree
//! - The `max_end` augmentation enables efficient overlap pruning

use std::cmp::{max, Ordering};
use std::collections::HashSet;

use crate::engine::media_time::MediaTime;
use crate::engine::timeline_state::ClipId;

// =============================================================================
// TYPES
// =============================================================================

/// A time range [start, end) - start inclusive, end exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeRange {
    pub start: MediaTime,
    pub end: MediaTime,
}

impl TimeRange {
    /// Create a new time range.
    pub fn new(start: MediaTime, end: MediaTime) -> Self {
        debug_assert!(end >= start, "TimeRange end must be >= start");
        Self { start, end }
    }

    /// Check if two ranges overlap.
    /// Ranges [a, b) and [c, d) overlap iff a < d AND c < b
    #[inline]
    pub fn overlaps(&self, other: &TimeRange) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// Check if this range contains a point.
    #[inline]
    pub fn contains_point(&self, time: MediaTime) -> bool {
        time >= self.start && time < self.end
    }

    /// Duration of the range.
    pub fn duration(&self) -> MediaTime {
        self.end - self.start
    }
}

/// An entry in the interval tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntervalEntry {
    pub range: TimeRange,
    pub clip_id: ClipId,
}

impl IntervalEntry {
    pub fn new(clip_id: ClipId, start: MediaTime, end: MediaTime) -> Self {
        Self {
            range: TimeRange::new(start, end),
            clip_id,
        }
    }
}

// =============================================================================
// INTERVAL NODE
// =============================================================================

/// A node in the AVL interval tree.
#[derive(Debug, Clone)]
struct IntervalNode {
    /// The interval stored at this node
    entry: IntervalEntry,

    /// Maximum end time in this subtree (augmentation for efficient queries)
    max_end: MediaTime,

    /// AVL height for balancing
    height: i32,

    /// Left child (intervals starting before this one)
    left: Option<Box<IntervalNode>>,

    /// Right child (intervals starting after this one)
    right: Option<Box<IntervalNode>>,
}

impl IntervalNode {
    fn new(entry: IntervalEntry) -> Self {
        let max_end = entry.range.end;
        Self {
            entry,
            max_end,
            height: 1,
            left: None,
            right: None,
        }
    }

    /// Get height of a node (0 for None).
    #[inline]
    fn height_of(node: &Option<Box<IntervalNode>>) -> i32 {
        node.as_ref().map(|n| n.height).unwrap_or(0)
    }

    /// Get max_end of a node (ZERO for None).
    #[inline]
    fn max_end_of(node: &Option<Box<IntervalNode>>) -> MediaTime {
        node.as_ref().map(|n| n.max_end).unwrap_or(MediaTime::ZERO)
    }

    /// Update height and max_end from children.
    fn update(&mut self) {
        self.height = 1 + max(Self::height_of(&self.left), Self::height_of(&self.right));

        self.max_end = self.entry.range.end;
        if let Some(ref left) = self.left {
            self.max_end = max(self.max_end, left.max_end);
        }
        if let Some(ref right) = self.right {
            self.max_end = max(self.max_end, right.max_end);
        }
    }

    /// Balance factor: height(left) - height(right).
    fn balance_factor(&self) -> i32 {
        Self::height_of(&self.left) - Self::height_of(&self.right)
    }
}

// =============================================================================
// INTERVAL TREE
// =============================================================================

/// AVL-balanced interval tree for efficient overlap queries.
#[derive(Debug, Clone, Default)]
pub struct IntervalTree {
    root: Option<Box<IntervalNode>>,
    len: usize,
}

impl IntervalTree {
    /// Create an empty interval tree.
    pub fn new() -> Self {
        Self { root: None, len: 0 }
    }

    /// Number of intervals in the tree.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if tree is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    // =========================================================================
    // INSERT
    // =========================================================================

    /// Insert an interval entry.
    /// Complexity: O(log n)
    pub fn insert(&mut self, entry: IntervalEntry) {
        self.root = Self::insert_recursive(self.root.take(), entry);
        self.len += 1;
    }

    fn insert_recursive(
        node: Option<Box<IntervalNode>>,
        entry: IntervalEntry,
    ) -> Option<Box<IntervalNode>> {
        let mut node = match node {
            None => return Some(Box::new(IntervalNode::new(entry))),
            Some(n) => n,
        };

        // BST insert by start time, then by clip_id for deterministic ordering
        let cmp = entry
            .range
            .start
            .cmp(&node.entry.range.start)
            .then_with(|| entry.clip_id.cmp(&node.entry.clip_id));

        match cmp {
            Ordering::Less | Ordering::Equal => {
                node.left = Self::insert_recursive(node.left.take(), entry);
            }
            Ordering::Greater => {
                node.right = Self::insert_recursive(node.right.take(), entry);
            }
        }

        node.update();
        Some(Self::balance(node))
    }

    // =========================================================================
    // REMOVE
    // =========================================================================

    /// Remove an interval by clip_id and range.
    /// Returns true if found and removed.
    /// Complexity: O(log n)
    pub fn remove(&mut self, clip_id: &ClipId, range: TimeRange) -> bool {
        let (new_root, removed) = Self::remove_recursive(self.root.take(), clip_id, range);
        self.root = new_root;
        if removed {
            self.len -= 1;
        }
        removed
    }

    fn remove_recursive(
        node: Option<Box<IntervalNode>>,
        clip_id: &ClipId,
        range: TimeRange,
    ) -> (Option<Box<IntervalNode>>, bool) {
        let mut node = match node {
            None => return (None, false),
            Some(n) => n,
        };

        // Find by start time, then by clip_id
        let cmp = range
            .start
            .cmp(&node.entry.range.start)
            .then_with(|| clip_id.cmp(&node.entry.clip_id));

        let removed;
        match cmp {
            Ordering::Less => {
                let (new_left, r) = Self::remove_recursive(node.left.take(), clip_id, range);
                node.left = new_left;
                removed = r;
            }
            Ordering::Greater => {
                let (new_right, r) = Self::remove_recursive(node.right.take(), clip_id, range);
                node.right = new_right;
                removed = r;
            }
            Ordering::Equal => {
                // Found the node to remove
                if node.entry.clip_id == *clip_id && node.entry.range == range {
                    // Case 1: Leaf node
                    if node.left.is_none() && node.right.is_none() {
                        return (None, true);
                    }
                    // Case 2: One child
                    if node.left.is_none() {
                        return (node.right, true);
                    }
                    if node.right.is_none() {
                        return (node.left, true);
                    }
                    // Case 3: Two children - replace with in-order successor
                    let (new_right, successor) = Self::remove_min(node.right.take().unwrap());
                    node.entry = successor.entry;
                    node.right = new_right;
                    removed = true;
                } else {
                    // Same start time but different clip_id, check left subtree
                    let (new_left, r) = Self::remove_recursive(node.left.take(), clip_id, range);
                    node.left = new_left;
                    removed = r;
                }
            }
        }

        if !removed {
            return (Some(node), false);
        }

        node.update();
        (Some(Self::balance(node)), true)
    }

    /// Remove and return the minimum node from a subtree.
    fn remove_min(mut node: Box<IntervalNode>) -> (Option<Box<IntervalNode>>, Box<IntervalNode>) {
        if node.left.is_none() {
            // This is the minimum
            return (node.right.take(), node);
        }

        let (new_left, min) = Self::remove_min(node.left.take().unwrap());
        node.left = new_left;
        node.update();
        (Some(Self::balance(node)), min)
    }

    // =========================================================================
    // QUERIES
    // =========================================================================

    /// Query all intervals containing a specific point.
    /// Complexity: O(log n + k) where k = result count
    pub fn query_point(&self, time: MediaTime) -> Vec<ClipId> {
        let mut results = Vec::new();
        Self::query_point_recursive(&self.root, time, &mut results);
        results
    }

    fn query_point_recursive(
        node: &Option<Box<IntervalNode>>,
        time: MediaTime,
        results: &mut Vec<ClipId>,
    ) {
        let node = match node {
            None => return,
            Some(n) => n,
        };

        // Pruning: if max_end <= time, no intervals in this subtree contain time
        if node.max_end <= time {
            return;
        }

        // Check left subtree
        Self::query_point_recursive(&node.left, time, results);

        // Check this node
        if node.entry.range.contains_point(time) {
            results.push(node.entry.clip_id.clone());
        }

        // Check right subtree only if its intervals could contain the point
        // (intervals start at or before the point)
        if node.entry.range.start <= time {
            Self::query_point_recursive(&node.right, time, results);
        }
    }

    /// Query all intervals overlapping a time range.
    /// Complexity: O(log n + k) where k = result count
    pub fn query_range(&self, range: TimeRange) -> Vec<ClipId> {
        let mut results = Vec::new();
        Self::query_range_recursive(&self.root, range, &mut results);
        results
    }

    fn query_range_recursive(
        node: &Option<Box<IntervalNode>>,
        range: TimeRange,
        results: &mut Vec<ClipId>,
    ) {
        let node = match node {
            None => return,
            Some(n) => n,
        };

        // Pruning: if max_end <= range.start, no overlaps in this subtree
        if node.max_end <= range.start {
            return;
        }

        // Check left subtree
        Self::query_range_recursive(&node.left, range, results);

        // Check this node
        if node.entry.range.overlaps(&range) {
            results.push(node.entry.clip_id.clone());
        }

        // Check right subtree only if intervals starting before range.end
        if node.entry.range.start < range.end {
            Self::query_range_recursive(&node.right, range, results);
        }
    }

    /// Check if any interval overlaps a range, optionally excluding one clip.
    /// Complexity: O(log n)
    pub fn has_overlap(&self, range: TimeRange, exclude: Option<&ClipId>) -> bool {
        Self::has_overlap_recursive(&self.root, range, exclude)
    }

    fn has_overlap_recursive(
        node: &Option<Box<IntervalNode>>,
        range: TimeRange,
        exclude: Option<&ClipId>,
    ) -> bool {
        let node = match node {
            None => return false,
            Some(n) => n,
        };

        // Pruning
        if node.max_end <= range.start {
            return false;
        }

        // Check left
        if Self::has_overlap_recursive(&node.left, range, exclude) {
            return true;
        }

        // Check this node (unless excluded)
        if node.entry.range.overlaps(&range) {
            if exclude.map_or(true, |ex| &node.entry.clip_id != ex) {
                return true;
            }
        }

        // Check right
        if node.entry.range.start < range.end {
            if Self::has_overlap_recursive(&node.right, range, exclude) {
                return true;
            }
        }

        false
    }

    // =========================================================================
    // AVL BALANCING
    // =========================================================================

    /// Balance a node using AVL rotations.
    fn balance(mut node: Box<IntervalNode>) -> Box<IntervalNode> {
        let bf = node.balance_factor();

        if bf > 1 {
            // Left-heavy
            let left = node.left.as_ref().unwrap();
            if left.balance_factor() < 0 {
                // Left-Right case
                node.left = Some(Self::rotate_left(node.left.take().unwrap()));
            }
            // Left-Left case
            Self::rotate_right(node)
        } else if bf < -1 {
            // Right-heavy
            let right = node.right.as_ref().unwrap();
            if right.balance_factor() > 0 {
                // Right-Left case
                node.right = Some(Self::rotate_right(node.right.take().unwrap()));
            }
            // Right-Right case
            Self::rotate_left(node)
        } else {
            node
        }
    }

    /// Right rotation.
    /// ```text
    ///       y                x
    ///      / \              / \
    ///     x   c    =>      a   y
    ///    / \                  / \
    ///   a   b                b   c
    /// ```
    fn rotate_right(mut y: Box<IntervalNode>) -> Box<IntervalNode> {
        let mut x = y.left.take().unwrap();
        y.left = x.right.take();
        y.update();
        x.right = Some(y);
        x.update();
        x
    }

    /// Left rotation.
    /// ```text
    ///     x                  y
    ///    / \                / \
    ///   a   y      =>      x   c
    ///      / \            / \
    ///     b   c          a   b
    /// ```
    fn rotate_left(mut x: Box<IntervalNode>) -> Box<IntervalNode> {
        let mut y = x.right.take().unwrap();
        x.right = y.left.take();
        x.update();
        y.left = Some(x);
        y.update();
        y
    }

    // =========================================================================
    // VALIDATION
    // =========================================================================

    /// Validate tree invariants for testing/debugging.
    pub fn validate_tree(&self) -> Result<(), String> {
        if let Some(ref root) = self.root {
            self.validate_node(root)?;
        }
        Ok(())
    }

    fn validate_node(&self, node: &IntervalNode) -> Result<MediaTime, String> {
        // Validate BST property
        if let Some(ref left) = node.left {
            let left_start = left.entry.range.start;
            if left_start > node.entry.range.start {
                return Err(format!(
                    "BST violation: left {} > node {}",
                    left_start.as_nanos(),
                    node.entry.range.start.as_nanos()
                ));
            }
        }

        if let Some(ref right) = node.right {
            let right_start = right.entry.range.start;
            if right_start < node.entry.range.start {
                return Err(format!(
                    "BST violation: right {} < node {}",
                    right_start.as_nanos(),
                    node.entry.range.start.as_nanos()
                ));
            }
        }

        // Validate AVL property
        let bf = node.balance_factor();
        if bf < -1 || bf > 1 {
            return Err(format!("AVL violation: balance factor = {}", bf));
        }

        // Validate height
        let expected_height = 1 + max(
            IntervalNode::height_of(&node.left),
            IntervalNode::height_of(&node.right),
        );
        if node.height != expected_height {
            return Err(format!(
                "Height violation: stored {} != computed {}",
                node.height, expected_height
            ));
        }

        // Validate max_end
        let mut expected_max = node.entry.range.end;
        if let Some(ref left) = node.left {
            let left_max = self.validate_node(left)?;
            expected_max = max(expected_max, left_max);
        }
        if let Some(ref right) = node.right {
            let right_max = self.validate_node(right)?;
            expected_max = max(expected_max, right_max);
        }

        if node.max_end != expected_max {
            return Err(format!(
                "max_end violation: stored {} != computed {}",
                node.max_end.as_nanos(),
                expected_max.as_nanos()
            ));
        }

        Ok(node.max_end)
    }

    // =========================================================================
    // UTILITIES
    // =========================================================================

    /// Get all clip IDs (for debugging).
    pub fn all_clip_ids(&self) -> Vec<ClipId> {
        let mut results = Vec::new();
        Self::collect_all(&self.root, &mut results);
        results
    }

    fn collect_all(node: &Option<Box<IntervalNode>>, results: &mut Vec<ClipId>) {
        if let Some(ref n) = node {
            Self::collect_all(&n.left, results);
            results.push(n.entry.clip_id.clone());
            Self::collect_all(&n.right, results);
        }
    }

    /// Clear the tree.
    pub fn clear(&mut self) {
        self.root = None;
        self.len = 0;
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(millis: u64) -> MediaTime {
        MediaTime::from_nanos(millis as i64 * 1_000_000)
    }

    fn entry(id: &str, start_ms: u64, end_ms: u64) -> IntervalEntry {
        IntervalEntry::new(id.to_string(), ms(start_ms), ms(end_ms))
    }

    // =========================================================================
    // BASIC TESTS
    // =========================================================================

    #[test]
    fn test_empty_tree() {
        let tree = IntervalTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
        assert!(tree.validate_tree().is_ok());
    }

    #[test]
    fn test_insert_single() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c1", 0, 1000));

        assert_eq!(tree.len(), 1);
        assert!(tree.validate_tree().is_ok());
    }

    #[test]
    fn test_insert_multiple_sorted() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c1", 0, 1000));
        tree.insert(entry("c2", 1000, 2000));
        tree.insert(entry("c3", 2000, 3000));

        assert_eq!(tree.len(), 3);
        assert!(tree.validate_tree().is_ok());
    }

    #[test]
    fn test_insert_multiple_reverse() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c3", 2000, 3000));
        tree.insert(entry("c2", 1000, 2000));
        tree.insert(entry("c1", 0, 1000));

        assert_eq!(tree.len(), 3);
        assert!(tree.validate_tree().is_ok());
    }

    #[test]
    fn test_insert_triggers_rotations() {
        let mut tree = IntervalTree::new();

        // Insert in order to trigger rotations
        for i in 0..10 {
            tree.insert(entry(&format!("c{}", i), i * 1000, (i + 1) * 1000));
            assert!(
                tree.validate_tree().is_ok(),
                "Failed after inserting c{}",
                i
            );
        }

        assert_eq!(tree.len(), 10);
    }

    // =========================================================================
    // REMOVE TESTS
    // =========================================================================

    #[test]
    fn test_remove_single() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c1", 0, 1000));

        let removed = tree.remove(&"c1".to_string(), TimeRange::new(ms(0), ms(1000)));

        assert!(removed);
        assert!(tree.is_empty());
        assert!(tree.validate_tree().is_ok());
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c1", 0, 1000));

        let removed = tree.remove(&"c2".to_string(), TimeRange::new(ms(0), ms(1000)));

        assert!(!removed);
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn test_remove_maintains_balance() {
        let mut tree = IntervalTree::new();

        for i in 0..10 {
            tree.insert(entry(&format!("c{}", i), i * 1000, (i + 1) * 1000));
        }

        // Remove every other node
        for i in (0..10).step_by(2) {
            let id = format!("c{}", i);
            let range = TimeRange::new(ms(i * 1000), ms((i + 1) * 1000));
            tree.remove(&id, range);
            assert!(tree.validate_tree().is_ok(), "Failed after removing c{}", i);
        }

        assert_eq!(tree.len(), 5);
    }

    // =========================================================================
    // QUERY TESTS
    // =========================================================================

    #[test]
    fn test_query_point_empty() {
        let tree = IntervalTree::new();
        let results = tree.query_point(ms(500));
        assert!(results.is_empty());
    }

    #[test]
    fn test_query_point_single_hit() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c1", 0, 1000));

        let results = tree.query_point(ms(500));
        assert_eq!(results, vec!["c1".to_string()]);
    }

    #[test]
    fn test_query_point_single_miss() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c1", 0, 1000));

        // At exactly end (exclusive)
        let results = tree.query_point(ms(1000));
        assert!(results.is_empty());

        // After end
        let results = tree.query_point(ms(1500));
        assert!(results.is_empty());
    }

    #[test]
    fn test_query_point_multiple_hits() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c1", 0, 2000));
        tree.insert(entry("c2", 500, 1500));
        tree.insert(entry("c3", 1000, 3000));

        let results = tree.query_point(ms(1000));
        let result_set: HashSet<_> = results.into_iter().collect();

        assert_eq!(result_set.len(), 3);
        assert!(result_set.contains("c1"));
        assert!(result_set.contains("c2"));
        assert!(result_set.contains("c3"));
    }

    #[test]
    fn test_query_range_overlap() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c1", 0, 1000));
        tree.insert(entry("c2", 1000, 2000));
        tree.insert(entry("c3", 2000, 3000));

        // Query overlapping c1 and c2
        let results = tree.query_range(TimeRange::new(ms(500), ms(1500)));
        let result_set: HashSet<_> = results.into_iter().collect();

        assert_eq!(result_set.len(), 2);
        assert!(result_set.contains("c1"));
        assert!(result_set.contains("c2"));
    }

    #[test]
    fn test_query_range_no_overlap() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c1", 0, 1000));
        tree.insert(entry("c2", 2000, 3000));

        // Query the gap
        let results = tree.query_range(TimeRange::new(ms(1000), ms(2000)));
        assert!(results.is_empty());
    }

    #[test]
    fn test_has_overlap() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c1", 0, 1000));
        tree.insert(entry("c2", 2000, 3000));

        // Overlap with c1
        assert!(tree.has_overlap(TimeRange::new(ms(500), ms(1500)), None));

        // No overlap in gap
        assert!(!tree.has_overlap(TimeRange::new(ms(1000), ms(2000)), None));

        // Exclude the overlapping clip
        assert!(!tree.has_overlap(TimeRange::new(ms(500), ms(1500)), Some(&"c1".to_string())));
    }

    // =========================================================================
    // ROTATION TESTS
    // =========================================================================

    #[test]
    fn test_left_left_rotation() {
        let mut tree = IntervalTree::new();
        // Insert in decreasing order to trigger right rotations
        tree.insert(entry("c3", 3000, 4000));
        tree.insert(entry("c2", 2000, 3000));
        tree.insert(entry("c1", 1000, 2000)); // Triggers rotation

        assert!(tree.validate_tree().is_ok());
        assert_eq!(tree.len(), 3);
    }

    #[test]
    fn test_right_right_rotation() {
        let mut tree = IntervalTree::new();
        // Insert in increasing order to trigger left rotations
        tree.insert(entry("c1", 1000, 2000));
        tree.insert(entry("c2", 2000, 3000));
        tree.insert(entry("c3", 3000, 4000)); // Triggers rotation

        assert!(tree.validate_tree().is_ok());
        assert_eq!(tree.len(), 3);
    }

    #[test]
    fn test_left_right_rotation() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c3", 3000, 4000));
        tree.insert(entry("c1", 1000, 2000));
        tree.insert(entry("c2", 2000, 3000)); // Triggers LR rotation

        assert!(tree.validate_tree().is_ok());
        assert_eq!(tree.len(), 3);
    }

    #[test]
    fn test_right_left_rotation() {
        let mut tree = IntervalTree::new();
        tree.insert(entry("c1", 1000, 2000));
        tree.insert(entry("c3", 3000, 4000));
        tree.insert(entry("c2", 2000, 3000)); // Triggers RL rotation

        assert!(tree.validate_tree().is_ok());
        assert_eq!(tree.len(), 3);
    }

    // =========================================================================
    // STRESS TESTS
    // =========================================================================

    #[test]
    fn test_many_inserts() {
        let mut tree = IntervalTree::new();

        for i in 0..100 {
            tree.insert(entry(&format!("c{}", i), i * 100, (i + 1) * 100));
        }

        assert_eq!(tree.len(), 100);
        assert!(tree.validate_tree().is_ok());

        // Verify all clips queryable
        for i in 0..100 {
            let results = tree.query_point(ms(i * 100 + 50));
            assert!(results.contains(&format!("c{}", i)));
        }
    }

    #[test]
    fn test_insert_remove_random_order() {
        let mut tree = IntervalTree::new();

        // Insert in "random" order
        let order = [5, 2, 8, 1, 7, 3, 9, 0, 6, 4];
        for i in order {
            tree.insert(entry(&format!("c{}", i), i * 1000, (i + 1) * 1000));
            assert!(tree.validate_tree().is_ok());
        }

        assert_eq!(tree.len(), 10);

        // Remove in different "random" order
        let remove_order = [3, 7, 1, 9, 5, 0, 8, 2, 6, 4];
        for i in remove_order {
            let id = format!("c{}", i);
            let range = TimeRange::new(ms(i * 1000), ms((i + 1) * 1000));
            assert!(tree.remove(&id, range));
            assert!(tree.validate_tree().is_ok());
        }

        assert!(tree.is_empty());
    }

    // =========================================================================
    // BRUTE FORCE COMPARISON
    // =========================================================================

    #[test]
    fn test_query_matches_brute_force() {
        let entries = vec![
            entry("c1", 0, 1000),
            entry("c2", 500, 1500),
            entry("c3", 1000, 2000),
            entry("c4", 1500, 2500),
            entry("c5", 3000, 4000),
        ];

        let mut tree = IntervalTree::new();
        for e in &entries {
            tree.insert(e.clone());
        }

        // Test many query points
        for t in (0..5000).step_by(100) {
            let time = ms(t);

            // Tree query
            let tree_results: HashSet<_> = tree.query_point(time).into_iter().collect();

            // Brute force
            let brute_results: HashSet<_> = entries
                .iter()
                .filter(|e| e.range.contains_point(time))
                .map(|e| e.clip_id.clone())
                .collect();

            assert_eq!(tree_results, brute_results, "Mismatch at t={}", t);
        }
    }

    #[test]
    fn test_range_query_matches_brute_force() {
        let entries = vec![
            entry("c1", 0, 1000),
            entry("c2", 500, 1500),
            entry("c3", 1000, 2000),
            entry("c4", 1500, 2500),
            entry("c5", 3000, 4000),
        ];

        let mut tree = IntervalTree::new();
        for e in &entries {
            tree.insert(e.clone());
        }

        // Test many query ranges
        for start in (0..4000).step_by(500) {
            for end in ((start + 100)..5000).step_by(500) {
                let range = TimeRange::new(ms(start), ms(end));

                // Tree query
                let tree_results: HashSet<_> = tree.query_range(range).into_iter().collect();

                // Brute force
                let brute_results: HashSet<_> = entries
                    .iter()
                    .filter(|e| e.range.overlaps(&range))
                    .map(|e| e.clip_id.clone())
                    .collect();

                assert_eq!(
                    tree_results, brute_results,
                    "Mismatch at range [{}, {})",
                    start, end
                );
            }
        }
    }
}
