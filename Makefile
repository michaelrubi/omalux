PREFIX ?= $(HOME)/.local

.PHONY: build test install uninstall

build:
	cargo build --release -p omalux

test:
	cargo test --workspace

# Installs for the current user: binary, launcher entry and icon. Does not
# change which app opens folders or raws by default.
install: build
	install -Dm755 target/release/omalux $(PREFIX)/bin/omalux
	install -Dm644 assets/omalux.desktop $(PREFIX)/share/applications/omalux.desktop
	install -Dm644 assets/omalux.svg $(PREFIX)/share/icons/hicolor/scalable/apps/omalux.svg
	-update-desktop-database $(PREFIX)/share/applications 2>/dev/null

uninstall:
	rm -f $(PREFIX)/bin/omalux $(PREFIX)/share/applications/omalux.desktop \
		$(PREFIX)/share/icons/hicolor/scalable/apps/omalux.svg
