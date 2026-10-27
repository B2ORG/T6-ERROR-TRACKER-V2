param(
    [Parameter(Mandatory = $true)][string]$PlutoniumRoot,
    [Parameter(Mandatory = $true)][string]$GameDirectory
)
$ErrorActionPreference = 'Stop'
$bootstrapper = Join-Path $PlutoniumRoot 'bin\plutonium-bootstrapper-win32.exe'
$tracker = Join-Path (Split-Path -Parent $PSScriptRoot) 'target\release\t6-error-tracker.exe'
foreach ($required in @($bootstrapper, $tracker, (Join-Path $GameDirectory 't6zm.exe'))) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "File not found: $required" }
}
if (Get-Process -Name 'plutonium-bootstrapper-win32' -ErrorAction SilentlyContinue) {
    throw 'Plutonium is already running. Run the tracker separately, or close the game before launching this test.'
}
$gameArguments = @(
    't6zm', ('"{0}"' -f $GameDirectory), '-lan',
    '+name', 'RustTrackerTest', '+set', 'r_fullscreen', '0',
    '+set', 'r_mode', '1280x720', '+set', 'com_introPlayed', '1',
    '+set', 'ui_zm_mapstartlocation', 'town',
    '+set', 'ui_zm_gamemodegroup', 'zsurvival',
    '+set', 'ui_gametype', 'zstandard', '+set', 'g_gametype', 'zstandard',
    '+map', 'zm_transit'
)
Start-Process -FilePath $bootstrapper -WorkingDirectory $PlutoniumRoot -ArgumentList $gameArguments -WindowStyle Normal
Start-Process -FilePath $tracker -WorkingDirectory (Split-Path -Parent $PSScriptRoot) -WindowStyle Normal
