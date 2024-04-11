#!/bin/bash
set -e

echo "Running entrypoint script..."

if [ -n "$GRPC_URL" ]; then
       export GRPC_OPTION="--grpcs $GRPC_URL";
    else
        export GRPC_OPTION="";
    fi

# Execute the CMD instruction
exec "$@"

exit 0
