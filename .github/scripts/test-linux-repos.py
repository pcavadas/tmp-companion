#!/usr/bin/env python3
"""Exercise real repository tools and package clients; never push production repos."""
import functools
import hashlib
import http.server
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[2]
BUILD = ROOT / "scripts/build-linux-repos.sh"
PUBLISH = ROOT / "scripts/publish-linux-repos.sh"
COMPOSE = ROOT / "scripts/compose-pages.sh"

def run(*args, env=None, check=True, capture=False):
    return subprocess.run([str(a) for a in args], env=env, check=check, text=True,
                          stdout=subprocess.PIPE if capture else None)

def output(*args, env=None):
    return run(*args, env=env, capture=True).stdout.strip()

def file_digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def digest(directory):
    return {str(p.relative_to(directory)): file_digest(p)
            for p in sorted(directory.rglob("*")) if p.is_file()}

def package(directory, suffix):
    found = list(directory.glob("*." + suffix))
    assert len(found) == 1, (directory, found)
    return found[0]

def build(deb, rpm, dest, fpr, env, check=True):
    return run("bash", BUILD, deb, rpm, dest, fpr, env=env, check=check)

def signatures(dest, env):
    if (dest / "apt").exists():
        run("gpg", "--batch", "--verify", dest / "apt/dists/stable/InRelease", env=env)
        run("gpg", "--batch", "--verify", dest / "apt/dists/stable/Release.gpg",
            dest / "apt/dists/stable/Release", env=env)
    if (dest / "rpm").exists():
        run("gpg", "--batch", "--verify", dest / "rpm/repodata/repomd.xml.asc",
            dest / "rpm/repodata/repomd.xml", env=env)

def artifact_selection(work, env):
    mock = work / "mock-gh"
    mock.mkdir()
    gh = mock / "gh"
    gh.write_text("""#!/bin/sh
printf '%s' "$MOCK_ARTIFACTS"
exit "${MOCK_EXIT:-0}"
""")
    gh.chmod(0o755)
    base = dict(env, PATH=str(mock) + os.pathsep + env["PATH"],
                GITHUB_REPOSITORY="fixture/repo", GITHUB_RUN_ID="123")
    script = ROOT / ".github/scripts/select-linux-artifacts.sh"
    cases = [("", "deb=false\nrpm=false"),
             ("linux-deb\tfalse\n", "deb=true\nrpm=false"),
             ("linux-rpm\tfalse\n", "deb=false\nrpm=true"),
             ("linux-deb\tfalse\nlinux-rpm\tfalse\n", "deb=true\nrpm=true")]
    for inventory, expected in cases:
        assert output("bash", script, env=dict(base, MOCK_ARTIFACTS=inventory)) == expected
    for inventory, code in [("", "42"), ("linux-deb\ttrue\n", "0"),
                            ("linux-rpm\tinvalid\n", "0")]:
        result = run("bash", script, env=dict(base, MOCK_ARTIFACTS=inventory,
                     MOCK_EXIT=code), check=False, capture=True)
        assert result.returncode != 0 and not result.stdout
    print("PASS: artifact inventory both/one/none, API failure and expired artifacts", flush=True)

