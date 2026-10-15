$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$env:FRAMESHIFT_BUILD_COMMIT = git rev-parse HEAD
$version = (Get-Content -Raw version.txt).Trim()
if ($env:PUBLISH -eq 'true' -and ($env:RELEASE_READY -ne 'true' -or $env:RELEASE_REF -cne "v$version")) { throw 'Publication requires completed M1 and an exact release tag.' }
$env:RUSTFLAGS = "--remap-path-prefix=$((Get-Location).Path)=."
if ($env:BUILD_OS -eq 'linux') { $env:RUSTFLAGS += ' -Clinker=rust-lld -Clinker-flavor=ld.lld' }
cargo fetch --locked
cargo build -p frameshift-cli --release --offline --locked --target $env:BUILD_TARGET
$binary = "target/$env:BUILD_TARGET/release/frameshift"
if ($env:BUILD_OS -eq 'windows') { $binary += '.exe' }
$identity = & $binary version --format json | ConvertFrom-Json
if ($identity.engineVersion -ne $version -or $identity.buildCommit -ne $env:FRAMESHIFT_BUILD_COMMIT) { throw 'Binary build identity differs from the packaged commit.' }
& $binary help
$epoch = git show -s --format=%ct HEAD
New-Item -ItemType Directory -Path target/archives, target/repeat | Out-Null
foreach ($dir in @('target/archives', 'target/repeat')) {
  go -C tools/frameshift-release run . pack --binary "../../$binary" --docs ../.. --out "../../$dir" --version $version --os $env:BUILD_OS --arch $env:BUILD_ARCH --epoch $epoch
}
$first = Get-ChildItem target/archives -File
$second = Get-ChildItem target/repeat -File
if ((Get-FileHash $first.FullName).Hash -ne (Get-FileHash $second.FullName).Hash) { throw 'Repeated archive differs.' }
