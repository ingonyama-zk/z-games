# Z-games

### [Z-games](https://www.ingonyama.com/zgames-by-ingonyama) wrapper code

The wrapper generates all data required for the prover to run, and measures hashrate for a long period of time. This is in order to average the rate, as in real-life scenarios

Add the repo to your miner snarkVM directory, and from [main.rs](../main/src/main.rs) call the coinbase_puzzle.prove() function

```
let result = prover_coinbase_puzzle
                    .prove(&prover_epoch_challenge, address, rng.gen(), Some(coinbase_target))
                    .ok()
                    .and_then(|solution| solution.to_target().ok().map(|solution_target| (solution_target, solution)));
```

Run the wrapper with basic `cargo run --release` and submit results to z_games@ingonyama.com
