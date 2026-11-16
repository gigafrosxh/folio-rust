//! Links between notes (`folio://note/<id>` and `[[Titel]]`), backlinks, orphans, shortest paths and a
//! deterministic force-directed graph layout.

use crate::model::NoteMetadata;
use crate::text::{title_key, wiki_targets};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub id: String,
    pub title: String,
    pub folder: String,
    pub tags: Vec<String>,
    pub modified: f64,
}

#[derive(Default, Clone)]
pub struct LinkIndex {
    pub nodes: HashMap<String, Node>,
    id_links: HashMap<String, HashSet<String>>,
    wiki: HashMap<String, HashSet<String>>,
    by_title: HashMap<String, HashSet<String>>,
}

impl LinkIndex {
    pub fn update(&mut self, meta: &NoteMetadata, folder: &str) {
        self.remove(&meta.id);
        self.nodes.insert(
            meta.id.clone(),
            Node { id: meta.id.clone(), title: meta.title.clone(), folder: folder.to_string(), tags: meta.tags.clone(), modified: meta.modified },
        );
        self.by_title.entry(title_key(&meta.title)).or_default().insert(meta.id.clone());
        self.id_links.insert(meta.id.clone(), meta.links.iter().filter(|l| **l != meta.id).cloned().collect());
        let targets = meta.wiki_links.clone().unwrap_or_else(|| wiki_targets(&meta.search_text));
        self.wiki.insert(meta.id.clone(), targets.iter().map(|t| title_key(t)).collect());
    }

    pub fn remove(&mut self, id: &str) {
        if let Some(old) = self.nodes.remove(id) {
            let k = title_key(&old.title);
            if let Some(set) = self.by_title.get_mut(&k) {
                set.remove(id);
                if set.is_empty() {
                    self.by_title.remove(&k);
                }
            }
        }
        self.id_links.remove(id);
        self.wiki.remove(id);
    }

    /// The note `[[title]]` points to; with several of that title the most recently changed.
    pub fn resolve(&self, title: &str) -> Option<String> {
        self.resolve_key(&title_key(title))
    }

    fn resolve_key(&self, key: &str) -> Option<String> {
        self.by_title.get(key)?.iter().max_by(|a, b| {
            let ma = self.nodes.get(*a).map(|n| n.modified).unwrap_or(0.0);
            let mb = self.nodes.get(*b).map(|n| n.modified).unwrap_or(0.0);
            ma.partial_cmp(&mb).unwrap().then(b.cmp(a))
        }).cloned()
    }

    pub fn outgoing(&self, id: &str) -> HashSet<String> {
        let mut out: HashSet<String> = self.id_links.get(id).map(|s| s.iter().filter(|x| self.nodes.contains_key(*x)).cloned().collect()).unwrap_or_default();
        for t in self.wiki.get(id).into_iter().flatten() {
            if let Some(r) = self.resolve_key(t) {
                out.insert(r);
            }
        }
        out.remove(id);
        out
    }

    pub fn unresolved(&self, id: &str) -> Vec<String> {
        let mut v: Vec<String> = self.wiki.get(id).into_iter().flatten().filter(|t| !self.by_title.contains_key(*t)).cloned().collect();
        v.sort();
        v
    }

    pub fn backlinks(&self, id: &str) -> HashSet<String> {
        self.nodes.keys().filter(|k| *k != id && self.outgoing(k).contains(id)).cloned().collect()
    }

    pub fn edges(&self) -> Vec<(String, String)> {
        let mut seen = HashSet::new();
        let mut out = vec![];
        let mut keys: Vec<&String> = self.nodes.keys().collect();
        keys.sort();
        for a in keys {
            let mut outs: Vec<String> = self.outgoing(a).into_iter().collect();
            outs.sort();
            for b in outs {
                let pair = if *a < b { (a.clone(), b) } else { (b, a.clone()) };
                if seen.insert(pair.clone()) {
                    out.push(pair);
                }
            }
        }
        out
    }

    fn adjacency(&self) -> HashMap<String, HashSet<String>> {
        let mut adj: HashMap<String, HashSet<String>> = HashMap::new();
        for (a, b) in self.edges() {
            adj.entry(a.clone()).or_default().insert(b.clone());
            adj.entry(b).or_default().insert(a);
        }
        adj
    }

