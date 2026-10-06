# Commit explicitly staged changes and push the current feature branch. No merge, pull or deploy.
[CmdletBinding()]
param([string]$Message, [string]$Remote = 'origin')
$ErrorActionPreference = 'Stop'

function Invoke-Checked([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command failed (exit $LASTEXITCODE). Stopped without continuing." }
}

$repo = Invoke-Checked git @('rev-parse', '--show-toplevel')
Push-Location $repo
try {
    $branch = Invoke-Checked git @('symbolic-ref', '--quiet', '--short', 'HEAD')
    if ($branch -in @('main', 'master')) { throw 'Publish from a feature branch, not main or master.' }
    if ($Remote -notmatch '^[A-Za-z0-9][A-Za-z0-9._/-]*$') { throw 'Invalid remote name.' }
    Invoke-Checked git @('remote', 'get-url', $Remote) | Out-Null
    $staged = @(Invoke-Checked git @('diff', '--cached', '--name-only'))
    foreach ($file in $staged) {
        $name = Split-Path -Leaf $file
        if ($file -in @('ChangeLog.md', 'docs/OPERATIONS.md', 'docs/PLATFORM_PLAN.md') -or
            (($name -like '.env*' -or $name -like '*.env') -and $name -notlike '*.env.example')) {
            throw "Private file staged: $file. Unstage it before publishing."
        }
    }
    if ($staged.Count) {
        if ([string]::IsNullOrWhiteSpace($Message)) { throw 'Supply -Message for the staged commit.' }
        Invoke-Checked git @('diff', '--cached', '--check')
        Invoke-Checked gitleaks @('git', '--pre-commit', '--staged', '--redact', '--no-banner', '.')
        Invoke-Checked git @('commit', '-m', $Message)
    }
    # Scan committed history too: retrying after a failed push must not skip the secret check.
    Invoke-Checked gitleaks @('git', '--redact', '--no-banner', '.')
    Invoke-Checked git @('push', '--set-upstream', $Remote, "HEAD:refs/heads/$branch")
    Write-Host "Published $branch. Merge, pull and deployment were not run."
} finally {
    Pop-Location
}
