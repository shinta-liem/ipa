use std::{
    fmt::{Debug, Display, Formatter},
    num::TryFromIntError,
    ops::{Index, IndexMut},
};

use ipa_metrics::LabelValue;

use crate::helpers::{HelperIdentity, TransportIdentity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShardedHelperIdentity {
    pub helper_identity: HelperIdentity,
    pub shard_index: ShardIndex,
}

impl ShardedHelperIdentity {
    pub const ONE_FIRST: ShardedHelperIdentity = ShardedHelperIdentity {
        helper_identity: HelperIdentity::ONE,
        shard_index: ShardIndex::FIRST,
    };

    #[must_use]
    pub fn new(helper_identity: HelperIdentity, shard_index: ShardIndex) -> Self {
        Self {
            helper_identity,
            shard_index,
        }
    }

    #[must_use]
    pub fn as_index(&self) -> usize {
        self.shard_index.as_index() * 3 + self.helper_identity.as_index()
    }
}

/// A unique zero-based index of the helper shard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShardIndex(pub u32);

impl ShardIndex {
    pub const FIRST: Self = Self(0);

    /// Returns an iterator over all shard indices that precede this one, excluding this one.
    pub fn iter(self) -> impl Iterator<Item = Self> {
        (0..self.0).map(Self)
    }
}

impl From<u32> for ShardIndex {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<ShardIndex> for u64 {
    fn from(value: ShardIndex) -> Self {
        u64::from(value.0)
    }
}

impl From<ShardIndex> for u128 {
    fn from(value: ShardIndex) -> Self {
        Self::from(value.0)
    }
}

#[cfg(target_pointer_width = "64")]
impl From<ShardIndex> for usize {
    fn from(value: ShardIndex) -> Self {
        usize::try_from(value.0).unwrap()
    }
}

impl From<ShardIndex> for u32 {
    fn from(value: ShardIndex) -> Self {
        value.0
    }
}

impl TryFrom<usize> for ShardIndex {
    type Error = TryFromIntError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        u32::try_from(value).map(Self)
    }
}

impl TryFrom<u128> for ShardIndex {
    type Error = TryFromIntError;

    fn try_from(value: u128) -> Result<Self, Self::Error> {
        u32::try_from(value).map(Self)
    }
}

impl Display for ShardIndex {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl<T> Index<ShardIndex> for Vec<T> {
    type Output = T;

    fn index(&self, index: ShardIndex) -> &Self::Output {
        self.as_slice().index(usize::from(index))
    }
}

impl<T> IndexMut<ShardIndex> for Vec<T> {
    fn index_mut(&mut self, index: ShardIndex) -> &mut Self::Output {
        self.as_mut_slice().index_mut(usize::from(index))
    }
}

impl LabelValue for ShardIndex {
    fn hash(&self) -> u64 {
        u64::from(self.0)
    }

    fn boxed(&self) -> Box<dyn LabelValue> {
        Box::new(*self)
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Sharded {
    pub shard_id: ShardIndex,
    pub shard_count: ShardIndex,
}

impl ShardConfiguration for Sharded {
    fn shard_id(&self) -> ShardIndex {
        self.shard_id
    }

    fn shard_count(&self) -> ShardIndex {
        self.shard_count
    }
}

/// Shard-specific configuration required by sharding API. Each shard must know its own index and
/// the total number of shards in the system.
pub trait ShardConfiguration {
    /// Returns the index of the current shard.
    fn shard_id(&self) -> ShardIndex;

    /// Total number of shards present on this helper. It is expected that all helpers have the
    /// same number of shards.
    fn shard_count(&self) -> ShardIndex;

    /// Returns an iterator that yields shard indices for all shards present in the system, except
    /// this one. Shards are yielded in ascending order.
    ///
    /// ## Panics
    /// if current shard index is greater or equal to the total number of shards.
    fn peer_shards(&self) -> impl Iterator<Item = ShardIndex> {
        let this = self.shard_id();
        let max = self.shard_count();
        assert!(
            this < max,
            "Current shard index '{this}' >= '{max}' (total number of shards)"
        );

        max.iter().filter(move |&v| v != this)
    }
}

/// This is a runtime version of `ShardBinding`. It is used by the stream interceptor to
/// avoid type parameter proliferation. It should not be used by protocols.
pub type ShardContext = Option<ShardIndex>;

pub trait ShardBinding: Debug + Send + Sync + Clone + 'static {
    fn context(&self) -> ShardContext;
}

#[derive(Debug, Copy, Clone)]
pub struct NotSharded;

impl ShardBinding for NotSharded {
    fn context(&self) -> ShardContext {
        None
    }
}

impl ShardBinding for Sharded {
    fn context(&self) -> ShardContext {
        Some(self.shard_id)
    }
}

#[cfg(all(test, unit_test))]
mod tests {
    use std::iter::empty;

    use crate::sharding::ShardIndex;

    fn shards<I: IntoIterator<Item = u32>>(input: I) -> impl Iterator<Item = ShardIndex> {
        input.into_iter().map(ShardIndex)
    }

    #[test]
    fn iter() {
        assert!(ShardIndex::FIRST.iter().eq(empty()));
        assert!(shards([0, 1, 2]).eq(ShardIndex::from(3).iter()));
    }

    /// It is often useful to keep a collection of elements indexed by shard.
    #[test]
    fn indexing() {
        let arr = [0, 1, 2];
        assert_eq!(0, arr[usize::from(ShardIndex::FIRST)]);
    }

    mod conf {
        use crate::sharding::{tests::shards, ShardConfiguration, ShardIndex};

        struct StaticConfig(u32, u32);
        impl ShardConfiguration for StaticConfig {
            fn shard_id(&self) -> ShardIndex {
                self.0.into()
            }

            fn shard_count(&self) -> ShardIndex {
                self.1.into()
            }
        }

        #[test]
        fn excludes_this_shard() {
            assert!(shards([0, 1, 2, 4]).eq(StaticConfig(3, 5).peer_shards()));
        }

        #[test]
        #[should_panic(expected = "Current shard index '5' >= '5' (total number of shards)")]
        fn shard_index_eq_shard_count() {
            let _ = StaticConfig(5, 5).peer_shards();
        }

        #[test]
        #[should_panic(expected = "Current shard index '7' >= '5' (total number of shards)")]
        fn shard_index_gt_shard_count() {
            let _ = StaticConfig(7, 5).peer_shards();
        }
    }
}
