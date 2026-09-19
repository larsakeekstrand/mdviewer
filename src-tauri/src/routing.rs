#![allow(dead_code)]
// Wired up in Task 2.

//! Which window an incoming path belongs to. `route` and `FocusOrder` are pure
//! and unit-tested; `fallback_root` is the only IO.

/// Most-recently-focused project windows, front = most recent.
#[derive(Default)]
pub struct FocusOrder {
    order: Vec<String>,
}

impl FocusOrder {
    pub fn touch(&mut self, label: &str) {
        self.order.retain(|l| l != label);
        self.order.insert(0, label.to_string());
    }

    pub fn remove(&mut self, label: &str) {
        self.order.retain(|l| l != label);
    }

    pub fn front(&self) -> Option<&str> {
        self.order.first().map(String::as_str)
    }

    pub fn as_slice(&self) -> &[String] {
        &self.order
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_moves_label_to_front_without_duplicates() {
        let mut f = FocusOrder::default();
        f.touch("main");
        f.touch("project-1");
        f.touch("main");
        assert_eq!(f.as_slice(), ["main", "project-1"]);
        assert_eq!(f.front(), Some("main"));
    }

    #[test]
    fn remove_drops_label() {
        let mut f = FocusOrder::default();
        f.touch("main");
        f.touch("project-1");
        f.remove("project-1");
        assert_eq!(f.as_slice(), ["main"]);
        f.remove("main");
        assert_eq!(f.front(), None);
    }
}
