#!/usr/bin/env bash
# Mock docker binary for autore-reconstruction build provider tests.
#
# Simulates a Docker container that runs cmkr + cmake build commands.
# Controlled by environment variables:
#   AUTORE_MOCK_BUILD_FAIL=1   → cmake commands exit 1 with MSVC-style errors
#   AUTORE_MOCK_ERROR_FILE=... → file path used in the mock error diagnostic
#   (unset or 0)               → all commands exit 0
#
# Usage: set docker_binary to this script's path in DockerMsvc2002Config.

case "$1" in
    run)
        # `docker run -d --name X -v ... -w ... IMAGE sleep infinity`
        # Always succeeds — container "started".
        echo "mock-container-id-1234567890ab"
        exit 0
        ;;
    exec)
        shift  # remove "exec"
        shift  # remove container name
        # Remaining args are the command: cmkr gen / cmake --build build / etc.
        cmd="$1"
        if [ "$AUTORE_MOCK_BUILD_FAIL" = "1" ]; then
            if [ "$cmd" = "cmake" ] || [ "$cmd" = "cmkr" ]; then
                error_file="${AUTORE_MOCK_ERROR_FILE:-src/generated/missing.cpp}"
                echo "${error_file}(10) : error C2079: 'missing_entity' : uses undefined struct 'UndefinedType'" >&2
                exit 1
            fi
        fi
        # Success path: print plausible output.
        echo "-- Mock build step: $*"
        echo "-- Build succeeded (mock)"
        exit 0
        ;;
    *)
        # Unknown docker command — succeed silently.
        exit 0
        ;;
esac
