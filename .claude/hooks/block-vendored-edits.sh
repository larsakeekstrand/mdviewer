#!/bin/sh
# PreToolUse(Edit|Write): refuse hand-edits to vendored frontend assets.
#
# These ship verbatim from upstream and must be RE-VENDORED, never patched in
# place (CLAUDE.md: the github-markdown.css is even structured specifically to
# survive re-vendoring). A hand-edit here is a silent, near-unreviewable change
# that the next upstream bump will clobber. Reads the tool-call JSON on stdin;
# exit 2 blocks the call and shows the message to Claude as the reason.
input=$(cat)
case "$input" in
  */ui/katex/*|*/ui/codemirror/*|*/ui/mermaid.min.js*|*/ui/github-markdown.css*|*/ui/morphdom-umd.min.js*)
    echo "Blocked: vendored asset. ui/katex/, ui/codemirror/, ui/mermaid.min.js, ui/github-markdown.css, and ui/morphdom-umd.min.js are vendored from upstream — re-vendor them, don't hand-edit (see CLAUDE.md). If this edit is genuinely intended, make it manually outside Claude." >&2
    exit 2
    ;;
esac
exit 0
