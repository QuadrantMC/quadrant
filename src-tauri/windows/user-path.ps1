# Adds a folder to, or removes it from, the per-user PATH. The NSIS installer
# hooks (installer-hooks.nsh) run it so `quadrantmc` works in new terminals.
# Both actions are no-ops when PATH is already in the wanted state, because the
# updater re-runs the installer on every update.
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet("Add", "Remove")]
    [string]$Action,
    [Parameter(Mandatory = $true)]
    [string]$Directory,
    # Tests point this at a scratch value instead of the real PATH.
    [string]$ValueName = "Path"
)

$ErrorActionPreference = "Stop"

function Get-NormalizedEntry([string]$Entry) {
    return $Entry.Trim().TrimEnd("\")
}

$target = Get-NormalizedEntry $Directory
$key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey("Environment")
try {
    # Read unexpanded so entries such as %USERPROFILE%\bin are written back as-is.
    $current = [string]$key.GetValue($ValueName, "", [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $entries = if ($current -eq "") { @() } else { $current -split ";" }
    $others = @($entries | Where-Object {
            -not [string]::Equals((Get-NormalizedEntry $_), $target, [StringComparison]::OrdinalIgnoreCase)
        })
    $present = $others.Count -ne $entries.Count

    if ($Action -eq "Add") {
        if ($present) { return }
        $updated = if ($current -eq "" -or $current.EndsWith(";")) { "$current$Directory" } else { "$current;$Directory" }
    }
    else {
        if (-not $present) { return }
        $updated = $others -join ";"
    }

    if ($updated -eq "") {
        $key.DeleteValue($ValueName, $false)
    }
    else {
        $key.SetValue($ValueName, $updated, [Microsoft.Win32.RegistryValueKind]::ExpandString)
    }
}
finally {
    $key.Dispose()
}
