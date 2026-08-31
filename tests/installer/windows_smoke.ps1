<#
.SYNOPSIS
    StarForge Windows installer/binary smoke tests.

.DESCRIPTION
    Verifies that a StarForge Windows binary starts up and that core help
    commands -- including the `config doctor` surface -- work. Runs against an
    isolated STARFORGE_CONFIG_DIR so the run is deterministic and leaves no
    user state behind on the machine.

    CI contract (matches .github/workflows/installer-tests.yml):
      - Exit code is asserted for --help and for the doctor surface
        (`config --help` must exit 0 and list `doctor`).
      - The live `config doctor` run is diagnostic: the offline "schema"
        finding must pass, while network/toolchain findings (Horizon,
        Soroban RPC, Stellar CLI on PATH) are reported but do not fail the
        job, so isolated runners without those tools stay green.
      - Every hard test continues on failure, so one broken command never
        hides another. The script exits 1 if any hard test failed.

    Failures print the exact command, exit code, and captured output so CI
    failures are actionable. All output is also teed to a log file.

.NOTES
    Run with:
      pwsh -File tests\installer\windows_smoke.ps1 -Binary target\release\starforge.exe
      powershell -ExecutionPolicy Bypass -File tests\installer\windows_smoke.ps1

    If -Binary is omitted the script auto-detects target\release\starforge.exe
    then target\debug\starforge.exe from the repo root.
#>

