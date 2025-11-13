#!/bin/bash
#
# Generate self-signed certificates for Rift HTTPS demo
#
# Usage: ./generate-certs.sh
#

set -e

CERTS_DIR="$(dirname "$0")/certs"

echo "Generating certificates in $CERTS_DIR..."
mkdir -p "$CERTS_DIR"

# Generate CA key and certificate
echo "Creating CA..."
openssl genrsa -out "$CERTS_DIR/ca.key" 4096 2>/dev/null
openssl req -new -x509 -key "$CERTS_DIR/ca.key" -out "$CERTS_DIR/ca.crt" -days 365 \
  -subj "/CN=Rift Demo CA/O=Rift/C=US" 2>/dev/null

# Create config for server certificate with SANs
cat > "$CERTS_DIR/server.cnf" << EOF
[req]
distinguished_name = req_distinguished_name
req_extensions = v3_req
prompt = no

[req_distinguished_name]
CN = localhost
O = Rift Demo
C = US

[v3_req]
basicConstraints = CA:FALSE
keyUsage = digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = @alt_names

[alt_names]
DNS.1 = localhost
DNS.2 = rift
DNS.3 = rift-https-demo
IP.1 = 127.0.0.1
EOF

# Generate server key and CSR
echo "Creating server certificate..."
openssl genrsa -out "$CERTS_DIR/server.key" 2048 2>/dev/null
openssl req -new -key "$CERTS_DIR/server.key" -out "$CERTS_DIR/server.csr" \
  -config "$CERTS_DIR/server.cnf" 2>/dev/null

# Sign server certificate with CA
openssl x509 -req -in "$CERTS_DIR/server.csr" \
  -CA "$CERTS_DIR/ca.crt" -CAkey "$CERTS_DIR/ca.key" \
  -CAcreateserial -out "$CERTS_DIR/server.crt" -days 365 \
  -extensions v3_req -extfile "$CERTS_DIR/server.cnf" 2>/dev/null

# Clean up CSR and config (no longer needed)
rm -f "$CERTS_DIR/server.csr" "$CERTS_DIR/server.cnf" "$CERTS_DIR/ca.srl"

echo ""
echo "Certificates generated successfully!"
echo ""
echo "Files created:"
echo "  $CERTS_DIR/ca.crt     - CA certificate (for client trust)"
echo "  $CERTS_DIR/ca.key     - CA private key"
echo "  $CERTS_DIR/server.crt - Server certificate"
echo "  $CERTS_DIR/server.key - Server private key"
echo ""
echo "To trust the CA on macOS:"
echo "  sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain $CERTS_DIR/ca.crt"
echo ""
echo "Or use curl with --cacert:"
echo "  curl --cacert $CERTS_DIR/ca.crt https://localhost:8443/get"
echo ""
