# ServerStatus Rust Agent Windows launcher.
# Keep credentials in C:\ProgramData\ServerStatus\agent-config.psd1,
# not in this launcher or in the Scheduled Task command line.

[CmdletBinding()]
param(
    [string]$InstallDir = 'C:\ServerStatus',
    [string]$ConfigFile = 'C:\ProgramData\ServerStatus\agent-config.psd1',
    [string]$LogFile = 'C:\ProgramData\ServerStatus\stat_client.log',
    [int]$RetryDelaySeconds = 10
)

$ErrorActionPreference = 'Continue'
$ExeFile = Join-Path $InstallDir 'stat_client.exe'
$LogDir = Split-Path -Parent $LogFile

New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

function Write-AgentLog([string]$Message) {
    $Line = "{0} {1}" -f (Get-Date -Format 'yyyy-MM-ddTHH:mm:ss.fffK'), $Message
    Add-Content -LiteralPath $LogFile -Value $Line -Encoding UTF8
}

if (-not (Test-Path -LiteralPath $ExeFile -PathType Leaf)) {
    Write-AgentLog "ERROR stat_client.exe not found: $ExeFile"
    exit 2
}

if (-not (Test-Path -LiteralPath $ConfigFile -PathType Leaf)) {
    Write-AgentLog "ERROR config file not found: $ConfigFile"
    exit 2
}

try {
    $Config = Import-PowerShellDataFile -LiteralPath $ConfigFile
} catch {
    Write-AgentLog "ERROR cannot read config: $($_.Exception.Message)"
    exit 2
}

function Set-AgentEnv([string]$Name, [string]$Value) {
    [Environment]::SetEnvironmentVariable($Name, $Value, 'Process')
}

Set-AgentEnv 'SSR_ADDR' ([string]$Config.Addr)
Set-AgentEnv 'SSR_USER' ([string]$Config.User)
Set-AgentEnv 'SSR_PASS' ([string]$Config.Pass)
Set-AgentEnv 'SSR_LOC' ([string]$Config.Location)
Set-AgentEnv 'SSR_TYPE' ([string]$Config.Type)
Set-AgentEnv 'SSR_INTERVAL' ([string]$Config.Interval)

if ($Config.ContainsKey('Gid') -and [string]$Config.Gid) {
    Set-AgentEnv 'SSR_GID' ([string]$Config.Gid)
    Set-AgentEnv 'SSR_ALIAS' ([string]$Config.Alias)
} else {
    Remove-Item Env:SSR_GID -ErrorAction SilentlyContinue
    Remove-Item Env:SSR_ALIAS -ErrorAction SilentlyContinue
}

Set-Location -LiteralPath $InstallDir
Write-AgentLog "INFO launcher started exe=$ExeFile addr=$($Config.Addr) user=$($Config.User)"

while ($true) {
    try {
        Write-AgentLog 'INFO starting stat_client.exe'
        & $ExeFile *>> $LogFile
        $ExitCode = $LASTEXITCODE
        Write-AgentLog "WARN stat_client.exe exited code=$ExitCode; retrying in ${RetryDelaySeconds}s"
    } catch {
        Write-AgentLog "ERROR stat_client.exe failed: $($_.Exception.Message); retrying in ${RetryDelaySeconds}s"
    }
    Start-Sleep -Seconds $RetryDelaySeconds
}
