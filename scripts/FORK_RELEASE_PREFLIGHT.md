# Fork release source preflight

This check is read-only and is separate from the upstream release workflow. It
accepts KooshaPari versions of the form `BASE-kMAJOR.MINOR.PATCH`, checks the
upstream release tag against the specified full commit, verifies candidate
ancestry and matching Cargo versions, and emits a source receipt for
`KooshaPari/herdr`.

From a clean checkout, run:

```text
python scripts/fork_release_preflight.py --version 0.9.3-k0.1.0 --upstream-commit 7b116c05bfda646af39d2524c54e70c751f57ee8
```

The result does not verify a fork remote, preview, artifacts, signature,
installer, runtime compatibility, or publication authority. It does not build,
tag, publish, alter upstream distribution, or establish release readiness.
The upstream stable release workflow is intentionally unchanged; a fork release
still needs a separately designed artifact and publication pipeline.
