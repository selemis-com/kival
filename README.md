<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/logo-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset=".github/assets/logo-light.svg">
  <img alt="Kival" src=".github/assets/logo-light.svg" width="100%" height="140px">
</picture>

<p align="center">
  Self-hosted knowledge system for organizations
</p>

<br/>

<p align="center">
  <a href="https://www.npmjs.com/package/kival-sdk"><picture><source media="(prefers-color-scheme: dark)" srcset="https://img.shields.io/npm/v/kival-sdk?colorA=21262d&colorB=21262d&style=flat"><img src="https://img.shields.io/npm/v/kival-sdk?colorA=f6f8fa&colorB=f6f8fa&style=flat" alt="Version"></picture></a>
  <a href="https://crates.io/crates/kival-sdk"><picture><source media="(prefers-color-scheme: dark)" srcset="https://img.shields.io/crates/v/kival-sdk?colorA=21262d&colorB=21262d&style=flat"><img src="https://img.shields.io/crates/v/kival-sdk?colorA=f6f8fa&colorB=f6f8fa&style=flat" alt="Version"></picture></a>
  <a href="#license"><picture><source media="(prefers-color-scheme: dark)" srcset="https://img.shields.io/npm/l/kival-sdk?colorA=21262d&colorB=21262d&style=flat"><img src="https://img.shields.io/npm/l/kival-sdk?colorA=f6f8fa&colorB=f6f8fa&style=flat" alt="MIT OR Apache-2.0"></picture></a>
</p>

<p align="center">
  <a href="#vision">Vision</a> ·
  <a href="#setup">Setup</a> ·
  <a href="#usage">Usage</a> ·
  <a href="#sdks">SDKs</a> ·
  <a href="#documentation">Documentation</a> ·
  <a href="#community">Community</a> ·
  <a href="#contributing">Contributing</a>
</p>

## Vision

Kival is a self-hosted knowledge system for organizations.

We preserve knowledge not only because we may need to retrieve it, but because accumulated material becomes part of the environment in which future thought happens.

For that to work, information cannot simply be kept. Its context has to survive with it: where it came from, what it relates to, how it changed, and why it mattered. Without that continuity, an organization may retain enormous amounts of information while repeatedly starting from the present.

Kival preserves knowledge as a connected body of work that can be returned to, questioned, revised, and built upon. Its history and relationships remain part of the record, so earlier work does not lose the context that made it meaningful.

Its purpose is to provide the soil in which ideas can take root, develop through sustained work, and eventually flower into new understanding and original work.

## Setup

Kival requires Rust 1.97+, Node.js 26, pnpm 11, and Docker. PostgreSQL 18 runs locally using Docker Compose.

Clone the repository:

```sh
git clone https://github.com/selemis-com/kival.git
cd kival
```

Create a local environment file:

```sh
cp .env.template .env
```

Start PostgreSQL and install the dependencies:

```sh
docker compose up -d postgres
pnpm install
make install
```

Start Kival:

```sh
kivald serve
```

Bootstrap the first administrator:

```sh
kivald admin bootstrap \
  --username admin \
  --display-name "Admin"
```

The command prints a one-time enrollment link. Open it in the browser to register a passkey and complete the initial setup.

