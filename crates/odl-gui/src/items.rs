//! In-memory download list with playlist grouping.

use odl_core::{DownloadItem, ItemId, ItemKind, ItemStatus};
use std::collections::BTreeMap;

#[derive(Debug, Default, Clone)]
pub struct GroupSummary {
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub running: usize,
    pub queued: usize,
    pub paused: usize,
    pub bytes: u64,
    pub duration: f64,
    pub fraction: f32,
    pub speed: f64,
    pub thumbnail: Option<String>,
}

impl GroupSummary {
    pub fn status(&self) -> ItemStatus {
        if self.running > 0 {
            ItemStatus::Running
        } else if self.queued > 0 {
            ItemStatus::Queued
        } else if self.paused > 0 {
            ItemStatus::Paused
        } else if self.failed > 0 {
            ItemStatus::Failed
        } else {
            ItemStatus::Completed
        }
    }
}

#[derive(Default)]
pub struct Items {
    map: BTreeMap<ItemId, DownloadItem>,
    next_id: ItemId,
}

impl Items {
    pub fn new(list: Vec<DownloadItem>) -> Self {
        let next_id = list.iter().map(|i| i.id).max().unwrap_or(0) + 1;
        let map = list.into_iter().map(|i| (i.id, i)).collect();
        Self { map, next_id }
    }

    pub fn alloc_id(&mut self) -> ItemId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn insert(&mut self, item: DownloadItem) {
        self.map.insert(item.id, item);
    }

    pub fn get(&self, id: ItemId) -> Option<&DownloadItem> {
        self.map.get(&id)
    }

    pub fn get_mut(&mut self, id: ItemId) -> Option<&mut DownloadItem> {
        self.map.get_mut(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &DownloadItem> {
        self.map.values()
    }

    /// Removes an item (and its children) and returns everything that was removed.
    pub fn remove(&mut self, id: ItemId) -> Vec<DownloadItem> {
        let mut removed = Vec::new();
        for child in self.children_ids(id) {
            if let Some(c) = self.map.remove(&child) {
                removed.push(c);
            }
        }
        if let Some(item) = self.map.remove(&id) {
            removed.push(item);
        }
        removed
    }

    pub fn children_ids(&self, id: ItemId) -> Vec<ItemId> {
        let mut kids: Vec<&DownloadItem> = self.map.values().filter(|i| i.parent == Some(id)).collect();
        kids.sort_by_key(|i| (i.index.unwrap_or(u32::MAX), i.id));
        kids.into_iter().map(|i| i.id).collect()
    }

    /// Top-level items, newest first.
    pub fn top_level(&self) -> impl Iterator<Item = &DownloadItem> {
        self.map.values().rev().filter(|i| i.parent.is_none())
    }

    pub fn summary(&self, id: ItemId) -> GroupSummary {
        let mut s = GroupSummary::default();
        let mut progress_sum = 0.0f32;
        for kid in self.map.values().filter(|i| i.parent == Some(id)) {
            s.total += 1;
            s.duration += kid.duration.unwrap_or(0.0);
            if s.thumbnail.is_none() {
                s.thumbnail = kid.thumbnail.clone();
            }
            match kid.status {
                ItemStatus::Completed => {
                    s.completed += 1;
                    s.bytes += kid.file_size.unwrap_or(0);
                    progress_sum += 1.0;
                }
                ItemStatus::Running => {
                    s.running += 1;
                    s.speed += kid.progress.speed.unwrap_or(0.0);
                    s.bytes += kid.progress.downloaded;
                    progress_sum += kid.progress.fraction.unwrap_or(0.0);
                }
                ItemStatus::Queued => s.queued += 1,
                ItemStatus::Paused => s.paused += 1,
                ItemStatus::Failed => s.failed += 1,
            }
        }
        s.fraction = if s.total > 0 { progress_sum / s.total as f32 } else { 0.0 };
        s
    }

    /// Recomputes a group's status from its children. Returns `(old, new)` status.
    pub fn refresh_group(&mut self, id: ItemId) -> Option<(ItemStatus, ItemStatus)> {
        let summary = self.summary(id);
        let group = self.map.get_mut(&id)?;
        if group.kind != ItemKind::Group {
            return None;
        }
        let old = group.status;
        let new = if summary.total == 0 { old } else { summary.status() };
        group.status = new;
        group.file_size = Some(summary.bytes);
        if new == ItemStatus::Completed && old != ItemStatus::Completed {
            group.finished_at = Some(chrono::Utc::now());
        }
        Some((old, new))
    }

    pub fn to_vec(&self) -> Vec<DownloadItem> {
        self.map.values().cloned().collect()
    }

    pub fn any_active(&self) -> bool {
        self.map.values().any(|i| i.kind != ItemKind::Group && i.status.is_active())
    }

    pub fn running_count(&self) -> (usize, usize, f64) {
        let mut running = 0;
        let mut queued = 0;
        let mut speed = 0.0;
        for i in self.map.values().filter(|i| i.kind != ItemKind::Group) {
            match i.status {
                ItemStatus::Running => {
                    running += 1;
                    speed += i.progress.speed.unwrap_or(0.0);
                }
                ItemStatus::Queued => queued += 1,
                _ => {}
            }
        }
        (running, queued, speed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn group_status_and_removal() {
        let mut items = Items::default();
        let g = items.alloc_id();
        items.insert(DownloadItem::new(g, ItemKind::Group, "pl", PathBuf::new()));
        for n in 0..3 {
            let id = items.alloc_id();
            let mut c = DownloadItem::new(id, ItemKind::Media, format!("v{n}"), PathBuf::new());
            c.parent = Some(g);
            c.index = Some(3 - n);
            c.status = if n == 0 { ItemStatus::Completed } else { ItemStatus::Queued };
            items.insert(c);
        }
        assert_eq!(items.children_ids(g).len(), 3);
        // Sorted by playlist index.
        let first = items.get(items.children_ids(g)[0]).unwrap();
        assert_eq!(first.index, Some(1));
        assert_eq!(items.refresh_group(g), Some((ItemStatus::Queued, ItemStatus::Queued)));
        for id in items.children_ids(g) {
            items.get_mut(id).unwrap().status = ItemStatus::Completed;
        }
        assert_eq!(items.refresh_group(g), Some((ItemStatus::Queued, ItemStatus::Completed)));
        assert_eq!(items.top_level().count(), 1);
        assert_eq!(items.remove(g).len(), 4);
        assert_eq!(items.iter().count(), 0);
    }
}
