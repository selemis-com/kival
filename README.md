<picture>
  <source media="(prefers-color-scheme: dark)" srcset=".github/assets/logo-dark.svg">
  <source media="(prefers-color-scheme: light)" srcset=".github/assets/logo-light.svg">
  <img alt="Kival" src=".github/assets/logo-light.svg" width="100%" height="140px">
</picture>

<p align="center">
  Self-hosted collaborative knowledge system for organizations
</p>

<br/>

<p align="center">
  <a href="https://www.npmjs.com/package/kival-sdk"><picture><source media="(prefers-color-scheme: dark)" srcset="https://img.shields.io/npm/v/kival-sdk?colorA=21262d&colorB=21262d&style=flat"><img src="https://img.shields.io/npm/v/kival-sdk?colorA=f6f8fa&colorB=f6f8fa&style=flat" alt="Version"></picture></a>
  <a href="https://crates.io/crates/kival-sdk"><picture><source media="(prefers-color-scheme: dark)" srcset="https://img.shields.io/crates/v/kival-sdk?colorA=21262d&colorB=21262d&style=flat"><img src="https://img.shields.io/crates/v/kival-sdk?colorA=f6f8fa&colorB=f6f8fa&style=flat" alt="Version"></picture></a>
  <a href="#license"><picture><source media="(prefers-color-scheme: dark)" srcset="https://img.shields.io/npm/l/kival-sdk?colorA=21262d&colorB=21262d&style=flat"><img src="https://img.shields.io/npm/l/kival-sdk?colorA=f6f8fa&colorB=f6f8fa&style=flat" alt="MIT OR Apache-2.0"></picture></a>
</p>

<p align="center">
  <a href="#overview">Overview</a> ·
  <a href="#setup">Setup</a> ·
  <a href="#resources">Resources</a> ·
  <a href="#sdks">SDKs</a> ·
  <a href="#community">Community</a> ·
  <a href="#contributing">Contributing</a>
</p>

## Overview

Kival is Selemis's self-hosted collaborative knowledge system for organizations.

Organizations constantly produce messages, documents, decisions, and records, yet the understanding that connects them is easily lost. Sources become detached from conclusions, decisions outlive their reasoning, and knowledge fragments across tools or leaves with the people who carried it.

Kival gives people and agents a shared place to deliberately create, edit, discuss, connect, version, and govern knowledge. History, relationships, discussions, authorship, and provenance remain connected as that knowledge develops, preserving the context needed to understand how something came to be and build on it over time.

* **Shared knowledge**: people and agents work against the same organizational knowledge rather than maintaining separate copies of context.
* **Continuity**: history, relationships, discussions, and provenance remain connected as knowledge develops, preserving how it came to be rather than only its latest state.
* **Deliberate authorship**: knowledge is explicitly created and maintained as organizational work, with clear authorship, access, and governance.
* **Durable context**: knowledge remains understandable even as the people, agents, models, applications, and tools around it change.
* **Self-hosted ownership**: the organization retains control over the knowledge and infrastructure on which its work depends.

## Setup

### Supported platforms

Prebuilt Kival releases are available for:

* Linux x86_64
* Linux ARM64
* macOS on Apple Silicon
* Windows through WSL

Native Windows and Intel Mac releases are not currently provided. Source builds on other platforms are outside the supported release matrix.

### Install a release

Install the version-bound `kivalup` installer from the latest stable GitHub release:

```sh
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/selemis-com/kival/releases/latest/download/install | bash

$HOME/.kival/bin/kivalup
```

The bootstrap installer verifies the downloaded `kivalup` checksum and, when the GitHub CLI is available, its build provenance.

`kivalup` installs the matching `kival` and `kivald` binaries under `$HOME/.kival/bin`.

To update later:

```sh
kivalup --update
```

### Quick start

Start PostgreSQL in the background with persistent local storage:

```sh
docker run -d \
  --name kival-postgres \
  -e POSTGRES_USER=kival \
  -e POSTGRES_PASSWORD=kival \
  -e POSTGRES_DB=kival \
  -p 5432:5432 \
  -v kival-postgres-data:/var/lib/postgresql \
  postgres:18
```

Point Kival at the database and start the server:

```sh
export DATABASE_URL=postgres://kival:kival@localhost:5432/kival
kivald serve
```

