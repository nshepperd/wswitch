#!/bin/sh
# Create a self-signed code signing certificate in your login keychain, so
# local builds keep a stable signature and macOS remembers wswitch's
# Accessibility / Screen Recording permissions across rebuilds.
#
# The certificate only works on this Mac; it doesn't make builds trusted
# anywhere else. Remove it later with Keychain Access (search "wswitch-local").
set -eu

NAME="wswitch-local"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

if security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; then
    echo "Certificate \"$NAME\" already exists."
    exit 0
fi

dir=$(mktemp -d)
trap 'trash "$dir" 2>/dev/null || true' EXIT

cat >"$dir/cert.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
EOF

openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$dir/cert.cnf" \
    -keyout "$dir/key.pem" -out "$dir/cert.pem" 2>/dev/null
# Import key and certificate separately; the keychain pairs them into an
# identity. (No .p12 needed, so no legacy PKCS#12 encryption either.)
security import "$dir/key.pem" -k "$KEYCHAIN" -t priv -T /usr/bin/codesign
security import "$dir/cert.pem" -k "$KEYCHAIN" -t cert

echo "Created \"$NAME\". 'make app' will now sign with it."
