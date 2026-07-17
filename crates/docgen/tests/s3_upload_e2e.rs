//! End-to-end S3 offload against a real MinIO over real TLS.
//!
//! This exists because the cheap version of this test does not work. The obvious
//! guard — assert the dependency graph resolves one rustls crypto provider — passed
//! green through a release in which *every* S3 upload failed, because
//! `attohttpc`'s `tls-rustls-*-ring` features still pull rustls into the tree (so the
//! graph looks healthy) while compiling `no_tls_impl.rs` behind the scenes, so every
//! request dies with "TLS is disabled, activate one of the tls- features". Nothing
//! short of an actual https:// request to an actual S3 endpoint distinguishes a
//! working TLS stack from that. Hence: a container, a certificate, a real handshake.
//!
//! Deliberately over HTTPS, never plain http://. MinIO is perfectly happy on http,
//! but rustls is never constructed on that path, so an http-only test would go green
//! against exactly the broken build this is here to catch.
//!
//! The MinIO cert is signed by a CA generated here and trusted via `SSL_CERT_FILE`,
//! which `rustls-native-certs` honours — that is why the `attohttpc` dep takes
//! `native-roots` rather than `webpki-roots`, whose compiled-in Mozilla roots cannot
//! be extended at runtime.
#![cfg(feature = "s3")]

use std::fs;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Pinned by digest, not `:latest`: this test leans on two undocumented properties
/// of the image — `mc` being bundled in the server image (`wait_until_ready` shells
/// into it) and a top-level directory under the data dir being treated as a bucket.
/// A floating tag lets an upstream release break CI with no change on our side.
const IMAGE: &str =
    "minio/minio@sha256:14cea493d9a34af32f524e538b8346cf79f3321eff8e708c1e2960462bd8936e";
const BUCKET: &str = "docgen-assets";
const ACCESS_KEY: &str = "docgentest";
const SECRET_KEY: &str = "docgentest123";

/// Kills the container on the way out, including on panic — otherwise a failing
/// assertion leaks a MinIO for every run.
struct Minio {
    name: String,
    port: u16,
}

impl Drop for Minio {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.name])
            .output();
    }
}

