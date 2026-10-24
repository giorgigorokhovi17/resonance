//! Play queue: ordered items, current pointer, shuffle and repeat.

use serde::{Deserialize, Serialize};

use crate::library::model::Track;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub uid: u64,
    pub track: Track,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct QueueSnapshot {
    pub items: Vec<QueueItem>,
    pub current_uid: Option<u64>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

#[derive(Default, Serialize, Deserialize)]
pub struct PlayQueue {
    items: Vec<QueueItem>,
    current: Option<u64>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    /// Item order before shuffling, used to restore when shuffle is turned off.
    original_order: Vec<u64>,
    next_uid: u64,
}

struct Rng(u64);
impl Rng {
    fn seeded() -> Self {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x1234_5678);
        Rng(t | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

impl PlayQueue {
    fn make_items(&mut self, tracks: Vec<Track>) -> Vec<QueueItem> {
        tracks
            .into_iter()
            .map(|track| {
                self.next_uid += 1;
                QueueItem { uid: self.next_uid, track }
            })
            .collect()
    }

    pub fn snapshot(&self) -> QueueSnapshot {
        QueueSnapshot {
            items: self.items.clone(),
            current_uid: self.current,
            shuffle: self.shuffle,
            repeat: self.repeat,
        }
    }

    /// Rebuild a queue saved on a previous run (items in their saved order).
    pub fn restore(tracks: Vec<Track>, current: Option<usize>, shuffle: bool, repeat: RepeatMode) -> Self {
        let mut q = PlayQueue::default();
        let items = q.make_items(tracks);
        q.current = current.and_then(|i| items.get(i)).map(|i| i.uid);
        q.original_order = items.iter().map(|i| i.uid).collect();
        q.items = items;
        q.shuffle = shuffle;
        q.repeat = repeat;
        q
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn index_of(&self, uid: u64) -> Option<usize> {
        self.items.iter().position(|i| i.uid == uid)
    }

    pub fn get(&self, uid: u64) -> Option<&QueueItem> {
        self.items.iter().find(|i| i.uid == uid)
    }

    pub fn current(&self) -> Option<&QueueItem> {
        self.current.and_then(|u| self.get(u))
    }

    pub fn current_uid(&self) -> Option<u64> {
        self.current
    }

    pub fn set_current(&mut self, uid: Option<u64>) {
        self.current = uid;
    }

    /// Replace the queue with `tracks` and make `start` current. Returns the item to play.
    pub fn replace(&mut self, tracks: Vec<Track>, start: usize) -> Option<QueueItem> {
        let items = self.make_items(tracks);
        let start_uid = items.get(start.min(items.len().saturating_sub(1))).map(|i| i.uid);
        self.items = items;
        self.original_order = self.items.iter().map(|i| i.uid).collect();
        self.current = start_uid;
        if self.shuffle {
            self.shuffle_upcoming(true);
        }
        self.current().cloned()
    }

    pub fn append(&mut self, tracks: Vec<Track>) -> Vec<QueueItem> {
        let items = self.make_items(tracks);
        self.original_order.extend(items.iter().map(|i| i.uid));
        self.items.extend(items.iter().cloned());
        items
    }

    /// Insert right after the current item.
    pub fn insert_next(&mut self, tracks: Vec<Track>) -> Vec<QueueItem> {
        let items = self.make_items(tracks);
        let pos = self
            .current
            .and_then(|u| self.index_of(u))
            .map(|i| i + 1)
            .unwrap_or(self.items.len());
        for (k, it) in items.iter().enumerate() {
            self.items.insert(pos + k, it.clone());
        }
        // Keep "original order" consistent: place after the current item there too.
        let opos = self
            .current
            .and_then(|u| self.original_order.iter().position(|x| *x == u))
            .map(|i| i + 1)
            .unwrap_or(self.original_order.len());
        for (k, it) in items.iter().enumerate() {
            self.original_order.insert(opos + k, it.uid);
        }
        items
    }

    /// Removes items. The currently playing item is never removed.
    pub fn remove(&mut self, uids: &[u64]) {
        let cur = self.current;
        self.items.retain(|i| Some(i.uid) == cur || !uids.contains(&i.uid));
        self.original_order.retain(|u| Some(*u) == cur || !uids.contains(u));
    }

    /// Clears everything except the current item.
    pub fn clear(&mut self) {
        let cur = self.current;
        self.items.retain(|i| Some(i.uid) == cur);
        self.original_order.retain(|u| Some(*u) == cur);
    }

    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return;
        }
        let it = self.items.remove(from);
        self.items.insert(to, it);
        if !self.shuffle {
            self.original_order = self.items.iter().map(|i| i.uid).collect();
        }
    }

    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        if on {
            self.original_order = self.items.iter().map(|i| i.uid).collect();
            self.shuffle_upcoming(false);
        } else {
            // Restore original order; items added later keep their relative order at the end.
            let mut restored = Vec::with_capacity(self.items.len());
            for uid in &self.original_order {
                if let Some(it) = self.items.iter().find(|i| i.uid == *uid) {
                    restored.push(it.clone());
                }
            }
            for it in &self.items {
                if !self.original_order.contains(&it.uid) {
                    restored.push(it.clone());
                }
            }
            self.items = restored;
        }
    }

    /// Shuffle everything except the current item, which moves to the front.
    fn shuffle_upcoming(&mut self, current_first: bool) {
        let mut rng = Rng::seeded();
        let cur_idx = self.current.and_then(|u| self.index_of(u));
        let mut rest: Vec<QueueItem> = Vec::with_capacity(self.items.len());
        let mut head: Vec<QueueItem> = Vec::new();
        for (i, it) in self.items.drain(..).enumerate() {
            if Some(i) == cur_idx {
                head.push(it);
            } else {
                rest.push(it);
            }
        }
        for i in (1..rest.len()).rev() {
            let j = rng.below(i + 1);
            rest.swap(i, j);
        }
        let _ = current_first;
        head.extend(rest);
        self.items = head;
    }

    /// The item that follows `uid`. `auto` means natural end of track (repeat-one replays).
    pub fn next_after(&self, uid: Option<u64>, auto: bool) -> Option<QueueItem> {
        if self.items.is_empty() {
            return None;
        }
        let Some(uid) = uid else { return self.items.first().cloned() };
        if auto && self.repeat == RepeatMode::One {
            if let Some(it) = self.get(uid) {
                return Some(it.clone());
            }
        }
        let idx = self.index_of(uid)?;
        if idx + 1 < self.items.len() {
            Some(self.items[idx + 1].clone())
        } else if self.repeat != RepeatMode::Off {
            self.items.first().cloned()
        } else {
            None
        }
    }

    pub fn prev_before(&self, uid: Option<u64>) -> Option<QueueItem> {
        let uid = uid?;
        let idx = self.index_of(uid)?;
        if idx > 0 {
            Some(self.items[idx - 1].clone())
        } else if self.repeat == RepeatMode::All {
            self.items.last().cloned()
        } else {
            self.items.first().cloned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(n: &str) -> Track {
        Track { title: n.into(), path: n.into(), ..Default::default() }
    }

    #[test]
    fn next_and_repeat() {
        let mut q = PlayQueue::default();
        let first = q.replace(vec![t("a"), t("b"), t("c")], 0).unwrap();
        let b = q.next_after(Some(first.uid), true).unwrap();
        assert_eq!(b.track.title, "b");
        let c = q.next_after(Some(b.uid), true).unwrap();
        assert!(q.next_after(Some(c.uid), true).is_none());
        q.repeat = RepeatMode::All;
        assert_eq!(q.next_after(Some(c.uid), true).unwrap().track.title, "a");
        q.repeat = RepeatMode::One;
        assert_eq!(q.next_after(Some(c.uid), true).unwrap().track.title, "c");
        assert_eq!(q.next_after(Some(b.uid), false).unwrap().track.title, "c");
    }

    #[test]
    fn shuffle_restores_order() {
        let mut q = PlayQueue::default();
        let tracks: Vec<Track> = (0..20).map(|i| t(&i.to_string())).collect();
        q.replace(tracks, 5);
        q.set_shuffle(true);
        assert_eq!(q.snapshot().items[0].track.title, "5");
        q.set_shuffle(false);
        let titles: Vec<String> = q.snapshot().items.iter().map(|i| i.track.title.clone()).collect();
        assert_eq!(titles, (0..20).map(|i| i.to_string()).collect::<Vec<_>>());
    }

    #[test]
    fn insert_next_and_remove() {
        let mut q = PlayQueue::default();
        let a = q.replace(vec![t("a"), t("b")], 0).unwrap();
        q.insert_next(vec![t("x")]);
        assert_eq!(q.next_after(Some(a.uid), false).unwrap().track.title, "x");
        q.remove(&[a.uid]);
        assert_eq!(q.len(), 3, "current item must not be removed");
    }
}
