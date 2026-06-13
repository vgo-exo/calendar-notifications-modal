.PHONY: help build-snap install-snap build-deb install-deb clean test build install

help:
	@echo "Calendar Notifications Modal - Build Commands"
	@echo ""
	@echo "Debian Package (Best for Ubuntu/Debian):"
	@echo "  make build-deb       - Build the Debian package"
	@echo "  make install-deb     - Install the Debian package locally"
	@echo "  make deb-all         - Build and install deb in one command"
	@echo ""
	@echo "Snap Package:"
	@echo "  make build-snap      - Build the snap package"
	@echo "  make install-snap    - Install the snap package locally"
	@echo "  make snap-all        - Build and install snap in one command"
	@echo ""
	@echo "Manual Build:"
	@echo "  make build           - Build release binary"
	@echo "  make test            - Run tests"
	@echo "  make install         - Install via systemd (manual method)"
	@echo ""
	@echo "Cleanup:"
	@echo "  make clean           - Clean build artifacts"
	@echo "  make clean-deb       - Clean Debian package artifacts"
	@echo "  make clean-snap      - Clean snap build artifacts"

# Debian package commands
build-deb:
	@echo "Building Debian package..."
	@./packaging/build-deb.sh

install-deb: build-deb
	@echo "Installing Debian package..."
	@DEB_FILE=$$(ls -t calendar-notifications-modal_*.deb 2>/dev/null | head -1); \
	if [ -z "$$DEB_FILE" ]; then \
		echo "Error: No .deb file found. Run 'make build-deb' first."; \
		exit 1; \
	fi; \
	sudo dpkg -i "$$DEB_FILE" || sudo apt --fix-broken install -y
	@echo ""
	@echo "✅ Installation complete!"
	@echo ""
	@echo "To start the service:"
	@echo "  systemctl --user enable calendar-notifications-modal.service"
	@echo "  systemctl --user start calendar-notifications-modal.service"
	@echo ""
	@echo "Configuration will be created at:"
	@echo "  ~/.config/calendar-notifications-modal/config.toml"

deb-all: install-deb

clean-deb:
	rm -f calendar-notifications-modal_*.deb
	rm -rf target/debian
	rm -rf debian/.debhelper debian/files debian/calendar-notifications-modal
	rm -f debian/*.log debian/*.substvars debian/debhelper-build-stamp

# Snap package commands
build-snap:
	@echo "Building snap package..."
	snapcraft

install-snap: build-snap
	@echo "Installing snap package..."
	@SNAP_FILE=$$(ls -t calendar-notifications-modal_*.snap 2>/dev/null | head -1); \
	if [ -z "$$SNAP_FILE" ]; then \
		echo "Error: No snap file found. Run 'make build-snap' first."; \
		exit 1; \
	fi; \
	sudo snap install --dangerous "$$SNAP_FILE"
	@echo ""
	@echo "✅ Installation complete!"
	@echo ""
	@echo "The daemon will start automatically with your graphical session."
	@echo "Configuration: ~/.config/calendar-notifications-modal/config.toml"
	@echo ""
	@echo "To configure and restart:"
	@echo "  1. Edit: nano ~/.config/calendar-notifications-modal/config.toml"
	@echo "  2. Restart: systemctl --user restart snap.calendar-notifications-modal.daemon.service"

snap-all: install-snap

clean-snap:
	snapcraft clean
	rm -f calendar-notifications-modal_*.snap

# Standard build commands
build:
	cargo build --release

test:
	cargo test --workspace

install: build
	./packaging/install.sh

# Clean all
clean:
	cargo clean
	rm -rf target

clean-all: clean clean-deb clean-snap
