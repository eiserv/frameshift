$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
cargo run -p frameshift-e2 --offline --locked -- write --out target/e2-determinism
$root = (Resolve-Path target/e2-determinism).Path
$files = [string[]](Get-ChildItem -LiteralPath $root -Recurse -File | ForEach-Object { [IO.Path]::GetRelativePath($root, $_.FullName).Replace('\', '/') })
[Array]::Sort($files, [StringComparer]::Ordinal)
$lines = foreach ($file in $files) { "$( (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $root $file)).Hash.ToLowerInvariant())  $file" }
[IO.File]::WriteAllText((Join-Path (Get-Location) 'target/e2-sha256.txt'), ($lines -join "`n") + "`n", [Text.UTF8Encoding]::new($false))
