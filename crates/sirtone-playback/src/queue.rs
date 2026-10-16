//! Play queue (T1.8 first slice, FR-PLAY-005/006): a context (album, artist, collection list)
//! played from a start index; starting a track from another context replaces the queue.
//! Shuffle, repeat, «играть следующим» and the queue panel come with Stage 2.

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

#[derive(Debug, Clone, Default)]
pub struct Queue<T> {
    items: Vec<T>,
    index: usize,
}

impl<T: Clone> Queue<T> {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            index: 0,
        }
    }

    /// FR-PLAY-006: a track started from a context replaces the whole queue with that context.
    pub fn replace(&mut self, items: Vec<T>, start: usize) -> Step<T> {
        self.items = items;
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
        self.index + 1 < self.items.len()
    }

    pub fn has_prev(&self) -> bool {
        self.index > 0
    }

    /// «Вперёд» and the automatic advance at the end of a track.
    pub fn advance(&mut self) -> Step<T> {
        if self.has_next() {
            self.index += 1;
            Step::Play(self.items[self.index].clone())
        } else {
            Step::End
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

#[cfg(test)]
mod tests {
    use super::*;

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
