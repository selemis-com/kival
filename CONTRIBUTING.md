# Contributing Guide

Kival is developed and maintained by Selemis B.V.

## Code contributions

At this time, we do **not accept pull requests or other code contributions from external contributors**.

## Feature requests

If you would like to propose a new feature or change in behavior, please open an issue describing the proposal or add your support to an existing enhancement request. We prioritize new features based on community feedback and alignment with our roadmap.

## Bug reports

If you encounter a bug, please open a bug report or check whether an existing report already covers the issue.

If you would like to help investigate, we encourage you to share analysis, reproduction details, root-cause hypotheses, or a high-level outline of a potential fix directly in the issue thread.

## Security

If you believe you have found a security vulnerability, please do not report it through GitHub Issues. See our [Security Policy](SECURITY.md) for reporting instructions.
 
## Development setup

Kival requires Rust 1.97+, Node.js 26, pnpm 11, and Docker. PostgreSQL 18 runs locally through Docker Compose.

Clone the repository:

```sh
git clone https://github.com/selemis-com/kival.git
cd kival
```

Create the local environment and start PostgreSQL:

```sh
cp .env.template .env
docker compose up -d postgres
```

Install the frontend dependencies and Kival binaries:

```sh
pnpm install
make install
```

Start Kival:

```sh
kivald serve
```

In another terminal, bootstrap the first global administrator:

```sh
kivald admin bootstrap \
  --username admin \
  --display-name "Admin"
```

The command prints a one-time enrollment link. Open it in your browser to register a passkey and complete the initial setup.

Kival is then available at [`http://localhost:3000`](http://localhost:3000).

### Tests

Database-backed tests require `DATABASE_URL`. SQLx creates, migrates, and removes an isolated database for each test, so the configured PostgreSQL user must be able to create and drop databases.

Run the default test suite with:

```sh
make test
```

Use `make help` to see the other build, test, lint, documentation, and maintenance targets available to developers.
