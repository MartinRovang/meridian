#!/usr/bin/env sh
# Download a meridian binary for this machine onto PATH.
# ponytail: one static binary from the GitHub release, no clone, no runtime. The app's own
# `meridian --update` does the same download over the running executable.
set -e
REPO=${REPO:-MartinRovang/meridian}
BIN=${BIN:-$HOME/.local/bin}

# ponytail: one script, two artefacts. COMPONENT=server puts the API binary on a host; the default
# puts the desktop app on a workstation.
COMPONENT=${COMPONENT:-app}
case "$COMPONENT" in
	app) PREFIX=meridian; NAME=${NAME:-meridian} ;;
	server) PREFIX=meridian-server; NAME=${NAME:-meridian-server} ;;
	*) echo "COMPONENT must be app or server"; exit 1 ;;
esac

# ponytail: colours only when stdout is a terminal, empty strings otherwise
if [ -t 1 ]; then
	B=$(printf '\033[1m'); DIM=$(printf '\033[2m'); R=$(printf '\033[0m')
	PURPLE=$(printf '\033[38;5;141m'); CYAN=$(printf '\033[38;5;75m')
	GREEN=$(printf '\033[38;5;78m'); YELLOW=$(printf '\033[38;5;221m')
else
	B= DIM= R= PURPLE= CYAN= GREEN= YELLOW=
fi

printf '\n  %s%smeridian%s %s: portfolio management and surveillance%s\n\n' "$B" "$PURPLE" "$R" "$DIM" "$R"

# the asset name update::asset_name() and ci.yml agree on
case "$(uname -s)-$(uname -m)" in
	Linux-x86_64) ASSET=$PREFIX-linux-x86_64 ;;
	Darwin-arm64) ASSET=$PREFIX-macos-arm64 ;;
	Darwin-x86_64) ASSET=$PREFIX-macos-x86_64 ;;
	*) printf '  no prebuilt binary for %s-%s; build from source: git clone https://github.com/%s && cargo build --release\n' "$(uname -s)" "$(uname -m)" "$REPO"; exit 1 ;;
esac

# ponytail: REF installs a release by tag; default is the newest release.
if [ -n "${REF:-}" ]; then
	URL="https://github.com/$REPO/releases/download/$REF/$ASSET"
else
	URL="https://github.com/$REPO/releases/latest/download/$ASSET"
fi
mkdir -p "$BIN"
TMP="$BIN/.$NAME.download"
curl -fsSL --retry 3 -o "$TMP" "$URL"
# the release publishes <asset>.sha256 beside the binary; a download that does not match it is not
# installed. This is a tamper check, not a signature: whoever can write the release writes both.
WANT=$(curl -fsSL --retry 2 "$URL.sha256" 2>/dev/null | cut -d' ' -f1 || true)
if [ -n "$WANT" ]; then
	GOT=$(sha256sum "$TMP" 2>/dev/null | cut -d' ' -f1 || shasum -a 256 "$TMP" | cut -d' ' -f1)
	[ "$GOT" = "$WANT" ] || { rm -f "$TMP"; printf '\n  checksum mismatch for %s: not installed\n\n' "$ASSET"; exit 1; }
else
	printf '  %s⚠%s  no published checksum for %s: installing unverified\n' "$YELLOW" "$R" "$ASSET"
fi
chmod +x "$TMP"
mv -f "$TMP" "$BIN/$NAME"

