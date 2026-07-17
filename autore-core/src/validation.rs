//! Validation primitives for Stage 0 data integrity.
//!
//! Provides reusable validation functions for:
//! - ID existence checks
//! - Namespaced ID format validation
//! - Confidence range validation
//! - Directed graph cycle detection

use std::collections::{HashMap, HashSet};

use crate::Error;

/// Validates that an ID exists in the given collection.
///
/// # Arguments
///
/// * `id` - The ID to look up
/// * `collection` - A slice of IDs to search
/// * `context` - Descriptive context for error messages (e.g., "task", "campaign")
///
/// # Errors
///
/// Returns `Error::NotFound` if the ID is not in the collection.
///
/// # Example
///
/// ```
/// use autore_core::validation::validate_id_exists;
///
/// let valid_ids = vec!["id-1", "id-2", "id-3"];
/// assert!(validate_id_exists("id-2", &valid_ids, "task").is_ok());
/// assert!(validate_id_exists("id-999", &valid_ids, "task").is_err());
/// ```
pub fn validate_id_exists<T: AsRef<str> + PartialEq>(
    id: &str,
    collection: &[T],
    context: &str,
) -> crate::Result<()> {
    let exists = collection.iter().any(|item| item.as_ref() == id);

    if exists {
        Ok(())
    } else {
        Err(Error::NotFound(format!("{} '{}' not found", context, id)))
    }
}

/// Validates that a namespaced ID follows the format `namespace.identifier`.
///
/// Rules:
/// - Must contain exactly one dot separator
/// - Namespace must be lowercase ASCII alphanumeric with optional underscores
/// - Identifier must be lowercase ASCII alphanumeric with optional underscores and hyphens
/// - Neither part can be empty
///
/// # Errors
///
/// Returns `Error::Validation` if the format is invalid.
///
/// # Example
///
/// ```
/// use autore_core::validation::validate_namespaced_id;
///
/// assert!(validate_namespaced_id("analysis.binary").is_ok());
/// assert!(validate_namespaced_id("my_namespace.my-id-123").is_ok());
/// assert!(validate_namespaced_id("invalid").is_err()); // no dot
/// assert!(validate_namespaced_id("UPPER.case").is_err()); // uppercase
/// ```
pub fn validate_namespaced_id(id: &str) -> crate::Result<()> {
    let parts: Vec<&str> = id.split('.').collect();

    if parts.len() != 2 {
        return Err(Error::Validation(format!(
            "namespaced ID '{}' must contain exactly one dot separator",
            id
        )));
    }

    let namespace = parts[0];
    let identifier = parts[1];

    if namespace.is_empty() {
        return Err(Error::Validation(format!(
            "namespaced ID '{}' has empty namespace",
            id
        )));
    }

    if identifier.is_empty() {
        return Err(Error::Validation(format!(
            "namespaced ID '{}' has empty identifier",
            id
        )));
    }

    // Validate namespace: lowercase ASCII alphanumeric + underscore
    if !namespace
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(Error::Validation(format!(
            "namespaced ID '{}' namespace must be lowercase ASCII alphanumeric with underscores",
            id
        )));
    }

    // Validate identifier: lowercase ASCII alphanumeric + underscore + hyphen
    if !identifier
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(Error::Validation(format!(
            "namespaced ID '{}' identifier must be lowercase ASCII alphanumeric with underscores and hyphens",
            id
        )));
    }

    Ok(())
}

/// Validates that a confidence value is in the range [0.0, 1.0] and finite.
///
/// # Errors
///
/// Returns `Error::Validation` if the value is out of range, NaN, or infinite.
///
/// # Example
///
/// ```
/// use autore_core::validation::validate_confidence_range;
///
/// assert!(validate_confidence_range(0.5, "task_confidence").is_ok());
/// assert!(validate_confidence_range(0.0, "task_confidence").is_ok());
/// assert!(validate_confidence_range(1.0, "task_confidence").is_ok());
/// assert!(validate_confidence_range(1.5, "task_confidence").is_err());
/// assert!(validate_confidence_range(f64::NAN, "task_confidence").is_err());
/// ```
pub fn validate_confidence_range(value: f64, context: &str) -> crate::Result<()> {
    if value.is_nan() {
        return Err(Error::Validation(format!(
            "{} confidence value is NaN",
            context
        )));
    }

    if value.is_infinite() {
        return Err(Error::Validation(format!(
            "{} confidence value is infinite",
            context
        )));
    }

    if !(0.0..=1.0).contains(&value) {
        return Err(Error::Validation(format!(
            "{} confidence value {} is out of range [0.0, 1.0]",
            context, value
        )));
    }

    Ok(())
}

