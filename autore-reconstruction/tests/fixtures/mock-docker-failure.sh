#!/usr/bin/env sh
# Mock docker binary — cmake/cmkr commands fail with MSVC C2079 error.
# Used by skeleton_first_build failure path test.
case "$1" in
    run)
        echo "mock-container-id-1234567890ab"
        exit 0
        ;;
    exec)
        shift
        shift
        cmd="$1"
        if [ "$cmd" = "cmake" ] || [ "$cmd" = "cmkr" ]; then
            echo "src/generated/missing_entity.cpp(10) : error C2079: 'missing_entity' : uses undefined struct 'UndefinedType'" >&2
            exit 1
        fi
        echo "-- Mock build step: success"
        exit 0
        ;;
    *)
        exit 0
        ;;
esac
