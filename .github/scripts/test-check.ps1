$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
. "$PSScriptRoot/dependencies.ps1"

$metadata = cargo metadata --offline --locked --format-version 1 | ConvertFrom-Json
$policy = Get-Content -Raw .github/dependencies.json | ConvertFrom-Json
$config = Get-Content -Raw release-please-config.json | ConvertFrom-Json
$version = (Get-Content -Raw version.txt).Trim()
Assert-CargoPolicy $metadata $policy $version $config
$script:cases = 1

function Assert-Rejected {
    param([string]$Name, [scriptblock]$Change, [string]$Reason)
    $m = $metadata | ConvertTo-Json -Depth 100 | ConvertFrom-Json
    $p = $policy | ConvertTo-Json -Depth 100 | ConvertFrom-Json
    $r = $config | ConvertTo-Json -Depth 100 | ConvertFrom-Json
    & $Change $m $p $r
    try {
        Assert-CargoPolicy $m $p $version $r
    } catch {
        if (-not $_.Exception.Message.Contains($Reason)) { throw "Wrong rejection for ${Name}: $_" }
        $script:cases++
        return
    }
    throw "Policy accepted $Name."
}

Assert-Rejected 'an unreviewed transitive version' {
    param($m)
    ($m.packages | Where-Object name -CEQ 'digest').version = '0.11.99'
} 'package set'
Assert-Rejected 'a duplicate replacing another policy entry' {
    param($m, $p)
    $p.packages[1] = $p.packages[0]
} 'package set'
Assert-Rejected 'a foreign package source' {
    param($m)
    ($m.packages | Where-Object name -CEQ 'sha2').source = 'git+https://example.invalid/hash'
} 'source'
Assert-Rejected 'an extra enabled feature' {
    param($m)
    $id = ($m.packages | Where-Object name -CEQ 'sha2').id
    ($m.resolve.nodes | Where-Object id -CEQ $id).features = @('alloc')
} 'features'
Assert-Rejected 'an undeclared build script' {
    param($m)
    $target = ($m.packages | Where-Object name -CEQ 'sha2').targets[0]
    $target.kind = @('custom-build')
} 'build script'
Assert-Rejected 'a relaxed direct version' {
    param($m)
    (($m.packages | Where-Object name -CEQ 'frameshift-core').dependencies | Where-Object name -CEQ 'sha2').req = '^0.11.1'
} 'External Cargo dependency'
Assert-Rejected 'default features on the direct dependency' {
    param($m)
    (($m.packages | Where-Object name -CEQ 'frameshift-core').dependencies | Where-Object name -CEQ 'sha2').uses_default_features = $true
} 'External Cargo dependency'
Assert-Rejected 'direct library access from the syntax layer' {
    param($m)
    $core = $m.packages | Where-Object name -CEQ 'frameshift-core'
    ($m.packages | Where-Object name -CEQ 'frameshift-syntax').dependencies += $core.dependencies
} 'External Cargo dependency'
Assert-Rejected 'an unreviewed local dependency' {
    param($m)
    (($m.packages | Where-Object name -CEQ 'frameshift-syntax').dependencies | Where-Object name -CEQ 'frameshift-core').name = 'foreign'
} 'Path dependencies'
Assert-Rejected 'a lockfile-wide release updater' {
    param($m, $p, $r)
    ($r.packages.'.'.'extra-files' | Where-Object path -CEQ 'Cargo.lock')[0].jsonpath = '$.package[*].version'
} 'exactly the workspace'
Assert-Rejected 'a missing workspace release updater' {
    param($m, $p, $r)
    $r.packages.'.'.'extra-files' = @($r.packages.'.'.'extra-files' | Select-Object -SkipLast 1)
} 'exactly the workspace'
Assert-Rejected 'an unexpected workspace version' {
    param($m)
    ($m.packages | Where-Object name -CEQ 'frameshift-core').version = '9.9.9'
} 'Workspace package version'

Write-Output "$script:cases dependency and release policy checks passed."
