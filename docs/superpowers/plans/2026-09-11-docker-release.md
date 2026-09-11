# Docker Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pushing a version tag builds, smoke-tests and publishes a `logb` image to GitHub Container Registry, and creates a GitHub Release for that tag.

**Architecture:** A new `.github/workflows/release.yml` triggered by `push` of `v*` tags. It refuses to build unless the tag matches the version in `Cargo.toml`, boots the built image and checks `/api/health` before pushing, then creates the Release. `ci.yml` and the `Dockerfile` are untouched.

**Tech Stack:** GitHub Actions, `docker/build-push-action`, `docker/metadata-action`, `softprops/action-gh-release`, GHCR, the existing multi-stage `Dockerfile` (static musl binary on `scratch`).

## Global Constraints

- amd64 only. No multi-arch, no QEMU, no buildx platform list.
- The image publishes to `ghcr.io/13/logb`, authenticated with the workflow's `GITHUB_TOKEN`. No stored registry secret.
- `.github/workflows/ci.yml` is **not** modified — it guards commits; this guards releases.
- The `Dockerfile` is **not** modified.
- The release does **not** carry the binary tarball, and does **not** produce a build attestation.
- The workflow must not deploy anything or reach the running instance.
- A tag whose version disagrees with `Cargo.toml` must fail **before** any image is built.

---

### Task 1: The release workflow, and the README section that makes it usable

**Files:**
- Create: `.github/workflows/release.yml`
- Modify: `README.md` (new section after `## Run with Docker`, which ends with the line about `./data`)

**Interfaces:**
- Consumes: the existing `Dockerfile` at the repository root, unchanged.
- Produces: images at `ghcr.io/13/logb` tagged `<major>.<minor>.<patch>`, `<major>.<minor>` and `latest`; a GitHub Release for the tag.

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/release.yml`:

```yaml
name: Release

# Deliberate releases only: nothing reaches the registry that was not named, and the tag is the
# single source of the version. ci.yml still guards every commit.
on:
  push:
    tags: ['v*']

jobs:
  release:
    name: publish image and release
    runs-on: ubuntu-latest
    permissions:
      contents: write   # create the GitHub Release
      packages: write   # push to ghcr.io
    steps:
      - uses: actions/checkout@v5
        with:
          fetch-depth: 0   # release notes are generated from the commits since the last tag

      # Before anything is built. An image whose /api/health reports a version it is not gets
      # believed, and the tag would disagree with the binary inside it. Everything else in this
      # workflow is recoverable by re-running; that is not.
      - name: The tag must match Cargo.toml
        run: |
          tag="${GITHUB_REF_NAME#v}"
          crate=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
          if [ "$tag" != "$crate" ]; then
            echo "tag $GITHUB_REF_NAME says $tag, Cargo.toml says $crate" >&2
            exit 1
          fi
          echo "version $crate"

      - uses: docker/setup-buildx-action@v3

      - uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - id: meta
        uses: docker/metadata-action@v5
        with:
          images: ghcr.io/${{ github.repository }}
          # 0.2.0 pins exactly, 0.2 follows patches without editing compose, latest is for
          # trying it out.
          tags: |
            type=semver,pattern={{version}}
            type=semver,pattern={{major}}.{{minor}}
            type=raw,value=latest

      # Built and loaded locally first: a tag can point at any commit, including one ci.yml
      # never ran against, so the image is booted before anyone can pull it.
      - name: Build
        uses: docker/build-push-action@v6
        with:
          context: .
          push: false
          load: true
          tags: logb:release-candidate
          cache-from: type=gha
          cache-to: type=gha,mode=max

      - name: Boot the image
        run: |
          docker run -d --name logb-release -p 8080:8080 -v logb-release-data:/data logb:release-candidate
          for i in $(seq 1 30); do
            if curl -fsS http://127.0.0.1:8080/api/health; then echo; exit 0; fi
            sleep 1
          done
          echo "health check never succeeded"; docker logs logb-release; exit 1

      # Re-runs the build, but every layer is a cache hit from the step above, so this pushes
      # the image that was just smoke-tested rather than building a second one.
      - name: Push
        uses: docker/build-push-action@v6
        with:
          context: .
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          labels: ${{ steps.meta.outputs.labels }}
          cache-from: type=gha
          cache-to: type=gha,mode=max

      - uses: softprops/action-gh-release@v2
        with:
          generate_release_notes: true
          body: |
            ```bash
            docker pull ghcr.io/${{ github.repository }}:${{ github.ref_name }}
            ```
```

- [ ] **Step 2: Check the workflow parses**

GitHub Actions will not tell you about a YAML error until you push a tag, and a failed tag is
awkward to redo. Parse it locally first:

Run: `python3 -c "import yaml,sys; d=yaml.safe_load(open('.github/workflows/release.yml')); print(sorted(d['jobs']['release']['permissions'].items())); print(len(d['jobs']['release']['steps']), 'steps')"`

Expected: `[('contents', 'write'), ('packages', 'write')]` and `8 steps`.

If `yaml` is not installed, `pip install pyyaml` or fall back to
`docker run --rm -v "$PWD":/repo rhysd/actionlint:latest -color`.

- [ ] **Step 3: Check the version comparison both ways**

The only logic in the file is the version check, and it is worth running before trusting it.
Extract it and try both cases:

```bash
check() {
  tag="${1#v}"
  crate=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
  [ "$tag" = "$crate" ] && echo "match: $tag" || echo "mismatch: tag $tag vs crate $crate"
}
check v0.1.0   # expect: match: 0.1.0
check v9.9.9   # expect: mismatch: tag 9.9.9 vs crate 0.1.0
```

Expected: exactly those two lines. If the first says mismatch, `Cargo.toml` is not `0.1.0` and
the tag in Task 2 must be changed to agree with it.

- [ ] **Step 4: Write the README section**

In `README.md`, immediately after the `## Run with Docker` section — which ends with the line
``Everything lives in `./data`: `logb.db` (SQLite), `files/` (originals, content-addressed), `thumbs/`.`` — insert:

