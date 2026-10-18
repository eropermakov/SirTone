//! Play queue (T1.8 first slice, FR-PLAY-005/006): a context (album, artist, collection list)
//! played from a start index; starting a track from another context replaces the queue.
//! Stage 2 (T2.11/T2.12, `01b` F-21): «играть следующим», «в конец очереди», removal and
//! reordering for the queue panel, repeat (off / track / context) and the «Fewer Repeats»
//! shuffle copied from Spotify.

/// What to do after a queue operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step<T> {
    /// Load and play this item.
    Play(T),
    /// Stay on the current item, back to its start.
    Restart,
    /// Nothing left: stop at the end.
    End,
}

/// FR-PLAY-007: repeat off / the current track / the whole context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    One,
    All,
}

#[derive(Debug, Clone, Default)]
pub struct Queue<T> {
    items: Vec<T>,
    index: usize,
    repeat: Repeat,
    /// The upcoming order before shuffling (restored when shuffle is turned off).
    unshuffled: Option<Vec<T>>,
}

impl<T: Clone> Queue<T> {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            index: 0,
            repeat: Repeat::Off,
            unshuffled: None,
        }
    }

    /// FR-PLAY-006: a track started from a context replaces the whole queue with that context.
    pub fn replace(&mut self, items: Vec<T>, start: usize) -> Step<T> {
        self.items = items;
        self.unshuffled = None;
        self.index = start.min(self.items.len().saturating_sub(1));
        match self.items.get(self.index) {
            Some(t) => Step::Play(t.clone()),
            None => Step::End,
        }
    }

    pub fn current(&self) -> Option<&T> {
        self.items.get(self.index)
    }

    /// The item after the current one (handed to the engine in advance for gapless play).
    pub fn peek_next(&self) -> Option<&T> {
        self.items.get(self.index + 1)
    }

    pub fn has_next(&self) -> bool {
        self.index + 1 < self.items.len() || (self.repeat == Repeat::All && !self.items.is_empty())
    }

    pub fn has_prev(&self) -> bool {
        self.index > 0
    }

    /// «Вперёд»: always moves on (repeat-one does not hold a manual skip).
    pub fn advance(&mut self) -> Step<T> {
        if self.index + 1 < self.items.len() {
            self.index += 1;
            Step::Play(self.items[self.index].clone())
        } else if self.repeat == Repeat::All && !self.items.is_empty() {
            self.index = 0;
            Step::Play(self.items[0].clone())
        } else {
            Step::End
        }
    }

    /// The automatic advance at the natural end of a track: repeat-one plays it again.
    pub fn on_track_end(&mut self) -> Step<T> {
        match (self.repeat, self.current()) {
            (Repeat::One, Some(t)) => Step::Play(t.clone()),
            _ => self.advance(),
        }
    }

    /// What follows the current track without a user action (for the gapless hand-over).
    pub fn upcoming(&self) -> Option<&T> {
        match self.repeat {
            Repeat::One => self.current(),
            Repeat::All if self.index + 1 >= self.items.len() => self.items.first(),
            _ => self.peek_next(),
        }
    }

    pub fn repeat(&self) -> Repeat {
        self.repeat
    }

    pub fn set_repeat(&mut self, r: Repeat) {
        self.repeat = r;
    }

    /// «Играть следующим»: right after the current track, in the given order.
    pub fn play_next(&mut self, items: Vec<T>) -> Step<T> {
        if self.items.is_empty() {
            return self.replace(items, 0);
        }
        let at = self.index + 1;
        self.items.splice(at..at, items);
        self.unshuffled = None;
        Step::Restart
    }

    /// «В конец очереди».
    pub fn append(&mut self, items: Vec<T>) -> Step<T> {
        if self.items.is_empty() {
            return self.replace(items, 0);
        }
        self.items.extend(items);
        self.unshuffled = None;
        Step::Restart
    }

    /// Removes an upcoming or earlier item (the playing one stays: it is skipped, not removed).
    pub fn remove(&mut self, at: usize) -> bool {
        if at == self.index || at >= self.items.len() {
            return false;
        }
        self.items.remove(at);
        if at < self.index {
            self.index -= 1;
        }
        self.unshuffled = None;
        true
    }

    /// Drag in the queue panel: the playing item keeps playing wherever it lands.
    pub fn move_item(&mut self, from: usize, to: usize) -> bool {
        let n = self.items.len();
        if from >= n || to >= n || from == to {
            return false;
        }
        let item = self.items.remove(from);
        self.items.insert(to, item);
        self.index = if from == self.index {
            to
        } else if from < self.index && to >= self.index {
            self.index - 1
        } else if from > self.index && to <= self.index {
            self.index + 1
        } else {
            self.index
        };
        self.unshuffled = None;
        true
    }

    /// A click in the queue panel: play that item, the queue stays as it is.
    pub fn jump(&mut self, at: usize) -> Step<T> {
        match self.items.get(at) {
            Some(t) => {
                self.index = at;
                Step::Play(t.clone())
            }
            None => Step::End,
        }
    }

    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn shuffled(&self) -> bool {
        self.unshuffled.is_some()
    }

    /// FR-PLAY-007 shuffle on: the tracks after the current one are reordered by
    /// [`fewer_repeats`]; off: their original order comes back.
    pub fn set_shuffle(&mut self, on: bool, seed: u64, key: impl Fn(&T) -> ShuffleKey) {
        let at = (self.index + 1).min(self.items.len());
        if on {
            if self.unshuffled.is_none() {
                let rest = self.items[at..].to_vec();
                let order = fewer_repeats(&rest.iter().map(&key).collect::<Vec<_>>(), seed);
                let shuffled: Vec<T> = order.iter().map(|&i| rest[i].clone()).collect();
                self.items.truncate(at);
                self.items.extend(shuffled);
                self.unshuffled = Some(rest);
            }
        } else if let Some(rest) = self.unshuffled.take() {
            self.items.truncate(at);
            self.items.extend(rest);
        }
    }

    /// «Назад»: the previous track; on the first one — back to its start.
    pub fn back(&mut self) -> Step<T> {
        if self.has_prev() {
            self.index -= 1;
            Step::Play(self.items[self.index].clone())
        } else if self.items.is_empty() {
            Step::End
        } else {
            Step::Restart
        }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn position(&self) -> usize {
        self.index
    }
}

