param([switch]$Offline, [switch]$SkipBuild)
$ErrorActionPreference = 'Stop'
$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
Push-Location -LiteralPath $repoRoot
try {
    $noticeArgs = @((Join-Path $repoRoot 'tools\make_notices.py'))
    if (-not $Offline) { $noticeArgs += '--online' }
    & python @noticeArgs
    if ($LASTEXITCODE -ne 0) { throw 'Dependency license collection failed.' }
    if (-not $SkipBuild) {
        $buildArgs = @('build', '--release', '--locked', '-j', '2')
        if ($Offline) { $buildArgs += '--offline' }
        & cargo @buildArgs
        if ($LASTEXITCODE -ne 0) { throw 'Release compilation failed.' }
    }
    $manifest = Get-Content -LiteralPath (Join-Path $repoRoot 'Cargo.toml') -Raw
    $version = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    if ($version -notmatch '^\d+\.\d+\.\d+$') { throw 'A stable package version is required.' }
    $output = Join-Path $repoRoot 'dist'
    New-Item -ItemType Directory -Path $output -Force | Out-Null
    $name = "WifiMap-v$version.exe"
    $artifact = Join-Path $output $name
    Copy-Item -LiteralPath (Join-Path $repoRoot 'target\release\WifiMap.exe') -Destination $artifact
    Copy-Item -LiteralPath (Join-Path $repoRoot 'THIRD_PARTY_NOTICES.txt') -Destination $output
    & python (Join-Path $repoRoot 'tools\verify_artifact.py') $artifact --report (Join-Path $output 'artifact-inspection.json')
    if ($LASTEXITCODE -ne 0) { throw 'Artifact verification failed.' }
    $hash = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
    [System.IO.File]::WriteAllText("$artifact.sha256", "$hash  $name`n", [System.Text.Encoding]::ASCII)
    Copy-Item -LiteralPath (Join-Path $repoRoot 'LICENSE') -Destination $output
    Write-Output "Prepared $name and checksum. Nothing was uploaded or published."
} finally {
    Pop-Location
}