/// Validates that a directed graph contains no cycles.
///
/// Uses depth-first search with three-color marking (white/gray/black) to detect
/// back edges, which indicate cycles.
///
/// # Arguments
///
/// * `ids` - Slice of node identifiers (can be any type that implements `AsRef<str>`)
/// * `edges` - Slice of `(from_index, to_index)` pairs representing directed edges
///
/// # Errors
///
/// Returns `Error::Validation` if a cycle is detected, including the cycle path.
///
/// # Example
///
/// ```
/// use autore_core::validation::validate_no_cycle;
///
/// let ids = vec!["a", "b", "c"];
/// let edges = vec![(0, 1), (1, 2)]; // a -> b -> c (no cycle)
/// assert!(validate_no_cycle(&ids, &edges).is_ok());
///
/// let cyclic_edges = vec![(0, 1), (1, 2), (2, 0)]; // a -> b -> c -> a (cycle!)
/// assert!(validate_no_cycle(&ids, &cyclic_edges).is_err());
/// ```
pub fn validate_no_cycle<T: AsRef<str>>(
    ids: &[T],
    edges: &[(usize, usize)],
) -> crate::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }

    // Build adjacency list
    let mut adjacency: HashMap<usize, Vec<usize>> = HashMap::new();
    for &(from, to) in edges {
        if from >= ids.len() || to >= ids.len() {
            return Err(Error::Validation(format!(
                "edge ({}, {}) references non-existent node (graph has {} nodes)",
                from,
                to,
                ids.len()
            )));
        }
        adjacency.entry(from).or_default().push(to);
    }

    // Three-color DFS: 0=white (unvisited), 1=gray (in progress), 2=black (done)
    let mut colors = vec![0u8; ids.len()];
    let mut path = Vec::new();

    fn dfs(
        node: usize,
        adjacency: &HashMap<usize, Vec<usize>>,
        colors: &mut [u8],
        path: &mut Vec<usize>,
        _ids: &[impl AsRef<str>],
    ) -> Option<Vec<usize>> {
        colors[node] = 1; // Mark as in-progress (gray)
        path.push(node);

        if let Some(neighbors) = adjacency.get(&node) {
            for &next in neighbors {
                if colors[next] == 1 {
                    // Found a cycle - extract the cycle path
                    let cycle_start = path.iter().position(|&x| x == next).unwrap();
                    let mut cycle = path[cycle_start..].to_vec();
                    cycle.push(next); // Close the cycle
                    return Some(cycle);
                }

                if colors[next] == 0 && let Some(cycle) = dfs(next, adjacency, colors, path, _ids) {
                    return Some(cycle);
                }
            }
        }

        path.pop();
        colors[node] = 2; // Mark as done (black)
        None
    }

    // Check all nodes (handles disconnected components)
    for start in 0..ids.len() {
        if colors[start] == 0 && let Some(cycle) = dfs(start, &adjacency, &mut colors, &mut path, ids) {
            let cycle_str: Vec<&str> = cycle
                .iter()
                .map(|&idx| ids[idx].as_ref())
                .collect();
            return Err(Error::Validation(format!(
                "cycle detected: {}",
                cycle_str.join(" -> ")
            )));
        }
    }

    Ok(())
}

