//! What a screen reader sees: a small tree of named elements describing the notch,
//! rebuilt from the app's state. No Windows here; `platform::uia` turns it into
//! UI Automation providers.
//!
//! Keys are stable across rebuilds (a task's row keeps its key while it is on
//! screen), so a screen reader's reference to an element stays valid.

use crate::modules::Action;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The notch itself.
    Pane,
    List,
    ListItem,
    Button,
    Text,
}

/// What activating an element does.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeAction {
    /// Open the panel and move keyboard focus into it (what the hotkey does).
    OpenPanel,
    /// Show this module (by position among the enabled ones).
    SwitchTab(usize),
    Module(Action),
}

pub const ROOT: i32 = 0;

#[derive(Clone, Debug)]
pub struct Node {
    pub key: i32,
    /// Key of the parent; `ROOT` for top-level elements.
    pub parent: i32,
    pub role: Role,
    pub name: String,
    pub help: String,
    /// Logical window coordinates: x, y, width, height.
    pub rect: (f32, f32, f32, f32),
    pub action: Option<NodeAction>,
    /// This element has keyboard focus (keyboard mode, selected row).
    pub focused: bool,
    pub offscreen: bool,
}

impl Node {
    pub fn new(key: i32, parent: i32, role: Role, name: impl Into<String>) -> Node {
        Node { key, parent, role, name: name.into(), help: String::new(), rect: (0.0, 0.0, 0.0, 0.0), action: None, focused: false, offscreen: false }
    }
}

/// `nodes[0]` is the root; the rest are in document order, parents before children.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
}

impl Tree {
    pub fn find(&self, key: i32) -> Option<&Node> {
        self.nodes.iter().find(|n| n.key == key)
    }

    pub fn children(&self, key: i32) -> Vec<&Node> {
        self.nodes.iter().filter(|n| n.parent == key && n.key != key).collect()
    }

    pub fn parent_of(&self, key: i32) -> Option<&Node> {
        let parent = self.find(key)?.parent;
        if key == ROOT { None } else { self.find(parent) }
    }

    /// The previous or next sibling (`delta` -1 or +1).
    pub fn sibling(&self, key: i32, delta: i32) -> Option<&Node> {
        let parent = self.find(key)?.parent;
        let siblings = self.children(parent);
        let at = siblings.iter().position(|n| n.key == key)? as i32 + delta;
        usize::try_from(at).ok().and_then(|i| siblings.get(i).copied())
    }

    pub fn focused(&self) -> Option<&Node> {
        self.nodes.iter().find(|n| n.focused)
    }

    /// The deepest visible element containing the point (window logical coordinates).
    pub fn at_point(&self, x: f32, y: f32) -> Option<&Node> {
        self.nodes
            .iter()
            .rev()
            .find(|n| !n.offscreen && n.key != ROOT && x >= n.rect.0 && x < n.rect.0 + n.rect.2 && y >= n.rect.1 && y < n.rect.1 + n.rect.3)
    }

    /// Same elements, same order, same names: nothing a screen reader would announce changed.
    pub fn same_structure(&self, other: &Tree) -> bool {
        self.nodes.len() == other.nodes.len() && self.nodes.iter().zip(&other.nodes).all(|(a, b)| a.key == b.key && a.parent == b.parent && a.name == b.name)
    }
}

/// A stable key from a string (a task id), leaving the low 4 bits for sub-elements.
pub fn key_for(id: &str, sub: i32) -> i32 {
    // FNV-1a, folded to 24 bits so shifting never overflows an i32.
    let mut h: u32 = 0x811c_9dc5;
    for b in id.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    (((h & 0x00ff_ffff) as i32) << 4 | (sub & 0xf)) + 0x100
}

// ------------------------------------------------------------------ words

