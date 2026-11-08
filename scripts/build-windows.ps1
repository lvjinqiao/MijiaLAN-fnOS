param(
    [ValidateSet('debug', 'release')][string]$Profile = 'release',
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')][string]$Target = 'x86_64-pc-windows-msvc',
    [switch]$Nsis
)
$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '..')
$scratch = $null
try {
    if ($Profile -eq 'release' -and -not $env:GPUI_FXC_PATH) {
        $sdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
        $fxc = Get-ChildItem $sdk -Filter fxc.exe -Recurse -ErrorAction SilentlyContinue |
            Where-Object { $_.Directory.Name -eq 'x64' } | Sort-Object FullName -Descending | Select-Object -First 1
        if (-not $fxc) { throw 'Windows SDK fxc.exe is required for the GPUI Release build.' }
        $env:GPUI_FXC_PATH = $fxc.FullName
    }
    $arguments = @('build', '--locked', '--target', $Target, '-p', 'mijialan-desktop', '-p', 'mijialan-cli')
    if ($Profile -eq 'release') { $arguments += '--release' }
    & cargo @arguments
    if ($LASTEXITCODE -ne 0) { throw 'Rust build failed' }
    $metadata = (& cargo metadata --no-deps --format-version 1 | ConvertFrom-Json)
    $version = ($metadata.packages | Where-Object name -eq 'mijialan-desktop').version
    $architecture = if ($Target.StartsWith('aarch64')) { 'arm64' } else { 'x86_64' }
    $suffix = if ($Profile -eq 'debug') { '-debug' } else { '' }
    $name = "MijiaLAN-Rust-$version-windows-$architecture$suffix"
    $out = Join-Path (Get-Location) 'artifacts'
    New-Item -ItemType Directory -Force $out | Out-Null
    $scratch = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString('N'))
    $stage = Join-Path $scratch $name
    New-Item -ItemType Directory -Force $stage | Out-Null
    $binaries = Join-Path $metadata.target_directory "$Target/$Profile"
    Copy-Item (Join-Path $binaries 'mijialan-rust.exe'), (Join-Path $binaries 'mijia-rust.exe') $stage
    Copy-Item LICENSE, README.md, PRIVACY.md, THIRD_PARTY_NOTICES.md $stage
    Copy-Item licenses (Join-Path $stage 'licenses') -Recurse
    Copy-Item docs (Join-Path $stage 'docs') -Recurse
    Copy-Item packaging/windows/START-HERE.txt $stage
    $licenseMetadata = Join-Path $scratch 'cargo-metadata.json'
    & cargo metadata --locked --format-version 1 --filter-platform $Target | Set-Content -Encoding utf8 $licenseMetadata
    if ($LASTEXITCODE -ne 0) { throw 'Cargo license metadata failed' }
    & python scripts/collect-cargo-licenses.py $licenseMetadata (Join-Path $stage 'licenses/Cargo')
    if ($LASTEXITCODE -ne 0) { throw 'Cargo license collection failed' }
    $zip = Join-Path $out "$name.zip"
    Compress-Archive -Path $stage -DestinationPath $zip -Force
    $compiler = Get-Command makensis.exe -ErrorAction SilentlyContinue
    $compilerPath = if ($compiler) { $compiler.Source } else { $null }
    if (-not $compilerPath) {
        $default = Join-Path ${env:ProgramFiles(x86)} 'NSIS/makensis.exe'
        if (Test-Path $default) { $compilerPath = $default }
    }
    if ($Nsis -and $compilerPath) {
        $setup = Join-Path $out "$name-Setup.exe"
        & $compilerPath /V2 "/DAPP_VERSION=$version" "/DSOURCE_DIR=$stage" "/DOUTPUT_FILE=$setup" packaging/windows/installer.nsi
        if ($LASTEXITCODE -ne 0) { throw 'NSIS packaging failed' }
        Get-FileHash -Algorithm SHA256 $zip, $setup | ForEach-Object { "$($_.Hash.ToLowerInvariant())  $([IO.Path]::GetFileName($_.Path))" } | Set-Content (Join-Path $out "$name.sha256")
    } else {
        Get-FileHash -Algorithm SHA256 $zip | ForEach-Object { "$($_.Hash.ToLowerInvariant())  $([IO.Path]::GetFileName($_.Path))" } | Set-Content (Join-Path $out "$name.sha256")
        Write-Host 'Portable ZIP created. Use the Linux packaging container to generate its per-user MSI.'
        if ($Nsis) { Write-Warning 'NSIS was not found. Its test installer remains experimental pending Defender validation.' }
    }
    Write-Host $zip
} finally {
    if ($scratch -and (Test-Path $scratch)) { Remove-Item $scratch -Recurse -Force }
    Pop-Location
}
