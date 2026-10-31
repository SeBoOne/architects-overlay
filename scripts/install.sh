#!/usr/bin/env bash
# SC Architects Overlay — Linux Installer / Uninstaller
#
# Einzeiler (Repo muss öffentlich lesbar sein):
#   curl -fsSL https://git.sebo-home.de/sebo/architects-overlay/raw/branch/main/scripts/install.sh | bash
#
# Flags:
#   --version vX.Y.Z   Release pinnen (default: neuestes Release)
#   --uninstall        App entfernen (Nutzerdaten bleiben)
#   --purge            App ENTFERNEN inkl. Nutzerdaten/Cache (impliziert --uninstall)
#   --yes              Nachfragen überspringen (für piped/ci-Ausführung)
#   --help
#
# Distro-Verhalten:
#   Debian-basiert (apt-get vorhanden): .deb aus dem Release via apt install
#     (sauberste Integration: Paket-Abhängigkeiten, Deinstall, arch-overlay-CLI).
#   Alle anderen: AppImage nach ~/.local/bin + Desktop-Datei + Icon + CLI-Wrapper
#     'arch-overlay' (toggle|show|hide|clickthrough|state via Control-Socket).
#
# Nutzerdaten (bleiben bei normaler Deinstallation):
#   ~/.local/share/com.sebo.architects-overlay

set -euo pipefail

GITEA_BASE="${GITEA_BASE:-https://git.sebo-home.de}"
REPO_SLUG="${REPO_SLUG:-sebo/architects-overlay}"
GITEA_TOKEN="${GITEA_TOKEN:-}"
APP_ID="com.sebo.architects-overlay"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/$APP_ID"
BIN_DIR="${XDG_BIN_DIR:-$HOME/.local/bin}"
DESKTOP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/128x128/apps"
APPIMAGE="$BIN_DIR/architects-overlay.AppImage"
WRAPPER="$BIN_DIR/arch-overlay"
DESKTOP_FILE="$DESKTOP_DIR/$APP_ID.desktop"
ICON_FILE="$ICON_DIR/architects-overlay.png"

MODE="install"
PINNED_VERSION=""
ASSUME_YES=0

log()  { printf '[install] %s\n' "$*"; }
warn() { printf '[install] WARNUNG: %s\n' "$*" >&2; }
die()  { printf '[install] FEHLER: %s\n' "$*" >&2; exit 1; }

usage() {
  sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
  exit 0
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)   [[ $# -ge 2 ]] || die "--version braucht einen Wert (z.B. v0.5.13)"
                 PINNED_VERSION="$2"; shift 2 ;;
    --version=*) PINNED_VERSION="${1#*=}"; shift ;;
    --uninstall) MODE="uninstall"; shift ;;
    --purge)     MODE="purge"; shift ;;
    --yes|-y)    ASSUME_YES=1; shift ;;
    --help|-h)   usage ;;
    *)           die "Unbekanntes Argument: $1 (siehe --help)" ;;
  esac
done

# ---------------------------------------------------------------- Werkzeuge --

need_cmd() { command -v "$1" >/dev/null 2>&1 || die "Benötigtes Werkzeug fehlt: $1"; }

is_debian() {
  command -v apt-get >/dev/null 2>&1 && [[ -e /etc/debian_version ]]
}

# Root (Container) darf direkt, sonst sudo.
SUDO=""
if [[ $(id -u) -eq 0 ]]; then
  SUDO=""
else
  command -v sudo >/dev/null 2>&1 || die "Kein root und kein sudo — Installation nicht möglich."
  SUDO="sudo"
fi

# ------------------------------------------------------------- Gitea-API -----

api_get() {
  # $1 = API-Pfad (ohne Base). Gibt Body aus, Fehlermeldung bei HTTP != 200.
  local url="$GITEA_BASE/api/v1/$1"
  local args=(-fsS -H "Accept: application/json")
  [[ -n "$GITEA_TOKEN" ]] && args+=(-H "Authorization: token $GITEA_TOKEN")
  curl "${args[@]}" "$url" || return 1
}

RELEASE_JSON=""

get_release_json() {
  # Holt die Release-Metadaten (inkl. Assets) einmalig; PINNED_VERSION via
  # tags-Endpunkt, sonst das neueste Release.
  local path body
  if [[ -n "$PINNED_VERSION" ]]; then
    local tag="${PINNED_VERSION#v}"
    path="repos/$REPO_SLUG/releases/tags/v$tag"
  else
    path="repos/$REPO_SLUG/releases?limit=1"
  fi
  body="$(api_get "$path")" \
    || die "Gitea-API nicht erreichbar ($GITEA_BASE) — Netzverbindung prüfen."
  RELEASE_JSON="$body"
}