/// The few strings only a screen reader hears. English and Indonesian, like the rest.
pub fn word(lang: blink_core::i18n::Lang, key: &str) -> &'static str {
    use blink_core::i18n::Lang;
    let id = lang == Lang::Id;
    match key {
        "notch" => "Blink",
        "next" => if id { "Tenggat berikutnya" } else { "Next deadline" },
        "none" => if id { "Tidak ada tenggat" } else { "No deadlines" },
        "upcoming" => if id { "Tenggat mendatang" } else { "Upcoming deadlines" },
        "overdue" => if id { "Terlewat" } else { "Overdue" },
        "done" => if id { "Selesai" } else { "Done" },
        "show_done" => if id { "Tampilkan yang selesai" } else { "Show done" },
        "mark_done" => if id { "Tandai selesai" } else { "Mark done" },
        "restore" => if id { "Pulihkan" } else { "Restore" },
        "snooze_30m" => if id { "Tunda 30 menit" } else { "Snooze 30 minutes" },
        "snooze_2h" => if id { "Tunda 2 jam" } else { "Snooze 2 hours" },
        "snooze_tomorrow" => if id { "Tunda sampai besok" } else { "Snooze until tomorrow" },
        "refresh" => if id { "Segarkan sekarang" } else { "Refresh now" },
        "settings" => if id { "Pengaturan" } else { "Settings" },
        "row_help" => if id { "Enter membuka halaman, D menandai selesai, S menunda" } else { "Enter opens the page, D marks done, S snoozes" },
        "marked_done" => if id { "Ditandai selesai" } else { "Marked done" },
        "restored" => if id { "Dipulihkan" } else { "Restored" },
        "snoozed" => if id { "Ditunda" } else { "Snoozed" },
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Tree {
        let mut t = Tree::default();
        t.nodes.push(Node::new(ROOT, ROOT, Role::Pane, "Blink"));
        t.nodes.push(Node::new(2, ROOT, Role::List, "Upcoming"));
        t.nodes.push(Node::new(20, 2, Role::ListItem, "A"));
        t.nodes.push(Node::new(21, 2, Role::ListItem, "B"));
        t.nodes.push(Node::new(3, ROOT, Role::Button, "Refresh"));
        t
    }

    #[test]
    fn navigation_follows_the_tree() {
        let t = sample();
        assert_eq!(t.children(ROOT).iter().map(|n| n.key).collect::<Vec<_>>(), [2, 3]);
        assert_eq!(t.children(2).iter().map(|n| n.key).collect::<Vec<_>>(), [20, 21]);
        assert_eq!(t.parent_of(20).map(|n| n.key), Some(2));
        assert_eq!(t.parent_of(ROOT).map(|n| n.key), None);
        assert_eq!(t.sibling(20, 1).map(|n| n.key), Some(21));
        assert_eq!(t.sibling(20, -1).map(|n| n.key), None);
        assert_eq!(t.sibling(2, 1).map(|n| n.key), Some(3));
    }

    #[test]
    fn keys_are_stable_distinct_and_clear_of_the_fixed_ones() {
        assert_eq!(key_for("task-1", 0), key_for("task-1", 0));
        assert_ne!(key_for("task-1", 0), key_for("task-2", 0));
        assert_ne!(key_for("task-1", 0), key_for("task-1", 1));
        assert!(key_for("", 0) > 0xff, "fixed keys 0..255 are reserved");
    }

    #[test]
    fn a_point_finds_the_deepest_visible_element() {
        let mut t = sample();
        t.nodes[1].rect = (0.0, 0.0, 100.0, 100.0);
        t.nodes[2].rect = (0.0, 0.0, 100.0, 40.0);
        assert_eq!(t.at_point(10.0, 10.0).map(|n| n.key), Some(20));
        t.nodes[2].offscreen = true;
        assert_eq!(t.at_point(10.0, 10.0).map(|n| n.key), Some(2));
        assert_eq!(t.at_point(500.0, 10.0).map(|n| n.key), None);
    }
}
