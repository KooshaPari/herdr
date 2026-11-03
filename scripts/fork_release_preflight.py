"""Read-only source preflight for a KooshaPari Herdr fork release candidate.

This does not build, sign, publish, validate artifacts, or authorize installation.
"""
import argparse
import json
import re
import subprocess
import tomllib


VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)-k(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\Z")
UPSTREAM = "https://github.com/herdrdev/herdr.git"
FORK = "KooshaPari/herdr"


def git(*args):
    return subprocess.check_output(["git", *args], text=True).strip()


def preflight(version, upstream_commit):
    match = VERSION.fullmatch(version)
    if not match:
        raise ValueError("expected fork version BASE-kMAJOR.MINOR.PATCH")
    if not re.fullmatch(r"[0-9a-f]{40}", upstream_commit):
        raise ValueError("upstream commit must be a full lowercase SHA-1")
    if git("status", "--porcelain", "--untracked-files=all"):
        raise ValueError("working tree must be clean")
    base_version = ".".join(match.groups()[:3])
    tag = f"refs/tags/v{base_version}"
    remote = git("ls-remote", UPSTREAM, tag, f"{tag}^{{}}")
    refs = dict(line.split()[::-1] for line in remote.splitlines())
    remote_commit = refs.get(f"{tag}^{{}}", refs.get(tag))
    if remote_commit != upstream_commit:
        raise ValueError("upstream tag does not resolve to the supplied commit")
    base = git("rev-parse", "--verify", f"{upstream_commit}^{{commit}}")
    head = git("rev-parse", "--verify", "HEAD^{commit}")
    if base != upstream_commit or subprocess.run(
        ["git", "merge-base", "--is-ancestor", base, head], check=False
    ).returncode != 0:
        raise ValueError("candidate must descend from the exact upstream release commit")
    manifest = tomllib.loads(git("show", f"{head}:Cargo.toml"))
    lock = tomllib.loads(git("show", f"{head}:Cargo.lock"))
    packages = [p for p in lock["package"] if p["name"] == "herdr" and "source" not in p]
    if manifest["package"]["version"] != version or len(packages) != 1 or packages[0]["version"] != version:
        raise ValueError("candidate Cargo.toml and Cargo.lock must match fork version")
    return {"fork": FORK, "version": version, "upstream_tag": f"v{base_version}",
            "upstream_commit": base, "candidate_commit": head,
            "result": "source_preflight_only"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--upstream-commit", required=True)
    args = parser.parse_args()
    print(json.dumps(preflight(args.version, args.upstream_commit), sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"error: {error}") from error
