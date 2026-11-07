APP     := target/wswitch.app
BIN     := target/release/wswitch
# Signs with the local "wswitch-local" certificate if you've made one with
# `make cert` (so macOS permissions survive rebuilds), otherwise ad hoc.
# Override with e.g. SIGN="Developer ID Application: ...".
SIGN    ?= $(shell security find-certificate -c wswitch-local >/dev/null 2>&1 && echo wswitch-local || echo -)

.PHONY: app install run cert clean

app: $(BIN) Info.plist
	mkdir -p $(APP)/Contents/MacOS
	cp Info.plist $(APP)/Contents/Info.plist
	cp $(BIN) $(APP)/Contents/MacOS/wswitch
	codesign --force --sign "$(SIGN)" --options runtime $(APP)
	@echo "Built $(APP)"

$(BIN): Cargo.toml $(wildcard src/*.rs)
	cargo build --release

install: app
	ditto $(APP) /Applications/wswitch.app
	@echo "Installed to /Applications/wswitch.app"

run: app
	@pkill -x wswitch; while pgrep -qx wswitch; do sleep 0.1; done
	open $(APP)

cert:
	./scripts/make-cert.sh

clean:
	cargo clean
