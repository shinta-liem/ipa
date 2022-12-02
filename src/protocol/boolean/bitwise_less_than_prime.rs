use super::any_ones;
use super::or::or;
use crate::error::Error;
use crate::ff::Field;
use crate::protocol::boolean::check_if_all_ones;
use crate::protocol::context::SemiHonestContext;
use crate::protocol::{context::Context, mul::SecureMul, BitOpStep, RecordId};
use crate::secret_sharing::Replicated;
use futures::future::try_join;
use std::cmp::Ordering;

/// This is an implementation of Bitwise Less-Than on bitwise-shared numbers.
///
/// `BitwiseLessThan` takes inputs `[x]_B = ([x_1]_p,...,[x_l]_p)` where
/// `x_1,...,x_l ∈ {0,1} ⊆ F_p` then computes `h ∈ {0, 1} <- x <? p` where
/// `h = 1` iff `x` is less than `p`.
///
/// Note that `[a]_B` can be converted to `[a]_p` by `Σ (2^i * a_i), i=0..l`. In
/// other words, if comparing two integers, the protocol expects inputs to be in
/// the little-endian; the least-significant byte at the smallest address (0'th
/// element).
///
pub struct BitwiseLessThanPrime {}

impl BitwiseLessThanPrime {
    pub async fn less_than_prime<F: Field>(
        ctx: SemiHonestContext<'_, F>,
        record_id: RecordId,
        x: &[Replicated<F>],
    ) -> Result<Replicated<F>, Error> {
        let one = ctx.share_of_one();
        let gtoe = Self::greater_than_or_equal_to_prime(ctx, record_id, x).await?;
        Ok(one - &gtoe)
    }

    pub async fn greater_than_or_equal_to_prime<F: Field>(
        ctx: SemiHonestContext<'_, F>,
        record_id: RecordId,
        x: &[Replicated<F>],
    ) -> Result<Replicated<F>, Error> {
        let prime = F::PRIME.into();
        let l = u128::BITS - prime.leading_zeros();
        let l_as_usize = l.try_into().unwrap();
        match x.len().cmp(&l_as_usize) {
            Ordering::Greater => {
                let (leading_ones, normal_check) = try_join(
                    any_ones(
                        ctx.narrow(&Step::CheckIfAnyOnes),
                        record_id,
                        &x[l_as_usize..],
                    ),
                    Self::greater_than_or_equal_to_prime_trimmed(
                        ctx.narrow(&Step::CheckTrimmed),
                        record_id,
                        &x[0..l_as_usize],
                    ),
                )
                .await?;
                or(
                    ctx.narrow(&Step::LeadingOnesOrRest),
                    record_id,
                    &leading_ones,
                    &normal_check,
                )
                .await
            }
            Ordering::Equal => {
                Self::greater_than_or_equal_to_prime_trimmed(
                    ctx.narrow(&Step::CheckTrimmed),
                    record_id,
                    x,
                )
                .await
            }
            Ordering::Less => {
                panic!();
            }
        }
    }

    async fn greater_than_or_equal_to_prime_trimmed<F: Field>(
        ctx: SemiHonestContext<'_, F>,
        record_id: RecordId,
        x: &[Replicated<F>],
    ) -> Result<Replicated<F>, Error> {
        let prime = F::PRIME.into();
        let l = u128::BITS - prime.leading_zeros();
        let l_as_usize: usize = l.try_into().unwrap();
        debug_assert!(x.len() == l_as_usize);

        // Check if this is a Mersenne Prime
        // In that special case, the only way for `x >= p` is if `x == p`,
        // meaning all the bits of `x` are shares of one.
        if prime == (1 << l) - 1 {
            return check_if_all_ones(ctx.narrow(&Step::CheckIfAllOnes), record_id, x).await;
        }

        // Assume this is an Fp32BitPrime
        // Meaning the least significant three bits are exactly [1, 1, 0]
        if prime == (1 << l) - 5 {
            let (check_least_significant_bits, most_significant_bits_all_ones) = try_join(
                Self::check_least_significant_bits(
                    ctx.narrow(&Step::CheckLeastSignificantBits),
                    record_id,
                    &x[0..3],
                ),
                check_if_all_ones(ctx.narrow(&Step::CheckIfAllOnes), record_id, &x[3..]),
            )
            .await?;
            return ctx
                .narrow(&Step::AllOnesAndFinalBits)
                .multiply(
                    record_id,
                    &check_least_significant_bits,
                    &most_significant_bits_all_ones,
                )
                .await;
        }
        // Not implemented for any other type of prime. Please add to this if you create a new type of Field which
        // is neither a Mersenne Prime, nor which is equal to `2^n - 5` for some value of `n`
        panic!();
    }

