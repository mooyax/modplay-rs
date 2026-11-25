# Builds the ahmod static library (modplay-rs engines with a libopenmpt-compatible C API)
# for iOS and Android, and puts the results into mobile\tracker:
#   tracker\ios\AHMod.xcframework            (iOS device arm64 + simulator arm64)
#   tracker\android\libs\<ABI>\libahmod.a    (arm64-v8a, armeabi-v7a, x86_64, x86)
#
# Run in PowerShell from anywhere:
#   powershell -ExecutionPolicy Bypass -File D:\develop\modplay-rs\mobile\build-mobile.ps1
# Needs rustup / cargo only (no Xcode, no Android NDK): static libraries are not linked
# here, the app build (EAS) links them.
#
#   -NoMod   leave out the ProTracker (MOD) engine: IT / S3M / XM only, about 50 KB smaller
param([switch]$NoMod)

$ErrorActionPreference = 'Stop'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$crate = Join-Path $here 'ahmod'
$module = Join-Path $here 'tracker'
$targetDir = Join-Path $crate 'target'

$android = [ordered]@{
  'aarch64-linux-android'   = 'arm64-v8a'
  'armv7-linux-androideabi' = 'armeabi-v7a'
  'x86_64-linux-android'    = 'x86_64'
  'i686-linux-android'      = 'x86'
}
$ios = [ordered]@{
  'aarch64-apple-ios'     = 'ios-arm64'
  'aarch64-apple-ios-sim' = 'ios-arm64-simulator'
}

Write-Host '== rustup targets'
rustup target add @($android.Keys + $ios.Keys)
if ($LASTEXITCODE -ne 0) { throw 'rustup target add failed' }

# the mobile library never uses libopenmpt (its own functions have the same names)
$env:PT2_NO_OPENMPT = '1'
$env:IPHONEOS_DEPLOYMENT_TARGET = '15.1'

foreach ($t in @($android.Keys + $ios.Keys)) {
  Write-Host "== cargo build $t"
  $extra = @()
  if ($NoMod) { $extra += '--no-default-features' }
  cargo build --release --manifest-path (Join-Path $crate 'Cargo.toml') --target $t @extra
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $t" }
}

# ---- Android ----
foreach ($t in $android.Keys) {
  $dst = Join-Path $module ("android\libs\" + $android[$t])
  New-Item -ItemType Directory -Force -Path $dst | Out-Null
  Copy-Item (Join-Path $targetDir "$t\release\libahmod.a") (Join-Path $dst 'libahmod.a') -Force
}

# ---- iOS: an XCFramework of two static libraries (written by hand, no xcodebuild needed) ----
$xcf = Join-Path $module 'ios\AHMod.xcframework'
if (Test-Path $xcf) { Remove-Item -Recurse -Force $xcf }
foreach ($t in $ios.Keys) {
  $dst = Join-Path $xcf $ios[$t]
  New-Item -ItemType Directory -Force -Path $dst | Out-Null
  Copy-Item (Join-Path $targetDir "$t\release\libahmod.a") (Join-Path $dst 'libahmod.a') -Force
}
$plist = @'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AvailableLibraries</key>
	<array>
		<dict>
			<key>BinaryPath</key>
			<string>libahmod.a</string>
			<key>LibraryIdentifier</key>
			<string>ios-arm64</string>
			<key>LibraryPath</key>
			<string>libahmod.a</string>
			<key>SupportedArchitectures</key>
			<array>
				<string>arm64</string>
			</array>
			<key>SupportedPlatform</key>
			<string>ios</string>
		</dict>
		<dict>
			<key>BinaryPath</key>
			<string>libahmod.a</string>
			<key>LibraryIdentifier</key>
			<string>ios-arm64-simulator</string>
			<key>LibraryPath</key>
			<string>libahmod.a</string>
			<key>SupportedArchitectures</key>
			<array>
				<string>arm64</string>
			</array>
			<key>SupportedPlatform</key>
			<string>ios</string>
			<key>SupportedPlatformVariant</key>
			<string>simulator</string>
		</dict>
	</array>
	<key>CFBundlePackageType</key>
	<string>XFWK</string>
	<key>XCFrameworkFormatVersion</key>
	<string>1.0</string>
</dict>
</plist>
'@
# UTF-8 without BOM, LF line endings
[System.IO.File]::WriteAllText((Join-Path $xcf 'Info.plist'), $plist.Replace("`r`n", "`n"), (New-Object System.Text.UTF8Encoding($false)))

Write-Host ''
Write-Host '== done'
Get-ChildItem -Recurse (Join-Path $module 'android\libs'), $xcf -Filter *.a | ForEach-Object { '{0,10:N0}  {1}' -f $_.Length, $_.FullName }
