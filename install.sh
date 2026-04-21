#!/bin/sh
set -e

REPO="listennn08/wt"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

detect_target() {
  OS=$(uname -s | tr '[:upper:]' '[:lower:]')
  ARCH=$(uname -m)

  case "${OS}-${ARCH}" in
    darwin-x86_64)  echo "x86_64-apple-darwin" ;;
    darwin-arm64)   echo "aarch64-apple-darwin" ;;
    linux-x86_64)   echo "x86_64-unknown-linux-gnu" ;;
    linux-aarch64)  echo "aarch64-unknown-linux-gnu" ;;
    *) echo "Unsupported platform: ${OS}-${ARCH}" >&2; exit 1 ;;
  esac
}

TARGET=$(detect_target)
VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed 's/.*"v\(.*\)".*/\1/')
URL="https://github.com/${REPO}/releases/download/v${VERSION}/wt-${TARGET}.tar.gz"

echo "Installing wt v${VERSION} for ${TARGET}..."
TMP=$(mktemp -d)
curl -fsSL "${URL}" | tar -xz -C "${TMP}"
install -m 755 "${TMP}/wt" "${INSTALL_DIR}/wt"
rm -rf "${TMP}"
echo "wt installed to ${INSTALL_DIR}/wt"
