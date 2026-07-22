#!/usr/bin/env sh
# Mock docker binary — always succeeds.
# Used by skeleton_first_build happy path test.
case "$1" in
    run)
        echo "mock-container-id-1234567890ab"
        exit 0
        ;;
    exec)
        echo "-- Mock build step: success"
        exit 0
        ;;
    *)
        exit 0
        ;;
esac
