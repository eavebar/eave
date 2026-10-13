# The AUR package

`eave/` is the [AUR package `eave`](https://aur.archlinux.org/packages/eave): it ships `eave-build` only, built from a release tarball ([D-53](../../docs/decisions.md)). CI builds it from every commit.

## Releasing

1. Bump `version` in `crates/*/Cargo.toml` and `plugins/*/Cargo.toml` where it changed, merge to `main`.
2. Tag and push: `git tag vX.Y.Z && git push origin vX.Y.Z`. A packaged eave-build builds `eaveshell/eave@vX.Y.Z`, so the tag must exist before the package does.
3. In `dist/aur/eave/`: set `pkgver=X.Y.Z` and `pkgrel=1`, run `updpkgsums` (from `pacman-contrib`), then `makepkg --printsrcinfo > .SRCINFO`. Commit.
4. Publish:

   ```sh
   git clone ssh://aur@aur.archlinux.org/eave.git aur-eave
   cp dist/aur/eave/{PKGBUILD,eave.install,.SRCINFO} aur-eave/
   cd aur-eave && git commit -am "eave X.Y.Z" && git push
   ```

A packaging-only fix bumps `pkgrel` instead of `pkgver`.
