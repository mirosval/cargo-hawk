RUST_VERSION = $(shell cat rust-toolchain.toml)

guard-%:
	@ if [ "${${*}}" = "" ]; then \
                echo "Environment variable $* not set"; \
                exit 1; \
        fi

print-%  : ; @echo $*=$($*)

.PHONY: dev
dev:
	RUST_LOG=debug cargo run hawk --verbose

tail:
	tspin cargo_hawk.log -f

outdated:
	cargo outdated -R

unused:
	cargo +nightly udeps

update:
	cargo update

clean:
	cargo clean

