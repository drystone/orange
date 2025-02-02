mod store;
mod test_data;

use crypto_hash::{Algorithm, digest};

use crate::store::Storer;
use crate::store::mem_store::MemStore;

struct MerkleHashTree<T: Storer> {
    pub store: T,
}

impl<T: Storer> MerkleHashTree<T> {
    const MHT_LEAF_PREFIX: u8 = 0;
    const MHT_NODE_PREFIX: u8 = 1;
    fn new(s: T) -> MerkleHashTree<T> {
        return MerkleHashTree {
            store: s,
        };
    }
    fn min_num_bits(x: isize) -> isize {
        /*
            there's an isize::BITS function that does this, but it is only a "nightly" build,
            so just working around that for now with count_ones() and count_zeros()
        */
        let total_bits: u32 = x.count_ones() + x.count_zeros();
        return (total_bits - x.leading_zeros()) as isize;
    }
    fn depth(&self) -> isize {
        /*
            canonically, a tree of size zero has depth of negative one
        */
        let width: isize = self.store.width();
        if width == 0 {
            return -1;
        }
        /*
            note that width is num leaves in tree, min_num_bits(width) is essentially log2 operation
        */
        return MerkleHashTree::<T>::min_num_bits(width - 1);
    }
    fn root(&self) -> Vec<u8> {
        /*
            per RFC the root hash of an empty tree is hash of empty string
        */
        let depth: isize = self.depth();
        if depth == -1 {
            return digest(Algorithm::SHA256, b"");
        }
        return self.store.get(depth, 0).unwrap();
    }
    fn hash_leaf(&self, data: Vec<u8>) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        buf.push(MerkleHashTree::<T>::MHT_LEAF_PREFIX);
        buf.extend(data.iter().cloned());
        return digest(Algorithm::SHA256, &buf);
    }
    fn append(&mut self, data: Vec<u8>) {
        self.append_hash(self.hash_leaf(data));
    }
    fn append_hash(&mut self, leaf_hash: Vec<u8>) {
        // append the leaf
        let mut width: isize = self.store.width();
        self.store.set(0, width, leaf_hash.to_vec());
        width += 1;

        // rebuild the root
        let mut i: isize = 0;
        let mut c: Vec<u8> = leaf_hash.to_vec();
        let mut t: Vec<u8> = Vec::new();
        while width > 1 {
            if width % 2 == 0 {
                t.resize(1, MerkleHashTree::<T>::MHT_NODE_PREFIX);
                t.extend(self.store.get(i, width-2).unwrap());
                t.extend(c.to_vec());
                c.resize(0, 0);
                c.extend(digest(Algorithm::SHA256, &t));
                i += 1;
                width >>= 1;
                self.store.set(i, width-1, c.to_vec());
            } else {
                width += 1;
                i += 1;
                width >>= 1;
            }
        }
    }
    fn is_frozen(layer: isize, index: isize, at: isize) -> bool {
        /*
            when a left subtree becomes perfect 2^i, it becomes "frozen"
        */
        let a: isize = 1 << layer; // 6 -> 64, 7 -> 128 etc
        return at >= index * a + a - 1;
    }
    fn hash_at(&self, l: isize, r: isize, at: isize) -> Vec<u8> {
        if r == l {
            return self.store.get(0, r).unwrap();
        }

        let layer: isize = MerkleHashTree::<T>::min_num_bits(r-l); // height of subtree
        let a: isize = 1 << layer; // width of subtree

        if at >= l+a-1 || at == self.store.width()-1 {
            return self.store.get(layer, l/a).unwrap();
        }

        let k: isize = a / 2;
        let mut c: Vec<u8> = Vec::new();
        c.push(MerkleHashTree::<T>::MHT_NODE_PREFIX);
        c.extend(self.hash_at(l, l+k-1, at).iter().cloned());
        c.extend(self.hash_at(l+k, r, at).iter().cloned());
        return digest(Algorithm::SHA256, &c);
    }
    fn inclusion_proof(&self, at: isize, i: isize) -> Option<Vec<Vec<u8>>> {
        let w: isize = self.store.width();
        if at == 0 && i == 0 {
            return Some(vec![]);
        }
        if i > at || at >= w || at < 1 {
            return None;
        }

        let mut m: isize = i;
        let mut n: isize = at + 1;

        let mut offset: isize = 0;
        let mut l: isize;
        let mut r: isize;
        let mut p: Vec<Vec<u8>> = Vec::new();
        loop {
            let d: isize = MerkleHashTree::<T>::min_num_bits(n - 1);
            let k: isize = 1 << (d - 1);
            if m < k {
                l = offset + k;
                r = offset + n - 1;
                n = k;
            } else {
                l = offset;
                r = offset + k - 1;
                m = m - k;
                n = n - k;
                offset += k;
            }

            p.insert(0, self.hash_at(l, r, at));
            if n < 1 || (n == 1 && m == 0) {
                return Some(p);
            }
        }
    }
    fn verify_inclusion(path: Vec<Vec<u8>>, root: Vec<u8>, leaf: Vec<u8>, mut at: isize, mut i: isize, ) -> bool {
        if i > at || (at > 0 && path.len() == 0) {
            return false;
        }

        let mut h: Vec<u8> = leaf;
        for p in path.iter() {
            let mut c: Vec<u8> = Vec::new();
            c.push(MerkleHashTree::<T>::MHT_NODE_PREFIX);
            if i % 2 == 0 && i != at {
                c.extend(h.iter().cloned());
                c.extend(p);
            } else {
                c.extend(p);
                c.extend(h.iter().cloned());
            }
            h = digest(Algorithm::SHA256, &c);
            i /= 2;
            at /= 2;
        }

        return at == i && h == root;
    }
}