````markdown
### Released images

Tagged releases are published to GitHub Container Registry:

```bash
docker pull ghcr.io/13/logb:latest
```

`0.2.0` pins exactly, `0.2` follows patches, `latest` follows releases. To use one instead of
building locally, replace the `build: .` line in `docker-compose.yml` with
`image: ghcr.io/13/logb:latest`.

Upgrading is a pull and a restart:

```bash
docker compose pull
docker compose up -d
```

A new image applies any pending database migrations when it starts. Take a snapshot first if the
release notes mention a schema change — see [Backup](#backup).

> The first release has to be made public by hand: GHCR creates packages private, and nothing
> can change that for a package that does not exist yet. After the first publish, open the
> package on GitHub → Package settings → Change visibility → Public. Until then `docker pull`
> fails with `denied`, which does not hint at the cause.
````

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release.yml README.md
git commit -m "ci: publish a release image to ghcr.io on version tags"
git push origin main
```

---

### Task 2: Cut `v0.1.0` and verify the whole path

**Files:** none — this task creates a tag and checks what happens.

**Interfaces:**
- Consumes: `.github/workflows/release.yml` from Task 1, on `main`.
- Produces: the `v0.1.0` tag, a published image, and a GitHub Release.

A workflow cannot be tested without running it, and this is the first thing that ever has.
`Cargo.toml` already says `0.1.0`, so the tag needs no version bump.

- [ ] **Step 1: Confirm Task 1 is on main and CI is green**

```bash
git log --oneline -1
gh run list --limit 1
```

Expected: the release-workflow commit at the tip, and its CI run `completed success`. Do not tag
a commit whose CI failed — the release workflow smoke-tests the image but does not run the test
suite.

- [ ] **Step 2: Tag and push**

```bash
git tag v0.1.0
git push origin v0.1.0
```

- [ ] **Step 3: Watch the run**

```bash
gh run list --workflow=release.yml --limit 1
gh run watch "$(gh run list --workflow=release.yml --limit 1 --json databaseId -q '.[0].databaseId')"
```

Expected: every step green. If `The tag must match Cargo.toml` fails, the tag and `Cargo.toml`
disagree — delete the tag locally and remotely (`git tag -d v0.1.0 && git push --delete origin
v0.1.0`), fix the version, and start again rather than forcing the tag.

- [ ] **Step 4: Make the package public**

Open `https://github.com/users/13/packages/container/logb/settings`, and under **Danger Zone →
Change visibility** set it to Public. This cannot be done before the first publish, because the
package does not exist until then.

- [ ] **Step 5: Prove a clean pull works**

The point of the whole task. From a shell with no GHCR credentials:

```bash
docker logout ghcr.io
docker pull ghcr.io/13/logb:0.1.0
docker run --rm ghcr.io/13/logb:0.1.0 --help | head -3
```

Expected: the pull succeeds without a login, and `--help` prints the usage line beginning
`logb`. A `denied` here means Step 4 did not take effect.

- [ ] **Step 6: Check the three tags and the Release exist**

```bash
gh api /users/13/packages/container/logb/versions -q '.[0].metadata.container.tags'
gh release view v0.1.0 --json tagName,body -q '.tagName + "\n" + .body'
```

Expected: the tag list contains `0.1.0`, `0.1` and `latest`; the Release exists and its body
carries the pull command and the generated notes.

- [ ] **Step 7: Record the result**

No commit — this task changes no files. Note in the session whether the published image digest
matches the one that was smoke-tested, which `gh run view` shows in the Push step's output. If
they differ, the cache did not hold between the two build steps and the pushed image was rebuilt
rather than reused; that is worth knowing, though not itself a failure.

---

## Self-Review

**Spec coverage.** Every section maps to a task: the tag trigger, the version check before any
build, the boot-and-health smoke test before the push, the GHCR push with its three tags, and
the GitHub Release with generated notes (Task 1 Step 1); the public-visibility manual step and
the README's pull/upgrade documentation (Task 1 Step 4 and Task 2 Step 4); the "first tag is the
test" claim (Task 2).

The spec's exclusions hold: amd64 only, no tarball, no attestation, no deployment, and neither
`ci.yml` nor the `Dockerfile` is touched.

**Placeholder scan.** No TBDs and no "add error handling". Steps that cannot be automated — the
visibility toggle, the clean-pull check — name the exact URL and the exact commands, with the
expected output and the meaning of the likely failure.

**Type consistency.** The image reference `ghcr.io/${{ github.repository }}` in `metadata-action`
and in the Release body resolves to `ghcr.io/13/logb`, which is what the README and Task 2 use
literally. The locally-loaded tag `logb:release-candidate` is used only by the Build and Boot
steps and never pushed; the pushed tags come from `steps.meta.outputs.tags`.

**One judgement call worth flagging.** The build runs twice — once with `load: true` to smoke-test
locally, once with `push: true`. `build-push-action` cannot both load and push in a single
invocation, and pushing first would publish an image before anything had booted it. The second
run is cache hits, so the cost is seconds; Task 2 Step 7 checks that assumption by comparing
digests rather than trusting it.