/// Validates that all referenced IDs exist in the provided ID set.
///
/// Checks that every ID in `references` exists in `available_ids`.
///
/// # Errors
///
/// Returns `Error::NotFound` listing all missing IDs.
///
/// # Example
///
/// ```
/// use std::collections::HashSet;
/// use autore_core::validation::validate_all_references_exist;
///
/// let available = HashSet::from(["id-1", "id-2", "id-3"]);
/// let references = vec!["id-1", "id-2"];
/// assert!(validate_all_references_exist(&references, &available, "task dependencies").is_ok());
///
/// let bad_refs = vec!["id-1", "id-999"];
/// assert!(validate_all_references_exist(&bad_refs, &available, "task dependencies").is_err());
/// ```
pub fn validate_all_references_exist<'a>(
    references: &[&'a str],
    available_ids: &HashSet<&'a str>,
    context: &str,
) -> crate::Result<()> {
    let missing: Vec<&&str> = references
        .iter()
        .filter(|id| !available_ids.contains(**id))
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        let missing_str: Vec<&&str> = missing;
        Err(Error::NotFound(format!(
            "{} reference missing IDs: {:?}",
            context, missing_str
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // validate_id_exists tests

    #[test]
    fn validate_id_exists_finds_present_id() {
        let ids = vec!["task-1", "task-2", "task-3"];
        assert!(validate_id_exists("task-2", &ids, "task").is_ok());
    }

    #[test]
    fn validate_id_exists_rejects_missing_id() {
        let ids = vec!["task-1", "task-2", "task-3"];
        let result = validate_id_exists("task-999", &ids, "task");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    #[test]
    fn validate_id_exists_empty_collection() {
        let ids: Vec<&str> = vec![];
        let result = validate_id_exists("any-id", &ids, "task");
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // validate_namespaced_id tests

    #[test]
    fn validate_namespaced_id_accepts_valid_simple() {
        assert!(validate_namespaced_id("analysis.binary").is_ok());
    }

    #[test]
    fn validate_namespaced_id_accepts_valid_with_underscores() {
        assert!(validate_namespaced_id("my_namespace.my_identifier").is_ok());
    }

    #[test]
    fn validate_namespaced_id_accepts_valid_with_digits() {
        assert!(validate_namespaced_id("stage1.task_123").is_ok());
    }

    #[test]
    fn validate_namespaced_id_accepts_valid_with_hyphens() {
        assert!(validate_namespaced_id("analysis.my-task-id").is_ok());
    }

    #[test]
    fn validate_namespaced_id_rejects_no_dot() {
        let result = validate_namespaced_id("invalid");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("dot separator"));
    }

    #[test]
    fn validate_namespaced_id_rejects_multiple_dots() {
        let result = validate_namespaced_id("a.b.c");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exactly one dot"));
    }

    #[test]
    fn validate_namespaced_id_rejects_empty_namespace() {
        let result = validate_namespaced_id(".identifier");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty namespace"));
    }

    #[test]
    fn validate_namespaced_id_rejects_empty_identifier() {
        let result = validate_namespaced_id("namespace.");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty identifier"));
    }

    #[test]
    fn validate_namespaced_id_rejects_uppercase_namespace() {
        let result = validate_namespaced_id("UPPER.identifier");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("lowercase ASCII"));
    }

    #[test]
    fn validate_namespaced_id_rejects_uppercase_identifier() {
        let result = validate_namespaced_id("namespace.UPPER");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("lowercase ASCII"));
    }

    #[test]
    fn validate_namespaced_id_rejects_special_chars_in_namespace() {
        let result = validate_namespaced_id("name-space.identifier");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("underscores"));
    }

    // -----------------------------------------------------------------------
    // validate_confidence_range tests

    #[test]
    fn validate_confidence_accepts_zero() {
        assert!(validate_confidence_range(0.0, "test").is_ok());
    }

    #[test]
    fn validate_confidence_accepts_one() {
        assert!(validate_confidence_range(1.0, "test").is_ok());
    }

    #[test]
    fn validate_confidence_accepts_midpoint() {
        assert!(validate_confidence_range(0.5, "test").is_ok());
    }

    #[test]
    fn validate_confidence_accepts_small_value() {
        assert!(validate_confidence_range(0.001, "test").is_ok());
    }

    #[test]
    fn validate_confidence_accepts_large_value() {
        assert!(validate_confidence_range(0.999, "test").is_ok());
    }

    #[test]
    fn validate_confidence_rejects_negative() {
        let result = validate_confidence_range(-0.1, "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of range"));
    }

    #[test]
    fn validate_confidence_rejects_greater_than_one() {
        let result = validate_confidence_range(1.1, "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of range"));
    }

    #[test]
    fn validate_confidence_rejects_nan() {
        let result = validate_confidence_range(f64::NAN, "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("NaN"));
    }

    #[test]
    fn validate_confidence_rejects_positive_infinity() {
        let result = validate_confidence_range(f64::INFINITY, "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("infinite"));
    }

    #[test]
    fn validate_confidence_rejects_negative_infinity() {
        let result = validate_confidence_range(f64::NEG_INFINITY, "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("infinite"));
    }

    // -----------------------------------------------------------------------
    // validate_no_cycle tests

    #[test]
    fn validate_no_cycle_accepts_empty_graph() {
        let ids: Vec<&str> = vec![];
        let edges: Vec<(usize, usize)> = vec![];
        assert!(validate_no_cycle(&ids, &edges).is_ok());
    }

    #[test]
    fn validate_no_cycle_accepts_single_node() {
        let ids = vec!["a"];
        let edges: Vec<(usize, usize)> = vec![];
        assert!(validate_no_cycle(&ids, &edges).is_ok());
    }

    #[test]
    fn validate_no_cycle_accepts_linear_chain() {
        let ids = vec!["a", "b", "c", "d"];
        let edges = vec![(0, 1), (1, 2), (2, 3)]; // a -> b -> c -> d
        assert!(validate_no_cycle(&ids, &edges).is_ok());
    }

    #[test]
    fn validate_no_cycle_accepts_tree() {
        let ids = vec!["root", "left", "right", "left_child"];
        let edges = vec![(0, 1), (0, 2), (1, 3)]; // root -> {left, right}, left -> left_child
        assert!(validate_no_cycle(&ids, &edges).is_ok());
    }

    #[test]
    fn validate_no_cycle_accepts_dag() {
        let ids = vec!["a", "b", "c", "d"];
        let edges = vec![(0, 1), (0, 2), (1, 3), (2, 3)]; // Diamond DAG
        assert!(validate_no_cycle(&ids, &edges).is_ok());
    }

    #[test]
    fn validate_no_cycle_rejects_self_loop() {
        let ids = vec!["a"];
        let edges = vec![(0, 0)]; // a -> a
        let result = validate_no_cycle(&ids, &edges);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cycle detected"));
    }

    #[test]
    fn validate_no_cycle_rejects_simple_cycle() {
        let ids = vec!["a", "b", "c"];
        let edges = vec![(0, 1), (1, 2), (2, 0)]; // a -> b -> c -> a
        let result = validate_no_cycle(&ids, &edges);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("cycle detected"));
        assert!(err_msg.contains("a -> b -> c -> a"));
    }

    #[test]
    fn validate_no_cycle_rejects_cycle_in_component() {
        let ids = vec!["a", "b", "c", "d", "e"];
        let edges = vec![
            (0, 1), // a -> b (no cycle)
            (2, 3), (3, 4), (4, 2), // c -> d -> e -> c (cycle!)
        ];
        let result = validate_no_cycle(&ids, &edges);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cycle detected"));
    }

    #[test]
    fn validate_no_cycle_rejects_invalid_edge_index() {
        let ids = vec!["a", "b"];
        let edges = vec![(0, 5)]; // Index 5 doesn't exist
        let result = validate_no_cycle(&ids, &edges);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("non-existent node"));
    }

    // -----------------------------------------------------------------------
    // validate_all_references_exist tests

    #[test]
    fn validate_all_references_exist_all_present() {
        let available: HashSet<&str> = ["id-1", "id-2", "id-3"].into_iter().collect();
        let references = vec!["id-1", "id-2"];
        assert!(validate_all_references_exist(&references, &available, "test").is_ok());
    }

    #[test]
    fn validate_all_references_exist_some_missing() {
        let available: HashSet<&str> = ["id-1", "id-2"].into_iter().collect();
        let references = vec!["id-1", "id-999"];
        let result = validate_all_references_exist(&references, &available, "test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("missing"));
    }

    #[test]
    fn validate_all_references_exist_empty_references() {
        let available: HashSet<&str> = ["id-1"].into_iter().collect();
        let references: Vec<&str> = vec![];
        assert!(validate_all_references_exist(&references, &available, "test").is_ok());
    }
}
