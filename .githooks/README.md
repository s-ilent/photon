# Git hooks

Install the hooks with:

```sh
git config core.hooksPath .githooks
```

`pre-commit` blocks an agent-driven commit that would be attributed to the
repository owner. ZCode exports `ZCODE_ENV`/`ZCODE_APP_VERSION`/`ZCODE_BASE_URL`/
`ZCODE_PROCESS_LABEL` (and an `*ZCode*` AppImage path) to every process it
spawns; when those are present and the commit author or committer is the owner's
address, the hook fails and points at `tools/agent_git.sh`, which forces the
agent identity:

```sh
sh tools/agent_git.sh commit -m '<message>'
```

Ported from `cannon-seed/.githooks` — that repo's other rules (the retail-hash
oracle, generated `asm/`, copyrighted disc directories) protect a decompilation
project and do not apply here.

Use `--no-verify` only when the owner is committing from inside this session; the
hook also accepts `ALLOW_OWNER_IDENT=1 git commit ...` for that case.
