#!/usr/bin/env sh
# Generates a throw-away PKI for trying the TLS link locally:
#   ca.pem            DSO certificate authority
#   station.pem/.key  the gateway (server) certificate, for localhost
#   dso.pem/.key      the control centre (client) certificate, signed by the CA
#   rogue.pem/.key    a client certificate from a *different* CA (must be rejected)
# Demo only: keys are unencrypted and never committed.
set -eu
cd "$(dirname "$0")"
mkdir -p demo && cd demo
export MSYS_NO_PATHCONV=1

ec() { openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out "$1"; }

ec ca.key.pem
openssl req -x509 -new -key ca.key.pem -sha256 -days 825 -subj "/CN=Demo DSO Telecontrol CA" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" -out ca.pem

leaf() { # name cn eku san ca
  ec "$1.key.pem"
  openssl req -new -key "$1.key.pem" -subj "/CN=$2" -out "$1.csr"
  printf "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=%s\n%s" "$3" "$4" > "$1.ext"
  openssl x509 -req -in "$1.csr" -CA "$5.pem" -CAkey "$5.key.pem" -CAcreateserial -days 365 -sha256 -extfile "$1.ext" -out "$1.pem" 2>/dev/null
  rm -f "$1.csr" "$1.ext"
}
leaf station "grid-edge-gateway" serverAuth "subjectAltName=DNS:localhost,IP:127.0.0.1" ca
leaf dso "DSO control centre" clientAuth "" ca

ec rogue-ca.key.pem
openssl req -x509 -new -key rogue-ca.key.pem -sha256 -days 30 -subj "/CN=Not the DSO" \
  -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign" -out rogue-ca.pem
leaf rogue "Intruder" clientAuth "" rogue-ca
rm -f ./*.srl
echo "demo certificates written to $(pwd)"