    /// This is a *special case* implementation which assumes the prime is all ones except for the least significant bits which are: `[1 1 0]` (little-endian)
    /// This is the case for `Fp32BitPrime`.
    ///
    /// Assuming that all the more significant bits of the value being checked are all shares of one, Just consider the least significant three bits:
    /// Assume those bits are [1 1 0] (little-endian)
    /// There are only 5 numbers that are greater than or equal to the prime
    /// 1.) Four of them look like [X X 1] (values of X are irrelevant)
    /// 2.) The final one is exactly [1 1 0]
    /// We can check if either of these conditions is true with just 3 multiplications
    pub async fn check_least_significant_bits<F: Field>(
        ctx: SemiHonestContext<'_, F>,
        record_id: RecordId,
        x: &[Replicated<F>],
    ) -> Result<Replicated<F>, Error> {
        let prime = F::PRIME.into();
        debug_assert!(prime & 0b111 == 0b011);
        debug_assert!(x.len() == 3);
        let least_significant_two_bits_both_one = ctx
            .narrow(&BitOpStep::from(0))
            .multiply(record_id, &x[0], &x[1])
            .await?;
        let pivot_bit = &x[2];
        let least_significant_three_bits_all_equal_to_prime = ctx
            .narrow(&BitOpStep::from(1))
            .multiply(
                record_id,
                &least_significant_two_bits_both_one,
                &(ctx.share_of_one() - pivot_bit),
            )
            .await?;
        or(
            ctx.narrow(&BitOpStep::from(2)),
            record_id,
            pivot_bit,
            &least_significant_three_bits_all_equal_to_prime,
        )
        .await
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Step {
    CheckTrimmed,
    CheckIfAnyOnes,
    LeadingOnesOrRest,
    CheckIfAllOnes,
    CheckLeastSignificantBits,
    AllOnesAndFinalBits,
}

impl crate::protocol::Substep for Step {}

impl AsRef<str> for Step {
    fn as_ref(&self) -> &str {
        match self {
            Self::CheckTrimmed => "check_trimmed",
            Self::CheckIfAnyOnes => "check_if_any_ones",
            Self::LeadingOnesOrRest => "leading_ones_or_rest",
            Self::CheckIfAllOnes => "check_if_all_ones",
            Self::CheckLeastSignificantBits => "check_least_significant_bits",
            Self::AllOnesAndFinalBits => "final_step",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BitwiseLessThanPrime;
    use crate::test_fixture::Runner;
    use crate::{
        ff::{Field, Fp31, Fp32BitPrime},
        protocol::{QueryId, RecordId},
        test_fixture::{get_bits, Reconstruct, TestWorld},
    };
    use rand::{distributions::Standard, prelude::Distribution};

    #[tokio::test]
    pub async fn fp31() {
        let zero = Fp31::ZERO;
        let one = Fp31::ONE;

        assert_eq!(one, bitwise_less_than_prime::<Fp31>(30, 5).await);
        assert_eq!(one, bitwise_less_than_prime::<Fp31>(30, 6).await);
        assert_eq!(zero, bitwise_less_than_prime::<Fp31>(31, 5).await);
        assert_eq!(zero, bitwise_less_than_prime::<Fp31>(32, 6).await);
        assert_eq!(zero, bitwise_less_than_prime::<Fp31>(64, 7).await);
        assert_eq!(zero, bitwise_less_than_prime::<Fp31>(64, 8).await);
        assert_eq!(zero, bitwise_less_than_prime::<Fp31>(128, 8).await);
        assert_eq!(zero, bitwise_less_than_prime::<Fp31>(224, 8).await);
        assert_eq!(one, bitwise_less_than_prime::<Fp31>(29, 5).await);
        assert_eq!(one, bitwise_less_than_prime::<Fp31>(0, 5).await);
        assert_eq!(one, bitwise_less_than_prime::<Fp31>(1, 5).await);
        assert_eq!(one, bitwise_less_than_prime::<Fp31>(3, 5).await);
        assert_eq!(one, bitwise_less_than_prime::<Fp31>(15, 5).await);
    }

    #[tokio::test]
    pub async fn fp32_bit_prime() {
        let zero = Fp32BitPrime::ZERO;
        let one = Fp32BitPrime::ONE;

        assert_eq!(
            zero,
            bitwise_less_than_prime::<Fp32BitPrime>(Fp32BitPrime::PRIME, 32).await
        );
        assert_eq!(
            zero,
            bitwise_less_than_prime::<Fp32BitPrime>(Fp32BitPrime::PRIME + 1, 32).await
        );
        assert_eq!(
            zero,
            bitwise_less_than_prime::<Fp32BitPrime>(Fp32BitPrime::PRIME + 2, 32).await
        );
        assert_eq!(
            zero,
            bitwise_less_than_prime::<Fp32BitPrime>(Fp32BitPrime::PRIME + 3, 32).await
        );
        assert_eq!(
            zero,
            bitwise_less_than_prime::<Fp32BitPrime>(Fp32BitPrime::PRIME + 4, 32).await
        );
        assert_eq!(
            one,
            bitwise_less_than_prime::<Fp32BitPrime>(Fp32BitPrime::PRIME - 1, 32).await
        );
        assert_eq!(
            one,
            bitwise_less_than_prime::<Fp32BitPrime>(Fp32BitPrime::PRIME - 2, 32).await
        );
        assert_eq!(
            one,
            bitwise_less_than_prime::<Fp32BitPrime>(Fp32BitPrime::PRIME - 3, 32).await
        );
        assert_eq!(
            one,
            bitwise_less_than_prime::<Fp32BitPrime>(Fp32BitPrime::PRIME - 4, 32).await
        );
        assert_eq!(one, bitwise_less_than_prime::<Fp32BitPrime>(0, 32).await);
        assert_eq!(one, bitwise_less_than_prime::<Fp32BitPrime>(1, 32).await);
        assert_eq!(
            one,
            bitwise_less_than_prime::<Fp32BitPrime>(65_536_u32, 32).await
        );
        assert_eq!(
            one,
            bitwise_less_than_prime::<Fp32BitPrime>(65_535_u32, 32).await
        );
    }

    async fn bitwise_less_than_prime<F: Field>(a: u32, num_bits: u32) -> F
    where
        F: Sized,
        Standard: Distribution<F>,
    {
        let world = TestWorld::new(QueryId);
        let bits = get_bits::<F>(a, num_bits);
        let result = world
            .semi_honest(bits, |ctx, x_share| async move {
                BitwiseLessThanPrime::less_than_prime(ctx, RecordId::from(0), &x_share)
                    .await
                    .unwrap()
            })
            .await;

        result.reconstruct()
    }
}