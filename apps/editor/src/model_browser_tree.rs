//! Metadata-only hierarchy. Geometry is loaded only after selecting a model.
use super::Entry;

#[derive(Default)]
pub(super) struct Tree {
    pub roots: Vec<Node>,
    pub matches: usize,
}

pub(super) struct Node {
    pub label: String,
    pub path: Vec<String>,
    pub entry: Option<Entry>,
    pub children: Vec<Node>,
    /// Number of selectable models in this filtered branch, including its root model.
    pub matches: usize,
}
impl Tree {
    pub fn build(entries: &[Entry], search: &str, category: Option<&str>) -> Self {
        let search = search.trim().to_lowercase();
        let entries = entries
            .iter()
            .filter(|entry| entry.matches(&search, category))
            .collect::<Vec<_>>();
        let mut tree = Self {
            roots: Vec::new(),
            matches: entries.len(),
        };
        // Build all branches before leaves so a model named Pioneer can also be
        // the selectable root of Ships / Pioneer, independent of catalog order.
        for entry in &entries {
            groups(&mut tree.roots, &entry.hierarchy, &[]);
        }
        for entry in entries {
            let children = groups(&mut tree.roots, &entry.hierarchy, &[]);
            if let Some(node) = children
                .iter_mut()
                .find(|node| node.label == entry.name && node.entry.is_none())
            {
                node.entry = Some(entry.clone());
            } else {
                let mut path = entry.hierarchy.clone();
                path.push(entry.name.clone());
                children.push(Node {
                    label: entry.name.clone(),
                    path,
                    entry: Some(entry.clone()),
                    children: Vec::new(),
                    matches: 0,
                });
            }
        }
        count_and_sort(&mut tree.roots);
        tree
    }
}

fn groups<'a>(nodes: &'a mut Vec<Node>, labels: &[String], parent: &[String]) -> &'a mut Vec<Node> {
    let Some((label, rest)) = labels.split_first() else {
        return nodes;
    };
    let index = nodes
        .iter()
        .position(|node| &node.label == label)
        .unwrap_or_else(|| {
            let mut path = parent.to_vec();
            path.push(label.clone());
            nodes.push(Node {
                label: label.clone(),
                path,
                entry: None,
                children: Vec::new(),
                matches: 0,
            });
            nodes.len() - 1
        });
    let node = &mut nodes[index];
    groups(&mut node.children, rest, &node.path)
}

fn count_and_sort(nodes: &mut [Node]) -> usize {
    for node in nodes.iter_mut() {
        node.matches = usize::from(node.entry.is_some()) + count_and_sort(&mut node.children);
    }
    nodes.sort_by(|a, b| {
        let priority = |node: &Node| match node.label.as_str() {
            "Ships" => 0,
            "Pioneer" => 1,
            "Source parts" => 2,
            _ => 3,
        };
        priority(a)
            .cmp(&priority(b))
            .then_with(|| a.children.is_empty().cmp(&b.children.is_empty()))
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            .then_with(|| {
                a.entry
                    .as_ref()
                    .map(|entry| &entry.id)
                    .cmp(&b.entry.as_ref().map(|entry| &entry.id))
            })
    });
    nodes.iter().map(|node| node.matches).sum()
}
