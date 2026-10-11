# Preparing release downloads

This workflow prepares review artifacts. It never publishes a GitHub release or
changes repository visibility.

1. Finish review and tests, set `BUILD_VERSION`, and commit the exact source.
2. Install Rust 1.89, Python 3.11 or newer, Git, and the platform build tools.
   macOS also requires Xcode command-line tools and macOS 14 or newer for the
   optional native speech helper. Linux graphics/audio build packages are listed
   in `.github/workflows/release.yml`.
3. Run `python3 tools/test_package_release.py`.
4. Run `python3 tools/package-release.py /absolute/empty/output-directory`.
   On Windows, use `python` and a Windows output path. Output must be outside the
   repository. The first run downloads the complete locked dependency source.
5. Inspect the artifacts, verify checksums, and test extracted executables on the
   intended operating system before approving publication.

The manual **Prepare release downloads** GitHub Actions workflow performs tests
and packages macOS, Linux and Windows as retained workflow artifacts. These are
configured targets until their workflow jobs pass; configuration alone does not
prove the binaries work. It has read-only repository permissions and no release
publishing step. Linux builds use Ubuntu 22.04 and need the matching system
window/audio libraries; these are not fully static binaries.

Each platform produces separate `Arkline-VERSION-TARGET-client` and `-server`
archives (`.zip` on Windows, `.tar.gz` elsewhere), a `-source.tar.gz`,
`RELEASE.json`, and `SHA256SUMS`. Client downloads include the local server. The
standalone server download also includes the CLI and ledger verifier. Build
identity comes only from `BUILD_VERSION`; `RELEASE.json` records its source
revision, target and compiler. `--allow-dirty` is for local review only and marks
uncommitted source explicitly; normal release builds require a clean checkout.

The source archive contains the exact source snapshot used for compilation,
Cargo.lock, toolchain/build scripts, original assets and all vendored dependency
sources with Cargo configuration for offline rebuilding. Extract it and run
`cargo build --workspace --locked --offline --release` with the documented
platform prerequisites. System libraries, compiler and Apple SDKs remain
external prerequisites. The source archive and its matching binary downloads
must be made available together; do not substitute an unrelated upstream branch.

All downloads contain the first-party license, trademarks document and dependency
notices. `ThirdParty/` collects original license/notice files, including bundled
font terms, and a machine-readable inventory. It includes all locked platform
and build dependencies, so it is a superset of any one executable. This is notice
collection, not a replacement for license/asset review. Full dependency sources
remain in the matching source archive even when a crate lacks a separate notice
file.

The macOS `.app` embeds its license, notices and source manifest before ad-hoc
signing. This verifies local bundle integrity but is **not Developer ID signing
or Apple notarization**. No public-distribution trust claim is made.

Checksum verification on macOS: `shasum -a 256 -c SHA256SUMS`. On Linux:
`sha256sum -c SHA256SUMS`. On Windows, compare `Get-FileHash -Algorithm SHA256`
results against that file. Checksums identify file contents; they do not establish
a trusted publisher by themselves.
