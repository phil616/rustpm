# Inspect the built package without installing services or modifying PATH.
param([Parameter(Mandatory)][string]$Msi)

$ErrorActionPreference = "Stop"
$installer = New-Object -ComObject WindowsInstaller.Installer
$database = $installer.OpenDatabase((Resolve-Path $Msi).Path, 0)

function Assert-MsiRow {
    param([string]$Query, [string[]]$Expected)
    $view = $database.OpenView($Query)
    try {
        $view.Execute()
        $row = $view.Fetch()
        if (-not $row) { throw "Missing MSI row: $Query" }
        for ($column = 1; $column -le $Expected.Count; $column++) {
            $actual = $row.StringData($column)
            if ($actual -cne $Expected[$column - 1]) {
                throw "Unexpected MSI value '$actual' in column $column of: $Query"
            }
        }
        if ($view.Fetch()) { throw "Expected exactly one MSI row: $Query" }
    } finally {
        $view.Close()
    }
}

Assert-MsiRow 'SELECT `Name`, `Value`, `Component_` FROM `Environment` WHERE `Environment` = ''RustpmCtlPath''' @('=-*PATH', '[~];[CliDir]', 'cmpRustpmctlPath')
Assert-MsiRow 'SELECT `Directory_`, `KeyPath` FROM `Component` WHERE `Component` = ''cmpRustpmctlPath''' @('CliDir', 'regCliPathInstalled')
Assert-MsiRow 'SELECT `Directory_Parent`, `DefaultDir` FROM `Directory` WHERE `Directory` = ''CliDir''' @('INSTALLFOLDER', 'cli')
Assert-MsiRow 'SELECT `Component_` FROM `File` WHERE `File` = ''CliEntryPointExe''' @('cmpCliEntryPoint')
Assert-MsiRow 'SELECT `Feature_` FROM `FeatureComponents` WHERE `Component_` = ''cmpRustpmctlPath''' @('MainFeature')
Assert-MsiRow 'SELECT `Action` FROM `InstallExecuteSequence` WHERE `Action` = ''WriteEnvironmentStrings''' @('WriteEnvironmentStrings')
Assert-MsiRow 'SELECT `Action` FROM `InstallExecuteSequence` WHERE `Action` = ''RemoveEnvironmentStrings''' @('RemoveEnvironmentStrings')
Write-Host "MSI CLI/PATH table checks passed: $Msi"