# Lädt ein Release-Asset nach $3 (Name $2), aus den Assets der geholten
# Release-Metadaten (get_release_json muss vorher gelaufen sein).
fetch_asset() {
  local name="$1" out="$2"
  local dl_url
  dl_url="$(printf '%s' "$RELEASE_JSON" | ASSET_NAME="$name" python3 -c '
import json, os, sys
name = os.environ["ASSET_NAME"]
data = json.load(sys.stdin)
data = data[0] if isinstance(data, list) else data
for a in data["assets"]:
    if a["name"] == name:
        print(a["uuid"]); break
else:
    sys.exit(3)
')" || die "Asset '$name' fehlt im Release (oder API-Antwort unlesbar)."

  local args=(-fsSL -o "$out" --connect-timeout 15 --retry 2)
  [[ -n "$GITEA_TOKEN" ]] && args+=(-H "Authorization: token $GITEA_TOKEN")
  curl "${args[@]}" "$GITEA_BASE/attachments/$dl_url" \
    || die "Download des Assets '$name' fehlgeschlagen."
  [[ -s "$out" ]] || die "Download von '$name' ist leer."
}

# ----------------------------------------------------------------- Uninstall -

do_uninstall() {
  local purge="$1"   # 0 = Nutzerdaten bleiben, 1 = alles weg
  log "Deinstallation (purge=$purge) ..."
  local removed_any=0

  if is_debian && dpkg -s architects-overlay >/dev/null 2>&1; then
    local apt_action="remove"
    [[ $purge -eq 1 ]] && apt_action="purge"
    log "Deb-Paket: apt $apt_action architects-overlay"
    $SUDO env DEBIAN_FRONTEND=noninteractive apt-get -y "$apt_action" architects-overlay \
      || die "apt $apt_action fehlgeschlagen."
    removed_any=1
  fi

  for f in "$APPIMAGE" "$WRAPPER" "$DESKTOP_FILE" "$ICON_FILE"; do
    if [[ -e "$f" ]]; then
      rm -f -- "$f" && log "entfernt: $f" && removed_any=1
    fi
  done

  # Altlasten aus früheren Installationen
  [[ -e "$BIN_DIR/architects-overlay" && ! -d "$BIN_DIR/architects-overlay" ]] && \
    rm -f -- "$BIN_DIR/architects-overlay" && removed_any=1

  if [[ $purge -eq 1 && -d "$DATA_DIR" ]]; then
    rm -rf -- "$DATA_DIR" && log "Nutzerdaten/Cache entfernt: $DATA_DIR"
    removed_any=1
  fi

  if [[ $purge -eq 0 && -d "$DATA_DIR" ]]; then
    log "Nutzerdaten bleiben erhalten: $DATA_DIR (--purge entfernt sie)"
  fi

  [[ $removed_any -eq 1 ]] || log "Nichts gefunden — war nichts installiert."
  log "Fertig."
}

# ------------------------------------------------------------------- Install -

do_install() {
  need_cmd curl
  need_cmd python3
  [[ "$(uname -m)" == "x86_64" ]] || die "Diese Release-Assets gibt es nur für x86_64 (dein System: $(uname -m))."

  get_release_json
  local ref
  ref="v$(printf '%s' "$RELEASE_JSON" | python3 -c 'import json,sys; d=json.load(sys.stdin); d=d[0] if isinstance(d,list) else d; print(d["tag_name"].lstrip("v"))')"
  log "Release: $ref"

  if is_debian; then
    log "Debian-basiert erkannt — .deb-Weg (apt)."
    local deb_name deb_tmp
    # Version aus dem Tag für den Asset-Namen ableiten (tags/vX.Y.Z -> X.Y.Z)
    deb_tmp="$(mktemp -d /tmp/arch-overlay-deb.XXXXXX)/architects-overlay.deb"
    local version
    version="${ref#v}"
    deb_name="architects-overlay_${version}_amd64.deb"
    fetch_asset "$deb_name" "$deb_tmp"
    log "Installiere $deb_name via apt ..."
    $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y "$deb_tmp" \
      || { rm -rf "$(dirname "$deb_tmp")"; die "apt install fehlgeschlagen (Abhängigkeiten/Rechte?)."; }
    rm -rf "$(dirname "$deb_tmp")"
    if command -v arch-overlay >/dev/null 2>&1; then
      log "CLI vorhanden: $(command -v arch-overlay)"
    else
      warn "arch-overlay nach dem .deb nicht im PATH gefunden — Paket prüfen."
    fi
    log "Fertig: Start über Anwendungsmenü oder 'architects-overlay'."
    return 0
  fi

  log "Non-Debian — AppImage-Weg nach $BIN_DIR."
  mkdir -p "$BIN_DIR" "$DESKTOP_DIR" "$ICON_DIR"

  log "Lade AppImage ..."
  fetch_asset "architects-overlay_${ref#v}_amd64.AppImage" "$APPIMAGE"
  chmod +x "$APPIMAGE"

  # CLI-Wrapper: spricht denselben Control-Socket wie das offizielle
  # arch-overlay-Binary (src-tauri/src/bin/arch-overlay.rs).
  log "Lege CLI-Wrapper an: $WRAPPER"
  cat > "$WRAPPER" <<'WRAPPER_EOF'
#!/usr/bin/env bash
# arch-overlay — Control-Socket-Client für das SC Architects Overlay
# (AppImage-Installation; das .deb liefert das echte Binary).
set -u
CMD="${1:-}"
case "$CMD" in toggle|show|hide|clickthrough|state|config) ;; *)
  echo "usage: arch-overlay <toggle|show|hide|clickthrough|state|config>" >&2
  exit 2 ;;
