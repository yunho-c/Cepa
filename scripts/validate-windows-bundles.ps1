[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [string] $BundleRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($env:OS -ne "Windows_NT") {
    throw "Windows bundle validation must run on Windows"
}

$Repository = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($BundleRoot)) {
    $BundleRoot = Join-Path $Repository "src-tauri\target\release\bundle"
} elseif (-not [System.IO.Path]::IsPathRooted($BundleRoot)) {
    $BundleRoot = Join-Path $Repository $BundleRoot
}

function Get-OneFile {
    param(
        [string] $Label,
        [string] $Directory,
        [string] $Filter
    )

    $Matches = @(Get-ChildItem -LiteralPath $Directory -Filter $Filter -File -ErrorAction SilentlyContinue)
    if ($Matches.Count -ne 1) {
        throw "Expected exactly one $Label in $Directory, found $($Matches.Count)"
    }
    return $Matches[0]
}

function Assert-Equal {
    param(
        [string] $Label,
        [string] $Expected,
        [string] $Actual
    )

    if ($Actual -cne $Expected) {
        throw "$Label`: expected '$Expected', found '$Actual'"
    }
}

function Get-MsiProperty {
    param(
        [object] $Database,
        [string] $Name
    )

    $View = $Database.OpenView("SELECT ``Value`` FROM ``Property`` WHERE ``Property``='$Name'")
    try {
        $null = $View.Execute()
        $Record = $View.Fetch()
        if ($null -eq $Record) {
            throw "MSI property is missing: $Name"
        }
        return ([string] $Record.StringData(1)).Trim()
    } finally {
        $null = $View.Close()
    }
}

function Get-MsiFileNames {
    param([object] $Database)

    $Names = @()
    $View = $Database.OpenView("SELECT ``FileName`` FROM ``File``")
    try {
        $null = $View.Execute()
        while ($null -ne ($Record = $View.Fetch())) {
            $Names += $Record.StringData(1).Split("|")[-1]
        }
    } finally {
        $null = $View.Close()
    }
    return $Names
}

function Assert-SignatureState {
    param(
        [string] $Label,
        [System.IO.FileInfo] $File
    )

    $Signature = Get-AuthenticodeSignature -LiteralPath $File.FullName
    if ($Signature.Status -notin @("Valid", "NotSigned")) {
        throw "$Label signature has unexpected status: $($Signature.Status)"
    }
    return $Signature.Status
}

$Package = Get-Content -LiteralPath (Join-Path $Repository "package.json") -Raw -Encoding UTF8 | ConvertFrom-Json
$TauriConfig = Get-Content -LiteralPath (Join-Path $Repository "src-tauri\tauri.conf.json") -Raw -Encoding UTF8 | ConvertFrom-Json
$ExpectedVersion = [string] $Package.version
# NSIS's Windows VERSIONINFO resource transliterates the configured copyright
# symbol even though the installer UI and source metadata retain the license.
$ExpectedWindowsCopyright = ([string] $TauriConfig.bundle.copyright).Replace("©", "c")
$Msi = Get-OneFile "MSI package" (Join-Path $BundleRoot "msi") "*.msi"
$Nsis = Get-OneFile "NSIS installer" (Join-Path $BundleRoot "nsis") "*.exe"

$Installer = New-Object -ComObject WindowsInstaller.Installer
$Database = $Installer.GetType().InvokeMember(
    "OpenDatabase",
    "InvokeMethod",
    $null,
    $Installer,
    @($Msi.FullName, 0)
)

Assert-Equal "MSI product name" "Cepa" (Get-MsiProperty $Database "ProductName")
Assert-Equal "MSI product version" $ExpectedVersion (Get-MsiProperty $Database "ProductVersion")
Assert-Equal "MSI manufacturer" "Cepa contributors" (Get-MsiProperty $Database "Manufacturer")
$MsiFiles = @(Get-MsiFileNames $Database)
if ($MsiFiles -cnotcontains "cepa.exe") {
    throw "MSI payload does not contain cepa.exe"
}

$ExtractionRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("cepa-msi-" + [guid]::NewGuid().ToString("N"))
$null = New-Item -ItemType Directory -Path $ExtractionRoot
try {
    $Arguments = @(
        "/a"
        "`"$($Msi.FullName)`""
        "/qn"
        "TARGETDIR=`"$ExtractionRoot`""
    )
    $Extraction = Start-Process msiexec.exe -ArgumentList $Arguments -Wait -PassThru
    if ($Extraction.ExitCode -ne 0) {
        throw "MSI administrative extraction failed with exit code $($Extraction.ExitCode)"
    }
    $ExtractedExecutables = @(Get-ChildItem -LiteralPath $ExtractionRoot -Recurse -Filter "cepa.exe" -File)
    if ($ExtractedExecutables.Count -ne 1) {
        throw "Expected exactly one extracted cepa.exe, found $($ExtractedExecutables.Count)"
    }
    Assert-Equal "MSI executable product name" "Cepa" $ExtractedExecutables[0].VersionInfo.ProductName
    Assert-Equal "MSI executable product version" $ExpectedVersion $ExtractedExecutables[0].VersionInfo.ProductVersion
} finally {
    Remove-Item -LiteralPath $ExtractionRoot -Recurse -Force -ErrorAction SilentlyContinue
}

$VersionInfo = $Nsis.VersionInfo
Assert-Equal "NSIS product name" "Cepa" $VersionInfo.ProductName
Assert-Equal "NSIS product version" $ExpectedVersion $VersionInfo.ProductVersion
Assert-Equal "NSIS file description" "Cepa" $VersionInfo.FileDescription
Assert-Equal "NSIS copyright" $ExpectedWindowsCopyright $VersionInfo.LegalCopyright

$MsiSignature = Assert-SignatureState "MSI" $Msi
$NsisSignature = Assert-SignatureState "NSIS" $Nsis

Write-Output "version=$ExpectedVersion"
Write-Output "msi=$($Msi.Name)"
Write-Output "nsis=$($Nsis.Name)"
Write-Output "msiPayload=cepa.exe"
Write-Output "msiSignature=$MsiSignature"
Write-Output "nsisSignature=$NsisSignature"
Get-FileHash -LiteralPath $Msi.FullName -Algorithm SHA256 | ForEach-Object {
    Write-Output "$($_.Hash.ToLowerInvariant())  $($_.Path)"
}
Get-FileHash -LiteralPath $Nsis.FullName -Algorithm SHA256 | ForEach-Object {
    Write-Output "$($_.Hash.ToLowerInvariant())  $($_.Path)"
}
