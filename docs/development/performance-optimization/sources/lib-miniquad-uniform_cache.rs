//! Exact uniform-value reuse. No floating-point comparison or approximation.
use std::{collections::HashSet, ops::Range};

#[derive(Clone, Copy, Debug, Default)]
pub struct UniformUploadStats {
    pub enabled: bool,
    pub issued_calls: u64,
    pub issued_bytes: u64,
    pub skipped_calls: u64,
    pub skipped_bytes: u64,
    pub uncached_fields: u64,
    pub cached_bytes: usize,
}

#[derive(Debug, Default)]
pub(super) struct Value {
    bytes: Vec<u8>,
    valid: bool,
}
impl Value {
    pub fn matches(&self, bytes: &[u8]) -> bool { self.valid && self.bytes == bytes }
    pub fn storage_len(&self) -> usize { self.bytes.len() }
    pub fn remember(&mut self, bytes: &[u8]) {
        self.bytes.clear(); self.bytes.extend_from_slice(bytes); self.valid = true;
    }
    pub fn invalidate(&mut self) { self.valid = false; }
}

pub(super) fn byte_range(offset: usize, unit: usize, count: usize, size: usize) -> Option<Range<usize>> {
    let end = offset.checked_add(unit.checked_mul(count)?)?;
    (end <= size).then_some(offset..end)
}

// Indexed names may partially alias another descriptor's array. Sampler
// assignments are made by apply_bindings outside the packed uniform block.
// Preserve ordinary uploads for ambiguous layouts, even when caching is on.
pub(super) fn names_disjoint<'a>(uniforms: impl Iterator<Item=&'a str>, images: impl Iterator<Item=&'a str>) -> bool {
    let mut names = HashSet::new();
    for name in uniforms.chain(images) {
        if name.contains('[') || !names.insert(name) { return false; }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_array_bytes_and_overflow_are_checked() {
        assert_eq!(byte_range(128,16,32,640),Some(128..640));
        assert_eq!(byte_range(128,16,32,639),None);
        assert_eq!(byte_range(usize::MAX,4,1,usize::MAX),None);
        assert_eq!(byte_range(0,64,usize::MAX,usize::MAX),None);
    }
    #[test]
    fn names_that_can_alias_use_original_uploads() {
        assert!(names_disjoint(["Model","Pose"].iter().copied(),["Texture"].iter().copied()));
        for names in [["Model","Model"],["Pose","Pose[1]"],["Pose[0]","Other"],["Texture","Other"]] {
            assert!(!names_disjoint(names.iter().copied(),["Texture"].iter().copied()));
        }
    }
    #[test]
    fn independent_programs_match_an_unconditional_driver_oracle() {
        let mut memo: Vec<Vec<Value>>=(0..3).map(|_|(0..4).map(|_|Value::default()).collect()).collect();
        let mut actual=vec![vec![Vec::<u8>::new();4];3];let mut oracle=actual.clone();
        let mut seed=0x1234abcd_u32;let mut skipped=0;
        for round in 0..2400 {
            seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let program=(seed as usize)%3;let field=((seed>>8) as usize)%4;
            let len=[4,16,64,512][field];
            let data=if round%4!=0 && !oracle[program][field].is_empty() {
                oracle[program][field].clone()
            } else {(0..len).map(|i|seed.wrapping_add(i as u32*17) as u8).collect()};
            if round%137==0 { for p in &mut memo {for v in p {v.invalidate();}} }
            oracle[program][field]=data.clone();
            if memo[program][field].matches(&data) {skipped+=1;} else {
                actual[program][field]=data.clone();memo[program][field].remember(&data);
            }
            assert_eq!(actual,oracle,"program state at {round}");
        }
        assert!(skipped>1000);
    }
    #[test]
    fn signed_zero_nan_payloads_and_raw_write_invalidation_remain_distinct() {
        let mut v=Value::default();
        for bits in [0_u32,0x80000000,0x7fc00001,0x7fc00002,0x7fc00001] {
            let bytes=bits.to_ne_bytes();assert!(!v.matches(&bytes));v.remember(&bytes);
            assert!(v.matches(&bytes));v.invalidate();assert!(!v.matches(&bytes));v.remember(&bytes);
        }
    }
}
