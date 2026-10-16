#!/usr/bin/env bash
# One-time KONKR APK signing setup. Run on your own trusted Mac/Linux shell.
# Never upload the private keystore or its password to GitHub source control.
set -euo pipefail
umask 077

REPO="${SLOT_GITHUB_REPO:-Baggio94/slot-konkr}"
SAFE_HOME="${XDG_CONFIG_HOME:-$HOME/.config}/slot-konkr-signing"
STORE="$SAFE_HOME/slot-konkr-signing.p12"
PASSWORD_FILE="$SAFE_HOME/store-password"
ALIAS="slot-konkr"

for bin in gh keytool openssl base64; do
  command -v "$bin" >/dev/null || { echo "Missing tool: $bin" >&2; exit 1; }
done
gh auth status >/dev/null || { echo "Run gh auth login first" >&2; exit 1; }
mkdir -p "$SAFE_HOME"
chmod 700 "$SAFE_HOME"

if [[ -f "$STORE" && ! -f "$PASSWORD_FILE" ]]; then
  echo "Signing keystore already exists without its password. Refusing to replace it." >&2
  exit 1
fi

if [[ ! -f "$STORE" ]]; then
  if [[ -f "$PASSWORD_FILE" ]]; then
    echo "A password exists without its keystore. Refusing to replace it." >&2
    exit 1
  fi
  openssl rand -hex 32 > "$PASSWORD_FILE"
  chmod 600 "$PASSWORD_FILE"
  PASSWORD="$(cat "$PASSWORD_FILE")"
  keytool -genkeypair -noprompt \
    -alias "$ALIAS" -keyalg RSA -keysize 3072 -validity 36500 \
    -storetype PKCS12 -keystore "$STORE" -storepass "$PASSWORD" \
    -keypass "$PASSWORD" -dname "CN=Slot KONKR, O=Slot Community, C=FR"
  chmod 600 "$STORE"
  echo "A permanent signing keystore was created locally."
fi

PASSWORD="$(cat "$PASSWORD_FILE")"
keytool -list -storetype PKCS12 -keystore "$STORE" \
  -storepass "$PASSWORD" -alias "$ALIAS" >/dev/null

# GitHub encrypts these as Actions secrets; no secret values are printed.
gh secret set SLOT_KEYSTORE_BASE64 -R "$REPO" \
  --body "$(base64 < "$STORE" | tr -d '\n\r')"
gh secret set SLOT_SIGNING_STORE_PASSWORD -R "$REPO" --body "$PASSWORD"
gh secret set SLOT_SIGNING_KEY_ALIAS -R "$REPO" --body "$ALIAS"
gh secret set SLOT_SIGNING_KEY_PASSWORD -R "$REPO" --body "$PASSWORD"

echo "Four encrypted GitHub Actions secrets have been configured for $REPO."
echo "Private signing material stays on your computer at $SAFE_HOME."
echo "Back up that directory securely: losing it prevents signing future updates."
