# Reviewed Cargo boundaries and Release Please selectors (D-051, D-058).
function Assert-CargoPolicy {
    param($Metadata, $Policy, [string]$ReleaseVersion, $ReleaseConfig)

    $external = @($metadata.packages | Where-Object { $_.id -notin $metadata.workspace_members })
    $actualPackages = @($external | ForEach-Object { "$($_.name)@$($_.version)" } | Sort-Object -CaseSensitive)
    $expectedPackages = @($policy.packages | ForEach-Object { "$($_.name)@$($_.version)" } | Sort-Object -CaseSensitive)
    if (($actualPackages -join "`n") -cne ($expectedPackages -join "`n")) { throw 'External Cargo package set differs from the reviewed policy.' }
    foreach ($entry in $policy.packages) {
        $package = @($external | Where-Object { $_.name -ceq $entry.name -and $_.version -ceq $entry.version })
        if ($package.Count -ne 1 -or $package[0].source -cne 'registry+https://github.com/rust-lang/crates.io-index') {
            throw 'External Cargo package version or source differs from the reviewed policy.'
        }
        $node = $metadata.resolve.nodes | Where-Object id -CEQ $package[0].id
        if (($node.features -join ',') -cne ($entry.features -join ',')) { throw 'External Cargo features differ from the reviewed policy.' }
        $hasBuild = @($package[0].targets | Where-Object { 'custom-build' -in $_.kind }).Count -ne 0
        if ($hasBuild -ne $entry.buildScript) { throw 'External Cargo build script differs from the reviewed policy.' }
    }
    $allowed = @{
        'frameshift-core' = @('sha2')
        'frameshift-syntax' = @('frameshift-core')
        'frameshift-input' = @('frameshift-core')
        'frameshift-package' = @('frameshift-core', 'frameshift-syntax', 'frameshift-input')
        'frameshift-modules' = @('frameshift-core', 'frameshift-syntax', 'frameshift-input')
        'frameshift-nextjs' = @('frameshift-core', 'frameshift-input', 'frameshift-package')
        'frameshift-transform' = @('frameshift-core')
        'frameshift-emit' = @('frameshift-core', 'frameshift-syntax', 'frameshift-transform')
        'frameshift-engine' = @('frameshift-core', 'frameshift-syntax', 'frameshift-transform', 'frameshift-emit', 'frameshift-input', 'frameshift-package', 'frameshift-nextjs', 'frameshift-modules')
        'frameshift-cli' = @('frameshift-engine')
    }
    foreach ($package in $metadata.packages | Where-Object { $_.id -in $metadata.workspace_members }) {
        if ($package.version -ne $releaseVersion) { throw 'Workspace package version differs from the release version.' }
        foreach ($dependency in $package.dependencies) {
            if ($null -eq $dependency.path) {
                if ($package.name -cne 'frameshift-core' -or $dependency.name -cne 'sha2' -or
                    $dependency.req -cne '=0.11.1' -or $dependency.uses_default_features -or
                    $dependency.features.Count -ne 0 -or $dependency.optional -or
                    $null -ne $dependency.kind -or $null -ne $dependency.target -or
                    $null -ne $dependency.rename -or $null -ne $dependency.registry) {
                    throw 'External Cargo dependency requires an accepted decision and policy update.'
                }
            } elseif ($dependency.name -notin ($metadata.packages | Where-Object { $_.id -in $metadata.workspace_members }).name) {
                throw 'Path dependencies must be reviewed workspace members.'
            }
            if ($allowed.ContainsKey($package.name) -and $dependency.name -notin $allowed[$package.name]) {
                throw "Forbidden dependency from $($package.name) to $($dependency.name)."
            }
        }
        if (-not $allowed.ContainsKey($package.name) -and -not $package.manifest_path.Replace('\', '/').Contains('/tools/')) {
            throw 'Unclassified product crate needs an explicit boundary policy.'
        }
    }
    $extra = @($releaseConfig.packages.'.'.'extra-files' | Where-Object path -CEQ 'Cargo.lock')
    $workspaceNames = @($metadata.packages | Where-Object { $_.id -in $metadata.workspace_members } | Select-Object -ExpandProperty name | Sort-Object -CaseSensitive)
    $selectors = @($workspaceNames | ForEach-Object { "`$.package[?(@.name.value=='$_')].version" })
    $actual = @($extra | Select-Object -ExpandProperty jsonpath | Sort-Object -CaseSensitive)
    if (($actual -join "`n") -cne (($selectors | Sort-Object -CaseSensitive) -join "`n")) {
        throw 'Release Please must update exactly the workspace lockfile packages.'
    }
}
