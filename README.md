# Toxic flow detector

Toxic flow = MEV, bots, wash trading, etc.

This is a toxic flow detector so the idea is it looks through the blockchain and identifies transactions and/or wallets that are considered toxic.

## Version 1 planning

We will use sample/fake data so we make our lives easier, not having to index it.

The important part of the version 1 is defining a toxic flow algorithm.

A few simple heuristics we can start from:
- wallet A has 2 swaps in the same block around 1 swap from wallet B = maybe sandwich? could then look into gas prices, slippage, price impact, etc.
- bought within first N blocks of token launch = maybe snipe bot? could then look into gas prices, % of initial supply bought, if there were other similar tx within the timeframe, if it was from a smart contract instead of a wallet
- same token pair traded on multiple exchanges in the same block / short timeframe = maybe arbitrage bot? could then look if price difference between exchanges is bigger than a certain threshold
- bought and sold immediately after each other = maybe wash trading? could then look if there was any meaningful price difference

## Interesting ideas for version 2 (no hard plan)

### Cluster analysis

1. flag a wallet as having toxic flow
2. look at all wallets that have interacted with flagged wallet as well (maybe some are seeders and can lead us to other toxic wallets)

### Use real transactions
