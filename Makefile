RUST_VERSION = $(shell cat rust-toolchain.toml)
LOG_FILE = cargo_hawk.log

guard-%:
	@ if [ "${${*}}" = "" ]; then \
                echo "Environment variable $* not set"; \
                exit 1; \
        fi

print-%  : ; @echo $*=$($*)

.PHONY: dev
dev:
	RUST_LOG=debug cargo run hawk --log-file $(LOG_FILE)

.PHONY: dev-failure
dev-failure:
	RUST_LOG=debug cargo run hawk --log-file $(LOG_FILE) --path examples/test_failure

.PHONY: tail
tail:
	tspin $(LOG_FILE) -f

.PHONY: outdated
outdated:
	cargo outdated -R

.PHONY: unused
unused:
	cargo +nightly udeps

.PHONY: update
update:
	cargo update

.PHONY: clean
clean:
	cargo clean

.PHONY: flame
flame:
	CARGO_PROFILE_RELEASE_DEBUG=true RUST_LOG=debug cargo flamegraph -- hawk --log-file $(LOG_FILE)
