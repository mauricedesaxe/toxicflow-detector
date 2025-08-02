use crate::sandwich::tokens::are_tokens_equivalent;
use crate::sandwich::transactions::SwapTransaction;

/// A rudimentary sandwich pattern detection function.
/// It assumes the transactions are in the correct order (front, victim, back).
///
/// Returning `true` doesn't mean it was a (profitable) sandwich attack,
/// but it means the swap directions are there.
///
/// TODO: An attacker could co-ordinate across multiple addresses to
/// obfuscate the attack. We could improve this by having a separate
/// module that tracks potentially related addresses and use it here
/// instead of a static `==` between `front.from_address` and `back.from_address`.
pub fn is_sandwich_pattern(
    front: &SwapTransaction,
    victim: &SwapTransaction,
    back: &SwapTransaction,
) -> bool {
    // Front-run and victim should be same pool
    if front.pool_address != victim.pool_address {
        return false;
    }

    // Should be same attacker
    if front.from_address != back.from_address {
        return false;
    }

    // Attacker should not be victim
    if front.from_address == victim.from_address {
        return false;
    }

    // Attacker should have gotten equivalent token back
    if !are_tokens_equivalent(&front.token_in, &back.token_out) {
        return false;
    }

    // Front and victim should be same token direction (attacker buys before victim)
    if !are_tokens_equivalent(&front.token_in, &victim.token_in)
        || !are_tokens_equivalent(&front.token_out, &victim.token_out)
    {
        return false;
    }

    // Victim and back should be different token direction (attacker sells back to victim)
    if are_tokens_equivalent(&victim.token_in, &back.token_in)
        && are_tokens_equivalent(&victim.token_out, &back.token_out)
    {
        return false;
    }

    return true;
}
