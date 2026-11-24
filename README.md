# goblon

Build runner for StudioZea. `.github/workflows/studiozea-release.yml` checks out the private
code repo with a token, builds and tests the Windows app and publishes signed releases to
[yopish04/studiozea-update](https://github.com/yopish04/studiozea-update). No source code is
stored in this repo. Setup and secrets: see the top of the workflow file.
