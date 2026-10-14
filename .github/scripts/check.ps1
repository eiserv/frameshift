param([switch]$Conventions)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

if ($Conventions) {
    $pattern = '^(feat|fix|perf|refactor|docs|test|build|ci|style|chore|revert)(\([a-z0-9][a-z0-9.-]*\))?!?: [^\s].*$'
    $event = Get-Content -Raw -LiteralPath $env:GITHUB_EVENT_PATH | ConvertFrom-Json
    if ($env:GITHUB_EVENT_NAME -eq 'pull_request') {
        if ($event.pull_request.title -cnotmatch $pattern) { throw 'Pull request title must be a Conventional Commit.' }
        # Read PR commit subjects through GitHub; never interpolate them into shell code.
        $subjects = gh api --paginate "repos/$env:GITHUB_REPOSITORY/pulls/$($event.number)/commits" --jq '.[].commit.message | split("\n")[0]'
    } elseif ($env:GITHUB_EVENT_NAME -eq 'push' -and $event.before -ne ('0' * 40)) {
        $subjects = git log --format=%s "$($event.before)..$($event.after)"
    } else {
        $subjects = git log -1 --format=%s
    }
    foreach ($subject in $subjects) {
        if ($subject -cnotmatch $pattern) { throw 'Commit subject must be a Conventional Commit.' }
    }
}

$metadata = cargo metadata --offline --locked --no-deps --format-version 1 | ConvertFrom-Json
$allowed = @{
    'frameshift-core' = @()
    'frameshift-syntax' = @('frameshift-core')
    'frameshift-transform' = @('frameshift-core')
    'frameshift-emit' = @('frameshift-core', 'frameshift-syntax', 'frameshift-transform')
    'frameshift-engine' = @('frameshift-core', 'frameshift-syntax', 'frameshift-transform', 'frameshift-emit')
    'frameshift-cli' = @('frameshift-engine')
}
$releaseVersion = (Get-Content -Raw version.txt).Trim()
foreach ($package in $metadata.packages) {
    if ($package.version -ne $releaseVersion) { throw 'Workspace package version differs from the release version.' }
    foreach ($dependency in $package.dependencies) {
        if ($null -eq $dependency.path) { throw 'External Cargo dependencies require a new accepted decision and checker update.' }
        if ($allowed.ContainsKey($package.name) -and $dependency.name -notin $allowed[$package.name]) {
            throw "Forbidden dependency from $($package.name) to $($dependency.name)."
        }
    }
    if (-not $allowed.ContainsKey($package.name) -and -not $package.manifest_path.Replace('\', '/').Contains('/tools/')) {
        throw 'Unclassified product crate needs an explicit boundary policy.'
    }
}
$manifest = Get-Content -Raw .release-please-manifest.json | ConvertFrom-Json
if ($manifest.'.' -ne $releaseVersion) { throw 'Release manifest differs from workspace version.' }

$invalidActions = Get-ChildItem .github/workflows/*.yml | Select-String -Pattern '^\s*-?\s*uses:\s+(?!\./)[^@\s]+@(?![0-9a-f]{40}(?:\s|$))'
if ($invalidActions) { throw 'Every external Action must use a full commit SHA.' }
Write-Output 'Crate boundaries, package versions, and Action pins are valid.'