    pub fn neighborhood(&self, id: &str, depth: usize) -> HashSet<String> {
        if !self.nodes.contains_key(id) {
            return HashSet::new();
        }
        let adj = self.adjacency();
        let mut seen: HashSet<String> = [id.to_string()].into();
        let mut frontier = seen.clone();
        for _ in 0..depth {
            let next: HashSet<String> = frontier.iter().flat_map(|n| adj.get(n).cloned().unwrap_or_default()).filter(|n| !seen.contains(n)).collect();
            if next.is_empty() {
                break;
            }
            seen.extend(next.iter().cloned());
            frontier = next;
        }
        seen
    }

    pub fn orphans(&self) -> HashSet<String> {
        let linked: HashSet<String> = self.edges().into_iter().flat_map(|(a, b)| [a, b]).collect();
        self.nodes.keys().filter(|k| !linked.contains(*k)).cloned().collect()
    }

    pub fn path(&self, a: &str, b: &str) -> Option<Vec<String>> {
        if !self.nodes.contains_key(a) || !self.nodes.contains_key(b) {
            return None;
        }
        if a == b {
            return Some(vec![a.to_string()]);
        }
        let adj = self.adjacency();
        let mut prev: HashMap<String, String> = HashMap::new();
        let mut queue = VecDeque::from([a.to_string()]);
        let mut seen: HashSet<String> = [a.to_string()].into();
        while let Some(n) = queue.pop_front() {
            let mut next: Vec<&String> = adj.get(&n).map(|s| s.iter().collect()).unwrap_or_default();
            next.sort();
            for m in next {
                if seen.insert(m.clone()) {
                    prev.insert(m.clone(), n.clone());
                    if m == b {
                        let mut path = vec![b.to_string()];
                        while let Some(p) = prev.get(&path[0]) {
                            path.insert(0, p.clone());
                        }
                        return Some(path);
                    }
                    queue.push_back(m.clone());
                }
            }
        }
        None
    }
}

/// Stable id for a folder node (#132).
pub fn folder_node_id(path: &str) -> String {
    let (mut h1, mut h2): (u64, u64) = (0xcbf29ce484222325, 0x84222325cbf29ce4);
    for b in format!("folder:{path}").bytes() {
        h1 = (h1 ^ b as u64).wrapping_mul(0x100000001b3);
        h2 = h2.wrapping_add(b as u64).wrapping_mul(0x9E3779B97F4A7C15);
    }
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&h1.to_be_bytes());
    bytes[8..].copy_from_slice(&h2.to_be_bytes());
    uuid::Uuid::from_bytes(bytes).to_string().to_uppercase()
}

/// Folder nodes and edges note→folder, folder→parent.
pub fn folder_graph(notes: &[(String, String)]) -> (HashMap<String, String>, Vec<(String, String)>) {
    let mut folders = HashMap::new();
    let mut edges = vec![];
    let mut linked = HashSet::new();
    for (id, folder) in notes {
        if folder.is_empty() {
            continue;
        }
        edges.push((id.clone(), folder_node_id(folder)));
        let mut path = folder.clone();
        while !path.is_empty() {
            folders.insert(folder_node_id(&path), path.clone());
            if !linked.insert(path.clone()) {
                break;
            }
            let Some(slash) = path.rfind('/') else { break };
            let parent = path[..slash].to_string();
            edges.push((folder_node_id(&path), folder_node_id(&parent)));
            path = parent;
        }
    }
    (folders, edges)
}

