//! The dev/test split: a case is dev when a keyed SipHash of its id is 0
//! mod 5, about 20% of the cases. Fixed before any test run; `split.txt`
//! beside the data holds the result, and a test checks the two agree.
//!
//! The consistency sample: the test cases whose hash under a second key is
//! 0 mod 4, about 60, are asked three times; the rest once (`sample.txt`).
//!
//! The reasoning model's cases (P2): the 100 test cases, and for pricing
//! the 8 dev cases, with the smallest hash under a third key (`p2.txt`).

use siphasher::sip::SipHasher24;
use std::hash::Hasher;

const KEY: (u64, u64) = (0x2026_1026, 0xa1_6c1e);

pub fn is_dev(id: &str) -> bool {
    let mut h = SipHasher24::new_with_keys(KEY.0, KEY.1);
    h.write(id.as_bytes());
    h.finish().is_multiple_of(5)
}

const SAMPLE_KEY: (u64, u64) = (0x2026_1026, 0x5a_3b1e);

pub fn in_sample(id: &str) -> bool {
    let mut h = SipHasher24::new_with_keys(SAMPLE_KEY.0, SAMPLE_KEY.1);
    h.write(id.as_bytes());
    !is_dev(id) && h.finish().is_multiple_of(4)
}

const P2_KEY: (u64, u64) = (0x2026_1027, 0x92_0b1e);

/// The P2 cases of `ids`: the 8 dev cases, then the 100 test cases, with the
/// smallest hash under `P2_KEY`, each group in hash order.
pub fn p2<'a>(ids: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let hash = |id: &str| {
        let mut h = SipHasher24::new_with_keys(P2_KEY.0, P2_KEY.1);
        h.write(id.as_bytes());
        h.finish()
    };
    let mut ids: Vec<&str> = ids.into_iter().collect();
    ids.sort_by_key(|id| hash(id));
    let dev = ids.iter().filter(|id| is_dev(id)).take(8);
    let test = ids.iter().filter(|id| !is_dev(id)).take(100);
    dev.chain(test).copied().collect()
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

    #[test]
    fn the_committed_sample_is_the_hash() {
        let cases = crate::data::cases(&crate::data_dir()).unwrap();
        let text = std::fs::read_to_string(crate::data_dir().join("../sample.txt")).unwrap();
        let got: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        let want: Vec<&str> = cases.iter().filter(|c| in_sample(&c.id)).map(|c| c.id.as_str()).collect();
        assert_eq!(got, want);
        assert!((45..=75).contains(&got.len()), "{} sample cases", got.len());
    }

    #[test]
    fn the_committed_p2_cases_are_the_hash() {
        let cases = crate::data::cases(&crate::data_dir()).unwrap();
        let text = std::fs::read_to_string(crate::data_dir().join("../p2.txt")).unwrap();
        let got: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        let want = p2(cases.iter().map(|c| c.id.as_str()));
        assert_eq!(got, want);
        assert_eq!(want.iter().filter(|id| is_dev(id)).count(), 8);
        assert_eq!(want.len(), 108);
    }
}
