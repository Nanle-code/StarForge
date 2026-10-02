# Gas Optimization Best Practices

A practical guide to writing cheaper, faster Soroban contracts, and to using
`starforge gas` to verify your changes actually help.

## Using the analyzer

```bash norun
# Profile a single contract
starforge gas analyze ./target/wasm32v1-none/release/my_contract.wasm

# Export a shareable HTML report
starforge gas analyze ./my_contract.wasm --format html --output report.html

# Export a machine-readable report for CI
starforge gas analyze ./my_contract.wasm --format json --output report.json

# Compare two builds (e.g. before/after a refactor)
starforge gas diff ./old.wasm ./new.wasm

# Benchmark every contract in a directory
starforge gas benchmark --dir ./target/wasm32v1-none/release
```

## What drives cost

The analyzer estimates cost from four signals pulled out of the compiled
wasm: binary size, host-call opcodes, control-flow opcodes, and memory
pages. In practice, these map to concrete habits:

**Binary size.** Every byte you ship is parsed and loaded. Strip debug
symbols and avoid pulling in large dependencies you only use a sliver of.
`cargo build --release` plus `starforge gas optimize` (which shells out to
`soroban-optimize` / `stellar contract optimize` when available) should be
your default release pipeline, not an afterthought.

**Host calls.** Every interaction with the Soroban host — storage
reads/writes, cross-contract calls, crypto operations — costs far more than
plain computation inside the wasm sandbox. Batch reads instead of calling
`get` in a loop; batch writes instead of writing the same key repeatedly
inside one invocation. If you find yourself calling a host function inside a
loop, that is almost always the first place to optimize.

**Storage shape.** Prefer fewer, larger storage entries over many small
ones where your access pattern allows it — each entry has fixed overhead.
Conversely, don't over-pack unrelated data into one key if it forces you to
read everything just to touch one field; that trades write cost for read
cost. Profile both directions if you're unsure.

**Control flow.** Deep branching and large match/loop structures inflate
both code size and CPU instruction estimates. This is rarely worth
micro-optimizing on its own, but it's a useful signal that a function has
grown too complex and might benefit from being split or simplified anyway.

**Logging and panics.** Debug `println!`/`panic!` strings get compiled into
the binary and inflate size for no runtime benefit in production builds.
Strip them, or gate them behind a debug feature flag.

## A practical workflow

1. Write the contract normally, correctness first.
2. Run `starforge gas analyze` and read the suggestions — they're
   heuristic, not gospel, but they flag the cheap wins.
3. Make the change, then run `starforge gas diff old.wasm new.wasm` to
   confirm the estimated fee actually went down. Don't optimize blind —
   verify.
4. Before merging, run `starforge gas benchmark --dir <build-output>` over
   your whole contract suite so a regression in one function doesn't slip
   through unnoticed while you were focused on another.
5. Treat a `regression` flag (>5% fee increase) on `gas diff` the same way
   you'd treat a failing test in CI — investigate before merging, don't
   wave it through.

## Caveats

These are **heuristic, static estimates** derived from the compiled wasm,
not a live Soroban RPC simulation. They're useful for fast local iteration
and catching regressions early, but before deploying to mainnet, always
cross-check the actual resource/fee numbers from `stellar contract
simulate` or `soroban contract invoke --send=no` against what the analyzer
predicted. Treat `starforge gas` as a fast first-pass filter, not the final
word on what you'll pay on-chain.