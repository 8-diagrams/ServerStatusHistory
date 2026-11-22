[CmdletBinding()]
param(
    [string]$InstallDir = 'C:\ServerStatus',
    [string]$ConfigFile = 'C:\ProgramData\ServerStatus\agent-config.psd1',
    [string]$LogFile = 'C:\ProgramData\ServerStatus\stat_client.log',
    [string]$TaskName = 'ServerStatus Rust Agent',
    [string]$Artifact = '',
    [string]$ReleaseUrl = 'https://github.com/8-diagrams/ServerStatusHistory/releases/download/master/ServerStatus-x86_64-pc-windows-msvc.zip',
    [string]$AgentAddr = 'https://st.9888812.xyz/report',
    [string]$AgentUser = '',
    [string]$AgentPassword = '',
    [string]$AgentLocation = 'cn',
    [string]$AgentType = 'windows',
    [string]$AgentGid = '',
    [string]$AgentAlias = '',
    [int]$AgentInterval = 1,
    [switch]$ForceConfig
)

$ErrorActionPreference = 'Stop'

# Windows PowerShell 5.1 may otherwise negotiate an obsolete TLS version.
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Fail([string]$Message) {
    Write-Error $Message
    exit 1
}

function Quote-Psd1([string]$Value) {
    if ($null -eq $Value) { return "''" }
    return "'" + $Value.Replace("'", "''") + "'"
}

function Download-File([string]$Url, [string]$Destination) {
    Write-Host "下载: $Url"
    Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $Destination
    if (-not (Test-Path -LiteralPath $Destination -PathType Leaf)) {
        Fail "下载失败: $Url"
    }
}

$CurrentPrincipal = [Security.Principal.WindowsPrincipal]::new(
    [Security.Principal.WindowsIdentity]::GetCurrent()
)
if (-not $CurrentPrincipal.IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Fail '请使用管理员 PowerShell 运行此脚本。'
}

$Architecture = $env:PROCESSOR_ARCHITEW6432
if (-not $Architecture) { $Architecture = $env:PROCESSOR_ARCHITECTURE }
if ($Architecture -ne 'AMD64') {
    Fail "当前 Windows 架构为 $Architecture，当前 Release 仅提供 x86_64-pc-windows-msvc。"
}
if ($AgentInterval -lt 1) {
    Fail '-AgentInterval 必须是大于 0 的整数。'
}

$ProgramDataDir = Split-Path -Parent $ConfigFile
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
New-Item -ItemType Directory -Force -Path $ProgramDataDir | Out-Null

