# 无显卡低配 Windows VPS 优化脚本
# 请以管理员身份运行

Write-Host "开始优化..." -ForegroundColor Green

# 1. 关闭视觉效果（性能优先）
Write-Host "1. 调整视觉效果为最佳性能..."
$path = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects"
if (!(Test-Path $path)) { New-Item -Path $path -Force | Out-Null }
Set-ItemProperty -Path $path -Name "VisualFXSetting" -Value 2

# 通过系统属性设置
$sysPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\VisualEffects"
Set-ItemProperty -Path "HKCU:\Control Panel\Desktop" -Name "UserPreferencesMask" -Value ([byte[]](0x90,0x12,0x03,0x80,0x10,0x00,0x00,0x00))

# 2. 关闭透明效果
Write-Host "2. 关闭透明效果..."
Set-ItemProperty -Path "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize" -Name "EnableTransparency" -Value 0 -ErrorAction SilentlyContinue

# 3. 禁用 Windows Search
Write-Host "3. 禁用 Windows Search..."
Stop-Service WSearch -Force -ErrorAction SilentlyContinue
Set-Service WSearch -StartupType Disabled -ErrorAction SilentlyContinue

# 4. 禁用 SysMain (Superfetch)
Write-Host "4. 禁用 SysMain..."
Stop-Service SysMain -Force -ErrorAction SilentlyContinue
Set-Service SysMain -StartupType Disabled -ErrorAction SilentlyContinue

# 5. 禁用诊断跟踪相关服务
Write-Host "5. 禁用部分诊断服务..."
$services = @(
    "DiagTrack",           # Connected User Experiences and Telemetry
    "dmwappushservice"     # Device Management Wireless Application Protocol
)
foreach ($svc in $services) {
    Stop-Service $svc -Force -ErrorAction SilentlyContinue
    Set-Service $svc -StartupType Disabled -ErrorAction SilentlyContinue
}

# 6. 关闭一些不必要的计划任务
Write-Host "6. 禁用部分计划任务..."
$tasks = @(
    "\Microsoft\Windows\Application Experience\Microsoft Compatibility Appraiser",
    "\Microsoft\Windows\Application Experience\ProgramDataUpdater",
    "\Microsoft\Windows\Autochk\Proxy",
    "\Microsoft\Windows\Customer Experience Improvement Program\Consolidator",
    "\Microsoft\Windows\Customer Experience Improvement Program\UsbCeip",
    "\Microsoft\Windows\DiskDiagnostic\Microsoft-Windows-DiskDiagnosticDataCollector",
    "\Microsoft\Windows\Feedback\Siuf\DmClient",
    "\Microsoft\Windows\Feedback\Siuf\DmClientOnScenarioDownload"
)
foreach ($task in $tasks) {
    Disable-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue | Out-Null
}

# 7. 优化远程桌面相关（降低开销）
Write-Host "7. 优化远程桌面设置..."
# 禁用远程桌面的一些视觉特性可以通过组策略，这里做基础设置
Set-ItemProperty -Path "HKLM:\SOFTWARE\Policies\Microsoft\Windows NT\Terminal Services" -Name "fDisableCpm" -Value 1 -ErrorAction SilentlyContinue

# 8. 关闭休眠（VPS 通常不需要）
Write-Host "8. 关闭休眠..."
powercfg -h off

# 9. 设置电源计划为高性能
Write-Host "9. 设置高性能电源计划..."
powercfg -setactive 8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c

# 10. 清理临时文件
Write-Host "10. 清理临时文件..."
Remove-Item -Path "$env:TEMP\*" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path "C:\Windows\Temp\*" -Recurse -Force -ErrorAction SilentlyContinue

Write-Host "`n优化完成！建议重启一次使所有设置生效。" -ForegroundColor Green
Write-Host "重启后资源管理器占用通常会明显下降。" -ForegroundColor Yellow
