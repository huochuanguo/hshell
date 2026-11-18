param(
    [string]$Python = 'python',
    [switch]$SkipIntegration,
    [switch]$SkipBundle
)

$ErrorActionPreference = 'Stop'

function Invoke-Checked {
    param([string]$Program, [string[]]$Arguments)
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program 执行失败，退出码：$LASTEXITCODE" }
}

if ($env:OS -ne 'Windows_NT') { throw '本脚本只用于 Windows MSVC 构建。' }

Push-Location (Join-Path $PSScriptRoot '..')
try {
    $sshProgram = Join-Path $env:SystemRoot 'System32/OpenSSH/ssh.exe'
    if (-not (Test-Path -LiteralPath $sshProgram)) { throw '请先在 Windows 可选功能中安装 OpenSSH 客户端。' }
    Invoke-Checked $sshProgram @('-V')
    Invoke-Checked 'node' @('--version')
    Invoke-Checked 'cargo' @('--version')
    Invoke-Checked 'npm' @('ci')
    Invoke-Checked 'npm' @('run', 'format:check')
    Invoke-Checked 'cargo' @('fmt', '--all', '--', '--check')
    Invoke-Checked 'npm' @('run', 'build')
    Invoke-Checked 'npm' @('run', 'test:frontend')
    Invoke-Checked 'cargo' @('test', '--workspace', '--locked')
    Invoke-Checked 'cargo' @('clippy', '--workspace', '--all-targets', '--all-features', '--locked', '--', '-D', 'warnings')

    if (-not $SkipIntegration) {
        Invoke-Checked $Python @('-m', 'venv', 'target/windows-test-venv')
        $fixturePython = Join-Path (Get-Location) 'target/windows-test-venv/Scripts/python.exe'
        Invoke-Checked $fixturePython @('-m', 'pip', 'install', 'paramiko==3.5.1')
        Invoke-Checked $fixturePython @('scripts/ssh_smoke.py')
        Invoke-Checked $fixturePython @('scripts/windows_sftp_smoke.py')
        Invoke-Checked 'cargo' @('run', '-p', 'ssh-core', '--example', 'credentials_check', '--locked')
    } else {
        Write-Warning '已明确跳过真实 OpenSSH、SFTP 和 Windows 凭据管理器集成测试。'
    }

    if (-not $SkipBundle) {
        # Windows 自动合并 tauri.windows.conf.json，默认生成当前用户安装的 NSIS 包。
        Invoke-Checked 'npm' @('run', 'bundle', '--', '--ci')
        Get-ChildItem -LiteralPath 'target/release/bundle/nsis' -Filter '*.exe' | ForEach-Object {
            Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256
        }
    }
} finally {
    Pop-Location
}
