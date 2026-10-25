#!/bin/sh
# PreToolUse hook for the eval: records "RUN_ID tool_name" in $EVAL_LOG, and
# denies the tools that would publish a write-up (Artifact, any Doc tool,
# Claude Docs), so nothing leaves the machine. Everything else is let through.
input=$(cat)
tool=$(printf '%s' "$input" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("tool_name",""))')
printf '%s %s\n' "$EVAL_RUN_ID" "$tool" >> "$EVAL_LOG"
case "$tool" in
  Artifact*|mcp__*doc*|mcp__*Doc*|mcp__claude_ai_Claude_Docs__*)
    printf '%s\n' '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"eval: recorded, not run"}}'
    ;;
esac
exit 0
