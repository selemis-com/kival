# Makefile for building, testing, and profiling Kival.
.DEFAULT_GOAL := help

# List of features to use when building. Can be overridden via the environment.
FEATURES ?=

# Cargo profile for builds.
PROFILE ?= dev

# Wall-clock budget for local stateful campaigns.
STATEFUL_DURATION ?= 10m

# Number of Proptest cases completed before checking the campaign deadline.
STATEFUL_BATCH_CASES ?= 4

# Minimum number of actions generated in each stateful case.
STATEFUL_MIN_STEPS ?= 128

# Maximum number of actions generated in each stateful case.
STATEFUL_STEPS ?= 256

# Optional deterministic Proptest campaign seed.
STATEFUL_SEED ?=

##@ Help

.PHONY: help
help: ## Display this help.
	@awk 'BEGIN {FS = ":.*##"; printf "Usage:\n  make \033[36m<target>\033[0m\n"} /^[a-zA-Z_0-9-]+:.*?##/ { printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2 } /^##@/ { printf "\n\033[1m%s\033[0m\n", substr($$0, 5) } ' $(MAKEFILE_LIST)

##@ Build

.PHONY: install
install: ## Build and install the Kival client and server under `$(CARGO_HOME)/bin`.
	cargo install --path bin/kivald --bin kivald --force --locked \
		--features "$(FEATURES)" \
		--profile "$(PROFILE)"

	cargo install --path bin/kival --bin kival --force --locked \
		--features "$(FEATURES)" \
		--profile "$(PROFILE)"

.PHONY: build
build: ## Build the Kival client and server into `target` directory.
	cargo build \
		--bin kivald \
		--bin kival \
		--features "$(FEATURES)" \
		--profile "$(PROFILE)" \
		--locked

##@ Test

.PHONY: test-unit
test-unit: ## Run unit and integration tests, excluding stateful fuzz tests.
	cargo nextest run \
		--workspace \
		--all-features \
		-E 'not binary(stateful)' \
		--no-fail-fast \
		--locked

.PHONY: test-stateful
test-stateful: ## Run stateful fuzz tests.
	$(if $(STATEFUL_SEED),PROPTEST_RNG_SEED="$(STATEFUL_SEED)" )\
	KIVAL_STATEFUL_MIN_STEPS="$(STATEFUL_MIN_STEPS)" \
	KIVAL_STATEFUL_STEPS="$(STATEFUL_STEPS)" cargo nextest run \
		--workspace \
		--all-features \
		-E 'binary(stateful)' \
		--no-capture \
		--no-fail-fast \
		--locked

.PHONY: test-stateful-for
test-stateful-for: ## Run stateful fuzz tests for STATEFUL_DURATION (default: 10m).
	@case "$(STATEFUL_BATCH_CASES)" in \
		''|*[!0-9]*|0) \
			echo "STATEFUL_BATCH_CASES must be a positive integer." >&2; \
			exit 2; \
			;; \
	esac; \
	timeout --foreground "$(STATEFUL_DURATION)" true || { \
		status=$$?; \
		echo "STATEFUL_DURATION is invalid: $(STATEFUL_DURATION)" >&2; \
		exit $$status; \
	}; \
	sleep "$(STATEFUL_DURATION)" & deadline_pid=$$!; \
	trap 'kill "$$deadline_pid" 2>/dev/null || true' EXIT; \
	batch=0; \
	while true; do \
		batch=$$((batch + 1)); \
		echo "Starting stateful batch $$batch ($(STATEFUL_BATCH_CASES) cases)."; \
		status=0; \
		env PROPTEST_CASES="$(STATEFUL_BATCH_CASES)" $(MAKE) test-stateful || status=$$?; \
		if [ $$status -ne 0 ]; then \
			exit $$status; \
		fi; \
		if ! kill -0 "$$deadline_pid" 2>/dev/null; then \
			echo "Completed stateful campaign after $$batch batches ($(STATEFUL_DURATION) budget)."; \
			exit 0; \
		fi; \
	done

.PHONY: test-doc
test-doc: ## Run doc tests.
	cargo test \
		--doc \
		--workspace \
		--all-features \
		--locked

.PHONY: test
test: ## Run the default test suite, excluding stateful fuzz tests.
	$(MAKE) test-unit && \
	$(MAKE) test-doc

.PHONY: test-coverage
test-coverage: ## Run unit tests with coverage and generate an LCOV report.
	cargo +nightly llvm-cov nextest \
		--workspace \
		--all-features \
		-E 'not binary(stateful)' \
		--lcov \
		--output-path lcov.info \
		--locked

.PHONY: test-coverage-html
test-coverage-html: ## Run unit tests with coverage and generate and open an HTML report.
	cargo +nightly llvm-cov nextest \
		--workspace \
		--all-features \
		-E 'not binary(stateful)' \
		--html \
		--open \
		--locked

##@ Linting

.PHONY: fmt
fmt: ## Run all formatters.
	cargo +nightly fmt --all

.PHONY: lint-clippy
lint-clippy: ## Run clippy on the codebase.
	cargo +nightly clippy \
		--workspace \
		--all-targets \
		--all-features \
		--locked \
		-- -D warnings

.PHONY: lint-clippy-fix
lint-clippy-fix: ## Run clippy on the codebase and fix warnings.
	cargo +nightly clippy \
		--workspace \
		--all-targets \
		--all-features \
		--fix \
		--allow-dirty \
		--allow-staged \
		--locked \
		-- -D warnings

.PHONY: lint-typos
lint-typos: ## Run typos on the codebase.
	@command -v typos >/dev/null || { \
		echo "typos not found. Please install it by running the command 'cargo install typos-cli' or refer to the following link for more information: https://github.com/crate-ci/typos"; \
		exit 1; \
	}
	typos

.PHONY: lint
lint: ## Run all linters.
	$(MAKE) fmt && \
	$(MAKE) lint-clippy && \
	$(MAKE) lint-typos

##@ Documentation

.PHONY: doc
doc: ## Build the documentation.
	RUSTDOCFLAGS="--cfg docsrs -D warnings -Zunstable-options --show-type-layout --generate-link-to-definition" \
		cargo +nightly doc \
			--workspace \
			--all-features \
			--document-private-items \
			--no-deps \
			--locked

##@ Other

.PHONY: lock
lock: ## Update the Cargo.lock file with the current dependencies.
	cargo fetch

.PHONY: clean
clean: ## Clean the project.
	cargo clean

.PHONY: deny
deny: ## Perform a `cargo deny` check.
	cargo deny --locked --all-features check all

.PHONY: about
about: ## Generate the `THIRD_PARTY_NOTICES.md` file.
	cargo about generate -c .github/about.toml -o THIRD_PARTY_NOTICES.md .github/about.hbs --locked

.PHONY: check
check: ## Check all crates and targets.
	cargo hack check --locked --feature-powerset --depth 1

.PHONY: pr
pr: ## Run all checks and tests, including stateful fuzz tests.
	$(MAKE) deny && \
	$(MAKE) lint && \
	$(MAKE) test && \
	$(MAKE) test-stateful && \
	$(MAKE) doc && \
	$(MAKE) about
