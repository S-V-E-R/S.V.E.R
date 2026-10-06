# Real Git round trip against a disposable local remote; no GitHub or server traffic.
$ErrorActionPreference = 'Stop'
$script = Join-Path $PSScriptRoot 'publish.ps1'
$fixture = Join-Path ([IO.Path]::GetTempPath()) "sver-publish-$([guid]::NewGuid().ToString('N'))"
function Run-Git([string[]]$Arguments) {
    & git @Arguments
    if ($LASTEXITCODE -ne 0) { throw "Fixture Git command failed: $Arguments" }
}
function Check([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
New-Item -ItemType Directory -Path $fixture | Out-Null
Push-Location $fixture
try {
    Run-Git @('init', '--bare', 'remote.git')
    Run-Git @('init', '-b', 'main', 'work')
    Set-Location work
    Run-Git @('config', 'user.name', 'Publish check')
    Run-Git @('config', 'user.email', 'publish-check@example.invalid')
    Run-Git @('config', 'commit.gpgsign', 'false')
    Run-Git @('config', 'core.autocrlf', 'true')
    Run-Git @('remote', 'add', 'origin', '../remote.git')
    Set-Content -LiteralPath tracked.txt -Value initial
    Run-Git @('add', 'tracked.txt')
    Run-Git @('commit', '-m', 'Fixture baseline')
    & pwsh -NoProfile -File $script 2>&1 | Out-Null
    Check ($LASTEXITCODE -ne 0) 'Must refuse publishing main.'
    Run-Git @('switch', '-c', 'feature/publish-check')
    Set-Content -LiteralPath staged.txt -Value selected
    Set-Content -LiteralPath tracked.txt -Value 'Leave unstaged work alone'
    Run-Git @('add', 'staged.txt')
    & pwsh -NoProfile -File $script -Message 'Publish selected change'
    Check ($LASTEXITCODE -eq 0) 'Publish failed.'
    $head = Run-Git @('rev-parse', 'HEAD')
    $remoteHead = Run-Git @('--git-dir=../remote.git', 'rev-parse', 'refs/heads/feature/publish-check')
    Check ($head -eq $remoteHead) 'Remote did not receive the exact commit.'
    Check ((Run-Git @('show', 'HEAD:tracked.txt')) -eq 'initial') 'Unstaged changes were committed.'
    Check ((Get-Content -LiteralPath tracked.txt) -eq 'Leave unstaged work alone') 'Worktree was changed.'
    & pwsh -NoProfile -File $script
    Check ($LASTEXITCODE -eq 0) 'An already committed branch must be pushable without another commit.'
    Check ((Run-Git @('rev-parse', 'HEAD')) -eq $head) 'Retry created another commit.'
    Set-Content -LiteralPath ChangeLog.md -Value private
    Run-Git @('add', 'ChangeLog.md')
    & pwsh -NoProfile -File $script -Message 'Must reject private file' 2>&1 | Out-Null
    Check ($LASTEXITCODE -ne 0) 'Private file was not rejected.'
    Check ((Run-Git @('rev-parse', 'HEAD')) -eq $head) 'Private-file rejection created a commit.'
    Write-Host 'Publish checks passed: branch guard, staged-only commit, push, retry and private-file rejection.'
} finally {
    Pop-Location
    $resolved = (Resolve-Path -LiteralPath $fixture).Path
    $temp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([IO.Path]::DirectorySeparatorChar)
    if ((Split-Path -Parent $resolved) -eq $temp -and (Split-Path -Leaf $resolved) -like 'sver-publish-*') {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}
