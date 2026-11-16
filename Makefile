.PHONY: build test app run installer selftest release clean

build:
	cargo build --workspace

test:
	cargo test --workspace

app:
	./scripts/build-app.sh

run: app
	open build/Folio.app

installer:
	./scripts/make-installer.sh

# App self test in the real window, invisible, with an empty throwaway library.
selftest:
	@BIN="$$(./scripts/make-test-app.sh build/Folio.app "$$(mktemp -d)")"; \
	LIB="$$(mktemp -d)"; \
	FOLIO_LIBRARY="$$LIB" FOLIO_BACKGROUND=1 FOLIO_SELFTEST=1 "$$BIN"

# make release V=X.Y.Z (the r is added: tag vX.Y.Zr)
release:
	./scripts/release.sh $(V)

clean:
	cargo clean
