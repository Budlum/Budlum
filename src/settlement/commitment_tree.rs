use crate::core::hash::hash_fields_bytes;
use crate::domain::types::Hash32;

pub fn merkle_root(leaves: &[Hash32]) -> Hash32 {
    if leaves.is_empty() {
        return hash_fields_bytes(&[b"BDLM_EMPTY_MERKLE_ROOT_V1"]);
    }

    let mut level = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity((level.len() + 1) / 2);
        for pair in level.chunks(2) {
            let left = pair[0];
            let right = if pair.len() == 2 { pair[1] } else { pair[0] };
            next.push(hash_fields_bytes(&[b"BDLM_MERKLE_NODE_V1", &left, &right]));
        }
        level = next;
    }

    level[0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_is_deterministic() {
        let a = hash_fields_bytes(&[b"a"]);
        let b = hash_fields_bytes(&[b"b"]);
        assert_eq!(merkle_root(&[a, b]), merkle_root(&[a, b]));
        assert_ne!(merkle_root(&[a, b]), merkle_root(&[b, a]));
    }
}