#[cfg(test)]
use crate::test_data::{get_test_roots, get_test_paths};
mod tests {
    use super::*;
    use std::time::{Instant};
    use std::convert::TryFrom;

    #[test]
    fn test_append() {
        let mut mht: MerkleHashTree<MemStore> = MerkleHashTree::new(MemStore::new());
        assert_eq!(-1, mht.depth());
        for index in 0..64 {
            let b: Vec<u8> = index.to_string().as_bytes().to_vec();
            mht.append(b);

            assert_eq!(index as isize, mht.store.width()-1);
            let d: f64 = ((index + 1) as f64).log2().ceil();
            assert_eq!(d as isize, mht.depth());

            assert_eq!(get_test_roots()[index as usize], mht.root());
        }
    }
    #[test]
    fn test_root() {
        let mut mht: MerkleHashTree<MemStore> = MerkleHashTree::new(MemStore::new());
        assert_eq!(digest(Algorithm::SHA256, b""), mht.root());
        let value: Vec<u8> = "my value".as_bytes().to_vec();
        mht.append(value.to_vec());
        assert_eq!(mht.hash_leaf(value), mht.root());
    }
    #[test]
    fn test_is_frozen() {
        // layer, index, width
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(0, 0, 0), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(0, 7, 6), false);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(0, 7, 7), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(3, 0, 6), false);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(2, 0, 6), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(2, 1, 6), false);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(1, 0, 6), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(1, 1, 6), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(1, 2, 6), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(1, 3, 6), false);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(0, 0, 6), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(0, 1, 6), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(0, 2, 6), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(0, 3, 6), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(0, 4, 6), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(0, 5, 6), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(0, 6, 6), true);
        assert_eq!(MerkleHashTree::<MemStore>::is_frozen(0, 7, 6), false);
    }
    fn mth(d: Vec<Vec<u8>>) -> Vec<u8> {
        /*
            note this is to test against the reference implementation as per
                https://tools.ietf.org/html/rfc6962#section-2.1
        */
        let n: isize = isize::try_from(d.len()).unwrap();
        if n == 0 {
            return digest(Algorithm::SHA256, b"");
        }
        if n == 1 {
            let mut c: Vec<u8> = Vec::new();
            c.push(MerkleHashTree::<MemStore>::MHT_LEAF_PREFIX);
            c.extend(d[0].to_vec());
            return digest(Algorithm::SHA256, &c);
        }

        let k: usize = 1 << (MerkleHashTree::<MemStore>::min_num_bits(n - 1) - 1);
        let mut c: Vec<u8> = Vec::new();
        c.push(MerkleHashTree::<MemStore>::MHT_NODE_PREFIX);
        c.extend(mth(d[0..k].to_vec()));
        c.extend(mth(d[k..(n as usize)].to_vec()));
        return digest(Algorithm::SHA256, &c);
    }
    #[test]
    fn test_mth() {
        let mut d: Vec<Vec<u8>> = Vec::new();
        assert_eq!(digest(Algorithm::SHA256, b""), mth(d.to_vec()));
        for index in 0..=64 {
            let b: Vec<u8> = index.to_string().as_bytes().to_vec();
            d.push(b);
            assert_eq!(get_test_roots()[index as usize], mth(d.to_vec()));
        }
    }
    fn mpath(m: isize, d: Vec<Vec<u8>>) -> Option<Vec<Vec<u8>>> {
        /*
            note this is also a reference to test against
        */
        let n: isize = isize::try_from(d.len()).unwrap();
        if 0 > m || m >= n {
            return None;
        }
        if n == 1 && m == 0 {
            return Some(vec![]);
        }


        let k: isize = 1 << (MerkleHashTree::<MemStore>::min_num_bits(n - 1) - 1);
        let mut path: Vec<Vec<u8>> = Vec::new();
        let sub_path_option: Option<Vec<Vec<u8>>>;
        if m < k {
            sub_path_option = mpath(m, d[0..k as usize].to_vec());
            if sub_path_option != None {
                path.extend(sub_path_option.unwrap());
            }
            path.push(mth(d[k as usize .. n as usize].to_vec()));
        } else {
            sub_path_option = mpath(m-k, d[k as usize..n as usize].to_vec());
            if sub_path_option != None {
                path.extend(sub_path_option.unwrap());
            }
            path.push(mth(d[0..k as usize].to_vec()));
        }
        return Some(path);
    }
    #[test]
    fn test_mpath() {
        let mut d: Vec<Vec<u8>> = Vec::new();
        assert_eq!(None, mpath(0, d.to_vec()));
        for index in 0..=8 {
            let b: Vec<u8> = index.to_string().as_bytes().to_vec();
            d.push(b);
            assert_eq!(None, mpath(index + 1, d.to_vec())); // undefined path
            for i in 0..=index {
                let path: Vec<Vec<u8>> = mpath(i, d.to_vec()).unwrap();
                assert_eq!(get_test_paths()[index as usize][i as usize], path);
            }
        }
    }
    #[test]
    fn test_inclusion_proof() {
        let mut mht: MerkleHashTree<MemStore> = MerkleHashTree::new(MemStore::new());
        let mut d: Vec<Vec<u8>> = Vec::new();
        for index in 0..=64 {
            let v: Vec<u8> = index.to_string().as_bytes().to_vec();
            d.push(v.to_vec());
            mht.append(v);

            // test out of range
            assert_eq!(mht.inclusion_proof(index + 1, index), None);
            assert_eq!(mht.inclusion_proof(index, index + 1), None);

            for at in 0..=index {
                for i in 0..=at {
                    let path_option: Option<Vec<Vec<u8>>> = mht.inclusion_proof(at, i);
                    let expected_option: Option<Vec<Vec<u8>>> = mpath(i, d[0..(at+1) as usize].to_vec());
                    assert_eq!(path_option, expected_option);
                }
            }
        }
    }
    #[test]
    fn test_verify_inclusion() {
        let mut path: Vec<Vec<u8>> = Vec::new();
        assert_eq!(true, MerkleHashTree::<MemStore>::verify_inclusion(path.to_vec(), vec![], vec![], 0, 0));
        assert_eq!(false, MerkleHashTree::<MemStore>::verify_inclusion(path.to_vec(), vec![], vec![], 0, 1));
        assert_eq!(false, MerkleHashTree::<MemStore>::verify_inclusion(path.to_vec(), vec![], vec![], 1, 0));
        assert_eq!(false, MerkleHashTree::<MemStore>::verify_inclusion(path.to_vec(), vec![], vec![], 1, 1));

        let mut mht: MerkleHashTree<MemStore> = MerkleHashTree::new(MemStore::new());
        let mut d: Vec<Vec<u8>> = Vec::new();
        for index in 0..=64 {
            let v: Vec<u8> = index.to_string().as_bytes().to_vec();
            d.push(v.to_vec());
            mht.append(v);
            for at in 0..=index {
                for i in 0..=at {
                    path = mpath(i, d[0..(at+1) as usize].to_vec()).unwrap();
                    let is_verified: bool = MerkleHashTree::<MemStore>::verify_inclusion(
                        path.to_vec(),
                        get_test_roots()[at as usize].to_vec(),
                        mht.store.get(0, i).unwrap(),
                        at,
                        i
                    );
                    assert_eq!(is_verified, true);
                }
            }
        }
    }
    #[test] #[ignore]
    fn time_commit_and_verify() {
        let mut mht: MerkleHashTree<MemStore> = MerkleHashTree::new(MemStore::new());
        let roughly_one_billion: isize = (2 as isize).pow(30);
        for index in 0..=roughly_one_billion {
            let v: Vec<u8> = index.to_string().as_bytes().to_vec();
            // time perfect trees
            let log_base_2: f64 = (index as f64).log2();
            if log_base_2.fract() == 0.0 {
                // time commit
                let n1 = Instant::now();
                mht.append(v);
                println!("a, {}, {:?}", log_base_2, n1.elapsed());
                // time retrieve path
                let n2 = Instant::now();
                let path: Vec<Vec<u8>> = mht.inclusion_proof(index, index).unwrap();
                println!("b, {}, {:?}", log_base_2, n2.elapsed());
                // time verify path
                let n3 = Instant::now();
                let is_verified: bool = MerkleHashTree::<MemStore>::verify_inclusion(
                    path.to_vec(),
                    mht.root(),
                    mht.store.get(0, index).unwrap(),
                    index,
                    index
                );
                println!("c, {}, {:?}", log_base_2, n3.elapsed());
                assert_eq!(is_verified, true);
            } else {
                mht.append(v);
            }
        }
    }
}
