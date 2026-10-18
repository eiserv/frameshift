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

$metadata = cargo metadata --offline --locked --format-version 1 | ConvertFrom-Json
$policy = Get-Content -Raw .github/dependencies.json | ConvertFrom-Json
$releaseVersion = (Get-Content -Raw version.txt).Trim()
$releaseConfig = Get-Content -Raw release-please-config.json | ConvertFrom-Json
. "$PSScriptRoot/dependencies.ps1"
Assert-CargoPolicy $metadata $policy $releaseVersion $releaseConfig
$manifest = Get-Content -Raw .release-please-manifest.json | ConvertFrom-Json
if ($manifest.'.' -ne $releaseVersion) { throw 'Release manifest differs from workspace version.' }

$invalidActions = Get-ChildItem .github/workflows/*.yml | Select-String -Pattern '^\s*-?\s*uses:\s+(?!\./)[^@\s]+@(?![0-9a-f]{40}(?:\s|$))'
if ($invalidActions) { throw 'Every external Action must use a full commit SHA.' }
Write-Output 'Crate boundaries, dependency policy, release versions/selectors, and Action pins are valid.'
