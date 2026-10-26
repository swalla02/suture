//! The dev/test split: a case is dev when a keyed SipHash of its id is 0
//! mod 5, about 20% of the cases. Fixed before any test run; `split.txt`
//! beside the data holds the result, and a test checks the two agree.

use siphasher::sip::SipHasher24;
use std::hash::Hasher;

const KEY: (u64, u64) = (0x2026_1026, 0xa1_6c1e);

pub fn is_dev(id: &str) -> bool {
    let mut h = SipHasher24::new_with_keys(KEY.0, KEY.1);
    h.write(id.as_bytes());
    h.finish() % 5 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_split_is_the_hash() {
        let cases = crate::data::cases(&crate::data_dir()).unwrap();
        let text = std::fs::read_to_string(crate::data_dir().join("../split.txt")).unwrap();
        let dev: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        let want: Vec<&str> = cases.iter().filter(|c| is_dev(&c.id)).map(|c| c.id.as_str()).collect();
        assert_eq!(dev, want);
        assert!((50..=70).contains(&dev.len()), "{} dev cases", dev.len());
    }
}
