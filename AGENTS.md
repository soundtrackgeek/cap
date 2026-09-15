## Development Workflow

**IMPORTANT**: After any code change, bug fix, or feature addition/removal, you MUST complete all of these steps:

1. **Update README.md** if the change affects:
   - Usage examples or commands
   - Installation instructions
   - Configuration options
   - Available features

2. **Update CHANGELOG.md**:
   - Add new version number following semantic versioning (MAJOR.MINOR.PATCH)
   - Add entry under appropriate category (Added, Changed, Fixed, Removed)
   - Include date in format YYYY-MM-DD

3. **Commit and push changes**:
   - Use `git add` to stage all modified files (README.md, CHANGELOG.md, and code files)
   - Create descriptive commit message following existing style
   - Push to remote repository with `git push`

4. **Publish a GitHub release for every version bump**:
   - Keep `Cargo.toml`, `Cargo.lock`, and `CHANGELOG.md` on the same version.
   - Complete formatting, strict Clippy, workspace tests, and Windows installation/update checks before releasing.
   - Commit the version bump and all release changes, then build a clean archive with `scripts/package.ps1` using the pinned Capsule core revision.
   - Create and push a `v<version>` tag on that exact commit. Publish a GitHub release for that tag with the Windows ZIP and its `.zip.sha256` file attached.
   - Use the version's changelog entry as release notes. Mark versions containing a prerelease suffix (such as `-dev.32`) as prereleases.
   - Verify that the release and both assets are downloadable, and that `cap update --check` can discover the version. Do not leave a version bump with only a source push.
   - Keep existing tags and release assets immutable; corrections require another version bump and release.