fn docker_available() -> bool {
    Command::new("docker")
        .arg("info")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run(cmd: &mut Command) -> String {
    let out = cmd
        .output()
        .unwrap_or_else(|e| panic!("spawn {cmd:?}: {e}"));
    assert!(
        out.status.success(),
        "{cmd:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A CA, plus a `localhost` server cert signed by it in the layout MinIO expects
/// (`certs/public.crt` + `certs/private.key`). Returns the CA path for SSL_CERT_FILE.
///
/// Shells out to `openssl` rather than using a Rust cert crate on purpose: `rcgen`
/// and friends carry their own crypto provider feature (aws-lc-rs by default), and a
/// dev-dependency that sways which provider this binary resolves would corrupt the
/// very thing under test.
fn generate_certs(dir: &Path) -> PathBuf {
    let certs = dir.join("certs");
    fs::create_dir_all(&certs).unwrap();
    let ca_key = dir.join("ca.key");
    let ca_crt = dir.join("ca.crt");
    let csr = dir.join("server.csr");
    let ext = dir.join("ext.cnf");

    // rustls requires a SAN (it ignores CN entirely) and webpki requires the
    // serverAuth EKU on the leaf; without either, this fails as a cert error that
    // looks nothing like the TLS bug being guarded.
    fs::write(
        &ext,
        "subjectAltName=DNS:localhost,IP:127.0.0.1\nextendedKeyUsage=serverAuth\nbasicConstraints=CA:FALSE\n",
    )
    .unwrap();

    run(Command::new("openssl").args([
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        ca_key.to_str().unwrap(),
        "-out",
        ca_crt.to_str().unwrap(),
        "-subj",
        "/CN=docgen-test-ca",
        "-days",
        "1",
    ]));
    run(Command::new("openssl").args([
        "req",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        certs.join("private.key").to_str().unwrap(),
        "-out",
        csr.to_str().unwrap(),
        "-subj",
        "/CN=localhost",
    ]));
    run(Command::new("openssl").args([
        "x509",
        "-req",
        "-in",
        csr.to_str().unwrap(),
        "-CA",
        ca_crt.to_str().unwrap(),
        "-CAkey",
        ca_key.to_str().unwrap(),
        "-CAcreateserial",
        "-out",
        certs.join("public.crt").to_str().unwrap(),
        "-days",
        "1",
        "-extfile",
        ext.to_str().unwrap(),
    ]));

    ca_crt
}

fn start_minio(dir: &Path) -> Minio {
    let certs = dir.join("certs");
    let data = dir.join("data");
    // A top-level directory in MinIO's data dir *is* a bucket, which saves both a
    // second container for `mc` and an alias/credential dance just to call mb.
    fs::create_dir_all(data.join(BUCKET)).unwrap();

    let name = format!("docgen-s3-e2e-{}", std::process::id());
    let _ = Command::new("docker").args(["rm", "-f", &name]).output();

    run(Command::new("docker").args([
        "run",
        "-d",
        "--name",
        &name,
        // Ephemeral host port: a fixed one collides with whatever else is on the
        // machine, and CI runners are not exclusively ours.
        "-p",
        "127.0.0.1::9000",
        "-v",
        &format!("{}:/root/.minio/certs:ro", certs.display()),
        "-v",
        &format!("{}:/data", data.display()),
        "-e",
        &format!("MINIO_ROOT_USER={ACCESS_KEY}"),
        "-e",
        &format!("MINIO_ROOT_PASSWORD={SECRET_KEY}"),
        IMAGE,
        "server",
        "/data",
    ]));

    // Take ownership of the container the moment it exists, before anything that can
    // panic: `docker port` and the parse below both can, and unwinding without this
    // guard in scope would leak a running MinIO.
    let mut minio = Minio { name, port: 0 };

    let mapping = run(Command::new("docker").args(["port", &minio.name, "9000/tcp"]));
    minio.port = mapping
        .lines()
        .next()
        .and_then(|l| l.rsplit(':').next())
        .and_then(|p| p.trim().parse().ok())
        .unwrap_or_else(|| panic!("could not parse host port from {mapping:?}"));

    wait_until_ready(&minio);
    minio
}

fn wait_until_ready(minio: &Minio) {
    let deadline = Instant::now() + Duration::from_secs(90);
    // TCP accept only means the listener is up; `mc alias set` performs a real
    // authenticated probe, so it is the honest readiness signal.
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", minio.port)).is_ok() {
            let ok = Command::new("docker")
                .args([
                    "exec",
                    &minio.name,
                    "mc",
                    "alias",
                    "set",
                    "local",
                    "https://localhost:9000",
                    ACCESS_KEY,
                    SECRET_KEY,
                    "--insecure",
                ])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                return;
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let logs = Command::new("docker")
        .args(["logs", &minio.name])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stderr).to_string())
        .unwrap_or_default();
    panic!("minio never became ready. logs:\n{logs}");
}

/// A minimal site with exactly one attachment: any non-`.md` file under `docs/`
/// is an asset, and assets are what the S3 pass offloads.
fn write_site(dir: &Path, port: u16) {
    fs::create_dir_all(dir.join("docs/attachments")).unwrap();
    fs::write(
        dir.join("docs/index.md"),
        "# Home\n\n![shot](./attachments/img.png)\n",
    )
    .unwrap();
    fs::write(
        dir.join("docs/attachments/img.png"),
        b"\x89PNG\r\n\x1a\n-not-a-real-png-",
    )
    .unwrap();
    fs::write(
        dir.join("docgen.toml"),
        format!(
            r#"[site]
title = "s3 e2e"

[s3]
bucket = "{BUCKET}"
region = "us-east-1"
endpoint = "https://localhost:{port}"
prefix = "docs-assets"
public_url = "https://cdn.example.com/"
path_style = true
"#
        ),
    )
    .unwrap();
}

fn build(site: &Path, ca: &Path) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_docgen"))
        .arg("build")
        .arg(site)
        .env("AWS_ACCESS_KEY_ID", ACCESS_KEY)
        .env("AWS_SECRET_ACCESS_KEY", SECRET_KEY)
        // Trusts our throwaway CA without touching the host's store. Honoured by
        // `rustls-native-certs`; this is the whole reason for the `native-roots`
        // feature choice.
        .env("SSL_CERT_FILE", ca)
        .output()
        .expect("running docgen build");

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "docgen build failed:\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
    format!("{stdout}{stderr}")
}

#[test]
fn uploads_assets_to_s3_over_tls() {
    if !docker_available() {
        // Skipping silently in CI would recreate the exact failure mode this test
        // exists to prevent: a green run that proved nothing about S3.
        if std::env::var("DOCGEN_S3_E2E").is_ok() {
            panic!("DOCGEN_S3_E2E is set but docker is unavailable — refusing to skip");
        }
        eprintln!("skipping s3 e2e: docker unavailable (set DOCGEN_S3_E2E=1 to make this fatal)");
        return;
    }

    let tmp = std::env::temp_dir().join(format!("docgen_s3_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();

    let ca = generate_certs(&tmp);
    let minio = start_minio(&tmp);

    let site = tmp.join("site");
    write_site(&site, minio.port);

    // First build: the bucket is empty, so the asset is PUT over TLS. A broken TLS
    // stack fails right here — "TLS is disabled" if the impl was compiled out, or a
    // provider panic if two are compiled in with no default installed.
    let first = build(&site, &ca);
    assert!(
        first.contains("1 uploaded, 0 already present"),
        "expected the asset to be uploaded, got:\n{first}"
    );

    // Second build: proves the PUT actually landed, and that the read direction
    // works too — `already present` is derived from a real ListObjectsV2 over TLS,
    // so it can only be 1 if the object is genuinely in the bucket.
    let second = build(&site, &ca);
    assert!(
        second.contains("0 uploaded, 1 already present"),
        "expected the asset to be found in the bucket and skipped, got:\n{second}"
    );

    // The point of offloading: the page references the CDN, and the attachment is
    // deliberately not copied into dist/.
    let html = fs::read_to_string(site.join("dist/index/index.html")).unwrap();
    assert!(
        html.contains("https://cdn.example.com/docs-assets/attachments/img."),
        "page should reference the public URL, got:\n{html}"
    );
    assert!(
        !site.join("dist/attachments").exists(),
        "offloaded attachments must not also be copied into dist/"
    );

    drop(minio);
    let _ = fs::remove_dir_all(&tmp);
}
