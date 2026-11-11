//! Retained mixed draw records. Stable material IDs are compiled at load;
//! execution still sorts by the original key and keeps insertion order.
use std::collections::BTreeMap;

pub(super) type Key = (i8, u32);

pub(super) struct Record<V> {
    pub page: Option<usize>,
    pub vertices: Vec<V>,
    pub indices: Vec<u16>,
}
impl<V> Default for Record<V> {
    fn default() -> Self {
        Self {
            page: None,
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Growth {
    pub bins: u64,
    pub spans: u64,
    pub vertices: u64,
    pub indices: u64,
}
struct Bin<V> {
    key: Key,
    records: Vec<Record<V>>,
    used: usize,
}
pub(super) struct Records<V> {
    ids: BTreeMap<Key, usize>,
    bins: Vec<Bin<V>>,
    order: Vec<usize>,
    pub growth: Growth,
    pub reserved_bytes: usize,
}
impl<V: Copy + Default> Default for Records<V> {
    fn default() -> Self {
        Self {
            ids: BTreeMap::new(),
            bins: Vec::new(),
            order: Vec::new(),
            growth: Growth::default(),
            reserved_bytes: 0,
        }
    }
}
impl<V: Copy + Default> Records<V> {
    pub fn material(&mut self, key: Key) -> usize {
        if let Some(id) = self.ids.get(&key) {
            return *id;
        }
        let id = self.bins.len();
        let position = self.order.partition_point(|&i| self.bins[i].key < key);
        self.bins.push(Bin {
            key,
            records: Vec::new(),
            used: 0,
        });
        self.order.insert(position, id);
        self.ids.insert(key, id);
        self.growth.bins += 1;
        id
    }
    /// Bounded loading-time reservation. Runtime overflow uses normal Vec
    /// growth, which is counted; this is not a promise of zero allocations.
    pub fn reserve(&mut self, key: Key, vertices: usize, indices: usize) {
        let id = self.material(key);
        let bin = &mut self.bins[id];
        if bin.records.is_empty() {
            bin.records.push(Record::default());
            self.growth.spans += 1;
        }
        let record = &mut bin.records[0];
        let remaining = 16 * 1024 * 1024 - self.reserved_bytes;
        let vertices = vertices.min(remaining / std::mem::size_of::<V>().max(1));
        record.vertices.reserve_exact(vertices);
        record.vertices.resize(vertices, V::default());
        std::hint::black_box(record.vertices.as_slice());
        record.vertices.clear();
        self.reserved_bytes += vertices * std::mem::size_of::<V>();
        let indices = indices.min((16 * 1024 * 1024 - self.reserved_bytes) / 2);
        record.indices.reserve_exact(indices);
        record.indices.resize(indices, 0);
        std::hint::black_box(record.indices.as_slice());
        record.indices.clear();
        self.reserved_bytes += indices * 2;
    }
    fn next(&mut self, id: usize, page: Option<usize>, cpu_limit: usize) -> &mut Record<V> {
        let bin = &mut self.bins[id];
        let merge =
            bin.used > 0 && bin.records[bin.used - 1].page == page && (page.is_some() || bin.records[bin.used - 1].vertices.len() + 4 <= cpu_limit);
        if !merge {
            if bin.used == bin.records.len() {
                bin.records.push(Record::default());
                self.growth.spans += 1;
            }
            let record = &mut bin.records[bin.used];
            record.page = page;
            record.vertices.clear();
            record.indices.clear();
            bin.used += 1;
        }
        &mut bin.records[bin.used - 1]
    }
    pub fn gpu(&mut self, material: usize, page: usize, first: u16) {
        let record = self.next(material, Some(page), 0);
        let growing = record.indices.capacity() < record.indices.len() + 6;
        record
            .indices
            .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
        self.growth.indices += growing as u64;
    }
    pub fn cpu(&mut self, key: Key, quad: [V; 4], max_vertices: usize) {
        let material = self.material(key);
        let record = self.next(material, None, max_vertices.clamp(4, 65536));
        let vg = record.vertices.capacity() < record.vertices.len() + 4;
        let ig = record.indices.capacity() < record.indices.len() + 6;
        let first = record.vertices.len() as u16;
        record.vertices.extend_from_slice(&quad);
        record
            .indices
            .extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
        self.growth.vertices += vg as u64;
        self.growth.indices += ig as u64;
    }
    pub fn iter(&self) -> impl Iterator<Item = (Key, &Record<V>)> {
        self.order.iter().flat_map(|&id| {
            let bin = &self.bins[id];
            bin.records[..bin.used].iter().map(move |r| (bin.key, r))
        })
    }
    pub fn end_frame(&mut self) {
        for bin in &mut self.bins {
            bin.used = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Debug, PartialEq)]
    enum Triangle {
        Cpu([u32; 3]),
        Gpu(usize, [u16; 3]),
    }
    fn triangles(r: &Records<u32>) -> BTreeMap<Key, Vec<Triangle>> {
        let mut result: BTreeMap<Key, Vec<Triangle>> = BTreeMap::new();
        for (key, record) in r.iter() {
            for t in record.indices.chunks_exact(3) {
                result.entry(key).or_default().push(match record.page {
                    Some(page) => Triangle::Gpu(page, t.try_into().unwrap()),
                    None => Triangle::Cpu([
                        record.vertices[t[0] as usize],
                        record.vertices[t[1] as usize],
                        record.vertices[t[2] as usize],
                    ]),
                });
            }
        }
        result
    }
    #[test]
    fn mixed_records_match_unbatched_sorted_triangle_oracle_across_frames() {
        let mut records = Records::<u32>::default();
        let mut seed = 43_u32;
        for _ in 0..20 {
            let mut expected: BTreeMap<Key, Vec<Triangle>> = BTreeMap::new();
            for n in 0..400 {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let key = ((seed % 4) as i8, (seed >> 8) % 7);
                if seed & 32 != 0 {
                    let page = ((seed >> 15) % 3) as usize;
                    let first = n * 4;
                    let id = records.material(key);
                    records.gpu(id, page, first);
                    expected.entry(key).or_default().extend([
                        Triangle::Gpu(page, [first, first + 1, first + 2]),
                        Triangle::Gpu(page, [first, first + 2, first + 3]),
                    ]);
                } else {
                    let b = n as u32 * 4;
                    records.cpu(key, [b, b + 1, b + 2, b + 3], 8);
                    expected
                        .entry(key)
                        .or_default()
                        .extend([Triangle::Cpu([b, b + 1, b + 2]), Triangle::Cpu([b, b + 2, b + 3])]);
                }
            }
            assert_eq!(triangles(&records), expected);
            let keys = records.iter().map(|(k, _)| k).collect::<Vec<_>>();
            assert!(keys.windows(2).all(|v| v[0] <= v[1]));
            records.end_frame();
            assert!(records.iter().next().is_none());
        }
    }
    #[test]
    fn repeated_workload_reuses_spans_and_capacity_without_stale_payloads() {
        let mut records = Records::<u32>::default();
        records.reserve((2, 5), 64, 96);
        let id = records.material((2, 5));
        let mut warm = None;
        for frame in 0..30 {
            records.gpu(id, 2, 0);
            records.cpu((2, 5), [frame, frame + 1, frame + 2, frame + 3], 8);
            records.gpu(id, 1, 4);
            records.cpu((2, 5), [frame + 4, frame + 5, frame + 6, frame + 7], 8);
            assert_eq!(records.iter().count(), 4);
            if frame == 0 {
                warm = Some(records.growth);
            }
            assert_eq!(Some(records.growth), warm);
            assert!(triangles(&records)[&(2, 5)].contains(&Triangle::Cpu([frame, frame + 1, frame + 2])));
            records.end_frame();
        }
    }
    #[test]
    fn late_material_keeps_existing_ids_and_draw_order() {
        let mut records = Records::<u32>::default();
        let high = records.material((3, 100));
        records.gpu(high, 0, 0);
        records.cpu((0, 1), [1, 2, 3, 4], 65536);
        assert_eq!(records.material((3, 100)), high);
        assert_eq!(records.iter().map(|(k, _)| k).collect::<Vec<_>>(), vec![(0, 1), (3, 100)]);
    }
}