/// Force-directed layout: linked nodes attract, all repel. Deterministic for the same input.
pub fn layout(nodes: &[String], edges: &[(String, String)], previous: &HashMap<String, (f64, f64)>, iterations: usize) -> HashMap<String, (f64, f64)> {
    let n = nodes.len();
    if n == 0 {
        return HashMap::new();
    }
    let index: HashMap<&String, usize> = nodes.iter().enumerate().map(|(i, id)| (id, i)).collect();
    let mut pos: Vec<(f64, f64)> = nodes
        .iter()
        .enumerate()
        .map(|(i, id)| {
            previous.get(id).copied().unwrap_or_else(|| {
                let a = i as f64 * 2.399_963; // golden angle spiral
                let r = 30.0 * (i as f64 + 1.0).sqrt();
                (r * a.cos(), r * a.sin())
            })
        })
        .collect();
    let links: Vec<(usize, usize)> = edges.iter().filter_map(|(a, b)| Some((*index.get(a)?, *index.get(b)?))).filter(|(a, b)| a != b).collect();
    let mut degree = vec![0usize; n];
    for &(a, b) in &links {
        degree[a] += 1;
        degree[b] += 1;
    }
    let rounds = if n > 1500 { iterations.min(80) } else if n > 500 { iterations.min(150) } else { iterations };
    let ideal = 70.0;
    let mut temp = 40.0;
    for round in 0..rounds {
        let mut disp = vec![(0.0f64, 0.0f64); n];
        // Repulsion: grid-bucketed for large graphs.
        let cell = ideal * 3.0;
        let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, p) in pos.iter().enumerate() {
            grid.entry(((p.0 / cell).floor() as i64, (p.1 / cell).floor() as i64)).or_default().push(i);
        }
        for (i, p) in pos.iter().enumerate() {
            let (cx, cy) = ((p.0 / cell).floor() as i64, (p.1 / cell).floor() as i64);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for &j in grid.get(&(cx + dx, cy + dy)).map(|v| v.as_slice()).unwrap_or(&[]) {
                        if i == j {
                            continue;
                        }
                        let (mut ddx, mut ddy) = (p.0 - pos[j].0, p.1 - pos[j].1);
                        let mut d2 = ddx * ddx + ddy * ddy;
                        if d2 < 0.01 {
                            ddx = 0.1 * (i as f64 - j as f64);
                            ddy = 0.1;
                            d2 = ddx * ddx + ddy * ddy;
                        }
                        let f = ideal * ideal / d2;
                        disp[i].0 += ddx * f;
                        disp[i].1 += ddy * f;
                    }
                }
            }
        }
        for &(a, b) in &links {
            let (dx, dy) = (pos[a].0 - pos[b].0, pos[a].1 - pos[b].1);
            let d = (dx * dx + dy * dy).sqrt().max(0.01);
            let f = d / ideal;
            let (fx, fy) = (dx / d * f * d * 0.5, dy / d * f * d * 0.5);
            disp[a].0 -= fx / (degree[a] as f64).max(1.0).sqrt();
            disp[a].1 -= fy / (degree[a] as f64).max(1.0).sqrt();
            disp[b].0 += fx / (degree[b] as f64).max(1.0).sqrt();
            disp[b].1 += fy / (degree[b] as f64).max(1.0).sqrt();
        }
        for i in 0..n {
            // Gravity keeps unconnected parts together.
            disp[i].0 -= pos[i].0 * 0.02;
            disp[i].1 -= pos[i].1 * 0.02;
            let d = (disp[i].0 * disp[i].0 + disp[i].1 * disp[i].1).sqrt();
            if d > 0.0 {
                let lim = d.min(temp);
                pos[i].0 += disp[i].0 / d * lim;
                pos[i].1 += disp[i].1 / d * lim;
            }
        }
        temp = (40.0 * (1.0 - round as f64 / rounds as f64)).max(1.0);
    }
    nodes.iter().cloned().zip(pos).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(id: &str, title: &str, text: &str) -> NoteMetadata {
        NoteMetadata { id: id.into(), title: title.into(), search_text: text.into(), ..Default::default() }
    }

    #[test]
    fn index() {
        let mut ix = LinkIndex::default();
        ix.update(&meta("A", "Alpha", "siehe [[Beta]] und [[Gamma]]"), "");
        ix.update(&meta("B", "Beta", ""), "");
        ix.update(&meta("C", "Delta", ""), "");
        assert_eq!(ix.outgoing("A"), ["B".to_string()].into());
        assert_eq!(ix.backlinks("B"), ["A".to_string()].into());
        assert_eq!(ix.unresolved("A"), vec!["gamma"]);
        assert_eq!(ix.orphans(), ["C".to_string()].into());
        assert_eq!(ix.path("B", "A"), Some(vec!["B".into(), "A".into()]));
        let l = layout(&["A".into(), "B".into(), "C".into()], &ix.edges(), &HashMap::new(), 100);
        assert_eq!(l.len(), 3);
    }
}