Kival is now available at [`http://localhost:3000`](http://localhost:3000).

Local passkey use supports `localhost` on ports 3000 and 5173. WebAuthn RP IDs are domain names,
so IP-address origins such as `127.0.0.1` are not supported. To allow additional internal DNS
origins, provide exact comma-separated HTTPS origins (including any non-default port):

```sh
export KIVAL_ALLOWED_ORIGINS=https://kival.internal.example,https://kival.lan:8443
```

`KIVAL_CANONICAL_URL` remains the canonical origin used in generated enrollment links. WebAuthn
credentials are scoped to the hostname on which they were enrolled, so users need a separate
passkey when switching between unrelated hostnames.

For remote or managed PostgreSQL deployments, tune the server's total connection budget with
`KIVAL_DATABASE_MAX_CONNECTIONS` and how long work waits for a free connection with
`KIVAL_DATABASE_ACQUIRE_TIMEOUT_SECONDS`. The defaults are 8 connections and 5 seconds. Kival's
realtime listener normally occupies one pool connection, so `kivald serve` requires at least 2.

During shutdown, Kival stops accepting new HTTP connections and lets in-flight requests drain for up to `KIVAL_GRACEFUL_SHUTDOWN_TIMEOUT_SECONDS` seconds (30 by default).

To explore Kival with a populated fictional company workspace, create the built-in ACME demo:

```sh
kivald admin workspaces create --name "ACME" --demo acme
```

The demo is fully synthetic and includes engineering, hiring, people operations, marketing, growth, partnerships, launch planning, company operations, discussions, access boundaries, and shared agent skills. It is useful for exploring search, graph navigation, history, permissions, and agent workflows before adding your own knowledge.

To start from scratch instead, create a workspace in the web application.

To invite another user, create their account from the server:

```sh
kivald admin users create --username alice --display-name "Alice"
```

The command prints a one-time enrollment link. Send it to the user so they can register a passkey and complete their account setup.

You can then add the user to a workspace and manage their access from the web application.

Deployment operators can also disable or re-enable an account by username or user ID:

```sh
kivald admin users disable alice
kivald admin users enable alice
```

Disabling blocks access without revoking passkeys, sessions, API keys, memberships, or roles. Enabling restores access with those existing credentials and assignments. Credential recovery remains a separate `kivald admin recover USER` operation. Recovery preserves API keys by default; use `kivald admin recover USER --revoke-api-keys` to revoke every active API key as part of the same recovery transaction.

An authenticated global admin can perform the same lifecycle changes through the API-backed CLI using the user's ID:

```sh
kival admin users disable <USER_ID>
kival admin users enable <USER_ID>
```

Finally, create an API key for the workspace in the web application. The API key is used to authenticate the CLI and SDKs.

## Usage

The web application provides an interactive browser interface authenticated with a passkey-backed user session. The `kival` CLI and SDKs use scoped API keys for terminal workflows, integrations, and agents.

Authenticate the CLI with the API key created during setup:

```sh
export KIVAL_API_KEY=<API_KEY>
```

For a remote Kival instance, also set its URL:

```sh
export KIVAL_URL=https://kival.example
```

Check the current identity and available workspaces:

```sh
kival whoami
kival workspaces list
```

Create an object:

```sh
kival objects create <WORKSPACE_ID> --title "Database migration" --body-file migration.md
```

Edit it using your local editor:

```sh
kival objects edit <WORKSPACE_ID> <OBJECT_ID>
```

Kival opens a Markdown document with editable YAML front matter. The title and metadata mapping
appear above the body:

```md
---
title: "Database migration"
metadata:
  status: "proposed"
  area: "infrastructure"
  tags:
    - "postgres"
    - "migration"
---
```

A successful save commits all changed fields atomically as one new version. Unchanged sessions
create no version, and failed saves retain the local file for recovery.

Previous versions remain available and can be compared:

```sh
kival objects diff <WORKSPACE_ID> <OBJECT_ID> --from -1
```

Connect two objects:

```sh
kival objects edges create <WORKSPACE_ID> \
  --source-object-id <SOURCE_OBJECT_ID> \
  --target-object-id <TARGET_OBJECT_ID>
```

Follow the surrounding context:

```sh
kival objects backlinks <WORKSPACE_ID> <OBJECT_ID>
kival objects graph <WORKSPACE_ID> <OBJECT_ID>
```

Search across a workspace:

```sh
kival search <WORKSPACE_ID> "database migration"
```

Most commands support machine-readable output:

```sh
kival objects get <WORKSPACE_ID> <OBJECT_ID> -O json
```

The CLI can also describe its own commands and JSON Schema contracts:

```sh
kival schema
kival schema objects create
kival schema objects --full
```

## Development

Database-backed tests require `DATABASE_URL`; SQLx creates, migrates, and removes an isolated
database for each test. The configured PostgreSQL user must be able to create and drop databases.

```sh
make test
```

## SDKs

Kival provides official SDKs for Rust and TypeScript. Both use scoped API keys and expose the same underlying system as the CLI.

* [Rust SDK](https://crates.io/crates/kival-sdk)
* [TypeScript SDK](https://www.npmjs.com/package/kival-sdk)

The SDKs are intended for applications, integrations, automation, and agents that need programmatic access to Kival.

## Documentation

Documentation, guides, and technical resources are available at [selemis.com/resources](https://selemis.com/resources).

## Sponsors

Kival is developed and maintained by [Selemis](https://selemis.com).

Sponsorship supports the continued development and long-term maintenance of Kival. Organizations interested in sponsoring the project can contact [hello@selemis.com](mailto:hello@selemis.com).

## Community

Join the conversation in [GitHub Discussions](https://github.com/selemis-com/kival/discussions) to ask questions, share ideas, and discuss how Kival is being used.

## Contributing

See the [Contributing Guide](CONTRIBUTING.md) for information on reporting bugs, proposing features, and contributing to Kival.

At this time, we do **not accept pull requests or other code contributions from external contributors**.

## Security Policy

If you believe you have found a security vulnerability, please do not report it through GitHub Issues. See our [Security Policy](SECURITY.md) for reporting instructions.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

This software includes third-party components subject to separate license
terms. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in these crates by you, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
