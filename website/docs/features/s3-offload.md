---
title: S3 asset offload
---

# S3 asset offload

**What it is.** An optional, opt-in feature that uploads large attachments
(images, PDFs, videos referenced from your docs) to an S3-compatible bucket
instead of copying them into `dist/`. The generated HTML points at the bucket
(or a CDN in front of it).

**Why you'd want it.** Keeps your deployed site small and lets a CDN serve big
binaries directly, which matters for image-heavy docs or size-limited static
hosts.

:::callout{type=note title="Off by default"}
S3 offload is opt-in at *two* levels: the `docgen` binary must be built with the
`s3` cargo feature, **and** your project must configure `[s3]`. Without both,
docgen behaves exactly as usual and copies attachments locally.
:::

## Configuration

Add an `[s3]` section to `docgen.toml`:

```toml
[s3]
bucket = "my-bucket"
region = "auto"                      # "auto" works for R2/MinIO; use a real
                                     # AWS region (e.g. "us-east-1") for AWS S3
endpoint = "https://<account>.r2.cloudflarestorage.com"  # omit for AWS S3
prefix = "docs-assets"               # optional key prefix within the bucket
public_url = "https://cdn.example.com"
path_style = true                    # required by MinIO and some S3-compatibles
```

`public_url` must actually be reachable by readers. docgen uploads objects but
does **not** set an ACL or configure bucket permissions — the bucket policy (or
the CDN/custom domain in front of it) must grant public read access, or images
will 403 on an otherwise successful build.

## Credentials

Credentials are **never** stored in `docgen.toml`. They are read from the
standard environment variables at build time:

```sh
export AWS_ACCESS_KEY_ID=...
export AWS_SECRET_ACCESS_KEY=...
```

## Installing with the feature

The `s3` feature is off by default. Install it explicitly:

```sh
cargo install docgen-rs --features s3
```

Building with `--features s3` needs a C/C++ compiler and `cmake` (pulled in
transitively for TLS). On a minimal CI image, install `build-essential` and
`cmake` (or your distro's equivalents) first.

## Auto-activation

Offload activates only when **all** of these hold:

1. the `docgen` binary was built with the `s3` feature;
2. `[s3]` is present in `docgen.toml`;
3. both credential environment variables are set at build time.

If any condition is missing, `docgen build` falls back to copying attachments
into `dist/` as usual and prints a one-line explanation to stderr — it never
fails the build for a missing or incomplete S3 setup. `docgen dev` never
uploads, regardless of configuration.

## How keys work

Objects use content-addressed keys of the form
`{prefix}/{dir}/{stem}.{hash}.{ext}`, where `{hash}` is a SHA-256 of the file
contents. Uploads are therefore idempotent: an unchanged file keeps the same key
and is skipped on the next build, so re-running after a transient failure
resumes cleanly.

## Limitations

- Only asset references written in **Markdown syntax** — `![](…)` images and
  `[](…)` links — are rewritten to bucket URLs. A raw HTML `<img src>` or
  `<a href>` is left untouched, and because offload mode skips the local copy,
  such a reference will 404 on the deployed site. Keep attachment references in
  Markdown syntax when offload is active.
- For non-AWS providers (R2, MinIO, B2, Spaces) you **must** set `endpoint`.
  Without it the client targets `https://s3.<region>.amazonaws.com`, so a bare
  `region = "auto"` with no `endpoint` will not resolve.

## Example: GitLab CI

```yaml
pages:
  stage: deploy
  variables:
    AWS_ACCESS_KEY_ID: $S3_ACCESS_KEY_ID
    AWS_SECRET_ACCESS_KEY: $S3_SECRET_ACCESS_KEY
  script:
    - apt-get update && apt-get install -y build-essential cmake
    - cargo install docgen-rs --features s3
    - docgen build .
  artifacts:
    paths: [dist]
```

Set `S3_ACCESS_KEY_ID` / `S3_SECRET_ACCESS_KEY` as masked CI/CD variables.
