# Publishing a release image from CI

Status: approved design, not yet implemented.

## Problem

`logb` has no releases. The `container image` job in `ci.yml` builds the image and boots it to
check `/api/health`, then throws it away — nothing is ever published. Running the app on another
machine means cloning the repository and building it, and there is no way to say "this version",
only "this commit".

There are no git tags, and `Cargo.toml` has said `0.1.0` since the first commit.

## Scope

In scope: a workflow that publishes a versioned image to GitHub Container Registry when a
version tag is pushed, and creates a GitHub Release for that tag.

Out of scope, each deliberately:

- **Multi-arch.** amd64 only. The image is a static musl binary on `scratch`, so arm64 would be
  cheap to add later if a Pi or an ARM NAS ever needs it, but nothing needs it today.
- **The binary tarball.** `build.sh` still produces one for local use; the release does not carry
  it.
- **Build attestation.**
- **Deployment.** The workflow's job ends at the registry. Updating the running instance stays a
  deliberate `docker compose pull && docker compose up -d`, so nothing in GitHub holds a route
  into a home network.
- **`ci.yml`.** It guards commits and is left alone; this guards releases.

## Trigger

A new workflow on `push` of tags matching `v*`. Releases are deliberate: nothing reaches the
registry that was not named, and the tag is the single source of the version.

## What it does, in order

1. **Check the version agrees.** The tag `v0.2.0` must match `version = "0.2.0"` in `Cargo.toml`,
   or the run fails before anything is built.
2. **Build the image**, reusing the existing `Dockerfile` unchanged.
3. **Boot it and check `/api/health`** — the same smoke test `ci.yml` already performs.
4. **Push to `ghcr.io/13/logb`**, authenticated with `GITHUB_TOKEN` and `packages: write`.
5. **Create the GitHub Release** for the tag, with notes generated from the commits since the
   previous tag and the pull command in the body.

Step 1 is the one worth arguing for. Everything else here is recoverable — a bad image can be
rebuilt and re-pushed, a Release edited. A version mismatch is not: the image's `/api/health`
would report a version it is not, the tag would disagree with the binary inside, and both would
be believed. It is a two-line check that removes an entire class of confusing bug.

Step 3 exists because a tag can point at any commit, including one CI never ran against. The
release path verifies rather than inheriting an assumption.

## Image tags

Pushing `v0.2.0` publishes three:

| Tag | Moves? | For |
|---|---|---|
| `0.2.0` | never | pinning exactly |
| `0.2` | with each patch | tracking fixes without editing compose |
| `latest` | with each release | trying it out |

## Visibility

The package is **public**. GHCR defaults new packages to private, which would mean the
deployment host needs a stored token and a `docker login` to pull its own image. The repository
is already public, so the image exposes nothing the source does not.

This requires one manual step after the first publish — flipping the package to public in its
GitHub settings, which no workflow can do for a package that does not yet exist. The README says
so, next to the pull command, because otherwise the first pull from another machine fails with a
confusing `denied` and the reason is not discoverable from the error.

## Documentation

The README gains a short section covering the pull command, the tag scheme, and the upgrade
being `docker compose pull && docker compose up -d` — noting that a new image applies any pending
migrations when it starts, which is worth knowing before upgrading rather than after.

## Testing

A workflow is not unit-testable, and asserting on YAML proves nothing. What is worth checking is
the part with actual logic:

- The version check is a shell step. It gets exercised both ways on a real run — the first
  release is the positive case, and a deliberate mismatch is worth trying once by hand before
  trusting it.
- Everything else is verified by the release actually happening: the image exists at the
  expected tags, it boots, and the Release page is created.

The first tag is therefore the test. `v0.1.0` matches the current `Cargo.toml`, so it can be cut
without touching any version number.
