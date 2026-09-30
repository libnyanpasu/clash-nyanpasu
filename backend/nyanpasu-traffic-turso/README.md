# Experimental Turso traffic store

This independent workspace implements the `nyanpasu-traffic` storage port using
the local Turso engine. It is an evaluation adapter and is not a backend workspace
member or application dependency. The domain crate has no Turso feature.

Run adapter tests with:

```powershell
cargo test --manifest-path backend/nyanpasu-traffic-turso/Cargo.toml --locked
```

The SDK's Windows build needs a resource compiler (`rc.exe`) on `PATH`. Storage
tests reference the domain crate's shared contract fixture directly.
