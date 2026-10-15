//! The recently-used order of the projects and, per project, of its branches
//! (the worktrees and local branches of the BRANCHES tree).
//!
//! Each list holds keys, most recently used first. Using an item moves its key
//! to the front ([`touched`]); dragging it elsewhere stores the order the user
//! made ([`reordered`]). An item whose key is not in the list (new, never used)
//! comes after the listed ones in the order it already had ([`sorted_by_order`]).
//! The lists are kept by the core (`runtime/core/order.rs`), so every client
//! shows the same order.

/// The most keys a list keeps; the oldest ones are dropped (their items fall
/// back to the end, in their natural order).
pub const MAX_ORDER_LEN: usize = 500;

/// `list` with `key` moved to the front (added when it was not in the list).
#[must_use]
pub fn touched(list: &[String], key: &str) -> Vec<String> {
    let mut out = Vec::with_capacity(list.len() + 1);
    out.push(key.to_string());
    out.extend(list.iter().filter(|k| *k != key).cloned());
    clamped(out)
}

/// `keys` as the first entries, in that order (empty and repeated keys are
/// dropped), then the entries of `list` they do not name, in their order: a
/// client that shows only some of the items still keeps the order of the rest.
#[must_use]
pub fn reordered(list: &[String], keys: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(keys.len() + list.len());
    for key in keys.iter().filter(|k| !k.is_empty()) {
        if !out.contains(key) {
            out.push(key.clone());
        }
    }
    let named = out.len();
    for key in list {
        if !out[..named].contains(key) {
            out.push(key.clone());
        }
    }
    clamped(out)
}

fn clamped(mut list: Vec<String>) -> Vec<String> {
    list.truncate(MAX_ORDER_LEN);
    list
}

/// `items` ordered by where their key is in `order`; the ones it does not name
/// follow in the order they were given (the sort is stable).
#[must_use]
pub fn sorted_by_order<T>(mut items: Vec<T>, order: &[String], key: impl Fn(&T) -> &str) -> Vec<T> {
    items.sort_by_key(|item| {
        let k = key(item);
        order.iter().position(|o| o == k).unwrap_or(usize::MAX)
    });
    items
}

/// A list as stored (JSON text).
#[must_use]
pub fn encode(list: &[String]) -> String {
    serde_json::to_string(list).unwrap_or_else(|_| "[]".to_string())
}

/// A list from its stored text; text that is not a list of strings reads as
/// empty (the natural order), never as an error: an order is only a preference.
#[must_use]
pub fn decode(text: &str) -> Vec<String> {
    serde_json::from_str(text).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(keys: &[&str]) -> Vec<String> {
        keys.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn using_an_item_moves_it_to_the_front() {
        assert_eq!(
            touched(&list(&["a", "b", "c"]), "c"),
            list(&["c", "a", "b"])
        );
        assert_eq!(touched(&list(&["a", "b"]), "a"), list(&["a", "b"]));
        assert_eq!(touched(&list(&["a"]), "z"), list(&["z", "a"]));
        assert_eq!(touched(&[], "z"), list(&["z"]));
    }

    #[test]
    fn a_list_keeps_at_most_max_keys() {
        let long: Vec<String> = (0..MAX_ORDER_LEN).map(|i| format!("k{i}")).collect();
        let out = touched(&long, "new");
        assert_eq!(out.len(), MAX_ORDER_LEN);
        assert_eq!(out[0], "new");
        assert!(!out.contains(&format!("k{}", MAX_ORDER_LEN - 1)));
    }

    #[test]
    fn a_manual_order_comes_first_and_the_rest_keeps_its_place() {
        let stored = list(&["a", "b", "c", "d"]);
        assert_eq!(
            reordered(&stored, &list(&["c", "a"])),
            list(&["c", "a", "b", "d"])
        );
        // Whole lists replace the order; repeated and empty keys are ignored.
        assert_eq!(
            reordered(&stored, &list(&["d", "", "c", "d", "b", "a"])),
            list(&["d", "c", "b", "a"])
        );
        // Keys the list did not have are added.
        assert_eq!(
            reordered(&stored, &list(&["x"])),
            list(&["x", "a", "b", "c", "d"])
        );
        assert_eq!(reordered(&stored, &[]), stored);
    }

    #[test]
    fn unlisted_items_follow_in_their_own_order() {
        let items = vec!["n1", "b", "n2", "a", "n3"];
        let order = list(&["a", "gone", "b"]);
        let sorted = sorted_by_order(items, &order, |s| s);
        assert_eq!(sorted, ["a", "b", "n1", "n2", "n3"]);
    }

    #[test]
    fn a_stored_list_round_trips_and_bad_text_reads_as_empty() {
        let l = list(&["a", "b/c"]);
        assert_eq!(decode(&encode(&l)), l);
        assert!(decode("not json").is_empty());
        assert!(decode(r#"{"a":1}"#).is_empty());
        assert!(decode("[1,2]").is_empty());
    }
}
