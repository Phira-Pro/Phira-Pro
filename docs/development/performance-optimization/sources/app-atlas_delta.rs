//! Bounded change history: each 32x32 tile stores its last modified generation.
//! A lagging image can replay changes directly from the authoritative bitmap.
pub const TILE: u32 = 32;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}
pub struct Delta {
    width: u32,
    height: u32,
    columns: u32,
    rows: u32,
    generation: u64,
    versions: Vec<u64>,
}
impl Delta {
    pub fn new(width: u32, height: u32) -> Self {
        assert!(width > 0 && height > 0);
        let columns = width.div_ceil(TILE);
        let rows = height.div_ceil(TILE);
        Self {
            width,
            height,
            columns,
            rows,
            generation: 0,
            versions: vec![0; (columns * rows) as usize],
        }
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn nominal_bytes(&self) -> usize {
        self.versions.capacity() * std::mem::size_of::<u64>()
    }
    pub fn advance(&mut self, writes: &[[u32; 4]]) {
        self.generation = self.generation.checked_add(1).expect("atlas generation overflow");
        for &[x0, y0, x1, y1] in writes {
            assert!(x0 < x1 && y0 < y1 && x1 <= self.width && y1 <= self.height);
            for y in y0 / TILE..y1.div_ceil(TILE) {
                for x in x0 / TILE..x1.div_ceil(TILE) {
                    self.versions[(y * self.columns + x) as usize] = self.generation;
                }
            }
        }
    }
    pub fn since(&self, version: u64) -> Vec<Rect> {
        assert!(version <= self.generation);
        let mut rectangles: Vec<Rect> = Vec::new();
        for row in 0..self.rows {
            let mut x = 0;
            while x < self.columns {
                if self.versions[(row * self.columns + x) as usize] <= version {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < self.columns && self.versions[(row * self.columns + x) as usize] > version {
                    x += 1;
                }
                let rect = Rect {
                    x: start * TILE,
                    y: row * TILE,
                    w: (x * TILE).min(self.width) - start * TILE,
                    h: TILE.min(self.height - row * TILE),
                };
                if let Some(previous) = rectangles
                    .iter_mut()
                    .rev()
                    .find(|r| r.x == rect.x && r.w == rect.w && r.y + r.h == rect.y)
                {
                    previous.h += rect.h;
                } else {
                    rectangles.push(rect);
                }
            }
        }
        rectangles
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn apply(image: &mut [u8], source: &[u8], width: u32, rect: Rect) {
        for y in rect.y..rect.y + rect.h {
            let from = ((y * width + rect.x) * 4) as usize;
            let end = from + (rect.w * 4) as usize;
            image[from..end].copy_from_slice(&source[from..end]);
        }
    }
    #[test]
    fn staggered_images_reconstruct_the_exact_bitmap_after_arbitrary_changes() {
        // Independent byte-array oracle; includes overlapping/edge writes and
        // slots that skip several publications. No finite journal to overflow.
        let (w, h) = (103, 77);
        let mut delta = Delta::new(w, h);
        let mut source = vec![0; (w * h * 4) as usize];
        let mut slots = vec![(0, source.clone()); 4];
        let mut random = 7u32;
        for frame in 0..1000 {
            let mut writes = Vec::new();
            for _ in 0..1 + frame % 5 {
                let mut next = || {
                    random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                    random
                };
                let x0 = next() % w;
                let y0 = next() % h;
                let x1 = (x0 + 1 + next() % 39).min(w);
                let y1 = (y0 + 1 + next() % 35).min(h);
                for y in y0..y1 {
                    for x in x0..x1 {
                        for c in 0..4 {
                            source[((y * w + x) * 4 + c) as usize] = next() as u8;
                        }
                    }
                }
                writes.push([x0, y0, x1, y1]);
            }
            delta.advance(&writes);
            let index = (frame / 3 % 4) as usize;
            let (version, image) = &mut slots[index];
            for rect in delta.since(*version) {
                apply(image, &source, w, rect);
            }
            assert_eq!(*image, source, "publication {frame}");
            *version = delta.generation();
        }
    }
    #[test]
    fn empty_changes_transfer_nothing_and_a_full_change_merges_to_one_rectangle() {
        let mut delta = Delta::new(103, 77);
        delta.advance(&[]);
        assert!(delta.since(0).is_empty());
        delta.advance(&[[0, 0, 103, 77]]);
        assert_eq!(delta.since(0), vec![Rect { x: 0, y: 0, w: 103, h: 77 }]);
        assert!(delta.since(delta.generation()).is_empty());
    }
}
