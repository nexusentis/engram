#!/usr/bin/env bash
set -euo pipefail

# Download and install Qdrant binary for the current platform.
# Usage: ./scripts/setup-qdrant.sh [--version v1.17.0]

VERSION="v1.17.0"
INSTALL_DIR="./bin"
FORCE=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version) VERSION="$2"; shift 2 ;;
        --force) FORCE=1; shift ;;
        -h|--help)
            echo "Usage: $0 [--version v1.17.0]"
            echo "Downloads Qdrant binary to ./bin/qdrant"
            exit 0
            ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

# Detect OS
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "${OS}" in
    darwin) ;;
    linux)  ;;
    *)      echo "Error: Unsupported OS: ${OS}"; exit 1 ;;
esac

# Map architecture and build the asset name
case "${OS}-${ARCH}" in
    darwin-arm64)
        ASSET="qdrant-aarch64-apple-darwin.tar.gz" ;;
    darwin-x86_64)
        ASSET="qdrant-x86_64-apple-darwin.tar.gz" ;;
    linux-x86_64)
        ASSET="qdrant-x86_64-unknown-linux-gnu.tar.gz" ;;
    linux-aarch64)
        ASSET="qdrant-aarch64-unknown-linux-musl.tar.gz" ;;
    *)
        echo "Error: Unsupported platform: ${OS}-${ARCH}"
        exit 1
        ;;
esac

URL="https://github.com/qdrant/qdrant/releases/download/${VERSION}/${ASSET}"

# Skip if already installed (use --force to re-download)
if [[ -x "${INSTALL_DIR}/qdrant" ]] && [[ "${FORCE}" -eq 0 ]]; then
    INSTALLED_VERSION=$("${INSTALL_DIR}/qdrant" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || echo "unknown")
    echo "Qdrant already installed at ${INSTALL_DIR}/qdrant (v${INSTALLED_VERSION})"
    echo "  Use --force to re-download, or just run: ${INSTALL_DIR}/qdrant"
    exit 0
fi

echo "Downloading Qdrant ${VERSION} for ${OS}/${ARCH}..."
echo "  ${URL}"

mkdir -p "${INSTALL_DIR}"

# Download and extract
curl -fSL "${URL}" | tar xz -C "${INSTALL_DIR}"

# Verify
if [[ -x "${INSTALL_DIR}/qdrant" ]]; then
    echo ""
    echo "Qdrant installed to ${INSTALL_DIR}/qdrant"
    echo ""
    echo "Start it with:"
    echo "  ${INSTALL_DIR}/qdrant"
    echo ""
    echo "Qdrant will listen on:"
    echo "  REST: http://localhost:6333"
    echo "  gRPC: http://localhost:6334"
else
    echo "Error: qdrant binary not found after extraction"
    exit 1
fi
