# spel-token-guest

The deployable **guest-binary** form of the `spel-token` example: the same program
logic, packaged as a RISC Zero guest (`#![no_main]` + `risc0_zkvm::guest::entry!`)
so it compiles to the on-chain `riscv32im-risc0-zkvm-elf` target and deploys to LEZ.

This is a **detached crate** (its own `[workspace]`, excluded from the repo
workspace) because it builds for the zkVM target with `nssa_core` in guest mode (no
`host` feature), which is why `mint-authority` makes its `host` feature optional.

## Building the guest ELF (no Docker)

```bash
cargo +risc0 build --release --target riscv32im-risc0-zkvm-elf
# -> target/riscv32im-risc0-zkvm-elf/release/spel_token  (RISC-V ELF)
```

The flags in [`.cargo/config.toml`](.cargo/config.toml) mirror what `risc0-build`
injects (lower-atomic pass, load address, `panic=abort`, the custom `getrandom`
backend). The prebuilt ELF is checked into [`bin/spel_token.elf`](bin/spel_token.elf).

## Producing the LEZ-deployable binary + deploying

LEZ loads a program via `risc0_binfmt::ProgramBinary::decode`, i.e. the user ELF
wrapped with the risc0 kernel and encoded. That packaging is what `cargo risczero
build` produces, and it runs in Docker for reproducibility:

```bash
cargo risczero build --manifest-path Cargo.toml          # needs Docker (e.g. colima)
spel program-id target/riscv32im-risc0-zkvm-elf/docker/spel_token   # image id
wallet deploy-program target/.../spel_token              # needs a running sequencer
```

The full deploy + end-to-end run is automated by [`../../../scripts/demo.sh`](../../../scripts/demo.sh).
