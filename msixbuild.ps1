param(
    [string]$ContentRoot = "AppxContent",
    [string]$OutDir = "AppxBundles",
    [string]$MsixBundleCli = "msixbundle-cli",
    [switch]$PrepareOnly,
    [string]$Pfx,
    [SecureString]$PfxPassword,
    [string]$TimestampUrl,
    [ValidateSet("rfc3161", "authenticode")]
    [string]$TimestampMode = "rfc3161"
)

$ErrorActionPreference = "Stop"

function Copy-CommonContent {
    param(
        [Parameter(Mandatory = $true)]
        [string]$SourceDir,
        [Parameter(Mandatory = $true)]
        [string]$DestinationDir
    )

    $excludedNames = @("x64", "arm64")

    Get-ChildItem -Path $SourceDir -Force | Where-Object { $_.Name -notin $excludedNames } | ForEach-Object {
        Copy-Item -Path $_.FullName -Destination $DestinationDir -Recurse -Force
    }
}

function Set-ManifestIdentity {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ManifestPath,
        [Parameter(Mandatory = $true)]
        [string]$ProcessorArchitecture,
        [Parameter(Mandatory = $true)]
        [string]$Version
    )

    [xml]$manifest = Get-Content -Path $ManifestPath
    $namespaceManager = New-Object System.Xml.XmlNamespaceManager($manifest.NameTable)
    $namespaceManager.AddNamespace("appx", $manifest.DocumentElement.NamespaceURI)

    $identity = $manifest.SelectSingleNode("/appx:Package/appx:Identity", $namespaceManager)
    if ($null -eq $identity) {
        throw "Identity node was not found in manifest: $ManifestPath"
    }

    $identity.SetAttribute("ProcessorArchitecture", $ProcessorArchitecture)
    $identity.SetAttribute("Version", $Version)
    $manifest.Save($ManifestPath)
}

function New-StageDirectory {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    if (Test-Path -Path $Path) {
        Remove-Item -Path $Path -Recurse -Force
    }

    New-Item -ItemType Directory -Path $Path -Force | Out-Null
}

function Get-BuildOutputCandidates {
    param(
        [Parameter(Mandatory = $true)]
        [hashtable]$Architecture,
        [Parameter(Mandatory = $true)]
        [string]$RepoRoot,
        [Parameter(Mandatory = $true)]
        [string]$FileName
    )

    $candidatePaths = @(
        (Join-Path $RepoRoot "src-tauri\target\$($Architecture.Triple)\release\$FileName")
    )

    # Cargo writes the host target to target\release when no explicit --target is supplied.
    if ($Architecture.Name -eq "x64") {
        $candidatePaths += Join-Path $RepoRoot "src-tauri\target\release\$FileName"
    }

    return $candidatePaths
}

function Get-CliCandidates {
    param(
        [Parameter(Mandatory = $true)]
        [hashtable]$Architecture,
        [Parameter(Mandatory = $true)]
        [string]$RepoRoot
    )

    # scripts/build-cli.ts puts the Tauri sidecar in src-tauri\binaries.
    return @(Join-Path $RepoRoot "src-tauri\binaries\quadrantmc-$($Architecture.Triple).exe") +
        (Get-BuildOutputCandidates -Architecture $Architecture -RepoRoot $RepoRoot -FileName "quadrantmc.exe")
}

function Find-FirstFile {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Paths
    )

    foreach ($candidatePath in $Paths) {
        if (Test-Path -Path $candidatePath -PathType Leaf) {
            return $candidatePath
        }
    }

    return $null
}

$repoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$contentRootPath = Join-Path $repoRoot $ContentRoot
$bundleOutputPath = Join-Path $repoRoot $OutDir

$templateContentManifestPath = Join-Path $contentRootPath "AppxManifest.xml"
$rootManifestPath = Join-Path $repoRoot "AppxManifest.xml"
$packageVersion = (Get-Content (Join-Path $repoRoot "package.json") | ConvertFrom-Json).version -replace "-.*$", ""
$manifestVersion = "$packageVersion.0"

$iconMappings = @(
    @{ Source = "src-tauri\icons\Square150x150Logo.png"; Destination = "Images\Square150x150Logo.png" },
    @{ Source = "src-tauri\icons\Square44x44Logo.png"; Destination = "Images\Square44x44Logo.png" },
    @{ Source = "src-tauri\icons\StoreLogo.png"; Destination = "Images\StoreLogo.png" }
)

$architectures = @(
    @{
        Name                  = "x64"
        Triple                = "x86_64-pc-windows-msvc"
        ProcessorArchitecture = "x64"
        CliFlag               = "--dir-x64"
    },
    @{
        Name                  = "arm64"
        Triple                = "aarch64-pc-windows-msvc"
        ProcessorArchitecture = "arm64"
        CliFlag               = "--dir-arm64"
    }
)