def regressions(work, deb, rpm, fpr, env):
    artifact_selection(work, env)
    missing, dest = work / "absent", work / "combined"
    template = (ROOT / "packaging/apt/conf/distributions").read_bytes()
    build(deb / "v1", rpm / "v1", dest, fpr, env)
    signatures(dest, env)
    old_rpm = digest(dest / "rpm")
    # No working DB is carried into the next build.
    assert not (dest / "apt/db").exists()
    build(deb / "v2", missing, dest, fpr, env)
    assert digest(dest / "rpm") == old_rpm
    published_debs = list((dest / "apt").rglob("*.deb"))
    assert len(published_debs) == 1
    # reprepro normalizes filenames from control metadata; verify the actual bytes.
    assert file_digest(published_debs[0]) == file_digest(package(deb / "v2", "deb"))
    old_apt = digest(dest / "apt")
    build(missing, rpm / "v2", dest, fpr, env)
    assert digest(dest / "apt") == old_apt
    assert [p.name for p in (dest / "rpm").glob("*.rpm")] == [package(rpm / "v2", "rpm").name]
    build(deb / "v2", rpm / "v2", dest, fpr, env)
    signatures(dest, env)
    before = digest(dest)
    # Make forbidden tools fail: the empty-artifact path must never call one.
    forbidden = work / "forbidden"
    forbidden.mkdir()
    for tool in ["gpg", "git", "reprepro", "createrepo_c", "rpm"]:
        script = forbidden / tool
        script.write_text("#!/bin/sh\nexit 91\n")
        script.chmod(0o755)
    empty_env = dict(env, PATH=str(forbidden) + os.pathsep + env["PATH"],
                     LINUX_REPO_REMOTE="invalid://must-not-be-used")
    build(missing, missing, dest, "invalid-unused-key", empty_env)
    run("bash", PUBLISH, missing, missing, "invalid-unused-key", "none", env=empty_env)
    assert digest(dest) == before
    bad_deb, bad_rpm = work / "bad-deb", work / "bad-rpm"
    bad_deb.mkdir()
    bad_rpm.mkdir()
    (bad_deb / "broken.deb").write_bytes(b"not a deb")
    (bad_rpm / "broken.rpm").write_bytes(b"not an rpm")
    assert build(bad_deb, rpm / "v2", dest, fpr, env, check=False).returncode != 0
    assert digest(dest) == before
    # A valid APT build followed by a bad RPM cannot replace either format.
    assert build(deb / "v2", bad_rpm, dest, fpr, env, check=False).returncode != 0
    assert digest(dest) == before
    assert (ROOT / "packaging/apt/conf/distributions").read_bytes() == template

    remote, seed = work / "remote.git", work / "website"
    run("git", "init", "--quiet", "--bare", remote)
    run("git", "init", "--quiet", "-b", "main", seed)
    run("git", "-C", seed, "config", "user.name", "Repository test")
    run("git", "-C", seed, "config", "user.email", "repo-test@example.invalid")
    (seed / "docs").mkdir()
    (seed / "docs/index.html").write_text("current website\n")
    (seed / "docs/.nojekyll").touch()
    run("git", "-C", seed, "add", "-A")
    run("git", "-C", seed, "commit", "--quiet", "-m", "test: website")
    run("git", "-C", seed, "remote", "add", "origin", remote)
    run("git", "-C", seed, "push", "--quiet", "origin", "main")
    main_before = output("git", "--git-dir", remote, "rev-parse", "main")
    remote_uri = remote.as_uri()  # file:// makes Git honor shallow clone depth.
    empty_site = work / "empty-site"
    run("bash", COMPOSE, seed / "docs", remote_uri, empty_site, env=env)
    assert (empty_site / "index.html").read_text() == "current website\n"
    assert (empty_site / ".nojekyll").exists()

    branch = "codex/linux-package-repos"
    pub_env = dict(env, LINUX_REPO_REMOTE=remote_uri)
    run("bash", PUBLISH, deb / "v1", missing, fpr, "0.0.1", env=pub_env)
    first_publication = output("git", "--git-dir", remote, "rev-parse", branch)
    apt_tree = output("git", "--git-dir", remote, "rev-parse", branch + ":apt")
    run("bash", PUBLISH, missing, rpm / "v1", fpr, "0.0.1", env=pub_env)
    assert output("git", "--git-dir", remote, "rev-parse", branch + ":apt") == apt_tree
    rpm_tree = output("git", "--git-dir", remote, "rev-parse", branch + ":rpm")
    run("bash", PUBLISH, deb / "v2", missing, fpr, "0.0.2", env=pub_env)
    assert output("git", "--git-dir", remote, "rev-parse", branch + ":rpm") == rpm_tree
    apt_tree = output("git", "--git-dir", remote, "rev-parse", branch + ":apt")
    run("bash", PUBLISH, missing, rpm / "v2", fpr, "0.0.2", env=pub_env)
    assert output("git", "--git-dir", remote, "rev-parse", branch + ":apt") == apt_tree
    run("bash", PUBLISH, deb / "v2", rpm / "v2", fpr, "0.0.2-repeat", env=pub_env)
    assert output("git", "--git-dir", remote, "rev-parse", "main") == main_before
    generated = output("git", "--git-dir", remote, "rev-parse", branch)
    run("git", "--git-dir", remote, "merge-base", "--is-ancestor", first_publication, generated)
    run("bash", PUBLISH, missing, missing, "invalid", "none", env=pub_env)
    assert output("git", "--git-dir", remote, "rev-parse", branch) == generated
    site = work / "composed-site"
    run("bash", COMPOSE, seed / "docs", remote_uri, site, env=env)
    assert (site / "index.html").read_text() == "current website\n"
    assert (site / ".nojekyll").exists()
    assert not (site / ".git").exists()
    site_debs = list((site / "apt").rglob("*.deb"))
    site_rpms = list((site / "rpm").glob("*.rpm"))
    assert len(site_debs) == len(site_rpms) == 1
    assert file_digest(site_debs[0]) == file_digest(package(deb / "v2", "deb"))
    assert file_digest(site_rpms[0]) == file_digest(package(rpm / "v2", "rpm"))
    signatures(site, env)
    print("PASS: both/one/no artifacts, pruning without DB, repeat builds, failure preservation, clean template, generated branch and Pages composition", flush=True)