esac
SOCKET=""
[[ -n "${XDG_RUNTIME_DIR:-}" ]] || { echo "XDG_RUNTIME_DIR nicht gesetzt — Desktop-Session?" >&2; exit 1; }
SOCKET="$XDG_RUNTIME_DIR/architects-overlay.sock"
exec python3 - "$SOCKET" "$CMD" <<'PY'
import socket, sys
path, cmd = sys.argv[1], sys.argv[2]
try:
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    s.connect(path)
except OSError as e:
    print(f"cannot reach overlay socket {path}: {e}\nis the overlay app running?", file=sys.stderr)
    sys.exit(1)
s.sendall((cmd + "\n").encode())
buf = b""
while not buf.endswith(b"\n"):
    chunk = s.recv(4096)
    if not chunk:
        break
    buf += chunk
print(buf.decode(errors="replace").strip())
sys.exit(0 if buf.startswith(b"OK") else 1)
PY
WRAPPER_EOF
  chmod +x "$WRAPPER"

  # Icon aus dem AppImage ziehen (falls im Bundle) — AppImage-Extraktion ist
  # optional: ohne Icon läuft die Desktop-Datei mit generischem Icon weiter.
  log "Versuche Icon aus dem AppImage zu extrahieren ..."
  local extract_dir
  extract_dir="$(mktemp -d /tmp/arch-overlay-icon.XXXXXX)"
  if (cd "$extract_dir" && "$APPIMAGE" --appimage-extract >/dev/null 2>&1); then
    local icon_src=""
    for cand in \
      "$extract_dir/squashfs-root/usr/share/icons/hicolor/128x128/apps/architects-overlay.png" \
      "$extract_dir/squashfs-root/usr/share/icons/hicolor/256x256/apps/architects-overlay.png" \
      "$extract_dir/squashfs-root/.DirIcon"; do
      [[ -f "$cand" ]] && icon_src="$cand" && break
    done
    if [[ -n "$icon_src" ]]; then
      cp "$icon_src" "$ICON_FILE" && log "Icon installiert: $ICON_FILE"
    else
      warn "Kein Icon im AppImage gefunden — Desktop-Eintrag ohne Icon."
    fi
  else
    warn "AppImage-Extraktion fehlgeschlagen — Desktop-Eintrag ohne Icon."
  fi
  rm -rf "$extract_dir"

  log "Schreibe Desktop-Datei: $DESKTOP_FILE"
  local icon_value="architects-overlay"
  [[ -f "$ICON_FILE" ]] || icon_value="applications-graphics"
  cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=SC Architects Overlay
Comment=Star-Citizen-Overlay (Architects)
Exec=$APPIMAGE
Icon=$icon_value
Terminal=false
Categories=Game;Utility;
StartupWMClass=architects-overlay
EOF

  log "Fertig. Start über Anwendungsmenü, oder:"
  log "  '$APPIMAGE' (App starten)"
  log "  'arch-overlay toggle' (Overlay umschalten, App muss laufen)"
}

# -------------------------------------------------------------------- Main --

case "$MODE" in
  uninstall) do_uninstall 0 ;;
  purge)     do_uninstall 1 ;;
  install)
    if [[ -t 0 && $ASSUME_YES -eq 0 ]]; then
      printf 'SC Architects Overlay installieren? [j/N] '
      read -r answer
      [[ "$answer" =~ ^[jJyY] ]] || { log "Abgebrochen."; exit 0; }
    fi
    do_install
    ;;
esac
