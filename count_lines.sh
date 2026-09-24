#!/usr/bin/env bash
# Counts the lines of code in every file under the folders next to this
# script, leaving out blank lines and comments.  Files sitting right next to
# the script aren't counted, only what's in the folders below it.
#
#   ./count_lines.sh
#
# What counts as a comment depends on the language:
#
#   .rs .cs          // /// //! and /* ... */ blocks
#   .py .sh .gd .toml  lines starting with #
#
# A line with code and then a comment on the end counts as code.  Python's
# """docstrings""" count as code, because telling them apart from a real
# string takes more than a line at a time.  Build output and git's own
# folders (target, bin, obj, .git, .godot, node_modules) are skipped.

set -euo pipefail

if [ $# -ne 0 ]; then
    echo "Usage: $0" >&2
    echo "It takes no arguments.  It counts the code in every folder under the one it sits in." >&2
    exit 1
fi

# The script's own folder, wherever it was run from.
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Code lines in a file where comments start with // or sit in /* */.
count_slash() {
    awk '
        {
            line = $0
            sub(/^[ \t]+/, "", line)
            if (in_block) {
                if (index(line, "*/") > 0) in_block = 0
                next
            }
            if (line == "") next
            if (substr(line, 1, 2) == "//") next
            if (substr(line, 1, 2) == "/*") {
                if (index(substr(line, 3), "*/") == 0) in_block = 1
                next
            }
            code++
        }
        END { print code + 0 }
    ' "$1"
}

# Code lines in a file where comments start with #.
count_hash() {
    awk '
        {
            line = $0
            sub(/^[ \t]+/, "", line)
            if (line == "") next
            if (substr(line, 1, 1) == "#") next
            code++
        }
        END { print code + 0 }
    ' "$1"
}

declare -A lines_by_type
declare -A files_by_type
total=0

# -mindepth 2 leaves out the files beside the script.  -prune stops find
# walking into the skipped folders at all.
while IFS= read -r -d '' file; do
    extension="${file##*.}"
    case "$extension" in
        rs|cs)          count=$(count_slash "$file") ;;
        py|sh|gd|toml)  count=$(count_hash "$file") ;;
        *)              continue ;;
    esac
    lines_by_type[$extension]=$(( ${lines_by_type[$extension]:-0} + count ))
    files_by_type[$extension]=$(( ${files_by_type[$extension]:-0} + 1 ))
    total=$(( total + count ))
done < <(find "$here" -mindepth 1 \
             \( -name target -o -name bin -o -name obj -o -name .git \
                -o -name .godot -o -name node_modules \) -prune \
             -o -mindepth 2 -type f -print0)

if [ ${#lines_by_type[@]} -eq 0 ]; then
    echo "No code found under $here."
    exit 0
fi

printf "%-6s %7s %9s\n" "Type" "Files" "Lines"
for extension in $(printf "%s\n" "${!lines_by_type[@]}" | sort); do
    printf "%-6s %7d %9d\n" ".$extension" "${files_by_type[$extension]}" "${lines_by_type[$extension]}"
done
printf "%-6s %7s %9d\n" "Total" "" "$total"