# ponytail: a launcher entry, so it starts from the app menu with no terminal attached. The app
# only: a server has no app menu. Linux only: macOS wants a .app bundle, which a bare binary is not.
if [ "$COMPONENT" = app ] && [ "$(uname -s)" = Linux ]; then
	APPS=$HOME/.local/share/applications
	ICONS=$HOME/.local/share/icons
	mkdir -p "$APPS" "$ICONS"
	# the binary carries the same artwork it shows on its splash; no download for it
	if "$BIN/$NAME" --icon > "$ICONS/.$NAME.new" 2>/dev/null; then
		# ponytail: the file name carries the art's checksum. GNOME caches a .desktop icon by path, so
		# writing new art to the same path leaves the old one in the app menu until the shell restarts;
		# a path it has never seen is the one thing it reliably picks up. Older ones are swept.
		SUM=$(sha256sum "$ICONS/.$NAME.new" 2>/dev/null | cut -c1-8 \
			|| shasum -a 256 "$ICONS/.$NAME.new" | cut -c1-8)
		ICON=$ICONS/$NAME-$SUM.png
		mv -f "$ICONS/.$NAME.new" "$ICON"
		find "$ICONS" -maxdepth 1 -name "$NAME-*.png" ! -name "$NAME-$SUM.png" -delete 2>/dev/null || true
	else
		rm -f "$ICONS/.$NAME.new"
		ICON=$NAME
	fi
	cat > "$APPS/meridian.desktop" <<-EOF
		[Desktop Entry]
		Type=Application
		Name=Meridian
		Comment=Portfolio management and surveillance
		Exec=$BIN/$NAME
		Icon=$ICON
		Terminal=false
		Categories=Office;Finance;
		StartupWMClass=meridian
	EOF
	update-desktop-database "$APPS" 2>/dev/null || true
fi

# ponytail: --version is the cheapest full load of the binary, so a missing Linux webview shows up
# here as a loader error instead of as a window that never opens.
ERR=$("$BIN/$NAME" --version 2>&1 >/dev/null) || true
TAG=$("$BIN/$NAME" --version 2>/dev/null | awk '{print $NF}')

row() { printf '    %s%-34s%s %s%s%s\n' "$CYAN" "$1" "$R" "$DIM" "$2" "$R"; }
warn() { printf '\n    %s⚠%s  %s\n' "$YELLOW" "$R" "$1"; }

printf '  %s✓%s installed %s%s%s  %s: %s%s\n\n' "$GREEN" "$R" "$B" "${TAG:-latest}" "$R" "$DIM" "$BIN/$NAME" "$R"
if [ "$COMPONENT" = app ]; then
	printf '  Run it with %s%s%s%s in your terminal:\n\n' "$B" "$PURPLE" "$NAME" "$R"
	row "$NAME"                            "the desktop app, with its own API in-process"
	row "$NAME --api-url URL --token T"    "connect to a meridian-server you host"
	row "$NAME --scope norway"             "market scope: norway, scandinavia, nordics, europe, global"
	row "$NAME --update"                   "install the newest release over this one"
	printf '\n  %sor launch it from your app menu: no terminal needed%s\n' "$DIM" "$R"
else
	printf '  Run it with %s%s%s%s on the host:\n\n' "$B" "$PURPLE" "$NAME" "$R"
	row "$NAME --port 8080"                "serve the API; prints a token if you give none"
	row "MERIDIAN_TOKEN=... $NAME"         "the shared secret, kept out of the process list"
	row "$NAME --scope scandinavia"        "which markets ticker search offers"
	printf '\n  %sit binds 127.0.0.1 only: put it behind a reverse proxy with TLS%s\n' "$DIM" "$R"
fi

if [ -z "$TAG" ]; then
	warn "$NAME did not start: ${ERR:-unknown error}"
	if [ "$COMPONENT" = app ] && [ "$(uname -s)" = Linux ]; then
		printf '       %sit needs the system webview:%s\n' "$DIM" "$R"
		printf '       %sapt install libwebkit2gtk-4.1-0 libgtk-3-0%s   %s(dnf: webkit2gtk4.1 gtk3 / pacman: webkit2gtk-4.1 gtk3)%s\n' "$CYAN" "$R" "$DIM" "$R"
	fi
fi

case ":$PATH:" in
	*":$BIN:"*) ;;
	*) warn "$BIN is not on your PATH: add to your shell rc:"
	   printf '       %sexport PATH="%s:$PATH"%s\n' "$CYAN" "$BIN" "$R" ;;
esac
printf '\n'
