# Building pd

The node software `pd` is part of the same repository as `pcli`, so follow
[those instructions](../pcli/install.md) to clone the repo and install dependencies.

To build `pd`, run

```bash
cargo build --release --bin pd
```

Because you are building a work-in-progress version of the node software, you may see compilation warnings,
which you can safely ignore.

### Installing CometBFT

You'll need to have [CometBFT installed](https://docs.cometbft.com/v0.37/guides/install)
on your system to join your node to the testnet.

**NOTE**: Previous versions of Penumbra used Tendermint, but as of Testnet 62 (released 2023-10-10),
only CometBFT `v0.37.2` is supported. **Do not use** any version of Tendermint, which may not work with `pd`.

You can download `v0.37.2` [from the CometBFT releases page](https://github.com/cometbft/cometbft/releases/tag/v0.37.2)
to install a binary. If you prefer to compile from source instead,
make sure you are compiling version `v0.37.2`.
