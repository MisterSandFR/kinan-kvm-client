# Contributing

Thanks for considering a contribution!

## Development loop

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p kinan-sim &        # starts the simulator on 127.0.0.1
cargo run -p kinan-kvm-client   # open a profile pointing at the simulator
```

## Protocol changes

The protocol crate (`crates/kinan-protocol`) is pure and unit-tested.
Every new packet type belongs in a dedicated module with its own tests.

## Translations

String tables live in `crates/kinan-kvm-client/assets/i18n/`. Each
language is one flat JSON file keyed by the same identifiers as the
English reference (`en.json`). Add a new language by creating
`<tag>.json`, translating every key, and registering the file in
`src/i18n.rs::SUPPORTED`.

## Commits

Short imperative messages, lower-case, under 50 characters where
possible. Keep logs and screenshots out of the repository unless they
come from the simulator.

## License

By contributing, you agree to license your contribution under MIT OR
Apache-2.0.