New-Item -ItemType Directory -Path $contentRootPath -Force | Out-Null
New-Item -ItemType Directory -Path $bundleOutputPath -Force | Out-Null

$stagedArchitectures = @()

foreach ($architecture in $architectures) {
    $sourceExe = Find-FirstFile -Paths (Get-BuildOutputCandidates -Architecture $architecture -RepoRoot $repoRoot -FileName "quadrant_next.exe")
    if ($null -eq $sourceExe) {
        continue
    }

    $cliCandidates = Get-CliCandidates -Architecture $architecture -RepoRoot $repoRoot
    $sourceCli = Find-FirstFile -Paths $cliCandidates
    if ($null -eq $sourceCli) {
        throw "quadrantmc.exe was not found for $($architecture.Name). Expected one of:`n$($cliCandidates -join "`n")"
    }

    $stageDir = Join-Path $contentRootPath $architecture.Name
    New-StageDirectory -Path $stageDir

    if (Test-Path -Path $templateContentManifestPath -PathType Leaf) {
        Copy-CommonContent -SourceDir $contentRootPath -DestinationDir $stageDir
    }
    elseif (Test-Path -Path $rootManifestPath -PathType Leaf) {
        $imagesDir = Join-Path $stageDir "Images"
        New-Item -ItemType Directory -Path $imagesDir -Force | Out-Null
        Copy-Item -Path $rootManifestPath -Destination (Join-Path $stageDir "AppxManifest.xml") -Force

        foreach ($iconMapping in $iconMappings) {
            $sourcePath = Join-Path $repoRoot $iconMapping.Source
            if (-not (Test-Path -Path $sourcePath -PathType Leaf)) {
                throw "Required icon was not found: $sourcePath"
            }

            $destinationPath = Join-Path $stageDir $iconMapping.Destination
            $destinationParent = Split-Path -Parent $destinationPath
            New-Item -ItemType Directory -Path $destinationParent -Force | Out-Null
            Copy-Item -Path $sourcePath -Destination $destinationPath -Force
        }
    }
    else {
        throw "No AppxManifest.xml template was found in $contentRootPath or $repoRoot."
    }

    $stageManifestPath = Join-Path $stageDir "AppxManifest.xml"
    if (-not (Test-Path -Path $stageManifestPath -PathType Leaf)) {
        throw "A manifest was not staged for $($architecture.Name): $stageManifestPath"
    }

    Set-ManifestIdentity -ManifestPath $stageManifestPath -ProcessorArchitecture $architecture.ProcessorArchitecture -Version $manifestVersion
    Copy-Item -Path $sourceExe -Destination (Join-Path $stageDir "quadrant_next.exe") -Force
    Copy-Item -Path $sourceCli -Destination (Join-Path $stageDir "quadrantmc.exe") -Force

    $stagedArchitectures += [pscustomobject]@{
        Name      = $architecture.Name
        CliFlag   = $architecture.CliFlag
        Directory = $stageDir
    }
}

if ($stagedArchitectures.Count -eq 0) {
    $expectedOutputs = $architectures | ForEach-Object {
        Get-BuildOutputCandidates -Architecture $_ -RepoRoot $repoRoot -FileName "quadrant_next.exe"
    }

    throw "No architecture-specific executables were found. Expected at least one of:`n$($expectedOutputs -join "`n")"
}

foreach ($stagedArchitecture in $stagedArchitectures) {
    Write-Host "Prepared $($stagedArchitecture.Name) staging content in $($stagedArchitecture.Directory)"
}

if ($PrepareOnly) {
    Write-Host "Skipping msixbundle-cli invocation because -PrepareOnly was supplied."
    return
}

$cliCommand = Get-Command -Name $MsixBundleCli -ErrorAction SilentlyContinue
if ($null -eq $cliCommand) {
    throw "msixbundle-cli was not found on PATH. Install it with: cargo install msixbundle-cli"
}

$cliArgs = @("--out-dir", $bundleOutputPath)
foreach ($stagedArchitecture in $stagedArchitectures) {
    $cliArgs += @($stagedArchitecture.CliFlag, $stagedArchitecture.Directory)
}

if ($Pfx) {
    $cliArgs += @("--pfx", $Pfx)
}

if ($PfxPassword) {
    $cliArgs += @("--pfx-password", $PfxPassword)
}

if ($TimestampUrl) {
    $cliArgs += @("--timestamp-url", $TimestampUrl, "--timestamp-mode", $TimestampMode)
}

Write-Host "Running $($cliCommand.Source) $($cliArgs -join ' ')"
& $cliCommand.Source @cliArgs

if ($LASTEXITCODE -ne 0) {
    throw "msixbundle-cli failed with exit code $LASTEXITCODE"
}
