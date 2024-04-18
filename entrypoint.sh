#!/bin/bash
set -e

echo "Running entrypoint script..."

if [ -n "$GRPC_URL" ]; then
       export GRPC_OPTION="--grpcs $GRPC_URL";
    else
        export GRPC_OPTION="";
fi

# Check if Vault secret file exists
if [ -f "/vault/secrets/bot" ]; then
  # Source the Vault secret file if it exists
  . /vault/secrets/bot
fi

# Execute the CMD instruction
exec "$@"
