#!/bin/sh
# tools/agent_git.sh — run any git command under the agent identity.
#
# The owner's personal identity lives in the ambient global git config, so a
# plain `git commit` in an agent session attributes the work to the owner.
# .githooks/pre-commit rejects that, and bypassing it with --no-verify is not
# an option. Force the bot identity here instead:
#
#   sh tools/agent_git.sh commit -m "Add paging."
#   sh tools/agent_git.sh push
#
# Ported from cannon-seed/tools/agent_git.sh.
set -e

AGENT_NAME='Super Z'
AGENT_EMAIL='41898282+github-actions[bot]@users.noreply.github.com'

exec env \
    GIT_AUTHOR_NAME="$AGENT_NAME" GIT_AUTHOR_EMAIL="$AGENT_EMAIL" \
    GIT_COMMITTER_NAME="$AGENT_NAME" GIT_COMMITTER_EMAIL="$AGENT_EMAIL" \
    git "$@"