class FreshHandler(http.server.SimpleHTTPRequestHandler):
    def send_head(self):
        # Tamper checks must fetch bytes even when two writes share an mtime second.
        for header in ["If-Modified-Since", "If-None-Match"]:
            if header in self.headers:
                del self.headers[header]
        return super().send_head()

def native_client(work, fmt, artifact, fpr, env):
    dest, missing = work / "served", work / "absent"
    build(artifact / "v1" if fmt == "apt" else missing,
          artifact / "v1" if fmt == "rpm" else missing, dest, fpr, env)
    signatures(dest, env)
    handler = functools.partial(FreshHandler, directory=str(dest))
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_port}"
    privilege = [] if os.geteuid() == 0 else ["sudo"]
    name = "tmp-companion"
    if fmt == "apt":
        source = Path("/etc/apt/sources.list.d/tmp-companion-repo-test.list")
        key = Path("/usr/share/keyrings/tmp-companion-repo-test.gpg")
        run(*privilege, "install", "-m", "644", dest / "apt/pubkey.gpg", key)
        text = f"deb [signed-by={key}] {url}/apt stable main\n"
    else:
        source, key = Path("/etc/yum.repos.d/tmp-companion-repo-test.repo"), None
        text = (dest / "rpm/tmp-companion.repo").read_text()
        text = text.replace("[tmp-companion]", "[tmp-companion-repo-test]")
        text = text.replace("https://pcavadas.github.io/tmp-companion/rpm", url + "/rpm")
        text += "metadata_expire=0\nskip_if_unavailable=0\n"
    config = work / "source-config"
    config.write_text(text)
    run(*privilege, "install", "-m", "644", config, source)

    def refresh(check=True):
        if fmt == "apt":
            return run(*privilege, "apt-get", "update", "--error-on=any",
                       "-o", f"Dir::Etc::sourcelist={source}",
                       "-o", "Dir::Etc::sourceparts=-", "-o", "APT::Get::List-Cleanup=0", check=check)
        return run(*privilege, "dnf", "--refresh", "--repo=tmp-companion-repo-test",
                   "makecache", "-y", check=check)

    try:
        refresh()
        if fmt == "apt":
            v1 = output("dpkg-deb", "-f", package(artifact / "v1", "deb"), "Version")
            v2 = output("dpkg-deb", "-f", package(artifact / "v2", "deb"), "Version")
            assert output("dpkg-deb", "-f", package(artifact / "v1", "deb"), "Package") == name
            run(*privilege, "apt-get", "install", "-y", name + "=" + v1)
            assert output("dpkg-query", "-W", "-f=${Version}", name) == v1
        else:
            v1 = output("rpm", "-qp", "--qf", "%{VERSION}-%{RELEASE}", package(artifact / "v1", "rpm"))
            v2 = output("rpm", "-qp", "--qf", "%{VERSION}-%{RELEASE}", package(artifact / "v2", "rpm"))
            assert output("rpm", "-qp", "--qf", "%{NAME}", package(artifact / "v1", "rpm")) == name
            run(*privilege, "dnf", "install", "-y", name)
            assert output("rpm", "-q", "--qf", "%{VERSION}-%{RELEASE}", name) == v1
        assert v1 != v2
        run("bash", ROOT / ".github/scripts/check-linux-payload.sh")
        # Force native clients to reject tampered metadata instead of reuse a cache.
        metadata = dest / ("apt/dists/stable/InRelease" if fmt == "apt" else "rpm/repodata/repomd.xml")
        original = metadata.read_bytes()
        if fmt == "apt":
            assert b"Label: TMP Companion" in original
            metadata.write_bytes(original.replace(b"Label: TMP Companion", b"Label: altered"))
        else:
            assert b"</repomd>" in original
            metadata.write_bytes(original.replace(b"</repomd>", b"<!-- altered --></repomd>"))
        assert refresh(check=False).returncode != 0, "native client accepted tampered metadata"
        metadata.write_bytes(original)
        build(artifact / "v2" if fmt == "apt" else missing,
              artifact / "v2" if fmt == "rpm" else missing, dest, fpr, env)
        signatures(dest, env)
        refresh()
        if fmt == "apt":
            run(*privilege, "apt-get", "install", "--only-upgrade", "-y", name)
            assert output("dpkg-query", "-W", "-f=${Version}", name) == v2
        else:
            run(*privilege, "dnf", "--refresh", "upgrade", "-y", name)
            assert output("rpm", "-q", "--qf", "%{VERSION}-%{RELEASE}", name) == v2
        run("bash", ROOT / ".github/scripts/check-linux-payload.sh")
        print(f"PASS: {fmt} signed repository install {v1}, tamper rejection, upgrade {v2}, real payload", flush=True)
    finally:
        run(*privilege, "apt-get" if fmt == "apt" else "dnf", "remove", "-y", name, check=False)
        run(*privilege, "rm", "-f", source, *([key] if key else []))
        server.shutdown()
        server.server_close()

def main():
    fmt, artifacts = sys.argv[1], Path(sys.argv[2]).resolve()
    assert fmt in {"apt", "rpm"}
    with tempfile.TemporaryDirectory(prefix="tmp-companion-repo-test-") as directory:
        work = Path(directory)
        gnupg = work / "gnupg"
        gnupg.mkdir(mode=0o700)
        env = dict(os.environ, GNUPGHOME=str(gnupg))
        try:
            run("gpg", "--batch", "--pinentry-mode", "loopback", "--passphrase", "",
                "--quick-generate-key", "TMP Companion CI <repo-test@example.invalid>",
                "rsa4096", "sign", "0", env=env)
            keys = output("gpg", "--batch", "--list-secret-keys", "--with-colons", env=env)
            fpr = next(line.split(":")[9] for line in keys.splitlines() if line.startswith("fpr:"))
            if fmt == "apt":
                regressions(work, artifacts / "linux-deb",
                            artifacts / "linux-rpm", fpr, env)
            native_client(work, fmt, artifacts / ("linux-deb" if fmt == "apt" else "linux-rpm"),
                          fpr, env)
        finally:
            run("gpgconf", "--homedir", gnupg, "--kill", "gpg-agent", check=False)

if __name__ == "__main__":
    main()
