pub mod asm;
pub mod gc;
pub mod heap;
pub mod runtime;
pub mod stubs;
pub fn compute_hash(key: u64) -> u32 {
    let mut hash: u32 = 0;
    hash = hash.wrapping_add(key.wrapping_shr(32) as u32);
    hash = hash.wrapping_add(hash.wrapping_shl(10));
    hash ^= hash.wrapping_shr(6);

    hash = hash.wrapping_add(key as u32 & 0xffffffff);
    hash = hash.wrapping_add(hash.wrapping_shl(10));
    hash ^= hash.wrapping_shr(6);
    hash = hash.wrapping_add(hash.wrapping_shl(3));
    hash = hash.wrapping_add(hash.wrapping_shr(11));
    hash = hash.wrapping_add(hash.wrapping_shl(15));
    hash
}
