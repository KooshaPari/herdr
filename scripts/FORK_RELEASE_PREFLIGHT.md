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

## Unpublished Windows candidate archive

After committing a clean checkout, create a directory beneath the current
user's temporary directory at `herdr-fork-packaging/<full-commit-sha>` and put
the candidate at `herdr.exe` there. Protect the directory from concurrent
modification by other processes. The packager takes no file paths from its CLI;
it derives the fixed input and `candidate.zip` output names from the verified
commit. Existing directories and binary must not be symlinks or Windows reparse
points, and output is created exclusively.

```text
python scripts/package_fork_windows.py --version 0.9.3-k0.1.0 --upstream-commit 7b116c05bfda646af39d2524c54e70c751f57ee8
```

The archive contains the supplied bytes as `herdr.exe` and a JSON receipt with
its SHA-256, size, and independently checked source commit. This does not prove
that the executable was built from that commit, that it has the stated version,
or that it is suitable for release. A two-byte Windows header check is not a
platform validation. Existing output is never replaced; a partial output may
remain if writing fails and must be inspected before retry. No installation,
release creation, signing, or upload is performed. The directory checks do not
prevent a concurrently privileged process from replacing paths during the run.