/// What the shuffle looks at for one track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShuffleKey {
    pub artist: u64,
    pub album: u64,
    /// Heard in the last day (from the listening journal).
    pub recent: bool,
}

/// Candidate permutations tried (Spotify «Fewer Repeats» generates several and keeps the best).
const CANDIDATES: usize = 12;

/// xorshift64*: deterministic for a seed (tests), seeded from the clock in the app.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

/// Lower is better: the same artist back to back costs most, the same album less (iTunes Smart
/// Shuffle), recently heard tracks near the start cost by how early they come (Spotify).
fn penalty(keys: &[ShuffleKey], order: &[usize]) -> f64 {
    let mut p = 0.0;
    for w in order.windows(2) {
        let (a, b) = (keys[w[0]], keys[w[1]]);
        if a.artist == b.artist {
            p += 10.0;
        } else if a.album == b.album {
            p += 3.0;
        }
    }
    let n = order.len().max(1) as f64;
    for (pos, &i) in order.iter().enumerate() {
        if keys[i].recent {
            p += 5.0 * (1.0 - pos as f64 / n);
        }
    }
    p
}

/// «Fewer Repeats» (Spotify, `01b` F-21): several random permutations (Fisher–Yates), keep the
/// one with the lowest [`penalty`]. Returns the new order as indices into `keys`.
pub fn fewer_repeats(keys: &[ShuffleKey], seed: u64) -> Vec<usize> {
    let mut rng = Rng(seed | 1);
    let mut best: Option<(f64, Vec<usize>)> = None;
    for _ in 0..CANDIDATES {
        let mut order: Vec<usize> = (0..keys.len()).collect();
        for i in (1..order.len()).rev() {
            let j = (rng.next() % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        let p = penalty(keys, &order);
        if best.as_ref().is_none_or(|(bp, _)| p < *bp) {
            best = Some((p, order));
        }
    }
    best.map(|(_, o)| o).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_next_append_remove_move() {
        let mut q = Queue::new();
        assert_eq!(
            q.append(vec![1, 2]),
            Step::Play(1),
            "empty queue starts playing"
        );
        q.play_next(vec![9, 8]);
        assert_eq!(q.items(), &[1, 9, 8, 2]);
        q.append(vec![7]);
        assert_eq!(q.items(), &[1, 9, 8, 2, 7]);
        assert!(!q.remove(0), "the playing track is not removed");
        assert!(q.remove(2));
        assert_eq!(q.items(), &[1, 9, 2, 7]);
        // Move the playing track down: it keeps playing at its new place.
        assert!(q.move_item(0, 2));
        assert_eq!((q.items(), q.position()), (&[9, 2, 1, 7][..], 2));
        assert!(q.move_item(3, 0));
        assert_eq!((q.items(), q.position()), (&[7, 9, 2, 1][..], 3));
        assert_eq!(q.advance(), Step::End);
        assert_eq!(q.jump(1), Step::Play(9));
        assert_eq!(q.position(), 1);
        assert_eq!(q.jump(9), Step::End);
    }

    #[test]
    fn repeat_modes() {
        let mut q = Queue::new();
        q.replace(vec![1, 2], 1);
        q.set_repeat(Repeat::One);
        assert_eq!(q.upcoming(), Some(&2));
        assert_eq!(
            q.on_track_end(),
            Step::Play(2),
            "repeat one replays at the end"
        );
        assert_eq!(q.advance(), Step::End, "a manual skip still moves on");
        q.set_repeat(Repeat::All);
        assert!(q.has_next());
        assert_eq!(q.upcoming(), Some(&1));
        assert_eq!(q.on_track_end(), Step::Play(1), "repeat all wraps around");
    }

    #[test]
    fn fewer_repeats_spreads_artists_and_restores_order() {
        // 4 artists × 3 tracks: a good shuffle never puts one artist twice in a row.
        let keys: Vec<ShuffleKey> = (0..12)
            .map(|i| ShuffleKey {
                artist: i / 3,
                album: i / 3,
                recent: false,
            })
            .collect();
        for seed in 1..50 {
            let order = fewer_repeats(&keys, seed);
            let mut sorted = order.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..12).collect::<Vec<_>>(), "a permutation");
            let same = order
                .windows(2)
                .filter(|w| keys[w[0]].artist == keys[w[1]].artist)
                .count();
            assert!(same <= 1, "seed {seed}: {same} same-artist neighbours");
        }
        let mut q = Queue::new();
        q.replace((0..12).collect::<Vec<u64>>(), 0);
        q.set_shuffle(true, 7, |&i| ShuffleKey {
            artist: i / 3,
            album: i / 3,
            recent: false,
        });
        assert_eq!(q.items()[0], 0, "the playing track stays first");
        assert_ne!(q.items(), (0..12).collect::<Vec<_>>().as_slice());
        q.set_shuffle(false, 0, |_| ShuffleKey::default());
        assert_eq!(
            q.items(),
            (0..12).collect::<Vec<_>>().as_slice(),
            "order restored"
        );
    }

    #[test]
    fn recently_heard_go_later() {
        let keys: Vec<ShuffleKey> = (0..20)
            .map(|i| ShuffleKey {
                artist: i,
                album: i,
                recent: i < 5,
            })
            .collect();
        let mut first_half_recent = 0;
        for seed in 1..40 {
            let order = fewer_repeats(&keys, seed);
            first_half_recent += order[..10].iter().filter(|&&i| keys[i].recent).count();
        }
        // Uniform would put 2.5 of 5 in the first half; the penalty pushes them back.
        assert!(first_half_recent < 39 * 2, "{first_half_recent}");
    }

    #[test]
    fn context_play_next_prev_and_replace() {
        let mut q = Queue::new();
        assert_eq!(q.advance(), Step::<u32>::End);
        assert_eq!(q.replace(vec![10, 11, 12], 1), Step::Play(11));
        assert_eq!(q.advance(), Step::Play(12));
        assert_eq!(q.advance(), Step::End, "end of the context");
        assert_eq!(q.back(), Step::Play(11));
        assert_eq!(q.back(), Step::Play(10));
        assert_eq!(q.back(), Step::Restart, "first track restarts");
        // Another context replaces the queue (FR-PLAY-006).
        assert_eq!(q.replace(vec![20, 21], 0), Step::Play(20));
        assert_eq!((q.len(), q.position(), q.current()), (2, 0, Some(&20)));
        assert_eq!(q.replace(vec![30], 9), Step::Play(30), "start is clamped");
        assert_eq!(q.replace(Vec::new(), 0), Step::End);
    }
}
