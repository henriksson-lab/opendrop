# OpenDrop — build, test, and cross-platform packaging.
#
# Style and conventions mirror the sibling project `traceanalyzer`:
#   * APP_VERSION is read from the workspace Cargo.toml (single source of truth).
#   * Freedesktop `install`/`uninstall` targets for a manual Linux install.
#   * Packaging targets that can only run on their native OS no-op with a clear
#     message elsewhere (so `make package` is safe to run anywhere).
#
# Packaging tooling: cargo-deb (Linux .deb), cargo-bundle (macOS .app -> .dmg),
# and a native release build + zip for Windows. Install the helpers with:
#   cargo install cargo-deb cargo-bundle

APP_NAME    := OpenDrop
BIN         := opendrop
PKG         := opendrop
APP_VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
APP_BINARY  := target/release/$(BIN)
DIST        := dist

# --- macOS (cargo-bundle) ---------------------------------------------------
OSX_APP := target/release/bundle/osx/$(APP_NAME).app
DMG     := $(DIST)/$(PKG)-$(APP_VERSION).dmg

# --- Windows ----------------------------------------------------------------
# Override to cross-compile from Linux, e.g.
#   make package-windows WIN_TARGET=x86_64-pc-windows-gnu
# Leave empty for a native build on a Windows host.
WIN_TARGET ?=
ifeq ($(strip $(WIN_TARGET)),)
  WIN_EXE := target/release/$(BIN).exe
  WIN_BUILD_FLAGS :=
else
  WIN_EXE := target/$(WIN_TARGET)/release/$(BIN).exe
  WIN_BUILD_FLAGS := --target $(WIN_TARGET)
endif
WIN_ZIP := $(DIST)/$(PKG)-windows-x64.zip

# --- Linux install (freedesktop layout) -------------------------------------
PREFIX  ?= /usr/local
DESTDIR ?=
BINDIR  := $(DESTDIR)$(PREFIX)/bin
DATADIR := $(DESTDIR)$(PREFIX)/share
ICONDIR := $(DATADIR)/icons/hicolor
# The window's Wayland app_id / X11 WM_CLASS; the icon and the .desktop
# StartupWMClass are all keyed to this name so the desktop finds the icon.
LINUX_APP_ID := opendrop

UNAME_S := $(shell uname -s)

.DEFAULT_GOAL := help
.PHONY: help build release run test check clippy fmt fmt-check clean \
        icons install uninstall \
        package-linux deb package-windows package-macos osx-app dmg \
        package dist

help: ## Show this help
	@printf 'OpenDrop %s — make targets:\n\n' "$(APP_VERSION)"
	@grep -hE '^[a-zA-Z0-9_-]+:.*?## ' $(MAKEFILE_LIST) \
		| sort \
		| awk 'BEGIN{FS=":.*?## "}{printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}'
	@printf '\nPackaging tools: cargo install cargo-deb cargo-bundle\n'

# --- Development ------------------------------------------------------------
build: ## Debug build
	cargo build

release: ## Optimized release build of the app
	cargo build -p $(PKG) --release

run: ## Run the app (debug)
	cargo run -p $(PKG)

test: ## Run the test suite
	cargo test

check: ## Fast type-check
	cargo check

clippy: ## Lint with clippy (warnings = errors)
	cargo clippy --all-targets -- -D warnings

fmt: ## Format all code
	cargo fmt --all

fmt-check: ## Verify formatting without writing
	cargo fmt --all -- --check

clean: ## Remove build artifacts and dist/
	cargo clean
	rm -rf $(DIST)

# --- Icons ------------------------------------------------------------------
# The scalable assets/icon.svg is the source of truth; the committed PNGs are
# rendered from it. Regenerate them if the SVG changes (needs ImageMagick).
icons: ## Re-render assets/icon-*.png from assets/icon.svg
	@command -v convert >/dev/null 2>&1 || { echo "ERROR: ImageMagick 'convert' not found"; exit 1; }
	@for s in 32 64 128 256 512 1024; do \
		convert -background none -density 384 assets/icon.svg -resize $${s}x$${s} assets/icon-$${s}.png && \
		echo "rendered assets/icon-$${s}.png"; \
	done