param(
    [string]$Binary = "",
    [string]$LogPath = ""
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
# Native exit codes must never be turned into terminating errors (PS 7.3+).
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$script:passed = 0
$script:failed = 0
$script:skipped = 0
$script:failures = [System.Collections.Generic.List[string]]::new()

function Get-RepoRoot {
    $candidate = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
    if ($null -ne $candidate -and (Test-Path (Join-Path $candidate "Cargo.toml"))) {
        return $candidate
    }
    throw "Could not locate repo root from $PSScriptRoot"
}

$repoRoot = Get-RepoRoot

# -- Resolve the binary -----------------------------------------------------
if ([string]::IsNullOrWhiteSpace($Binary)) {
    foreach ($candidate in @("target\release\starforge.exe", "target\debug\starforge.exe")) {
        $full = Join-Path $repoRoot $candidate
        if (Test-Path -LiteralPath $full) { $Binary = $full; break }
    }
}
if ([string]::IsNullOrWhiteSpace($Binary)) {
    Write-Error "No StarForge binary found. Build first or pass -Binary."
    exit 2
}
$Binary = (Resolve-Path -LiteralPath $Binary).Path
if (-not (Test-Path -LiteralPath $Binary)) {
    Write-Error "StarForge binary not found: $Binary"
    exit 2
}

# -- Resolve the log file ---------------------------------------------------
if ([string]::IsNullOrWhiteSpace($LogPath)) {
    $LogPath = Join-Path $repoRoot "windows-smoke.log"
}
Remove-Item -LiteralPath $LogPath -ErrorAction SilentlyContinue

function Write-Log([string]$Message) {
    Write-Host $Message
    Add-Content -LiteralPath $script:LogPath -Value $Message
}

# Runs starforge.exe in an isolated STARFORGE_CONFIG_DIR and captures everything.
function Invoke-Starforge([string[]]$Arguments, [string]$ConfigDir) {
    $previousConfig = $env:STARFORGE_CONFIG_DIR
    $env:STARFORGE_CONFIG_DIR = $ConfigDir
    $env:NO_COLOR = "1"
    try {
        $output = & $Binary @Arguments 2>&1 | Out-String
        $exitCode = [int]$LASTEXITCODE
    }
    finally {
        $env:NO_COLOR = ""
        if ($null -eq $previousConfig) {
            Remove-Item Env:STARFORGE_CONFIG_DIR -ErrorAction SilentlyContinue
        } else {
            $env:STARFORGE_CONFIG_DIR = $previousConfig
        }
    }
    [pscustomobject]@{ ExitCode = $exitCode; Output = $output }
}

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-Contains([string]$Text, [string]$Fragment, [string]$Message) {
    if ($Text -notlike "*$Fragment*") { throw "$Message (looked for: '$Fragment')" }
}

function Run-Test([string]$Name, [scriptblock]$Body) {
    try {
        & $Body
        $script:passed++
        Write-Log "PASS  $Name"
    }
    catch {
        $script:failed++
        $script:failures.Add("FAIL  $Name :: $($_.Exception.Message)")
        Write-Log "FAIL  $Name :: $($_.Exception.Message)"
    }
}

$configDir = Join-Path $env:TEMP ("starforge-smoke-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $configDir -Force | Out-Null

Write-Log "StarForge Windows smoke tests"
Write-Log "--------------------------------------------------------"
Write-Log "Binary   : $Binary"
Write-Log "ConfigDir: $configDir"
Write-Log ""

try {
    # -- 1. Version / startup ------------------------------------------------
    Run-Test "starforge --version exits 0" {
        $r = Invoke-Starforge @("--version") $configDir
        Assert-True ($r.ExitCode -eq 0) "starforge --version exited $($r.ExitCode)$(if ($r.Output) { '; output: ' + $r.Output.Trim() })"
        Assert-Contains $r.Output "starforge" "version output should name the binary"
    }

    # -- 2. Core help exit code ----------------------------------------------
    Run-Test "starforge --help exits 0" {
        $r = Invoke-Starforge @("--help") $configDir
        Assert-True ($r.ExitCode -eq 0) "starforge --help exited $($r.ExitCode)$(if ($r.Output) { '; output: ' + $r.Output.Trim() })"
    }

    Run-Test "starforge info exits 0" {
        $r = Invoke-Starforge @("info") $configDir
        Assert-True ($r.ExitCode -eq 0) "starforge info exited $($r.ExitCode)$(if ($r.Output) { '; output: ' + $r.Output.Trim() })"
    }

    # -- 3. Doctor subset: config --help exposes the doctor surface ----------
    Run-Test "starforge config --help exits 0 and lists doctor" {
        $r = Invoke-Starforge @("config", "--help") $configDir
        Assert-True ($r.ExitCode -eq 0) "starforge config --help exited $($r.ExitCode)$(if ($r.Output) { '; output: ' + $r.Output.Trim() })"
        Assert-Contains $r.Output "doctor" "config help should list the doctor subcommand"
    }

    # -- 4. Live config doctor (diagnostic) ----------------------------------
    # The offline "schema" finding must pass. Network/toolchain findings are
    # reported without failing the job so isolated Windows runners (no
    # internet or no Stellar CLI on PATH) can still validate the binary.
    $r = Invoke-Starforge @("config", "doctor") $configDir
    $bannerOk = $r.Output -like "*StarForge Config Doctor*"
    # Avoid a non-ASCII literal in the source file (PowerShell 5.1 assumes ANSI
    # for BOM-less scripts) and build the mark glyph from its code point.
    $crossMarkSchema = [string][char]0x2717 + " schema"
    $schemaFailed = $r.Output -match [regex]::Escape($crossMarkSchema)
    if (-not $bannerOk) {
        $script:failed++
        $script:failures.Add("FAIL  config doctor :: doctor banner not found")
        Write-Log "FAIL  config doctor :: doctor banner not found"
    }
    elseif ($schemaFailed) {
        $script:failed++
        $script:failures.Add("FAIL  config doctor :: offline schema check failed")
        Write-Log "FAIL  config doctor :: offline schema check failed"
    }
    elseif ($r.ExitCode -ne 0) {
        $script:skipped++
        Write-Log "SKIP  config doctor :: exit $($r.ExitCode) (network/toolchain findings only; see log)"
    }
    else {
        $script:passed++
        Write-Log "PASS  config doctor (all checks passed, exit 0)"
    }
    Write-Log ""
    Write-Log "  -- config doctor output --"
    $r.Output -split "\r?\n" | ForEach-Object { Write-Log "  $_" }
    Write-Log "  -- /doctor output --"

    Write-Log ""
    Write-Log "--------------------------------------------------------"
    Write-Log ("Results: {0} passed, {1} failed, {2} skipped" -f $script:passed, $script:failed, $script:skipped)
    Write-Log "Log file: $LogPath"

    if ($script:failed -gt 0) {
        Write-Log ""
        Write-Log "Failures:"
        foreach ($f in $script:failures) { Write-Log "  $f" }
        exit 1
    }
    exit 0
}
finally {
    Remove-Item -LiteralPath $configDir -Recurse -Force -ErrorAction SilentlyContinue
}