$StageDir = Join-Path $env:TEMP ("ServerStatus-agent-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null
try {
    $ZipFile = Join-Path $StageDir 'ServerStatus-windows.zip'

    if ($Artifact) {
        if ($Artifact -match '^https://') {
            Download-File $Artifact $ZipFile
        } elseif (Test-Path -LiteralPath $Artifact -PathType Leaf) {
            $ZipFile = (Resolve-Path -LiteralPath $Artifact).Path
        } else {
            Fail "构建产物不存在: $Artifact"
        }
    } else {
        Download-File $ReleaseUrl $ZipFile
    }

    $ExtractDir = Join-Path $StageDir 'extract'
    Expand-Archive -LiteralPath $ZipFile -DestinationPath $ExtractDir -Force
    $DownloadedExe = Get-ChildItem -LiteralPath $ExtractDir -Recurse -Filter 'stat_client.exe' |
        Select-Object -First 1
    if (-not $DownloadedExe) {
        Fail 'ZIP 中找不到 stat_client.exe。'
    }

    $BinaryFile = Join-Path $InstallDir 'stat_client.exe'
    $PreviousBinary = Join-Path $InstallDir 'stat_client.exe.previous'
    $ExistingTask = Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    if ($ExistingTask) {
        Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
    }
    if (Test-Path -LiteralPath $BinaryFile -PathType Leaf) {
        Copy-Item -LiteralPath $BinaryFile -Destination $PreviousBinary -Force
    }
    Copy-Item -LiteralPath $DownloadedExe.FullName -Destination $BinaryFile -Force
    $VersionOutput = & $BinaryFile --version 2>&1
    if ($LASTEXITCODE -ne 0 -or -not ($VersionOutput -match '^stat_client')) {
        Fail "stat_client.exe 无法在当前 Windows 上运行，可能是架构或文件错误: $VersionOutput"
    }
    Write-Host "检测到: $VersionOutput"

    $LauncherFile = Join-Path $InstallDir 'run-rust-agent.ps1'
    $BundledLauncher = Get-ChildItem -LiteralPath $ExtractDir -Recurse -Filter 'run-rust-agent.ps1' |
        Select-Object -First 1
    if ($BundledLauncher) {
        Copy-Item -LiteralPath $BundledLauncher.FullName -Destination $LauncherFile -Force
    } elseif (-not (Test-Path -LiteralPath $LauncherFile -PathType Leaf)) {
        $RawBase = 'https://raw.githubusercontent.com/8-diagrams/ServerStatusHistory/master/client'
        Download-File "$RawBase/run-rust-agent.ps1" $LauncherFile
    }

    $ConfigExists = Test-Path -LiteralPath $ConfigFile -PathType Leaf
    if ($ForceConfig -or -not $ConfigExists) {
        if (-not $AgentUser -and -not $AgentGid) {
            $AgentUser = Read-Host '请输入固定主机用户名（动态组可直接回车）'
        }
        if (-not $AgentGid -and -not $AgentUser) {
            Fail '必须指定 AgentUser，或指定 AgentGid。'
        }
        if (-not $AgentPassword) {
            $SecurePassword = Read-Host '请输入 Agent 密码' -AsSecureString
            $PasswordPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($SecurePassword)
            try {
                $AgentPassword = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($PasswordPointer)
            } finally {
                [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($PasswordPointer)
            }
        }
        if (-not $AgentPassword) {
            Fail 'AgentPassword 不能为空。'
        }
        if ($AgentGid -and -not $AgentAlias) {
            $AgentAlias = $env:COMPUTERNAME
        }

        $EffectiveUser = if ($AgentUser) { $AgentUser } else { 'h1' }
        $ConfigLines = @(
            '@{',
            "    Addr     = $(Quote-Psd1 $AgentAddr)",
            "    User     = $(Quote-Psd1 $EffectiveUser)",
            "    Pass     = $(Quote-Psd1 $AgentPassword)",
            "    Location = $(Quote-Psd1 $AgentLocation)",
            "    Type     = $(Quote-Psd1 $AgentType)",
            "    Interval = $AgentInterval"
        )
        if ($AgentGid) {
            $ConfigLines += "    Gid      = $(Quote-Psd1 $AgentGid)"
            $ConfigLines += "    Alias    = $(Quote-Psd1 $AgentAlias)"
        }
        $ConfigLines += '}'
        Set-Content -LiteralPath $ConfigFile -Value $ConfigLines -Encoding UTF8
        Write-Host "已生成配置: $ConfigFile"
    } else {
        Write-Host "保留已有配置: $ConfigFile"
    }

    try {
        $Config = Import-PowerShellDataFile -LiteralPath $ConfigFile
    } catch {
        Fail "配置文件格式错误: $($_.Exception.Message)"
    }
    foreach ($RequiredKey in @('Addr', 'User', 'Pass', 'Location', 'Type', 'Interval')) {
        if (-not $Config.ContainsKey($RequiredKey) -or -not [string]$Config[$RequiredKey]) {
            Fail "配置文件缺少有效字段: $RequiredKey"
        }
    }

    $LogDir = Split-Path -Parent $LogFile
    New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
    & icacls.exe $ConfigFile /inheritance:r /grant:r '*S-1-5-18:(F)' '*S-1-5-32-544:(F)' | Out-Null

    $Action = New-ScheduledTaskAction `
        -Execute 'powershell.exe' `
        -Argument "-NoProfile -NonInteractive -ExecutionPolicy Bypass -File `"$LauncherFile`" -InstallDir `"$InstallDir`" -ConfigFile `"$ConfigFile`" -LogFile `"$LogFile`""
    $Trigger = New-ScheduledTaskTrigger -AtStartup
    $Principal = New-ScheduledTaskPrincipal `
        -UserId 'SYSTEM' `
        -LogonType ServiceAccount `
        -RunLevel Highest
    $Settings = New-ScheduledTaskSettingsSet `
        -StartWhenAvailable `
        -RestartCount 999 `
        -RestartInterval (New-TimeSpan -Seconds 10)

    Register-ScheduledTask `
        -TaskName $TaskName `
        -Action $Action `
        -Trigger $Trigger `
        -Principal $Principal `
        -Settings $Settings `
        -Description 'ServerStatus Rust monitoring agent' `
        -Force | Out-Null
    Start-ScheduledTask -TaskName $TaskName

    Write-Host ''
    Write-Host "安装完成: $InstallDir" -ForegroundColor Green
    Write-Host "任务名称: $TaskName"
    Write-Host "日志文件: $LogFile"
    Write-Host "查看日志: Get-Content '$LogFile' -Wait"
} finally {
    Remove-Item -LiteralPath $StageDir -Recurse -Force -ErrorAction SilentlyContinue
}