# --- Linux: manual freedesktop install --------------------------------------
install: release ## Install binary + .desktop + icons into $(PREFIX)
	install -Dm755 "$(APP_BINARY)" "$(BINDIR)/$(LINUX_APP_ID)"
	install -Dm644 packaging/opendrop.desktop "$(DATADIR)/applications/$(LINUX_APP_ID).desktop"
	install -Dm644 assets/icon.svg "$(ICONDIR)/scalable/apps/$(LINUX_APP_ID).svg"
	install -Dm644 assets/icon-256.png "$(ICONDIR)/256x256/apps/$(LINUX_APP_ID).png"
	if [ -z "$(DESTDIR)" ] && command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database "$(DATADIR)/applications"; fi
	if [ -z "$(DESTDIR)" ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then gtk-update-icon-cache -q -t -f "$(ICONDIR)"; fi
	@printf 'Installed to %s\n' "$(DESTDIR)$(PREFIX)"

uninstall: ## Remove a manual freedesktop install
	rm -f "$(BINDIR)/$(LINUX_APP_ID)" \
		"$(DATADIR)/applications/$(LINUX_APP_ID).desktop" \
		"$(ICONDIR)/scalable/apps/$(LINUX_APP_ID).svg" \
		"$(ICONDIR)/256x256/apps/$(LINUX_APP_ID).png"

# --- Linux: .deb (cargo-deb) ------------------------------------------------
# Metadata lives in Cargo.toml under [package.metadata.deb]. We build the
# release binary ourselves and pass --no-build (cargo-deb does not treat the
# workspace target dir as buildable for our `../target/...` asset path).
package-linux: release ## Build a Debian/Ubuntu .deb (cargo-deb)
	@command -v cargo-deb >/dev/null 2>&1 || { echo "ERROR: cargo-deb not installed (cargo install cargo-deb)"; exit 1; }
	@# cargo-deb only auto-strips binaries it built; ours is a custom asset, so strip it here.
	if command -v strip >/dev/null 2>&1; then strip --strip-unneeded "$(APP_BINARY)" || strip "$(APP_BINARY)" || true; fi
	cargo deb -p $(PKG) --no-build --strip
	@mkdir -p $(DIST)
	@cp target/debian/*.deb $(DIST)/ 2>/dev/null || true
	@printf 'Built .deb(s):\n'; ls -1 target/debian/*.deb

deb: package-linux ## Alias for package-linux

# --- macOS: .app bundle (cargo-bundle) + .dmg -------------------------------
# Metadata lives in Cargo.toml under [package.metadata.bundle].
osx-app: ## Build the macOS .app (cargo-bundle; macOS only)
ifeq ($(UNAME_S),Darwin)
	@command -v cargo-bundle >/dev/null 2>&1 || { echo "ERROR: cargo-bundle not installed (cargo install cargo-bundle)"; exit 1; }
	cargo bundle -p $(PKG) --release
	@printf 'Built %s\n' "$(OSX_APP)"
else
	@echo "SKIP osx-app: a real .app must be built on macOS. On macOS run: cargo bundle -p $(PKG) --release"
endif

dmg: osx-app ## Wrap the .app into a .dmg (macOS only)
ifeq ($(UNAME_S),Darwin)
	@command -v hdiutil >/dev/null 2>&1 || { echo "ERROR: hdiutil not found"; exit 1; }
	@mkdir -p $(DIST)
	rm -f "$(DMG)"
	hdiutil create -volname "$(APP_NAME)" -srcfolder "$(OSX_APP)" -ov -format UDZO "$(DMG)"
	@printf 'Built %s\n' "$(DMG)"
else
	@echo "SKIP dmg: building a .dmg requires macOS (hdiutil)."
endif

package-macos: dmg ## Alias: build the macOS .app and .dmg

# --- Windows: release build + zip -------------------------------------------
# Native on a Windows host; or cross with WIN_TARGET=x86_64-pc-windows-gnu
# (needs the rustup target + a mingw-w64 toolchain and may not build Skia).
package-windows: ## Build opendrop.exe and zip it (Windows host or cross)
	cargo build -p $(PKG) --release $(WIN_BUILD_FLAGS)
	@mkdir -p $(DIST)
	@if [ ! -f "$(WIN_EXE)" ]; then echo "ERROR: $(WIN_EXE) not found (are you building for Windows?)"; exit 1; fi
	@if command -v zip >/dev/null 2>&1; then \
		( cd "$(dir $(WIN_EXE))" && zip -j "$(CURDIR)/$(WIN_ZIP)" "$(notdir $(WIN_EXE))" ); \
	else \
		python3 -c "import zipfile,sys; z=zipfile.ZipFile('$(WIN_ZIP)','w',zipfile.ZIP_DEFLATED); z.write('$(WIN_EXE)', '$(BIN).exe'); z.close()"; \
	fi
	@printf 'Built %s\n' "$(WIN_ZIP)"

# --- Umbrella ---------------------------------------------------------------
# Build every package the current host can produce. Non-native targets no-op
# with a message, so this is safe to invoke on any OS.
package: ## Build all packages the current host supports
	@mkdir -p $(DIST)
ifeq ($(UNAME_S),Linux)
	$(MAKE) package-linux
	@echo "NOTE: Windows/macOS packages must be built on their native OS (or CI)."
else ifeq ($(UNAME_S),Darwin)
	$(MAKE) package-macos
	@echo "NOTE: Linux .deb and Windows .zip must be built on their native OS (or CI)."
else
	$(MAKE) package-windows
endif
	@echo "Artifacts in $(DIST)/"

dist: package ## Alias for package