In another terminal, bootstrap the first global administrator:

```sh
export DATABASE_URL=postgres://kival:kival@localhost:5432/kival

kivald admin bootstrap \
  --username admin \
  --display-name "Admin"
```

The command prints a one-time enrollment link. Open it in your browser to register a passkey and complete the initial administrator setup.

Kival is now available at [`http://localhost:3000`](http://localhost:3000).

### Create a workspace

Create a workspace from the web application, or explore Kival using the built-in fictional ACME workspace:

```sh
kivald admin workspaces create --name "ACME" --demo acme
```

The demo includes connected documents, discussions, access boundaries, history, and shared agent skills for exploring Kival before adding your own knowledge.

### Configure CLI access

From the web application, [create an API key](http://localhost:3000/settings/api-keys) for the administrator and allow it access to the workspace.

Then configure the local CLI:

```sh
export KIVAL_API_KEY=<API_KEY>
```

For a remote Kival instance, also set its URL:

```sh
export KIVAL_URL=https://kival.example
```

Verify the CLI is authenticated and can access the workspace:

```sh
kival whoami
kival workspaces list
```

### Explore with an agent

Agents can use the same CLI and access model to inspect existing knowledge, follow relationships, and contribute new material.

**Trace a decision and its supporting context**

> Use the Kival CLI to explore the ACME workspace. Find an important decision, explain what was decided and why, trace the supporting knowledge that led to it, and include links to the relevant Kival objects.

**Synthesize project state into new knowledge**

> Use the Kival CLI to inspect Project Relay and RFC 024. Create a new object titled "Project Relay rollout review" summarizing the current rollout state, remaining risks, and next decision point, then link it to the relevant existing Kival objects. Show me what you created and include links to the objects you used.

**Turn operational history into follow-up knowledge**

> Use the Kival CLI to find a recent ACME incident and its related runbook. Create a short follow-up object with the key operational lesson and recommended next action, link it to both the incident and runbook, and include links to the resulting Kival objects.

## Resources

The [Kival resources](https://selemis.com/resources) contain the complete guides and reference material.

### Users

* [Getting started](https://selemis.com/resources/docs/users/getting-started)
* [Objects and content](https://selemis.com/resources/docs/users/objects-and-content)
* [Relations and context](https://selemis.com/resources/docs/users/relations-and-context)
* [Access and collaboration](https://selemis.com/resources/docs/users/access-and-collaboration)
* [Account and security](https://selemis.com/resources/docs/users/account-and-security)

### Developers

* [Getting started](https://selemis.com/resources/docs/developers/getting-started)
* [Core concepts](https://selemis.com/resources/docs/developers/core-concepts)
* [SDKs](https://selemis.com/resources/docs/developers/sdks)
* [Integrations](https://selemis.com/resources/docs/developers/integrations)
* [API reference](https://selemis.com/resources/docs/developers/api-reference)
* [CLI automation](https://selemis.com/resources/docs/developers/cli-automation)

### Administrators

* [Deployment](https://selemis.com/resources/docs/administrators/deployment)
* [Administration](https://selemis.com/resources/docs/administrators/administration)

## SDKs

Kival provides official SDKs for Rust and TypeScript:

* [Rust SDK](https://crates.io/crates/kival-sdk)
* [TypeScript SDK](https://www.npmjs.com/package/kival-sdk)

Both use scoped API keys and expose the same underlying system as the CLI.

They are intended for applications, integrations, automation, and agents that need programmatic access to Kival.

## Sponsors

Kival is developed and maintained by [Selemis](https://selemis.com).

Sponsorship supports the continued development and long-term maintenance of Kival. Organizations interested in sponsoring the project can contact [hello@selemis.com](mailto:hello@selemis.com).

## Community

Join the conversation in [GitHub Discussions](https://github.com/selemis-com/kival/discussions) to ask questions, share ideas, and discuss how Kival is being used.

## Contributing

See the [Contributing Guide](CONTRIBUTING.md) for information on reporting bugs, proposing features, and contributing to Kival.

At this time, we do **not accept pull requests or other code contributions from external contributors**.

## Security Policy

If you believe you have found a security vulnerability, please do not report it through GitHub Issues.

See the [Security Policy](SECURITY.md) for reporting instructions.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

This software includes third-party components subject to separate license terms. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
