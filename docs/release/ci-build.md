# Release build on CI

`.github/workflows/release.yml` builds the Windows release zip on a GitHub-hosted runner by calling `tools/package-release.ps1`.

Run it from the Actions tab (Release build, Run workflow), or push a tag matching `v*`. It uploads one artifact, `tessalith-win-x64`, holding `Tessalith-<version>-win-x64.zip` (version from the workspace `Cargo.toml`) and `SHA256SUMS.txt`.

The workflow does not create or edit a GitHub Release; attach the files to a release by hand. The artifact is the CI-built exe that SignPath code signing would sign. No SignPath step exists yet.
