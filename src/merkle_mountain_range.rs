use crypto_hash::{Algorithm, digest};
use std::convert::TryInto;

use crate::merkle_mountain_range::mem_store::Storer;

pub mod mem_store;

const MMR_LEAF_PREFIX: u8 = 0;
const MMR_NODE_PREFIX: u8 = 1;

fn depth(store: &impl Storer) -> usize {
    let s: usize = store.size();
    let bits: u32 = s.count_ones() + s.count_zeros();
    return (bits - s.leading_zeros()).try_into().unwrap();
}

fn root(store: &impl Storer) -> Vec<u8> {
    let d: usize = depth(store);
    if d == 0 {
        return digest(Algorithm::SHA256, &[]);
    }
    let h_opt: Option<&Vec<u8>> = store.get(d-1, 0);
    return h_opt.unwrap().to_vec();
}

fn leaf_hash(data: Vec<u8>) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::new();
    buf.push(MMR_LEAF_PREFIX);
    buf.extend(data.iter().cloned());
    return digest(Algorithm::SHA256, &buf);
}

fn append(store: &mut impl Storer, data: Vec<u8>) {
    let h: Vec<u8> = leaf_hash(data);
    append_hash(store, h);
}

fn append_hash(store: &mut impl Storer, h: Vec<u8>) {
    // append the leaf
    let mut s: usize = store.size();
    store.set(0, s, h.to_vec());
    s += 1;

    // rebuild the root
    let mut i: usize = 0;
    let mut c: Vec<u8> = h.to_vec();
    let mut t: Vec<u8> = Vec::new();
    while s > 1 {
        if s % 2 == 0 {
            t.resize(1, MMR_NODE_PREFIX);
            t.extend(store.get(i, s-2).unwrap().iter().cloned());
            t.extend(c.to_vec().iter().cloned());
            c.resize(0, 0);
            c.extend(digest(Algorithm::SHA256, &t));
            i += 1;
            s >>= 1;
            store.set(i, s-1, c.to_vec());
        } else {
            s += 1;
            i += 1;
            s >>= 1;
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merkle_mountain_range::mem_store::MemStore;

    #[test]
    fn test_root() {
        let mut mem_store: MemStore = Storer::new();
        let expected_1: Vec<u8> = digest(Algorithm::SHA256, &[]);
        let computed_1: Vec<u8> = root(&mem_store);
        assert_eq!(expected_1, computed_1);

        let value: Vec<u8> = vec![104, 101, 108, 108, 109];
        append(&mut mem_store, value.to_vec());
        let expected_2: Vec<u8> = leaf_hash(value.to_vec());
        let computed_2: Vec<u8> = root(&mem_store);
        assert_eq!(expected_2, computed_2);
    }
}
