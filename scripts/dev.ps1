param([ValidateSet('api','web','database','test','mail')][string]$Part='api', [string]$ConfigPath="$env:USERPROFILE\SVER-dev\login.env")
$ErrorActionPreference='Stop'
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
if (-not(Test-Path -LiteralPath $ConfigPath)) { throw "Create the external environment file described in README.md first." }
foreach($line in Get-Content -LiteralPath $ConfigPath) {
    if($line -match '^([A-Z][A-Z0-9_]*)=(.*)$') { [Environment]::SetEnvironmentVariable($Matches[1],$Matches[2],'Process') }
}
switch($Part) {
    database { docker compose --env-file $ConfigPath up -d --wait }
    api {
        Set-Location apps\api
        cargo build
        if($LASTEXITCODE -ne 0){exit $LASTEXITCODE}
        New-Item -ItemType Directory -Path "$Root\tmp" -Force | Out-Null
        # Run a copy so Windows does not lock Cargo's output during builds/tests.
        Copy-Item -LiteralPath 'target\debug\sver.exe' -Destination "$Root\tmp\sver-api.exe" -Force
        Set-Location $Root
        & "$Root\tmp\sver-api.exe"
    }
    web { Set-Location apps\web; corepack pnpm dev }
    test {
        Set-Location apps\api
        cargo test --test reserved_routes
        if($LASTEXITCODE -eq 0){ cargo test --test login -- --nocapture }
        if($LASTEXITCODE -eq 0){ cargo test --test profiles -- --nocapture }
        if($LASTEXITCODE -eq 0){ cargo test --test streams -- --nocapture }
        if($LASTEXITCODE -eq 0){ cargo test --lib --bins }
    }
    mail { Set-Location apps\api; cargo run -- preview-mail }
}
if($LASTEXITCODE -ne 0){exit $LASTEXITCODE}
