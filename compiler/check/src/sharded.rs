use smallvec::SmallVec;
use std::borrow::Borrow;
use std::collections::hash_map::RawEntryMut;
use std::hash::{Hash, Hasher};
use std::mem;
use std::sync::{Mutex, MutexGuard};

#[derive(Clone, Default)]
struct CacheAligned<T>(T);

#[cfg(parallel_compiler)]
// 32 shards is sufficient to reduce contention on an 8-core Ryzen 7 1700,
// but this should be tested on higher core count CPUs. How the `Sharded` type gets used
// may also affect the ideal number of shards.
const SHARD_BITS: usize = 5;

#[cfg(not(parallel_compiler))]
const SHARD_BITS: usize = 0;

pub const SHARDS: usize = 1 << SHARD_BITS;

/// An array of cache-line aligned inner locked structures with convenience methods.
pub struct Sharded<T> {
    shards: [CacheAligned<Mutex<T>>; SHARDS],
}

impl<T: Default> Default for Sharded<T> {
    #[inline]
    fn default() -> Self {
        Self::new(T::default)
    }
}

#[allow(dead_code)]
impl<T> Sharded<T> {
    #[inline]
    pub fn new(mut value: impl FnMut() -> T) -> Self {
        // Create a vector of the values we want
        let mut values: SmallVec<[_; SHARDS]> = (0..SHARDS)
            .map(|_| CacheAligned(Mutex::new(value())))
            .collect();

        // Create an uninitialized array
        let mut shards: mem::MaybeUninit<[CacheAligned<Mutex<T>>; SHARDS]> =
            mem::MaybeUninit::uninit();

        unsafe {
            // Copy the values into our array
            let first = shards.as_mut_ptr() as *mut CacheAligned<Mutex<T>>;
            values.as_ptr().copy_to_nonoverlapping(first, SHARDS);

            // Ignore the content of the vector
            values.set_len(0);

            Sharded {
                shards: shards.assume_init(),
            }
        }
    }

    /// The shard is selected by hashing `val` with `FxHasher`.
    #[inline]
    pub fn get_shard_by_value<K: Hash + ?Sized>(&self, val: &K) -> &Mutex<T> {
        if SHARDS == 1 {
            &self.shards[0].0
        } else {
            self.get_shard_by_hash(make_hash(val))
        }
    }

    /// Get a shard with a pre-computed hash value. If `get_shard_by_value` is
    /// ever used in combination with `get_shard_by_hash` on a single `Sharded`
    /// instance, then `hash` must be computed with `FxHasher`. Otherwise,
    /// `hash` can be computed with any hasher, so long as that hasher is used
    /// consistently for each `Sharded` instance.
    #[inline]
    pub fn get_shard_index_by_hash(&self, hash: u64) -> usize {
        let hash_len = mem::size_of::<usize>();
        // Ignore the top 7 bits as hashbrown uses these and get the next SHARD_BITS highest bits.
        // hashbrown also uses the lowest bits, so we can't use those
        let bits = (hash >> (hash_len * 8 - 7 - SHARD_BITS)) as usize;
        bits % SHARDS
    }

    #[inline]
    pub fn get_shard_by_hash(&self, hash: u64) -> &Mutex<T> {
        &self.shards[self.get_shard_index_by_hash(hash)].0
    }

    #[inline]
    pub fn get_shard_by_index(&self, i: usize) -> &Mutex<T> {
        &self.shards[i].0
    }

    pub fn lock_shards(&self) -> Vec<MutexGuard<'_, T>> {
        (0..SHARDS)
            .map(|i| self.shards[i].0.lock().unwrap())
            .collect()
    }

    pub fn try_lock_shards(&self) -> Option<Vec<MutexGuard<'_, T>>> {
        (0..SHARDS)
            .map(|i| self.shards[i].0.try_lock().ok())
            .collect()
    }
}

pub type ShardedHashMap<K, V> = Sharded<fxhash::FxHashMap<K, V>>;

#[allow(dead_code)]
impl<K: Eq, V> ShardedHashMap<K, V> {
    pub fn len(&self) -> usize {
        self.lock_shards().iter().map(|shard| shard.len()).sum()
    }
}

impl<K: Eq + Hash + Copy> ShardedHashMap<K, ()> {
    #[inline]
    pub fn intern_ref<Q: ?Sized>(&self, value: &Q, make: impl FnOnce() -> K) -> K
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        let hash = make_hash(value);
        let mut shard = self.get_shard_by_hash(hash).lock().unwrap();
        let entry = shard.raw_entry_mut().from_key_hashed_nocheck(hash, value);

        match entry {
            RawEntryMut::Occupied(e) => *e.key(),
            RawEntryMut::Vacant(e) => {
                let v = make();
                e.insert_hashed_nocheck(hash, v, ());
                v
            }
        }
    }

    #[inline]
    pub fn intern<Q>(&self, value: Q, make: impl FnOnce(Q) -> K) -> K
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        let hash = make_hash(&value);
        let mut shard = self.get_shard_by_hash(hash).lock().unwrap();
        let entry = shard.raw_entry_mut().from_key_hashed_nocheck(hash, &value);

        match entry {
            RawEntryMut::Occupied(e) => *e.key(),
            RawEntryMut::Vacant(e) => {
                let v = make(value);
                e.insert_hashed_nocheck(hash, v, ());
                v
            }
        }
    }
}

#[inline]
fn make_hash<K: Hash + ?Sized>(val: &K) -> u64 {
    let mut state = fxhash::FxHasher::default();
    val.hash(&mut state);
    state.finish()
}