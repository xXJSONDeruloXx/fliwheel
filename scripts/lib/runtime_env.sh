#!/usr/bin/env bash

# Set a canonical runtime option only when neither the current fliwheel name
# nor its historical Clicky alias was supplied by the caller. The variable
# names are constructed from fixed suffixes at call sites, while values are
# assigned with printf so spaces and punctuation remain data.
fliwheel_env_default() {
    local suffix="$1"
    local default_value="$2"
    local current_var="FLIWHEEL_${suffix}"
    local legacy_var="CLICKY_${suffix}"

    if [[ -n "${!current_var+x}" ]]; then
        return
    fi
    if [[ -n "${!legacy_var+x}" ]]; then
        printf -v "$current_var" '%s' "${!legacy_var}"
    else
        printf -v "$current_var" '%s' "$default_value"
    fi
    export "$current_var"
}
