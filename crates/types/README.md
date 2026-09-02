# Kival Types

`kival-types` provides the canonical Rust vocabulary shared by Kival's kernel and public Rust SDK.

It defines stable concepts whose meaning is common across Kival's internal model and HTTP interface, including API-key scopes, authorization roles, lifecycle states, search modes, graph traversal, and event ordering. Transport-specific request and response models remain in [`kival-sdk`](https://crates.io/crates/kival-sdk).

Most applications interacting with Kival over HTTP should use `kival-sdk`, which re-exports the relevant types from this crate. Depend on `kival-types` directly when you need Kival's shared vocabulary without the SDK's complete wire model or HTTP client.

## Features

`kival-types` has no default features.

The core crate provides Kival's types without requiring Serde or Schemars:

```toml
[dependencies]
kival-types = "0.1"
```

Enable the `wire` feature to add `serde::Serialize`, `serde::Deserialize`, and `schemars::JsonSchema` implementations:

```toml
[dependencies]
kival-types = { version = "0.1", features = ["wire"] }
```

This is useful for servers, schema tooling, protocol implementations, and other crates that need Kival-compatible serialization without depending on the full SDK.

## Vocabulary

The crate includes shared types for:

* API-key capabilities through `ApiKeyScope`;
* object and membership authorization through `ObjectRole`, `MembershipRole`, and `GrantPrincipal`;
* resource lifecycle and collection filtering through `ArchiveStatus`, `ArchiveListStatus`, `UserStatus`, and `UserListStatus`;
* object collection ordering through `ObjectListOrder`;
* search behavior through `SearchCategory`, `SearchMode`, and `SearchMatchKind`;
* graph traversal through `ObjectGraphDirection`;
* commentary lifecycle through `CommentStatus`;
* event ordering through `EventOrder`.

Stored and serialized values are stable parts of Kival's contract. Types that expose `as_str` return their canonical representation, while parsing implementations accept those same representations where applicable.

For example:

```rust
use kival_types::{ApiKeyScope, ObjectRole};

assert_eq!(ApiKeyScope::ObjectRead.as_str(), "objects:read");
assert_eq!(ObjectRole::Editor.as_str(), "editor");

let role: ObjectRole = "admin".parse().expect("valid Kival object role");
assert_eq!(role, ObjectRole::Admin);
```

## API-key scopes

`ApiKeyScope` defines Kival's API-key capability vocabulary from a single canonical source.

Write scopes satisfy their corresponding read capability:

```rust
use kival_types::ApiKeyScope;

assert!(
    ApiKeyScope::ObjectWrite.permits(ApiKeyScope::ObjectRead)
);

assert!(
    !ApiKeyScope::ObjectRead.permits(ApiKeyScope::ObjectWrite)
);
```

Unrelated capabilities do not imply one another. Administrative authority, realtime access, event access, and resource capabilities remain independently scoped.

`ApiKeyScope::ALL` exposes every supported scope in stable declaration order.

## Using kival-sdk

The public Rust SDK re-exports the Kival types that form part of its HTTP interface, so SDK consumers generally do not need a separate `kival-types` dependency:

```rust
use kival_sdk::{ApiKeyScope, ObjectRole, SearchMode};
```

Use `kival-types` directly when sharing these concepts with code that should not depend on Kival's transport-specific request and response types or client implementation.

## Scope

`kival-types` intentionally contains only concepts whose semantics are shared across Kival boundaries.

HTTP request and response structures, pagination envelopes, API errors, realtime messages, and other transport-specific models belong to `kival-sdk`. Kernel-specific persistence and execution types remain internal to Kival.

Keeping this boundary small gives Kival one canonical definition for shared vocabulary without coupling the kernel to the public SDK's transport model.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this crate by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
